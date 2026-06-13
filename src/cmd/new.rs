use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use chrono::{Local, SecondsFormat};
use colored::Colorize;
use eyre::{bail, eyre, Context, Result};
use indoc::formatdoc;
use inquire::{Select, Text};
use regex::Regex;
use titlecase::titlecase;
use tracing::{debug, info, instrument, warn};
use whoami::username;

use crate::{config, fs};

/// Supported asset types for creation
#[derive(Debug, Clone, Copy, PartialEq)]
enum AssetType {
    Js,
    Css,
    Content,
}

impl AssetType {
    /// Determine asset type from file extension
    #[instrument]
    fn from_extension(ext: &str) -> Result<Self> {
        debug!(extension = %ext, "Determining asset type");
        let asset_type = match ext.to_lowercase().as_str() {
            "js" => Ok(Self::Js),
            "css" => Ok(Self::Css),
            "norg" => Ok(Self::Content),
            _ => bail!("Unsupported file extension: {}", ext),
        };

        debug!(asset_type = ?asset_type, "Determined asset type");
        asset_type
    }

    /// Get directory name for asset type
    #[instrument]
    fn directory(&self) -> &'static str {
        let dir = match self {
            Self::Js | Self::Css => "assets",
            Self::Content => "content",
        };
        debug!(directory = dir, "Resolved asset directory");
        dir
    }

    /// Get subdirectory for asset type
    #[instrument]
    fn subdirectory(&self) -> Option<&'static str> {
        let subdir = match self {
            Self::Js => Some("js"),
            Self::Css => Some("css"),
            Self::Content => None,
        };
        debug!(subdirectory = ?subdir, "Resolved asset subdirectory");
        subdir
    }
}

/// Generate content title from file path
#[instrument(skip(base_path, full_path))]
fn generate_content_title(base_path: &Path, full_path: &Path) -> String {
    debug!("Generating content title");

    // Get relative path within content directory
    let relative_path = full_path
        .strip_prefix(base_path.join("content"))
        .unwrap_or(full_path);

    // Determine if we're dealing with an index file
    let is_index = relative_path.ends_with("index.norg");

    // Extract the relevant component name
    let raw_name = if is_index {
        // Get parent directory name for index files
        relative_path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy())
            .unwrap_or_else(|| {
                debug!("Using default title for root index");
                Cow::Borrowed("Home")
            })
    } else {
        // Get filename stem for regular files
        relative_path
            .file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_else(|| {
                debug!("No valid filename stem found");
                Cow::Borrowed("Untitled")
            })
    };

    // Clean and format the title
    let title = raw_name
        .replace(['-', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // Titlecase the first letter of each word
    let title = titlecase(
        &title
            .split(' ')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    );

    debug!(title = %title, "Generated content title");
    title
}

/// Create a new norg document
#[instrument(level = "debug", skip(path, title))]
async fn create_norg_document(path: &Path, title: &str) -> Result<()> {
    debug!("Creating new norg document: {}", path.display());
    let re = Regex::new(r"[,\s+?]+")?;
    let creation_date = Local::now().to_rfc3339_opts(SecondsFormat::Secs, false);

    // Prompt norg file metadata
    let title = Text::new("Title:")
        .with_default(title)
        .with_help_message("Document title")
        .prompt()
        .map_err(|e| eyre!("Failed to get document title: {}", e))?;
    let description = Text::new("Description:")
        .with_default("")
        .with_help_message("Document description")
        .prompt()
        .map_err(|e| eyre!("Failed to get document description: {}", e))?;
    let authors = Text::new("Author(s):")
        .with_default(username().as_str())
        .with_help_message("Document authors separated by comma")
        .with_placeholder("e.g. NTBBloodbath, Vhyrro")
        .prompt()
        .map_err(|e| eyre!("Failed to get document author: {}", e))?;
    let categories = Text::new("Categories:")
        .with_default("")
        .with_help_message("Document categories separated by comma")
        .with_placeholder("e.g. Neovim, Neorg")
        .prompt()
        .map_err(|e| eyre!("Failed to get document categories: {}", e))?;
    let layout = Text::new("Layout:")
        .with_default("default")
        .with_help_message("Template to be used for this file")
        .with_placeholder("e.g. post")
        .prompt()
        .map_err(|e| eyre!("Failed to get document layout: {}", e))?;

    let content = formatdoc!(
        r#"
        @document.meta
        title: {title}
        description: {description}
        authors: [
          {}
        ]
        categories: [
          {}
        ]
        created: {creation_date}
        updated: {creation_date}
        draft: true
        layout: {layout}
        version: 1.1.1
        @end

        * {title}
          Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut
          labore et dolore magna aliqua. Lobortis scelerisque fermentum dui faucibus in ornare."#,
        re.replace_all(&authors, "\n  "),
        re.replace_all(&categories, "\n  "),
    );
    tokio::fs::write(path, content)
        .await
        .map_err(|e| eyre!("Failed to write norg document: {}", e))?;

    info!("Created norg document: {}", path.display());
    Ok(())
}

/// Create necessary directories for the asset
#[instrument(skip(path))]
async fn ensure_directory_exists(path: &Path) -> Result<()> {
    debug!(path = %path.display(), "Ensuring directory exists");
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            debug!("Creating directory: {}", parent.display());
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

fn resolve_collection<'a>(
    collections: &'a [config::CollectionConfig],
    collection_name: Option<&String>,
) -> Result<&'a config::CollectionConfig> {
    if collections.is_empty() {
        bail!("No collections configured. Add [[collections]] to norgolith.toml");
    }
    if let Some(name) = collection_name {
        collections.iter().find(|c| c.name == *name).ok_or_else(|| {
            let available = collections
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            eyre!("Unknown collection '{}'. Available: {}", name, available)
        })
    } else if collections.len() == 1 {
        Ok(&collections[0])
    } else {
        let names: Vec<&str> = collections.iter().map(|c| c.name.as_str()).collect();
        let selected_name = Select::new("Select a collection:", names)
            .prompt()
            .map_err(|e| eyre!("Failed to select collection: {}", e))?;
        Ok(collections
            .iter()
            .find(|c| c.name == selected_name)
            .unwrap())
    }
}

