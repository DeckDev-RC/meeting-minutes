use crate::models::meeting::MeetingMetadata;
use chrono::{DateTime, Utc};
use std::fs;
use std::path::Path;
use std::time::SystemTime;
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
    metadata.duration_sec = parse_duration_from_ffmpeg_stderr(stderr);

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

pub(super) fn media_metadata_from_filesystem(input_path: &str) -> MeetingMetadata {
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
