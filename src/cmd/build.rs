use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use eyre::{bail, Result, WrapErr};
use futures_util::{self, StreamExt};
use tera::{Context, Tera};
use tokio::sync::Mutex;
use tracing::{error, info};
use walkdir::{DirEntry, WalkDir};

use crate::{config, fs, schema::ContentSchema, shared};

/// Represents the directory structure of a Norgolith site.
///
/// This struct defines paths to key directories used during the build process,
/// including build artifacts, public output, content sources, and theme resources.
struct SitePaths {
    build: PathBuf,
    public: PathBuf,
    content: PathBuf,
    assets: PathBuf,
    theme_assets: PathBuf,
    templates: PathBuf,
}

impl SitePaths {
    /// Creates a new `SitePaths` instance based on the provided root directory.
    ///
    /// Initializes paths for build artifacts, public output, content sources,
    /// assets, theme assets, and templates by combining with root subdirectories.
    ///
    /// # Arguments
    /// * `root` - Root directory containing norgolith.toml config file
    fn new(root: PathBuf) -> Self {
        Self {
            build: root.join(".build"),
            public: root.join("public"),
            content: root.join("content"),
            assets: root.join("assets"),
            theme_assets: root.join("theme/assets"),
            templates: root.join("templates"),
        }
    }
}

/// Prepares the build directory by cleaning existing artifacts
///
/// # Arguments
/// * `root_path` - Root directory of the site
async fn prepare_build_directory(root_path: &Path) -> Result<()> {
    let public_dir = root_path.join("public");
    if public_dir.exists() {
        tokio::fs::remove_dir_all(&public_dir)
            .await
            .wrap_err(format!(
                "Failed to remove existing public directory: {}",
                public_dir.display()
            ))?;
    }

    tokio::fs::create_dir_all(&public_dir)
        .await
        .wrap_err(format!(
            "Failed to create public directory: {}",
            public_dir.display()
        ))?;

    Ok(())
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
async fn generate_public_build(
    tera: &Tera,
    paths: &SitePaths,
    site_config: config::SiteConfig,
    minify: bool,
) -> Result<()> {
    let entries = WalkDir::new(&paths.build)
        .into_iter()
        .filter_map(|e| e.ok());

    // Shared error state for concurrent validation
    let validation_errors = Arc::new(Mutex::new(Vec::new()));

    // Parallel processing
    futures_util::stream::iter(entries)
        .for_each_concurrent(num_cpus::get(), |entry| {
            let site_config = site_config.clone();
            let validation_errors = Arc::clone(&validation_errors);

            async move {
                if let Err(e) = process_build_entry(
                    entry,
                    tera,
                    paths,
                    &site_config,
                    minify,
                    &validation_errors,
                )
                .await
                {
                    error!("Error processing entry: {:?}", e);
                }
            }
        })
        .await;

    let errors = validation_errors.lock().await;
    if !errors.is_empty() {
        bail!(errors.concat());
    }

    Ok(())
}

/// Processes a single build entry (HTML file with metadata)
///
/// Handles template rendering, metadata validation, and output path determination.
/// Skips draft content and applies minification when enabled.
async fn process_build_entry(
    entry: DirEntry,
    tera: &Tera,
    paths: &SitePaths,
    site_config: &config::SiteConfig,
    minify: bool,
    validation_errors: &Arc<Mutex<Vec<String>>>,
) -> Result<()> {
    let path = entry.path();

    if path.is_file() && path.extension().map(|e| e == "html").unwrap_or(false) {
        let rel_path = path
            .strip_prefix(&paths.build)
            .wrap_err("Failed to strip prefix")?;
        let stem = rel_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| eyre::eyre!("No file stem"))?;

        // Determine output path
        let public_path = determine_public_path(&paths.public, rel_path, stem);

        // Read content and metadata
        let html = tokio::fs::read_to_string(path)
            .await
            .wrap_err_with(|| format!("Failed to read HTML file: {:?}", path))?;
        let meta_path = path.with_extension("meta.toml");

        // Handle metadata loading with proper error fallback
        let metadata = shared::load_metadata(meta_path).await;

        // Metadata schema validation
        if let Some(schema) = &site_config.content_schema {
            validate_metadata(
                path,
                &paths.build,
                &paths.content,
                schema,
                validation_errors,
            )
            .await?;
        }

        // Do not try to build draft content for production builds
        if toml::Value::as_bool(metadata.get("draft").unwrap_or(&toml::Value::from(false)))
            .expect("draft metadata field should be a boolean")
        {
            return Ok(());
        }

        // Get the layout (template) to render the content, fallback to default if not found.
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

        // Render template
        let rendered = tera.render(&(layout + ".html"), &context).unwrap();

        let rendered = if minify {
            minify_html_content(rendered)?
        } else {
            rendered
        };

        // Write rendered output to public path
        write_public_file(&public_path, rendered).await?;
    }
    Ok(())
}

