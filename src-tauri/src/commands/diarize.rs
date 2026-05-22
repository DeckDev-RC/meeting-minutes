use crate::models::audio::ExportedChunk;
use crate::models::transcription::{DiarizedResult, DiarizedSegment, TranscriptionSegment};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig, Wave,
};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use tauri::{command, Manager};
use tauri_plugin_shell::ShellExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

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
struct GlobalSpeakerCentroid {
    speaker: String,
    embedding: Vec<f64>,
    observations: usize,
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

        if let Some(expected_speakers) = expected_speakers.filter(|value| *value > 0) {
            command
                .arg("--num-speakers")
                .arg(expected_speakers.to_string());
        }

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

pub async fn diarize_audio_by_chunks_with_sherpa(
    audio_chunks: Vec<ExportedChunk>,
    segments: Arc<Vec<TranscriptionSegment>>,
    assets: DiarizationAssetPaths,
    expected_speakers: Option<i32>,
    num_threads: i32,
) -> Result<DiarizedResult, String> {
    if audio_chunks.is_empty() {
        return Ok(diarize_transcription_locally(segments.as_slice()));
    }

    let mut sorted_chunks = audio_chunks;
    sorted_chunks.sort_by(|a, b| {
        a.start_sec
            .partial_cmp(&b.start_sec)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.index.cmp(&b.index))
    });

    let available_threads = available_parallelism_count();
    let worker_count =
        ((available_threads as i32 / num_threads.max(1)).max(1) as usize).min(sorted_chunks.len());
    let queue = Arc::new(Mutex::new(VecDeque::from(sorted_chunks)));
    let mut handles = Vec::new();

    for _ in 0..worker_count {
        let queue = queue.clone();
        let segments = segments.clone();
        let assets = assets.clone();
        handles.push(tokio::spawn(async move {
            let mut local_results = Vec::new();

            loop {
                let chunk = {
                    let mut guard = queue.lock().await;
                    guard.pop_front()
                };
                let Some(chunk) = chunk else {
                    break;
                };

                let chunk_segments = segments_for_chunk(&segments, &chunk);
                if chunk_segments.is_empty() {
                    continue;
                }

                let chunk_path = chunk.audio_path.clone();
                let chunk_assets = assets.clone();
                let chunk_segments_for_sherpa = chunk_segments.clone();
                let diarized = tokio::task::spawn_blocking(move || {
                    diarize_audio_with_sherpa(
                        chunk_path,
                        &chunk_segments_for_sherpa,
                        chunk_assets,
                        expected_speakers,
                        num_threads,
                    )
                })
                .await
                .map_err(|e| format!("Hybrid diarization worker failed: {e}"))?
                .map_err(|e| {
                    format!(
                        "Hybrid Sherpa diarization failed for chunk {}: {e}",
                        chunk.index
                    )
                })?;

                local_results.push((
                    chunk.index,
                    shift_diarized_result(diarized, chunk.offset_sec),
                ));
            }

            Ok::<Vec<(usize, DiarizedResult)>, String>(local_results)
        }));
    }

    let mut chunk_results = Vec::new();
    for handle in handles {
        chunk_results.extend(
            handle
                .await
                .map_err(|e| format!("Hybrid diarization join failed: {e}"))??,
        );
    }

    chunk_results.sort_by_key(|(index, _)| *index);
    Ok(stitch_diarized_chunk_results(
        chunk_results
            .into_iter()
            .map(|(_, result)| result)
            .collect(),
        3.0,
    ))
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
            if expected_speakers.unwrap_or(0) <= 0 {
                return Err(
                    "Modern CPU chunked diarization requires expectedSpeakers for stable labels"
                        .to_string(),
                );
            }
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

fn chunk_path_is_wav(chunk: &ExportedChunk) -> bool {
    Path::new(&chunk.audio_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("wav"))
        .unwrap_or(false)
}

fn chunks_can_use_sherpa(chunks: &[ExportedChunk]) -> bool {
    !chunks.is_empty() && chunks.iter().all(chunk_path_is_wav)
}

fn chunk_for_segment<'a>(
    segment: &TranscriptionSegment,
    chunks: &'a [ExportedChunk],
) -> Option<&'a ExportedChunk> {
    let midpoint = (segment.start + segment.end) / 2.0;
    chunks
        .iter()
        .find(|chunk| midpoint >= chunk.start_sec && midpoint <= chunk.end_sec)
        .or_else(|| {
            chunks
                .iter()
                .find(|chunk| segment.end > chunk.start_sec && segment.start < chunk.end_sec)
        })
}

fn segment_overlap_stats(segment: &TranscriptionSegment, turns: &[SpeakerTurn]) -> (f64, f64) {
    let mut best = 0.0;
    let mut second = 0.0;
    let first_candidate = turns.partition_point(|turn| turn.end <= segment.start);

    for turn in &turns[first_candidate..] {
        if turn.start >= segment.end {
            break;
        }
        let overlap = overlap_seconds(segment.start, segment.end, turn.start, turn.end);
        if overlap > best {
            second = best;
            best = overlap;
        } else if overlap > second {
            second = overlap;
        }
    }

    (best, second)
}

fn suspicion_score_for_segment(segment: &TranscriptionSegment, turns: &[SpeakerTurn]) -> f64 {
    let duration = (segment.end - segment.start).max(0.0);
    if duration <= 0.0 {
        return 0.0;
    }

    let (best_overlap, second_overlap) = segment_overlap_stats(segment, turns);
    let coverage_ratio = best_overlap / duration;
    let ambiguity_ratio = second_overlap / duration;
    let mut score = 0.0;

    if coverage_ratio < 0.45 {
        score += 3.0;
    } else if coverage_ratio < 0.7 {
        score += 1.5;
    }

    if ambiguity_ratio >= 0.25 {
        score += 2.0;
    }

    if duration >= 18.0 && segment.text.contains('?') {
        score += 0.75;
    }

    score
}

fn segments_for_refinement_chunk<'a>(
    segments: &'a [TranscriptionSegment],
    chunk: &ExportedChunk,
) -> Vec<&'a TranscriptionSegment> {
    let first_candidate = segments.partition_point(|segment| segment.end <= chunk.start_sec);
    segments[first_candidate..]
        .iter()
        .take_while(|segment| segment.start < chunk.end_sec)
        .filter(|segment| segment.end > chunk.start_sec)
        .collect()
}