#[instrument(skip(kind, name, open, collection))]
pub async fn new(kind: &str, name: &str, open: bool, collection: Option<&String>) -> Result<()> {
    debug!(type = kind, name = name, "Creating new asset");

    // Find site root early — needed for "post" collection resolution
    let config_file = fs::find_config_file().await?.ok_or_else(|| {
        eyre!(
            "{}: not in a Norgolith site directory",
            "Unable to create site asset".bold()
        )
    })?;
    let site_root = config_file.parent().unwrap().to_path_buf();

    // "post" kind: resolve the target collection and delegate to Content creation
    let (resolved_kind, resolved_name);
    if kind == "post" {
        let config_content = tokio::fs::read_to_string(&config_file)
            .await
            .map_err(|e| eyre!("Failed to read config: {}", e))?;
        let site_config: config::SiteConfig =
            toml::from_str(&config_content).map_err(|e| eyre!("Failed to parse config: {}", e))?;
        let col = resolve_collection(&site_config.collections, collection)?;
        resolved_kind = "norg".to_string();
        resolved_name = format!("{}/{}", col.dir, name);
    } else {
        resolved_kind = kind.to_string();
        resolved_name = name.to_string();
    }

    let asset_type = AssetType::from_extension(&resolved_kind)?;
    let mut input_path = PathBuf::from(&resolved_name);

    // Validate file extension
    if let AssetType::Content = asset_type {
        // Add norg file extension to content name if it is missing an extension
        if input_path.extension().is_none() {
            debug!("Content file is missing norg extension, adding it from inference");
            input_path = input_path.with_extension("norg");
        }
        if input_path.extension().map(|e| e != "norg").unwrap_or(true) {
            bail!("Norg documents must have .norg extension");
        }
    }

    // Build target path
    let mut target_path = site_root.join(asset_type.directory());

    if let Some(subdir) = asset_type.subdirectory() {
        target_path.push(subdir);
    }

    target_path.push(&input_path);
    debug!(target_path = %target_path.display(), "Resolved target path");

    // Create directories and file
    ensure_directory_exists(&target_path).await?;

    match asset_type {
        AssetType::Content => {
            let title = generate_content_title(&site_root, &target_path);
            create_norg_document(&target_path, &title).await?;
        }
        AssetType::Js | AssetType::Css => {
            debug!("Creating empty asset file: {}", target_path.display());
            tokio::fs::File::create(&target_path)
                .await
                .with_context(|| format!("Failed to create file: {}", target_path.display()))?;
            info!("Created asset file: {}", target_path.display());
        }
    }

    // Open file if requested
    if open {
        open_file_editor(&target_path).await?;
    }

    Ok(())
}
