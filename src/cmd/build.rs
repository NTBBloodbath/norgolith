use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::Instant,
};

use colored::{ColoredString, Colorize};
use eyre::{bail, eyre, Result, WrapErr};
use futures_util::{self, StreamExt};
use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
use rss::Channel;
use tera::{Context, Tera};
use tokio::sync::Mutex;
use tracing::{debug, error, instrument, warn};
use walkdir::WalkDir;

fn href_root_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#"href="(/|&#x2F;)"#).expect("valid regex"))
}

use crate::{cache::BuildCache, config, fs, shared};

/// Represents the directory structure of a Norgolith site.
///
/// This struct defines paths to key directories used during the build process,
/// including build artifacts, public output, content sources, and theme resources.
#[derive(Debug)]
struct SitePaths {
    public: PathBuf,
    content: PathBuf,
    assets: PathBuf,
    theme_assets: PathBuf,
    templates: PathBuf,
    theme_templates: PathBuf,
}

impl SitePaths {
    /// Creates a new `SitePaths` instance based on the provided root directory.
    ///
    /// Initializes paths for build artifacts, public output, content sources,
    /// assets, theme assets, and templates by combining with root subdirectories.
    ///
    /// # Arguments
    /// * `root` - Root directory containing norgolith.toml config file
    #[instrument]
    fn new(root: PathBuf) -> Self {
        debug!("Initializing site paths");
        let paths = Self {
            public: root.join("public"),
            content: root.join("content"),
            assets: root.join("assets"),
            theme_assets: root.join("theme/assets"),
            theme_templates: root.join("theme/templates"),
            templates: root.join("templates"),
        };
        debug!(?paths, "Configured site directories");
        paths
    }
}

/// Prepares the build directory by cleaning existing artifacts
///
/// # Arguments
/// * `public_dir` - build target directory of the site
#[instrument(skip(public_dir))]
async fn prepare_build_directory(public_dir: &Path) -> Result<()> {
    debug!(path = %public_dir.display(), "Preparing build directory");
    if public_dir.exists() {
        let mut entries = tokio::fs::read_dir(public_dir).await.wrap_err(format!(
            "{}: {}",
            "Failed to read existing public directory".bold(),
            public_dir.display()
        ))?;

        while let Some(entry) = entries.next_entry().await.wrap_err(format!(
            "{}: {}",
            "Failed to iterate existing public directory".bold(),
            public_dir.display()
        ))? {
            let path = entry.path();
            let file_name = path.file_name().and_then(|name| name.to_str());

            // Keep git metadata so users can treat ./public as a separate repository.
            if file_name == Some(".git") {
                debug!(path = %path.display(), "Keeping git metadata directory");
                continue;
            }

            let metadata = entry.metadata().await.wrap_err(format!(
                "{}: {}",
                "Failed to stat existing public entry".bold(),
                path.display()
            ))?;

            if metadata.is_dir() {
                tokio::fs::remove_dir_all(&path).await.wrap_err(format!(
                    "{}: {}",
                    "Failed to remove existing public directory entry".bold(),
                    path.display()
                ))?;
            } else {
                tokio::fs::remove_file(&path).await.wrap_err(format!(
                    "{}: {}",
                    "Failed to remove existing public file entry".bold(),
                    path.display()
                ))?;
            }
        }
    } else {
        debug!(path = %public_dir.display(), "Creating public directory");
        tokio::fs::create_dir_all(&public_dir)
            .await
            .wrap_err(format!(
                "{}: {}",
                "Failed to create public directory".bold(),
                public_dir.display()
            ))?;
    }

    debug!("Build directory prepared successfully");
    Ok(())
}

/// Collects the names of all XML templates registered in the Tera instance.
fn collect_xml_templates(tera: &Tera) -> Vec<String> {
    tera.get_template_names()
        .filter(|name| name.ends_with(".xml"))
        .map(|name| name.to_string())
        .collect()
}

/// Renders all XML feed templates and writes them to the public directory.
///
/// Each XML template found in the Tera instance is rendered with site and post
/// context, then written to the corresponding path under `public_dir`.
/// Subdirectories are created as needed. RSS validation is attempted per file;
/// failures emit a warning rather than aborting, so non-RSS formats (Atom,
/// sitemaps, etc.) are also supported.
#[instrument(level = "debug", skip(tera, shared_context, public_dir))]
async fn generate_xml_feeds(
    tera: &Tera,
    shared_context: &Context,
    public_dir: &Path,
) -> Result<usize> {
    let xml_templates = collect_xml_templates(tera);
    let count = xml_templates.len();
    if count == 0 {
        return Ok(0);
    }

    let mut context = shared_context.clone();
    context.insert("now", &chrono::Utc::now());

    for template_name in &xml_templates {
        let rendered = tera
            .render(template_name, &context)
            .map_err(|e| eyre!("{}: {}", "Failed to render XML template".bold(), e))?;

        if template_name.contains("rss") && template_name.ends_with(".xml") {
            if let Err(e) = Channel::read_from(rendered.as_bytes()) {
                warn!(
                    template = %template_name,
                    "'{}' does not validate as RSS ({}); written as-is",
                    template_name,
                    e
                );
            }
        }

        let output_path = public_dir.join(template_name);
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await.wrap_err(format!(
                "Failed to create output directory for '{}'",
                template_name
            ))?;
        }
        tokio::fs::write(&output_path, &rendered)
            .await
            .wrap_err(format!("Failed to write '{}'", output_path.display()))?;
    }

    Ok(count)
}