fn chunk_context_suspicion_score(
    chunk_segments: &[&TranscriptionSegment],
    turns: &[SpeakerTurn],
    expected_speakers: Option<i32>,
) -> f64 {
    if chunk_segments.len() < 3 || turns.is_empty() {
        return 0.0;
    }

    let assigned_speakers = best_speakers_for_segment_refs_sweep(chunk_segments, turns);
    let unique_speakers = assigned_speakers.iter().copied().collect::<BTreeSet<_>>();
    let switches = assigned_speakers
        .windows(2)
        .filter(|pair| pair[0] != pair[1])
        .count();
    let switch_ratio = switches as f64 / (assigned_speakers.len().saturating_sub(1).max(1) as f64);
    let mut score = 0.0;

    if chunk_segments.len() >= 6 && switch_ratio >= 0.55 {
        score += 3.0 + switch_ratio;
    }

    if expected_speakers.unwrap_or(0) >= 2 && unique_speakers.len() <= 1 {
        let speech_span = chunk_segments.last().map(|last| last.end).unwrap_or(0.0)
            - chunk_segments
                .first()
                .map(|first| first.start)
                .unwrap_or(0.0);
        if speech_span >= 30.0 {
            score += 2.75;
        }
    }

    score
}

fn refinement_window_for_segments(
    chunk: &ExportedChunk,
    segments: &[&TranscriptionSegment],
) -> Option<RefinementWindow> {
    const PADDING_SEC: f64 = 4.0;
    const MAX_WINDOW_SEC: f64 = 45.0;

    let first = segments.first()?;
    let last = segments.last()?;
    let mut start_sec = (first.start - PADDING_SEC).max(chunk.start_sec);
    let mut end_sec = (last.end + PADDING_SEC).min(chunk.end_sec);
    if end_sec <= start_sec {
        return None;
    }

    if end_sec - start_sec > MAX_WINDOW_SEC {
        let midpoint = (start_sec + end_sec) / 2.0;
        start_sec = (midpoint - MAX_WINDOW_SEC / 2.0).max(chunk.start_sec);
        end_sec = (start_sec + MAX_WINDOW_SEC).min(chunk.end_sec);
        start_sec = (end_sec - MAX_WINDOW_SEC).max(chunk.start_sec);
    }

    Some(RefinementWindow {
        chunk: chunk.clone(),
        start_sec,
        end_sec,
    })
}

