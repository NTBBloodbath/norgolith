use std::{
    path::{Path, PathBuf},
    sync::{OnceLock},
    time::Instant,
};

use colored::{ColoredString, Colorize};
use eyre::{bail, eyre, Result, WrapErr};
use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
use rss::Channel;
use tera::{Context, Tera};
use tracing::{debug, error, instrument, warn};
use walkdir::WalkDir;

fn href_root_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#"href="(/|&#x2F;)"#).expect("valid regex"))
}

use crate::{cache::BuildCache, config, fs, plugin, shared};

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
fn prepare_build_directory(public_dir: &Path) -> Result<()> {
    debug!(path = %public_dir.display(), "Preparing build directory");
    if public_dir.exists() {
        for entry in std::fs::read_dir(public_dir).wrap_err(format!(
            "{}: {}",
            "Failed to read existing public directory".bold(),
            public_dir.display()
        ))? {
            let entry = entry.wrap_err(format!(
                "{}: {}",
                "Failed to iterate existing public directory".bold(),
                public_dir.display()
            ))?;
            let path = entry.path();
            let file_name = path.file_name().and_then(|name| name.to_str());

            // Keep git metadata so users can treat ./public as a separate repository.
            if file_name == Some(".git") {
                debug!(path = %path.display(), "Keeping git metadata directory");
                continue;
            }

            let metadata = entry.metadata().wrap_err(format!(
                "{}: {}",
                "Failed to stat existing public entry".bold(),
                path.display()
            ))?;

            if metadata.is_dir() {
                std::fs::remove_dir_all(&path).wrap_err(format!(
                    "{}: {}",
                    "Failed to remove existing public directory entry".bold(),
                    path.display()
                ))?;
            } else {
                std::fs::remove_file(&path).wrap_err(format!(
                    "{}: {}",
                    "Failed to remove existing public file entry".bold(),
                    path.display()
                ))?;
            }
        }
    } else {
        debug!(path = %public_dir.display(), "Creating public directory");
        std::fs::create_dir_all(public_dir)
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
fn generate_xml_feeds(
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
            std::fs::create_dir_all(parent).wrap_err(format!(
                "Failed to create output directory for '{}'",
                template_name
            ))?;
        }
        std::fs::write(&output_path, &rendered)
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
#[allow(clippy::too_many_arguments)]
#[instrument(level = "debug", skip(tera, paths, site_config, shared_context, cache, plugin_mgr))]
fn build_contents(
    tera: &Tera,
    paths: &SitePaths,
    posts: &[toml::Value],
    site_config: &config::SiteConfig,
    shared_context: &Context,
    cache: &mut BuildCache,
    minify: bool,
    plugin_mgr: &plugin::PluginManager,
) -> Result<(usize, BuildTimings)> {
    use rayon::prelude::*;

    let entries: Vec<_> = WalkDir::new(&paths.content)
        .into_iter()
        .filter_map(|e| match e {
            Ok(e) => Some(e),
            Err(e) => {
                warn!("WalkDir error: {}", e);
                None
            }
        })
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "norg"))
        .collect();

    // Parallel processing with rayon, buffer rendered content in memory
    let results: Vec<BuildResult> = entries
        .par_iter()
        .map(|entry| {
            let path = entry.path();
            build_content_entry(
                path,
                tera,
                paths,
                site_config,
                minify,
                shared_context,
                cache,
                plugin_mgr,
            )
        })
        .collect();

    // Collect results and handle errors
    let mut buffered_writes = Vec::new();
    for result in results {
        match result {
            Ok(Some((public_path, content, cache_entry))) => {
                buffered_writes.push((public_path, content));
                if let Some((key, content_str, metadata)) = cache_entry {
                    cache.insert(&key, &content_str, metadata);
                }
            }
            Ok(None) => {} // draft or missing
            Err(e) => error!("{:?}", e),
        }
    }

    // Sequential writes, single I/O path, no contention
    let write_start = Instant::now();
    let mut built_count = 0usize;
    for (public_path, content) in &buffered_writes {
        if write_public_file(public_path, content)? {
            built_count += 1;
        }
    }
    let write_ms = write_start.elapsed().as_millis();

    let mut timings = BuildTimings::new();
    timings.page_write_ms = write_ms;
    timings.page_count = built_count;

    Ok((built_count, timings))
}