/// Generates the final public build from intermediate build artifacts
///
/// Processes HTML files through templates and handles minification.
/// Performs concurrent processing of build artifacts with validation.
///
/// # Arguments
/// * `tera` - Template engine instance
/// * `paths` - Site directory paths
/// * `site_config` - Site configuration
/// * `minify` - Enable minification of output
#[instrument(level = "debug", skip(tera, paths, site_config, shared_context, cache))]
async fn build_contents(
    tera: &Tera,
    paths: &SitePaths,
    posts: &[toml::Value],
    site_config: &config::SiteConfig,
    shared_context: &Context,
    cache: &Arc<tokio::sync::Mutex<BuildCache>>,
    minify: bool,
) -> Result<(usize, Arc<Mutex<BuildTimings>>)> {
    let entries = WalkDir::new(&paths.content)
        .into_iter()
        .filter_map(|e| match e {
            Ok(e) => Some(e),
            Err(e) => {
                warn!("WalkDir error: {}", e);
                None
            }
        })
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "norg"));

    // Shared state for concurrent processing
    let validation_errors = Arc::new(Mutex::new(Vec::new()));
    let timings = Arc::new(Mutex::new(BuildTimings::new()));

    // Parallel processing — buffer rendered content in memory
    let entries: Vec<_> = entries.collect();
    let buffered = Arc::new(Mutex::new(Vec::with_capacity(entries.len())));
    futures_util::stream::iter(entries)
        .for_each_concurrent(num_cpus::get(), |entry| {
            let validation_errors = Arc::clone(&validation_errors);
            let cache = Arc::clone(cache);
            let timings = Arc::clone(&timings);
            let buffered = Arc::clone(&buffered);

            async move {
                let path = entry.path();
                match build_content_entry(
                    path,
                    tera,
                    paths,
                    site_config,
                    minify,
                    validation_errors,
                    shared_context,
                    &cache,
                    &timings,
                )
                .await
                {
                    Ok(Some((public_path, content))) => {
                        buffered.lock().await.push((public_path, content));
                    }
                    Ok(None) => {} // draft or missing
                    Err(e) => error!("{:?}", e),
                }
            }
        })
        .await;
    let buffered_writes = Arc::try_unwrap(buffered)
        .expect("buffered Arc should have single owner")
        .into_inner();

    // Sequential writes — single I/O path, no contention
    let write_start = Instant::now();
    let mut built_count = 0usize;
    for (public_path, content) in buffered_writes {
        if write_public_file(&public_path, &content).await? {
            built_count += 1;
        }
    }
    {
        let mut t = timings.lock().await;
        t.page_write_ms = write_start.elapsed().as_millis();
    }

    let errors = validation_errors.lock().await;
    if !errors.is_empty() {
        bail!(errors.join("\n"));
    }

    let count = built_count;
    Ok((count, timings))
}

