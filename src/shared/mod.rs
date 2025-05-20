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

/// Render full norg page by converting it to HTML and applying tera template
pub async fn render_norg_page(
    tera: &Tera,
    metadata: &toml::Value,
    posts: &[toml::Value],
    config: &SiteConfig,
) -> Result<String> {
    let content = metadata.get("raw").and_then(|v| v.as_str()).unwrap_or("");
    let layout = metadata
        .get("layout")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    let mut context = Context::new();
    context.insert("config", config);
    context.insert("content", content);
    context.insert("metadata", metadata);
    context.insert("posts", posts);

    tera.render(&format!("{}.html", layout), &context)
        .map_err(|e| {
            // Store the reason why Tera failed to render the template
            let msg = format!("Failed to render template for '{}'", "").bold();
            if let Some(source) = e.source() {
                eyre!("{msg}: {source}")
            } else {
                eyre!(msg)
            }
        })
}

pub async fn render_category_index(
    tera: &Tera,
    posts: &[toml::Value],
    config: &SiteConfig,
) -> Result<String> {
    let categories = collect_all_posts_categories(posts).await;
    let context = {
        let mut ctx = Context::new();
        ctx.insert("config", config);
        ctx.insert("posts", posts);
        ctx.insert("categories", &categories.iter().collect::<Vec<_>>());
        ctx
    };

    tera.render("categories.html", &context).map_err(|e| {
        let internal_err = e.source().unwrap();
        eyre!(
            "{}: {}",
            "Failed to render categories index".bold(),
            internal_err
        )
    })
}

pub async fn render_category_page(
    tera: &Tera,
    name: &str,
    cat_posts: &[&toml::Value],
    config: &SiteConfig,
) -> Result<String> {
    let context = {
        let mut ctx = Context::new();
        ctx.insert("config", config);
        ctx.insert("category", name);
        ctx.insert("posts", cat_posts);
        ctx
    };
    tera.render("category.html", &context).map_err(|e| {
        let internal_err = e.source().unwrap();
        eyre!(
            "{}: {}",
            "Failed to render category page".bold(),
            internal_err
        )
    })
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
            "Norg file not found for".bold(),
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
/// * `content_dir` - The content directory.
/// * `path` - The path to the content file.
/// * `schema` - The schema to validate the metadata against.
/// * `as_warnings` - Whether to format errors as warnings or errors.
///
/// # Returns
/// * `Result<String>` - Empty String if the validation did not find any error, an String containing all the errors otherwise.
pub async fn validate_content_metadata(
    content_dir: &Path,
    path: &Path,
    metadata: &toml::Value,
    schema: &ContentSchema,
    as_warnings: bool,
) -> Result<String> {
    let relative_path = path.strip_prefix(content_dir).unwrap();
    // We do not need to do anything with the metadata permalink here so we pass an empty string to it
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
