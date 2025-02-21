use eyre::{bail, Context, Result};

use clap::Subcommand;

use crate::{
    fs,
    theme::{self, ThemeManager, ThemeMetadata},
};

#[derive(Subcommand, Clone)]
pub enum ThemeCommands {
    /// Install a theme from a repository (github, codeberg or sourcehut)
    Pull {
        /// Repository shorthand (e.g. user/repo or github:user/repo)
        repo: String,

        /// Theme version (optional, defaults to the latest release)
        version: Option<String>,

        /// Pin to current major version
        #[arg(long)]
        pin: bool,
    },
    /// Update the current theme
    Update,
    /// Initialize theme structure (WIP)
    Init,
    /// Show theme information
    Info,
}

async fn pull_theme(repo: &str, version: &Option<String>, pin: bool) -> Result<()> {
    // Try to find a 'norgolith.toml' file in the current working directory and its parents
    let mut current_dir = std::env::current_dir()?;
    let found_site_root =
        fs::find_in_previous_dirs("file", "norgolith.toml", &mut current_dir).await?;

    if let Some(mut root) = found_site_root {
        // Remove `norgolith.toml` from the root path
        root.pop();
        let theme_dir = root.join("theme");

        let mut theme = ThemeManager {
            repo: repo.to_string(),
            version: semver::Version::new(0, 0, 0), // Placeholder, we will grab the version from latest release
            pin,
            theme_dir,
        };
        if let Some(version) = version {
            theme.version =
                semver::Version::parse(version).context("No valid semantic version provided")?;
        }

        println!("[theme] Pulling theme from '{}' ...", theme::resolve_repo_shorthand(repo).await?);
        theme.pull().await?;
    } else {
        bail!("[theme] Could not pull the theme: not in a Norgolith site directory");
    }

    Ok(())
}

async fn update_theme() -> Result<()> {
    // Try to find a 'norgolith.toml' file in the current working directory and its parents
    let mut current_dir = std::env::current_dir()?;
    let found_site_root =
        fs::find_in_previous_dirs("file", "norgolith.toml", &mut current_dir).await?;

    if let Some(mut root) = found_site_root {
        // Remove `norgolith.toml` from the root path
        root.pop();
        let theme_dir = root.join("theme");

        // Check if there is a 'metadata.toml' in the theme directory before proceeding
        if theme_dir.join("metadata.toml").exists() {
            // Load the current theme metadata
            let metadata_content =
                tokio::fs::read_to_string(theme_dir.join("metadata.toml")).await?;
            let theme_metadata: ThemeMetadata = toml::from_str(&metadata_content)?;

            let mut theme = ThemeManager {
                repo: theme_metadata.repo.clone(),
                version: theme_metadata.version,
                pin: theme_metadata.pin,
                theme_dir,
            };

            println!("[theme] Updating theme ...");
            theme.update().await?;
        } else {
            bail!("[theme] Could not update the theme: there is no theme installed");
        }
    } else {
        bail!("[theme] Could not update the theme: not in a Norgolith site directory");
    }
    Ok(())
}

async fn init_theme() -> Result<()> {
    println!("Initializing theme structure... WIP");
    Ok(())
}

async fn show_theme_info() -> Result<()> {
    // Try to find a 'norgolith.toml' file in the current working directory and its parents
    let mut current_dir = std::env::current_dir()?;
    let found_site_root =
        fs::find_in_previous_dirs("file", "norgolith.toml", &mut current_dir).await?;

    if let Some(mut root) = found_site_root {
        // Remove `norgolith.toml` from the root path
        root.pop();
        let theme_dir = root.join("theme");

        // Check if there is a 'metadata.toml' in the theme directory before proceeding
        if theme_dir.join("metadata.toml").exists() {
            let metadata_content =
                tokio::fs::read_to_string(theme_dir.join("metadata.toml")).await?;
            let theme_metadata: ThemeMetadata = toml::from_str(&metadata_content)?;

            println!(
                "[theme] Current theme information:\n→ Repository: {}\n→ Version: {}\n→ Pinned: {}",
                theme_metadata.repo,
                theme_metadata.version,
                if theme_metadata.pin { "yes" } else { "no" },
            );
        } else {
            bail!("[theme] Could not display the theme info: there is no theme installed");
        }
    } else {
        bail!("[theme] Could not display the theme info: not in a Norgolith site directory");
    }
    Ok(())
}

pub async fn handle(subcommand: &ThemeCommands) -> Result<()> {
    match subcommand {
        ThemeCommands::Pull { repo, version, pin } => pull_theme(repo, version, *pin).await,
        ThemeCommands::Update => update_theme().await,
        ThemeCommands::Init => init_theme().await,
        ThemeCommands::Info => show_theme_info().await,
    }
}