/// Processes a single build entry (HTML file with metadata)
///
/// Handles template rendering, metadata validation, and output path determination.
/// Skips draft content and applies minification when enabled.
/// Returns `(public_path, rendered_content)` for deferred writing.
#[allow(clippy::too_many_arguments)]
#[instrument(
    level = "debug",
    skip(tera, paths, site_config, validation_errors, shared_context, cache)
)]
async fn build_content_entry(
    path: &Path,
    tera: &Tera,
    paths: &SitePaths,
    site_config: &config::SiteConfig,
    minify: bool,
    validation_errors: Arc<Mutex<Vec<String>>>,
    shared_context: &Context,
    cache: &Arc<tokio::sync::Mutex<BuildCache>>,
    timings: &Arc<Mutex<BuildTimings>>,
) -> Result<Option<(PathBuf, String)>> {
    let rel_path = path
        .strip_prefix(&paths.content)
        .wrap_err("Failed to strip prefix")?;

    // Read file
    let t = Instant::now();
    let Ok(content) = tokio::fs::read_to_string(path).await else {
        error!(
            "{} {}",
            "Norg file not found for".bold(),
            rel_path.display()
        );
        return Ok(None);
    };
    let file_ms = t.elapsed().as_millis();

    // Lightweight metadata extraction
    let t = Instant::now();
    let metadata = shared::extract_metadata_from_content(&content, rel_path, &site_config.root_url);
    let meta_ms = t.elapsed().as_millis();

    // Schema validation
    let t = Instant::now();
    if let Some(schema) = &site_config.content_schema {
        if !rel_path.starts_with(&site_config.categories_dir) {
            let errors =
                shared::validate_content_metadata(&paths.content, path, &metadata, schema, false)
                    .await?;
            if !errors.is_empty() {
                validation_errors.lock().await.push(errors);
            }
        }
    }
    let schema_ms = t.elapsed().as_millis();

    // Draft check
    let t = Instant::now();
    if toml::Value::as_bool(metadata.get("draft").unwrap_or(&toml::Value::from(false)))
        .expect("draft metadata field should be a boolean")
    {
        let draft_ms = t.elapsed().as_millis();
        {
            let mut t = timings.lock().await;
            t.page_file_ms += file_ms;
            t.page_meta_ms += meta_ms;
            t.page_schema_ms += schema_ms;
            t.page_draft_ms += draft_ms;
        }
        return Ok(None);
    }
    let draft_ms = t.elapsed().as_millis();

    // Cache get
    let t = Instant::now();
    let cache_key = rel_path.with_extension("");
    let cached = {
        let cache_guard = cache.lock().await;
        cache_guard.get(&cache_key, &content)
    };
    let cache_get_ms = t.elapsed().as_millis();

    // Load (parse_tree + HTML on miss, deserialization on hit)
    let t = Instant::now();
    let metadata = if let Some(cached) = cached {
        match serde_json::from_value(cached.clone()) {
            Ok(md) => md,
            Err(_) => shared::load_metadata_from_content(&content, rel_path, &site_config.root_url),
        }
    } else {
        let md = shared::load_metadata_from_content(&content, rel_path, &site_config.root_url);
        if let Ok(json_val) = serde_json::to_value(&md) {
            let mut cache_guard = cache.lock().await;
            cache_guard.insert(&cache_key, &content, json_val);
        }
        md
    };
    let load_ms = t.elapsed().as_millis();

    // Determine output path
    let public_path = determine_public_path(&paths.public, rel_path)?;

    // Template render
    let t = Instant::now();
    let mut rendered = shared::render_norg_page(tera, &metadata, shared_context).await?;
    let render_ms = t.elapsed().as_millis();

    // Href rewrite
    let t = Instant::now();
    let href_re = href_root_re();
    rendered = href_re
        .replace_all(&rendered, format!("href=\"{}/", site_config.root_url))
        .into_owned();
    let href_ms = t.elapsed().as_millis();

    // Minify
    let t = Instant::now();
    let rendered = if minify && !rendered.is_empty() {
        minify_html_content(rendered)?
    } else {
        rendered
    };
    let minify_ms = t.elapsed().as_millis();

    // Accumulate timings
    {
        let mut t = timings.lock().await;
        t.page_file_ms += file_ms;
        t.page_meta_ms += meta_ms;
        t.page_schema_ms += schema_ms;
        t.page_draft_ms += draft_ms;
        t.page_cache_get_ms += cache_get_ms;
        t.page_load_ms += load_ms;
        t.page_render_ms += render_ms;
        t.page_href_ms += href_ms;
        t.page_minify_ms += minify_ms;
    }

    Ok(Some((public_path, rendered)))
}

/// Generates category listing pages
pub async fn build_category_pages(
    tera: &Tera,
    public_dir: &Path,
    posts: &[toml::Value],
    config: &config::SiteConfig,
    collections: &shared::PrecomputedCollections,
) -> Result<usize> {
    let categories = shared::collect_all_posts_categories(posts).await;
    let categories_dir = public_dir.join(&config.categories_dir);

    // Generate category pages only if the site has posts
    if posts.is_empty() {
        return Ok(0);
    }

    let content = shared::render_category_index(tera, posts, config, collections).await?;

    tokio::fs::create_dir_all(&categories_dir).await?;
    tokio::fs::write(categories_dir.join("index.html"), content).await?;
    let mut page_count = 1usize;

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

        let content = shared::render_category_page(tera, &category, &cat_posts, config).await?;

        let cat_dir = categories_dir.join(&category);
        tokio::fs::create_dir_all(&cat_dir).await?;

        tokio::fs::write(cat_dir.join("index.html"), content).await?;
        page_count += 1;
    }

    Ok(page_count)
}

