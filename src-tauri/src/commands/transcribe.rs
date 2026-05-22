use crate::models::audio::ExportedChunk;
use crate::models::transcription::TranscriptionSegment;
use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use rand::Rng;
use reqwest::multipart;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tauri::{command, State};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub mod parakeet;

pub use parakeet::{
    normalize_parakeet_model, parakeet_backend_paths, resolve_parakeet_backend_from_dir,
    transcribe_chunks_parakeet_local, transcribe_chunks_with_parakeet, ParakeetBackendPaths,
};

const DEFAULT_ASR_KEYTERMS: &[&str] = &[
    "Caio",
    "Manuela",
    "Manu",
    "Renato",
    "Rafaela",
    "Marcos",
    "leitor de documentos",
    "WhatsApp",
    "Drive",
];

fn hide_command_window(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = command;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FasterWhisperBackendPaths {
    pub python_exe: PathBuf,
    pub script_path: PathBuf,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalTranscriptionChunkResult {
    pub index: usize,
    pub segments: Vec<TranscriptionSegment>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FasterWhisperChunkOutput {
    index: usize,
    segments: Vec<TranscriptionSegment>,
}

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

pub fn faster_whisper_backend_paths(project_root: &Path) -> FasterWhisperBackendPaths {
    FasterWhisperBackendPaths {
        python_exe: project_root
            .join(".venv-transcribe")
            .join("Scripts")
            .join("python.exe"),
        script_path: project_root
            .join("scripts")
            .join("transcribe_faster_whisper_backend.py"),
    }
}

pub fn resolve_faster_whisper_backend_from_dir(
    start_dir: &Path,
) -> Option<FasterWhisperBackendPaths> {
    for dir in start_dir.ancestors() {
        let paths = faster_whisper_backend_paths(dir);
        if paths.python_exe.exists() && paths.script_path.exists() {
            return Some(paths);
        }
    }

    None
}

pub fn normalize_faster_whisper_model(model: Option<String>) -> String {
    match model
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("whisper-large-v3-turbo")
        | Some("large-v3-turbo")
        | Some("faster-whisper-large-v3-turbo")
        | None
        | Some("") => "turbo".to_string(),
        Some(other) => other.to_string(),
    }
}

fn command_output_error(context: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let details = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("process exited with status {:?}", output.status.code())
    };
    format!("{context}: {details}")
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

fn clean_transcription_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn segment_from_json(
    id: usize,
    value: &serde_json::Value,
    offset_sec: f64,
    text_key: &str,
) -> Option<TranscriptionSegment> {
    let text = value
        .get(text_key)
        .and_then(|item| item.as_str())
        .map(clean_transcription_text)
        .unwrap_or_default();
    if text.is_empty() {
        return None;
    }
    let start = value
        .get("start")
        .and_then(|item| item.as_f64())
        .unwrap_or(0.0)
        + offset_sec;
    let mut end = value
        .get("end")
        .and_then(|item| item.as_f64())
        .unwrap_or(start - offset_sec)
        + offset_sec;
    if end < start {
        end = start;
    }
    Some(TranscriptionSegment {
        id: id as i32,
        start,
        end,
        text,
    })
}

fn fallback_text_segment(value: &serde_json::Value, offset_sec: f64) -> Vec<TranscriptionSegment> {
    let text = value
        .get("text")
        .and_then(|item| item.as_str())
        .map(clean_transcription_text)
        .unwrap_or_default();
    if text.is_empty() {
        Vec::new()
    } else {
        vec![TranscriptionSegment {
            id: 0,
            start: offset_sec,
            end: offset_sec,
            text,
        }]
    }
}

fn parse_cloudflare_segments(
    value: serde_json::Value,
    offset_sec: f64,
) -> Vec<TranscriptionSegment> {
    let body = value.get("result").unwrap_or(&value);
    let mut segments = body
        .get("segments")
        .and_then(|item| item.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| segment_from_json(0, item, offset_sec, "text"))
                .enumerate()
                .map(|(id, mut segment)| {
                    segment.id = id as i32;
                    segment
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if segments.is_empty() {
        segments = fallback_text_segment(body, offset_sec);
    }
    segments
}

fn parse_deepgram_segments(value: serde_json::Value, offset_sec: f64) -> Vec<TranscriptionSegment> {
    let results = value.get("results").unwrap_or(&value);
    let mut segments = results
        .get("utterances")
        .and_then(|item| item.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| segment_from_json(0, item, offset_sec, "transcript"))
                .enumerate()
                .map(|(id, mut segment)| {
                    segment.id = id as i32;
                    segment
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if !segments.is_empty() {
        return segments;
    }

    let transcript = results
        .get("channels")
        .and_then(|channels| channels.as_array())
        .and_then(|channels| channels.first())
        .and_then(|channel| channel.get("alternatives"))
        .and_then(|alternatives| alternatives.as_array())
        .and_then(|alternatives| alternatives.first())
        .and_then(|alternative| alternative.get("transcript"))
        .and_then(|text| text.as_str())
        .map(clean_transcription_text)
        .unwrap_or_default();

    if !transcript.is_empty() {
        segments.push(TranscriptionSegment {
            id: 0,
            start: offset_sec,
            end: offset_sec,
            text: transcript,
        });
    }
    segments
}

async fn transcription_error_response(
    provider: &str,
    status: reqwest::StatusCode,
    resp: reqwest::Response,
) -> String {
    let body = resp.text().await.unwrap_or_default();
    let detail = clean_transcription_text(&body);
    if detail.is_empty() {
        format!("{provider} API error {status}")
    } else {
        format!("{provider} API error {status}: {detail}")
    }
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
    fn cloudflare_response_parses_wrapped_segments() {
        let value = serde_json::json!({
            "success": true,
            "result": {
                "text": "Ola Caio.",
                "segments": [
                    { "start": 1.0, "end": 2.5, "text": "Ola Caio." }
                ]
            }
        });

        let segments = parse_cloudflare_segments(value, 10.0);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].id, 0);
        assert_eq!(segments[0].start, 11.0);
        assert_eq!(segments[0].end, 12.5);
        assert_eq!(segments[0].text, "Ola Caio.");
    }

    #[test]
    fn deepgram_response_prefers_utterances() {
        let value = serde_json::json!({
            "results": {
                "channels": [
                    { "alternatives": [ { "transcript": "fallback" } ] }
                ],
                "utterances": [
                    { "start": 0.25, "end": 1.5, "transcript": "Boa tarde Caio." },
                    { "start": 1.5, "end": 2.0, "transcript": "" }
                ]
            }
        });

        let segments = parse_deepgram_segments(value, 30.0);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].id, 0);
        assert_eq!(segments[0].start, 30.25);
        assert_eq!(segments[0].end, 31.5);
        assert_eq!(segments[0].text, "Boa tarde Caio.");
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

    #[test]
    fn faster_whisper_backend_paths_point_to_project_tools() {
        let root = std::path::Path::new("C:/project");
        let paths = faster_whisper_backend_paths(root);

        assert!(paths
            .python_exe
            .ends_with(".venv-transcribe/Scripts/python.exe"));
        assert!(paths
            .script_path
            .ends_with("scripts/transcribe_faster_whisper_backend.py"));
    }

    #[test]
    fn faster_whisper_model_aliases_match_backend() {
        assert_eq!(
            normalize_faster_whisper_model(Some("whisper-large-v3-turbo".to_string())),
            "turbo"
        );
        assert_eq!(
            normalize_faster_whisper_model(Some("large-v3".to_string())),
            "large-v3"
        );
        assert_eq!(normalize_faster_whisper_model(None), "turbo");
    }

    #[test]
    fn parakeet_backend_paths_point_to_project_tools() {
        let root = std::path::Path::new("C:/project");
        let paths = parakeet_backend_paths(root);

        assert!(paths
            .python_exe
            .ends_with(".venv-parakeet/Scripts/python.exe"));
        assert!(paths
            .script_path
            .ends_with("scripts/transcribe_parakeet_backend.py"));
    }

    #[test]
    fn parakeet_model_aliases_match_hugging_face_model() {
        assert_eq!(
            normalize_parakeet_model(Some("parakeet".to_string())),
            "nvidia/parakeet-tdt-0.6b-v3"
        );
        assert_eq!(
            normalize_parakeet_model(Some("parakeet-tdt-0.6b-v3".to_string())),
            "nvidia/parakeet-tdt-0.6b-v3"
        );
        assert_eq!(
            normalize_parakeet_model(Some("custom/model".to_string())),
            "custom/model"
        );
        assert_eq!(
            normalize_parakeet_model(None),
            "nvidia/parakeet-tdt-0.6b-v3"
        );
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

#[command]
pub async fn transcribe_chunk_cloudflare(
    http: State<'_, crate::HttpClientState>,
    audio_path: String,
    cloudflare_account_id: String,
    cloudflare_api_token: String,
    offset_sec: f64,
) -> Result<Vec<TranscriptionSegment>, String> {
    transcribe_chunk_cloudflare_with_client(
        &http.0,
        audio_path,
        cloudflare_account_id,
        cloudflare_api_token,
        offset_sec,
    )
    .await
}

pub async fn transcribe_chunk_cloudflare_with_client(
    client: &reqwest::Client,
    audio_path: String,
    cloudflare_account_id: String,
    cloudflare_api_token: String,
    offset_sec: f64,
) -> Result<Vec<TranscriptionSegment>, String> {
    let audio_bytes = tokio::fs::read(&audio_path)
        .await
        .map_err(|e| e.to_string())?;
    let audio_base64 = general_purpose::STANDARD.encode(audio_bytes);
    let prompt = DEFAULT_ASR_KEYTERMS.join(", ");
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/ai/run/@cf/openai/whisper-large-v3-turbo",
        cloudflare_account_id.trim()
    );
    let body = serde_json::json!({
        "audio": audio_base64,
        "language": "pt",
        "task": "transcribe",
        "vad_filter": true,
        "condition_on_previous_text": false,
        "initial_prompt": prompt,
    });
    let max_attempts = 3u32;

    for attempt in 0..max_attempts {
        let resp = client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", cloudflare_api_token.trim()),
            )
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let status = resp.status();
        if status.is_success() {
            let value = resp
                .json::<serde_json::Value>()
                .await
                .map_err(|e| e.to_string())?;
            return Ok(parse_cloudflare_segments(value, offset_sec));
        } else if should_retry_groq_status(status) && attempt < max_attempts - 1 {
            tokio::time::sleep(groq_retry_delay(attempt)).await;
            continue;
        } else {
            return Err(transcription_error_response("Cloudflare", status, resp).await);
        }
    }

    Err("Cloudflare max retries exceeded".to_string())
}

#[command]
pub async fn transcribe_chunk_deepgram(
    http: State<'_, crate::HttpClientState>,
    audio_path: String,
    deepgram_api_key: String,
    offset_sec: f64,
) -> Result<Vec<TranscriptionSegment>, String> {
    transcribe_chunk_deepgram_with_client(&http.0, audio_path, deepgram_api_key, offset_sec).await
}

pub async fn transcribe_chunk_deepgram_with_client(
    client: &reqwest::Client,
    audio_path: String,
    deepgram_api_key: String,
    offset_sec: f64,
) -> Result<Vec<TranscriptionSegment>, String> {
    let file_bytes = Bytes::from(
        tokio::fs::read(&audio_path)
            .await
            .map_err(|e| e.to_string())?,
    );
    let params = {
        let mut params = vec![
            ("model".to_string(), "nova-3".to_string()),
            ("language".to_string(), "pt".to_string()),
            ("smart_format".to_string(), "true".to_string()),
            ("punctuate".to_string(), "true".to_string()),
            ("utterances".to_string(), "true".to_string()),
        ];
        params.extend(
            DEFAULT_ASR_KEYTERMS
                .iter()
                .map(|term| ("keyterm".to_string(), (*term).to_string())),
        );
        params
    };
    let max_attempts = 3u32;

    for attempt in 0..max_attempts {
        let (_, mime_type) = audio_upload_metadata(&audio_path);

        let resp = client
            .post("https://api.deepgram.com/v1/listen")
            .header(
                "Authorization",
                format!("Token {}", deepgram_api_key.trim()),
            )
            .header("Content-Type", mime_type)
            .query(&params)
            .body(file_bytes.clone())
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let status = resp.status();
        if status.is_success() {
            let value = resp
                .json::<serde_json::Value>()
                .await
                .map_err(|e| e.to_string())?;
            return Ok(parse_deepgram_segments(value, offset_sec));
        } else if should_retry_groq_status(status) && attempt < max_attempts - 1 {
            tokio::time::sleep(groq_retry_delay(attempt)).await;
            continue;
        } else {
            return Err(transcription_error_response("Deepgram", status, resp).await);
        }
    }

    Err("Deepgram max retries exceeded".to_string())
}

fn resolve_faster_whisper_backend() -> Result<FasterWhisperBackendPaths, String> {
    let current_dir =
        std::env::current_dir().map_err(|e| format!("Failed to resolve current directory: {e}"))?;
    if let Some(paths) = resolve_faster_whisper_backend_from_dir(&current_dir) {
        return Ok(paths);
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            if let Some(paths) = resolve_faster_whisper_backend_from_dir(exe_dir) {
                return Ok(paths);
            }
        }
    }

    Err(
        "Backend local de transcricao nao esta instalado. Rode npm run setup:transcribe-local no projeto."
            .to_string(),
    )
}

fn available_cpu_threads_for_transcription() -> usize {
    std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4)
        .clamp(2, 8)
}

pub async fn transcribe_chunks_with_faster_whisper(
    audio_chunks: Vec<ExportedChunk>,
    model: Option<String>,
) -> Result<Vec<LocalTranscriptionChunkResult>, String> {
    if audio_chunks.is_empty() {
        return Ok(Vec::new());
    }

    let backend = resolve_faster_whisper_backend()?;
    let output_dir = std::env::temp_dir().join(format!(
        "meeting-minutes-faster-whisper-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| format!("Failed to create {}: {e}", output_dir.display()))?;
    let chunks_path = output_dir.join("chunks.json");
    let chunks_json = serde_json::to_string(&audio_chunks)
        .map_err(|e| format!("Failed to serialize local transcription chunks: {e}"))?;
    std::fs::write(&chunks_path, chunks_json)
        .map_err(|e| format!("Failed to write {}: {e}", chunks_path.display()))?;
    let model = normalize_faster_whisper_model(model);
    let cpu_threads = available_cpu_threads_for_transcription();

    tokio::task::spawn_blocking(move || {
        let mut command = Command::new(&backend.python_exe);
        hide_command_window(&mut command);
        command
            .env("OMP_NUM_THREADS", cpu_threads.to_string())
            .arg(&backend.script_path)
            .arg("--chunks-json")
            .arg(&chunks_path)
            .arg("--out-dir")
            .arg(&output_dir)
            .arg("--model")
            .arg(model)
            .arg("--device")
            .arg("auto")
            .arg("--compute-type")
            .arg("auto")
            .arg("--cpu-threads")
            .arg(cpu_threads.to_string())
            .arg("--batch-size")
            .arg("8")
            .arg("--beam-size")
            .arg("1")
            .arg("--language")
            .arg("pt");

        let output = command
            .output()
            .map_err(|e| format!("Failed to run local faster-whisper backend: {e}"))?;
        if !output.status.success() {
            return Err(command_output_error(
                "Local faster-whisper backend failed",
                &output,
            ));
        }

        let results_path = output_dir.join("chunk-transcription-results.json");
        let raw = std::fs::read_to_string(&results_path)
            .map_err(|e| format!("Failed to read {}: {e}", results_path.display()))?;
        let mut parsed = serde_json::from_str::<Vec<FasterWhisperChunkOutput>>(&raw)
            .map_err(|e| format!("Failed to parse faster-whisper chunk JSON: {e}"))?;
        parsed.sort_by_key(|item| item.index);
        Ok(parsed
            .into_iter()
            .map(|item| LocalTranscriptionChunkResult {
                index: item.index,
                segments: item.segments,
            })
            .collect::<Vec<_>>())
    })
    .await
    .map_err(|e| format!("Local faster-whisper worker failed: {e}"))?
}

#[command]
pub async fn transcribe_chunks_local(
    audio_chunks: Vec<ExportedChunk>,
    model: Option<String>,
) -> Result<Vec<LocalTranscriptionChunkResult>, String> {
    transcribe_chunks_with_faster_whisper(audio_chunks, model).await
}

#[command]
pub async fn transcribe_chunk_local(
    audio_path: String,
    offset_sec: f64,
    model: Option<String>,
) -> Result<Vec<TranscriptionSegment>, String> {
    let chunk = ExportedChunk {
        index: 0,
        audio_path,
        start_sec: offset_sec,
        end_sec: offset_sec,
        offset_sec,
        duration_sec: 0.0,
    };
    let mut results = transcribe_chunks_with_faster_whisper(vec![chunk], model).await?;
    Ok(results
        .pop()
        .map(|result| result.segments)
        .unwrap_or_default())
}