/// (cache_key, content, metadata) for cache insertion
type CacheInsert = (PathBuf, String, serde_json::Value);
/// Result of building a single content entry
type BuildResult = Result<Option<(PathBuf, String, Option<CacheInsert>)>>;

/// Processes a single build entry (HTML file with metadata)
///
/// Handles template rendering, metadata validation, and output path determination.
/// Skips draft content and applies minification when enabled.
/// Returns `(public_path, rendered_content)` for deferred writing.
#[allow(clippy::too_many_arguments)]
#[instrument(
    level = "debug",
    skip(tera, paths, site_config, shared_context, cache, plugin_mgr)
)]
fn build_content_entry(
    path: &Path,
    tera: &Tera,
    paths: &SitePaths,
    site_config: &config::SiteConfig,
    minify: bool,
    shared_context: &Context,
    cache: &BuildCache,
    plugin_mgr: &plugin::PluginManager,
) -> BuildResult {
    let rel_path = path
        .strip_prefix(&paths.content)
        .wrap_err("Failed to strip prefix")?;

    // Read file
    let Ok(content) = std::fs::read_to_string(path) else {
        error!(
            "{} {}",
            "Norg file not found for".bold(),
            rel_path.display()
        );
        return Ok(None);
    };

    // Lightweight metadata extraction
    let metadata = shared::extract_metadata_from_content(&content, rel_path, &site_config.root_url);

    // Schema validation
    if let Some(schema) = &site_config.content_schema {
        if !rel_path.starts_with(&site_config.categories_dir) {
            let errors =
                shared::validate_content_metadata(&paths.content, path, &metadata, schema, false)?;
            if !errors.is_empty() {
                return Err(eyre!("{}", errors));
            }
        }
    }

    // Draft check
    if toml::Value::as_bool(metadata.get("draft").unwrap_or(&toml::Value::from(false)))
        .expect("draft metadata field should be a boolean")
    {
        return Ok(None);
    }

    // Cache get (read-only, misses will be inserted later)
    let cache_key = rel_path.with_extension("");
    let cached = cache.get(&cache_key, &content);

    // Load (parse_tree + HTML on miss, deserialization on hit)
    let (mut metadata, cache_insert) = if let Some(cached) = cached {
        match serde_json::from_value::<toml::Value>(cached.clone()) {
            Ok(md) => (md, None),
            Err(_) => {
                let md = shared::load_metadata_from_content(&content, rel_path, &site_config.root_url);
                let cache_val = serde_json::to_value(&md).unwrap_or_default();
                (md, Some((cache_key, content.clone(), cache_val)))
            }
        }
    } else {
        let md = shared::load_metadata_from_content(&content, rel_path, &site_config.root_url);
        let cache_val = serde_json::to_value(&md).unwrap_or_default();
        (md, Some((cache_key, content.clone(), cache_val)))
    };

    // post_convert hook: modify HTML after Norg conversion, before Tera
    if plugin_mgr.has_hook(plugin::HOOK_POST_CONVERT) {
        if let Some(html) = metadata.get("raw").and_then(|v| v.as_str()) {
            let input = serde_json::json!({
                "html": html,
                "metadata": metadata,
                "rel_path": rel_path.to_string_lossy(),
            })
            .to_string();
            for p in plugin_mgr.plugins() {
                if let Some(f) = p.hooks.post_convert {
                    if let Some(new_html) = p.call_hook(f, &input) {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&new_html) {
                            if let Some(s) = val.get("html").and_then(|v| v.as_str()) {
                                if let toml::Value::Table(ref mut table) = metadata {
                                    table.insert("raw".to_string(), toml::Value::String(s.to_string()));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Determine output path
    let public_path = determine_public_path(&paths.public, rel_path)?;

    // Template render
    let mut rendered = shared::render_norg_page(tera, &metadata, shared_context)?;

    // post_render hook: modify final HTML after Tera, before write
    if plugin_mgr.has_hook(plugin::HOOK_POST_RENDER) {
        let input = serde_json::json!({
            "html": rendered,
            "metadata": metadata,
            "rel_path": rel_path.to_string_lossy(),
        })
        .to_string();
        for p in plugin_mgr.plugins() {
            if let Some(f) = p.hooks.post_render {
                if let Some(new_html) = p.call_hook(f, &input) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&new_html) {
                        if let Some(s) = val.get("html").and_then(|v| v.as_str()) {
                            rendered = s.to_string();
                        }
                    }
                }
            }
        }
    }

    // Href rewrite
    let href_re = href_root_re();
    rendered = href_re
        .replace_all(&rendered, format!("href=\"{}/", site_config.root_url))
        .into_owned();

    // Minify
    let rendered = if minify && !rendered.is_empty() {
        minify_html_content(rendered)?
    } else {
        rendered
    };

    Ok(Some((public_path, rendered, cache_insert)))
}

/// Generates category listing pages
pub fn build_category_pages(
    tera: &Tera,
    public_dir: &Path,
    posts: &[toml::Value],
    config: &config::SiteConfig,
    collections: &shared::PrecomputedCollections,
) -> Result<usize> {
    let categories = shared::collect_all_posts_categories(posts);
    let categories_dir = public_dir.join(&config.categories_dir);

    // Generate category pages only if the site has posts
    if posts.is_empty() {
        return Ok(0);
    }

    let content = shared::render_category_index(tera, posts, config, collections)?;

    std::fs::create_dir_all(&categories_dir)?;
    std::fs::write(categories_dir.join("index.html"), content)?;
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

        let content = shared::render_category_page(tera, &category, &cat_posts, config)?;

        let cat_dir = categories_dir.join(&category);
        std::fs::create_dir_all(&cat_dir)?;

        std::fs::write(cat_dir.join("index.html"), content)?;
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
fn write_public_file(public_path: &Path, rendered: &str) -> Result<bool> {
    // Skip write if file exists with identical content
    if let Ok(existing) = std::fs::read(public_path) {
        if existing == rendered.as_bytes() {
            return Ok(false);
        }
    }
    std::fs::write(public_path, rendered)
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
fn minify_html_cfg() -> &'static minify_html::Cfg {
    static CFG: OnceLock<minify_html::Cfg> = OnceLock::new();
    CFG.get_or_init(|| minify_html::Cfg {
        minify_js: true,
        minify_css: true,
        ..minify_html::Cfg::default()
    })
}

#[instrument]
fn minify_html_content(rendered: String) -> Result<String> {
    String::from_utf8(minify_html::minify(rendered.as_bytes(), minify_html_cfg()))
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
fn minify_js_asset(src_path: &Path, dest_path: &Path) -> Result<()> {
    let content = std::fs::read(src_path)?;
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
    std::fs::write(dest_path, minified)
        .wrap_err_with(|| format!("Failed to write minified JS to {}", dest_path.display()))?;
    Ok(())
}

#[instrument(skip(src_path, dest_path))]
fn minify_css_asset(src_path: &Path, dest_path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(src_path)?;

    let mut stylesheet =
        StyleSheet::parse(&content, ParserOptions::default()).map_err(|e| eyre!("{}", e))?;
    stylesheet.minify(MinifyOptions::default())?;
    let minified = stylesheet.to_css(PrinterOptions {
        minify: true,
        ..Default::default()
    })?;

    std::fs::write(dest_path, minified.code)
        .wrap_err_with(|| {
            format!("Failed to write minified CSS to {}", dest_path.display()).bold()
        })?;
    Ok(())
}

#[instrument(skip(src_path, dest_path))]
fn copy_binary_asset(src_path: &Path, dest_path: &Path) -> Result<()> {
    let content = std::fs::read(src_path)?;
    std::fs::write(dest_path, content)
        .wrap_err_with(|| {
            format!(
                "Failed to copy asset from {} to {}",
                src_path.display(),
                dest_path.display()
            )
            .bold()
        })?;
    Ok(())
}

/// Minifies a CSS asset using the `css-minify` crate.
///
/// This function reads a CSS file, applies level 2 optimizations, and writes the
/// minified output to the destination path. It is used for production builds
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
fn copy_asset_file(src_path: &Path, dest_path: &Path, minify: bool) -> Result<()> {
    if minify && should_minify_asset(src_path) {
        let file_ext = src_path.extension().unwrap().to_str().unwrap();

        match file_ext {
            "js" => minify_js_asset(src_path, dest_path)?,
            "css" => minify_css_asset(src_path, dest_path)?,
            _ => copy_binary_asset(src_path, dest_path)?,
        }
    } else {
        // Copy file as binary, this lets us write images and some other formats as well instead of only text files
        copy_binary_asset(src_path, dest_path)?;
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
fn copy_assets(assets_dir: &Path, target_dir: &Path, minify: bool) -> Result<usize> {
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
                std::fs::create_dir_all(target_path)?;
            }
        } else {
            copy_asset_file(entry.path(), &target_path, minify)?;
            file_count += 1;
        }
    }

    Ok(file_count)
}

#[derive(Debug)]
struct BuildTimings {
    config_ms: u128,
    tera_ms: u128,
    plugins_ms: u128,
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
            plugins_ms: 0,
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
pub fn build(minify: bool) -> Result<()> {
    let Some(root) = fs::find_config_file()? else {
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
    let config_content = std::fs::read_to_string(&root)
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
    let tera = shared::init_tera(paths.templates.to_str().unwrap(), &paths.theme_templates)?;
    timings.tera_ms = t.elapsed().as_millis();

    // Load plugins and apply sandbox
    let t = Instant::now();
    let plugin_mgr = plugin::PluginManager::load(&root_dir);
    let _ = plugin::sandbox::apply_landlock(&root_dir);
    timings.plugins_ms = t.elapsed().as_millis();

    if !plugin_mgr.is_empty() {
        println!(
            "  {} {}  {} plugins",
            "•".green(),
            format!("{:<12}", "Plugins").bold(),
            plugin_mgr.len()
        );
    }

    // pre_build hook
    if plugin_mgr.has_hook(plugin::HOOK_PRE_BUILD) {
        let config_json = serde_json::to_string(&site_config)
            .unwrap_or_default();
        for p in plugin_mgr.plugins() {
            if let Some(f) = p.hooks.pre_build {
                p.call_hook(f, &config_json);
            }
        }
    }

    // Prepare build directory
    let t = Instant::now();
    prepare_build_directory(&paths.public)?;
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
    )?
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
    let mut cache = BuildCache::open(&root_dir)?;
    timings.cache_open_ms = t.elapsed().as_millis();

    println!();

    // Build content
    let t = Instant::now();
    let (page_count, content_timings) = build_contents(&tera, &paths, &posts, &site_config, &shared_context, &mut cache, minify, &plugin_mgr)?;
    timings.content_ms = t.elapsed().as_millis();
    timings.page_count = page_count;
    // Copy per-page sub-timings from the concurrent build
    timings.page_write_ms = content_timings.page_write_ms;
    println!(
        "  {} {}  {:<12}  {}",
        "•".green(),
        format!("{:<12}", "Content").bold(),
        format!("{} pages", page_count),
        shared::get_elapsed_time(t).dimmed()
    );

    // Category pages
    let t = Instant::now();
    let cat_count = build_category_pages(&tera, &paths.public, &posts, &site_config, &collections)?;
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
    let feed_count = generate_xml_feeds(&tera, &shared_context, &paths.public)?;
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
        asset_count += copy_assets(&paths.theme_assets, &public_assets_dir, minify)?;
    }
    asset_count += copy_assets(&paths.assets, &public_assets_dir, minify)?;
    timings.assets_ms = t.elapsed().as_millis();
    println!(
        "  {} {}  {:<12}  {}",
        "•".green(),
        format!("{:<12}", "Assets").bold(),
        format!("{} files", asset_count),
        shared::get_elapsed_time(t).dimmed()
    );

    // post_build hook
    if plugin_mgr.has_hook(plugin::HOOK_POST_BUILD) {
        let config_json = serde_json::to_string(&site_config)
            .unwrap_or_default();
        for p in plugin_mgr.plugins() {
            if let Some(f) = p.hooks.post_build {
                p.call_hook(f, &config_json);
            }
        }
    }

    println!();
    let total_ms = build_start.elapsed().as_millis();
    println!(
        "{} Built in {}",
        "✓".green().bold(),
        shared::get_elapsed_time(build_start)
    );

    // Save cache
    let t = Instant::now();
    if let Err(e) = cache.save() {
        warn!("Failed to save build cache: {}", e);
    }
    timings.cache_save_ms = t.elapsed().as_millis();

    if tracing::enabled!(tracing::Level::DEBUG) {
        timings.print_summary(total_ms);
    }

    Ok(())
}
