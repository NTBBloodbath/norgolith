use std::path::{Path, PathBuf};

use chrono::{Local, SecondsFormat};
use eyre::{bail, eyre, Context, Result};
use indoc::formatdoc;
use tracing::info;
use whoami::username;

use crate::fs;

/// Supported asset types for creation
#[derive(Debug, Clone, Copy)]
enum AssetType {
    Js,
    Css,
    Content,
}

impl AssetType {
    /// Determine asset type from file extension
    fn from_extension(ext: &str) -> Result<Self> {
        match ext.to_lowercase().as_str() {
            "js" => Ok(Self::Js),
            "css" => Ok(Self::Css),
            "norg" => Ok(Self::Content),
            _ => bail!("Unsupported file extension: {}", ext),
        }
    }

    /// Get directory name for asset type
    fn directory(&self) -> &'static str {
        match self {
            Self::Js | Self::Css => "assets",
            Self::Content => "content",
        }
    }

    /// Get subdirectory for asset type
    fn subdirectory(&self) -> Option<&'static str> {
        match self {
            Self::Js => Some("js"),
            Self::Css => Some("css"),
            Self::Content => None,
        }
    }
}

/// Generate content title from file path
fn generate_content_title(base_path: &Path, full_path: &Path) -> String {
    let relative_path = full_path
        .strip_prefix(base_path.join("content"))
        .unwrap_or(full_path);

    let mut components = relative_path
        .iter()
        .filter(|c| *c != "index.norg")
        .map(|c| {
            c.to_string_lossy()
                .trim_end_matches(".norg")
                .replace(['-', '_'], " ")
        })
        .collect::<Vec<_>>();

    if components.is_empty() {
        return "index".to_string();
    }

    if let Some(last) = components.last_mut() {
        if last == "index" {
            components.pop();
        }
    }

    components.join(" | ")
}

/// Create a new norg document
async fn create_norg_document(path: &Path, title: &str) -> Result<()> {
    let creation_date = Local::now().to_rfc3339_opts(SecondsFormat::Secs, false);
    let username = username();

    let content = formatdoc!(
        r#"
        @document.meta
        title: {title}
        description:
        authors: [
          {username}
        ]
        categories: []
        created: {creation_date}
        updated: {creation_date}
        draft: true
        version: 1.1.1
        @end

        * {title}
          Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut
          labore et dolore magna aliqua. Lobortis scelerisque fermentum dui faucibus in ornare."#,
    );
    tokio::fs::write(path, content).await?;

    Ok(())
}

/// Validate and parse input path
fn parse_input_path(name: &str) -> PathBuf {
    let path = PathBuf::from(name);
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "untitled".to_string());

    PathBuf::from(file_name)
}

/// Create necessary directories for the asset
async fn ensure_directory_exists(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }
    }
    Ok(())
}

/// Handle file opening with system editor
async fn open_file_editor(path: &Path) -> Result<()> {
    open::that(path).with_context(|| format!("Failed to open file: {}", path.display()))?;
    info!("Opened file in editor: {}", path.display());
    Ok(())
}

pub async fn new(kind: &str, name: &str, open: bool) -> Result<()> {
    let asset_type = AssetType::from_extension(kind)?;
    let input_path = parse_input_path(name);

    // Validate file extension
    if let AssetType::Content = asset_type {
        if input_path.extension().map(|e| e != "norg").unwrap_or(true) {
            bail!("Norg documents must have .norg extension");
        }
    }

    // Find site root
    let site_root = fs::find_config_file()
        .await?
        .ok_or_else(|| eyre!("Unable to create site asset: not in a Norgolith site directory"))?;

    // Build target path
    let mut target_path = site_root.parent().unwrap().join(asset_type.directory());

    if let Some(subdir) = asset_type.subdirectory() {
        target_path.push(subdir);
    }

    target_path.push(&input_path);

    // Create directories and file
    ensure_directory_exists(&target_path).await?;

    match asset_type {
        AssetType::Content => {
            let title = generate_content_title(&site_root, &target_path);
            create_norg_document(&target_path, &title).await?;
        }
        AssetType::Js | AssetType::Css => {
            tokio::fs::File::create(&target_path)
                .await
                .with_context(|| format!("Failed to create file: {}", target_path.display()))?;
        }
    }

    // Open file if requested
    if open {
        open_file_editor(&target_path).await?;
    }

    info!("Successfully created: {}", target_path.display());

    Ok(())
}
