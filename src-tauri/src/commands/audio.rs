use crate::models::audio::{
    ChunkPlan, ExportedChunk, PreparedAudio, SilenceRange, SmartChunkOptions,
};
use crate::models::meeting::MeetingMetadata;
use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use tauri::command;
use tauri_plugin_shell::ShellExt;

fn command_output_error(context: &str, output: &tauri_plugin_shell::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

    let details = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("process exited with status {:?}", output.status.code())
    };

    format!("{}: {}", context, details)
}

pub fn parse_silencedetect(stderr: &str) -> Vec<SilenceRange> {
    let mut ranges = Vec::new();
    let mut current_start: Option<f64> = None;

    for line in stderr.lines() {
        if let Some(raw) = line.split("silence_start:").nth(1) {
            current_start = raw
                .trim()
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<f64>().ok());
        }

        if let Some(raw) = line.split("silence_end:").nth(1) {
            if let Some(start_sec) = current_start {
                if let Some(end_sec) = raw
                    .trim()
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse::<f64>().ok())
                {
                    if end_sec > start_sec {
                        ranges.push(SilenceRange { start_sec, end_sec });
                        current_start = None;
                    }
                }
            }
        }
    }

    ranges
}

fn midpoint(range: &SilenceRange) -> f64 {
    (range.start_sec + range.end_sec) / 2.0
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SilenceCache {
    noise_db: f64,
    min_duration_sec: f64,
    ranges: Vec<SilenceRange>,
}

fn silence_cache_path(audio_path: &str) -> PathBuf {
    Path::new(audio_path).with_extension("silences.json")
}

fn silence_cache_matches(cache: &SilenceCache, noise_db: f64, min_duration_sec: f64) -> bool {
    (cache.noise_db - noise_db).abs() <= f64::EPSILON
        && (cache.min_duration_sec - min_duration_sec).abs() <= f64::EPSILON
}

async fn write_silence_cache(
    audio_path: &str,
    noise_db: f64,
    min_duration_sec: f64,
    ranges: Vec<SilenceRange>,
) {
    let cache = SilenceCache {
        noise_db,
        min_duration_sec,
        ranges,
    };
    let Ok(raw) = serde_json::to_string(&cache) else {
        return;
    };
    let _ = tokio::fs::write(silence_cache_path(audio_path), raw).await;
}

async fn read_silence_cache(
    audio_path: &str,
    noise_db: f64,
    min_duration_sec: f64,
) -> Option<Vec<SilenceRange>> {
    let raw = tokio::fs::read_to_string(silence_cache_path(audio_path))
        .await
        .ok()?;
    let cache = serde_json::from_str::<SilenceCache>(&raw).ok()?;
    if silence_cache_matches(&cache, noise_db, min_duration_sec) {
        Some(cache.ranges)
    } else {
        None
    }
}

fn find_cut_point(desired_end: f64, min_end: f64, max_end: f64, silences: &[SilenceRange]) -> f64 {
    let first_candidate = silences.partition_point(|range| midpoint(range) < min_end);
    let mut best = None;

    for range in &silences[first_candidate..] {
        let point = midpoint(range);
        if point > max_end {
            break;
        }

        let distance = (point - desired_end).abs();
        if best
            .map(|(_, best_distance)| distance < best_distance)
            .unwrap_or(true)
        {
            best = Some((point, distance));
        }
    }

    best.map(|(point, _)| point)
        .unwrap_or(desired_end.min(max_end))
}

struct NormalizedChunkOptions {
    target_sec: f64,
    min_sec: f64,
    max_sec: f64,
    overlap_sec: f64,
}

fn positive_finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

fn normalize_chunk_options(
    target_sec: f64,
    min_sec: f64,
    max_sec: f64,
    overlap_sec: f64,
) -> NormalizedChunkOptions {
    let defaults = SmartChunkOptions::default();
    let mut ordered = [
        positive_finite_or(min_sec, defaults.min_sec),
        positive_finite_or(target_sec, defaults.target_sec),
        positive_finite_or(max_sec, defaults.max_sec),
    ];

    ordered.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let min_sec = ordered[0];
    let target_sec = ordered[1];
    let max_sec = ordered[2];
    let mut overlap_sec = if overlap_sec.is_finite() && overlap_sec >= 0.0 {
        overlap_sec
    } else {
        defaults.overlap_sec
    };

    if overlap_sec >= min_sec {
        overlap_sec = defaults.overlap_sec.min(min_sec / 2.0);
    }

    NormalizedChunkOptions {
        target_sec,
        min_sec,
        max_sec,
        overlap_sec,
    }
}

pub fn plan_smart_chunks(
    duration_sec: f64,
    silences: &[SilenceRange],
    target_sec: f64,
    min_sec: f64,
    max_sec: f64,
    overlap_sec: f64,
) -> Vec<ChunkPlan> {
    if !duration_sec.is_finite() || duration_sec <= 0.0 {
        return Vec::new();
    }

    let options = normalize_chunk_options(target_sec, min_sec, max_sec, overlap_sec);
    let mut chunks = Vec::new();
    let mut start_sec = 0.0;
    let mut index = 0usize;

    while start_sec < duration_sec {
        let remaining = duration_sec - start_sec;
        let mut end_sec = if remaining <= options.max_sec {
            duration_sec
        } else {
            let desired_end = (start_sec + options.target_sec).min(duration_sec);
            let min_end = (start_sec + options.min_sec).min(duration_sec);
            let max_end = (start_sec + options.max_sec).min(duration_sec);
            find_cut_point(desired_end, min_end, max_end, silences)
        };

        if !end_sec.is_finite() || end_sec <= start_sec {
            end_sec = (start_sec + options.target_sec).min(duration_sec);
        }

        if end_sec <= start_sec {
            break;
        }

        chunks.push(ChunkPlan {
            index,
            start_sec,
            end_sec,
            offset_sec: start_sec,
        });

        if end_sec >= duration_sec {
            break;
        }

        let next_start = (end_sec - options.overlap_sec).max(0.0);
        start_sec = if next_start > start_sec {
            next_start
        } else {
            end_sec
        };
        index += 1;
    }

    chunks
}

pub fn audio_codec_args(format: &str) -> Vec<String> {
    match format {
        "wav" => vec!["-c:a".to_string(), "pcm_s16le".to_string()],
        "mp3" => vec![
            "-acodec".to_string(),
            "libmp3lame".to_string(),
            "-ab".to_string(),
            "128k".to_string(),
        ],
        _ => vec!["-c:a".to_string(), "flac".to_string()],
    }
}

pub fn validate_chunk_output_format(format: &str) -> Result<String, String> {
    let normalized = format.trim().trim_start_matches('.').to_ascii_lowercase();

    match normalized.as_str() {
        "" | "flac" => Ok("flac".to_string()),
        "wav" => Ok("wav".to_string()),
        "mp3" => Ok("mp3".to_string()),
        _ => Err(format!(
            "Formato de audio invalido para chunks: {}. Use flac, wav ou mp3.",
            format
        )),
    }
}

pub fn build_extract_audio_args(
    input_path: &Path,
    output_path: &Path,
    output_format: &str,
    audio_filter: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        "-i".to_string(),
        input_path.to_string_lossy().to_string(),
        "-vn".to_string(),
    ];
    if let Some(filter) = audio_filter.filter(|value| !value.trim().is_empty()) {
        args.extend(["-af".to_string(), filter.to_string()]);
    }
    args.extend([
        "-ar".to_string(),
        "16000".to_string(),
        "-ac".to_string(),
        "1".to_string(),
    ]);
    args.extend(audio_codec_args(output_format));
    args.extend(["-y".to_string(), output_path.to_string_lossy().to_string()]);
    args
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartChunkExportStrategy {
    SegmentMuxerCopy,
    SegmentMuxerEncode,
    ParallelPerChunk,
}

const MIN_SEGMENT_REENCODE_CHUNKS: usize = 16;

fn format_ffmpeg_seconds(value: f64) -> String {
    format!("{:.3}", value.max(0.0))
}

fn chunk_file_name(index: usize, output_format: &str) -> String {
    format!("chunk_{index:03}.{output_format}")
}

fn chunk_output_path(output_dir: &Path, index: usize, output_format: &str) -> String {
    output_dir
        .join(chunk_file_name(index, output_format))
        .to_string_lossy()
        .to_string()
}

fn exported_chunks_from_plans(
    output_dir: &Path,
    output_format: &str,
    plans: &[ChunkPlan],
) -> Vec<ExportedChunk> {
    plans
        .iter()
        .map(|plan| ExportedChunk {
            index: plan.index,
            audio_path: chunk_output_path(output_dir, plan.index, output_format),
            start_sec: plan.start_sec,
            end_sec: plan.end_sec,
            offset_sec: plan.offset_sec,
            duration_sec: plan.end_sec - plan.start_sec,
        })
        .collect()
}

fn input_format_matches_output(input_path: &Path, output_format: &str) -> bool {
    input_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case(output_format))
        .unwrap_or(false)
}

