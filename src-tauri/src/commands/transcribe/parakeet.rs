use super::{
    available_cpu_threads_for_transcription, command_output_error, hide_command_window,
    LocalTranscriptionChunkResult,
};
use crate::models::audio::ExportedChunk;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::command;

#[derive(Debug, Clone, PartialEq)]
pub struct ParakeetBackendPaths {
    pub python_exe: PathBuf,
    pub script_path: PathBuf,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParakeetChunkOutput {
    index: usize,
    segments: Vec<crate::models::transcription::TranscriptionSegment>,
}

pub fn parakeet_backend_paths(project_root: &Path) -> ParakeetBackendPaths {
    ParakeetBackendPaths {
        python_exe: project_root
            .join(".venv-parakeet")
            .join("Scripts")
            .join("python.exe"),
        script_path: project_root
            .join("scripts")
            .join("transcribe_parakeet_backend.py"),
    }
}

pub fn resolve_parakeet_backend_from_dir(start_dir: &Path) -> Option<ParakeetBackendPaths> {
    for dir in start_dir.ancestors() {
        let paths = parakeet_backend_paths(dir);
        if paths.python_exe.exists() && paths.script_path.exists() {
            return Some(paths);
        }
    }

    None
}

pub fn normalize_parakeet_model(model: Option<String>) -> String {
    match model
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("parakeet")
        | Some("parakeet-tdt")
        | Some("parakeet-tdt-0.6b")
        | Some("parakeet-tdt-0.6b-v3")
        | Some("nvidia/parakeet-tdt-0.6b-v3")
        | None
        | Some("") => "nvidia/parakeet-tdt-0.6b-v3".to_string(),
        Some(other) => other.to_string(),
    }
}

fn resolve_parakeet_backend() -> Result<ParakeetBackendPaths, String> {
    let current_dir =
        std::env::current_dir().map_err(|e| format!("Failed to resolve current directory: {e}"))?;
    if let Some(paths) = resolve_parakeet_backend_from_dir(&current_dir) {
        return Ok(paths);
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            if let Some(paths) = resolve_parakeet_backend_from_dir(exe_dir) {
                return Ok(paths);
            }
        }
    }

    Err(
        "Backend local Parakeet nao esta instalado. Rode npm run setup:transcribe-parakeet no projeto."
            .to_string(),
    )
}

pub fn parakeet_backend_available() -> bool {
    resolve_parakeet_backend().is_ok()
}

pub async fn transcribe_chunks_with_parakeet(
    audio_chunks: Vec<ExportedChunk>,
    model: Option<String>,
) -> Result<Vec<LocalTranscriptionChunkResult>, String> {
    if audio_chunks.is_empty() {
        return Ok(Vec::new());
    }

    let backend = resolve_parakeet_backend()?;
    let output_dir =
        std::env::temp_dir().join(format!("meeting-minutes-parakeet-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| format!("Failed to create {}: {e}", output_dir.display()))?;
    let chunks_path = output_dir.join("chunks.json");
    let chunks_json = serde_json::to_string(&audio_chunks)
        .map_err(|e| format!("Failed to serialize Parakeet chunks: {e}"))?;
    std::fs::write(&chunks_path, chunks_json)
        .map_err(|e| format!("Failed to write {}: {e}", chunks_path.display()))?;

    let model = normalize_parakeet_model(model);
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
            .arg("--batch-size")
            .arg("4")
            .arg("--window-sec")
            .arg("30")
            .arg("--overlap-sec")
            .arg("1");

        let output = command
            .output()
            .map_err(|e| format!("Failed to run local Parakeet backend: {e}"))?;
        if !output.status.success() {
            return Err(command_output_error(
                "Local Parakeet backend failed",
                &output,
            ));
        }

        let results_path = output_dir.join("chunk-transcription-results.json");
        let raw = std::fs::read_to_string(&results_path)
            .map_err(|e| format!("Failed to read {}: {e}", results_path.display()))?;
        let mut parsed = serde_json::from_str::<Vec<ParakeetChunkOutput>>(&raw)
            .map_err(|e| format!("Failed to parse Parakeet chunk JSON: {e}"))?;
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
    .map_err(|e| format!("Local Parakeet worker failed: {e}"))?
}

#[command]
pub async fn transcribe_chunks_parakeet_local(
    audio_chunks: Vec<ExportedChunk>,
    model: Option<String>,
) -> Result<Vec<LocalTranscriptionChunkResult>, String> {
    transcribe_chunks_with_parakeet(audio_chunks, model).await
}
