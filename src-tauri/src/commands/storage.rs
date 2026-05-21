use std::fs;
use std::path::{Path, PathBuf};
use tauri::command;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

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
}