fn plans_are_contiguous_for_segment_muxer(plans: &[ChunkPlan]) -> bool {
    const MAX_BOUNDARY_DRIFT_SEC: f64 = 0.010;

    if plans.len() < 2 {
        return false;
    }

    if plans[0].start_sec.abs() > MAX_BOUNDARY_DRIFT_SEC {
        return false;
    }

    plans.iter().all(|plan| {
        plan.end_sec.is_finite() && plan.start_sec.is_finite() && plan.end_sec > plan.start_sec
    }) && plans.windows(2).all(|pair| {
        let previous = &pair[0];
        let next = &pair[1];
        (previous.end_sec - next.start_sec).abs() <= MAX_BOUNDARY_DRIFT_SEC
    })
}

pub fn select_smart_chunk_export_strategy(
    input_path: &Path,
    output_format: &str,
    plans: &[ChunkPlan],
) -> SmartChunkExportStrategy {
    if plans_are_contiguous_for_segment_muxer(plans) {
        if input_format_matches_output(input_path, output_format) {
            SmartChunkExportStrategy::SegmentMuxerCopy
        } else if plans.len() >= MIN_SEGMENT_REENCODE_CHUNKS {
            SmartChunkExportStrategy::SegmentMuxerEncode
        } else {
            SmartChunkExportStrategy::ParallelPerChunk
        }
    } else {
        SmartChunkExportStrategy::ParallelPerChunk
    }
}

