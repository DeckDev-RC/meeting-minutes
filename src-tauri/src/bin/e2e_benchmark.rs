use meeting_minutes_lib::benchmark::{
    compute_benchmark_speed, render_benchmark_markdown, BenchmarkReport, BenchmarkStage,
};
use meeting_minutes_lib::commands::audio::{
    audio_codec_args, parse_duration_from_ffmpeg_stderr, parse_silencedetect, plan_smart_chunks,
    validate_chunk_output_format,
};
use meeting_minutes_lib::commands::diarize::{
    align_speaker_turns_to_transcription, build_diarization_telemetry, diarization_asset_paths,
    diarization_mode_label, diarize_audio_turns_modern_cpu, diarize_audio_with_modern_cpu,
    diarize_with_mode_report, ensure_diarization_assets_in_dir, merge_selective_refinement,
    normalize_diarization_threads, select_suspicious_refinement_windows, DiarizationBackend,
    DiarizationMode, DiarizationTelemetry, RefinementWindow,
};
use meeting_minutes_lib::commands::generate::{
    extract_chunk_facts_with_client, generate_ata_from_facts_with_client,
};
use meeting_minutes_lib::commands::transcribe::transcribe_chunk_with_client;
use meeting_minutes_lib::models::audio::{ExportedChunk, SmartChunkOptions};
use meeting_minutes_lib::models::transcription::{
    DiarizedResult, DiarizedSegment, MeetingChunkInsights, TranscriptionSegment,
};
use serde_json::Value;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

#[derive(Debug)]
struct CliOptions {
    input: PathBuf,
    out_dir: PathBuf,
    source: String,
    id: String,
    ffmpeg: PathBuf,
    config: PathBuf,
    app_data_dir: PathBuf,
    transcribe_concurrency: usize,
    facts_concurrency: usize,
    expected_speakers: Option<i32>,
    diarization_mode: DiarizationMode,
    diarization_threads: i32,
}

