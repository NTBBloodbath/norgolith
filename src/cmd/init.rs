use std::collections::HashMap;
use std::path::PathBuf;

use comfy_table::modifiers::UTF8_SOLID_INNER_BORDERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, ContentArrangement, Table};
use eyre::{bail, eyre, Result};
use indoc::formatdoc;
use inquire::Text;
use tokio::fs;
use tracing::{debug, info, instrument};

/// Create basic site configuration TOML
#[instrument(level = "debug", skip(root, root_url, language, title))]
async fn create_config(root: &str, root_url: &str, language: &str, title: &str) -> Result<()> {
    debug!("Creating site configuration");
    let config_path = PathBuf::from(root).join("norgolith.toml");
    debug!(config_path = %config_path.display(), "Writing config file");

    let site_config = formatdoc!(
        r#"
        rootUrl = '{}'
        language = '{}'
        title = '{}'
        author = '{}'

        # Code blocks highlighting
        [highlighter]
        enable = false
        # engine = 'prism' # Can be 'prism' or 'hljs'. Defaults to 'prism'"#,
        root_url, // this is the default port
        language,
        title,
        whoami::username()
    );

    fs::write(config_path, site_config)
        .await
        .map_err(|e| {
            eyre!("Failed to write config file: {}", e)
        })?;

    info!("Created norgolith.toml");
    Ok(())
}

/// Create a basic hello world norg document
#[instrument(level = "debug", skip(root))]
async fn create_index_norg(root: &str) -> Result<()> {
    debug!("Creating index.norg");
    let content_path = PathBuf::from(root).join("content/index.norg");
    debug!(content_path = %content_path.display(), "Writing index.norg");

    let creation_date =
        chrono::offset::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let norg_index = format!(
        "{}",
        format_args!(
            include_str!("../resources/content/index.norg"),
            username = whoami::username(),
            created_at = creation_date,
            updated_at = creation_date
        )
    );
    fs::write(content_path, norg_index)
        .await
        .map_err(|e| {
            eyre!("Failed to write index.norg: {}", e)
        })?;

    info!("Created index.norg");
    Ok(())
}

/// Create basic HTML templates
#[instrument(level = "debug", skip(root))]
async fn create_html_templates(root: &str) -> Result<()> {
    debug!("Creating HTML templates");

    // TODO: add 'head.html', 'footer.html' for more granular content?
    let templates = HashMap::from([
        ("base", include_str!("../resources/templates/base.html")),
        (
            "default",
            include_str!("../resources/templates/default.html"),
        ),
    ]);

    let templates_dir = PathBuf::from(root).join("templates");
    debug!(templates_dir = %templates_dir.display(), "Creating templates directory");

    for (&name, &contents) in templates.iter() {
        let template_path = templates_dir.join(name.to_owned() + ".html");
        fs::write(template_path, contents)
            .await
            .map_err(|e| {
                eyre!("Failed to write template {}: {}", name, e)
            })?;
    }

    info!("Created HTML templates");
    Ok(())
}

#[instrument(level = "debug", skip(root))]
async fn create_assets(root: &str) -> Result<()> {
    debug!("Creating assets");
    let assets_dir = PathBuf::from(root).join("assets");
    debug!(assets_dir = %assets_dir.display(), "Creating assets directory");

    let base_style = include_str!("../resources/assets/style.css");
    let style_path = assets_dir.join("style.css");
    debug!(style_path = %style_path.display(), "Writing style.css");
    fs::write(&style_path, base_style)
        .await
        .map_err(|e| {
            eyre!("Failed to write style.css: {}", e)
        })?;

    let norgolith_logo = include_str!("../../res/norgolith.svg");
    let logo_path = assets_dir.join("norgolith.svg");
    debug!(logo_path = %logo_path.display(), "Writing norgolith.svg");
    fs::write(&logo_path, norgolith_logo)
        .await
        .map_err(|e| {
            eyre!("Failed to write norgolith.svg: {}", e)
        })?;

    info!("Created assets");
    Ok(())
}

