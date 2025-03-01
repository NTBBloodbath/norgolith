use std::collections::HashMap;

use clap::Subcommand;
use colored::Colorize;
use eyre::{bail, eyre, Context, Result};
use indoc::formatdoc;
use inquire::{validator::Validation, Confirm, Select, Text};
use spinoff::{spinners, Spinner};
use tracing::info;

use crate::{
    fs,
    theme::{self, ThemeInstalledMetadata, ThemeManager, ThemeMetadata},
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
    /// Restore previous theme version from backup
    Rollback,
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

        let mut sp = Spinner::new(
            spinners::Dots2,
            format!(
                "Pulling theme from '{}'...",
                theme::resolve_repo_shorthand(repo).await?
            ),
            None,
        );
        theme.pull(&mut sp).await?;
        sp.stop_and_persist("✓", "Successfully pulled theme");
    } else {
        bail!("{}: not in a Norgolith site directory", "Could not pull the theme".bold());
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

        // Check if there is a '.metadata.toml' in the theme directory before proceeding
        if theme_dir.join(".metadata.toml").exists() {
            // Load the current theme metadata
            let metadata_content =
                tokio::fs::read_to_string(theme_dir.join(".metadata.toml")).await?;
            let theme_metadata: ThemeInstalledMetadata = toml::from_str(&metadata_content)?;

            let mut theme = ThemeManager {
                repo: theme_metadata.repo.clone(),
                version: theme_metadata.version,
                pin: theme_metadata.pin,
                theme_dir,
            };

            let mut sp = Spinner::new(spinners::Dots2, "Updating theme...", None);
            theme.update(&mut sp).await?;
        } else {
            bail!("{}: there is no theme installed", "Could not update the theme".bold());
        }
    } else {
        bail!("{}: not in a Norgolith site directory", "Could not update the theme".bold());
    }
    Ok(())
}

async fn rollback_theme() -> Result<()> {
    // Try to find a 'norgolith.toml' file in the current working directory and its parents
    let mut current_dir = std::env::current_dir()?;
    let found_site_root =
        fs::find_in_previous_dirs("file", "norgolith.toml", &mut current_dir).await?;

    if let Some(mut root) = found_site_root {
        let mut sp = Spinner::new(spinners::Dots2, "Rolling back to previous state...", None);

        // Remove `norgolith.toml` from the root path
        root.pop();
        let theme_dir = root.join("theme");

        let backup_dir = theme_dir
            .parent()
            .ok_or_else(|| eyre!("Invalid theme directory"))?
            .join(".theme_backup");

        if !backup_dir.exists() {
            sp.stop_and_persist("✖", "No previous state backup found");
            return Ok(());
        }

        // Remove existing theme
        if theme_dir.exists() && theme_dir.join("theme.toml").exists() {
            tokio::fs::remove_dir_all(theme_dir.clone())
                .await
                .context("Failed to remove current theme")?;
        }

        // Restore backup
        fs::copy_dir_all(backup_dir, theme_dir)
            .await
            .context("Failed to restore backup")?;

        sp.stop_and_persist("✓", "Successfully restored previous theme state");
    } else {
        bail!("{}: not in a Norgolith site directory", "Could not rollback the theme".bold());
    }

    Ok(())
}