pub fn select_suspicious_chunks_for_refinement(
    segments: &[TranscriptionSegment],
    turns: &[SpeakerTurn],
    chunks: &[ExportedChunk],
    max_chunks: usize,
) -> Vec<ExportedChunk> {
    if segments.is_empty() || chunks.is_empty() || max_chunks == 0 || !chunks_can_use_sherpa(chunks)
    {
        return Vec::new();
    }

    let mut scores: HashMap<usize, f64> = HashMap::new();
    for segment in segments {
        let score = suspicion_score_for_segment(segment, turns);
        if score <= 0.0 {
            continue;
        }

        if let Some(chunk) = chunk_for_segment(segment, chunks) {
            *scores.entry(chunk.index).or_insert(0.0) += score;
        }
    }

    let mut ranked = chunks
        .iter()
        .filter_map(|chunk| {
            scores
                .get(&chunk.index)
                .copied()
                .filter(|score| *score > 0.0)
                .map(|score| (score, chunk.clone()))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|(a_score, a_chunk), (b_score, b_chunk)| {
        b_score
            .partial_cmp(a_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a_chunk.index.cmp(&b_chunk.index))
    });

    ranked
        .into_iter()
        .take(max_chunks)
        .map(|(_, chunk)| chunk)
        .collect()
}

pub fn select_suspicious_refinement_windows(
    segments: &[TranscriptionSegment],
    turns: &[SpeakerTurn],
    chunks: &[ExportedChunk],
    max_windows: usize,
) -> Vec<RefinementWindow> {
    select_suspicious_refinement_windows_with_context(segments, turns, chunks, max_windows, None)
}

pub fn select_suspicious_refinement_windows_with_context(
    segments: &[TranscriptionSegment],
    turns: &[SpeakerTurn],
    chunks: &[ExportedChunk],
    max_windows: usize,
    expected_speakers: Option<i32>,
) -> Vec<RefinementWindow> {
    const PADDING_SEC: f64 = 4.0;
    const MAX_WINDOW_SEC: f64 = 45.0;

    if segments.is_empty()
        || chunks.is_empty()
        || max_windows == 0
        || !chunks_can_use_sherpa(chunks)
    {
        return Vec::new();
    }

    let mut ranked = Vec::new();
    for chunk in chunks {
        let chunk_segments = segments_for_refinement_chunk(segments, chunk);
        let score = chunk_context_suspicion_score(&chunk_segments, turns, expected_speakers);
        if score <= 0.0 {
            continue;
        }
        if let Some(window) = refinement_window_for_segments(chunk, &chunk_segments) {
            ranked.push((score, window));
        }
    }

    for segment in segments {
        let score = suspicion_score_for_segment(segment, turns);
        if score <= 0.0 {
            continue;
        }

        let Some(chunk) = chunk_for_segment(segment, chunks) else {
            continue;
        };

        let mut start_sec = (segment.start - PADDING_SEC).max(chunk.start_sec);
        let mut end_sec = (segment.end + PADDING_SEC).min(chunk.end_sec);
        if end_sec <= start_sec {
            continue;
        }

        if end_sec - start_sec > MAX_WINDOW_SEC {
            let midpoint = (segment.start + segment.end) / 2.0;
            start_sec = (midpoint - MAX_WINDOW_SEC / 2.0).max(chunk.start_sec);
            end_sec = (start_sec + MAX_WINDOW_SEC).min(chunk.end_sec);
            start_sec = (end_sec - MAX_WINDOW_SEC).max(chunk.start_sec);
        }

        ranked.push((
            score,
            RefinementWindow {
                chunk: chunk.clone(),
                start_sec,
                end_sec,
            },
        ));
    }

    ranked.sort_by(|(a_score, a_window), (b_score, b_window)| {
        b_score
            .partial_cmp(a_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a_window
                    .start_sec
                    .partial_cmp(&b_window.start_sec)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    let mut windows: Vec<RefinementWindow> = Vec::new();
    for (_, window) in ranked {
        if windows.iter().any(|existing| {
            existing.chunk.index == window.chunk.index
                && window.start_sec < existing.end_sec
                && window.end_sec > existing.start_sec
        }) {
            continue;
        }
        windows.push(window);
        if windows.len() >= max_windows {
            break;
        }
    }

    windows
}

fn segment_overlaps_refined_chunks(segment: &DiarizedSegment, chunks: &[ExportedChunk]) -> bool {
    chunks
        .iter()
        .any(|chunk| segment.end > chunk.start_sec && segment.start < chunk.end_sec)
}

pub fn merge_selective_refinement(
    base: DiarizedResult,
    refined: DiarizedResult,
    refined_chunks: &[ExportedChunk],
) -> DiarizedResult {
    if refined_chunks.is_empty() || refined.segments.is_empty() {
        return base;
    }

    let mut segments = base
        .segments
        .into_iter()
        .filter(|segment| !segment_overlaps_refined_chunks(segment, refined_chunks))
        .collect::<Vec<_>>();
    segments.extend(refined.segments);
    segments.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.end
                    .partial_cmp(&b.end)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let mut speakers = Vec::new();
    for segment in &segments {
        push_speaker_once(&mut speakers, &segment.speaker);
    }
    if speakers.is_empty() {
        speakers.push("Falante 1".to_string());
    }

    DiarizedResult { speakers, segments }
}

#[command]
pub fn diarize_transcription_fast(segments_json: String) -> Result<DiarizedResult, String> {
    let segments = serde_json::from_str::<Vec<TranscriptionSegment>>(&segments_json)
        .map_err(|e| format!("Failed to parse transcription segments JSON: {e}"))?;
    Ok(diarize_transcription_locally(&segments))
}

pub fn diarize_audio_with_sherpa(
    audio_path: String,
    segments: &[TranscriptionSegment],
    assets: DiarizationAssetPaths,
    expected_speakers: Option<i32>,
    num_threads: i32,
) -> Result<DiarizedResult, String> {
    let num_clusters = expected_speakers.filter(|value| *value > 0).unwrap_or(-1);
    let num_threads = num_threads.max(1);
    let config = OfflineSpeakerDiarizationConfig {
        segmentation: OfflineSpeakerSegmentationModelConfig {
            pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                model: Some(assets.segmentation_model.to_string_lossy().to_string()),
            },
            provider: Some("cpu".to_string()),
            num_threads,
            ..Default::default()
        },
        embedding: SpeakerEmbeddingExtractorConfig {
            model: Some(assets.embedding_model.to_string_lossy().to_string()),
            provider: Some("cpu".to_string()),
            num_threads,
            ..Default::default()
        },
        clustering: FastClusteringConfig {
            num_clusters,
            threshold: 0.5,
        },
        min_duration_on: 0.3,
        min_duration_off: 0.5,
    };

    let diarizer = OfflineSpeakerDiarization::create(&config)
        .ok_or_else(|| "Failed to initialize offline speaker diarization".to_string())?;
    let wave = Wave::read(&audio_path).ok_or_else(|| "Failed to read WAV audio".to_string())?;

    if diarizer.sample_rate() != wave.sample_rate() {
        return Err(format!(
            "Unexpected diarization sample rate. Model expects {} Hz, audio has {} Hz",
            diarizer.sample_rate(),
            wave.sample_rate()
        ));
    }

    let result = diarizer
        .process(wave.samples())
        .ok_or_else(|| "Offline speaker diarization failed".to_string())?;
    let turns = result
        .sort_by_start_time()
        .into_iter()
        .map(|segment| SpeakerTurn {
            start: segment.start as f64,
            end: segment.end as f64,
            speaker_index: segment.speaker,
        })
        .collect::<Vec<_>>();

    if turns.is_empty() {
        return Err("Offline speaker diarization returned no speaker turns".to_string());
    }

    Ok(diarize_segments_with_speaker_turns(&segments, &turns))
}

pub fn segments_for_chunk(
    segments: &[TranscriptionSegment],
    chunk: &ExportedChunk,
) -> Vec<TranscriptionSegment> {
    let sorted_segments = segments
        .windows(2)
        .all(|pair| pair[0].start <= pair[1].start);
    let first_candidate = if sorted_segments {
        segments.partition_point(|segment| segment.end <= chunk.start_sec)
    } else {
        0
    };

    let candidates = segments[first_candidate..]
        .iter()
        .take_while(|segment| !sorted_segments || segment.start < chunk.end_sec)
        .filter(|segment| segment.end > chunk.start_sec && segment.start < chunk.end_sec);

    candidates
        .filter_map(|segment| {
            let start = (segment.start - chunk.offset_sec).max(0.0);
            let end = (segment.end - chunk.offset_sec).min(chunk.duration_sec);
            if end <= start {
                return None;
            }

            Some(TranscriptionSegment {
                id: segment.id,
                start,
                end,
                text: segment.text.clone(),
            })
        })
        .collect()
}

pub fn shift_diarized_result(mut result: DiarizedResult, offset_sec: f64) -> DiarizedResult {
    for segment in &mut result.segments {
        segment.start += offset_sec;
        segment.end += offset_sec;
    }
    result
}

fn normalized_embedding(embedding: &[f64]) -> Option<Vec<f64>> {
    let norm = embedding
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm <= f64::EPSILON {
        return None;
    }

    Some(embedding.iter().map(|value| value / norm).collect())
}

fn cosine_similarity(left: &[f64], right: &[f64]) -> Option<f64> {
    if left.len() != right.len() || left.is_empty() {
        return None;
    }

    let left = normalized_embedding(left)?;
    let right = normalized_embedding(right)?;
    Some(
        left.iter()
            .zip(right.iter())
            .map(|(left, right)| left * right)
            .sum(),
    )
}

fn best_centroid_match<'a>(
    local: &SpeakerCentroid,
    global_centroids: &'a [GlobalSpeakerCentroid],
    min_cosine: Option<f64>,
) -> Option<&'a str> {
    global_centroids
        .iter()
        .filter_map(|global| {
            cosine_similarity(&local.embedding, &global.embedding)
                .map(|score| (global.speaker.as_str(), score))
        })
        .filter(|(_, score)| min_cosine.map(|min| *score >= min).unwrap_or(true))
        .max_by(|(_, left), (_, right)| {
            left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(speaker, _)| speaker)
}

fn upsert_global_centroid(
    global_centroids: &mut Vec<GlobalSpeakerCentroid>,
    speaker: &str,
    local_embedding: &[f64],
) {
    let Some(local_embedding) = normalized_embedding(local_embedding) else {
        return;
    };

    if let Some(global) = global_centroids
        .iter_mut()
        .find(|global| global.speaker == speaker)
    {
        if global.embedding.len() != local_embedding.len() {
            return;
        }
        let previous_weight = global.observations.max(1) as f64;
        for (global_value, local_value) in global.embedding.iter_mut().zip(local_embedding) {
            *global_value =
                (*global_value * previous_weight + local_value) / (previous_weight + 1.0);
        }
        if let Some(normalized) = normalized_embedding(&global.embedding) {
            global.embedding = normalized;
        }
        global.observations += 1;
        return;
    }

    global_centroids.push(GlobalSpeakerCentroid {
        speaker: speaker.to_string(),
        embedding: local_embedding,
        observations: 1,
    });
}

fn expected_speaker_name_from_local_label(label: &str, expected_speakers: usize) -> Option<String> {
    if expected_speakers == 0 {
        return None;
    }

    let trimmed = label.trim();
    let index = if let Some(suffix) = trimmed
        .strip_prefix("SPEAKER_")
        .or_else(|| trimmed.strip_prefix("speaker_"))
    {
        suffix.trim_start_matches('0').parse::<usize>().unwrap_or(0) + 1
    } else {
        let digits = trimmed
            .chars()
            .rev()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        digits.parse::<usize>().ok()?
    };

    if index == 0 || index > expected_speakers {
        return None;
    }

    Some(format!("Falante {index}"))
}

pub fn stitch_diarized_chunk_results(
    chunk_results: Vec<DiarizedResult>,
    overlap_sec: f64,
) -> DiarizedResult {
    stitch_diarized_chunk_results_internal(chunk_results, overlap_sec, None)
}

pub fn stitch_diarized_chunk_results_with_expected_speakers(
    chunk_results: Vec<DiarizedResult>,
    overlap_sec: f64,
    expected_speakers: Option<i32>,
) -> DiarizedResult {
    stitch_diarized_chunk_results_internal(
        chunk_results,
        overlap_sec,
        expected_speakers
            .filter(|value| *value > 0)
            .map(|value| value as usize),
    )
}

fn stitch_diarized_chunk_results_internal(
    chunk_results: Vec<DiarizedResult>,
    overlap_sec: f64,
    expected_speakers: Option<usize>,
) -> DiarizedResult {
    let mut speakers = Vec::new();
    let mut segments: Vec<DiarizedSegment> = Vec::new();

    for result in chunk_results {
        let mut local_to_global: HashMap<String, String> = HashMap::new();
        let mut overlap_scores: HashMap<String, HashMap<String, f64>> = HashMap::new();

        for local in &result.segments {
            let first_candidate = segments.partition_point(|existing| existing.end <= local.start);
            for existing in &segments[first_candidate..] {
                if existing.start >= local.end {
                    break;
                }
                let overlap = overlap_seconds(local.start, local.end, existing.start, existing.end);
                if overlap <= 0.0 {
                    continue;
                }
                *overlap_scores
                    .entry(local.speaker.clone())
                    .or_default()
                    .entry(existing.speaker.clone())
                    .or_insert(0.0) += overlap;
            }
        }

        for (local_speaker, global_scores) in overlap_scores {
            if let Some((global_speaker, _)) = global_scores
                .into_iter()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            {
                local_to_global.insert(local_speaker, global_speaker);
            }
        }

        for mut segment in result.segments {
            let original_speaker = segment.speaker.clone();
            let canonical = local_to_global
                .get(&original_speaker)
                .cloned()
                .or_else(|| {
                    expected_speakers.and_then(|expected| {
                        expected_speaker_name_from_local_label(&original_speaker, expected)
                    })
                })
                .unwrap_or_else(|| {
                    let speaker = format!("Falante {}", speakers.len() + 1);
                    local_to_global.insert(original_speaker.clone(), speaker.clone());
                    speaker
                });
            segment.speaker = canonical.clone();

            push_speaker_once(&mut speakers, &canonical);
            if let Some(last) = segments.last_mut() {
                if last.speaker == segment.speaker && segment.start <= last.end + overlap_sec {
                    last.end = last.end.max(segment.end);
                    let text = segment.text.trim();
                    if !text.is_empty() && !last.text.contains(text) {
                        if !last.text.is_empty() {
                            last.text.push(' ');
                        }
                        last.text.push_str(text);
                    }
                    continue;
                }
            }

            segments.push(segment);
        }
    }

    if speakers.is_empty() {
        speakers.push("Falante 1".to_string());
    }

    DiarizedResult { speakers, segments }
}

fn stitch_diarized_chunk_results_with_centroids(
    chunk_results: Vec<DiarizedChunkResult>,
    overlap_sec: f64,
    expected_speakers: Option<i32>,
) -> DiarizedResult {
    let expected_speakers = expected_speakers
        .filter(|value| *value > 0)
        .map(|value| value as usize);
    let mut speakers = Vec::new();
    let mut segments: Vec<DiarizedSegment> = Vec::new();
    let mut global_centroids: Vec<GlobalSpeakerCentroid> = Vec::new();

    for chunk in chunk_results {
        let has_centroids = !chunk.speaker_centroids.is_empty();
        let mut local_to_global: HashMap<String, String> = HashMap::new();
        let mut overlap_scores: HashMap<String, HashMap<String, f64>> = HashMap::new();

        for local in &chunk.diarized.segments {
            let first_candidate = segments.partition_point(|existing| existing.end <= local.start);
            for existing in &segments[first_candidate..] {
                if existing.start >= local.end {
                    break;
                }
                let overlap = overlap_seconds(local.start, local.end, existing.start, existing.end);
                if overlap <= 0.0 {
                    continue;
                }
                *overlap_scores
                    .entry(local.speaker.clone())
                    .or_default()
                    .entry(existing.speaker.clone())
                    .or_insert(0.0) += overlap;
            }
        }

        for (local_speaker, global_scores) in overlap_scores {
            if let Some((global_speaker, _)) = global_scores
                .into_iter()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            {
                local_to_global.insert(local_speaker, global_speaker);
            }
        }

        for centroid in &chunk.speaker_centroids {
            if local_to_global.contains_key(&centroid.speaker) {
                continue;
            }

            let speaker_cap_reached = expected_speakers
                .map(|expected| speakers.len() >= expected)
                .unwrap_or(false);
            let min_cosine = if speaker_cap_reached { None } else { Some(0.72) };

            if let Some(global_speaker) =
                best_centroid_match(centroid, &global_centroids, min_cosine)
            {
                local_to_global.insert(centroid.speaker.clone(), global_speaker.to_string());
            }
        }

        for mut segment in chunk.diarized.segments {
            let original_speaker = segment.speaker.clone();
            let canonical = local_to_global
                .get(&original_speaker)
                .cloned()
                .or_else(|| {
                    if has_centroids {
                        None
                    } else {
                        expected_speakers.and_then(|expected| {
                            expected_speaker_name_from_local_label(&original_speaker, expected)
                        })
                    }
                })
                .unwrap_or_else(|| {
                    let speaker = format!("Falante {}", speakers.len() + 1);
                    local_to_global.insert(original_speaker.clone(), speaker.clone());
                    speaker
                });
            segment.speaker = canonical.clone();

            push_speaker_once(&mut speakers, &canonical);
            if let Some(last) = segments.last_mut() {
                if last.speaker == segment.speaker && segment.start <= last.end + overlap_sec {
                    last.end = last.end.max(segment.end);
                    let text = segment.text.trim();
                    if !text.is_empty() && !last.text.contains(text) {
                        if !last.text.is_empty() {
                            last.text.push(' ');
                        }
                        last.text.push_str(text);
                    }
                    continue;
                }
            }

            segments.push(segment);
        }

        for centroid in &chunk.speaker_centroids {
            if let Some(global_speaker) = local_to_global.get(&centroid.speaker) {
                upsert_global_centroid(&mut global_centroids, global_speaker, &centroid.embedding);
            }
        }
    }

    if speakers.is_empty() {
        speakers.push("Falante 1".to_string());
    }

    DiarizedResult { speakers, segments }
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
mod tests {
    use super::*;
    use crate::models::transcription::TranscriptionSegment;

    #[test]
    fn local_diarization_merges_continuations_and_alternates_after_question() {
        let segments = vec![
            TranscriptionSegment {
                id: 0,
                start: 0.0,
                end: 4.0,
                text: "Bom dia pessoal.".to_string(),
            },
            TranscriptionSegment {
                id: 1,
                start: 4.2,
                end: 8.0,
                text: "Hoje vamos revisar o projeto.".to_string(),
            },
            TranscriptionSegment {
                id: 2,
                start: 10.0,
                end: 12.0,
                text: "Podem validar o prazo?".to_string(),
            },
            TranscriptionSegment {
                id: 3,
                start: 12.2,
                end: 15.0,
                text: "Sim, ate sexta.".to_string(),
            },
        ];

        let result = diarize_transcription_locally(&segments);

        assert_eq!(result.speakers, vec!["Falante 1", "Falante 2"]);
        assert_eq!(result.segments.len(), 3);
        assert_eq!(result.segments[0].speaker, "Falante 1");
        assert_eq!(
            result.segments[0].text,
            "Bom dia pessoal. Hoje vamos revisar o projeto."
        );
        assert_eq!(result.segments[1].speaker, "Falante 1");
        assert_eq!(result.segments[1].text, "Podem validar o prazo?");
        assert_eq!(result.segments[2].speaker, "Falante 2");
        assert_eq!(result.segments[2].text, "Sim, ate sexta.");
    }

    #[test]
    fn local_diarization_uses_inline_speaker_labels_when_present() {
        let segments = vec![
            TranscriptionSegment {
                id: 0,
                start: 0.0,
                end: 2.0,
                text: "Ana: vamos comecar pela pauta.".to_string(),
            },
            TranscriptionSegment {
                id: 1,
                start: 2.5,
                end: 4.0,
                text: "Bruno - combinado, eu atualizo o cronograma.".to_string(),
            },
            TranscriptionSegment {
                id: 2,
                start: 5.0,
                end: 7.0,
                text: "Ana: obrigado.".to_string(),
            },
        ];

        let result = diarize_transcription_locally(&segments);

        assert_eq!(result.speakers, vec!["Ana", "Bruno"]);
        assert_eq!(result.segments.len(), 3);
        assert_eq!(result.segments[0].speaker, "Ana");
        assert_eq!(result.segments[0].text, "vamos comecar pela pauta.");
        assert_eq!(result.segments[1].speaker, "Bruno");
        assert_eq!(
            result.segments[1].text,
            "combinado, eu atualizo o cronograma."
        );
        assert_eq!(result.segments[2].speaker, "Ana");
        assert_eq!(result.segments[2].text, "obrigado.");
    }

    #[test]
    fn audio_turns_are_aligned_to_transcript_by_largest_overlap() {
        let segments = vec![
            TranscriptionSegment {
                id: 0,
                start: 0.0,
                end: 4.0,
                text: "Abrimos a reuniao.".to_string(),
            },
            TranscriptionSegment {
                id: 1,
                start: 4.0,
                end: 8.0,
                text: "Revisamos o cronograma.".to_string(),
            },
            TranscriptionSegment {
                id: 2,
                start: 8.0,
                end: 12.0,
                text: "Eu envio ate sexta.".to_string(),
            },
        ];
        let turns = vec![
            SpeakerTurn {
                start: 0.0,
                end: 7.5,
                speaker_index: 0,
            },
            SpeakerTurn {
                start: 7.5,
                end: 12.5,
                speaker_index: 1,
            },
        ];

        let result = diarize_segments_with_speaker_turns(&segments, &turns);

        assert_eq!(result.speakers, vec!["Falante 1", "Falante 2"]);
        assert_eq!(result.segments.len(), 2);
        assert_eq!(result.segments[0].speaker, "Falante 1");
        assert_eq!(
            result.segments[0].text,
            "Abrimos a reuniao. Revisamos o cronograma."
        );
        assert_eq!(result.segments[1].speaker, "Falante 2");
        assert_eq!(result.segments[1].text, "Eu envio ate sexta.");
    }

    #[test]
    fn audio_turn_fallback_uses_nearest_speaker_when_segment_has_no_overlap() {
        let segments = vec![TranscriptionSegment {
            id: 0,
            start: 12.0,
            end: 13.0,
            text: "fala entre turnos".to_string(),
        }];
        let turns = vec![
            SpeakerTurn {
                start: 10.0,
                end: 11.0,
                speaker_index: 0,
            },
            SpeakerTurn {
                start: 30.0,
                end: 31.0,
                speaker_index: 1,
            },
        ];

        let result = diarize_segments_with_speaker_turns(&segments, &turns);

        assert_eq!(result.segments[0].speaker, "Falante 1");
    }

    #[test]
    fn modern_cpu_turns_align_to_transcript_text() {
        let modern_result = DiarizedResult {
            speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
            segments: vec![
                DiarizedSegment {
                    speaker: "Falante 1".to_string(),
                    start: 0.0,
                    end: 5.0,
                    text: String::new(),
                },
                DiarizedSegment {
                    speaker: "Falante 2".to_string(),
                    start: 5.0,
                    end: 10.0,
                    text: String::new(),
                },
            ],
        };
        let transcript = vec![
            TranscriptionSegment {
                id: 0,
                start: 1.0,
                end: 3.0,
                text: "A primeira fala tem texto.".to_string(),
            },
            TranscriptionSegment {
                id: 1,
                start: 6.0,
                end: 9.0,
                text: "A segunda fala tambem.".to_string(),
            },
        ];

        let result = align_modern_cpu_diarization_to_transcript(modern_result, &transcript);

        assert_eq!(result.speakers, vec!["Falante 1", "Falante 2"]);
        assert_eq!(result.segments.len(), 2);
        assert_eq!(result.segments[0].text, "A primeira fala tem texto.");
        assert_eq!(result.segments[1].speaker, "Falante 2");
        assert_eq!(result.segments[1].text, "A segunda fala tambem.");
    }

    #[test]
    fn speaker_turns_are_json_serializable_for_speculative_pipeline() {
        let turns = vec![SpeakerTurn {
            start: 1.0,
            end: 3.5,
            speaker_index: 2,
        }];

        let json = serde_json::to_string(&turns).unwrap();

        assert_eq!(json, r#"[{"start":1.0,"end":3.5,"speakerIndex":2}]"#);
    }

    #[test]
    fn serialized_speaker_turns_align_to_transcript_text() {
        let segments_json = serde_json::to_string(&vec![
            TranscriptionSegment {
                id: 1,
                start: 0.0,
                end: 2.0,
                text: "primeira fala".to_string(),
            },
            TranscriptionSegment {
                id: 2,
                start: 2.1,
                end: 4.0,
                text: "segunda fala".to_string(),
            },
        ])
        .unwrap();
        let turns_json =
            r#"[{"start":0.0,"end":2.2,"speakerIndex":0},{"start":2.2,"end":4.5,"speakerIndex":1}]"#
                .to_string();

        let result = align_speaker_turns_to_transcription(segments_json, turns_json).unwrap();

        assert_eq!(result.speakers, vec!["Falante 1", "Falante 2"]);
        assert_eq!(result.segments[0].speaker, "Falante 1");
        assert_eq!(result.segments[1].speaker, "Falante 2");
    }

    #[test]
    fn selective_refinement_selects_chunk_with_weak_turn_coverage() {
        let chunks = vec![
            ExportedChunk {
                index: 0,
                audio_path: "chunk_000.wav".to_string(),
                start_sec: 0.0,
                end_sec: 100.0,
                offset_sec: 0.0,
                duration_sec: 100.0,
            },
            ExportedChunk {
                index: 1,
                audio_path: "chunk_001.wav".to_string(),
                start_sec: 100.0,
                end_sec: 200.0,
                offset_sec: 100.0,
                duration_sec: 100.0,
            },
        ];
        let segments = vec![
            TranscriptionSegment {
                id: 0,
                start: 10.0,
                end: 15.0,
                text: "coberto".to_string(),
            },
            TranscriptionSegment {
                id: 1,
                start: 120.0,
                end: 130.0,
                text: "sem cobertura".to_string(),
            },
        ];
        let turns = vec![SpeakerTurn {
            start: 0.0,
            end: 20.0,
            speaker_index: 0,
        }];

        let selected = select_suspicious_chunks_for_refinement(&segments, &turns, &chunks, 1);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].index, 1);
    }

    #[test]
    fn selective_refinement_windows_are_short_and_padded() {
        let chunks = vec![ExportedChunk {
            index: 1,
            audio_path: "chunk_001.wav".to_string(),
            start_sec: 100.0,
            end_sec: 200.0,
            offset_sec: 100.0,
            duration_sec: 100.0,
        }];
        let segments = vec![TranscriptionSegment {
            id: 1,
            start: 120.0,
            end: 130.0,
            text: "sem cobertura".to_string(),
        }];
        let turns = vec![SpeakerTurn {
            start: 0.0,
            end: 20.0,
            speaker_index: 0,
        }];

        let windows = select_suspicious_refinement_windows(&segments, &turns, &chunks, 1);

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].chunk.index, 1);
        assert_eq!(windows[0].start_sec, 116.0);
        assert_eq!(windows[0].end_sec, 134.0);
        assert!(windows[0].duration_sec() <= 45.0);
    }

    #[test]
    fn selective_refinement_selects_chunk_with_excessive_speaker_switches() {
        let chunks = vec![ExportedChunk {
            index: 0,
            audio_path: "chunk_000.wav".to_string(),
            start_sec: 0.0,
            end_sec: 80.0,
            offset_sec: 0.0,
            duration_sec: 80.0,
        }];
        let segments = (0..8)
            .map(|index| TranscriptionSegment {
                id: index,
                start: index as f64 * 5.0,
                end: index as f64 * 5.0 + 4.0,
                text: format!("fala {index}"),
            })
            .collect::<Vec<_>>();
        let turns = (0..8)
            .map(|index| SpeakerTurn {
                start: index as f64 * 5.0,
                end: index as f64 * 5.0 + 4.0,
                speaker_index: (index % 2) as i32,
            })
            .collect::<Vec<_>>();

        let windows = select_suspicious_refinement_windows(&segments, &turns, &chunks, 1);

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].chunk.index, 0);
    }

    #[test]
    fn selective_refinement_selects_single_speaker_chunk_when_multiple_expected() {
        let chunks = vec![ExportedChunk {
            index: 0,
            audio_path: "chunk_000.wav".to_string(),
            start_sec: 0.0,
            end_sec: 120.0,
            offset_sec: 0.0,
            duration_sec: 120.0,
        }];
        let segments = vec![
            TranscriptionSegment {
                id: 0,
                start: 10.0,
                end: 20.0,
                text: "primeira fala".to_string(),
            },
            TranscriptionSegment {
                id: 1,
                start: 40.0,
                end: 50.0,
                text: "segunda fala".to_string(),
            },
            TranscriptionSegment {
                id: 2,
                start: 80.0,
                end: 90.0,
                text: "terceira fala".to_string(),
            },
        ];
        let turns = vec![SpeakerTurn {
            start: 0.0,
            end: 120.0,
            speaker_index: 0,
        }];

        let windows = select_suspicious_refinement_windows_with_context(
            &segments,
            &turns,
            &chunks,
            1,
            Some(3),
        );

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].chunk.index, 0);
    }

    #[test]
    fn selective_refinement_replaces_only_selected_chunk_segments() {
        let base = DiarizedResult {
            speakers: vec!["Falante 1".to_string()],
            segments: vec![
                DiarizedSegment {
                    speaker: "Falante 1".to_string(),
                    start: 0.0,
                    end: 50.0,
                    text: "fora".to_string(),
                },
                DiarizedSegment {
                    speaker: "Falante 1".to_string(),
                    start: 120.0,
                    end: 130.0,
                    text: "trocar".to_string(),
                },
            ],
        };
        let refined = DiarizedResult {
            speakers: vec!["Falante 2".to_string()],
            segments: vec![DiarizedSegment {
                speaker: "Falante 2".to_string(),
                start: 120.0,
                end: 130.0,
                text: "trocar".to_string(),
            }],
        };
        let chunks = vec![ExportedChunk {
            index: 1,
            audio_path: "chunk_001.wav".to_string(),
            start_sec: 100.0,
            end_sec: 200.0,
            offset_sec: 100.0,
            duration_sec: 100.0,
        }];

        let merged = merge_selective_refinement(base, refined, &chunks);

        assert_eq!(merged.segments.len(), 2);
        assert_eq!(merged.segments[0].speaker, "Falante 1");
        assert_eq!(merged.segments[1].speaker, "Falante 2");
        assert_eq!(merged.speakers, vec!["Falante 1", "Falante 2"]);
    }

    #[test]
    fn diarization_asset_paths_point_to_expected_local_model_files() {
        let base_dir = std::path::Path::new("C:/app-data");
        let paths = diarization_asset_paths(base_dir);

        assert!(paths
            .segmentation_model
            .ends_with("models/diarization/sherpa-onnx-pyannote-segmentation-3-0-model.int8.onnx"));
        assert!(paths.embedding_model.ends_with(
            "models/diarization/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx"
        ));
    }

    #[test]
    fn modern_cpu_backend_paths_point_to_project_tools() {
        let root = std::path::Path::new("C:/project");
        let paths = modern_cpu_backend_paths(root);

        assert!(paths
            .python_exe
            .ends_with(".venv-diarize/Scripts/python.exe"));
        assert!(paths
            .script_path
            .ends_with("scripts/diarize_cpu_backend.py"));
    }

    #[test]
    fn pyannote_backend_paths_point_to_project_tools() {
        let root = std::path::Path::new("C:/project");
        let paths = pyannote_backend_paths(root);

        assert!(paths
            .python_exe
            .ends_with(".venv-pyannote/Scripts/python.exe"));
        assert!(paths
            .script_path
            .ends_with("scripts/pyannote_community_backend.py"));
    }

    #[test]
    fn diarization_mode_parses_supported_values() {
        assert_eq!(
            DiarizationMode::from_option(Some("fast".to_string())),
            DiarizationMode::Fast
        );
        assert_eq!(
            DiarizationMode::from_option(Some("hybrid".to_string())),
            DiarizationMode::Hybrid
        );
        assert_eq!(
            DiarizationMode::from_option(Some("modern-cpu".to_string())),
            DiarizationMode::ModernCpu
        );
        assert_eq!(
            DiarizationMode::from_option(Some("modern-cpu-chunked".to_string())),
            DiarizationMode::ModernCpuChunked
        );
        assert_eq!(
            DiarizationMode::from_option(Some("diarize-cpu".to_string())),
            DiarizationMode::ModernCpu
        );
        assert_eq!(
            DiarizationMode::from_option(Some("precise".to_string())),
            DiarizationMode::Precise
        );
        assert_eq!(
            DiarizationMode::from_option(Some("pyannote".to_string())),
            DiarizationMode::Pyannote
        );
        assert_eq!(
            DiarizationMode::from_option(Some("auto".to_string())),
            DiarizationMode::Auto
        );
        assert_eq!(
            DiarizationMode::from_option(Some("bad".to_string())),
            DiarizationMode::Auto
        );
        assert_eq!(DiarizationMode::from_option(None), DiarizationMode::Auto);
    }

    #[test]
    fn diarization_thread_count_is_bounded() {
        assert_eq!(normalize_diarization_threads(Some(0), 20), 2);
        assert_eq!(normalize_diarization_threads(Some(12), 20), 12);
        assert_eq!(normalize_diarization_threads(Some(64), 20), 16);
        assert_eq!(normalize_diarization_threads(None, 20), 8);
    }

    #[test]
    fn explicit_diarization_modes_do_not_allow_local_fallback() {
        assert!(mode_allows_local_fallback(DiarizationMode::Auto));
        assert!(!mode_allows_local_fallback(DiarizationMode::Fast));
        assert!(!mode_allows_local_fallback(DiarizationMode::Hybrid));
        assert!(!mode_allows_local_fallback(DiarizationMode::ModernCpu));
        assert!(!mode_allows_local_fallback(
            DiarizationMode::ModernCpuChunked
        ));
        assert!(!mode_allows_local_fallback(DiarizationMode::Precise));
        assert!(!mode_allows_local_fallback(DiarizationMode::Pyannote));
    }

    #[test]
    fn diarization_telemetry_records_backend_and_counts() {
        let result = DiarizedResult {
            speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
            segments: vec![DiarizedSegment {
                speaker: "Falante 1".to_string(),
                start: 0.0,
                end: 1.0,
                text: "ola".to_string(),
            }],
        };

        let telemetry = build_diarization_telemetry(
            DiarizationMode::Precise,
            DiarizationBackend::SherpaPrecise,
            Some("modern CPU backend unavailable".to_string()),
            12.5,
            &result,
        );
        let json = serde_json::to_value(&telemetry).unwrap();

        assert_eq!(json["requestedMode"], "precise");
        assert_eq!(json["backendUsed"], "sherpa-precise");
        assert_eq!(json["fallbackReason"], "modern CPU backend unavailable");
        assert_eq!(json["wallClockSec"], 12.5);
        assert_eq!(json["speakerCount"], 2);
        assert_eq!(json["segmentCount"], 1);
    }

    #[test]
    fn shifted_segments_for_chunk_use_chunk_relative_time() {
        let chunk = ExportedChunk {
            index: 1,
            audio_path: "chunk.flac".to_string(),
            start_sec: 100.0,
            end_sec: 200.0,
            offset_sec: 100.0,
            duration_sec: 100.0,
        };
        let segments = vec![
            TranscriptionSegment {
                id: 1,
                start: 110.0,
                end: 115.0,
                text: "hello".to_string(),
            },
            TranscriptionSegment {
                id: 2,
                start: 220.0,
                end: 225.0,
                text: "outside".to_string(),
            },
        ];

        let shifted = segments_for_chunk(&segments, &chunk);

        assert_eq!(shifted.len(), 1);
        assert_eq!(shifted[0].start, 10.0);
        assert_eq!(shifted[0].end, 15.0);
    }

    #[test]
    fn stitch_chunk_results_maps_overlap_speakers() {
        let first = DiarizedResult {
            speakers: vec!["Falante 1".to_string()],
            segments: vec![DiarizedSegment {
                speaker: "Falante 1".to_string(),
                start: 0.0,
                end: 10.0,
                text: "same overlap".to_string(),
            }],
        };
        let second = DiarizedResult {
            speakers: vec!["Falante 1".to_string()],
            segments: vec![DiarizedSegment {
                speaker: "Falante 1".to_string(),
                start: 8.0,
                end: 18.0,
                text: "same overlap continues".to_string(),
            }],
        };

        let stitched = stitch_diarized_chunk_results(vec![first, second], 3.0);

        assert_eq!(stitched.speakers, vec!["Falante 1"]);
        assert_eq!(stitched.segments.len(), 1);
        assert_eq!(stitched.segments[0].speaker, "Falante 1");
        assert_eq!(stitched.segments[0].start, 0.0);
        assert_eq!(stitched.segments[0].end, 18.0);
    }

    #[test]
    fn stitch_chunk_results_maps_different_local_labels_by_overlap() {
        let first = DiarizedResult {
            speakers: vec!["Falante 1".to_string()],
            segments: vec![DiarizedSegment {
                speaker: "Falante 1".to_string(),
                start: 0.0,
                end: 10.0,
                text: "overlap speaker".to_string(),
            }],
        };
        let second = DiarizedResult {
            speakers: vec!["Falante 2".to_string()],
            segments: vec![DiarizedSegment {
                speaker: "Falante 2".to_string(),
                start: 8.0,
                end: 18.0,
                text: "overlap speaker continues".to_string(),
            }],
        };

        let stitched = stitch_diarized_chunk_results(vec![first, second], 3.0);

        assert_eq!(stitched.speakers, vec!["Falante 1"]);
        assert_eq!(stitched.segments.len(), 1);
        assert_eq!(stitched.segments[0].speaker, "Falante 1");
        assert_eq!(stitched.segments[0].end, 18.0);
    }

    #[test]
    fn stitch_chunk_results_does_not_collapse_new_local_speakers_near_overlap() {
        let first = DiarizedResult {
            speakers: vec!["Falante 1".to_string()],
            segments: vec![DiarizedSegment {
                speaker: "Falante 1".to_string(),
                start: 0.0,
                end: 10.0,
                text: "overlap speaker".to_string(),
            }],
        };
        let second = DiarizedResult {
            speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
            segments: vec![
                DiarizedSegment {
                    speaker: "Falante 1".to_string(),
                    start: 8.0,
                    end: 11.0,
                    text: "overlap speaker continues".to_string(),
                },
                DiarizedSegment {
                    speaker: "Falante 2".to_string(),
                    start: 11.2,
                    end: 18.0,
                    text: "new speaker near overlap".to_string(),
                },
            ],
        };

        let stitched = stitch_diarized_chunk_results(vec![first, second], 3.0);

        assert_eq!(stitched.speakers, vec!["Falante 1", "Falante 2"]);
        assert_eq!(stitched.segments.len(), 2);
        assert_eq!(stitched.segments[0].speaker, "Falante 1");
        assert_eq!(stitched.segments[1].speaker, "Falante 2");
    }

    #[test]
    fn expected_speaker_stitch_reuses_local_label_ids_across_chunks() {
        let first = DiarizedResult {
            speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
            segments: vec![
                DiarizedSegment {
                    speaker: "Falante 1".to_string(),
                    start: 0.0,
                    end: 5.0,
                    text: "primeiro falante".to_string(),
                },
                DiarizedSegment {
                    speaker: "Falante 2".to_string(),
                    start: 5.0,
                    end: 10.0,
                    text: "segundo falante".to_string(),
                },
            ],
        };
        let second = DiarizedResult {
            speakers: vec!["Falante 1".to_string(), "Falante 2".to_string()],
            segments: vec![
                DiarizedSegment {
                    speaker: "Falante 1".to_string(),
                    start: 100.0,
                    end: 105.0,
                    text: "primeiro falante no chunk dois".to_string(),
                },
                DiarizedSegment {
                    speaker: "Falante 2".to_string(),
                    start: 105.0,
                    end: 110.0,
                    text: "segundo falante no chunk dois".to_string(),
                },
            ],
        };

        let stitched =
            stitch_diarized_chunk_results_with_expected_speakers(vec![first, second], 3.0, Some(2));

        assert_eq!(stitched.speakers, vec!["Falante 1", "Falante 2"]);
        assert_eq!(stitched.segments.len(), 4);
        assert_eq!(stitched.segments[2].speaker, "Falante 1");
        assert_eq!(stitched.segments[3].speaker, "Falante 2");
    }

    #[test]
    fn modern_cpu_batch_centroids_map_local_labels_across_chunks() {
        let raw = r#"[
            {
                "index": 0,
                "offsetSec": 0.0,
                "diarized": {
                    "speakers": ["Falante 1"],
                    "segments": [{"speaker": "Falante 1", "start": 0.0, "end": 5.0, "text": ""}]
                },
                "speakerCentroids": [
                    {"speaker": "Falante 1", "embedding": [1.0, 0.0]}
                ]
            },
            {
                "index": 1,
                "offsetSec": 100.0,
                "diarized": {
                    "speakers": ["Falante 2"],
                    "segments": [{"speaker": "Falante 2", "start": 1.0, "end": 6.0, "text": ""}]
                },
                "speakerCentroids": [
                    {"speaker": "Falante 2", "embedding": [0.98, 0.02]}
                ]
            }
        ]"#;

        let stitched = stitch_modern_cpu_batch_outputs(raw, Some(2)).unwrap();

        assert_eq!(stitched.speakers, vec!["Falante 1"]);
        assert_eq!(stitched.segments.len(), 2);
        assert_eq!(stitched.segments[1].speaker, "Falante 1");
        assert_eq!(stitched.segments[1].start, 101.0);
    }

    #[test]
    fn modern_cpu_batch_centroids_respect_expected_speaker_cap() {
        let raw = r#"[
            {
                "index": 0,
                "offsetSec": 0.0,
                "diarized": {
                    "speakers": ["Falante 1"],
                    "segments": [{"speaker": "Falante 1", "start": 0.0, "end": 5.0, "text": ""}]
                },
                "speakerCentroids": [
                    {"speaker": "Falante 1", "embedding": [1.0, 0.0]}
                ]
            },
            {
                "index": 1,
                "offsetSec": 100.0,
                "diarized": {
                    "speakers": ["Falante 2"],
                    "segments": [{"speaker": "Falante 2", "start": 1.0, "end": 6.0, "text": ""}]
                },
                "speakerCentroids": [
                    {"speaker": "Falante 2", "embedding": [0.5, 0.5]}
                ]
            }
        ]"#;

        let stitched = stitch_modern_cpu_batch_outputs(raw, Some(1)).unwrap();

        assert_eq!(stitched.speakers, vec!["Falante 1"]);
        assert_eq!(stitched.segments[1].speaker, "Falante 1");
    }

    #[test]
    fn modern_cpu_batch_outputs_are_shifted_and_stitched() {
        let raw = r#"[
            {
                "index": 0,
                "offsetSec": 0.0,
                "diarized": {
                    "speakers": ["Falante 1"],
                    "segments": [{"speaker": "Falante 1", "start": 0.0, "end": 5.0, "text": ""}]
                }
            },
            {
                "index": 1,
                "offsetSec": 100.0,
                "diarized": {
                    "speakers": ["Falante 2"],
                    "segments": [{"speaker": "Falante 2", "start": 1.0, "end": 6.0, "text": ""}]
                }
            }
        ]"#;

        let stitched = stitch_modern_cpu_batch_outputs(raw, Some(2)).unwrap();

        assert_eq!(stitched.speakers, vec!["Falante 1", "Falante 2"]);
        assert_eq!(stitched.segments[1].speaker, "Falante 2");
        assert_eq!(stitched.segments[1].start, 101.0);
        assert_eq!(stitched.segments[1].end, 106.0);
    }

    #[test]
    fn hybrid_sherpa_requires_wav_chunks() {
        let wav_chunk = ExportedChunk {
            index: 0,
            audio_path: "chunk.wav".to_string(),
            start_sec: 0.0,
            end_sec: 10.0,
            offset_sec: 0.0,
            duration_sec: 10.0,
        };
        let flac_chunk = ExportedChunk {
            audio_path: "chunk.flac".to_string(),
            ..wav_chunk.clone()
        };

        assert!(chunks_can_use_sherpa(&[wav_chunk]));
        assert!(!chunks_can_use_sherpa(&[flac_chunk]));
        assert!(!chunks_can_use_sherpa(&[]));
    }
}
