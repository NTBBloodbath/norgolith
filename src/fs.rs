use std::path::{Path, PathBuf};

use eyre::Result;
use tokio::fs::{copy, create_dir_all, metadata, read_dir};

#[cfg(test)]
use tokio::fs::{canonicalize, create_dir, remove_dir, remove_file, File};

/// Find a given file or directory in the current working directory and its parent directories recursively
pub async fn find_in_previous_dirs(
    kind: &str,
    filename: &str,
    current_dir: &mut PathBuf,
) -> Result<Option<PathBuf>> {
    loop {
        // Check if the file|dir exists in the current directory first
        let path = current_dir.join(filename);
        if let Ok(metadata) = metadata(&path).await {
            if (metadata.is_file() && kind == "file") || (metadata.is_dir() && kind == "dir") {
                return Ok(Some(path));
            }
        }

        // Move to the parent directory if the file|dir was not found
        match current_dir.parent() {
            Some(parent_dir) => *current_dir = parent_dir.to_path_buf(),
            None => break, // Reached root directory
        }

        let mut entries = read_dir(&current_dir).await?;
        if entries.next_entry().await?.is_none() {
            break;
        }
    }

    Ok(None)
}

pub async fn copy_dir_all(src: impl AsRef<Path>, dest: impl AsRef<Path>) -> Result<()> {
    Box::pin(create_dir_all(&dest)).await?;
    let mut entries = read_dir(&src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        if file_type.is_dir() {
            Box::pin(copy_dir_all(entry.path(), dest.as_ref().join(entry.file_name()))).await?;
        } else {
            copy(entry.path(), dest.as_ref().join(entry.file_name())).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_find_file_in_current_directory() -> Result<()> {
        // Create temporal test file
        let test_file = "test_file_1.txt";
        let test_file_path = PathBuf::from(test_file);
        File::create(test_file).await?;

        // Look for the temporal test file
        let mut current_dir = std::env::current_dir()?;
        let result = find_in_previous_dirs("file", test_file, &mut current_dir).await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            Some(canonicalize(test_file_path.clone()).await?)
        );

        // Cleanup test file
        remove_file(test_file_path).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_find_file_from_child_directory() -> Result<()> {
        // Create temporal test directory and test file
        let test_file = "test_file_2.txt";
        let test_directory = PathBuf::from("parent_dir");

        create_dir(&test_directory).await?;
        File::create(test_file).await?;

        // Save current directory as the previous directory to restore it later
        let previous_dir = std::env::current_dir()?;

        // Enter the test directory
        std::env::set_current_dir(canonicalize(test_directory.clone()).await?)?;

        // Look for the temporal test file
        let result = find_in_previous_dirs(
            "file",
            test_file,
            &mut previous_dir.join(test_directory.clone()),
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(previous_dir.join(test_file)));

        // Restore previous directory
        std::env::set_current_dir(previous_dir)?;

        // Cleanup test directory and test file
        remove_file(test_file).await?;
        remove_dir(test_directory).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_find_file_not_found() -> Result<()> {
        let test_file = "non_existent_file.txt";

        let mut current_dir = std::env::current_dir()?;
        let result = find_in_previous_dirs("file", test_file, &mut current_dir).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);

        Ok(())
    }

    #[tokio::test]
    async fn test_find_dir_in_current_dir() -> Result<()> {
        // Create temporal test directory
        let test_directory = PathBuf::from("parent_dir2");

        create_dir(&test_directory).await?;

        // Look for the temporal directory
        let mut current_dir = std::env::current_dir()?;
        let result = find_in_previous_dirs(
            "dir",
            test_directory.clone().to_str().unwrap(),
            &mut current_dir,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            Some(canonicalize(test_directory.clone()).await?)
        );

        // Cleanup test directory
        remove_dir(test_directory).await?;

        Ok(())
    }
}