fn segment_muxer_times(plans: &[ChunkPlan]) -> String {
    plans
        .iter()
        .take(plans.len().saturating_sub(1))
        .map(|plan| format_ffmpeg_seconds(plan.end_sec))
        .collect::<Vec<_>>()
        .join(",")
}

fn build_segment_muxer_export_args(
    input_path: &Path,
    output_dir: &Path,
    output_format: &str,
    plans: &[ChunkPlan],
    copy_audio: bool,
) -> Vec<String> {
    let mut args = vec![
        "-y".to_string(),
        "-i".to_string(),
        input_path.to_string_lossy().to_string(),
        "-map".to_string(),
        "0:a:0".to_string(),
        "-vn".to_string(),
        "-f".to_string(),
        "segment".to_string(),
        "-segment_times".to_string(),
        segment_muxer_times(plans),
        "-reset_timestamps".to_string(),
        "1".to_string(),
    ];

    if copy_audio {
        args.extend(["-c:a".to_string(), "copy".to_string()]);
    } else {
        args.extend([
            "-ar".to_string(),
            "16000".to_string(),
            "-ac".to_string(),
            "1".to_string(),
        ]);
        args.extend(audio_codec_args(output_format));
    }

    args.push(
        output_dir
            .join(format!("chunk_%03d.{output_format}"))
            .to_string_lossy()
            .to_string(),
    );
    args
}

pub fn build_smart_chunk_export_args(
    input_path: &Path,
    output_dir: &Path,
    output_format: &str,
    plans: &[ChunkPlan],
) -> Vec<String> {
    if plans.is_empty() {
        return Vec::new();
    }

    match select_smart_chunk_export_strategy(input_path, output_format, plans) {
        SmartChunkExportStrategy::SegmentMuxerCopy => {
            build_segment_muxer_export_args(input_path, output_dir, output_format, plans, true)
        }
        SmartChunkExportStrategy::SegmentMuxerEncode => {
            build_segment_muxer_export_args(input_path, output_dir, output_format, plans, false)
        }
        SmartChunkExportStrategy::ParallelPerChunk => Vec::new(),
    }
}

async fn run_extract_audio(
    app: tauri::AppHandle,
    input_path: String,
    output_path: String,
    audio_filter: Option<String>,
) -> Result<String, String> {
    let extension = Path::new(&output_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("flac")
        .to_ascii_lowercase();
    let args = build_extract_audio_args(
        Path::new(&input_path),
        Path::new(&output_path),
        &extension,
        audio_filter.as_deref(),
    );

    let output = app
        .shell()
        .sidecar("ffmpeg")
        .map_err(|e| e.to_string())?
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if output.status.code() != Some(0) {
        return Err(command_output_error(
            "Falha ao extrair audio com ffmpeg",
            &output,
        ));
    }

    Ok(String::from_utf8_lossy(&output.stderr).to_string())
}

fn parse_hms_duration(value: &str) -> Option<f64> {
    let parts = value.trim().split(':').collect::<Vec<_>>();
    if parts.len() != 3 {
        return None;
    }

    let h = parts[0].parse::<f64>().ok()?;
    let m = parts[1].parse::<f64>().ok()?;
    let s = parts[2].parse::<f64>().ok()?;
    let duration_sec = h * 3600.0 + m * 60.0 + s;
    if duration_sec.is_finite() && duration_sec > 0.0 {
        Some(duration_sec)
    } else {
        None
    }
}

pub fn parse_duration_from_ffmpeg_stderr(stderr: &str) -> Option<f64> {
    for line in stderr.lines() {
        if let Some(raw_duration) = line.split("Duration:").nth(1) {
            let duration_part = raw_duration.split(',').next().unwrap_or("").trim();
            if let Some(duration) = parse_hms_duration(duration_part) {
                return Some(duration);
            }
        }
    }

    for line in stderr.lines().rev() {
        if line.contains("time=") {
            if let Some(time_str) = line.split("time=").nth(1) {
                let time_part = time_str.split_whitespace().next().unwrap_or("0");
                if let Some(duration) = parse_hms_duration(time_part) {
                    return Some(duration);
                }
            }
        }
    }

    None
}

pub fn parse_duration_from_ffprobe_stdout(stdout: &str) -> Option<f64> {
    stdout
        .lines()
        .find_map(|line| line.trim().parse::<f64>().ok())
        .filter(|duration| duration.is_finite() && *duration > 0.0)
}

fn system_time_to_rfc3339(value: SystemTime) -> String {
    let datetime: DateTime<Utc> = value.into();
    datetime.to_rfc3339()
}

fn normalize_metadata_datetime(value: &str) -> Option<String> {
    let mut cleaned = value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string();
    if cleaned.is_empty() {
        return None;
    }

    if cleaned.ends_with(" UTC") {
        cleaned.truncate(cleaned.len() - 4);
        cleaned.push('Z');
    }

    if cleaned.len() >= 5 {
        let offset_start = cleaned.len() - 5;
        let offset = &cleaned[offset_start..];
        let mut chars = offset.chars();
        let sign = chars.next();
        if matches!(sign, Some('+') | Some('-')) && chars.all(|ch| ch.is_ascii_digit()) {
            cleaned.insert(cleaned.len() - 2, ':');
        }
    }

    if let Ok(datetime) = DateTime::parse_from_rfc3339(&cleaned) {
        return Some(datetime.to_rfc3339());
    }

    for format in [
        "%Y-%m-%d %H:%M:%S %z",
        "%Y-%m-%dT%H:%M:%S%.f%z",
        "%Y-%m-%dT%H:%M:%S%z",
    ] {
        if let Ok(datetime) = DateTime::parse_from_str(&cleaned, format) {
            return Some(datetime.to_rfc3339());
        }
    }

    Some(cleaned)
}

fn ffmpeg_metadata_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.trim().split_once(':')?;
    let key = key.trim();
    let value = value.trim();
    if key.is_empty() || value.is_empty() {
        return None;
    }
    Some((key, value))
}

