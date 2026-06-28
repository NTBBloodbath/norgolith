use std::path::{Path, PathBuf};

use eyre::Result;
use tokio::fs::{copy, create_dir_all, read_dir};
use tracing::{debug, instrument};

#[cfg(test)]
use tempfile::tempdir;

/// Find a given file or directory in the current working directory and its parent directories recursively
#[instrument(skip(kind, filename, current_dir))]
pub fn find_in_previous_dirs(
    kind: &str,
    filename: &str,
    current_dir: &mut PathBuf,
) -> Result<Option<PathBuf>> {
    loop {
        // Check if the file|dir exists in the current directory first
        let path = current_dir.join(filename);
        debug!("Checking path for {}", kind);
        if let Ok(metadata) = std::fs::metadata(&path) {
            if (metadata.is_file() && kind == "file") || (metadata.is_dir() && kind == "dir") {
                debug!("Found matching {} at path", kind);
                return Ok(Some(path));
            } else {
                debug!("Path exists but is not a {}: {}", kind, path.display());
            }
        }

        // Move to the parent directory if the file|dir was not found
        match current_dir.parent() {
            Some(parent_dir) => {
                debug!("Moving up to parent directory: {}", parent_dir.display());
                *current_dir = parent_dir.to_path_buf()
            }
            None => {
                debug!("Reached root directory, stopping search");
                break; // Reached root directory
            }
        }
    }

    debug!("No matching {} found in parent directories", kind);
    Ok(None)
}

#[instrument]
pub fn find_config_file() -> Result<Option<PathBuf>> {
    // Try to find a 'norgolith.toml' file in the current working directory and its parents
    let mut current_dir = std::env::current_dir()?;
    debug!("Starting search for config file 'norgolith.toml'");

    let found_site_root = find_in_previous_dirs("file", "norgolith.toml", &mut current_dir)?;

    match &found_site_root {
        Some(path) => debug!("Found config file: {}", path.display()),
        None => debug!("Config file not found in any parent directories"),
    }

    Ok(found_site_root)
}

#[instrument(skip(src, dest))]
pub async fn copy_dir_all(src: impl AsRef<Path>, dest: impl AsRef<Path>) -> Result<()> {
    let src = src.as_ref();
    let dest = dest.as_ref();

    debug!("Creating destination directory: {}", dest.display());
    create_dir_all(&dest).await?;

    let mut entries = read_dir(&src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        if file_type.is_dir() {
            debug!(
                "Copying directory from {} to {}",
                entry.path().display(),
                dest.display()
            );
            Box::pin(copy_dir_all(entry.path(), dest.join(entry.file_name()))).await?;
        } else {
            debug!(
                "Copying file from {} to {}",
                entry.path().display(),
                dest.display()
            );
            copy(entry.path(), dest.join(entry.file_name())).await?;
        }
    }

    debug!("Finished copying directory");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eyre::Result;
    use tempfile::tempdir;

    #[test]
    fn test_find_file_in_current_directory() -> Result<()> {
        let dir = tempdir()?;
        let test_file = "test_file_1.txt";
        let test_file_path = dir.path().join(test_file);
        std::fs::File::create(&test_file_path)?;

        let result = find_in_previous_dirs("file", test_file, &mut dir.path().to_path_buf());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(test_file_path));

        Ok(())
    }

    #[test]
    fn test_find_file_from_child_directory() -> Result<()> {
        let dir = tempdir()?;
        let test_file_name = "test_file_2.txt";
        let test_file = dir.path().join(test_file_name);
        let test_directory = dir.path().join("parent_dir");

        std::fs::create_dir(&test_directory)?;
        std::fs::File::create(&test_file)?;

        let result =
            find_in_previous_dirs("file", test_file_name, &mut test_directory.clone());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(test_file.clone()));

        Ok(())
    }

    #[test]
    fn test_find_file_not_found() -> Result<()> {
        let dir = tempdir()?;
        let test_file = "non_existent_file.txt";

        let result = find_in_previous_dirs("file", test_file, &mut dir.path().to_path_buf());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);

        Ok(())
    }

    #[test]
    fn test_find_dir_in_current_dir() -> Result<()> {
        let dir = tempdir()?;
        let test_directory_name = "parent_dir2";
        let test_directory = dir.path().join(test_directory_name);

        std::fs::create_dir(&test_directory)?;

        let result =
            find_in_previous_dirs("dir", test_directory_name, &mut dir.path().to_path_buf());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(test_directory));

        Ok(())
    }
}
