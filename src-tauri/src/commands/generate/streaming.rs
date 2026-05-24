use super::gemini::{
    drain_complete_sse_lines, extract_gemini_stream_delta, gemini_stream_url,
    strip_markdown_code_fence,
};
use futures::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
const MINUTES_STREAM_EVENT: &str = "meeting-minutes://minutes-stream";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MinutesStreamPayload {
    meeting_id: String,
    delta: String,
    done: bool,
}

pub(super) fn emit_minutes_stream_delta(
    app: &AppHandle,
    meeting_id: &str,
    delta: String,
    done: bool,
) {
    let _ = app.emit(
        MINUTES_STREAM_EVENT,
        MinutesStreamPayload {
            meeting_id: meeting_id.to_string(),
            delta,
            done,
        },
    );
}

pub(super) async fn stream_gemini_text(
    client: &reqwest::Client,
    gemini_api_key: &str,
    body: &serde_json::Value,
    app: &AppHandle,
    meeting_id: &str,
) -> Result<String, String> {
    let response = client
        .post(gemini_stream_url(gemini_api_key))
        .json(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = response.status();
    if !status.is_success() {
        let err = response.text().await.unwrap_or_default();
        return Err(format!("Gemini API error {}: {}", status, err));
    }

    let mut stream = response.bytes_stream();
    let mut pending = String::new();
    let mut full_text = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        pending.push_str(&String::from_utf8_lossy(&chunk));

        for line in drain_complete_sse_lines(&mut pending) {
            if let Some(delta) = extract_gemini_stream_delta(&line)? {
                full_text.push_str(&delta);
                emit_minutes_stream_delta(app, meeting_id, delta, false);
            }
        }
    }

    if !pending.trim().is_empty() {
        if let Some(delta) = extract_gemini_stream_delta(&pending)? {
            full_text.push_str(&delta);
            emit_minutes_stream_delta(app, meeting_id, delta, false);
        }
    }

    let html = strip_markdown_code_fence(&full_text);
    emit_minutes_stream_delta(app, meeting_id, String::new(), true);
    Ok(html)
}