pub fn parse_ffmpeg_media_metadata(stderr: &str) -> MeetingMetadata {
    let mut metadata = MeetingMetadata::default();

    for line in stderr.lines() {
        let Some((key, value)) = ffmpeg_metadata_value(line) else {
            continue;
        };
        let normalized_key = key.to_ascii_lowercase();
        if metadata.embedded_created_at.is_none()
            && (normalized_key == "creation_time"
                || normalized_key.ends_with(".creationdate")
                || normalized_key.ends_with(".creation_time"))
        {
            metadata.embedded_created_at = normalize_metadata_datetime(value);
            continue;
        }

        if metadata.source_title.is_none() && normalized_key == "title" {
            metadata.source_title = Some(value.trim().to_string());
        }
    }

    metadata
}

pub fn apply_recorded_at_source(metadata: &mut MeetingMetadata) {
    for (source, value) in [
        (
            "embedded_created_at",
            metadata.embedded_created_at.as_deref(),
        ),
        ("file_created_at", metadata.file_created_at.as_deref()),
        ("file_modified_at", metadata.file_modified_at.as_deref()),
    ] {
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            metadata.recorded_at = Some(value.to_string());
            metadata.recorded_at_source = Some(source.to_string());
            return;
        }
    }
}

fn media_metadata_from_filesystem(input_path: &str) -> MeetingMetadata {
    let path = Path::new(input_path);
    let mut metadata = MeetingMetadata {
        source_path: Some(input_path.to_string()),
        source_file_name: path
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value.to_string()),
        ..Default::default()
    };

    if let Ok(fs_metadata) = fs::metadata(path) {
        metadata.file_created_at = fs_metadata.created().ok().map(system_time_to_rfc3339);
        metadata.file_modified_at = fs_metadata.modified().ok().map(system_time_to_rfc3339);
    }

    apply_recorded_at_source(&mut metadata);
    metadata
}

#[command]
pub async fn probe_media_metadata(
    app: tauri::AppHandle,
    input_path: String,
) -> Result<MeetingMetadata, String> {
    let mut metadata = media_metadata_from_filesystem(&input_path);

    if let Ok(command) = app.shell().sidecar("ffmpeg") {
        if let Ok(output) = command
            .args(["-hide_banner".to_string(), "-i".to_string(), input_path])
            .output()
            .await
        {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let parsed = parse_ffmpeg_media_metadata(&stderr);
            if parsed.embedded_created_at.is_some() {
                metadata.embedded_created_at = parsed.embedded_created_at;
            }
            if parsed.source_title.is_some() {
                metadata.source_title = parsed.source_title;
            }
        }
    }

    apply_recorded_at_source(&mut metadata);
    Ok(metadata)
}

#[command]
pub async fn extract_audio(
    app: tauri::AppHandle,
    input_path: String,
    output_path: String,
) -> Result<f64, String> {
    let defaults = SmartChunkOptions::default();
    let silence_filter = format!(
        "silencedetect=noise={}dB:d={}",
        defaults.silence_noise_db, defaults.silence_min_duration_sec
    );
    let stderr = run_extract_audio(
        app.clone(),
        input_path,
        output_path.clone(),
        Some(silence_filter),
    )
    .await?;
    let silences = parse_silencedetect(&stderr);
    write_silence_cache(
        &output_path,
        defaults.silence_noise_db,
        defaults.silence_min_duration_sec,
        silences,
    )
    .await;

    let duration = get_duration(&app, &output_path).await?;
    Ok(duration)
}

#[command]
pub async fn chunk_audio(
    app: tauri::AppHandle,
    input_path: String,
    output_dir: String,
    chunk_duration_sec: u32,
) -> Result<Vec<String>, String> {
    fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;

    let pattern = format!("{}/chunk_%03d.mp3", output_dir);

    let output = app
        .shell()
        .sidecar("ffmpeg")
        .map_err(|e| e.to_string())?
        .args([
            "-i",
            &input_path,
            "-f",
            "segment",
            "-segment_time",
            &chunk_duration_sec.to_string(),
            "-c",
            "copy",
            "-reset_timestamps",
            "1",
            &pattern,
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if output.status.code() != Some(0) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("Output") {
            return Err(command_output_error(
                "Falha ao dividir audio com ffmpeg",
                &output,
            ));
        }
    }

    let mut entries = tokio::fs::read_dir(&output_dir)
        .await
        .map_err(|e| e.to_string())?;
    let mut chunks = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        let path = entry.path();
        if path.extension().map(|e| e == "mp3").unwrap_or(false) {
            chunks.push(path.to_string_lossy().to_string());
        }
    }

    chunks.sort();
    Ok(chunks)
}

