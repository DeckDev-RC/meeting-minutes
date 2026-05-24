use crate::models::audio::{ChunkPlan, ExportedChunk, SilenceRange, SmartChunkOptions};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_shell::ShellExt;

use super::{command_output_error, detect_silences};
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
    #[serde(default)]
    source_fingerprint: Option<String>,
    ranges: Vec<SilenceRange>,
}

fn silence_cache_path(audio_path: &str) -> PathBuf {
    Path::new(audio_path).with_extension("silences.json")
}

pub(super) const FINGERPRINT_SAMPLE_BYTES: u64 = 256 * 1024;

fn hash_file_sample(
    file: &mut fs::File,
    hasher: &mut Sha256,
    offset: u64,
    len: usize,
) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer = vec![0_u8; len];
    let read = file.read(&mut buffer)?;
    hasher.update(&buffer[..read]);
    Ok(())
}

pub(super) fn source_audio_fingerprint(input_path: &str) -> Option<String> {
    let mut file = fs::File::open(input_path).ok()?;
    let size = file.metadata().ok()?.len();
    let mut hasher = Sha256::new();
    hasher.update(size.to_le_bytes());

    let first_len = usize::try_from(size.min(FINGERPRINT_SAMPLE_BYTES)).ok()?;
    hash_file_sample(&mut file, &mut hasher, 0, first_len).ok()?;

    if size > FINGERPRINT_SAMPLE_BYTES {
        let tail_offset = size.saturating_sub(FINGERPRINT_SAMPLE_BYTES);
        let tail_len = usize::try_from(size - tail_offset).ok()?;
        hash_file_sample(&mut file, &mut hasher, tail_offset, tail_len).ok()?;
    }

    Some(format!("{:x}", hasher.finalize()))
}

fn silence_cache_options_slug(noise_db: f64, min_duration_sec: f64) -> String {
    format!("{noise_db:.1}_{min_duration_sec:.2}")
        .replace('-', "m")
        .replace('.', "p")
}

pub(super) fn silence_cache_file_name_for_fingerprint(
    fingerprint: &str,
    noise_db: f64,
    min_duration_sec: f64,
) -> String {
    format!(
        "{}-{}.json",
        fingerprint,
        silence_cache_options_slug(noise_db, min_duration_sec)
    )
}

fn global_silence_cache_path(
    app: &tauri::AppHandle,
    input_path: &str,
    noise_db: f64,
    min_duration_sec: f64,
) -> Option<(PathBuf, String)> {
    let fingerprint = source_audio_fingerprint(input_path)?;
    let file_name =
        silence_cache_file_name_for_fingerprint(&fingerprint, noise_db, min_duration_sec);
    let path = app
        .path()
        .app_data_dir()
        .ok()?
        .join("audio-cache")
        .join("silences")
        .join(file_name);
    Some((path, fingerprint))
}

fn silence_cache_matches(cache: &SilenceCache, noise_db: f64, min_duration_sec: f64) -> bool {
    (cache.noise_db - noise_db).abs() <= f64::EPSILON
        && (cache.min_duration_sec - min_duration_sec).abs() <= f64::EPSILON
}

pub(super) async fn write_silence_cache(
    audio_path: &str,
    noise_db: f64,
    min_duration_sec: f64,
    ranges: Vec<SilenceRange>,
) {
    write_silence_cache_at_path(
        silence_cache_path(audio_path),
        noise_db,
        min_duration_sec,
        None,
        ranges,
    )
    .await;
}

pub(super) async fn write_silence_cache_at_path(
    cache_path: PathBuf,
    noise_db: f64,
    min_duration_sec: f64,
    source_fingerprint: Option<String>,
    ranges: Vec<SilenceRange>,
) {
    let cache = SilenceCache {
        noise_db,
        min_duration_sec,
        source_fingerprint,
        ranges,
    };
    let Ok(raw) = serde_json::to_string(&cache) else {
        return;
    };
    if let Some(parent) = cache_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(cache_path, raw).await;
}

async fn read_silence_cache(
    audio_path: &str,
    noise_db: f64,
    min_duration_sec: f64,
) -> Option<Vec<SilenceRange>> {
    read_silence_cache_at_path(silence_cache_path(audio_path), noise_db, min_duration_sec).await
}

async fn read_silence_cache_at_path(
    cache_path: PathBuf,
    noise_db: f64,
    min_duration_sec: f64,
) -> Option<Vec<SilenceRange>> {
    let raw = tokio::fs::read_to_string(cache_path).await.ok()?;
    let cache = serde_json::from_str::<SilenceCache>(&raw).ok()?;
    if silence_cache_matches(&cache, noise_db, min_duration_sec) {
        Some(cache.ranges)
    } else {
        None
    }
}