/// Determines the final public path for an HTML file based on its name and location.
///
/// This function creates SEO-friendly URLs by nesting non-index files in directories.
/// For example, a file named `about.html` will be placed in `about/index.html`.
/// Files named `index.html` are placed directly in their parent directory.
///
/// # Arguments
/// * `public_dir` - The root public directory where the final build is stored.
/// * `rel_path` - The relative path of the file within the build directory.
///
/// # Returns
/// * `PathBuf` - The final public path for the HTML file.
#[instrument]
fn determine_public_path(public_dir: &Path, rel_path: &Path) -> Result<PathBuf> {
    let stem = rel_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| eyre!("Invalid file stem for path: {}", rel_path.display()))?;
    if stem == "index" {
        Ok(public_dir.join(rel_path).with_extension("html"))
    } else {
        Ok(public_dir
            .join(rel_path.parent().unwrap_or(Path::new(""))) // Handle root path parent gracefully
            .join(stem)
            .join("index.html"))
    }
}

/// Pre-creates all output directories needed by content entries.
///
/// Walks the content directory, computes each file's public output path via
/// `determine_public_path`, and creates all unique parent directories once.
/// This avoids redundant `create_dir_all` syscalls inside the parallel build loop.
fn precreate_output_dirs(paths: &SitePaths) -> Result<()> {
    let entries = WalkDir::new(&paths.content)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "norg"));

    let mut dirs = std::collections::HashSet::new();
    for entry in entries {
        let rel_path = entry.path().strip_prefix(&paths.content)?;
        if let Ok(public_path) = determine_public_path(&paths.public, rel_path) {
            if let Some(parent) = public_path.parent() {
                dirs.insert(parent.to_path_buf());
            }
        }
    }
    for dir in &dirs {
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}

/// Writes rendered content to the public directory, skipping if content is unchanged.
///
/// If the file already exists with identical content, the write is skipped entirely.
/// Parent directories must be created beforehand (see `precreate_output_dirs`).
///
/// # Arguments
/// * `public_path` - The path where the file should be written in the public directory.
/// * `rendered` - The content to write to the file.
///
/// # Returns
/// * `Result<bool>` - `Ok(true)` if file was written, `Ok(false)` if skipped (unchanged).
#[instrument(skip(rendered))]
async fn write_public_file(public_path: &Path, rendered: &str) -> Result<bool> {
    // Skip write if file exists with identical content
    if let Ok(existing) = tokio::fs::read(public_path).await {
        if existing == rendered.as_bytes() {
            return Ok(false);
        }
    }
    tokio::fs::write(public_path, rendered)
        .await
        .wrap_err(format!(
            "{}: {}",
            "Failed to write to public path".bold(),
            public_path.display()
        ))?;
    Ok(true)
}

/// Determines whether an asset should be minified based on its name and extension.
///
/// This function checks if the asset is a JavaScript or CSS file and does not already
/// have "min" in its name. Assets with "min" in their name or non-JS/CSS extensions
/// are skipped for minification.
///
/// # Arguments
/// * `src` - The path to the asset file.
///
/// # Returns
/// * `bool` - `true` if the asset should be minified, `false` otherwise.
#[instrument]
fn should_minify_asset(src: &Path) -> bool {
    let file_stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let file_ext = src.extension().and_then(|s| s.to_str()).unwrap_or_default();
    !file_stem.ends_with(".min") && (file_ext == "js" || file_ext == "css")
}

/// Minifies HTML content using optimized settings for production builds.
///
/// This function applies minification to HTML content, including optional minification
/// of embedded JavaScript and CSS. It uses the `minify-html` crate for efficient
/// minification.
///
/// # Arguments
/// * `rendered` - The HTML content to minify.
///
/// # Returns
/// * `Result<String>` - The minified HTML content if successful, otherwise an error.
#[instrument]
fn minify_html_content(rendered: String) -> Result<String> {
    let minify_config = minify_html::Cfg {
        minify_js: true,
        minify_css: true,
        ..minify_html::Cfg::default()
    };
    String::from_utf8(minify_html::minify(rendered.as_bytes(), &minify_config))
        .map_err(|e| eyre!("{}: {}", "HTML minification failed".bold(), e))
}

/// Minifies a JavaScript asset using the `minify-js` crate.
///
/// This function reads a JavaScript file, minifies its content, and writes the
/// minified output to the destination path. It is used for production builds
/// to reduce file size and improve performance.
///
/// # Arguments
/// * `src_path` - The path to the source JavaScript file.
/// * `dest_path` - The path where the minified JavaScript should be saved.
///
/// # Returns
/// * `Result<()>` - `Ok(())` if minification and writing succeed, otherwise an error.
#[instrument(skip(src_path, dest_path))]
async fn minify_js_asset(src_path: &Path, dest_path: &Path) -> Result<()> {
    let content = tokio::fs::read(src_path).await?;
    let mut minified = Vec::new();
    let session = minify_js::Session::new();
    minify_js::minify(
        &session,
        minify_js::TopLevelMode::Global,
        &content,
        &mut minified,
    )
    .map_err(|e| {
        eyre!(
            "{}: {}",
            format!("JS minification failed for {}", src_path.display()).bold(),
            e
        )
    })?;
    tokio::fs::write(dest_path, minified)
        .await
        .wrap_err_with(|| format!("Failed to write minified JS to {}", dest_path.display()))?;
    Ok(())
}

