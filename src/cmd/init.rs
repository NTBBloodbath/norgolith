use anyhow::Result;
use chrono;
use tokio::fs;
use whoami;

/// Create a basic hello world norg document
async fn create_hello_norg(root: &String) -> Result<()> {
    let creation_date = chrono::offset::Local::now();
    let norg_metadata = format!(
        r#"@document.meta
title: hello
description: This is my first post made with Norgolith :D
authors: {}
categories: []
created: {}
updated: {}
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
    fs::write(root.clone() + "/content/hello.norg", norg_metadata).await?;

    Ok(())
}

pub async fn init(name: &String) -> Result<()> {
    let path_exists = fs::try_exists(name).await?;

    if path_exists {
        eprintln!("The target directory {} already exists.", name);
        std::process::exit(1);
    } else {
        // Create the site directories and all their parent directories if required
        let directories = vec!["content", "templates", "theme"];
        for dir in directories {
            // TBD: add Windows separator support
            fs::create_dir_all(name.clone() + "/" + dir).await?;
        }

        // Create initial files
        // TBD: Basic HTML templates and start work with Tera
        create_hello_norg(name).await?;

        // Get the canonical (absolute) path to the new site root
        let path = fs::canonicalize(name).await?;
        let init_message = format!(
            r#"
Congratulations, your new Norgolith site was created in {}

Please make sure to read the documentation at {}.
            "#,
            path.display(),
            "https://foobar.wip/"
        );
        println!("{}", init_message);
    }

    Ok(())
}
