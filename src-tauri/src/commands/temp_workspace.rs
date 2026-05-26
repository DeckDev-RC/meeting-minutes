use std::path::{Path, PathBuf};

pub(crate) async fn create_temp_workspace(prefix: &str) -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|e| format!("Failed to create {}: {e}", path.display()))?;
    Ok(path)
}

pub(crate) async fn cleanup_temp_workspace(path: impl AsRef<Path>) {
    let path = path.as_ref();
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => eprintln!("Failed to cleanup temp workspace {}: {err}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::{cleanup_temp_workspace, create_temp_workspace};

    #[tokio::test]
    async fn cleanup_temp_workspace_removes_created_directory() {
        let path = create_temp_workspace("meeting-minutes-cleanup-test")
            .await
            .unwrap();
        let marker = path.join("marker.txt");
        tokio::fs::write(&marker, "ok").await.unwrap();

        cleanup_temp_workspace(&path).await;

        assert!(!path.exists());
    }
}
