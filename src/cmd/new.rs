use eyre::{bail, Result};
use indoc::formatdoc;
use tokio::fs::{create_dir_all, metadata, write};

use crate::fs;

/// Create a new norg document
async fn create_norg_document(path: &str, name: &str) -> Result<()> {
    // FIXME: this code is really dirty, should be re-organized or refactored later.
    let mut title: String;
    // Remove the '/home/foo/.../my-site/content' from the file path as it is redundant for the title generation
    let path_offset = path.find("content").unwrap_or(path.len());
    if path[path_offset + "content".len()..].is_empty() {
        if name == "index.norg" {
            title = String::from("index");
        } else {
            title = String::from(&name[0..name.len() - 5]);
        }
    } else {
        let doc_name = &name[0..name.len() - 5]; // 'index.norg' -> 'index'
        let title_path = [
            String::from(&path[path_offset + "content".len() + 1..]),
            String::from(doc_name),
        ]
        .join("/");
        title = title_path.replace(['-', '_'], " ");
        if name == "index.norg" {
            // 'foo/index' -> 'foo'
            title = String::from(&title[0..title.len() - doc_name.len() - 1]);
        }
        title = title.replace('/', " | ");
    }

    let creation_date =
        chrono::offset::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let norg_document = formatdoc!(
        r#"
        @document.meta
        title: {}
        description:
        authors: [
          {}
        ]
        categories: []
        created: {}
        updated: {}
        draft: true
        version: 1.1.1
        @end

        * {}
          Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut
          labore et dolore magna aliqua. Lobortis scelerisque fermentum dui faucibus in ornare."#,
        title,
        whoami::username(),
        creation_date,
        creation_date,
        title,
    );
    // TBD: add Windows separator support
    write([path, name].join("/"), norg_document).await?;

    Ok(())
}

pub async fn new(kind: &str, name: &str, open: bool) -> Result<()> {
    // Save the asset name in a variable to re-use it later
    let mut file_path = name;
    let file_extension = name
        .split('.')
        .collect::<Vec<&str>>()
        .last()
        .unwrap()
        .to_owned();

    // 'norgolith new' only supports writing JS, CSS and Norg files as content/assets
    if ![
        String::from("js"),
        String::from("css"),
        String::from("norg"),
    ]
    .contains(&String::from(file_extension))
    {
        bail!("Unable to create site asset: invalid asset file extension provided (an asset can be a 'js', 'css' or 'norg' file)");
    }

    // If the name contains directories then split it and create the directories if needed later
    let mut subdirs: Vec<&str> = Vec::new();
    if name.contains('/') {
        subdirs = name.split('/').collect();
        file_path = subdirs.pop().unwrap(); // Extract the filename from the path
    }

    // Find Norgolith site root directory
    let mut current_dir = std::env::current_dir()?;
    let found_site_root = fs::find_in_previous_dirs("file", "norgolith.toml", &mut current_dir).await?;

    let site_root = match found_site_root {
        Some(mut root) => {
            root.pop(); // Remove the 'norgolith.toml' file path from the absolute path to the site root directory
            root.to_str().unwrap().to_string()
        }
        None => bail!("Unable to create site asset: not in a Norgolith site directory"),
    };

    match kind {
        "js" | "css" | "content" => {
            let asset_dir = if kind == "js" || kind == "css" {
                String::from("assets")
            } else {
                String::from("content")
            };
            // '/home/foo/.../my-site/content|assets'
            let mut full_path = vec![site_root.clone(), asset_dir];
            // '/home/foo/.../my-site/assets/js|css'
            if kind == "js" || kind == "css" {
                full_path.push(String::from(kind));
            }
            // '/home/foo/.../my-site/content/third-post' | '/home/foo/.../my-site/assets/js|css/foo'
            if !subdirs.is_empty() {
                full_path.push(subdirs.join("/"));
            }

            // Create the subdirectories if they don't exist yet
            if metadata(full_path.join("/")).await.is_err() {
                create_dir_all(full_path.join("/")).await?;
            }

            // NOTE: JS/CSS assets creation is more simple, they do not require an external function
            if kind == "content" {
                create_norg_document(&full_path.join("/"), file_path).await?;
            } else {
                tokio::fs::File::create(full_path.join("/")).await?;
            }

            // Add the filename to the mix
            full_path.push(String::from(file_path));

            // Open the file using the preferred system editor if '--open' has been passed to the command
            if open {
                match open::that(full_path.join("/")) {
                    Ok(()) => println!(
                        "Opening '{}' with your preferred system editor ...",
                        full_path.join("/")
                    ),
                    Err(e) => bail!("Unable to open the asset '{}': {}", full_path.join("/"), e),
                }
            }
        }
        // XXX: it is impossible to reach an invalid asset kind because it is already filtered in the cli module
        _ => unreachable!(),
    }

    Ok(())
}
