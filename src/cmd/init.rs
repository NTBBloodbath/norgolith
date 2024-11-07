use comfy_table::modifiers::UTF8_SOLID_INNER_BORDERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, ContentArrangement, Table};
use eyre::{bail, Result};
use indoc::formatdoc;
use tokio::fs;

/// Create basic site configuration TOML
async fn create_config(root: &str) -> Result<()> {
    let site_config = formatdoc!(r#"
        rootUrl = '{}'
        language = '{}'
        title = '{}'"#,
        "http://localhost:3030", // this is the default port
        "en-us",
        root.to_owned()
    );
    // TBD: add Windows separator support
    fs::write(root.to_owned() + "/norgolith.toml", site_config).await?;

    Ok(())
}

/// Create a basic hello world norg document
async fn create_index_norg(root: &str) -> Result<()> {
    let creation_date =
        chrono::offset::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let norg_metadata = formatdoc!(r#"
        @document.meta
        title: hello norgolith
        description: This is my first post made with Norgolith :D
        authors: [
          {}
        ]
        categories: []
        created: {}
        updated: {}
        draft: true
        version: 1.1.1
        @end

        * Hello world
          Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut
          labore et dolore magna aliqua. Lobortis scelerisque fermentum dui faucibus in ornare."#,
        whoami::username(),
        creation_date,
        creation_date
    );
    // TBD: add Windows separator support
    fs::write(root.to_owned() + "/content/index.norg", norg_metadata).await?;

    Ok(())
}

/// Create basic HTML templates
async fn create_html_templates(root: &str) -> Result<()> {
    // TODO: add 'head.html', 'footer.html'
    // TODO: extract some information like language and title from the site config?
    let base_template = formatdoc!(r#"
        <!DOCTYPE html>
        <html lang="en">
        <head>
            {{% block head %}}
            <meta charset="UTF-8" />
            <meta name="viewport" content="width=device-width, initial-scale=1.0" />
            <link rel="stylesheet" href="style.css" />
            <title>{{% block title %}}{{% endblock title %}} - {}</title>
            {{% endblock head %}}
        </head>
        <body>
            <div id="content">{{% block content %}}{{% endblock content %}}</div>
            <div id="footer">
                {{% block footer %}}
                &copy; Copyright {} by {}.
                {{% endblock footer %}}
            </div>
        </body>
        </html>"#,
        root.to_owned(),
        chrono::offset::Local::now().format("%Y"),
        whoami::username()
    );
    // TBD: add Windows separator support
    fs::write(root.to_owned() + "/templates/base.html", base_template).await?;

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
        // TBD: Basic HTML templates and start work with Tera
        create_config(name).await?;
        create_index_norg(name).await?;
        create_html_templates(name).await?;

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

        let init_message = formatdoc!(r#"
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