#[command]
pub async fn detect_silences(
    app: tauri::AppHandle,
    input_path: String,
    noise_db: f64,
    min_duration_sec: f64,
) -> Result<Vec<SilenceRange>, String> {
    let filter = format!("silencedetect=noise={}dB:d={}", noise_db, min_duration_sec);
    let output = app
        .shell()
        .sidecar("ffmpeg")
        .map_err(|e| e.to_string())?
        .args(["-i", &input_path, "-af", &filter, "-f", "null", "-"])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if output.status.code() != Some(0) {
        return Err(command_output_error(
            "Falha ao detectar pausas com ffmpeg",
            &output,
        ));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(parse_silencedetect(&stderr))
}

#[command]
pub async fn create_smart_chunks(
    app: tauri::AppHandle,
    input_path: String,
    output_dir: String,
    duration_sec: f64,
    options: Option<SmartChunkOptions>,
) -> Result<Vec<ExportedChunk>, String> {
    let opts = options.unwrap_or_default();
    fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;

    let silences = if let Some(cached) = read_silence_cache(
        &input_path,
        opts.silence_noise_db,
        opts.silence_min_duration_sec,
    )
    .await
    {
        cached
    } else {
        detect_silences(
            app.clone(),
            input_path.clone(),
            opts.silence_noise_db,
            opts.silence_min_duration_sec,
        )
        .await
        .unwrap_or_default()
    };

    let plans = plan_smart_chunks(
        duration_sec,
        &silences,
        opts.target_sec,
        opts.min_sec,
        opts.max_sec,
        opts.overlap_sec,
    );

    let output_format = validate_chunk_output_format(&opts.output_format)?;
    match export_smart_chunks_batch(
        app.clone(),
        &input_path,
        &output_dir,
        &output_format,
        &plans,
    )
    .await
    {
        Ok(exported) => Ok(exported),
        Err(batch_error) => {
            export_smart_chunks_parallel(app, input_path, output_dir, output_format, plans)
                .await
                .map_err(|parallel_error| {
                    format!(
                        "{}\nTentativa otimizada em lote falhou antes: {}",
                        parallel_error, batch_error
                    )
                })
        }
    }
}

#[command]
pub async fn prepare_audio_and_chunks(
    app: tauri::AppHandle,
    input_path: String,
    audio_output_path: String,
    chunk_output_dir: String,
    options: Option<SmartChunkOptions>,
) -> Result<PreparedAudio, String> {
    let opts = options.unwrap_or_default();
    fs::create_dir_all(&chunk_output_dir).map_err(|e| e.to_string())?;
    if let Some(parent) = Path::new(&audio_output_path).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let normalize_future = run_extract_audio(
        app.clone(),
        input_path.clone(),
        audio_output_path.clone(),
        None,
    );

    let chunk_future = async {
        let duration_future = get_duration(&app, &input_path);
        let silence_future = detect_silences(
            app.clone(),
            input_path.clone(),
            opts.silence_noise_db,
            opts.silence_min_duration_sec,
        );
        let (duration, silences) = tokio::join!(duration_future, silence_future);
        let duration = duration?;
        let silences = silences.unwrap_or_default();
        write_silence_cache(
            &audio_output_path,
            opts.silence_noise_db,
            opts.silence_min_duration_sec,
            silences.clone(),
        )
        .await;

        let plans = plan_smart_chunks(
            duration,
            &silences,
            opts.target_sec,
            opts.min_sec,
            opts.max_sec,
            opts.overlap_sec,
        );
        let output_format = validate_chunk_output_format(&opts.output_format)?;
        let chunks = match export_smart_chunks_batch(
            app.clone(),
            &input_path,
            &chunk_output_dir,
            &output_format,
            &plans,
        )
        .await
        {
            Ok(exported) => exported,
            Err(_) => {
                export_smart_chunks_parallel(
                    app.clone(),
                    input_path.clone(),
                    chunk_output_dir.clone(),
                    output_format,
                    plans,
                )
                .await?
            }
        };

        Ok::<PreparedAudio, String>(PreparedAudio {
            duration_sec: duration,
            chunks,
        })
    };

    let (normalize_result, prepared_result) = tokio::join!(normalize_future, chunk_future);
    normalize_result?;
    prepared_result
}

async fn export_smart_chunks_batch(
    app: tauri::AppHandle,
    input_path: &str,
    output_dir: &str,
    output_format: &str,
    plans: &[ChunkPlan],
) -> Result<Vec<ExportedChunk>, String> {
    if plans.is_empty() {
        return Ok(Vec::new());
    }

    let output_dir_path = Path::new(output_dir);
    let exported = exported_chunks_from_plans(output_dir_path, output_format, plans);
    let args =
        build_smart_chunk_export_args(Path::new(input_path), output_dir_path, output_format, plans);

    if args.is_empty() {
        return Err("Nenhum argumento de exportacao em lote foi gerado".to_string());
    }

    let output = app
        .shell()
        .sidecar("ffmpeg")
        .map_err(|e| e.to_string())?
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if output.status.code() != Some(0) {
        return Err(command_output_error(
            "Falha ao exportar trechos com ffmpeg em lote",
            &output,
        ));
    }

    let missing = exported
        .iter()
        .find(|chunk| !Path::new(&chunk.audio_path).is_file());
    if let Some(chunk) = missing {
        return Err(format!(
            "FFmpeg em lote terminou sem criar o trecho esperado: {}",
            chunk.audio_path
        ));
    }

    Ok(exported)
}

async fn export_smart_chunks_parallel(
    app: tauri::AppHandle,
    input_path: String,
    output_dir: String,
    output_format: String,
    plans: Vec<ChunkPlan>,
) -> Result<Vec<ExportedChunk>, String> {
    let parallelism = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(2, 6)
        .min(plans.len().max(1));
    let input_path = Arc::<str>::from(input_path);
    let output_dir = Arc::<str>::from(output_dir);
    let output_format = Arc::<str>::from(output_format);
    let export_results = stream::iter(plans.into_iter())
        .map(|plan| {
            let app = app.clone();
            let input_path = input_path.clone();
            let output_dir = output_dir.clone();
            let output_format = output_format.clone();
            async move {
                let chunk_duration = plan.end_sec - plan.start_sec;
                let audio_path = Path::new(output_dir.as_ref())
                    .join(format!(
                        "chunk_{:03}.{}",
                        plan.index,
                        output_format.as_ref()
                    ))
                    .to_string_lossy()
                    .to_string();
                let start_arg = format!("{:.3}", plan.start_sec);
                let duration_arg = format!("{:.3}", chunk_duration);
                let mut args = vec![
                    "-ss".to_string(),
                    start_arg,
                    "-i".to_string(),
                    input_path.to_string(),
                    "-t".to_string(),
                    duration_arg,
                    "-ar".to_string(),
                    "16000".to_string(),
                    "-ac".to_string(),
                    "1".to_string(),
                ];
                args.extend(audio_codec_args(output_format.as_ref()));
                args.extend(["-y".to_string(), audio_path.clone()]);

                let output = app
                    .shell()
                    .sidecar("ffmpeg")
                    .map_err(|e| e.to_string())?
                    .args(args)
                    .output()
                    .await
                    .map_err(|e| e.to_string())?;

                if output.status.code() != Some(0) {
                    return Err(command_output_error(
                        "Falha ao exportar trecho com ffmpeg",
                        &output,
                    ));
                }

                Ok(ExportedChunk {
                    index: plan.index,
                    audio_path,
                    start_sec: plan.start_sec,
                    end_sec: plan.end_sec,
                    offset_sec: plan.offset_sec,
                    duration_sec: chunk_duration,
                })
            }
        })
        .buffer_unordered(parallelism)
        .collect::<Vec<_>>()
        .await;

    let mut exported = export_results
        .into_iter()
        .collect::<Result<Vec<_>, String>>()?;
    exported.sort_by_key(|chunk| chunk.index);

    Ok(exported)
}

async fn probe_duration_with_ffprobe(path: &str) -> Option<f64> {
    let output = tokio::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            path,
        ])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    parse_duration_from_ffprobe_stdout(&String::from_utf8_lossy(&output.stdout))
}