#[derive(Debug)]
struct ApiKeys {
    groq: String,
    gemini: String,
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn parse_positive_usize(value: Option<String>, fallback: usize) -> usize {
    value
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn parse_optional_i32(value: Option<String>) -> Option<i32> {
    value.and_then(|value| value.parse::<i32>().ok())
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

fn default_config_path() -> Result<PathBuf, String> {
    let appdata = std::env::var("APPDATA").map_err(|_| "APPDATA is not set".to_string())?;
    Ok(PathBuf::from(appdata)
        .join("com.agregar.meeting-minutes")
        .join("config.json"))
}

fn default_app_data_dir() -> Result<PathBuf, String> {
    let appdata = std::env::var("APPDATA").map_err(|_| "APPDATA is not set".to_string())?;
    Ok(PathBuf::from(appdata).join("com.agregar.meeting-minutes"))
}

fn parse_cli() -> Result<CliOptions, String> {
    let args = std::env::args().collect::<Vec<_>>();
    let project_root = default_project_root()?;
    let input = arg_value(&args, "--input")
        .map(PathBuf::from)
        .ok_or_else(|| "Use --input <audio-or-video-path>".to_string())?;
    let id = arg_value(&args, "--id")
        .unwrap_or_else(|| format!("e2e-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S")));
    let out_dir = arg_value(&args, "--out-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| project_root.join("benchmarks").join("runs").join(&id));
    let source = arg_value(&args, "--source").unwrap_or_else(|| "local audio".to_string());
    let ffmpeg = arg_value(&args, "--ffmpeg")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_ffmpeg_path(&project_root));
    let config = arg_value(&args, "--config")
        .map(PathBuf::from)
        .unwrap_or(default_config_path()?);
    let app_data_dir = arg_value(&args, "--app-data-dir")
        .map(PathBuf::from)
        .unwrap_or(default_app_data_dir()?);
    let expected_speakers =
        arg_value(&args, "--expected-speakers").and_then(|value| value.parse::<i32>().ok());
    let diarization_mode = DiarizationMode::from_option(arg_value(&args, "--diarization-mode"));
    let available_threads = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(2);
    let diarization_threads = normalize_diarization_threads(
        parse_optional_i32(arg_value(&args, "--diarization-threads")),
        available_threads,
    );

    Ok(CliOptions {
        input,
        out_dir,
        source,
        id,
        ffmpeg,
        config,
        app_data_dir,
        transcribe_concurrency: parse_positive_usize(
            arg_value(&args, "--transcribe-concurrency"),
            3,
        ),
        facts_concurrency: parse_positive_usize(arg_value(&args, "--facts-concurrency"), 2),
        expected_speakers,
        diarization_mode,
        diarization_threads,
    })
}

fn load_api_keys(config_path: &Path) -> Result<ApiKeys, String> {
    let raw = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read config at {}: {e}", config_path.display()))?;
    let value = serde_json::from_str::<Value>(&raw).map_err(|e| e.to_string())?;
    let groq = value
        .get("groq_api_key")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let gemini = value
        .get("gemini_api_key")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if groq.trim().is_empty() {
        return Err("Groq API key is missing in config".to_string());
    }
    if gemini.trim().is_empty() {
        return Err("Gemini API key is missing in config".to_string());
    }

    Ok(ApiKeys { groq, gemini })
}

fn run_ffmpeg(ffmpeg: &Path, args: &[String]) -> Result<String, String> {
    let output = Command::new(ffmpeg)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run ffmpeg at {}: {e}", ffmpeg.display()))?;
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    if !output.status.success() {
        let details = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(format!("ffmpeg failed: {details}"));
    }

    Ok(stderr)
}

fn measure_duration(ffmpeg: &Path, input_path: &Path) -> Result<f64, String> {
    let stderr = run_ffmpeg(
        ffmpeg,
        &[
            "-i".to_string(),
            input_path.to_string_lossy().to_string(),
            "-f".to_string(),
            "null".to_string(),
            "-".to_string(),
        ],
    )?;

    parse_duration_from_ffmpeg_stderr(&stderr)
        .ok_or_else(|| format!("Could not measure duration for {}", input_path.display()))
}

fn extract_audio(ffmpeg: &Path, input_path: &Path, output_path: &Path) -> Result<f64, String> {
    run_ffmpeg(
        ffmpeg,
        &[
            "-i".to_string(),
            input_path.to_string_lossy().to_string(),
            "-vn".to_string(),
            "-ar".to_string(),
            "16000".to_string(),
            "-ac".to_string(),
            "1".to_string(),
            "-c:a".to_string(),
            "pcm_s16le".to_string(),
            "-y".to_string(),
            output_path.to_string_lossy().to_string(),
        ],
    )?;
    measure_duration(ffmpeg, output_path)
}

fn detect_silences(
    ffmpeg: &Path,
    input_path: &Path,
    noise_db: f64,
    min_duration_sec: f64,
) -> Result<Vec<meeting_minutes_lib::models::audio::SilenceRange>, String> {
    let filter = format!("silencedetect=noise={}dB:d={}", noise_db, min_duration_sec);
    let stderr = run_ffmpeg(
        ffmpeg,
        &[
            "-i".to_string(),
            input_path.to_string_lossy().to_string(),
            "-af".to_string(),
            filter,
            "-f".to_string(),
            "null".to_string(),
            "-".to_string(),
        ],
    )?;

    Ok(parse_silencedetect(&stderr))
}

fn create_smart_chunks(
    ffmpeg: &Path,
    input_path: &Path,
    output_dir: &Path,
    duration_sec: f64,
    options: &SmartChunkOptions,
) -> Result<Vec<ExportedChunk>, String> {
    std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;
    let silences = detect_silences(
        ffmpeg,
        input_path,
        options.silence_noise_db,
        options.silence_min_duration_sec,
    )
    .unwrap_or_default();
    let plans = plan_smart_chunks(
        duration_sec,
        &silences,
        options.target_sec,
        options.min_sec,
        options.max_sec,
        options.overlap_sec,
    );
    let output_format = validate_chunk_output_format(&options.output_format)?;
    let codec_args = audio_codec_args(&output_format);
    let mut chunks = Vec::new();

    for plan in plans {
        let chunk_duration = plan.end_sec - plan.start_sec;
        let audio_path = output_dir.join(format!("chunk_{:03}.{}", plan.index, output_format));
        let mut args = vec![
            "-ss".to_string(),
            format!("{:.3}", plan.start_sec),
            "-i".to_string(),
            input_path.to_string_lossy().to_string(),
            "-t".to_string(),
            format!("{:.3}", chunk_duration),
            "-ar".to_string(),
            "16000".to_string(),
            "-ac".to_string(),
            "1".to_string(),
        ];
        args.extend(codec_args.clone());
        args.extend(["-y".to_string(), audio_path.to_string_lossy().to_string()]);
        run_ffmpeg(ffmpeg, &args)?;

        chunks.push(ExportedChunk {
            index: plan.index,
            audio_path: audio_path.to_string_lossy().to_string(),
            start_sec: plan.start_sec,
            end_sec: plan.end_sec,
            offset_sec: plan.offset_sec,
            duration_sec: chunk_duration,
        });
    }

    Ok(chunks)
}

fn export_refinement_windows_with_ffmpeg(
    ffmpeg: &Path,
    windows: &[RefinementWindow],
    output_dir: &Path,
) -> Result<Vec<ExportedChunk>, String> {
    std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;
    let mut exported = Vec::new();

    for (index, window) in windows.iter().enumerate() {
        let duration = window.duration_sec();
        if duration <= 0.0 {
            continue;
        }

        let audio_path = output_dir.join(format!("refine_{index:03}.wav"));
        let local_start = (window.start_sec - window.chunk.offset_sec).max(0.0);
        run_ffmpeg(
            ffmpeg,
            &[
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
            ],
        )?;

        exported.push(ExportedChunk {
            index,
            audio_path: audio_path.to_string_lossy().to_string(),
            start_sec: window.start_sec,
            end_sec: window.end_sec,
            offset_sec: window.start_sec,
            duration_sec: duration,
        });
    }

    Ok(exported)
}

async fn transcribe_chunks_concurrently(
    chunks: Vec<ExportedChunk>,
    api_key: String,
    concurrency: usize,
) -> Result<Vec<TranscriptionSegment>, String> {
    if chunks.is_empty() {
        return Ok(Vec::new());
    }

    let queue = std::sync::Arc::new(Mutex::new(VecDeque::from(chunks)));
    let client = Arc::new(reqwest::Client::new());
    let worker_count = concurrency.max(1).min(queue.lock().await.len());
    let mut handles = Vec::new();

    for _ in 0..worker_count {
        let queue = queue.clone();
        let api_key = api_key.clone();
        let client = client.clone();
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

                eprintln!(
                    "[transcribe] chunk {} ({:.1}s)",
                    chunk.index, chunk.duration_sec
                );
                let segments = transcribe_chunk_with_client(
                    &client,
                    chunk.audio_path.clone(),
                    api_key.clone(),
                    chunk.offset_sec,
                )
                .await?;
                local_results.push((chunk.index, segments));
            }
            Ok::<Vec<(usize, Vec<TranscriptionSegment>)>, String>(local_results)
        }));
    }

    let mut by_chunk = Vec::new();
    for handle in handles {
        let local = handle
            .await
            .map_err(|e| format!("Transcription worker failed: {e}"))??;
        by_chunk.extend(local);
    }

    by_chunk.sort_by_key(|(index, _)| *index);
    Ok(by_chunk
        .into_iter()
        .flat_map(|(_, segments)| segments)
        .enumerate()
        .map(|(id, mut segment)| {
            segment.id = id as i32;
            segment
        })
        .collect())
}

