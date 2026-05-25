use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use tauri::command;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use zip::write::SimpleFileOptions;

fn safe_path_component(value: &str) -> String {
    let sanitized = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "meeting".to_string()
    } else {
        sanitized
    }
}

pub fn processing_work_dir_path(app_data_dir: &Path, meeting_id: &str) -> PathBuf {
    app_data_dir
        .join("processing")
        .join(safe_path_component(meeting_id))
}

fn write_text_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    fs::write(path, content).map_err(|e| e.to_string())
}

fn is_sensitive_json_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace(['_', '-'], "");
    key.contains("apikey")
        || key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || matches!(
            key.as_str(),
            "groq" | "gemini" | "deepgramapikey" | "cloudflareapitoken"
        )
}

fn mask_json_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                if is_sensitive_json_key(key) && value.is_string() {
                    *value = serde_json::Value::String("***REDACTED***".to_string());
                } else {
                    mask_json_value(value);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                mask_json_value(item);
            }
        }
        _ => {}
    }
}

fn mask_token_after_prefix(value: String, prefix: &str, replacement: &str) -> String {
    let mut output = value;
    let mut offset = 0usize;
    while let Some(relative) = output[offset..].find(prefix) {
        let start = offset + relative;
        let token_start = start + prefix.len();
        let token_end = output[token_start..]
            .find(|ch: char| {
                !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' || ch == ':')
            })
            .map(|relative_end| token_start + relative_end)
            .unwrap_or(output.len());
        output.replace_range(start..token_end, replacement);
        offset = start + replacement.len();
    }
    output
}

fn mask_prefixed_secret(value: String, prefix: &str) -> String {
    mask_token_after_prefix(value, prefix, &format!("{prefix}***REDACTED***"))
}

fn mask_secrets(content: &str) -> String {
    let mut masked = if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(content) {
        mask_json_value(&mut value);
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| content.to_string())
    } else {
        content.to_string()
    };

    for prefix in [
        "gsk_", "cfat_", "xai-", "sk-ant-", "sk-proj-", "sk-", "AIza",
    ] {
        masked = mask_prefixed_secret(masked, prefix);
    }
    mask_token_after_prefix(masked, "Bearer ", "Bearer ***REDACTED***")
}

fn diagnostics_file_allowed(path: &Path) -> bool {
    if path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("db.sqlite"))
    {
        return false;
    }
    matches!(
        path.extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase),
        Some(ext) if matches!(ext.as_str(), "json" | "log" | "txt" | "md")
    )
}

fn collect_diagnostics_files(root: &Path, values: &mut Vec<PathBuf>) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_diagnostics_files(&path, values)?;
        } else if diagnostics_file_allowed(&path) {
            values.push(path);
        }
    }
    Ok(())
}

fn build_diagnostics_zip(app_data_dir: &Path, meeting_id: Option<&str>) -> Result<Vec<u8>, String> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let summary = serde_json::json!({
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "app": "meeting-minutes",
        "meetingId": meeting_id,
        "appDataDir": app_data_dir.to_string_lossy(),
        "note": "Arquivos textuais sao mascarados antes de entrar neste pacote."
    });
    zip.start_file("diagnostics/summary.json", options)
        .map_err(|e| e.to_string())?;
    zip.write_all(
        serde_json::to_string_pretty(&summary)
            .map_err(|e| e.to_string())?
            .as_bytes(),
    )
    .map_err(|e| e.to_string())?;

    let mut files = Vec::new();
    collect_diagnostics_files(&app_data_dir.join("processing"), &mut files)?;
    files.sort();

    let mut total_bytes = 0usize;
    for path in files {
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.len() > 2 * 1024 * 1024 || total_bytes > 20 * 1024 * 1024 {
            continue;
        }
        let relative = path
            .strip_prefix(app_data_dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(meeting_id) = meeting_id {
            if !relative.contains(meeting_id) {
                continue;
            }
        }
        let content = fs::read_to_string(&path).unwrap_or_default();
        let masked = mask_secrets(&content);
        total_bytes += masked.len();
        zip.start_file(format!("diagnostics/{relative}"), options)
            .map_err(|e| e.to_string())?;
        zip.write_all(masked.as_bytes())
            .map_err(|e| e.to_string())?;
    }

    let cursor = zip.finish().map_err(|e| e.to_string())?;
    Ok(cursor.into_inner())
}

