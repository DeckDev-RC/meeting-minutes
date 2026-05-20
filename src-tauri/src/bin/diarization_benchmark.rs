use meeting_minutes_lib::benchmark::compute_benchmark_speed;
use meeting_minutes_lib::commands::diarize::{
    diarization_asset_paths, diarize_transcription_locally, diarize_with_mode,
    ensure_diarization_assets_in_dir, normalize_diarization_threads, DiarizationMode,
};
use meeting_minutes_lib::models::audio::ExportedChunk;
use meeting_minutes_lib::models::transcription::{DiarizedResult, TranscriptionSegment};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug)]
struct CliOptions {
    audio: PathBuf,
    chunks: PathBuf,
    segments: PathBuf,
    out_dir: PathBuf,
    app_data_dir: PathBuf,
    mode: DiarizationMode,
    threads: i32,
    expected_speakers: Option<i32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiarizationBenchmarkReport {
    mode: String,
    threads: i32,
    audio_path: String,
    audio_duration_sec: f64,
    wall_clock_sec: f64,
    realtime_factor: f64,
    speed_x: f64,
    projected_three_hour_sec: f64,
    projected_three_hour_min: f64,
    chunk_count: usize,
    transcript_segment_count: usize,
    diarized_segment_count: usize,
    speaker_count: usize,
    output_files: Vec<String>,
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn default_app_data_dir() -> Result<PathBuf, String> {
    let appdata = std::env::var("APPDATA").map_err(|_| "APPDATA is not set".to_string())?;
    Ok(PathBuf::from(appdata).join("com.agregar.meeting-minutes"))
}

fn parse_optional_i32(value: Option<String>) -> Option<i32> {
    value.and_then(|value| value.parse::<i32>().ok())
}

fn mode_label(mode: DiarizationMode) -> &'static str {
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

fn parse_cli() -> Result<CliOptions, String> {
    let args = std::env::args().collect::<Vec<_>>();
    let audio = arg_value(&args, "--audio")
        .map(PathBuf::from)
        .ok_or_else(|| "Use --audio <normalized-wav-path>".to_string())?;
    let chunks = arg_value(&args, "--chunks")
        .map(PathBuf::from)
        .ok_or_else(|| "Use --chunks <chunks-json-path>".to_string())?;
    let segments = arg_value(&args, "--segments")
        .map(PathBuf::from)
        .ok_or_else(|| "Use --segments <transcription-segments-json-path>".to_string())?;
    let out_dir = arg_value(&args, "--out-dir")
        .map(PathBuf::from)
        .ok_or_else(|| "Use --out-dir <benchmark-output-dir>".to_string())?;
    let app_data_dir = arg_value(&args, "--app-data-dir")
        .map(PathBuf::from)
        .unwrap_or(default_app_data_dir()?);
    let mode = DiarizationMode::from_option(arg_value(&args, "--mode"));
    let available_threads = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(2);
    let threads = normalize_diarization_threads(
        parse_optional_i32(arg_value(&args, "--threads")),
        available_threads,
    );
    let expected_speakers = parse_optional_i32(arg_value(&args, "--expected-speakers"));

    Ok(CliOptions {
        audio,
        chunks,
        segments,
        out_dir,
        app_data_dir,
        mode,
        threads,
        expected_speakers,
    })
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("Failed to parse {}: {e}", path.display()))
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

fn audio_duration_from_chunks(chunks: &[ExportedChunk], segments: &[TranscriptionSegment]) -> f64 {
    chunks
        .iter()
        .map(|chunk| chunk.end_sec)
        .chain(segments.iter().map(|segment| segment.end))
        .fold(0.0_f64, f64::max)
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let options = parse_cli()?;
    std::fs::create_dir_all(&options.out_dir).map_err(|e| e.to_string())?;

    let chunks = read_json::<Vec<ExportedChunk>>(&options.chunks)?;
    let segments = read_json::<Vec<TranscriptionSegment>>(&options.segments)?;
    let audio_duration_sec = audio_duration_from_chunks(&chunks, &segments);
    if audio_duration_sec <= 0.0 {
        return Err("Could not infer positive audio duration".to_string());
    }

    eprintln!(
        "[diarization-benchmark] mode={} threads={} chunks={} segments={}",
        mode_label(options.mode),
        options.threads,
        chunks.len(),
        segments.len()
    );

    let started = Instant::now();
    let diarized: DiarizedResult = if options.mode == DiarizationMode::Fast {
        diarize_transcription_locally(&segments)
    } else {
        let assets = if matches!(
            options.mode,
            DiarizationMode::ModernCpu
                | DiarizationMode::ModernCpuChunked
                | DiarizationMode::Pyannote
        ) {
            diarization_asset_paths(&options.app_data_dir)
        } else {
            ensure_diarization_assets_in_dir(&options.app_data_dir).await?
        };
        diarize_with_mode(
            options.audio.to_string_lossy().to_string(),
            segments.clone(),
            Some(chunks.clone()),
            assets,
            options.expected_speakers,
            options.mode,
            options.threads,
        )
        .await?
    };
    let wall_clock_sec = started.elapsed().as_secs_f64();
    let speed = compute_benchmark_speed(audio_duration_sec, wall_clock_sec)?;
    let projected_three_hour_sec = 10_800.0 * speed.realtime_factor;

    let diarized_path = options.out_dir.join("diarized-transcription.json");
    let report_path = options.out_dir.join("diarization-benchmark-report.json");
    write_json(&diarized_path, &diarized)?;

    let report = DiarizationBenchmarkReport {
        mode: mode_label(options.mode).to_string(),
        threads: options.threads,
        audio_path: options.audio.to_string_lossy().to_string(),
        audio_duration_sec,
        wall_clock_sec,
        realtime_factor: speed.realtime_factor,
        speed_x: speed.speed_x,
        projected_three_hour_sec,
        projected_three_hour_min: projected_three_hour_sec / 60.0,
        chunk_count: chunks.len(),
        transcript_segment_count: segments.len(),
        diarized_segment_count: diarized.segments.len(),
        speaker_count: diarized.speakers.len(),
        output_files: vec![
            report_path.to_string_lossy().to_string(),
            diarized_path.to_string_lossy().to_string(),
        ],
    };
    write_json(&report_path, &report)?;

    eprintln!(
        "[diarization-benchmark] done: {:.2}s wall, RTF {:.3}, {:.2}x",
        report.wall_clock_sec, report.realtime_factor, report.speed_x
    );

    Ok(())
}