fn overlapping_diarized_segments(
    chunk: &ExportedChunk,
    diarized_segments: &[DiarizedSegment],
    raw_segments: &[TranscriptionSegment],
) -> Value {
    let diarized = diarized_segments
        .iter()
        .filter(|segment| segment.end >= chunk.start_sec && segment.start <= chunk.end_sec)
        .cloned()
        .collect::<Vec<_>>();

    if !diarized.is_empty() {
        serde_json::to_value(diarized).unwrap_or(Value::Array(Vec::new()))
    } else {
        let raw = raw_segments
            .iter()
            .filter(|segment| segment.end >= chunk.start_sec && segment.start <= chunk.end_sec)
            .cloned()
            .collect::<Vec<_>>();
        serde_json::to_value(raw).unwrap_or(Value::Array(Vec::new()))
    }
}

async fn extract_facts_concurrently(
    chunks: Vec<ExportedChunk>,
    diarized: DiarizedResult,
    raw_segments: Vec<TranscriptionSegment>,
    api_key: String,
    concurrency: usize,
) -> Result<Vec<MeetingChunkInsights>, String> {
    if chunks.is_empty() {
        return Ok(Vec::new());
    }

    let queue = std::sync::Arc::new(Mutex::new(VecDeque::from(chunks)));
    let diarized = std::sync::Arc::new(diarized);
    let raw_segments = std::sync::Arc::new(raw_segments);
    let client = Arc::new(reqwest::Client::new());
    let worker_count = concurrency.max(1).min(queue.lock().await.len());
    let mut handles = Vec::new();

    for _ in 0..worker_count {
        let queue = queue.clone();
        let diarized = diarized.clone();
        let raw_segments = raw_segments.clone();
        let api_key = api_key.clone();
        let client = client.clone();
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

                eprintln!("[facts] chunk {}", chunk.index);
                let source_segments =
                    overlapping_diarized_segments(&chunk, &diarized.segments, &raw_segments);
                let insights = extract_chunk_facts_with_client(
                    &client,
                    chunk.index,
                    chunk.start_sec,
                    chunk.end_sec,
                    source_segments.to_string(),
                    api_key.clone(),
                    None,
                )
                .await?;
                local_results.push(insights);
            }
            Ok::<Vec<MeetingChunkInsights>, String>(local_results)
        }));
    }

    let mut facts = Vec::new();
    for handle in handles {
        facts.extend(
            handle
                .await
                .map_err(|e| format!("Facts worker failed: {e}"))??,
        );
    }
    facts.sort_by_key(|item| item.chunk_index);
    Ok(facts)
}