async fn read_global_silence_cache(
    app: &tauri::AppHandle,
    input_path: &str,
    noise_db: f64,
    min_duration_sec: f64,
) -> Option<Vec<SilenceRange>> {
    let (cache_path, _) = global_silence_cache_path(app, input_path, noise_db, min_duration_sec)?;
    read_silence_cache_at_path(cache_path, noise_db, min_duration_sec).await
}

pub(super) async fn write_global_silence_cache(
    app: &tauri::AppHandle,
    input_path: &str,
    noise_db: f64,
    min_duration_sec: f64,
    ranges: Vec<SilenceRange>,
) {
    let Some((cache_path, fingerprint)) =
        global_silence_cache_path(app, input_path, noise_db, min_duration_sec)
    else {
        return;
    };
    write_silence_cache_at_path(
        cache_path,
        noise_db,
        min_duration_sec,
        Some(fingerprint),
        ranges,
    )
    .await;
}

pub(super) async fn cached_or_detect_silences(
    app: tauri::AppHandle,
    input_path: String,
    local_cache_audio_path: Option<String>,
    noise_db: f64,
    min_duration_sec: f64,
) -> Vec<SilenceRange> {
    if let Some(local_path) = local_cache_audio_path.as_deref() {
        if let Some(cached) = read_silence_cache(local_path, noise_db, min_duration_sec).await {
            return cached;
        }
    }

    if let Some(cached) = read_silence_cache(&input_path, noise_db, min_duration_sec).await {
        return cached;
    }

    if let Some(cached) =
        read_global_silence_cache(&app, &input_path, noise_db, min_duration_sec).await
    {
        if let Some(local_path) = local_cache_audio_path.as_deref() {
            write_silence_cache(local_path, noise_db, min_duration_sec, cached.clone()).await;
        }
        return cached;
    }

    let silences = detect_silences(app.clone(), input_path.clone(), noise_db, min_duration_sec)
        .await
        .unwrap_or_default();
    if let Some(local_path) = local_cache_audio_path.as_deref() {
        write_silence_cache(local_path, noise_db, min_duration_sec, silences.clone()).await;
    }
    write_global_silence_cache(
        &app,
        &input_path,
        noise_db,
        min_duration_sec,
        silences.clone(),
    )
    .await;
    silences
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

pub(super) fn build_silence_filter(noise_db: f64, min_duration_sec: f64) -> String {
    format!("silencedetect=noise={}dB:d={}", noise_db, min_duration_sec)
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

pub(super) fn build_parallel_chunk_export_args(
    input_path: &Path,
    audio_path: &Path,
    output_format: &str,
    plan: &ChunkPlan,
) -> Vec<String> {
    let chunk_duration = plan.end_sec - plan.start_sec;
    let mut args = vec![
        "-ss".to_string(),
        format_ffmpeg_seconds(plan.start_sec),
        "-i".to_string(),
        input_path.to_string_lossy().to_string(),
        "-t".to_string(),
        format_ffmpeg_seconds(chunk_duration),
        "-map".to_string(),
        "0:a:0".to_string(),
        "-vn".to_string(),
        "-ar".to_string(),
        "16000".to_string(),
        "-ac".to_string(),
        "1".to_string(),
    ];
    args.extend(audio_codec_args(output_format));
    args.extend(["-y".to_string(), audio_path.to_string_lossy().to_string()]);
    args
}

pub(super) async fn run_extract_audio(
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

pub(super) async fn export_planned_smart_chunks(
    app: tauri::AppHandle,
    input_path: String,
    chunk_output_dir: String,
    output_format: String,
    plans: Vec<ChunkPlan>,
) -> Result<Vec<ExportedChunk>, String> {
    match export_smart_chunks_batch(
        app.clone(),
        &input_path,
        &chunk_output_dir,
        &output_format,
        &plans,
    )
    .await
    {
        Ok(exported) => Ok(exported),
        Err(_) => {
            export_smart_chunks_parallel(app, input_path, chunk_output_dir, output_format, plans)
                .await
        }
    }
}

pub(super) async fn export_smart_chunks_batch(
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

pub(super) async fn export_smart_chunks_parallel(
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
                let audio_path_buf = Path::new(output_dir.as_ref()).join(format!(
                    "chunk_{:03}.{}",
                    plan.index,
                    output_format.as_ref()
                ));
                let audio_path = audio_path_buf.to_string_lossy().to_string();
                let args = build_parallel_chunk_export_args(
                    Path::new(input_path.as_ref()),
                    &audio_path_buf,
                    output_format.as_ref(),
                    &plan,
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
