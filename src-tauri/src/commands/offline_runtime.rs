use crate::commands::transcribe::{
    faster_whisper_backend_available, faster_whisper_backend_exists, faster_whisper_backend_paths,
    local_transcription_runtime_root,
};
use crate::HttpClientState;
use futures::StreamExt;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tauri::{command, Manager, State};
use tokio::io::AsyncWriteExt;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineTranscriptionRuntimeStatus {
    pub installed: bool,
    pub faster_whisper_available: bool,
    pub parakeet_available: bool,
    pub root_path: String,
    pub source: String,
    pub version: Option<String>,
    pub size_bytes: u64,
}

fn dir_size_bytes(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }

    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                stack.push(entry.path());
            } else {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

fn runtime_version(root: &Path) -> Option<String> {
    let manifest_path = root.join("runtime-manifest.json");
    let raw = fs::read_to_string(manifest_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value
        .get("version")
        .and_then(|item| item.as_str())
        .map(ToString::to_string)
}

fn runtime_status_for_root(root: PathBuf, source: &str) -> OfflineTranscriptionRuntimeStatus {
    let faster_whisper_available =
        faster_whisper_backend_exists(&faster_whisper_backend_paths(&root));
    OfflineTranscriptionRuntimeStatus {
        installed: root.exists(),
        faster_whisper_available,
        parakeet_available: crate::commands::transcribe::parakeet::parakeet_backend_available(),
        version: runtime_version(&root),
        size_bytes: dir_size_bytes(&root),
        root_path: root.display().to_string(),
        source: source.to_string(),
    }
}

fn app_runtime_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Falha ao localizar AppData: {e}"))?;
    Ok(local_transcription_runtime_root(&app_data_dir))
}

#[command]
pub fn get_offline_transcription_runtime_status(
    app: tauri::AppHandle,
) -> Result<OfflineTranscriptionRuntimeStatus, String> {
    let root = app_runtime_root(&app)?;
    Ok(runtime_status_for_root(root, "app-data"))
}

fn normalize_sha256(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| !item.is_empty())
}

fn is_url(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.starts_with("https://") || normalized.starts_with("http://")
}

async fn source_to_zip(
    client: &reqwest::Client,
    source: &str,
    expected_sha256: Option<String>,
) -> Result<(PathBuf, bool), String> {
    let expected_sha256 = normalize_sha256(expected_sha256);
    let temp_path = std::env::temp_dir().join(format!(
        "meeting-minutes-transcribe-runtime-{}.zip",
        uuid::Uuid::new_v4()
    ));
    let mut hasher = Sha256::new();

    if is_url(source) {
        let response = client
            .get(source.trim())
            .send()
            .await
            .map_err(|e| format!("Falha ao baixar pacote offline: {e}"))?
            .error_for_status()
            .map_err(|e| format!("Download do pacote offline falhou: {e}"))?;
        let mut file = tokio::fs::File::create(&temp_path)
            .await
            .map_err(|e| format!("Falha ao criar {}: {e}", temp_path.display()))?;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Falha ao ler download offline: {e}"))?;
            hasher.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("Falha ao gravar {}: {e}", temp_path.display()))?;
        }
        file.flush()
            .await
            .map_err(|e| format!("Falha ao finalizar {}: {e}", temp_path.display()))?;
    } else {
        let source_path = PathBuf::from(source.trim());
        let mut input = fs::File::open(&source_path)
            .map_err(|e| format!("Falha ao abrir {}: {e}", source_path.display()))?;
        let mut output = fs::File::create(&temp_path)
            .map_err(|e| format!("Falha ao criar {}: {e}", temp_path.display()))?;
        let mut buffer = [0u8; 1024 * 1024];
        loop {
            let read = input
                .read(&mut buffer)
                .map_err(|e| format!("Falha ao ler {}: {e}", source_path.display()))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            output
                .write_all(&buffer[..read])
                .map_err(|e| format!("Falha ao copiar pacote offline: {e}"))?;
        }
        output
            .flush()
            .map_err(|e| format!("Falha ao finalizar {}: {e}", temp_path.display()))?;
    }

    if let Some(expected) = expected_sha256 {
        let actual = format!("{:x}", hasher.finalize());
        if actual != expected {
            let _ = fs::remove_file(&temp_path);
            return Err(format!(
                "Hash SHA-256 do pacote offline nao confere. Esperado {expected}, obtido {actual}."
            ));
        }
    }

    Ok((temp_path, true))
}

fn clear_dir(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_dir_all(path)
            .map_err(|e| format!("Falha ao remover {}: {e}", path.display()))?;
    }
    Ok(())
}

