use std::path::{Path, PathBuf};

use eyre::{bail, eyre, Context, Result};
use git2::{build::CheckoutBuilder, Repository};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;
use tokio::fs;

use crate::fs::copy_dir_all;

#[derive(Clone, Debug)]
pub struct ThemeManager {
    pub repo: String,
    pub version: Version,
    pub pin: bool,
    pub theme_dir: PathBuf,
}

#[derive(Serialize, Deserialize)]
pub struct ThemeMetadata {
    pub repo: String,
    pub version: Version,
    pub pin: bool,
}

pub async fn resolve_repo_shorthand(repo: &str) -> Result<String> {
    if let Some((service, rest)) = repo.split_once(':') {
        match service.to_lowercase().as_str() {
            "gh" | "github" => Ok(format!("https://github.com/{}", rest)),
            "srht" | "sourcehut" => Ok(format!("https://git.sr.ht/~{}", rest)),
            "berg" | "codeberg" => Ok(format!("https://codeberg.org/{}", rest)),
            _ => bail!("Unknown repository service: {}", service),
        }
    } else {
        // Assume GitHub by default if 'author/repo' has been passed instead of
        // trying to use something like gh:author/repo
        Ok(format!("https://github.com/{}", repo))
    }
}

async fn get_version(repo: &Repository, requirement: Option<String>) -> Result<Version> {
    let mut versions = repo
        .tag_names(None)?
        .iter()
        .flatten()
        .filter_map(|t| Version::parse(t).ok())
        .collect::<Vec<_>>();

    if let Some(req) = requirement {
        let version_req = VersionReq::parse(&req)?;
        versions.retain(|v| version_req.matches(v));
    }

    versions.sort();
    versions
        .last()
        .cloned()
        .ok_or_else(|| eyre!("No matching versions found"))
}

async fn checkout_version(repo: &Repository, version: &Version) -> Result<()> {
    let tag_name = version.to_string();
    let (object, reference) = repo.revparse_ext(&tag_name)?;

    repo.checkout_tree(&object, Some(CheckoutBuilder::new().force()))?;

    if let Some(reference) = reference {
        repo.set_head(
            reference
                .name()
                .ok_or_else(|| eyre!("Invalid reference name"))?,
        )?;
    }

    Ok(())
}

async fn backup_theme_files(src: &Path, dest: &Path) -> Result<()> {
    // If the theme directory is empty then early return
    if src.read_dir()?.next().is_none() {
        return Ok(());
    }

    // TODO: make backup directory capable of holding more states
    // than just the last one before pulling/updating a theme
    if dest.exists() {
        tokio::fs::remove_dir_all(dest).await?;
    }
    tokio::fs::create_dir_all(dest).await?;

    println!("[theme] Backing up theme files ...");
    copy_dir_all(src, dest).await?;

    Ok(())
}

async fn copy_theme_files(src: &Path, dest: &Path) -> Result<()> {
    let allowed_dirs = ["templates", "assets"];
    let allowed_files = ["README.md", "LICENSE"];

    // Clean existing theme directory
    if dest.exists() {
        fs::remove_dir_all(dest).await?;
    }
    fs::create_dir_all(dest).await?;

    println!("[theme] Copying theme files ...");
    let mut entries = fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let file_name = entry.file_name();
        let file_name_str = file_name.clone().into_string().unwrap();

        if allowed_dirs.contains(&file_name_str.as_ref()) {
            copy_dir_all(entry.path(), dest.join(file_name)).await?;
        } else if allowed_files.contains(&file_name_str.as_ref()) {
            fs::copy(entry.path(), dest.join(file_name)).await?;
        }
    }

    Ok(())
}

impl ThemeManager {
    pub async fn pull(&mut self) -> Result<Self> {
        let repo_url = resolve_repo_shorthand(&self.repo).await?;
        let temp_dir = tempdir().context("Failed to create temporary directory")?;

        // Clone repository
        let repo = Repository::clone(&repo_url, temp_dir.path())
            .context("Failed to clone theme repository")?;

        // Get the version tag
        let version = if self.version.to_string() == "0.0.0" {
            get_version(&repo, None)
                .await
                .context("No valid semantic versions found in repository")?
        } else {
            get_version(&repo, Some(self.version.to_string()))
                .await
                .context(format!("Version {} not found in repository", self.version))?
        };
        checkout_version(&repo, &version).await?;

        // Backup existing theme files before installing a new one
        let backup_dir = self.theme_dir.parent().unwrap().join(".theme_backup");
        backup_theme_files(&self.theme_dir, &backup_dir)
            .await
            .context("Failed to backup theme files")?;

        // Copy theme files
        copy_theme_files(temp_dir.path(), &self.theme_dir)
            .await
            .context("Failed to copy theme files")?;

        // Write metadata
        self.version = version;
        self.write_metadata()
            .await
            .context("Failed to write theme metadata")?;

        Ok(self.clone())
    }

    pub async fn update(&mut self) -> Result<Self> {
        // 1. Check remote tags respecting semver and pin status
        // 2. Diff existing theme files
        // 3. Apply updates if needed
        let repo_url = resolve_repo_shorthand(&self.repo).await?;
        let temp_dir = tempdir().context("Failed to create temporary directory")?;

        // Clone repository
        let repo = Repository::clone(&repo_url, temp_dir.path())
            .context("Failed to clone theme repository")?;

        // Calculate version requirement
        let version_req = if self.pin {
            format!("^{}.0.0", self.version.major)
        } else {
            "*".to_string()
        };

        // Get updatable version
        let latest_version = get_version(&repo, Some(version_req))
            .await
            .context("No valid update versions found")?;

        if latest_version > self.version {
            // Checkout new version
            checkout_version(&repo, &latest_version)
                .await
                .context("Failed to checkout new theme version")?;

            // Backup current theme files
            let backup_dir = self.theme_dir.parent().unwrap().join(".theme_backup");
            backup_theme_files(&self.theme_dir, &backup_dir)
                .await
                .context("Failed to backup theme files")?;

            // Copy new theme version files
            copy_theme_files(temp_dir.path(), &self.theme_dir)
                .await
                .context("Failed to update theme files")?;

            // Update metadata
            self.version = latest_version;
            self.write_metadata()
                .await
                .context("Failed to update theme metadata")?;
        } else {
            println!(
                "[theme] The theme version is up-to-date{}",
                if self.pin {
                    format!(" (pinned to: {})", self.version)
                } else {
                    String::from("")
                }
            );
        }

        Ok(self.clone())
    }

    async fn write_metadata(&mut self) -> Result<()> {
        let metadata_path = self.theme_dir.join("metadata.toml");
        let metadata = ThemeMetadata {
            repo: self.repo.clone(),
            version: self.version.clone(),
            pin: self.pin,
        };

        fs::write(metadata_path, toml::to_string_pretty(&metadata)?)
            .await
            .context("Failed to write metadata file")
    }
}