/// Minifies a CSS asset using the `css-minify` crate.
///
/// This function reads a CSS file, applies level 2 optimizations, and writes the
/// minified output to the destination path. It is used for production builds
/// to reduce file size and improve performance.
///
/// # Arguments
/// * `src_path` - The path to the source CSS file.
/// * `dest_path` - The path where the minified CSS should be saved.
///
/// # Returns
/// * `Result<()>` - `Ok(())` if minification and writing succeed, otherwise an error.
#[instrument(skip(src_path, dest_path))]
async fn minify_css_asset(src_path: &Path, dest_path: &Path) -> Result<()> {
    let content = tokio::fs::read_to_string(src_path).await?;

    let mut stylesheet =
        StyleSheet::parse(&content, ParserOptions::default()).map_err(|e| eyre!("{}", e))?;
    stylesheet.minify(MinifyOptions::default())?;
    let minified = stylesheet.to_css(PrinterOptions {
        minify: true,
        ..Default::default()
    })?;

    tokio::fs::write(dest_path, minified.code)
        .await
        .wrap_err_with(|| {
            format!("Failed to write minified CSS to {}", dest_path.display()).bold()
        })?;
    Ok(())
}

/// Copies a binary asset without modification.
///
/// This function reads a binary file (e.g., images, fonts) and writes it to the
/// destination path. It is used for assets that do not require minification.
///
/// # Arguments
/// * `src_path` - The path to the source binary file.
/// * `dest_path` - The path where the binary file should be saved.
///
/// # Returns
/// * `Result<()>` - `Ok(())` if the file is copied successfully, otherwise an error.
#[instrument(skip(src_path, dest_path))]
async fn copy_binary_asset(src_path: &Path, dest_path: &Path) -> Result<()> {
    let content = tokio::fs::read(src_path).await?;
    tokio::fs::write(dest_path, content)
        .await
        .wrap_err_with(|| {
            format!(
                "Failed to copy asset from {} to {}",
                src_path.display(),
                dest_path.display()
            )
        })?;
    Ok(())
}

/// Copies an asset file with optional minification based on its type.
///
/// This function handles the copying of assets, applying minification to JavaScript
/// and CSS files when enabled. Other file types are copied without modification.
///
/// # Arguments
/// * `src_path` - The path to the source asset file.
/// * `dest_path` - The path where the asset should be saved.
/// * `minify` - Whether to minify supported assets during the copy process.
///
/// # Returns
/// * `Result<()>` - `Ok(())` if the file is processed successfully, otherwise an error.
#[instrument(skip(src_path, dest_path, minify))]
async fn copy_asset_file(src_path: &Path, dest_path: &Path, minify: bool) -> Result<()> {
    if minify && should_minify_asset(src_path) {
        let file_ext = src_path.extension().unwrap().to_str().unwrap();

        match file_ext {
            "js" => minify_js_asset(src_path, dest_path).await?,
            "css" => minify_css_asset(src_path, dest_path).await?,
            _ => copy_binary_asset(src_path, dest_path).await?,
        }
    } else {
        // Copy file as binary, this lets us write images and some other formats as well instead of only text files
        copy_binary_asset(src_path, dest_path).await?;
    }
    Ok(())
}

/// Recursively copies assets from a source directory to the public assets directory.
///
/// This function processes all files and subdirectories in the source directory,
/// copying them to the corresponding location in the public assets directory.
/// It handles both files and directories, ensuring the directory structure is preserved.
///
/// # Arguments
/// * `assets_dir` - The source directory containing the assets to copy.
/// * `target_dir` - Target assets directory to paste in.
/// * `minify` - Whether to minify supported assets (e.g., JS and CSS) during the copy process.
///
/// # Returns
/// * `Result<()>` - `Ok(())` if all assets are copied successfully, otherwise an error.
#[instrument(skip(assets_dir, target_dir, minify))]
async fn copy_assets(assets_dir: &Path, target_dir: &Path, minify: bool) -> Result<usize> {
    let mut file_count = 0usize;
    for entry in WalkDir::new(assets_dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| match e {
            Ok(e) => Some(e),
            Err(e) => {
                warn!("WalkDir error: {}", e);
                None
            }
        })
    {
        let Some(rel_path) = entry.path().strip_prefix(assets_dir).ok() else {
            warn!("Skipping asset outside assets directory: {}", entry.path().display());
            continue;
        };
        let target_path = target_dir.join(rel_path);
        if entry.path().is_dir() {
            if !target_path.exists() {
                tokio::fs::create_dir_all(target_path).await?;
            }
        } else {
            copy_asset_file(entry.path(), &target_path, minify).await?;
            file_count += 1;
        }
    }

    Ok(file_count)
}

