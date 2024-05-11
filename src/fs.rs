use std::path::PathBuf;

use anyhow::Result;
use tokio::fs::{metadata, read_dir};

/// Find a given file in the current working directory and its parent directories recursively
pub async fn find_file_in_previous_dirs(filename: &str) -> Result<Option<PathBuf>> {
    let mut current_dir = std::env::current_dir()?;

    loop {
        // Check if the file exists in the current directory first
        let path = current_dir.join(filename);
        if metadata(&path).await.is_ok() && metadata(&path).await?.is_file() {
            return Ok(Some(path));
        }

        // Move to the parent directory if the file was not found
        match current_dir.parent() {
            Some(parent_dir) => current_dir = parent_dir.to_path_buf(),
            None => break, // Reached root directory
        }

        let mut entries = read_dir(&current_dir).await?;
        if entries.next_entry().await.is_err() {
            break;
        }
    }

    Ok(None)
}