#[command]
pub async fn save_pdf(
    app: tauri::AppHandle,
    pdf_bytes: Vec<u8>,
    suggested_name: String,
) -> Result<String, String> {
    let file_path = app
        .dialog()
        .file()
        .set_file_name(&suggested_name)
        .add_filter("PDF", &["pdf"])
        .blocking_save_file();

    match file_path {
        Some(path) => {
            let path_str = path.to_string();
            fs::write(&path_str, &pdf_bytes).map_err(|e| e.to_string())?;
            Ok(path_str)
        }
        None => Err("Save cancelled".to_string()),
    }
}

#[command]
pub async fn save_html(
    app: tauri::AppHandle,
    html_content: String,
    suggested_name: String,
) -> Result<String, String> {
    let file_path = app
        .dialog()
        .file()
        .set_file_name(&suggested_name)
        .add_filter("HTML", &["html", "htm"])
        .blocking_save_file();

    match file_path {
        Some(path) => {
            let path_str = path.to_string();
            fs::write(&path_str, html_content).map_err(|e| e.to_string())?;
            Ok(path_str)
        }
        None => Err("Save cancelled".to_string()),
    }
}

#[command]
pub async fn export_diagnostics(
    app: tauri::AppHandle,
    meeting_id: Option<String>,
) -> Result<String, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {e}"))?;
    let zip_bytes = build_diagnostics_zip(&app_data_dir, meeting_id.as_deref())?;
    let file_path = app
        .dialog()
        .file()
        .set_file_name("meeting-minutes-diagnostics.zip")
        .add_filter("ZIP", &["zip"])
        .blocking_save_file();

    match file_path {
        Some(path) => {
            let path_str = path.to_string();
            fs::write(&path_str, zip_bytes).map_err(|e| e.to_string())?;
            Ok(path_str)
        }
        None => Err("Save cancelled".to_string()),
    }
}

#[command]
pub async fn open_folder(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    let folder = if p.is_file() {
        p.parent()
            .map(|pp| pp.to_string_lossy().to_string())
            .unwrap_or(path.clone())
    } else {
        path.clone()
    };

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&folder)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&folder)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[command]
pub async fn resolve_processing_work_dir(
    app: tauri::AppHandle,
    meeting_id: String,
) -> Result<String, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {e}"))?;
    let path = processing_work_dir_path(&app_data_dir, &meeting_id);
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[command]
pub async fn save_benchmark_run(path: String, content: String) -> Result<String, String> {
    let path = Path::new(&path);
    write_text_file(path, &content)?;
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processing_work_dir_stays_under_app_data_and_sanitizes_id() {
        let app_data_dir = std::env::temp_dir().join("meeting-minutes-app-data");

        let path = processing_work_dir_path(&app_data_dir, r"..\shared/meeting:1");

        assert_eq!(path.parent().unwrap(), app_data_dir.join("processing"));
        assert_eq!(path.file_name().unwrap(), "___shared_meeting_1");
    }

    #[test]
    fn processing_work_dir_uses_placeholder_for_empty_id() {
        let app_data_dir = std::env::temp_dir().join("meeting-minutes-app-data");

        let path = processing_work_dir_path(&app_data_dir, "   ");

        assert_eq!(path.file_name().unwrap(), "meeting");
    }

    #[test]
    fn write_text_file_creates_parent_directories_and_file() {
        let dir =
            std::env::temp_dir().join(format!("meeting-minutes-storage-{}", uuid::Uuid::new_v4()));
        let path = dir.join("reports").join("run.json");

        write_text_file(&path, "{\"version\":1}").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"version\":1}");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn masks_known_api_secret_patterns_in_diagnostics() {
        let masked = mask_secrets(
            r#"{"groq_api_key":"gsk_live_secret","cloudflareApiToken":"cfat_live_secret","note":"Bearer abc123","gemini":"AIzaSySecret"}"#,
        );

        assert!(!masked.contains("gsk_live_secret"));
        assert!(!masked.contains("cfat_live_secret"));
        assert!(!masked.contains("abc123"));
        assert!(!masked.contains("AIzaSySecret"));
        assert!(masked.contains("REDACTED"));
    }
}
