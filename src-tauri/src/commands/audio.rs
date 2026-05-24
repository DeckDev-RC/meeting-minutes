mod chunks;
mod metadata;

pub use chunks::{
    audio_codec_args, build_extract_audio_args, build_smart_chunk_export_args, parse_silencedetect,
    plan_smart_chunks, select_smart_chunk_export_strategy, validate_chunk_output_format,
    SmartChunkExportStrategy,
};
pub use metadata::{
    apply_recorded_at_source, parse_duration_from_ffmpeg_stderr,
    parse_duration_from_ffprobe_stdout, parse_ffmpeg_media_metadata,
};

use crate::models::audio::{ExportedChunk, PreparedAudio, SilenceRange, SmartChunkOptions};
use crate::models::meeting::MeetingMetadata;
use std::fs;
use std::path::Path;
use tauri::command;
use tauri_plugin_shell::ShellExt;

#[cfg(test)]
use crate::models::audio::ChunkPlan;
use chunks::{
    build_silence_filter, cached_or_detect_silences, export_planned_smart_chunks,
    export_smart_chunks_batch, export_smart_chunks_parallel, run_extract_audio,
    write_global_silence_cache, write_silence_cache,
};
use metadata::media_metadata_from_filesystem;

#[cfg(test)]
use chunks::{
    build_parallel_chunk_export_args, silence_cache_file_name_for_fingerprint,
    source_audio_fingerprint, FINGERPRINT_SAMPLE_BYTES,
};
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
            if parsed.duration_sec.is_some() {
                metadata.duration_sec = parsed.duration_sec;
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
    let silence_filter =
        build_silence_filter(defaults.silence_noise_db, defaults.silence_min_duration_sec);
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

    let silences = cached_or_detect_silences(
        app.clone(),
        input_path.clone(),
        None,
        opts.silence_noise_db,
        opts.silence_min_duration_sec,
    )
    .await;

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

fn should_use_parallel_prepare(opts: &SmartChunkOptions) -> bool {
    opts.prepare_strategy.as_deref() == Some("parallel")
}

async fn prepare_audio_and_chunks_single_pass(
    app: tauri::AppHandle,
    input_path: String,
    audio_output_path: String,
    chunk_output_dir: String,
    opts: SmartChunkOptions,
) -> Result<PreparedAudio, String> {
    let silence_filter = build_silence_filter(opts.silence_noise_db, opts.silence_min_duration_sec);
    let stderr = run_extract_audio(
        app.clone(),
        input_path.clone(),
        audio_output_path.clone(),
        Some(silence_filter),
    )
    .await?;
    let silences = parse_silencedetect(&stderr);
    let duration = if let Some(duration) = parse_duration_from_ffmpeg_stderr(&stderr) {
        duration
    } else {
        get_duration(&app, &audio_output_path).await?
    };

    write_silence_cache(
        &audio_output_path,
        opts.silence_noise_db,
        opts.silence_min_duration_sec,
        silences.clone(),
    )
    .await;
    write_global_silence_cache(
        &app,
        &input_path,
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
    let chunks = export_planned_smart_chunks(
        app,
        audio_output_path,
        chunk_output_dir,
        output_format,
        plans,
    )
    .await?;

    Ok(PreparedAudio {
        duration_sec: duration,
        chunks,
    })
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

    if !should_use_parallel_prepare(&opts) {
        return prepare_audio_and_chunks_single_pass(
            app,
            input_path,
            audio_output_path,
            chunk_output_dir,
            opts,
        )
        .await;
    }

    let normalize_future = run_extract_audio(
        app.clone(),
        input_path.clone(),
        audio_output_path.clone(),
        None,
    );

    let audio_cache_path = audio_output_path.clone();
    let chunk_future = async {
        let duration_future = get_duration(&app, &input_path);
        let silence_future = cached_or_detect_silences(
            app.clone(),
            input_path.clone(),
            Some(audio_cache_path.clone()),
            opts.silence_noise_db,
            opts.silence_min_duration_sec,
        );
        let (duration, silences) = tokio::join!(duration_future, silence_future);
        let duration = duration?;

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
mod tests;