fn stage_push(stages: &mut Vec<BenchmarkStage>, name: &str, started: Instant) {
    stages.push(BenchmarkStage {
        name: name.to_string(),
        duration_sec: started.elapsed().as_secs_f64(),
    });
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let options = parse_cli()?;
    std::fs::create_dir_all(&options.out_dir).map_err(|e| e.to_string())?;
    let keys = load_api_keys(&options.config)?;
    let mut stages = Vec::new();
    let total_started = Instant::now();

    eprintln!("[benchmark] input: {}", options.input.display());
    eprintln!("[benchmark] output: {}", options.out_dir.display());

    let normalized_audio = options.out_dir.join("normalized-audio.wav");
    let started = Instant::now();
    let audio_duration_sec = extract_audio(&options.ffmpeg, &options.input, &normalized_audio)?;
    stage_push(&mut stages, "extract_audio", started);
    eprintln!("[benchmark] audio duration: {:.2}s", audio_duration_sec);

    let started = Instant::now();
    let mut chunk_options = SmartChunkOptions::default();
    chunk_options.output_format = "flac".to_string();
    let chunks = create_smart_chunks(
        &options.ffmpeg,
        &normalized_audio,
        &options.out_dir.join("chunks"),
        audio_duration_sec,
        &chunk_options,
    )?;
    stage_push(&mut stages, "create_smart_chunks", started);
    if chunks.is_empty() {
        return Err("No chunks were created".to_string());
    }
    write_json(&options.out_dir.join("chunks.json"), &chunks)?;
    eprintln!("[benchmark] chunks: {}", chunks.len());

    let diarization_started = Instant::now();
    let speculative_turns = if matches!(
        options.diarization_mode,
        DiarizationMode::Auto | DiarizationMode::ModernCpu
    ) {
        let audio_path = normalized_audio.to_string_lossy().to_string();
        let expected_speakers = options.expected_speakers;
        Some(tokio::spawn(async move {
            diarize_audio_turns_modern_cpu(audio_path, expected_speakers).await
        }))
    } else {
        None
    };

    let started = Instant::now();
    let transcript_segments =
        transcribe_chunks_concurrently(chunks.clone(), keys.groq, options.transcribe_concurrency)
            .await?;
    stage_push(&mut stages, "transcribe", started);
    write_json(
        &options.out_dir.join("transcription-segments.json"),
        &transcript_segments,
    )?;
    eprintln!(
        "[benchmark] transcript segments: {}",
        transcript_segments.len()
    );

    let mut notes = Vec::new();

    let facts_started = Instant::now();
    let facts_future = extract_facts_concurrently(
        chunks.clone(),
        DiarizedResult {
            speakers: Vec::new(),
            segments: Vec::new(),
        },
        transcript_segments.clone(),
        keys.gemini.clone(),
        options.facts_concurrency,
    );

    let diarized_future = async {
        let segments_json =
            serde_json::to_string(&transcript_segments).map_err(|e| e.to_string())?;
        if let Some(handle) = speculative_turns {
            match handle
                .await
                .map_err(|e| format!("Speculative diarization task failed: {e}"))?
            {
                Ok(turns) if !turns.is_empty() => {
                    let turns_json = serde_json::to_string(&turns).map_err(|e| e.to_string())?;
                    let base = align_speaker_turns_to_transcription(segments_json, turns_json)?;
                    let windows = select_suspicious_refinement_windows(
                        &transcript_segments,
                        &turns,
                        &chunks,
                        1,
                    );
                    if windows.is_empty() {
                        let telemetry = build_diarization_telemetry(
                            options.diarization_mode,
                            DiarizationBackend::ModernCpu,
                            None,
                            diarization_started.elapsed().as_secs_f64(),
                            &base,
                        );
                        return Ok((base, telemetry));
                    }
                    let selected_chunks = export_refinement_windows_with_ffmpeg(
                        &options.ffmpeg,
                        &windows,
                        &options.out_dir.join("refinement-windows"),
                    )?;
                    if selected_chunks.is_empty() {
                        let telemetry = build_diarization_telemetry(
                            options.diarization_mode,
                            DiarizationBackend::ModernCpu,
                            None,
                            diarization_started.elapsed().as_secs_f64(),
                            &base,
                        );
                        return Ok((base, telemetry));
                    }

                    let local_expected_speakers =
                        options.expected_speakers.map(|value| value.clamp(2, 3));
                    let mut refined_parts = Vec::new();
                    for window_chunk in &selected_chunks {
                        let window_segments =
                            meeting_minutes_lib::commands::diarize::segments_for_chunk(
                                &transcript_segments,
                                window_chunk,
                            );
                        if window_segments.is_empty() {
                            continue;
                        }
                        if let Ok(local_refined) = diarize_audio_with_modern_cpu(
                            window_chunk.audio_path.clone(),
                            &window_segments,
                            local_expected_speakers,
                        )
                        .await
                        {
                            refined_parts.push(
                                meeting_minutes_lib::commands::diarize::shift_diarized_result(
                                    local_refined,
                                    window_chunk.offset_sec,
                                ),
                            );
                        }
                    }
                    let refined =
                        meeting_minutes_lib::commands::diarize::stitch_diarized_chunk_results(
                            refined_parts,
                            1.0,
                        );

                    let merged = merge_selective_refinement(base, refined, &selected_chunks);
                    let telemetry = build_diarization_telemetry(
                        options.diarization_mode,
                        DiarizationBackend::ModernCpu,
                        None,
                        diarization_started.elapsed().as_secs_f64(),
                        &merged,
                    );

                    return Ok((merged, telemetry));
                }
                Ok(_) if options.diarization_mode == DiarizationMode::ModernCpu => {
                    return Err("Modern CPU diarization returned no speaker turns".to_string());
                }
                Err(error) if options.diarization_mode == DiarizationMode::ModernCpu => {
                    return Err(error);
                }
                _ => {}
            }
        }

        let assets = if matches!(
            options.diarization_mode,
            DiarizationMode::Fast
                | DiarizationMode::Auto
                | DiarizationMode::ModernCpu
                | DiarizationMode::ModernCpuChunked
                | DiarizationMode::Pyannote
        ) {
            diarization_asset_paths(&options.app_data_dir)
        } else {
            ensure_diarization_assets_in_dir(&options.app_data_dir).await?
        };
        let run = diarize_with_mode_report(
            normalized_audio.to_string_lossy().to_string(),
            transcript_segments.clone(),
            Some(chunks.clone()),
            assets,
            options.expected_speakers,
            options.diarization_mode,
            options.diarization_threads,
        )
        .await?;
        Ok::<(DiarizedResult, DiarizationTelemetry), String>((run.result, run.telemetry))
    };

    let ((diarized, diarization_telemetry), facts) =
        tokio::try_join!(diarized_future, facts_future)?;
    stages.push(BenchmarkStage {
        name: "diarize_speculative".to_string(),
        duration_sec: diarization_started.elapsed().as_secs_f64(),
    });
    stages.push(BenchmarkStage {
        name: "extract_facts_parallel".to_string(),
        duration_sec: facts_started.elapsed().as_secs_f64(),
    });
    write_json(
        &options.out_dir.join("diarized-transcription.json"),
        &diarized,
    )?;
    write_json(&options.out_dir.join("meeting-facts.json"), &facts)?;
    eprintln!(
        "[benchmark] diarized segments: {}, speakers: {}",
        diarized.segments.len(),
        diarized.speakers.len()
    );
    eprintln!("[benchmark] fact chunks: {}", facts.len());

    let started = Instant::now();
    let client = reqwest::Client::new();
    let minutes_html = generate_ata_from_facts_with_client(
        &client,
        serde_json::to_string(&diarized).map_err(|e| e.to_string())?,
        serde_json::to_string(&facts).map_err(|e| e.to_string())?,
        keys.gemini,
        None,
        None,
    )
    .await?;
    stage_push(&mut stages, "generate_minutes", started);
    let minutes_path = options.out_dir.join("minutes.html");
    std::fs::write(&minutes_path, minutes_html)
        .map_err(|e| format!("Failed to write {}: {e}", minutes_path.display()))?;

    let wall_clock_sec = total_started.elapsed().as_secs_f64();
    let speed = compute_benchmark_speed(audio_duration_sec, wall_clock_sec)?;
    notes.push(
        "Qualidade factual: n/a neste run porque nao ha gabarito manual anexado.".to_string(),
    );
    notes.push("Dataset AMI ES2002a e reuniao real em ingles; este run mede E2E/velocidade do pipeline, nao precisao PT-BR.".to_string());

    let decision_count = facts.iter().map(|item| item.decisions.len()).sum();
    let action_count = facts.iter().map(|item| item.actions.len()).sum();
    let output_files = vec![
        options.out_dir.join("benchmark-report.json"),
        options.out_dir.join("benchmark-report.md"),
        options.out_dir.join("minutes.html"),
        options.out_dir.join("transcription-segments.json"),
        options.out_dir.join("diarized-transcription.json"),
        options.out_dir.join("meeting-facts.json"),
        options.out_dir.join("chunks.json"),
    ]
    .into_iter()
    .map(|path| path.to_string_lossy().to_string())
    .collect::<Vec<_>>();

    let report = BenchmarkReport {
        id: options.id,
        source: options.source,
        audio_path: options.input.to_string_lossy().to_string(),
        audio_duration_sec,
        wall_clock_sec,
        realtime_factor: speed.realtime_factor,
        speed_x: speed.speed_x,
        chunk_count: chunks.len(),
        transcript_segment_count: transcript_segments.len(),
        diarized_segment_count: diarized.segments.len(),
        speaker_count: diarized.speakers.len(),
        fact_chunk_count: facts.len(),
        decision_count,
        action_count,
        diarization_mode: diarization_mode_label(options.diarization_mode).to_string(),
        diarization_threads: options.diarization_threads,
        diarization_backend_used: diarization_telemetry.backend_used,
        diarization_fallback_reason: diarization_telemetry.fallback_reason,
        diarization_wall_clock_sec: diarization_telemetry.wall_clock_sec,
        stages,
        output_files,
        notes,
    };

    write_json(&options.out_dir.join("benchmark-report.json"), &report)?;
    std::fs::write(
        options.out_dir.join("benchmark-report.md"),
        render_benchmark_markdown(&report),
    )
    .map_err(|e| e.to_string())?;

    eprintln!(
        "[benchmark] done: {:.2}s wall, RTF {:.3}, {:.2}x",
        report.wall_clock_sec, report.realtime_factor, report.speed_x
    );
    Ok(())
}
