use std::path::{Path, PathBuf};
use std::time::Instant;

use eyre::{bail, eyre, Result};
use tera::Tera;
use tracing::{error, info};
use walkdir::WalkDir;

use crate::converter;
use crate::schema::{format_errors, validate_metadata, ContentSchema};

pub async fn get_content(name: &str) -> Result<(String, PathBuf)> {
    let build_path = Path::new(".build");
    let mut candidates: Vec<PathBuf> = Vec::new();

    // Normalize path by trimming slashes
    let clean_name = name.trim_matches('/');

    if clean_name.is_empty() {
        // Root path
        candidates.push(build_path.join("index.html"));
    } else {
        // Generate potential file paths
        candidates.push(build_path.join(format!("{}.html", clean_name))); // /docs -> docs.html
        candidates.push(build_path.join(clean_name).join("index.html")); // /docs -> docs/index.html
    }

    // Try candidates in order
    for path in &candidates {
        if tokio::fs::try_exists(path).await? {
            return Ok((tokio::fs::read_to_string(path).await?, path.to_path_buf()));
        }
    }

    Err(eyre::eyre!("Content not found for path: {}", name))
}

/// Recursively converts all the norg files in the content directory
pub async fn convert_content(
    content_dir: &Path,
    convert_drafts: bool,
    root_url: &str,
) -> Result<()> {
    async fn process_entry(
        entry: tokio::fs::DirEntry,
        content_dir: &Path,
        convert_drafts: bool,
        root_url: &str,
    ) -> Result<()> {
        let path = entry.path();
        if path.is_dir() {
            // Process directory recursively
            let mut content_stream = tokio::fs::read_dir(&path).await?;
            while let Some(entry) = content_stream.next_entry().await? {
                Box::pin(process_entry(entry, content_dir, convert_drafts, root_url)).await?;
            }
        } else {
            convert_document(&path, content_dir, convert_drafts, root_url).await?;
        }
        Ok(())
    }

    let mut content_stream = tokio::fs::read_dir(content_dir).await?;
    while let Some(entry) = content_stream.next_entry().await? {
        Box::pin(process_entry(entry, content_dir, convert_drafts, root_url)).await?;
    }

    Ok(())
}

pub async fn convert_document(
    file_path: &Path,
    content_dir: &Path,
    convert_drafts: bool,
    root_url: &str,
) -> Result<()> {
    if file_path.extension().unwrap_or_default() == "norg"
        && tokio::fs::try_exists(file_path).await?
    {
        let mut should_convert = true;
        let mut should_write_meta = true;

        // Preserve directory structure relative to content directory
        let relative_path = file_path.strip_prefix(content_dir).map_err(|_| {
            eyre!(
                "File {:?} is not in content directory {:?}",
                file_path,
                content_dir
            )
        })?;

        let html_file_path = Path::new(".build")
            .join(relative_path)
            .with_extension("html");
        let meta_file_path = html_file_path.with_extension("meta.toml");

        // Convert html content
        let norg_document = tokio::fs::read_to_string(file_path).await?;
        let (norg_html, toc) = converter::html::convert(norg_document.clone(), root_url);

        // Convert metadata
        let norg_meta = converter::meta::convert(
            &norg_document,
            Some(converter::html::toc_to_toml(&toc))
        )?;
        let meta_toml = toml::to_string_pretty(&norg_meta)?;

        // Check if the current document is a draft post and also whether we should finish the conversion
        // NOTE: content is not marked as draft by default
        if toml::Value::as_bool(norg_meta.get("draft").unwrap_or(&toml::Value::from(false)))
            .expect("draft metadata field should be a boolean")
            && !convert_drafts
        {
            return Ok(());
        }

        // Check existing metadata only if file exists
        if tokio::fs::try_exists(&meta_file_path).await? {
            let meta_content = tokio::fs::read_to_string(&meta_file_path).await?;
            should_write_meta = meta_toml != meta_content;
        }

        // Check existing content only if file exists
        if tokio::fs::try_exists(&html_file_path).await? {
            let html_content = tokio::fs::read_to_string(&html_file_path).await?;
            should_convert = norg_html != html_content;
        }

        if should_convert || should_write_meta {
            // Create parent directories if needed
            if let Some(parent) = html_file_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            // XXX: maybe these info makes stuff too verbose? Modifying a norg file already triggers two stdout messages
            if should_convert {
                // info!("[server] Converting norg file: {}", relative_path.display());
                tokio::fs::write(&html_file_path, norg_html).await?;
            }
            if should_write_meta {
                // info!("[server] Converting norg meta: {}", relative_path.display());
                tokio::fs::write(&meta_file_path, meta_toml).await?;
            }
        }
    }

    Ok(())
}

