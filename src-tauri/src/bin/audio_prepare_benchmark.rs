use meeting_minutes_lib::commands::audio::{
    audio_codec_args, build_extract_audio_args, build_smart_chunk_export_args,
    parse_duration_from_ffmpeg_stderr, parse_silencedetect, plan_smart_chunks,
    validate_chunk_output_format,
};
use meeting_minutes_lib::models::audio::{ChunkPlan, SilenceRange, SmartChunkOptions};
use serde::Serialize;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug)]
struct CliOptions {
    input: PathBuf,
    out_dir: PathBuf,
    ffmpeg: PathBuf,
    output_format: String,
    target_sec: f64,
    min_sec: f64,
    max_sec: f64,
    overlap_sec: f64,
    workers: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrepareBenchResult {
    label: String,
    wall_clock_sec: f64,
    duration_sec: f64,
    chunks: usize,
    output_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrepareBenchReport {
    input: String,
    output_format: String,
    overlap_sec: f64,
    workers: usize,
    legacy: PrepareBenchResult,
    prepared: PrepareBenchResult,
    speedup_x: f64,
    faster_strategy: String,
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn parse_f64(value: Option<String>, fallback: f64) -> f64 {
    value
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(fallback)
}

fn parse_usize(value: Option<String>, fallback: usize) -> usize {
    value
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn default_project_root() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    if cwd.file_name().and_then(|name| name.to_str()) == Some("src-tauri") {
        Ok(cwd.parent().unwrap_or(&cwd).to_path_buf())
    } else {
        Ok(cwd)
    }
}

fn default_ffmpeg_path(project_root: &Path) -> PathBuf {
    project_root
        .join("src-tauri")
        .join("binaries")
        .join("ffmpeg-x86_64-pc-windows-msvc.exe")
}

fn parse_cli() -> Result<CliOptions, String> {
    let args = std::env::args().collect::<Vec<_>>();
    let project_root = default_project_root()?;
    let defaults = SmartChunkOptions::default();
    let input = arg_value(&args, "--input")
        .map(PathBuf::from)
        .ok_or_else(|| "Use --input <audio-or-video-path>".to_string())?;
    let id = format!(
        "audio-prepare-{}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    let out_dir = arg_value(&args, "--out-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| project_root.join("benchmarks").join("runs").join(id));
    let ffmpeg = arg_value(&args, "--ffmpeg")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_ffmpeg_path(&project_root));
    let output_format = validate_chunk_output_format(
        &arg_value(&args, "--format").unwrap_or(defaults.output_format),
    )?;

    Ok(CliOptions {
        input,
        out_dir,
        ffmpeg,
        output_format,
        target_sec: parse_f64(arg_value(&args, "--target-sec"), defaults.target_sec),
        min_sec: parse_f64(arg_value(&args, "--min-sec"), defaults.min_sec),
        max_sec: parse_f64(arg_value(&args, "--max-sec"), defaults.max_sec),
        overlap_sec: parse_f64(arg_value(&args, "--overlap-sec"), defaults.overlap_sec),
        workers: parse_usize(arg_value(&args, "--workers"), 4),
    })
}

fn run_ffmpeg(ffmpeg: &Path, args: &[String]) -> Result<String, String> {
    let output = Command::new(ffmpeg)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run ffmpeg at {}: {e}", ffmpeg.display()))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let details = if stderr.is_empty() { stdout } else { stderr };
    Err(format!("ffmpeg failed: {details}"))
}

fn probe_duration(ffmpeg: &Path, input_path: &Path) -> Result<f64, String> {
    let stderr = run_ffmpeg(
        ffmpeg,
        &[
            "-hide_banner".to_string(),
            "-i".to_string(),
            input_path.to_string_lossy().to_string(),
        ],
    )
    .unwrap_or_else(|stderr| stderr);
    parse_duration_from_ffmpeg_stderr(&stderr)
        .ok_or_else(|| format!("Could not parse duration for {}", input_path.display()))
}

fn reset_dir(path: &Path) -> Result<(), String> {
    if path.exists() {
        std::fs::remove_dir_all(path).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(path).map_err(|e| e.to_string())
}

fn output_bytes(path: &Path) -> Result<u64, String> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        if metadata.is_file() {
            total += metadata.len();
        }
    }
    Ok(total)
}

fn chunk_args(
    input_path: &Path,
    output_dir: &Path,
    output_format: &str,
    plan: &ChunkPlan,
) -> Vec<String> {
    let audio_path = output_dir.join(format!("chunk_{:03}.{output_format}", plan.index));
    let mut args = vec![
        "-ss".to_string(),
        format!("{:.3}", plan.start_sec),
        "-i".to_string(),
        input_path.to_string_lossy().to_string(),
        "-t".to_string(),
        format!("{:.3}", plan.end_sec - plan.start_sec),
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

fn export_chunks(
    ffmpeg: &Path,
    input_path: &Path,
    output_dir: &Path,
    output_format: &str,
    plans: &[ChunkPlan],
    workers: usize,
) -> Result<(), String> {
    reset_dir(output_dir)?;
    let batch_args = build_smart_chunk_export_args(input_path, output_dir, output_format, plans);
    if !batch_args.is_empty() {
        run_ffmpeg(ffmpeg, &batch_args)?;
        return Ok(());
    }

    let queue = Arc::new(Mutex::new(VecDeque::from(plans.to_vec())));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    std::thread::scope(|scope| {
        for _ in 0..workers.min(plans.len().max(1)) {
            let queue = Arc::clone(&queue);
            let errors = Arc::clone(&errors);
            scope.spawn(move || loop {
                let plan = {
                    let mut queue = queue.lock().expect("queue lock should not be poisoned");
                    queue.pop_front()
                };
                let Some(plan) = plan else {
                    break;
                };
                let args = chunk_args(input_path, output_dir, output_format, &plan);
                if let Err(error) = run_ffmpeg(ffmpeg, &args) {
                    errors
                        .lock()
                        .expect("error lock should not be poisoned")
                        .push(error);
                    break;
                }
            });
        }
    });

    let errors = errors.lock().map_err(|e| e.to_string())?;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn plan_from_silences(
    options: &CliOptions,
    duration_sec: f64,
    silences: &[SilenceRange],
) -> Vec<ChunkPlan> {
    plan_smart_chunks(
        duration_sec,
        silences,
        options.target_sec,
        options.min_sec,
        options.max_sec,
        options.overlap_sec,
    )
}

fn run_legacy(options: &CliOptions) -> Result<PrepareBenchResult, String> {
    let root = options.out_dir.join("legacy");
    reset_dir(&root)?;
    let audio_path = root.join("normalized.wav");
    let chunks_dir = root.join("chunks");
    let started = Instant::now();
    let filter = "silencedetect=noise=-35dB:d=0.45".to_string();
    let stderr = run_ffmpeg(
        &options.ffmpeg,
        &build_extract_audio_args(&options.input, &audio_path, "wav", Some(&filter)),
    )?;
    let silences = parse_silencedetect(&stderr);
    let duration_sec = probe_duration(&options.ffmpeg, &audio_path)?;
    let plans = plan_from_silences(options, duration_sec, &silences);
    export_chunks(
        &options.ffmpeg,
        &audio_path,
        &chunks_dir,
        &options.output_format,
        &plans,
        options.workers,
    )?;

    Ok(PrepareBenchResult {
        label: "single_pass_silence_then_chunks".to_string(),
        wall_clock_sec: started.elapsed().as_secs_f64(),
        duration_sec,
        chunks: plans.len(),
        output_bytes: output_bytes(&chunks_dir)?,
    })
}

fn run_prepared(options: &CliOptions) -> Result<PrepareBenchResult, String> {
    let root = options.out_dir.join("prepared");
    reset_dir(&root)?;
    let audio_path = root.join("normalized.wav");
    let chunks_dir = root.join("chunks");
    let started = Instant::now();
    let ffmpeg = options.ffmpeg.clone();
    let input = options.input.clone();
    let normalize_args = build_extract_audio_args(&input, &audio_path, "wav", None);

    let normalize = std::thread::spawn(move || run_ffmpeg(&ffmpeg, &normalize_args));
    let duration_sec = probe_duration(&options.ffmpeg, &options.input)?;
    let silence_stderr = run_ffmpeg(
        &options.ffmpeg,
        &[
            "-i".to_string(),
            options.input.to_string_lossy().to_string(),
            "-af".to_string(),
            "silencedetect=noise=-35dB:d=0.45".to_string(),
            "-f".to_string(),
            "null".to_string(),
            "-".to_string(),
        ],
    )?;
    let silences = parse_silencedetect(&silence_stderr);
    let plans = plan_from_silences(options, duration_sec, &silences);
    export_chunks(
        &options.ffmpeg,
        &options.input,
        &chunks_dir,
        &options.output_format,
        &plans,
        options.workers,
    )?;
    normalize
        .join()
        .map_err(|_| "normalize worker panicked".to_string())??;

    Ok(PrepareBenchResult {
        label: "parallel_prepare_audio_and_chunks".to_string(),
        wall_clock_sec: started.elapsed().as_secs_f64(),
        duration_sec,
        chunks: plans.len(),
        output_bytes: output_bytes(&chunks_dir)?,
    })
}

fn run() -> Result<(), String> {
    let options = parse_cli()?;
    std::fs::create_dir_all(&options.out_dir).map_err(|e| e.to_string())?;
    eprintln!(
        "[audio-prepare-benchmark] input={} out={} format={} overlap={}",
        options.input.display(),
        options.out_dir.display(),
        options.output_format,
        options.overlap_sec
    );

    let legacy = run_legacy(&options)?;
    let prepared = run_prepared(&options)?;
    let speedup_x = legacy.wall_clock_sec / prepared.wall_clock_sec.max(0.001);
    let faster_strategy = if prepared.wall_clock_sec <= legacy.wall_clock_sec {
        prepared.label.clone()
    } else {
        legacy.label.clone()
    };
    let report = PrepareBenchReport {
        input: options.input.to_string_lossy().to_string(),
        output_format: options.output_format,
        overlap_sec: options.overlap_sec,
        workers: options.workers,
        legacy,
        prepared,
        speedup_x,
        faster_strategy,
    };
    let report_path = options.out_dir.join("audio-prepare-benchmark.json");
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    eprintln!(
        "[audio-prepare-benchmark] single-pass={:.3}s parallel={:.3}s speedup={:.2}x faster={} report={}",
        report.legacy.wall_clock_sec,
        report.prepared.wall_clock_sec,
        report.speedup_x,
        report.faster_strategy,
        report_path.display()
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
