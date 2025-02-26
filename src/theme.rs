use std::path::{Path, PathBuf};

use eyre::{bail, eyre, Context, Result};
use git2::{build::CheckoutBuilder, Repository};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use spinoff::Spinner;
use tempfile::tempdir;
use tokio::fs;
use tracing::{debug, error, instrument};

use crate::fs::copy_dir_all;

#[derive(Clone, Debug)]
pub struct ThemeManager {
    pub repo: String,
    pub version: Version,
    pub pin: bool,
    pub theme_dir: PathBuf,
}

/// theme.toml file contents
#[derive(Serialize, Deserialize)]
pub struct ThemeMetadata {
    pub name: String,
    pub author: String,
    pub description: String,
    pub version: String,
    pub license: String,
}

/// .metadata.toml file contents
/// used for update/backup/rollback mechanisms
#[derive(Serialize, Deserialize)]
pub struct ThemeInstalledMetadata {
    pub repo: String,
    pub version: Version,
    pub pin: bool,
}

#[instrument(skip(repo))]
pub async fn resolve_repo_shorthand(repo: &str) -> Result<String> {
    debug!("Resolving repository shorthand");
    if let Some((service, rest)) = repo.split_once(':') {
        debug!("Processing repository service {}", service.to_lowercase());
        match service.to_lowercase().as_str() {
            "gh" | "github" => Ok(format!("https://github.com/{}", rest)),
            "srht" | "sourcehut" => Ok(format!("https://git.sr.ht/~{}", rest)),
            "berg" | "codeberg" => Ok(format!("https://codeberg.org/{}", rest)),
            _ => bail!("Unknown repository service: {}", service),
        }
    } else {
        // Assume GitHub by default if 'author/repo' has been passed instead of
        // trying to use something like gh:author/repo
        debug!("Assuming GitHub repository");
        Ok(format!("https://github.com/{}", repo))
    }
}

#[instrument(skip(repo, requirement))]
async fn get_version(repo: &Repository, requirement: Option<String>) -> Result<Version> {
    debug!("Finding compatible version");
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

#[instrument(skip(repo, version))]
async fn checkout_version(repo: &Repository, version: &Version) -> Result<()> {
    debug!(%version, "Checking out version");
    let tag_name = version.to_string();
    let (object, reference) = repo.revparse_ext(&tag_name).map_err(|e| {
        error!(error = %e, "Failed to parse version reference");
        e
    })?;

    repo.checkout_tree(&object, Some(CheckoutBuilder::new().force()))
        .map_err(|e| {
            error!(error = %e, "Failed to checkout tree");
            e
        })?;

    if let Some(reference) = reference {
        let ref_name = reference
            .name()
            .ok_or_else(|| eyre!("Invalid reference name"))?;
        repo.set_head(ref_name).map_err(|e| {
            error!(error = %e, "Failed to set HEAD");
            e
        })?;
    }

    Ok(())
}

#[instrument(skip(src, dest, sp))]
async fn backup_theme_files(src: &Path, dest: &Path, sp: &mut Spinner) -> Result<()> {
    // If the theme directory is empty then early return
    if src.read_dir()?.next().is_none() {
        debug!("Source directory is empty, skipping backup");
        return Ok(());
    }

    // TODO: make backup directory capable of holding more states
    // than just the last one before pulling/updating a theme
    if dest.exists() {
        debug!(backup_path = %dest.display(), "Removing existing backup");
        tokio::fs::remove_dir_all(dest).await?;
    }
    tokio::fs::create_dir_all(dest).await?;

    sp.update_after_time(
        "Backing up existing theme files...",
        std::time::Duration::from_millis(200),
    );
    debug!(src = %src.display(), dest = %dest.display(), "Copying directory");
    copy_dir_all(src, dest).await?;

    Ok(())
}

#[instrument(skip(src, dest, sp))]
async fn copy_theme_files(src: &Path, dest: &Path, sp: &mut Spinner) -> Result<()> {
    let allowed_dirs = ["templates", "assets"];
    let allowed_files = ["README.md", "LICENSE", "theme.toml"];

    // Clean existing theme directory
    if dest.exists() {
        debug!(dest = %dest.display(), "Cleaning existing theme directory");
        fs::remove_dir_all(dest).await?;
    }
    fs::create_dir_all(dest).await?;

    sp.update_after_time(
        "Copying theme files...",
        std::time::Duration::from_millis(200),
    );
    let mut entries = fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let file_name = entry.file_name();
        let file_name_str = file_name.clone().into_string().unwrap();

        if allowed_dirs.contains(&file_name_str.as_ref()) {
            debug!(dir = %file_name_str, "Copying directory");
            copy_dir_all(entry.path(), dest.join(file_name)).await?;
        } else if allowed_files.contains(&file_name_str.as_ref()) {
            debug!(file = %file_name_str, "Copying file");
            fs::copy(entry.path(), dest.join(file_name)).await?;
        } else {
            debug!(file = %file_name_str, "Skipping disallowed file/directory");
        }
    }

    Ok(())
}

