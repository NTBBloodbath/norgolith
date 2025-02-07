use std::collections::HashMap;
use std::path::PathBuf;

use comfy_table::modifiers::UTF8_SOLID_INNER_BORDERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, ContentArrangement, Table};
use eyre::{bail, Result};
use indoc::formatdoc;
use tokio::fs;

/// Create basic site configuration TOML
async fn create_config(root: &str) -> Result<()> {
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
        "http://localhost:3030", // this is the default port
        "en-us",
        root.to_owned(),
        whoami::username()
    );
    // TBD: add Windows separator support
    fs::write(root.to_owned() + "/norgolith.toml", site_config).await?;

    Ok(())
}

/// Create a basic hello world norg document
async fn create_index_norg(root: &str) -> Result<()> {
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
    // TBD: add Windows separator support
    fs::write(root.to_owned() + "/content/index.norg", norg_index).await?;

    Ok(())
}

/// Create basic HTML templates
async fn create_html_templates(root: &str) -> Result<()> {
    // TODO: add 'head.html', 'footer.html' for more granular content?
    let templates = HashMap::from([
        ("base", include_str!("../resources/templates/base.html")),
        (
            "default",
            include_str!("../resources/templates/default.html"),
        ),
    ]);

    let templates_dir = PathBuf::from(root).join("templates");
    for (&name, &contents) in templates.iter() {
        let template_path = templates_dir.join(name.to_owned() + ".html");
        fs::write(template_path, contents).await?;
    }

    Ok(())
}

async fn create_assets(root: &str) -> Result<()> {
    let base_style = include_str!("../resources/assets/style.css");
    fs::write(root.to_owned() + "/assets/style.css", base_style).await?;

    let norgolith_logo = include_str!("../../res/norgolith.svg");
    fs::write(root.to_owned() + "/assets/norgolith.svg", norgolith_logo).await?;

    Ok(())
}

async fn create_directories(path: &str) -> Result<()> {
    // Create the site directories and all their parent directories if required
    let directories = vec!["content", "templates", "assets", "theme", ".build"];
    for dir in directories {
        // TBD: add Windows separator support
        fs::create_dir_all(path.to_owned() + "/" + dir).await?;
    }

    Ok(())
}

pub async fn init(name: &str) -> Result<()> {
    let path_exists = fs::try_exists(name).await?;

    if path_exists {
        // Get the canonical (absolute) path to the existing site root
        let path = fs::canonicalize(name).await?;
        bail!(
            "Could not initialize the new Norgolith site: the target directory {} already exists.",
            path.display()
        );
    } else {
        // Create site directories
        create_directories(name).await?;

        // Create initial files
        // TBD: Basic HTML templates
        create_config(name).await?;
        create_index_norg(name).await?;
        create_html_templates(name).await?;
        create_assets(name).await?;

        // Get the canonical (absolute) path to the new site root
        let path = fs::canonicalize(name).await?;

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
                Cell::new("Site assets (JS, CSS, favicon, etc)"),
            ])
            .add_row(vec![Cell::new("theme"), Cell::new("Site theme files")])
            .add_row(vec![Cell::new(".build"), Cell::new("Dev server artifacts")]);

        let init_message = formatdoc!(
            r#"
            Congratulations, your new Norgolith site was created in {}

            Your new site structure:
            {}

            Please make sure to read the documentation at {}."#,
            path.display(),
            structure_table,
            "https://foobar.wip/"
        );
        println!("{}", init_message);
    }

    Ok(())
}
