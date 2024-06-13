use std::path::PathBuf;

use eyre::Result;
use tokio::fs::{metadata, read_dir};

#[cfg(test)]
use tokio::fs::{canonicalize, create_dir, remove_dir, remove_file, File};

/// Find a given file in the current working directory and its parent directories recursively
pub async fn find_file_in_previous_dirs(filename: &str) -> Result<Option<PathBuf>> {
    let mut current_dir = std::env::current_dir()?;

    loop {
        // Check if the file exists in the current directory first
        let path = current_dir.join(filename);
        if let Ok(metadata) = metadata(&path).await {
            if metadata.is_file() {
                return Ok(Some(path));
            }
        }

        // Move to the parent directory if the file was not found
        match current_dir.parent() {
            Some(parent_dir) => current_dir = parent_dir.to_path_buf(),
            None => break, // Reached root directory
        }

        let mut entries = read_dir(&current_dir).await?;
        if entries.next_entry().await?.is_none() {
            break;
        }
    }

    Ok(None)
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
        let result = find_file_in_previous_dirs(test_file).await;
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
        let result = find_file_in_previous_dirs(test_file).await;
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

        let result = find_file_in_previous_dirs(test_file).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);

        Ok(())
    }
}
