use crate::models::audio::ExportedChunk;
use crate::models::transcription::{DiarizedResult, DiarizedSegment, TranscriptionSegment};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use tauri::{command, Manager};
use tauri_plugin_shell::ShellExt;
use tokio::io::AsyncWriteExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

mod refinement;
pub(crate) mod sherpa;
mod stitching;

use refinement::chunks_can_use_sherpa;
pub use refinement::{
    merge_selective_refinement, select_suspicious_chunks_for_refinement,
    select_suspicious_refinement_windows, select_suspicious_refinement_windows_with_context,
};
pub use sherpa::{
    diarize_audio_by_chunks_with_sherpa, diarize_audio_turns_sherpa_chunked,
    diarize_audio_with_sherpa,
};
#[cfg(test)]
use stitching::global_centroid_speaker_map;
use stitching::stitch_diarized_chunk_results_with_centroids;
pub use stitching::{
    segments_for_chunk, shift_diarized_result, stitch_diarized_chunk_results,
    stitch_diarized_chunk_results_with_expected_speakers,
};

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

const QUICK_MERGE_GAP_SEC: f64 = 1.25;
const QUICK_ANSWER_WINDOW_SEC: f64 = 4.0;
const DEFAULT_CHUNKED_MAX_SPEAKERS: i32 = 8;
const MAX_CONFIGURED_CHUNKED_SPEAKERS: i32 = 12;
const SEGMENTATION_MODEL_URL: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.int8.onnx";
const EMBEDDING_MODEL_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerTurn {
    pub start: f64,
    pub end: f64,
    pub speaker_index: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiarizationAssetPaths {
    pub model_dir: PathBuf,
    pub segmentation_model: PathBuf,
    pub embedding_model: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModernCpuBackendPaths {
    pub python_exe: PathBuf,
    pub script_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PyannoteBackendPaths {
    pub python_exe: PathBuf,
    pub script_path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModernCpuBatchChunkOutput {
    index: usize,
    offset_sec: f64,
    diarized: DiarizedResult,
    #[serde(default)]
    speaker_centroids: Vec<SpeakerCentroid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerCentroid {
    pub speaker: String,
    pub embedding: Vec<f64>,
}

#[derive(Debug, Clone)]
struct DiarizedChunkResult {
    diarized: DiarizedResult,
    speaker_centroids: Vec<SpeakerCentroid>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RefinementWindow {
    pub chunk: ExportedChunk,
    pub start_sec: f64,
    pub end_sec: f64,
}

impl RefinementWindow {
    pub fn duration_sec(&self) -> f64 {
        (self.end_sec - self.start_sec).max(0.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiarizationMode {
    Auto,
    Fast,
    Hybrid,
    ModernCpu,
    ModernCpuChunked,
    Precise,
    Pyannote,
}

impl DiarizationMode {
    pub fn from_option(value: Option<String>) -> Self {
        match value
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("fast") => Self::Fast,
            Some("hybrid") => Self::Hybrid,
            Some("modern-cpu") | Some("diarize-cpu") | Some("cpu") => Self::ModernCpu,
            Some("modern-cpu-chunked") | Some("chunked-cpu") | Some("cpu-chunked") => {
                Self::ModernCpuChunked
            }
            Some("pyannote") | Some("community-1") | Some("pyannote-community") => Self::Pyannote,
            Some("precise") => Self::Precise,
            Some("auto") => Self::Auto,
            _ => Self::Auto,
        }
    }
}

pub fn diarization_mode_label(mode: DiarizationMode) -> &'static str {
    match mode {
        DiarizationMode::Auto => "auto",
        DiarizationMode::Fast => "fast",
        DiarizationMode::Hybrid => "hybrid",
        DiarizationMode::ModernCpu => "modern-cpu",
        DiarizationMode::ModernCpuChunked => "modern-cpu-chunked",
        DiarizationMode::Precise => "precise",
        DiarizationMode::Pyannote => "pyannote",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiarizationBackend {
    FastLocal,
    ModernCpu,
    ModernCpuChunked,
    Pyannote,
    SherpaHybrid,
    SherpaPrecise,
}

pub fn diarization_backend_label(backend: DiarizationBackend) -> &'static str {
    match backend {
        DiarizationBackend::FastLocal => "fast-local",
        DiarizationBackend::ModernCpu => "modern-cpu",
        DiarizationBackend::ModernCpuChunked => "modern-cpu-chunked",
        DiarizationBackend::Pyannote => "pyannote",
        DiarizationBackend::SherpaHybrid => "sherpa-hybrid",
        DiarizationBackend::SherpaPrecise => "sherpa-precise",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiarizationTelemetry {
    pub requested_mode: String,
    pub backend_used: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub wall_clock_sec: f64,
    pub speaker_count: usize,
    pub segment_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiarizationRun {
    pub result: DiarizedResult,
    pub telemetry: DiarizationTelemetry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiarizedResultWithTelemetry {
    pub speakers: Vec<String>,
    pub segments: Vec<DiarizedSegment>,
    pub telemetry: DiarizationTelemetry,
}

impl From<DiarizationRun> for DiarizedResultWithTelemetry {
    fn from(run: DiarizationRun) -> Self {
        Self {
            speakers: run.result.speakers,
            segments: run.result.segments,
            telemetry: run.telemetry,
        }
    }
}

pub fn mode_allows_local_fallback(mode: DiarizationMode) -> bool {
    mode == DiarizationMode::Auto
}

pub fn build_diarization_telemetry(
    requested_mode: DiarizationMode,
    backend_used: DiarizationBackend,
    fallback_reason: Option<String>,
    wall_clock_sec: f64,
    result: &DiarizedResult,
) -> DiarizationTelemetry {
    DiarizationTelemetry {
        requested_mode: diarization_mode_label(requested_mode).to_string(),
        backend_used: diarization_backend_label(backend_used).to_string(),
        fallback_reason: fallback_reason.filter(|value| !value.trim().is_empty()),
        wall_clock_sec,
        speaker_count: result.speakers.len(),
        segment_count: result.segments.len(),
    }
}

fn build_diarization_run(
    requested_mode: DiarizationMode,
    backend_used: DiarizationBackend,
    fallback_reason: Option<String>,
    started: Instant,
    result: DiarizedResult,
) -> DiarizationRun {
    let telemetry = build_diarization_telemetry(
        requested_mode,
        backend_used,
        fallback_reason,
        started.elapsed().as_secs_f64(),
        &result,
    );
    DiarizationRun { result, telemetry }
}

fn push_fallback_reason(reasons: &mut Vec<String>, backend: DiarizationBackend, error: String) {
    let trimmed = error.trim();
    if trimmed.is_empty() {
        return;
    }
    reasons.push(format!(
        "{} failed: {}",
        diarization_backend_label(backend),
        trimmed
    ));
}

fn join_fallback_reasons(reasons: &[String]) -> Option<String> {
    if reasons.is_empty() {
        None
    } else {
        Some(reasons.join(" | "))
    }
}

pub fn normalize_diarization_threads(requested: Option<i32>, available_threads: usize) -> i32 {
    let available = available_threads.max(2).min(16) as i32;
    match requested {
        Some(value) if value > 0 => value.clamp(2, 16),
        Some(_) => 2,
        None => available.min(8),
    }
}

fn chunked_max_speakers(expected_speakers: Option<i32>) -> i32 {
    expected_speakers
        .filter(|value| *value > 0)
        .map(|value| value.clamp(1, MAX_CONFIGURED_CHUNKED_SPEAKERS))
        .unwrap_or(DEFAULT_CHUNKED_MAX_SPEAKERS)
}

fn normalize_sherpa_provider(value: Option<&str>) -> Option<String> {
    let normalized = value?.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "cpu" | "cuda" | "coreml" => Some(normalized),
        "auto" | "" => None,
        _ => None,
    }
}

fn requested_sherpa_provider(provider: Option<String>) -> Option<String> {
    normalize_sherpa_provider(provider.as_deref()).or_else(|| {
        normalize_sherpa_provider(
            std::env::var("MEETING_MINUTES_SHERPA_PROVIDER")
                .ok()
                .as_deref(),
        )
    })
}

fn sherpa_provider_candidates(provider: Option<String>) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(provider) = provider {
        candidates.push(provider);
    }
    if !candidates.iter().any(|candidate| candidate == "cpu") {
        candidates.push("cpu".to_string());
    }
    candidates
}

fn available_parallelism_count() -> usize {
    std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(2)
}

pub fn diarization_asset_paths(app_data_dir: &Path) -> DiarizationAssetPaths {
    let model_dir = app_data_dir.join("models").join("diarization");
    DiarizationAssetPaths {
        segmentation_model: model_dir.join("sherpa-onnx-pyannote-segmentation-3-0-model.int8.onnx"),
        embedding_model: model_dir
            .join("3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx"),
        model_dir,
    }
}

pub fn modern_cpu_backend_paths(project_root: &Path) -> ModernCpuBackendPaths {
    ModernCpuBackendPaths {
        python_exe: project_root
            .join(".venv-diarize")
            .join("Scripts")
            .join("python.exe"),
        script_path: project_root.join("scripts").join("diarize_cpu_backend.py"),
    }
}

pub fn pyannote_backend_paths(project_root: &Path) -> PyannoteBackendPaths {
    PyannoteBackendPaths {
        python_exe: project_root
            .join(".venv-pyannote")
            .join("Scripts")
            .join("python.exe"),
        script_path: project_root
            .join("scripts")
            .join("pyannote_community_backend.py"),
    }
}

pub fn resolve_modern_cpu_backend_from_dir(start_dir: &Path) -> Option<ModernCpuBackendPaths> {
    for dir in start_dir.ancestors() {
        let paths = modern_cpu_backend_paths(dir);
        if paths.python_exe.exists() && paths.script_path.exists() {
            return Some(paths);
        }
    }

    None
}

pub fn resolve_pyannote_backend_from_dir(start_dir: &Path) -> Option<PyannoteBackendPaths> {
    for dir in start_dir.ancestors() {
        let paths = pyannote_backend_paths(dir);
        if paths.python_exe.exists() && paths.script_path.exists() {
            return Some(paths);
        }
    }

    None
}

async fn download_file_if_missing(url: &str, path: &Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create model directory: {e}"))?;
    }

    let response = reqwest::get(url)
        .await
        .map_err(|e| format!("Failed to download model from {url}: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Model download failed from {url}: {e}"))?;
    let tmp_path = path.with_extension("download.tmp");
    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("Failed to create {}: {e}", tmp_path.display()))?;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Failed to read model download from {url}: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Failed to write {}: {e}", tmp_path.display()))?;
    }
    file.flush()
        .await
        .map_err(|e| format!("Failed to flush {}: {e}", tmp_path.display()))?;

    tokio::fs::rename(&tmp_path, path)
        .await
        .map_err(|e| format!("Failed to save model at {}: {e}", path.display()))
}

pub async fn ensure_diarization_assets_in_dir(
    app_data_dir: &Path,
) -> Result<DiarizationAssetPaths, String> {
    let paths = diarization_asset_paths(app_data_dir);

    tokio::try_join!(
        download_file_if_missing(SEGMENTATION_MODEL_URL, &paths.segmentation_model),
        download_file_if_missing(EMBEDDING_MODEL_URL, &paths.embedding_model)
    )?;

    Ok(paths)
}

fn is_title_like_name(candidate: &str) -> bool {
    let words: Vec<&str> = candidate.split_whitespace().collect();
    if words.is_empty() || words.len() > 3 {
        return false;
    }

    words.iter().all(|word| {
        let cleaned = word.trim_matches(|ch: char| !ch.is_alphabetic());
        !cleaned.is_empty()
            && cleaned
                .chars()
                .next()
                .map(|ch| ch.is_uppercase())
                .unwrap_or(false)
    })
}

fn split_inline_speaker_label(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    let separator_index = trimmed
        .find(':')
        .or_else(|| trimmed.find(" - "))
        .or_else(|| trimmed.find(" – "));
    let separator_index = separator_index?;

    if separator_index > 32 {
        return None;
    }

    let speaker = trimmed[..separator_index].trim();
    if !is_title_like_name(speaker) {
        return None;
    }

    let separator_len = if trimmed[separator_index..].starts_with(':') {
        1
    } else {
        3
    };
    let content = trimmed[separator_index + separator_len..].trim();
    if content.is_empty() {
        return None;
    }

    Some((speaker.to_string(), content.to_string()))
}

fn is_question(text: &str) -> bool {
    text.trim_end().ends_with('?')
}

fn starts_like_answer(text: &str) -> bool {
    let lower = text.trim_start().to_lowercase();
    [
        "sim",
        "nao",
        "não",
        "ok",
        "claro",
        "combinado",
        "perfeito",
        "beleza",
        "exato",
        "isso",
        "posso",
        "vou",
        "eu ",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix))
}

fn next_generic_speaker(current: &str) -> String {
    if current == "Falante 1" {
        "Falante 2".to_string()
    } else {
        "Falante 1".to_string()
    }
}

fn push_speaker_once(speakers: &mut Vec<String>, speaker: &str) {
    if !speakers.iter().any(|item| item == speaker) {
        speakers.push(speaker.to_string());
    }
}

fn push_or_merge_segment(
    diarized_segments: &mut Vec<DiarizedSegment>,
    speaker: &str,
    start: f64,
    end: f64,
    text: &str,
    should_merge: bool,
) {
    if let Some(last) = diarized_segments.last_mut() {
        if should_merge && last.speaker == speaker {
            last.end = end;
            if !last.text.is_empty() {
                last.text.push(' ');
            }
            last.text.push_str(text);
            return;
        }
    }

    diarized_segments.push(DiarizedSegment {
        speaker: speaker.to_string(),
        start,
        end,
        text: text.to_string(),
    });
}

fn overlap_seconds(segment_start: f64, segment_end: f64, turn_start: f64, turn_end: f64) -> f64 {
    let start = segment_start.max(turn_start);
    let end = segment_end.min(turn_end);
    (end - start).max(0.0)
}

fn best_speaker_for_segment(segment: &TranscriptionSegment, turns: &[SpeakerTurn]) -> i32 {
    let mut best_speaker = None;
    let mut best_overlap = 0.0;
    let mut nearest_speaker = None;
    let mut nearest_distance = f64::INFINITY;
    for turn in turns.iter().filter(|turn| turn.end > turn.start) {
        let overlap = overlap_seconds(segment.start, segment.end, turn.start, turn.end);
        if overlap > best_overlap {
            best_overlap = overlap;
            best_speaker = Some(turn.speaker_index);
        }
        let distance = turn_distance_to_segment(segment, turn);
        if distance < nearest_distance {
            nearest_distance = distance;
            nearest_speaker = Some(turn.speaker_index);
        }
    }
    best_speaker.or(nearest_speaker).unwrap_or(0)
}

fn best_speakers_for_segments_sweep_iter<'a, I>(segments: I, turns: &[SpeakerTurn]) -> Vec<i32>
where
    I: Iterator<Item = &'a TranscriptionSegment> + Clone,
{
    let mut last_start = None;
    let sorted_segments = segments.clone().all(|segment| {
        let is_sorted = last_start
            .map(|previous| previous <= segment.start)
            .unwrap_or(true);
        last_start = Some(segment.start);
        is_sorted
    });
    let sorted_turns = turns.windows(2).all(|pair| pair[0].start <= pair[1].start);
    if !sorted_segments || !sorted_turns {
        return segments
            .map(|segment| best_speaker_for_segment(segment, turns))
            .collect();
    }

    let valid_turns = turns
        .iter()
        .filter(|turn| turn.end > turn.start)
        .collect::<Vec<_>>();
    let mut cursor = 0usize;

    segments
        .map(|segment| {
            while cursor < valid_turns.len() && valid_turns[cursor].end <= segment.start {
                cursor += 1;
            }

            let mut best_speaker = None;
            let mut best_overlap = 0.0;
            let mut index = cursor;
            while index < valid_turns.len() {
                let turn = valid_turns[index];
                if turn.start >= segment.end {
                    break;
                }

                let overlap = overlap_seconds(segment.start, segment.end, turn.start, turn.end);
                if overlap > best_overlap {
                    best_overlap = overlap;
                    best_speaker = Some(turn.speaker_index);
                }
                index += 1;
            }

            best_speaker
                .unwrap_or_else(|| nearest_speaker_for_segment_sorted(segment, &valid_turns))
        })
        .collect()
}

fn best_speakers_for_segments_sweep(
    segments: &[TranscriptionSegment],
    turns: &[SpeakerTurn],
) -> Vec<i32> {
    best_speakers_for_segments_sweep_iter(segments.iter(), turns)
}

fn best_speakers_for_segment_refs_sweep(
    segments: &[&TranscriptionSegment],
    turns: &[SpeakerTurn],
) -> Vec<i32> {
    best_speakers_for_segments_sweep_iter(segments.iter().copied(), turns)
}

fn turn_distance_to_segment(segment: &TranscriptionSegment, turn: &SpeakerTurn) -> f64 {
    if turn.end <= segment.start {
        segment.start - turn.end
    } else if turn.start >= segment.end {
        turn.start - segment.end
    } else {
        0.0
    }
}

fn nearest_speaker_for_segment_sorted(
    segment: &TranscriptionSegment,
    sorted_turns: &[&SpeakerTurn],
) -> i32 {
    if sorted_turns.is_empty() {
        return 0;
    }

    let before_count = sorted_turns.partition_point(|turn| turn.end <= segment.start);
    let after_index = sorted_turns.partition_point(|turn| turn.start < segment.end);
    let mut best: Option<(&SpeakerTurn, f64)> = None;

    for candidate in [
        before_count
            .checked_sub(1)
            .and_then(|index| sorted_turns.get(index)),
        sorted_turns.get(after_index),
    ]
    .into_iter()
    .flatten()
    {
        let distance = turn_distance_to_segment(segment, candidate);
        if best
            .as_ref()
            .map(|(_, best_distance)| distance < *best_distance)
            .unwrap_or(true)
        {
            best = Some((candidate, distance));
        }
    }

    best.map(|(turn, _)| turn.speaker_index)
        .unwrap_or_else(|| sorted_turns[0].speaker_index)
}

fn speaker_index_from_name(name: &str) -> i32 {
    let trimmed = name.trim();
    let number = trimmed
        .strip_prefix("Falante ")
        .or_else(|| trimmed.strip_prefix("Speaker "))
        .or_else(|| trimmed.strip_prefix("SPEAKER_"))
        .and_then(|value| value.trim().parse::<i32>().ok())
        .unwrap_or(1);

    (number - 1).max(0)
}

pub fn speaker_turns_from_diarized_segments(segments: &[DiarizedSegment]) -> Vec<SpeakerTurn> {
    segments
        .iter()
        .filter(|segment| segment.end > segment.start)
        .map(|segment| SpeakerTurn {
            start: segment.start,
            end: segment.end,
            speaker_index: speaker_index_from_name(&segment.speaker),
        })
        .collect()
}

pub fn align_modern_cpu_diarization_to_transcript(
    modern_result: DiarizedResult,
    segments: &[TranscriptionSegment],
) -> DiarizedResult {
    let turns = speaker_turns_from_diarized_segments(&modern_result.segments);
    if turns.is_empty() {
        return diarize_transcription_locally(segments);
    }

    diarize_segments_with_speaker_turns(segments, &turns)
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

fn shell_output_error(context: &str, output: &tauri_plugin_shell::process::Output) -> String {
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

async fn export_refinement_windows(
    app: &tauri::AppHandle,
    windows: &[RefinementWindow],
) -> Result<Vec<ExportedChunk>, String> {
    if windows.is_empty() {
        return Ok(Vec::new());
    }

    let output_dir = std::env::temp_dir().join(format!(
        "meeting-minutes-selective-refine-{}",
        uuid::Uuid::new_v4()
    ));
    tokio::fs::create_dir_all(&output_dir)
        .await
        .map_err(|e| format!("Failed to create {}: {e}", output_dir.display()))?;

    let output_dir = Arc::new(output_dir);
    let export_results = stream::iter(windows.iter().cloned().enumerate())
        .map(|(index, window)| {
            let app = app.clone();
            let output_dir = output_dir.clone();
            async move {
                let audio_path = output_dir.join(format!("refine_{index:03}.wav"));
                let local_start = (window.start_sec - window.chunk.offset_sec).max(0.0);
                let duration = window.duration_sec();
                if duration <= 0.0 {
                    return Ok(None);
                }

                let args = vec![
                    "-ss".to_string(),
                    format!("{local_start:.3}"),
                    "-i".to_string(),
                    window.chunk.audio_path.clone(),
                    "-t".to_string(),
                    format!("{duration:.3}"),
                    "-ar".to_string(),
                    "16000".to_string(),
                    "-ac".to_string(),
                    "1".to_string(),
                    "-c:a".to_string(),
                    "pcm_s16le".to_string(),
                    "-y".to_string(),
                    audio_path.to_string_lossy().to_string(),
                ];

                let output = app
                    .shell()
                    .sidecar("ffmpeg")
                    .map_err(|e| e.to_string())?
                    .args(args)
                    .output()
                    .await
                    .map_err(|e| e.to_string())?;

                if output.status.code() != Some(0) {
                    return Err(shell_output_error(
                        "Failed to export selective refinement window",
                        &output,
                    ));
                }

                Ok(Some(ExportedChunk {
                    index,
                    audio_path: audio_path.to_string_lossy().to_string(),
                    start_sec: window.start_sec,
                    end_sec: window.end_sec,
                    offset_sec: window.start_sec,
                    duration_sec: duration,
                }))
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<Result<Option<ExportedChunk>, String>>>()
        .await;

    let mut exported = export_results
        .into_iter()
        .collect::<Result<Vec<_>, String>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    exported.sort_by_key(|chunk| chunk.index);

    Ok(exported)
}

async fn cleanup_exported_chunk_parent_dirs(chunks: &[ExportedChunk]) {
    let dirs = chunks
        .iter()
        .filter_map(|chunk| Path::new(&chunk.audio_path).parent().map(Path::to_path_buf))
        .collect::<BTreeSet<_>>();

    for dir in dirs {
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
}

async fn run_modern_cpu_backend(
    audio_path: String,
    expected_speakers: Option<i32>,
) -> Result<DiarizedResult, String> {
    let current_dir =
        std::env::current_dir().map_err(|e| format!("Failed to resolve current directory: {e}"))?;
    let backend = resolve_modern_cpu_backend_from_dir(&current_dir)
        .ok_or_else(|| "Modern CPU diarization backend is not installed".to_string())?;
    let output_dir = std::env::temp_dir().join(format!(
        "meeting-minutes-diarize-cpu-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| format!("Failed to create {}: {e}", output_dir.display()))?;

    tokio::task::spawn_blocking(move || {
        let mut command = Command::new(&backend.python_exe);
        hide_command_window(&mut command);
        command
            .arg(&backend.script_path)
            .arg("--audio")
            .arg(&audio_path)
            .arg("--out-dir")
            .arg(&output_dir);

        if let Some(expected_speakers) = expected_speakers.filter(|value| *value > 0) {
            command
                .arg("--num-speakers")
                .arg(expected_speakers.to_string());
        }

        let output = command
            .output()
            .map_err(|e| format!("Failed to run modern CPU diarization backend: {e}"))?;
        if !output.status.success() {
            return Err(command_output_error(
                "Modern CPU diarization backend failed",
                &output,
            ));
        }

        let diarized_path = output_dir.join("diarized-transcription.json");
        let raw = std::fs::read_to_string(&diarized_path)
            .map_err(|e| format!("Failed to read {}: {e}", diarized_path.display()))?;
        serde_json::from_str::<DiarizedResult>(&raw)
            .map_err(|e| format!("Failed to parse modern CPU diarization JSON: {e}"))
    })
    .await
    .map_err(|e| format!("Modern CPU diarization worker failed: {e}"))?
}

fn stitch_modern_cpu_batch_outputs(
    raw: &str,
    expected_speakers: Option<i32>,
) -> Result<DiarizedResult, String> {
    let mut outputs = serde_json::from_str::<Vec<ModernCpuBatchChunkOutput>>(raw)
        .map_err(|e| format!("Failed to parse modern CPU batch diarization JSON: {e}"))?;
    outputs.sort_by_key(|output| output.index);
    let shifted_chunks = outputs
        .into_iter()
        .map(|output| DiarizedChunkResult {
            diarized: shift_diarized_result(output.diarized, output.offset_sec),
            speaker_centroids: output.speaker_centroids,
        })
        .collect::<Vec<_>>();
    if shifted_chunks
        .iter()
        .any(|chunk| !chunk.speaker_centroids.is_empty())
    {
        return Ok(stitch_diarized_chunk_results_with_centroids(
            shifted_chunks,
            3.0,
            expected_speakers,
        ));
    }

    let shifted = shifted_chunks
        .into_iter()
        .map(|chunk| chunk.diarized)
        .collect::<Vec<_>>();
    Ok(stitch_diarized_chunk_results_with_expected_speakers(
        shifted,
        3.0,
        expected_speakers,
    ))
}

async fn run_modern_cpu_backend_batch(
    audio_chunks: Vec<ExportedChunk>,
    expected_speakers: Option<i32>,
    max_parallel_chunks: usize,
) -> Result<DiarizedResult, String> {
    let current_dir =
        std::env::current_dir().map_err(|e| format!("Failed to resolve current directory: {e}"))?;
    let backend = resolve_modern_cpu_backend_from_dir(&current_dir)
        .ok_or_else(|| "Modern CPU diarization backend is not installed".to_string())?;
    let output_dir = std::env::temp_dir().join(format!(
        "meeting-minutes-diarize-cpu-batch-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| format!("Failed to create {}: {e}", output_dir.display()))?;
    let chunks_path = output_dir.join("chunks.json");
    let chunks_json = serde_json::to_string(&audio_chunks)
        .map_err(|e| format!("Failed to serialize modern CPU chunks: {e}"))?;
    std::fs::write(&chunks_path, chunks_json)
        .map_err(|e| format!("Failed to write {}: {e}", chunks_path.display()))?;

    tokio::task::spawn_blocking(move || {
        let mut command = Command::new(&backend.python_exe);
        hide_command_window(&mut command);
        command
            .arg(&backend.script_path)
            .arg("--chunks-json")
            .arg(&chunks_path)
            .arg("--out-dir")
            .arg(&output_dir)
            .arg("--max-workers")
            .arg(max_parallel_chunks.max(1).to_string());

        command
            .arg("--max-speakers")
            .arg(chunked_max_speakers(expected_speakers).to_string());

        let output = command
            .output()
            .map_err(|e| format!("Failed to run modern CPU batch diarization backend: {e}"))?;
        if !output.status.success() {
            return Err(command_output_error(
                "Modern CPU batch diarization backend failed",
                &output,
            ));
        }

        let results_path = output_dir.join("chunk-diarized-results.json");
        let raw = std::fs::read_to_string(&results_path)
            .map_err(|e| format!("Failed to read {}: {e}", results_path.display()))?;
        stitch_modern_cpu_batch_outputs(&raw, expected_speakers)
    })
    .await
    .map_err(|e| format!("Modern CPU batch diarization worker failed: {e}"))?
}

fn resolve_hf_token() -> Option<String> {
    if let Ok(token) = std::env::var("HF_TOKEN") {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("powershell");
        hide_command_window(&mut command);
        let output = command
            .arg("-NoProfile")
            .arg("-Command")
            .arg("[Environment]::GetEnvironmentVariable('HF_TOKEN','User')")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !token.is_empty() {
            return Some(token);
        }
    }

    None
}

async fn run_pyannote_backend(
    audio_path: String,
    expected_speakers: Option<i32>,
) -> Result<DiarizedResult, String> {
    let current_dir =
        std::env::current_dir().map_err(|e| format!("Failed to resolve current directory: {e}"))?;
    let backend = resolve_pyannote_backend_from_dir(&current_dir)
        .ok_or_else(|| "Pyannote Community-1 backend is not installed".to_string())?;
    let hf_token = resolve_hf_token()
        .ok_or_else(|| "HF_TOKEN is not available for pyannote Community-1".to_string())?;
    let output_dir =
        std::env::temp_dir().join(format!("meeting-minutes-pyannote-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| format!("Failed to create {}: {e}", output_dir.display()))?;

    tokio::task::spawn_blocking(move || {
        let mut command = Command::new(&backend.python_exe);
        hide_command_window(&mut command);
        command
            .env("HF_TOKEN", hf_token)
            .arg(&backend.script_path)
            .arg("--audio")
            .arg(&audio_path)
            .arg("--out-dir")
            .arg(&output_dir);

        if let Some(expected_speakers) = expected_speakers.filter(|value| *value > 0) {
            command
                .arg("--num-speakers")
                .arg(expected_speakers.to_string());
        }

        let output = command
            .output()
            .map_err(|e| format!("Failed to run pyannote Community-1 backend: {e}"))?;
        if !output.status.success() {
            return Err(command_output_error(
                "Pyannote Community-1 backend failed",
                &output,
            ));
        }

        let diarized_path = output_dir.join("diarized-transcription.json");
        let raw = std::fs::read_to_string(&diarized_path)
            .map_err(|e| format!("Failed to read {}: {e}", diarized_path.display()))?;
        serde_json::from_str::<DiarizedResult>(&raw)
            .map_err(|e| format!("Failed to parse pyannote diarization JSON: {e}"))
    })
    .await
    .map_err(|e| format!("Pyannote Community-1 worker failed: {e}"))?
}

pub async fn diarize_audio_with_modern_cpu(
    audio_path: String,
    segments: &[TranscriptionSegment],
    expected_speakers: Option<i32>,
) -> Result<DiarizedResult, String> {
    let modern_result = run_modern_cpu_backend(audio_path, expected_speakers).await?;
    Ok(align_modern_cpu_diarization_to_transcript(
        modern_result,
        segments,
    ))
}

#[command]
pub async fn diarize_audio_turns_modern_cpu(
    audio_path: String,
    expected_speakers: Option<i32>,
) -> Result<Vec<SpeakerTurn>, String> {
    let modern_result = run_modern_cpu_backend(audio_path, expected_speakers).await?;
    let turns = speaker_turns_from_diarized_segments(&modern_result.segments);
    if turns.is_empty() {
        return Err("Modern CPU diarization returned no speaker turns".to_string());
    }

    Ok(turns)
}

#[command]
pub async fn diarize_audio_turns_modern_cpu_chunked(
    audio_chunks: Vec<ExportedChunk>,
    expected_speakers: Option<i32>,
    num_threads: Option<i32>,
) -> Result<Vec<SpeakerTurn>, String> {
    let threads = normalize_diarization_threads(num_threads, available_parallelism_count());
    let modern_result = diarize_audio_chunks_with_modern_cpu(
        audio_chunks,
        Vec::new(),
        expected_speakers,
        threads as usize,
    )
    .await?;
    let turns = speaker_turns_from_diarized_segments(&modern_result.segments);
    if turns.is_empty() {
        return Err("Modern CPU chunked diarization returned no speaker turns".to_string());
    }

    Ok(turns)
}

#[command]
pub async fn diarize_audio_turns_pyannote(
    audio_path: String,
    expected_speakers: Option<i32>,
) -> Result<Vec<SpeakerTurn>, String> {
    let pyannote_result = run_pyannote_backend(audio_path, expected_speakers).await?;
    let turns = speaker_turns_from_diarized_segments(&pyannote_result.segments);
    if turns.is_empty() {
        return Err("Pyannote Community-1 returned no speaker turns".to_string());
    }

    Ok(turns)
}

#[command]
pub fn align_speaker_turns_to_transcription(
    segments_json: String,
    speaker_turns_json: String,
) -> Result<DiarizedResult, String> {
    let segments = serde_json::from_str::<Vec<TranscriptionSegment>>(&segments_json)
        .map_err(|e| format!("Failed to parse transcription segments JSON: {e}"))?;
    let turns = serde_json::from_str::<Vec<SpeakerTurn>>(&speaker_turns_json)
        .map_err(|e| format!("Failed to parse speaker turns JSON: {e}"))?;

    Ok(diarize_segments_with_speaker_turns(&segments, &turns))
}

pub async fn diarize_audio_chunks_with_modern_cpu(
    audio_chunks: Vec<ExportedChunk>,
    segments: Vec<TranscriptionSegment>,
    expected_speakers: Option<i32>,
    max_parallel_chunks: usize,
) -> Result<DiarizedResult, String> {
    if audio_chunks.is_empty() {
        return Err("Modern CPU chunked diarization requires audio chunks".to_string());
    }

    let stitched_turns =
        run_modern_cpu_backend_batch(audio_chunks, expected_speakers, max_parallel_chunks).await?;
    if segments.is_empty() {
        return Ok(stitched_turns);
    }

    Ok(align_modern_cpu_diarization_to_transcript(
        stitched_turns,
        &segments,
    ))
}

pub async fn diarize_audio_with_pyannote(
    audio_path: String,
    segments: Vec<TranscriptionSegment>,
    expected_speakers: Option<i32>,
) -> Result<DiarizedResult, String> {
    let pyannote_result = run_pyannote_backend(audio_path, expected_speakers).await?;
    Ok(align_modern_cpu_diarization_to_transcript(
        pyannote_result,
        &segments,
    ))
}

#[command]
pub async fn refine_diarization_selectively(
    app: tauri::AppHandle,
    segments_json: String,
    speaker_turns_json: String,
    audio_chunks: Vec<ExportedChunk>,
    expected_speakers: Option<i32>,
    num_threads: Option<i32>,
    max_refinement_chunks: Option<usize>,
) -> Result<DiarizedResult, String> {
    let segments = Arc::new(
        serde_json::from_str::<Vec<TranscriptionSegment>>(&segments_json)
            .map_err(|e| format!("Failed to parse transcription segments JSON: {e}"))?,
    );
    let turns = serde_json::from_str::<Vec<SpeakerTurn>>(&speaker_turns_json)
        .map_err(|e| format!("Failed to parse speaker turns JSON: {e}"))?;
    let base = diarize_segments_with_speaker_turns(segments.as_ref().as_slice(), &turns);
    let _ = num_threads;
    let default_budget = (audio_chunks.len() / 6).clamp(1, 3);
    let budget = max_refinement_chunks.unwrap_or(default_budget);
    let windows = select_suspicious_refinement_windows_with_context(
        segments.as_ref().as_slice(),
        &turns,
        &audio_chunks,
        budget,
        expected_speakers,
    );

    if windows.is_empty() {
        return Ok(base);
    }

    let window_chunks = match export_refinement_windows(&app, &windows).await {
        Ok(chunks) if !chunks.is_empty() => chunks,
        _ => return Ok(base),
    };
    let local_expected_speakers = expected_speakers.map(|value| value.clamp(2, 3));
    let source_segments = segments.clone();
    let mut refined_parts = stream::iter(window_chunks.iter().cloned())
        .map(|window_chunk| {
            let source_segments = source_segments.clone();
            async move {
                let window_segments = segments_for_chunk(&source_segments, &window_chunk);
                if window_segments.is_empty() {
                    return None;
                }

                let local_refined = diarize_audio_with_modern_cpu(
                    window_chunk.audio_path.clone(),
                    &window_segments,
                    local_expected_speakers,
                )
                .await
                .ok()?;

                Some(shift_diarized_result(
                    local_refined,
                    window_chunk.offset_sec,
                ))
            }
        })
        .buffer_unordered(3)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    refined_parts.sort_by(|a, b| {
        let a_start = a
            .segments
            .first()
            .map(|segment| segment.start)
            .unwrap_or(0.0);
        let b_start = b
            .segments
            .first()
            .map(|segment| segment.start)
            .unwrap_or(0.0);
        a_start
            .partial_cmp(&b_start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let refined = stitch_diarized_chunk_results(refined_parts, 1.0);
    let merged = merge_selective_refinement(base, refined, &window_chunks);
    cleanup_exported_chunk_parent_dirs(&window_chunks).await;
    Ok(merged)
}

pub fn diarize_segments_with_speaker_turns(
    segments: &[TranscriptionSegment],
    turns: &[SpeakerTurn],
) -> DiarizedResult {
    if turns.is_empty() {
        return diarize_transcription_locally(segments);
    }

    let speaker_indexes = turns
        .iter()
        .map(|turn| turn.speaker_index.max(0))
        .collect::<BTreeSet<_>>();
    let speakers = if speaker_indexes.is_empty() {
        vec!["Falante 1".to_string()]
    } else {
        speaker_indexes
            .into_iter()
            .map(|speaker| format!("Falante {}", speaker + 1))
            .collect::<Vec<_>>()
    };
    let mut diarized_segments = Vec::new();
    let assigned_speakers = best_speakers_for_segments_sweep(segments, turns);

    for (segment, speaker_index) in segments.iter().zip(assigned_speakers) {
        let speaker_index = speaker_index.max(0);
        let speaker = format!("Falante {}", speaker_index + 1);
        push_or_merge_segment(
            &mut diarized_segments,
            &speaker,
            segment.start,
            segment.end,
            segment.text.trim(),
            true,
        );
    }

    DiarizedResult {
        speakers,
        segments: diarized_segments,
    }
}

pub fn diarize_transcription_locally(segments: &[TranscriptionSegment]) -> DiarizedResult {
    let mut speakers = Vec::new();
    let mut diarized_segments = Vec::new();
    let mut current_speaker = "Falante 1".to_string();
    let mut previous_end: Option<f64> = None;
    let mut previous_text = String::new();

    for segment in segments {
        let (explicit_speaker, clean_text) = split_inline_speaker_label(&segment.text)
            .map(|(speaker, text)| (Some(speaker), text))
            .unwrap_or_else(|| (None, segment.text.trim().to_string()));

        if clean_text.is_empty() {
            previous_end = Some(segment.end);
            continue;
        }

        let gap = previous_end
            .map(|end| (segment.start - end).max(0.0))
            .unwrap_or(0.0);

        let speaker = if let Some(speaker) = explicit_speaker {
            speaker
        } else if !previous_text.is_empty()
            && gap <= QUICK_ANSWER_WINDOW_SEC
            && (is_question(&previous_text) || starts_like_answer(&clean_text))
        {
            next_generic_speaker(&current_speaker)
        } else {
            current_speaker.clone()
        };

        let should_merge = gap <= QUICK_MERGE_GAP_SEC;
        push_speaker_once(&mut speakers, &speaker);
        push_or_merge_segment(
            &mut diarized_segments,
            &speaker,
            segment.start,
            segment.end,
            &clean_text,
            should_merge,
        );

        current_speaker = speaker;
        previous_end = Some(segment.end);
        previous_text = clean_text;
    }

    if speakers.is_empty() {
        speakers.push("Falante 1".to_string());
    }

    DiarizedResult {
        speakers,
        segments: diarized_segments,
    }
}

pub async fn diarize_with_mode_report(
    audio_path: String,
    segments: Vec<TranscriptionSegment>,
    audio_chunks: Option<Vec<ExportedChunk>>,
    assets: DiarizationAssetPaths,
    expected_speakers: Option<i32>,
    mode: DiarizationMode,
    num_threads: i32,
) -> Result<DiarizationRun, String> {
    let started = Instant::now();
    if mode == DiarizationMode::Fast {
        let result = diarize_transcription_locally(&segments);
        return Ok(build_diarization_run(
            mode,
            DiarizationBackend::FastLocal,
            None,
            started,
            result,
        ));
    }
    let segments = Arc::new(segments);

    match mode {
        DiarizationMode::ModernCpu => {
            let result = diarize_audio_with_modern_cpu(
                audio_path,
                segments.as_ref().as_slice(),
                expected_speakers,
            )
            .await?;
            return Ok(build_diarization_run(
                mode,
                DiarizationBackend::ModernCpu,
                None,
                started,
                result,
            ));
        }
        DiarizationMode::ModernCpuChunked => {
            let chunks = audio_chunks.ok_or_else(|| {
                "Modern CPU chunked diarization requires audio chunks".to_string()
            })?;
            let result = diarize_audio_chunks_with_modern_cpu(
                chunks,
                segments.as_ref().clone(),
                expected_speakers,
                num_threads.max(1) as usize,
            )
            .await?;
            return Ok(build_diarization_run(
                mode,
                DiarizationBackend::ModernCpuChunked,
                None,
                started,
                result,
            ));
        }
        DiarizationMode::Pyannote => {
            let result = diarize_audio_with_pyannote(
                audio_path,
                segments.as_ref().clone(),
                expected_speakers,
            )
            .await?;
            return Ok(build_diarization_run(
                mode,
                DiarizationBackend::Pyannote,
                None,
                started,
                result,
            ));
        }
        DiarizationMode::Hybrid => {
            let chunks = audio_chunks
                .ok_or_else(|| "Hybrid diarization requires audio chunks".to_string())?;
            if !chunks_can_use_sherpa(&chunks) {
                return Err("Hybrid Sherpa diarization requires WAV chunks".to_string());
            }
            let result = diarize_audio_by_chunks_with_sherpa(
                chunks,
                segments.clone(),
                assets,
                expected_speakers,
                num_threads,
            )
            .await?;
            return Ok(build_diarization_run(
                mode,
                DiarizationBackend::SherpaHybrid,
                None,
                started,
                result,
            ));
        }
        DiarizationMode::Precise => {
            let sherpa_segments = segments.clone();
            let result = tokio::task::spawn_blocking(move || {
                diarize_audio_with_sherpa(
                    audio_path,
                    sherpa_segments.as_ref().as_slice(),
                    assets,
                    expected_speakers,
                    num_threads,
                )
            })
            .await
            .map_err(|e| format!("Offline diarization worker failed: {e}"))??;
            return Ok(build_diarization_run(
                mode,
                DiarizationBackend::SherpaPrecise,
                None,
                started,
                result,
            ));
        }
        DiarizationMode::Auto | DiarizationMode::Fast => {}
    }

    let mut fallback_reasons = Vec::new();
    match diarize_audio_with_modern_cpu(
        audio_path.clone(),
        segments.as_ref().as_slice(),
        expected_speakers,
    )
    .await
    {
        Ok(result) => {
            return Ok(build_diarization_run(
                mode,
                DiarizationBackend::ModernCpu,
                join_fallback_reasons(&fallback_reasons),
                started,
                result,
            ));
        }
        Err(error) => {
            push_fallback_reason(&mut fallback_reasons, DiarizationBackend::ModernCpu, error)
        }
    }

    if expected_speakers.unwrap_or(0) > 0 {
        if let Some(chunks) = audio_chunks.clone().filter(|chunks| chunks.len() > 1) {
            match diarize_audio_chunks_with_modern_cpu(
                chunks,
                segments.as_ref().clone(),
                expected_speakers,
                num_threads.max(1) as usize,
            )
            .await
            {
                Ok(result) => {
                    return Ok(build_diarization_run(
                        mode,
                        DiarizationBackend::ModernCpuChunked,
                        join_fallback_reasons(&fallback_reasons),
                        started,
                        result,
                    ));
                }
                Err(error) => push_fallback_reason(
                    &mut fallback_reasons,
                    DiarizationBackend::ModernCpuChunked,
                    error,
                ),
            }
        }
    }

    if let Some(chunks) = audio_chunks
        .clone()
        .filter(|chunks| chunks_can_use_sherpa(chunks))
    {
        match diarize_audio_by_chunks_with_sherpa(
            chunks,
            segments.clone(),
            assets.clone(),
            expected_speakers,
            num_threads,
        )
        .await
        {
            Ok(result) => {
                return Ok(build_diarization_run(
                    mode,
                    DiarizationBackend::SherpaHybrid,
                    join_fallback_reasons(&fallback_reasons),
                    started,
                    result,
                ));
            }
            Err(error) => push_fallback_reason(
                &mut fallback_reasons,
                DiarizationBackend::SherpaHybrid,
                error,
            ),
        }
    }

    let sherpa_segments = segments.clone();
    let sherpa_result = tokio::task::spawn_blocking(move || {
        diarize_audio_with_sherpa(
            audio_path,
            sherpa_segments.as_ref().as_slice(),
            assets,
            expected_speakers,
            num_threads,
        )
    })
    .await
    .map_err(|e| format!("Offline diarization worker failed: {e}"))?;

    match sherpa_result {
        Ok(result) => Ok(build_diarization_run(
            mode,
            DiarizationBackend::SherpaPrecise,
            join_fallback_reasons(&fallback_reasons),
            started,
            result,
        )),
        Err(error) if mode_allows_local_fallback(mode) => {
            push_fallback_reason(
                &mut fallback_reasons,
                DiarizationBackend::SherpaPrecise,
                error,
            );
            let result = diarize_transcription_locally(segments.as_ref().as_slice());
            Ok(build_diarization_run(
                mode,
                DiarizationBackend::FastLocal,
                join_fallback_reasons(&fallback_reasons),
                started,
                result,
            ))
        }
        Err(error) => Err(error),
    }
}

pub async fn diarize_with_mode(
    audio_path: String,
    segments: Vec<TranscriptionSegment>,
    audio_chunks: Option<Vec<ExportedChunk>>,
    assets: DiarizationAssetPaths,
    expected_speakers: Option<i32>,
    mode: DiarizationMode,
    num_threads: i32,
) -> Result<DiarizedResult, String> {
    Ok(diarize_with_mode_report(
        audio_path,
        segments,
        audio_chunks,
        assets,
        expected_speakers,
        mode,
        num_threads,
    )
    .await?
    .result)
}

#[command]
pub fn diarize_transcription_fast(segments_json: String) -> Result<DiarizedResult, String> {
    let segments = serde_json::from_str::<Vec<TranscriptionSegment>>(&segments_json)
        .map_err(|e| format!("Failed to parse transcription segments JSON: {e}"))?;
    Ok(diarize_transcription_locally(&segments))
}

#[command]
pub async fn diarize_transcription_end_to_end(
    app: tauri::AppHandle,
    audio_path: String,
    segments_json: String,
    audio_chunks: Option<Vec<ExportedChunk>>,
    mode: Option<String>,
    num_threads: Option<i32>,
    expected_speakers: Option<i32>,
) -> Result<DiarizedResultWithTelemetry, String> {
    let started = Instant::now();
    let segments = serde_json::from_str::<Vec<TranscriptionSegment>>(&segments_json)
        .map_err(|e| format!("Failed to parse transcription segments JSON: {e}"))?;
    let mode = DiarizationMode::from_option(mode);

    if mode == DiarizationMode::Fast {
        let result = diarize_transcription_locally(&segments);
        return Ok(build_diarization_run(
            mode,
            DiarizationBackend::FastLocal,
            None,
            started,
            result,
        )
        .into());
    }

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {e}"))?;
    let assets = if matches!(mode, DiarizationMode::Hybrid | DiarizationMode::Precise) {
        ensure_diarization_assets_in_dir(&app_data_dir).await?
    } else {
        diarization_asset_paths(&app_data_dir)
    };

    let num_threads = normalize_diarization_threads(num_threads, available_parallelism_count());
    let run = match diarize_with_mode_report(
        audio_path,
        segments,
        audio_chunks,
        assets,
        expected_speakers,
        mode,
        num_threads,
    )
    .await
    {
        Ok(run) => run,
        Err(error) if mode_allows_local_fallback(mode) => {
            let segments = serde_json::from_str::<Vec<TranscriptionSegment>>(&segments_json)
                .map_err(|e| format!("Failed to parse transcription segments JSON: {e}"))?;
            let result = diarize_transcription_locally(&segments);
            build_diarization_run(
                mode,
                DiarizationBackend::FastLocal,
                Some(error),
                started,
                result,
            )
        }
        Err(error) => return Err(error),
    };

    Ok(run.into())
}

#[command]
pub async fn diarize_transcription(
    segments_json: String,
    gemini_api_key: String,
) -> Result<DiarizedResult, String> {
    let prompt = format!(
        r#"Voce recebera a transcricao segmentada de uma reuniao em PT-BR no formato JSON.
Identifique os diferentes falantes e associe cada segmento a um falante.

REGRAS:
- Identifique falantes como "Falante 1", "Falante 2", etc.
- Se um nome for dito explicitamente na conversa ("Fala Joao"), use o nome.
- Agrupe segmentos consecutivos do mesmo falante em um unico objeto.
- Retorne SOMENTE JSON valido. Sem markdown. Sem texto antes ou depois.

INPUT:
{}

OUTPUT (formato exato):
{{
  "speakers": ["Falante 1", "Falante 2"],
  "segments": [
    {{
      "speaker": "Falante 1",
      "start": 0.0,
      "end": 9.1,
      "text": "texto completo do bloco"
    }}
  ]
}}"#,
        segments_json
    );

    let body = serde_json::json!({
        "contents": [{
            "parts": [{ "text": prompt }]
        }],
        "generationConfig": {
            "temperature": 0.1,
            "responseMimeType": "application/json"
        }
    });

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        gemini_api_key
    );

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let err = response.text().await.unwrap_or_default();
        return Err(format!("Gemini API error: {}", err));
    }

    let result: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

    let text = result["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .ok_or("Empty response from Gemini")?;

    serde_json::from_str::<DiarizedResult>(text).map_err(|e| {
        format!(
            "Failed to parse diarization JSON: {} - Response: {}",
            e, text
        )
    })
}

#[cfg(test)]
mod tests;