#[derive(Debug)]
struct BuildTimings {
    config_ms: u128,
    tera_ms: u128,
    prepare_dir_ms: u128,
    collect_posts_ms: u128,
    collections_ms: u128,
    shared_ctx_ms: u128,
    cache_open_ms: u128,
    content_ms: u128,
    categories_ms: u128,
    feeds_ms: u128,
    assets_ms: u128,
    cache_save_ms: u128,
    // Per-page sub-timing (sums across all pages)
    page_file_ms: u128,
    page_meta_ms: u128,
    page_schema_ms: u128,
    page_draft_ms: u128,
    page_cache_get_ms: u128,
    page_load_ms: u128,
    page_render_ms: u128,
    page_href_ms: u128,
    page_minify_ms: u128,
    page_write_ms: u128,
    page_count: usize,
}

impl BuildTimings {
    fn new() -> Self {
        Self {
            config_ms: 0,
            tera_ms: 0,
            prepare_dir_ms: 0,
            collect_posts_ms: 0,
            collections_ms: 0,
            shared_ctx_ms: 0,
            cache_open_ms: 0,
            content_ms: 0,
            categories_ms: 0,
            feeds_ms: 0,
            assets_ms: 0,
            cache_save_ms: 0,
            page_file_ms: 0,
            page_meta_ms: 0,
            page_schema_ms: 0,
            page_draft_ms: 0,
            page_cache_get_ms: 0,
            page_load_ms: 0,
            page_render_ms: 0,
            page_href_ms: 0,
            page_minify_ms: 0,
            page_write_ms: 0,
            page_count: 0,
        }
    }

    fn print_summary(&self, total_ms: u128) {
        let overhead = total_ms
            .saturating_sub(self.config_ms)
            .saturating_sub(self.tera_ms)
            .saturating_sub(self.prepare_dir_ms)
            .saturating_sub(self.collect_posts_ms)
            .saturating_sub(self.collections_ms)
            .saturating_sub(self.shared_ctx_ms)
            .saturating_sub(self.cache_open_ms)
            .saturating_sub(self.content_ms)
            .saturating_sub(self.categories_ms)
            .saturating_sub(self.feeds_ms)
            .saturating_sub(self.assets_ms)
            .saturating_sub(self.cache_save_ms);

        println!();
        println!("{}", "=== Build Timing Breakdown ===".bold());
        println!("  {:<30} {:>6}ms  ({:>4.1}%)", "Config load+validate", self.config_ms, pct(self.config_ms, total_ms));
        println!("  {:<30} {:>6}ms  ({:>4.1}%)", "Tera init", self.tera_ms, pct(self.tera_ms, total_ms));
        println!("  {:<30} {:>6}ms  ({:>4.1}%)", "Prepare build dir", self.prepare_dir_ms, pct(self.prepare_dir_ms, total_ms));
        println!("  {:<30} {:>6}ms  ({:>4.1}%)", "Collect post metadata", self.collect_posts_ms, pct(self.collect_posts_ms, total_ms));
        println!("  {:<30} {:>6}ms  ({:>4.1}%)", "Collection subsets", self.collections_ms, pct(self.collections_ms, total_ms));
        println!("  {:<30} {:>6}ms  ({:>4.1}%)", "Shared context", self.shared_ctx_ms, pct(self.shared_ctx_ms, total_ms));
        println!("  {:<30} {:>6}ms  ({:>4.1}%)", "Cache open", self.cache_open_ms, pct(self.cache_open_ms, total_ms));
        println!("  {:<30} {:>6}ms  ({:>4.1}%)", "Content build (all pages)", self.content_ms, pct(self.content_ms, total_ms));
        println!("  {:<30} {:>6}ms  ({:>4.1}%)", "Category pages", self.categories_ms, pct(self.categories_ms, total_ms));
        println!("  {:<30} {:>6}ms  ({:>4.1}%)", "XML feeds", self.feeds_ms, pct(self.feeds_ms, total_ms));
        println!("  {:<30} {:>6}ms  ({:>4.1}%)", "Asset copy", self.assets_ms, pct(self.assets_ms, total_ms));
        println!("  {:<30} {:>6}ms  ({:>4.1}%)", "Cache save", self.cache_save_ms, pct(self.cache_save_ms, total_ms));
        println!("  {:<30} {:>6}ms  ({:>4.1}%)", "Overhead/other", overhead, pct(overhead, total_ms));
        println!("  {}", "─".repeat(50));
        println!("  {:<30} {:>6}ms", "TOTAL", total_ms);

        if self.page_count > 0 {
            println!();
            println!("{}", "=== Per-Page Sub-Timing (sums across all pages) ===".bold());
            println!("  {:<30} {:>6}ms  (avg {:>4.1}ms)", "File read (I/O)", self.page_file_ms, avg(self.page_file_ms, self.page_count));
            println!("  {:<30} {:>6}ms  (avg {:>4.1}ms)", "Metadata extract", self.page_meta_ms, avg(self.page_meta_ms, self.page_count));
            println!("  {:<30} {:>6}ms  (avg {:>4.1}ms)", "Schema validation", self.page_schema_ms, avg(self.page_schema_ms, self.page_count));
            println!("  {:<30} {:>6}ms  (avg {:>4.1}ms)", "Draft check", self.page_draft_ms, avg(self.page_draft_ms, self.page_count));
            println!("  {:<30} {:>6}ms  (avg {:>4.1}ms)", "Cache lock get", self.page_cache_get_ms, avg(self.page_cache_get_ms, self.page_count));
            println!("  {:<30} {:>6}ms  (avg {:>4.1}ms)", "Parse+HTML convert", self.page_load_ms, avg(self.page_load_ms, self.page_count));
            println!("  {:<30} {:>6}ms  (avg {:>4.1}ms)", "Template render", self.page_render_ms, avg(self.page_render_ms, self.page_count));
            println!("  {:<30} {:>6}ms  (avg {:>4.1}ms)", "Href rewrite", self.page_href_ms, avg(self.page_href_ms, self.page_count));
            println!("  {:<30} {:>6}ms  (avg {:>4.1}ms)", "HTML minify", self.page_minify_ms, avg(self.page_minify_ms, self.page_count));
            println!("  {:<30} {:>6}ms  (avg {:>4.1}ms)", "File write", self.page_write_ms, avg(self.page_write_ms, self.page_count));
            let page_sum = self.page_file_ms + self.page_meta_ms + self.page_schema_ms
                + self.page_draft_ms + self.page_cache_get_ms + self.page_load_ms
                + self.page_render_ms + self.page_href_ms + self.page_minify_ms + self.page_write_ms;
            println!("  {}", "─".repeat(50));
            println!("  {:<30} {:>6}ms  (sum of above)", "Page sub-timing sum", page_sum);
            println!("  {:<30} {:>6}ms  (content_ms - page sub-sum)", "Lock/scheduling gap",
                self.content_ms.saturating_sub(page_sum));
        }
    }
}

