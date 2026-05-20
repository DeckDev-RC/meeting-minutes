use crate::models::transcription::TranscriptionSegment;
use bytes::Bytes;
use rand::Rng;
use reqwest::multipart;
use std::path::Path;
use std::time::Duration;
use tauri::{command, State};

fn audio_upload_metadata(audio_path: &str) -> (String, String) {
    match Path::new(audio_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("flac") => ("audio.flac".to_string(), "audio/flac".to_string()),
        Some("wav") => ("audio.wav".to_string(), "audio/wav".to_string()),
        _ => ("audio.mp3".to_string(), "audio/mpeg".to_string()),
    }
}

#[derive(Debug, serde::Deserialize)]
struct GroqTranscriptionResponse {
    #[serde(default)]
    segments: Vec<GroqTranscriptionSegment>,
}

#[derive(Debug, serde::Deserialize)]
struct GroqTranscriptionSegment {
    #[serde(default)]
    start: f64,
    #[serde(default)]
    end: f64,
    #[serde(default)]
    text: String,
}

fn groq_retry_delay(attempt: u32) -> Duration {
    let base = Duration::from_secs(2u64.pow(attempt + 1));
    let max_jitter_ms = (base.as_millis() / 2).max(1) as u64;
    let jitter_ms = rand::thread_rng().gen_range(0..=max_jitter_ms);
    base + Duration::from_millis(jitter_ms)
}

fn should_retry_groq_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_metadata_matches_audio_extension() {
        assert_eq!(
            audio_upload_metadata("chunk_001.flac"),
            ("audio.flac".to_string(), "audio/flac".to_string())
        );
        assert_eq!(
            audio_upload_metadata("chunk_001.wav"),
            ("audio.wav".to_string(), "audio/wav".to_string())
        );
        assert_eq!(
            audio_upload_metadata("chunk_001.mp3"),
            ("audio.mp3".to_string(), "audio/mpeg".to_string())
        );
    }

    #[test]
    fn groq_response_deserializes_segments_without_generic_value() {
        let body = r#"{"segments":[{"start":1.5,"end":3.0,"text":"  ola  "}]}"#;
        let parsed: GroqTranscriptionResponse = serde_json::from_str(body).unwrap();

        assert_eq!(parsed.segments.len(), 1);
        assert_eq!(parsed.segments[0].start, 1.5);
        assert_eq!(parsed.segments[0].text, "  ola  ");
    }

    #[test]
    fn groq_retry_policy_retries_rate_limits_and_server_errors_only() {
        assert!(should_retry_groq_status(
            reqwest::StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(should_retry_groq_status(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(should_retry_groq_status(reqwest::StatusCode::BAD_GATEWAY));
        assert!(should_retry_groq_status(
            reqwest::StatusCode::SERVICE_UNAVAILABLE
        ));

        assert!(!should_retry_groq_status(reqwest::StatusCode::BAD_REQUEST));
        assert!(!should_retry_groq_status(reqwest::StatusCode::UNAUTHORIZED));
    }
}

#[command]
pub async fn transcribe_chunk(
    http: State<'_, crate::HttpClientState>,
    audio_path: String,
    groq_api_key: String,
    offset_sec: f64,
) -> Result<Vec<TranscriptionSegment>, String> {
    transcribe_chunk_with_client(&http.0, audio_path, groq_api_key, offset_sec).await
}

pub async fn transcribe_chunk_with_client(
    client: &reqwest::Client,
    audio_path: String,
    groq_api_key: String,
    offset_sec: f64,
) -> Result<Vec<TranscriptionSegment>, String> {
    let file_bytes = Bytes::from(
        tokio::fs::read(&audio_path)
            .await
            .map_err(|e| e.to_string())?,
    );
    let max_attempts = 3u32;

    for attempt in 0..max_attempts {
        let (file_name, mime_type) = audio_upload_metadata(&audio_path);
        let file_part = multipart::Part::stream_with_length(
            reqwest::Body::from(file_bytes.clone()),
            file_bytes.len() as u64,
        )
        .file_name(file_name)
        .mime_str(&mime_type)
        .map_err(|e| e.to_string())?;

        let form = multipart::Form::new()
            .part("file", file_part)
            .text("model", "whisper-large-v3-turbo")
            .text("language", "pt")
            .text("response_format", "verbose_json")
            .text("timestamp_granularities[]", "segment");

        let resp = client
            .post("https://api.groq.com/openai/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {}", groq_api_key))
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let status = resp.status();
        if status.is_success() {
            let body: GroqTranscriptionResponse = resp.json().await.map_err(|e| e.to_string())?;
            let segments = body
                .segments
                .into_iter()
                .enumerate()
                .map(|(i, s)| TranscriptionSegment {
                    id: i as i32,
                    start: s.start + offset_sec,
                    end: s.end + offset_sec,
                    text: s.text.trim().to_string(),
                })
                .collect();
            return Ok(segments);
        } else if should_retry_groq_status(status) && attempt < max_attempts - 1 {
            tokio::time::sleep(groq_retry_delay(attempt)).await;
            continue;
        } else {
            let err_body = resp.text().await.unwrap_or_default();
            return Err(format!("Groq API error {}: {}", status, err_body));
        }
    }

    Err("Max retries exceeded".to_string())
}
