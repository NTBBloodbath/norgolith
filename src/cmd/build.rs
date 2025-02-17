use std::path::Path;

use eyre::{bail, Result};
use futures_util::{self, StreamExt};
use tera::{Context, Tera};
use walkdir::WalkDir;

use crate::{config, fs, shared};

async fn prepare_build_directory(root_path: &Path) -> Result<()> {
    let public_dir = root_path.join("public");
    if public_dir.exists() {
        tokio::fs::remove_dir_all(&public_dir).await?;
    }

    Ok(tokio::fs::create_dir_all(public_dir).await?)
}

async fn generate_public_build(
    tera: &Tera,
    root_path: &Path,
    site_config: config::SiteConfig,
    minify: bool,
) -> Result<()> {
    let build_dir = root_path.join(".build");
    let public_dir = root_path.join("public");
    let entries = WalkDir::new(&build_dir).into_iter().filter_map(|e| e.ok());

    // Parallel processing
    futures_util::stream::iter(entries)
        .for_each_concurrent(num_cpus::get(), |entry| {
            let build_dir = build_dir.clone();
            let public_dir = public_dir.clone();
            let site_config = site_config.clone();
            async move {
                let path = entry.path();
                if path.is_file() && path.extension().map(|e| e == "html").unwrap_or(false) {
                    let rel_path = path.strip_prefix(&build_dir).unwrap();
                    let stem = rel_path.file_stem().unwrap().to_str().unwrap();

                    // Determine output path
                    let public_path = if stem == "index" {
                        public_dir.join(rel_path)
                    } else {
                        public_dir
                            .join(rel_path.parent().unwrap())
                            .join(stem)
                            .join("index.html")
                    };

                    // Read content and metadata
                    let html = tokio::fs::read_to_string(path).await.unwrap();
                    let meta_path = path.with_extension("meta.toml");

                    // Handle metadata loading with proper error fallback
                    let metadata: toml::Value =
                        match tokio::fs::read_to_string(meta_path.clone()).await {
                            Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
                                // Fallback to empty table on parse errors
                                eprintln!("[build] Failed to parse metadata: {}", e);
                                toml::Value::Table(toml::map::Map::new())
                            }),
                            Err(e) => {
                                // Fallback to empty table if file not found
                                eprintln!("[build] Metadata file not found: {}", e);
                                toml::Value::Table(toml::map::Map::new())
                            }
                        };

                    // Do not try to build draft content for production builds
                    if toml::Value::as_bool(
                        metadata.get("draft").unwrap_or(&toml::Value::from(false)),
                    )
                    .expect("draft metadata field should be a boolean")
                    {
                        return;
                    }

                    // Get the layout (template) to render the content, fallback to default if the metadata field was not found.
                    let layout = metadata
                        .get("layout")
                        .unwrap_or(&toml::Value::from("default"))
                        .as_str()
                        .unwrap()
                        .to_owned();

                    // Build template context
                    let mut context = Context::new();
                    context.insert("content", &html);
                    context.insert("config", &site_config);
                    context.insert("metadata", &metadata);

                    // Get the template to use for rendering
                    let mut rendered = tera.render(&(layout + ".html"), &context).unwrap();

                    if minify {
                        let minify_config = minify_html::Cfg {
                            minify_js: true,
                            minify_css: true,
                            ..minify_html::Cfg::default()
                        };
                        rendered = String::from_utf8(minify_html::minify(
                            rendered.as_bytes(),
                            &minify_config,
                        ))
                        .unwrap();
                    }

                    tokio::fs::create_dir_all(public_path.parent().unwrap())
                        .await
                        .unwrap();
                    tokio::fs::write(public_path, rendered).await.unwrap();
                }
            }
        })
        .await;

    Ok(())
}