async fn get_duration(app: &tauri::AppHandle, path: &str) -> Result<f64, String> {
    let header_output = app
        .shell()
        .sidecar("ffmpeg")
        .map_err(|e| e.to_string())?
        .args(["-hide_banner", "-i", path])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    let header_stderr = String::from_utf8_lossy(&header_output.stderr);
    if let Some(duration) = parse_duration_from_ffmpeg_stderr(&header_stderr) {
        return Ok(duration);
    }

    if let Some(duration) = probe_duration_with_ffprobe(path).await {
        return Ok(duration);
    }

    let output = app
        .shell()
        .sidecar("ffmpeg")
        .map_err(|e| e.to_string())?
        .args(["-i", path, "-f", "null", "-"])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if output.status.code() != Some(0) {
        return Err(command_output_error(
            "Falha ao medir duracao com ffmpeg",
            &output,
        ));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_duration_from_ffmpeg_stderr(&stderr)
        .ok_or_else(|| "Nao foi possivel medir duracao positiva do audio".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::audio::SilenceRange;
    use std::sync::mpsc;
    use std::time::Duration;

    const EPSILON: f64 = 1e-9;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= EPSILON,
            "expected {actual} to be within {EPSILON} of {expected}"
        );
    }

    fn plan_with_timeout(
        duration_sec: f64,
        silences: Vec<SilenceRange>,
        target_sec: f64,
        min_sec: f64,
        max_sec: f64,
        overlap_sec: f64,
    ) -> Option<Vec<ChunkPlan>> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let chunks = plan_smart_chunks(
                duration_sec,
                &silences,
                target_sec,
                min_sec,
                max_sec,
                overlap_sec,
            );
            let _ = tx.send(chunks);
        });

        rx.recv_timeout(Duration::from_millis(200)).ok()
    }

    #[test]
    fn validates_chunk_output_format_whitelist() {
        assert_eq!(validate_chunk_output_format("").unwrap(), "flac");
        assert_eq!(validate_chunk_output_format("FLAC").unwrap(), "flac");
        assert_eq!(validate_chunk_output_format(".wav").unwrap(), "wav");
        assert_eq!(validate_chunk_output_format(" mp3 ").unwrap(), "mp3");

        let err = validate_chunk_output_format("../bad").unwrap_err();
        assert!(err.contains("Formato de audio invalido"));
    }

    #[test]
    fn normalized_audio_args_can_skip_silence_detection_for_parallel_prepare() {
        let args = build_extract_audio_args(
            Path::new("meeting.mp4"),
            Path::new("meeting.wav"),
            "wav",
            None,
        );

        assert!(args.windows(2).any(|pair| pair == ["-vn", "-ar"]));
        assert!(!args.iter().any(|arg| arg.contains("silencedetect")));
        assert!(args.iter().any(|arg| arg == "meeting.mp4"));
        assert!(args.iter().any(|arg| arg == "meeting.wav"));
    }

    #[test]
    fn contiguous_chunk_plans_use_segment_muxer_and_copy_when_format_matches() {
        let plans = vec![
            ChunkPlan {
                index: 0,
                start_sec: 0.0,
                end_sec: 10.0,
                offset_sec: 0.0,
            },
            ChunkPlan {
                index: 1,
                start_sec: 10.0,
                end_sec: 20.0,
                offset_sec: 10.0,
            },
            ChunkPlan {
                index: 2,
                start_sec: 20.0,
                end_sec: 30.0,
                offset_sec: 20.0,
            },
        ];

        let args = build_smart_chunk_export_args(
            Path::new("meeting.wav"),
            Path::new("chunks"),
            "wav",
            &plans,
        );

        assert!(args.windows(2).any(|pair| pair == ["-f", "segment"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-segment_times", "10.000,20.000"]));
        assert!(args.windows(2).any(|pair| pair == ["-c:a", "copy"]));
        assert!(!args.iter().any(|arg| arg == "-filter_complex"));
        assert!(args
            .last()
            .expect("output pattern should exist")
            .ends_with("chunk_%03d.wav"));
    }

    #[test]
    fn short_contiguous_reencode_stays_on_parallel_export() {
        let plans = vec![
            ChunkPlan {
                index: 0,
                start_sec: 0.0,
                end_sec: 10.0,
                offset_sec: 0.0,
            },
            ChunkPlan {
                index: 1,
                start_sec: 10.0,
                end_sec: 20.0,
                offset_sec: 10.0,
            },
            ChunkPlan {
                index: 2,
                start_sec: 20.0,
                end_sec: 30.0,
                offset_sec: 20.0,
            },
            ChunkPlan {
                index: 3,
                start_sec: 30.0,
                end_sec: 40.0,
                offset_sec: 30.0,
            },
        ];

        let strategy = select_smart_chunk_export_strategy(Path::new("meeting.wav"), "flac", &plans);

        assert_eq!(strategy, SmartChunkExportStrategy::ParallelPerChunk);
    }

    #[test]
    fn overlapping_chunk_plans_stay_on_parallel_export_to_preserve_overlap() {
        let plans = vec![
            ChunkPlan {
                index: 0,
                start_sec: 0.0,
                end_sec: 10.0,
                offset_sec: 0.0,
            },
            ChunkPlan {
                index: 1,
                start_sec: 7.0,
                end_sec: 20.0,
                offset_sec: 7.0,
            },
        ];

        let strategy = select_smart_chunk_export_strategy(Path::new("meeting.wav"), "flac", &plans);
        let args = build_smart_chunk_export_args(
            Path::new("meeting.wav"),
            Path::new("chunks"),
            "flac",
            &plans,
        );

        assert_eq!(strategy, SmartChunkExportStrategy::ParallelPerChunk);
        assert!(
            args.is_empty(),
            "overlapping plans cannot use segment muxer without dropping overlap"
        );
    }

    #[test]
    fn parses_duration_from_ffmpeg_progress_line() {
        let stderr = "
            size= 1024kB time=00:01:02.50 bitrate= 128.0kbits/s speed=1x
            size= 2048kB time=00:01:03.25 bitrate= 128.0kbits/s speed=1x
        ";

        assert_close(
            parse_duration_from_ffmpeg_stderr(stderr).expect("duration should parse"),
            63.25,
        );
        assert_eq!(parse_duration_from_ffmpeg_stderr("no duration here"), None);
        assert_eq!(
            parse_duration_from_ffmpeg_stderr("time=00:nope:01.00"),
            None
        );
        assert_eq!(parse_duration_from_ffmpeg_stderr("time=00:00:00.00"), None);
    }

    #[test]
    fn parses_duration_from_ffmpeg_header_without_decoding_progress() {
        let stderr = "
Input #0, wav, from 'meeting.wav':
  Duration: 02:03:04.56, bitrate: 256 kb/s
";

        assert_close(
            parse_duration_from_ffmpeg_stderr(stderr).expect("header duration should parse"),
            7384.56,
        );
    }

    #[test]
    fn parses_duration_from_ffprobe_stdout() {
        assert_close(
            parse_duration_from_ffprobe_stdout("3723.450000\n").expect("ffprobe duration"),
            3723.45,
        );
        assert_eq!(parse_duration_from_ffprobe_stdout("N/A\n"), None);
        assert_eq!(parse_duration_from_ffprobe_stdout("0\n"), None);
    }

    #[test]
    fn parses_ffmpeg_silencedetect_ranges() {
        let stderr = "
            [silencedetect @ 000] silence_start: 12.345
            [silencedetect @ 000] silence_end: 14.890 | silence_duration: 2.545
            [silencedetect @ 000] silence_start: 44
            [silencedetect @ 000] silence_end: 45.25 | silence_duration: 1.25
        ";

        let ranges = parse_silencedetect(stderr);

        assert_eq!(ranges.len(), 2);
        assert_close(ranges[0].start_sec, 12.345);
        assert_close(ranges[0].end_sec, 14.890);
        assert_close(ranges[1].start_sec, 44.0);
        assert_close(ranges[1].end_sec, 45.25);
    }

    #[test]
    fn parses_video_creation_time_from_ffmpeg_metadata() {
        let stderr = r#"
Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'reuniao.mp4':
  Metadata:
    major_brand     : isom
    creation_time   : 2026-05-08T18:48:29.000000Z
    title           : Reuniao de alinhamento
"#;

        let metadata = parse_ffmpeg_media_metadata(stderr);

        assert_eq!(
            metadata.embedded_created_at.as_deref(),
            Some("2026-05-08T18:48:29+00:00")
        );
        assert_eq!(
            metadata.source_title.as_deref(),
            Some("Reuniao de alinhamento")
        );
    }

    #[test]
    fn chooses_embedded_creation_time_before_filesystem_time() {
        let mut metadata = crate::models::meeting::MeetingMetadata {
            embedded_created_at: Some("2026-05-08T18:48:29+00:00".to_string()),
            file_modified_at: Some("2026-05-19T16:10:00+00:00".to_string()),
            ..Default::default()
        };

        apply_recorded_at_source(&mut metadata);

        assert_eq!(
            metadata.recorded_at.as_deref(),
            Some("2026-05-08T18:48:29+00:00")
        );
        assert_eq!(
            metadata.recorded_at_source.as_deref(),
            Some("embedded_created_at")
        );
    }

    #[test]
    fn malformed_silence_end_keeps_pending_start_for_next_valid_end() {
        let stderr = "
            [silencedetect @ 000] silence_start: 10
            [silencedetect @ 000] silence_end: nope | silence_duration: nope
            [silencedetect @ 000] silence_end: 12.5 | silence_duration: 2.5
            [silencedetect @ 000] silence_start: 20
            [silencedetect @ 000] silence_end: 22 | silence_duration: 2
        ";

        let ranges = parse_silencedetect(stderr);

        assert_eq!(ranges.len(), 2);
        assert_close(ranges[0].start_sec, 10.0);
        assert_close(ranges[0].end_sec, 12.5);
        assert_close(ranges[1].start_sec, 20.0);
        assert_close(ranges[1].end_sec, 22.0);
    }

    #[test]
    fn plans_chunks_near_silence_with_overlap() {
        let silences = vec![
            SilenceRange {
                start_sec: 295.0,
                end_sec: 300.0,
            },
            SilenceRange {
                start_sec: 602.0,
                end_sec: 606.0,
            },
        ];

        let chunks = plan_smart_chunks(900.0, &silences, 300.0, 180.0, 360.0, 3.0);

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].index, 0);
        assert_close(chunks[0].start_sec, 0.0);
        assert_close(chunks[0].end_sec, 297.5);
        assert_close(chunks[0].offset_sec, 0.0);
        assert_close(chunks[1].start_sec, 294.5);
        assert_close(chunks[1].end_sec, 604.0);
        assert_close(chunks[1].offset_sec, 294.5);
        assert_close(chunks[2].start_sec, 601.0);
        assert_close(chunks[2].end_sec, 900.0);
    }

    #[test]
    fn fallback_avoids_tiny_tail_when_remaining_fits_under_max() {
        let chunks = plan_smart_chunks(650.0, &[], 300.0, 180.0, 360.0, 3.0);

        assert_eq!(chunks.len(), 2);
        assert_close(chunks[0].start_sec, 0.0);
        assert_close(chunks[0].end_sec, 300.0);
        assert_close(chunks[1].start_sec, 297.0);
        assert_close(chunks[1].end_sec, 650.0);
    }

    #[test]
    fn invalid_and_hostile_values_terminate_with_sensible_chunks() {
        assert!(plan_smart_chunks(0.0, &[], 300.0, 180.0, 360.0, 3.0).is_empty());
        assert!(plan_smart_chunks(-1.0, &[], 300.0, 180.0, 360.0, 3.0).is_empty());
        assert!(plan_smart_chunks(f64::NAN, &[], 300.0, 180.0, 360.0, 3.0).is_empty());

        let infinite_duration = plan_with_timeout(f64::INFINITY, vec![], 300.0, 180.0, 360.0, 3.0);
        let hostile_options = plan_with_timeout(650.0, vec![], 0.0, -10.0, 0.0, 999.0);

        assert!(
            infinite_duration.as_ref().is_some_and(Vec::is_empty),
            "non-finite duration should terminate with no chunks"
        );

        let chunks = hostile_options.expect("hostile options should terminate");
        assert!(!chunks.is_empty());
        assert!(chunks.len() <= 4);
        assert_close(chunks[0].start_sec, 0.0);
        assert_close(chunks.last().unwrap().end_sec, 650.0);

        for chunk in &chunks {
            assert!(chunk.start_sec.is_finite());
            assert!(chunk.end_sec.is_finite());
            assert!(chunk.end_sec > chunk.start_sec);
            assert!(chunk.end_sec - chunk.start_sec <= 480.0);
        }
    }
}
