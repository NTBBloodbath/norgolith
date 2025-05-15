use std::collections::HashSet;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Instant;

use colored::Colorize;
use eyre::{eyre, Result};
use tera::{Context, Tera};
use tracing::error;
use walkdir::WalkDir;

use crate::config::SiteConfig;
use crate::converter;
use crate::schema::{format_errors, validate_metadata, ContentSchema};

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
    if file_path.extension().unwrap_or_default() != "norg" {
        return Ok(())
    }
    if !tokio::fs::try_exists(file_path).await? {
        return Ok(())
    }
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
        let (norg_html, toc) = converter::html::convert(&norg_document, root_url);

        // Convert metadata
        let norg_meta =
            converter::meta::convert(&norg_document, Some(converter::html::toc_to_toml(&toc)))?;
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
    let mut tera = Tera::default();

    // Loading theme templates first allows the user to extend the theme templates using their own user-defined
    // templates aka inheriting from the theme templates.
    if tokio::fs::try_exists(&theme_templates_dir).await? {
        let theme_glob = format!("{}/**/*.html", theme_templates_dir.display());
        let theme_tera =
            Tera::parse(&theme_glob).map_err(|e| eyre!("Error parsing theme templates: {}", e))?;
        tera.extend(&theme_tera)?;
    }

    // Load user's templates
    let user_glob = format!("{}/**/*.html", templates_dir);
    let user_tera =
        Tera::parse(&user_glob).map_err(|e| eyre!("Error parsing user templates: {}", e))?;
    tera.extend(&user_tera)?;

    let xml_glob = format!("{}/**/*.xml", templates_dir);
    let xml_tera =
        Tera::parse(&xml_glob).map_err(|e| eyre!("Error parsing user XML templates: {}", e))?;
    tera.extend(&xml_tera)?;

    tera.build_inheritance_chains()
        .map_err(|e| eyre!("{}: {}", "Failed to build templates inheritance".bold(), e))?;

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
/// * `path` - The path to the norg file.
/// * `rel_path` - Relative path to the norg file without the content directory prefix.
/// * `routes_url` - The URL used for routing.
///
/// # Returns
/// * `toml::Value` - The parsed metadata or an empty table if an error occurs.
pub async fn load_metadata(path: PathBuf, rel_path: PathBuf, routes_url: &str) -> toml::Value {
    let Ok(content) = tokio::fs::read_to_string(&path).await else {
        error!(
            "{} {}",
            "Metadata file not found for".bold(),
            rel_path.display()
        );
        return toml::Value::Table(toml::map::Map::new());
    };
    let (html, toc) = converter::html::convert(&content, routes_url);
    let mut metadata = converter::meta::convert(&content, Some(converter::html::toc_to_toml(&toc)))
        .unwrap_or(toml::Value::Table(toml::map::Map::new()));
    let permalink = {
        let mut permalink_path = rel_path.with_extension("");
        if permalink_path
            .file_name()
            .is_some_and(|name| name == "index")
        {
            permalink_path = permalink_path
                .parent()
                .unwrap_or(Path::new(""))
                .to_path_buf();
        }
        let permalink = permalink_path.to_string_lossy();
        if permalink.is_empty() {
            format!("{}/", routes_url)
        } else {
            format!("{}/{}/", routes_url, permalink)
        }
    };
    if let toml::Value::Table(ref mut table) = metadata {
        // Convert TOML datetimes to RFC3339 strings
        for (_k, v) in table.iter_mut() {
            if let toml::Value::Datetime(dt) = v {
                *v = toml::Value::String(dt.to_string());
            }
        }
        table.insert("raw".to_string(), toml::Value::String(html));
        table.insert("permalink".to_string(), toml::Value::String(permalink));
    }

    metadata
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

/// Collects all unique categories from post metadata
pub async fn collect_all_posts_categories(posts: &[toml::Value]) -> HashSet<String> {
    let mut categories = HashSet::new();

    for post in posts {
        if let Some(cats) = post.get("categories").and_then(|v| v.as_array()) {
            for cat in cats {
                if let Some(cat_str) = cat.as_str() {
                    categories.insert(cat_str.to_lowercase());
                }
            }
        }
    }

    categories
}

pub async fn collect_all_posts_metadata(
    content_dir: &Path,
    routes_url: &str,
) -> Result<Vec<toml::Value>> {
    let mut posts = Vec::new();

    for entry in WalkDir::new(content_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            let is_norg_file = path.extension().is_some_and(|ext| ext == "norg");
            let is_post = path
                .strip_prefix(content_dir)
                .is_ok_and(|p| p.starts_with("posts") && *p != PathBuf::from("posts/index.norg"));
            is_norg_file && is_post
        })
    {
        let path = entry.path().to_path_buf();
        let rel_path = path.strip_prefix(content_dir)?.to_path_buf();

        let metadata = load_metadata(path, rel_path, routes_url).await;

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

/// Generates category listing pages
pub async fn generate_category_pages(
    tera: &Tera,
    public_dir: &Path,
    posts: &[toml::Value],
    config: &SiteConfig,
) -> Result<()> {
    let categories = collect_all_posts_categories(posts).await;
    let categories_dir = public_dir.join("categories");

    // Generate main categories index only if the site has posts
    if !posts.is_empty() {
        let mut context = Context::new();
        context.insert("config", config);
        context.insert("posts", &posts);
        context.insert("categories", &categories.iter().collect::<Vec<_>>());

        let content = tera.render("categories.html", &context).map_err(|e| {
            let internal_err = e.source().unwrap();
            eyre!(
                "{}: {}",
                "Failed to render categories index".bold(),
                internal_err
            )
        })?;

        tokio::fs::create_dir_all(&categories_dir).await?;
        tokio::fs::write(categories_dir.join("index.html"), content).await?;
    }

    // Generate individual category pages
    for category in categories {
        let cat_posts: Vec<_> = posts
            .iter()
            .filter(|post| {
                post.get("categories")
                    .and_then(|c| c.as_array())
                    .map(|cats| cats.iter().any(|c| c.as_str() == Some(category.as_str())))
                    .unwrap_or(false)
            })
            .collect();

        let mut context = Context::new();
        context.insert("config", config);
        context.insert("category", &category);
        context.insert("posts", &cat_posts);

        let cat_dir = categories_dir.join(&category);
        tokio::fs::create_dir_all(&cat_dir).await?;

        let content = tera.render("category.html", &context).map_err(|e| {
            let internal_err = e.source().unwrap();
            eyre!(
                "{}: {}",
                "Failed to render category page".bold(),
                internal_err
            )
        })?;

        tokio::fs::write(cat_dir.join("index.html"), content).await?;
    }

    Ok(())
}