fn pct(ms: u128, total: u128) -> f64 {
    if total == 0 { 0.0 } else { ms as f64 / total as f64 * 100.0 }
}
fn avg(ms: u128, n: usize) -> f64 {
    if n == 0 { 0.0 } else { ms as f64 / n as f64 }
}

/// Main build entry point
///
/// Orchestrates the complete build process:
/// 1. Load configuration
/// 2. Prepare directories
/// 3. Convert content
/// 4. Render templates
/// 5. Copy assets
///
/// # Arguments
/// * `minify` - Enable minification of HTML/CSS/JS outputs
#[instrument(skip(minify))]
pub async fn build(minify: bool) -> Result<()> {
    let Some(root) = fs::find_config_file().await? else {
        bail!(
            "{}: not in a Norgolith site directory",
            "Could not build the site".bold()
        );
    };

    println!(
        "{} Building site{}...",
        "→".cyan().bold(),
        if minify {
            " (minified)".dimmed()
        } else {
            ColoredString::from("")
        }
    );
    let build_start = Instant::now();
    let mut timings = BuildTimings::new();

    // Load site configuration
    let t = Instant::now();
    let config_content = tokio::fs::read_to_string(&root)
        .await
        .wrap_err("Failed to read config file")?;
    let site_config: config::SiteConfig =
        toml::from_str(&config_content).wrap_err("Failed to parse site configuration")?;
    let validation_errors = site_config.validate();
    if !validation_errors.is_empty() {
        for error in &validation_errors {
            eprintln!("{}", error);
        }
        bail!("Site configuration has validation errors");
    }
    debug!(?site_config, "Loaded site configuration");
    timings.config_ms = t.elapsed().as_millis();

    let root_dir = root.parent().unwrap().to_path_buf();
    let paths = SitePaths::new(root_dir.clone());

    // Initialize Tera
    let t = Instant::now();
    debug!("Initializing template engine");
    let tera = shared::init_tera(paths.templates.to_str().unwrap(), &paths.theme_templates).await?;
    timings.tera_ms = t.elapsed().as_millis();

    // Prepare build directory
    let t = Instant::now();
    prepare_build_directory(&paths.public).await?;
    timings.prepare_dir_ms = t.elapsed().as_millis();

    // Pre-create output directories for all content entries
    let t = Instant::now();
    precreate_output_dirs(&paths)?;
    timings.prepare_dir_ms += t.elapsed().as_millis();

    // Collect post metadata
    let t = Instant::now();
    let posts: Vec<_> = shared::collect_all_posts_metadata(
        &paths.content,
        &site_config.root_url,
        &site_config.collections,
    )
    .await?
    .into_iter()
    .filter(|post| {
        !post
            .get("draft")
            .map(|v| {
                v.as_bool()
                    .expect("draft metadata field should be a boolean")
            })
            .unwrap_or(false)
    })
    .collect();
    timings.collect_posts_ms = t.elapsed().as_millis();

    // Pre-compute collection subsets
    let t = Instant::now();
    let collections = shared::precompute_collection_subsets(&posts, &site_config);
    timings.collections_ms = t.elapsed().as_millis();

    // Build shared context
    let t = Instant::now();
    let shared_context = shared::build_shared_context(&posts, &site_config, &collections);
    timings.shared_ctx_ms = t.elapsed().as_millis();

    // Open cache
    let t = Instant::now();
    let cache = BuildCache::open(&root_dir)?;
    let cache = Arc::new(tokio::sync::Mutex::new(cache));
    timings.cache_open_ms = t.elapsed().as_millis();

    println!();

    // Build content
    let t = Instant::now();
    let (page_count, page_timings) = build_contents(&tera, &paths, &posts, &site_config, &shared_context, &cache, minify).await?;
    timings.content_ms = t.elapsed().as_millis();
    timings.page_count = page_count;
    // Copy per-page sub-timings from the concurrent build
    {
        let pt = page_timings.lock().await;
        timings.page_file_ms = pt.page_file_ms;
        timings.page_meta_ms = pt.page_meta_ms;
        timings.page_schema_ms = pt.page_schema_ms;
        timings.page_draft_ms = pt.page_draft_ms;
        timings.page_cache_get_ms = pt.page_cache_get_ms;
        timings.page_load_ms = pt.page_load_ms;
        timings.page_render_ms = pt.page_render_ms;
        timings.page_href_ms = pt.page_href_ms;
        timings.page_minify_ms = pt.page_minify_ms;
        timings.page_write_ms = pt.page_write_ms;
    }
    println!(
        "  {} {}  {:<12}  {}",
        "•".green(),
        format!("{:<12}", "Content").bold(),
        format!("{} pages", page_count),
        shared::get_elapsed_time(t).dimmed()
    );

    // Category pages
    let t = Instant::now();
    let cat_count = build_category_pages(&tera, &paths.public, &posts, &site_config, &collections).await?;
    timings.categories_ms = t.elapsed().as_millis();
    if cat_count > 0 {
        println!(
            "  {} {}  {:<12}  {}",
            "•".green(),
            format!("{:<12}", "Categories").bold(),
            format!("{} pages", cat_count),
            shared::get_elapsed_time(t).dimmed()
        );
    }

    // XML feeds
    let t = Instant::now();
    let feed_count = generate_xml_feeds(&tera, &shared_context, &paths.public).await?;
    timings.feeds_ms = t.elapsed().as_millis();
    if feed_count > 0 {
        println!(
            "  {} {}  {:<12}  {}",
            "•".green(),
            format!("{:<12}", "Feeds").bold(),
            format!("{} files", feed_count),
            shared::get_elapsed_time(t).dimmed()
        );
    }

    // Assets
    let t = Instant::now();
    let public_assets_dir = paths.public.join("assets");
    let mut asset_count = 0usize;
    if paths.theme_assets.exists() {
        asset_count += copy_assets(&paths.theme_assets, &public_assets_dir, minify).await?;
    }
    asset_count += copy_assets(&paths.assets, &public_assets_dir, minify).await?;
    timings.assets_ms = t.elapsed().as_millis();
    println!(
        "  {} {}  {:<12}  {}",
        "•".green(),
        format!("{:<12}", "Assets").bold(),
        format!("{} files", asset_count),
        shared::get_elapsed_time(t).dimmed()
    );

    println!();
    let total_ms = build_start.elapsed().as_millis();
    println!(
        "{} Built in {}",
        "✓".green().bold(),
        shared::get_elapsed_time(build_start)
    );

    // Save cache
    let t = Instant::now();
    let cache_guard = cache.lock().await;
    if let Err(e) = cache_guard.save(&root_dir) {
        warn!("Failed to save build cache: {}", e);
    }
    drop(cache_guard);
    timings.cache_save_ms = t.elapsed().as_millis();

    timings.print_summary(total_ms);

    Ok(())
}