#[instrument(level = "debug", skip(path))]
async fn create_directories(path: &str) -> Result<()> {
    debug!("Creating site directories");

    // Create the site directories and all their parent directories if required
    let directories = vec!["content", "templates", "assets", "theme", ".build"];
    for dir in directories {
        let dir_path = PathBuf::from(path).join(dir);
        debug!(dir_path = %dir_path.display(), "Creating directory");
        fs::create_dir_all(dir_path)
            .await
            .map_err(|e| {
                eyre!("Failed to create directory {}: {}", dir, e)
            })?;
    }

    info!("Created site directories");
    Ok(())
}

#[instrument(skip(name, prompt))]
pub async fn init(name: &str, prompt: bool) -> Result<()> {
    info!("Initializing new Norgoliht site: {}", name);

    let path_exists = fs::try_exists(name)
        .await
        .map_err(|e| {
            eyre!("Failed to check if path exists: {}", e)
        })?;

    if path_exists {
        // Get the canonical (absolute) path to the existing site root
        let path = fs::canonicalize(name)
            .await
            .map_err(|e| {
                eyre!("Failed to get canonnical path: {}", e)
            })?;
        bail!(
            "Could not initialize the new Norgolith site: the target directory {} already exists.",
            path.display()
        );
    } else {
        // Prompt site configuration if wanted, otherwise fallback to sane default values
        let root_url = if prompt {
            Text::new("Site URL:")
                .with_default("http://localhost:3030")
                .with_help_message("URL to your production site")
                .prompt()
                .map_err(|e| {
                    eyre!("Failed to get site URL: {}", e)
                })?
        } else {
            String::from("http://localhost:3030")
        };
        let language = if prompt {
            Text::new("Site language:")
                .with_default("en-US")
                .with_help_message("Your site language")
                .prompt()
                .map_err(|e| {
                    eyre!("Failed to get site language: {}", e)
                })?
        } else {
            String::from("en-US")
        };
        let title = if prompt {
            Text::new("Site title:")
                .with_default(name)
                .with_help_message("Site title")
                .prompt()
                .map_err(|e| {
                    eyre!("Failed to get site title: {}", e)
                })?
        } else {
            String::from(name)
        };

        // Create site structure
        create_directories(name).await?;
        create_config(name, &root_url, &language, &title).await?;
        create_index_norg(name).await?;
        create_html_templates(name).await?;
        create_assets(name).await?;

        // Get the canonical (absolute) path to the new site root
        let path = fs::canonicalize(name)
            .await
            .map_err(|e| {
                eyre!("Failed to get canonical path: {}", e)
            })?;

        // Create structure table
        let mut structure_table = Table::new();
        structure_table
            .load_preset(UTF8_FULL)
            .apply_modifier(UTF8_SOLID_INNER_BORDERS)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_width(60)
            .set_header(vec!["Directory", "Description"])
            .add_row(vec![
                Cell::new("content"),
                Cell::new("Norg site content files"),
            ])
            .add_row(vec![Cell::new("templates"), Cell::new("HTML templates")])
            .add_row(vec![
                Cell::new("assets"),
                Cell::new("Site assets (JS, CSS, images, etc)"),
            ])
            .add_row(vec![Cell::new("theme"), Cell::new("Site theme files")])
            .add_row(vec![Cell::new("public"), Cell::new("Production artifacts")])
            .add_row(vec![Cell::new(".build"), Cell::new("Dev server artifacts")]);

        let init_message = formatdoc!(
            r#"
            Congratulations, your new Norgolith site was created in {}

            Your new site structure:
            {}

            Please make sure to read the documentation at {}"#,
            path.display(),
            structure_table,
            "https://ntbbloodbath.github.io/norgolith"
        );
        info!("{}", init_message);
    }

    Ok(())
}
