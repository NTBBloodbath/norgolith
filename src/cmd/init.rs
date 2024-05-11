use anyhow::{anyhow, Result};
use tokio::fs;

/// Create basic site configuration TOML
async fn create_config(root: &str) -> Result<()> {
    let site_config = format!(
        r#"rootUrl = '{}'
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
    let norg_metadata = format!(
        r#"@document.meta
title: hello norgolith
description: This is my first post made with Norgolith :D
authors: {}
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

async fn create_directories(path: &str) -> Result<()> {
    // Create the site directories and all their parent directories if required
    let directories = vec!["content", "templates", "assets", "theme"];
    for dir in directories {
        // TBD: add Windows separator support
        fs::create_dir_all(path.to_owned() + "/" + dir).await?;
    }

    Ok(())
}

pub async fn init(name: &String) -> Result<()> {
    let path_exists = fs::try_exists(name).await?;

    if path_exists {
        // Get the canonical (absolute) path to the existing site root
        let path = fs::canonicalize(name).await?;
        return Err(
            anyhow!("The target directory {} already exists.", path.display())
                .context("could not initialize the new Norgolith site"),
        );
    } else {
        // Create site directories
        create_directories(name).await?;

        // Create initial files
        // TBD: Basic HTML templates and start work with Tera
        create_config(name).await?;
        create_index_norg(name).await?;

        // Get the canonical (absolute) path to the new site root
        let path = fs::canonicalize(name).await?;
        let init_message = format!(
            r#"Congratulations, your new Norgolith site was created in {}

Please make sure to read the documentation at {}.
            "#,
            path.display(),
            "https://foobar.wip/"
        );
        println!("{}", init_message);
    }

    Ok(())
}
