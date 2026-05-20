use std::fs;
use std::path::Path;
use tauri::command;
use tauri_plugin_dialog::DialogExt;

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
pub async fn save_benchmark_run(path: String, content: String) -> Result<String, String> {
    let path = Path::new(&path);
    write_text_file(path, &content)?;
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