async fn init_theme() -> Result<()> {
    // NOTE: perhaps we should allow to create a new theme outside of an existing site?
    // Try to find a 'norgolith.toml' file in the current working directory and its parents
    let mut current_dir = std::env::current_dir()?;
    let found_site_root =
        fs::find_in_previous_dirs("file", "norgolith.toml", &mut current_dir).await?;

    if let Some(mut root) = found_site_root {
        // Remove `norgolith.toml` from the root path
        root.pop();
        let theme_dir = root.join("theme");
        let theme_metadata = theme_dir.join(".metadata.toml");

        // Check for existing .metadata.toml
        if theme_metadata.exists() {
            let overwrite = Confirm::new("A theme already exists. Overwrite it?")
                .with_default(false)
                .prompt()?;

            if !overwrite {
                info!("Theme initialization canceled");
                return Ok(());
            }
        }

        // Collect theme metadata
        let name = Text::new("Theme name:")
            .with_help_message("e.g. 'Norgolith Pico'")
            .prompt()?;
        let author = Text::new("Author:")
            .with_help_message("Your name or organization")
            .prompt()?;
        let description = Text::new("Description:")
            .with_help_message("Short description of your theme")
            .prompt()?;
        let version = Text::new("Version:")
            .with_default("0.1.0")
            .with_validator(|v: &str| match semver::Version::parse(v) {
                Ok(_) => Ok(Validation::Valid),
                Err(_) => Ok(Validation::Invalid(
                    "Invalid semantic version format".into(),
                )),
            })
            .prompt()?;
        let license = Select::new(
            "License:",
            vec![
                "MIT",
                "Apache-2.0",
                "GPL-2.0",
                "GPL-3.0",
                "BSD-3-Clause",
                "Unlicense",
                "Other",
            ],
        )
        // .with_starting_cursor(0)
        .with_help_message("Choose a license for your theme")
        .prompt()?;

        let repository = Text::new("Repository URL (optional):")
            .with_help_message(
                "Format: 'github:user/repo', 'codeberg:user/repo' or 'sourcehut:user/repo'",
            )
            .prompt()?;

        let theme_config = theme::ThemeMetadata {
            name,
            author,
            description,
            version,
            license: license.to_string(),
        };

        // Theme directory structure
        let theme_templates = theme_dir.join("templates");
        let theme_directories = vec![
            theme_templates.clone(),
            theme_dir.join("assets/js"),
            theme_dir.join("assets/css"),
            theme_dir.join("assets/images"),
        ];
        for dir in theme_directories {
            tokio::fs::create_dir_all(dir).await?;
        }

        // Write default html templates
        // TODO: add 'head.html', 'footer.html' for more granular content?
        let templates = HashMap::from([
            ("base", include_str!("../resources/templates/base.html")),
            (
                "default",
                include_str!("../resources/templates/default.html"),
            ),
        ]);
        for (&name, &contents) in templates.iter() {
            let template_path = theme_templates.join(name.to_owned() + ".html");
            tokio::fs::write(template_path, contents).await?;
        }

        // Write README
        let repo = repository.clone();
        let readme_pull_repo = if !repo.is_empty() {
            repo
        } else {
            String::from("# TODO: add your 'user/repo' here")
        };
        let readme = formatdoc!(
            r#"
            # {}
            {}

            ## Installation
            ```bash
            lith theme pull {}
            ```

            ## License
            {} is licensed under {} license.
            "#,
            theme_config.name,
            theme_config.description,
            readme_pull_repo,
            theme_config.name,
            theme_config.license,
        );
        tokio::fs::write(theme_dir.join("README.md"), readme)
            .await
            .context("Failed to write README.md")?;

        // Write theme.toml
        tokio::fs::write(
            theme_dir.join("theme.toml"),
            toml::to_string_pretty(&theme_config)?,
        )
        .await
        .context("Failed to write theme.toml")?;

        info!("\nTheme initialized successfully!");
        println!("Next steps:");
        println!("1. Edit templates in the 'templates/' directory");
        println!("2. Add scripts to 'assets/js/'");
        println!("3. Add styles to 'assets/css/'");
    } else {
        bail!("{}: not in a Norgolith site directory", "Could not initialize the theme".bold());
    }
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

        // Check if there is a '.metadata.toml' in the theme directory before proceeding
        if theme_dir.join(".metadata.toml").exists() {
            let metadata_content =
                tokio::fs::read_to_string(theme_dir.join(".metadata.toml")).await?;
            let theme_metadata: ThemeInstalledMetadata = toml::from_str(&metadata_content)?;
            let theme_toml_content =
                tokio::fs::read_to_string(theme_dir.join("theme.toml")).await?;
            let theme_toml: ThemeMetadata = toml::from_str(&theme_toml_content)?;

            let theme_info: Vec<String> = vec![
                format!("\n{}", "Metadata".bold().green()),
                format!("  {} {}:\t {}", "→".blue(), "Name".bold(), theme_toml.name),
                format!("  {} {}: {}", "→".blue(), "Description".bold(), theme_toml.description),
                format!("  {} {}:\t {}", "→".blue(), "Author".bold(), theme_toml.author),
                format!("  {} {}:\t {}", "→".blue(), "License".bold(), theme_toml.license),
                format!("\n{}", "Status".bold().green()),
                format!("  {} {}:\t {}", "→".blue(), "Version".bold(), theme_toml.version),
                format!("  {} {}:\t {}", "→".blue(), "Pinned".bold(), if theme_metadata.pin { "yes" } else { "no" }),
            ];
            println!("{}:\n{}", "Current theme information".bold(), theme_info.join("\n"));
        } else {
            bail!("{}: there is no theme installed", "Could not display the theme info".bold());
        }
    } else {
        bail!("{}: not in a Norgolith site directory", "Could not display the theme info".bold());
    }
    Ok(())
}

pub async fn handle(subcommand: &ThemeCommands) -> Result<()> {
    match subcommand {
        ThemeCommands::Pull { repo, version, pin } => pull_theme(repo, version, *pin).await,
        ThemeCommands::Update => update_theme().await,
        ThemeCommands::Rollback => rollback_theme().await,
        ThemeCommands::Init => init_theme().await,
        ThemeCommands::Info => show_theme_info().await,
    }
}