/// Validates content metadata against a schema
///
/// Collects validation errors for aggregated reporting
async fn validate_metadata(
    path: &Path,
    build_dir: &Path,
    content_dir: &Path,
    schema: &ContentSchema,
    validation_errors: &Arc<Mutex<Vec<String>>>,
) -> Result<()> {
    // Get relative content path
    let content_path = path
        .strip_prefix(build_dir)
        .wrap_err("Failed to strip build dir prefix for content path")?
        .with_extension("")
        .to_str()
        .ok_or_else(|| eyre::eyre!("Failed to convert content path to string"))?
        .replace('\\', "/"); // Normalize path separators

    let norg_content_path = content_dir.join(content_path).with_extension("norg");

    // Perform validation
    let errors = shared::validate_content_metadata(
        &norg_content_path,
        build_dir,
        content_dir,
        schema,
        false,
    )
    .await?;

    // Collect errors
    if !errors.is_empty() {
        validation_errors.lock().await.push(errors);
    }
    Ok(())
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
/// * `stem` - The file stem (name without extension) of the HTML file.
///
/// # Returns
/// * `PathBuf` - The final public path for the HTML file.
fn determine_public_path(public_dir: &Path, rel_path: &Path, stem: &str) -> PathBuf {
    if stem == "index" {
        public_dir.join(rel_path)
    } else {
        public_dir
            .join(rel_path.parent().unwrap_or(Path::new(""))) // Handle root path parent gracefully
            .join(stem)
            .join("index.html")
    }
}

/// Writes rendered content to the public directory, ensuring parent directories exist.
///
/// This function creates any necessary parent directories before writing the file
/// to the specified public path. It is used to save rendered HTML content and
/// other assets to their final locations.
///
/// # Arguments
/// * `public_path` - The path where the file should be written in the public directory.
/// * `rendered` - The content to write to the file.
///
/// # Returns
/// * `Result<()>` - `Ok(())` if the file is written successfully, otherwise an error.
async fn write_public_file(public_path: &Path, rendered: String) -> Result<()> {
    tokio::fs::create_dir_all(public_path.parent().unwrap())
        .await
        .wrap_err(format!(
            "Failed to create parent directory for: {}",
            public_path.display()
        ))?;
    tokio::fs::write(public_path, rendered)
        .await
        .wrap_err(format!(
            "Failed to write to public path: {}",
            public_path.display()
        ))?;
    Ok(())
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
fn should_minify_asset(src: &Path) -> bool {
    let file_stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let file_ext = src.extension().and_then(|s| s.to_str()).unwrap_or_default();
    file_stem != "min" && (file_ext == "js" || file_ext == "css")
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
fn minify_html_content(rendered: String) -> Result<String> {
    let minify_config = minify_html::Cfg {
        minify_js: true,
        minify_css: true,
        ..minify_html::Cfg::default()
    };
    String::from_utf8(minify_html::minify(rendered.as_bytes(), &minify_config))
        .map_err(|e| eyre::eyre!("HTML minification failed: {}", e))
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
    .map_err(|e| eyre::eyre!("JS minification failed for {}: {}", src_path.display(), e))?;
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
async fn minify_css_asset(src_path: &Path, dest_path: &Path) -> Result<()> {
    let content = tokio::fs::read_to_string(src_path).await?;
    // See https://docs.rs/css-minify/0.5.2/css_minify/optimizations/enum.Level.html#variants
    let minified = css_minify::optimizations::Minifier::default()
        .minify(&content, css_minify::optimizations::Level::Two)
        .map_err(|e| eyre::eyre!("CSS minification failed for {}: {}", src_path.display(), e))?;
    tokio::fs::write(dest_path, minified.into_bytes())
        .await
        .wrap_err_with(|| format!("Failed to write minified CSS to {}", dest_path.display()))?;
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
async fn copy_asset_file(src_path: &Path, dest_path: &Path, minify: bool) -> Result<()> {
    if minify {
        if should_minify_asset(src_path) {
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
    } else {
        // Copy file as binary, this lets us write images and some other formats as well instead of only text files
        copy_binary_asset(src_path, dest_path).await?;
    }
    Ok(())
}

/// Copies all site and theme assets to the public directory.
///
/// Theme assets are copied first, followed by site assets, allowing site assets to override
/// theme assets with the same name. Supports optional minification of JS and CSS files.
///
/// # Arguments
/// * `site_assets_dir` - Path to the site's assets directory.
/// * `theme_assets_dir` - Path to the theme's assets directory.
/// * `root_path` - Root directory of the site.
/// * `minify` - Whether to minify supported assets during copying.
///
/// # Returns
/// * `Result<()>` - `Ok(())` if all assets are copied successfully, otherwise an error.
async fn copy_all_assets(
    site_assets_dir: &Path,
    theme_assets_dir: &Path,
    root_path: &Path,
    minify: bool,
) -> Result<()> {
    // Copy theme assets first
    if theme_assets_dir.exists() {
        copy_assets(theme_assets_dir, root_path, minify).await?;
    }

    // Copy site assets (overrides theme assets)
    copy_assets(site_assets_dir, root_path, minify).await?;

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
/// * `root_path` - The root directory of the site, used to determine the public output path.
/// * `minify` - Whether to minify supported assets (e.g., JS and CSS) during the copy process.
///
/// # Returns
/// * `Result<()>` - `Ok(())` if all assets are copied successfully, otherwise an error.
async fn copy_assets(assets_dir: &Path, root_path: &Path, minify: bool) -> Result<()> {
    let public_dir = root_path.join("public");
    let public_assets = public_dir.join("assets");

    /// Recursively processes a directory entry and copies it to the destination.
    ///
    /// This helper function is used by `copy_assets` to handle individual files and directories.
    /// For directories, it recursively processes their contents. For files, it copies them
    /// to the destination with optional minification.
    ///
    /// # Arguments
    /// * `src_path` - The source path of the file or directory to process.
    /// * `dest_path` - The destination path where the file or directory should be copied.
    /// * `minify` - Whether to minify supported assets during the copy process.
    ///
    /// # Returns
    /// * `Result<()>` - `Ok(())` if the entry is processed successfully, otherwise an error.
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
        } else {
            copy_asset_file(src_path, dest_path, minify).await?;
        }
        Ok(())
    }

    Box::pin(process_entry(assets_dir, &public_assets, minify)).await?;

    Ok(())
}

/// Main build entry point
///
/// Orchestrates the complete build process:
/// 1. Load configuration
/// 2. Prepare directories
/// 3. Convert content
/// 4. Generate public build
/// 5. Copy assets
///
/// # Arguments
/// * `minify` - Enable minification of HTML/CSS/JS outputs
pub async fn build(minify: bool) -> Result<()> {
    let root = fs::find_config_file().await?;
    if let Some(root) = root {
        let build_start = std::time::Instant::now();

        // Load site configuration, root already contains the norgolith.toml path
        let config_content = tokio::fs::read_to_string(&root).await?;
        let site_config: config::SiteConfig = toml::from_str(&config_content)?;

        let root_dir = root.parent().unwrap().to_path_buf();

        // Tera wants a `dir: &str` parameter for some reason instead of asking for a `&Path` or `&PathBuf`...
        let paths = SitePaths::new(root_dir.clone());

        // Initialize Tera once
        let tera = shared::init_tera(
            paths.templates.to_str().unwrap(),
            paths.theme_assets.parent().unwrap(),
        )
        .await?;

        // Prepare the public build directory
        prepare_build_directory(Path::new(&root_dir)).await?;

        // Convert the norg documents to html
        shared::convert_content(&paths.content, false, &site_config.root_url).await?;

        // Clean up orphaned files before building the site
        shared::cleanup_orphaned_build_files(&paths.content).await?;

        // Generate public HTML build
        generate_public_build(&tera, &paths, site_config, minify).await?;

        // Copy site assets
        copy_all_assets(
            &paths.assets,
            &paths.theme_assets,
            Path::new(&root_dir.clone()),
            minify,
        )
        .await?;

        info!(
            "Finished site build in {}",
            shared::get_elapsed_time(build_start)
        );
    } else {
        bail!("Could not build the site: not in a Norgolith site directory");
    }

    Ok(())
}