async fn copy_assets(assets_dir: &Path, root_path: &Path, minify: bool) -> Result<()> {
    let public_dir = root_path.join("public");
    let public_assets = public_dir.join("assets");

    async fn process_entry(src_path: &Path, dest_path: &Path, minify: bool) -> Result<()> {
        if src_path.is_dir() {
            // Create destination directory
            tokio::fs::create_dir_all(dest_path).await?;

            // Process all entries in the directory
            let mut entries = tokio::fs::read_dir(src_path).await?;
            while let Some(entry) = entries.next_entry().await? {
                let entry_path = entry.path();
                let entry_name = entry.file_name();
                let new_dest = dest_path.join(entry_name);

                Box::pin(process_entry(&entry_path, &new_dest, minify)).await?;
            }
        } else if minify {
            let file_ext = src_path.extension().unwrap().to_str().unwrap();

            if file_ext == "js" {
                let content = tokio::fs::read(src_path).await?;
                let mut minified = Vec::new();
                let session = minify_js::Session::new();
                minify_js::minify(
                    &session,
                    minify_js::TopLevelMode::Global,
                    &content,
                    &mut minified,
                )
                .unwrap();
                tokio::fs::write(dest_path, content).await?;
            } else if file_ext == "css" {
                let content = tokio::fs::read_to_string(src_path).await?;
                // See https://docs.rs/css-minify/0.5.2/css_minify/optimizations/enum.Level.html#variants
                let minified = css_minify::optimizations::Minifier::default()
                    .minify(&content, css_minify::optimizations::Level::Two)
                    .unwrap();
                tokio::fs::write(dest_path, minified).await?;
            } else {
                // Copy file as binary, this lets us write images and some other formats as well instead of only text files
                let content = tokio::fs::read(&src_path).await?;
                tokio::fs::write(&dest_path, content).await?;
            }
        } else {
            // Copy file as binary, this lets us write images and some other formats as well instead of only text files
            let content = tokio::fs::read(&src_path).await?;
            tokio::fs::write(&dest_path, content).await?;
        }
        Ok(())
    }

    Box::pin(process_entry(assets_dir, &public_assets, minify)).await?;

    Ok(())
}

pub async fn build(minify: bool) -> Result<()> {
    // Try to find a 'norgolith.toml' file in the current working directory and its parents
    let mut current_dir = std::env::current_dir()?;
    let found_site_root =
        fs::find_in_previous_dirs("file", "norgolith.toml", &mut current_dir).await?;

    if let Some(mut root) = found_site_root {
        let build_start = std::time::Instant::now();

        // Load site configuration, root already contains the norgolith.toml path
        let config_content = tokio::fs::read_to_string(&root).await?;
        let site_config: config::SiteConfig = toml::from_str(&config_content)?;

        // Remove `norgolith.toml` from the root path
        root.pop();
        let root_dir = root.into_os_string().into_string().unwrap();

        // Tera wants a `dir: &str` parameter for some reason instead of asking for a `&Path` or `&PathBuf`...
        let templates_dir = root_dir.clone() + "/templates";
        let content_dir = Path::new(&root_dir.clone()).join("content");
        let assets_dir = Path::new(&root_dir.clone()).join("assets");

        // Initialize Tera once
        let tera = shared::init_tera(&templates_dir).await?;

        // Prepare the public build directory
        prepare_build_directory(Path::new(&root_dir)).await?;

        // Convert the norg documents to html
        shared::convert_content(&content_dir, false, &site_config.root_url).await?;

        // Clean up orphaned files before building the site
        shared::cleanup_orphaned_build_files(&content_dir).await?;

        // Generate public HTML build
        generate_public_build(&tera, Path::new(&root_dir.clone()), site_config, minify).await?;

        // Copy site assets
        copy_assets(&assets_dir, Path::new(&root_dir.clone()), minify).await?;

        println!(
            "[build] Finished site build in {}",
            shared::get_elapsed_time(build_start)
        );
    } else {
        bail!("[build] Could not build the site: not in a Norgolith site directory");
    }

    Ok(())
}
