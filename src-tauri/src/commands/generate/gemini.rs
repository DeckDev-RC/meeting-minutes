use reqwest::header::CONTENT_TYPE;
use std::time::Duration;

const GEMINI_MODEL: &str = "gemini-2.5-flash";

pub(super) fn gemini_generate_url(gemini_api_key: &str) -> String {
    format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{GEMINI_MODEL}:generateContent?key={}",
        gemini_api_key
    )
}

pub(super) fn gemini_stream_url(gemini_api_key: &str) -> String {
    format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{GEMINI_MODEL}:streamGenerateContent?alt=sse&key={}",
        gemini_api_key
    )
}

pub(super) fn should_retry_gemini_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

pub(super) fn gemini_retry_delay(attempt: u32) -> Duration {
    Duration::from_secs(2u64.saturating_pow(attempt).max(1))
}

pub(super) fn gemini_request_body_bytes(body: &serde_json::Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec(body).map_err(|e| e.to_string())
}

fn configured_gemini_thinking_budget() -> Option<i32> {
    std::env::var("MEETING_MINUTES_GEMINI_THINKING_BUDGET")
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
}

pub(super) fn apply_gemini_thinking_config(mut config: serde_json::Value) -> serde_json::Value {
    if let Some(budget) = configured_gemini_thinking_budget() {
        config["thinkingConfig"] = serde_json::json!({ "thinkingBudget": budget });
    }
    config
}

pub(super) async fn send_gemini_request(
    client: &reqwest::Client,
    gemini_api_key: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let max_attempts = 3u32;
    let body_bytes = gemini_request_body_bytes(body)?;
    let url = gemini_generate_url(gemini_api_key);

    for attempt in 0..max_attempts {
        let response = client
            .post(&url)
            .header(CONTENT_TYPE, "application/json")
            .body(body_bytes.clone())
            .send()
            .await;

        match response {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return resp.json().await.map_err(|e| e.to_string());
                }

                let err = resp.text().await.unwrap_or_default();
                if should_retry_gemini_status(status) && attempt < max_attempts - 1 {
                    tokio::time::sleep(gemini_retry_delay(attempt)).await;
                    continue;
                }
                return Err(format!("Gemini API error {status}: {err}"));
            }
            Err(err) => {
                if attempt < max_attempts - 1 && (err.is_connect() || err.is_timeout()) {
                    tokio::time::sleep(gemini_retry_delay(attempt)).await;
                    continue;
                }
                return Err(err.to_string());
            }
        }
    }

    Err("Gemini API error: max retries exceeded".to_string())
}

pub(super) fn extract_gemini_text(result: serde_json::Value) -> Result<String, String> {
    result["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(strip_markdown_code_fence)
        .ok_or("Empty response from Gemini".to_string())
}

pub(super) fn extract_gemini_stream_delta(line: &str) -> Result<Option<String>, String> {
    let Some(raw) = line.trim().strip_prefix("data:") else {
        return Ok(None);
    };
    let raw = raw.trim();
    if raw.is_empty() || raw == "[DONE]" {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    Ok(value["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(ToString::to_string))
}

pub(super) fn drain_complete_sse_lines(pending: &mut String) -> Vec<String> {
    let mut lines = Vec::new();
    while let Some(line_end) = pending.find('\n') {
        let line = pending[..line_end].trim_end_matches('\r').to_string();
        pending.drain(..line_end + 1);
        lines.push(line);
    }
    lines
}

pub(super) fn strip_markdown_code_fence(text: &str) -> String {
    let trimmed = text.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }

    let Some(first_newline) = trimmed.find('\n') else {
        return trimmed.to_string();
    };
    let body = &trimmed[first_newline + 1..];
    let body = body.strip_suffix("```").unwrap_or(body).trim();

    body.to_string()
}