pub async fn cleanup_orphaned_build_files(content_dir: &Path) -> Result<()> {
    let build_dir = Path::new(".build");
    if !build_dir.exists() {
        return Ok(());
    }

    let mut stack = vec![build_dir.to_path_buf()];

    while let Some(current_dir) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&current_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_dir() {
                stack.push(path);
            } else if path.extension().map(|e| e == "html").unwrap_or(false) {
                let relative_path = path.strip_prefix(build_dir)?;
                let norg_path = content_dir.join(relative_path).with_extension("norg");

                if !norg_path.exists() {
                    // Delete HTML and meta files
                    let meta_path = path.with_extension("meta.toml");

                    tokio::fs::remove_file(&path).await?;
                    if tokio::fs::try_exists(&meta_path).await? {
                        tokio::fs::remove_file(&meta_path).await?;
                    }

                    info!("Cleaned orphaned build file: {}", path.display());
                }
            }
        }
    }

    Ok(())
}

pub fn get_elapsed_time(instant: Instant) -> String {
    let duration = instant.elapsed();
    let secs = duration.as_secs_f64();

    if secs < 1.0 {
        format!("{:.0}ms", secs * 1000.0)
    } else {
        format!("{:.1}s", secs)
    }
}

pub async fn init_tera(templates_dir: &str, theme_templates_dir: &Path) -> Result<Tera> {
    // Initialize Tera with the user-defined templates first
    let mut tera = match Tera::parse(&(templates_dir.to_owned() + "/**/*.html")) {
        Ok(t) => t,
        Err(e) => bail!(
            "Error parsing templates from the templates directory: {}",
            e
        ),
    };

    // Theme templates will override the user-defined templates by design if they are named exactly
    // the same in both the user's templates directory and the theme templates directory
    if tokio::fs::try_exists(&theme_templates_dir).await? {
        let tera_theme =
            match Tera::parse(&(theme_templates_dir.display().to_string() + "/**/*.html")) {
                Ok(t) => t,
                Err(e) => bail!("Error parsing templates from themes: {}", e),
            };
        tera.extend(&tera_theme)?;
    }
    tera.build_inheritance_chains()
        .map_err(|e| eyre!("Failed to build templates inheritance: {}", e))?;

    // Register functions
    tera.register_function("now", crate::tera_functions::NowFunction);
    tera.register_function("generate_toc", crate::tera_functions::GenerateToc);

    Ok(tera)
}

/// Loads metadata from a TOML file.
///
/// This function reads metadata from a TOML file and returns it as a `toml::Value`.
/// If the file cannot be read or parsed, it logs the error and returns an empty table.
///
/// # Arguments
/// * `path` - The path to the metadata file.
/// * `rel_path` - Relative path to the metadata file without the build directory prefix.
/// * `routes_url` - The URL used for routing.
///
/// # Returns
/// * `toml::Value` - The parsed metadata or an empty table if an error occurs.
pub async fn load_metadata(path: PathBuf, rel_path: PathBuf, routes_url: &str) -> toml::Value {
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => {
            let mut value = toml::from_str(&content).unwrap_or_else(|e| {
                error!("Metadata parse error: {}", e);
                toml::Value::Table(toml::map::Map::new())
            });

            // Convert TOML datetimes to RFC3339 strings
            if let Some(table) = value.as_table_mut() {
                for (_k, v) in table.iter_mut() {
                    if let toml::Value::Datetime(dt) = v {
                        *v = toml::Value::String(dt.to_string());
                    }
                }
            }

            // Generate permalink from file structure
            // Remove .meta.toml
            let mut permalink_path = rel_path.with_extension("").with_extension("");

            // Handle index pages
            if let Some(file_name) = permalink_path.file_name() {
                if file_name == "index" {
                    permalink_path = permalink_path
                        .parent()
                        .unwrap_or_else(|| Path::new(""))
                        .to_path_buf();
                }
            }

            // Convert to URL path
            let permalink_str = permalink_path
                .to_string_lossy()
                .trim_start_matches('/')
                .to_string();

            let permalink = if permalink_str.is_empty() {
                format!("{}/", routes_url)
            } else {
                format!("{}/{}/", routes_url, permalink_str)
            };

            // Add permalink and html content to metadata
            if let toml::Value::Table(ref mut table) = value {
                table.insert("permalink".to_string(), toml::Value::String(permalink));
            }

            value
        }
        Err(e) => {
            error!("Metadata file not found: {}", e);
            toml::Value::Table(toml::map::Map::new())
        }
    }
}