fn extract_zip(zip_path: &Path, destination: &Path) -> Result<(), String> {
    let zip_path = zip_path.to_path_buf();
    let destination = destination.to_path_buf();
    std::thread::spawn(move || {
        clear_dir(&destination)?;
        fs::create_dir_all(&destination)
            .map_err(|e| format!("Falha ao criar {}: {e}", destination.display()))?;
        let file = fs::File::open(&zip_path)
            .map_err(|e| format!("Falha ao abrir {}: {e}", zip_path.display()))?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|e| format!("ZIP offline invalido: {e}"))?;

        for index in 0..archive.len() {
            let mut file = archive
                .by_index(index)
                .map_err(|e| format!("Falha ao ler entrada {index} do ZIP: {e}"))?;
            let Some(enclosed_name) = file.enclosed_name() else {
                return Err(format!(
                    "ZIP offline contem caminho inseguro: {}",
                    file.name()
                ));
            };
            let output_path = destination.join(enclosed_name);

            if file.is_dir() {
                fs::create_dir_all(&output_path)
                    .map_err(|e| format!("Falha ao criar {}: {e}", output_path.display()))?;
                continue;
            }

            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Falha ao criar {}: {e}", parent.display()))?;
            }
            let mut output = fs::File::create(&output_path)
                .map_err(|e| format!("Falha ao criar {}: {e}", output_path.display()))?;
            std::io::copy(&mut file, &mut output)
                .map_err(|e| format!("Falha ao extrair {}: {e}", output_path.display()))?;
        }

        Ok::<(), String>(())
    })
    .join()
    .map_err(|_| "Extracao do runtime offline falhou".to_string())?
}

fn resolve_extracted_root(staging_root: &Path) -> Result<PathBuf, String> {
    if faster_whisper_backend_exists(&faster_whisper_backend_paths(staging_root)) {
        return Ok(staging_root.to_path_buf());
    }

    let entries = fs::read_dir(staging_root)
        .map_err(|e| format!("Falha ao ler {}: {e}", staging_root.display()))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .collect::<Vec<_>>();
    if entries.len() == 1 {
        let candidate = entries[0].path();
        if faster_whisper_backend_exists(&faster_whisper_backend_paths(&candidate)) {
            return Ok(candidate);
        }
    }

    Err("Pacote offline nao contem um runtime faster-whisper valido.".to_string())
}

fn promote_runtime(staging_root: &Path, final_root: &Path) -> Result<(), String> {
    let extracted_root = resolve_extracted_root(staging_root)?;
    let parent = final_root
        .parent()
        .ok_or_else(|| format!("Destino invalido: {}", final_root.display()))?;
    fs::create_dir_all(parent).map_err(|e| format!("Falha ao criar {}: {e}", parent.display()))?;
    let backup_root = final_root.with_extension("previous");
    clear_dir(&backup_root)?;

    if final_root.exists() {
        fs::rename(final_root, &backup_root)
            .map_err(|e| format!("Falha ao preparar atualizacao offline: {e}"))?;
    }

    let install_result = if extracted_root == staging_root {
        fs::rename(staging_root, final_root)
            .map_err(|e| format!("Falha ao instalar runtime offline: {e}"))
    } else {
        fs::rename(&extracted_root, final_root)
            .map_err(|e| format!("Falha ao instalar runtime offline: {e}"))
    };

    if let Err(error) = install_result {
        if backup_root.exists() && !final_root.exists() {
            let _ = fs::rename(&backup_root, final_root);
        }
        return Err(error);
    }

    clear_dir(&backup_root)?;
    Ok(())
}

#[command]
pub async fn install_offline_transcription_runtime(
    app: tauri::AppHandle,
    http: State<'_, HttpClientState>,
    source: String,
    expected_sha256: Option<String>,
) -> Result<OfflineTranscriptionRuntimeStatus, String> {
    if source.trim().is_empty() {
        return Err("Informe uma URL ou um arquivo ZIP do runtime offline.".to_string());
    }

    let final_root = app_runtime_root(&app)?;
    let staging_root = final_root.with_extension(format!("install-{}", uuid::Uuid::new_v4()));
    let (zip_path, cleanup_zip) = source_to_zip(&http.0, &source, expected_sha256).await?;

    let result = (|| {
        extract_zip(&zip_path, &staging_root)?;
        promote_runtime(&staging_root, &final_root)?;
        std::env::set_var("MEETING_MINUTES_TRANSCRIBE_ROOT", &final_root);
        Ok(runtime_status_for_root(final_root, "app-data"))
    })();

    let _ = clear_dir(&staging_root);
    if cleanup_zip {
        let _ = fs::remove_file(&zip_path);
    }

    result
}

#[command]
pub fn remove_offline_transcription_runtime(
    app: tauri::AppHandle,
) -> Result<OfflineTranscriptionRuntimeStatus, String> {
    let root = app_runtime_root(&app)?;
    clear_dir(&root)?;
    if !faster_whisper_backend_available() {
        std::env::remove_var("MEETING_MINUTES_TRANSCRIBE_ROOT");
    }
    Ok(runtime_status_for_root(root, "app-data"))
}