impl ThemeManager {
    #[instrument(skip(self, sp))]
    pub async fn pull(&mut self, sp: &mut Spinner) -> Result<Self> {
        debug!("Starting theme pull operation");
        let repo_url = resolve_repo_shorthand(&self.repo).await?;
        let temp_dir = tempdir().context("Failed to create temporary directory")?;
        debug!(temp_dir = %temp_dir.path().display(), "Created temporary directory");

        // Clone repository
        debug!(url = %repo_url, "Cloning theme directory");
        let repo = Repository::clone(&repo_url, temp_dir.path())
            .context("Failed to clone theme repository")?;

        // Get the version tag
        let version = if self.version.to_string() == "0.0.0" {
            debug!("Looking for latest version");
            get_version(&repo, None)
                .await
                .context("No valid semantic versions found in repository")?
        } else {
            debug!(current_version = %self.version, "Looking for specific version");
            get_version(&repo, Some(self.version.to_string()))
                .await
                .context(format!("Version {} not found in repository", self.version))?
        };
        debug!(selected_version = %version, "Found version");
        checkout_version(&repo, &version).await?;

        // Backup existing theme files before installing a new one
        let backup_dir = self.theme_dir.parent().unwrap().join(".theme_backup");
        debug!(backup_path = %backup_dir.display(), "Starting theme backup");
        backup_theme_files(&self.theme_dir, &backup_dir, sp)
            .await
            .context("Failed to backup theme files")?;

        // Copy theme files
        debug!(theme_dir = %self.theme_dir.display(), "Copying theme files to destination");
        copy_theme_files(temp_dir.path(), &self.theme_dir, sp)
            .await
            .context("Failed to copy theme files")?;

        // Write metadata
        self.version = version;
        self.write_metadata(sp)
            .await
            .context("Failed to write theme metadata")?;

        debug!("Theme pull completed successfully");
        Ok(self.clone())
    }

    #[instrument(skip(self, sp))]
    pub async fn update(&mut self, sp: &mut Spinner) -> Result<Self> {
        debug!("Starting theme update operation");
        let repo_url = resolve_repo_shorthand(&self.repo).await?;
        let temp_dir = tempdir().context("Failed to create temporary directory")?;
        debug!(temp_dir = %temp_dir.path().display(), "Created temporary directory");

        // Clone repository
        debug!(url = %repo_url, "Cloning theme repository for update");
        let repo = Repository::clone(&repo_url, temp_dir.path())
            .context("Failed to clone theme repository")?;

        // Calculate version requirement
        let version_req = if self.pin {
            format!("^{}.0.0", self.version.major)
        } else {
            "*".to_string()
        };
        debug!(version_requirement = %version_req, "Calculated version requirement");

        // Get updatable version
        let latest_version = get_version(&repo, Some(version_req))
            .await
            .context("No valid update versions found")?;

        if latest_version > self.version {
            // Checkout new version
            debug!(current_version = %self.version, new_version = %latest_version, "New version available");
            checkout_version(&repo, &latest_version)
                .await
                .context("Failed to checkout new theme version")?;

            // Backup current theme files
            let backup_dir = self.theme_dir.parent().unwrap().join(".theme_backup");
            backup_theme_files(&self.theme_dir, &backup_dir, sp)
                .await
                .context("Failed to backup theme files")?;

            // Copy new theme version files
            copy_theme_files(temp_dir.path(), &self.theme_dir, sp)
                .await
                .context("Failed to update theme files")?;

            // Update metadata
            self.version = latest_version;
            self.write_metadata(sp)
                .await
                .context("Failed to update theme metadata")?;
            sp.stop_and_persist("✓", "Theme updated successfully");
        } else {
            sp.stop_and_persist(
                "✓",
                &format!(
                    "Theme is already up-to-date (version: {}, pinned: {})",
                    self.version, self.pin
                ),
            );
        }

        Ok(self.clone())
    }

    #[instrument(skip(self, sp))]
    async fn write_metadata(&mut self, sp: &mut Spinner) -> Result<()> {
        debug!("Writing theme metadata");
        let metadata_path = self.theme_dir.join(".metadata.toml");
        let metadata = ThemeInstalledMetadata {
            repo: self.repo.clone(),
            version: self.version.clone(),
            pin: self.pin,
        };

        sp.update_after_time(
            "Writing theme metadata file...",
            std::time::Duration::from_millis(200),
        );
        debug!(path = %metadata_path.display(), "Writing metadata file");
        fs::write(metadata_path, toml::to_string_pretty(&metadata)?)
            .await
            .context("Failed to write metadata file")?;
        debug!("Metadata written successfully");

        Ok(())
    }
}