/// Validates content metadata against a schema.
///
/// This function validates the metadata of a content file against a provided schema.
/// If validation errors are found, they are logged in a user-friendly format.
///
/// # Arguments
/// * `path` - The path to the content file.
/// * `build_dir` - The build directory.
/// * `content_dir` - The content directory.
/// * `schema` - The schema to validate the metadata against.
/// * `as_warnings` - Whether to format errors as warnings or errors.
///
/// # Returns
/// * `Result<String>` - Empty String if the validation did not find any error, an String containing all the errors otherwise.
pub async fn validate_content_metadata(
    path: &Path,
    build_dir: &Path,
    content_dir: &Path,
    schema: &ContentSchema,
    as_warnings: bool,
) -> Result<String> {
    let relative_path = path.strip_prefix(content_dir).unwrap();
    let meta_path = build_dir.join(relative_path).with_extension("meta.toml");

    let rel_path = meta_path
        .clone()
        .strip_prefix(build_dir)
        .map(|p| p.to_path_buf())?;
    // We do not need to do anything with the metadata permalink here so we pass an empty string to it
    let metadata = load_metadata(meta_path, rel_path, "").await;
    let metadata_map = metadata
        .as_table()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let content_path = relative_path
        .to_str()
        .unwrap()
        .replace('\\', "/")
        .trim_end_matches(".norg")
        .to_string();

    let schema_nodes = schema.resolve_path(&content_path);
    let merged_schema = ContentSchema::merge_hierarchy(&schema_nodes);
    let errors = validate_metadata(&metadata_map, &merged_schema);

    if !errors.is_empty() {
        return Ok(format_errors(path, &content_path, &errors, as_warnings));
    }
    Ok(String::new())
}

pub async fn collect_all_posts_metadata(
    build_dir: &Path,
    routes_url: &str,
) -> Result<Vec<toml::Value>> {
    let mut posts = Vec::new();

    for entry in WalkDir::new(build_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            let file_name = path.file_name().and_then(|name| name.to_str());
            let is_meta_file = file_name.is_some_and(|name| name.ends_with(".meta.toml"));
            let is_post = path
                .strip_prefix(build_dir)
                .is_ok_and(|p| p.starts_with("posts") && !p.ends_with("posts/index.meta.toml"));
            is_post && is_meta_file
        })
    {
        let meta_path = entry.path();
        let rel_path = meta_path.strip_prefix(build_dir)?.to_path_buf();
        let mut metadata = load_metadata(entry.path().to_path_buf(), rel_path, routes_url).await;

        // TODO: this won't hot reload if the content changes, should be passed as an argument instead
        // Get the raw html content
        let html_file = entry.path().with_extension("").with_extension("html");
        let html = tokio::fs::read_to_string(&html_file).await?;

        // Add html content to metadata
        if let toml::Value::Table(ref mut table) = metadata {
            table.insert("raw".to_string(), toml::Value::String(html));
        }
        posts.push(metadata);
    }

    posts.sort_by(|a, b| {
        let a_date = a.get("date").and_then(|v| v.as_str()).unwrap_or_default();
        let b_date = b.get("date").and_then(|v| v.as_str()).unwrap_or_default();

        let parse_date = |s: &str| {
            chrono::DateTime::parse_from_str(s, "%Y-%m-%d")
                .unwrap_or_else(|_| chrono::DateTime::from_timestamp(0, 0).unwrap().into())
                .with_timezone(&chrono::Utc)
        };

        parse_date(b_date).cmp(&parse_date(a_date))
    });

    Ok(posts)
}
