use std::collections::HashMap;
use std::convert::Infallible;
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr, TcpListener as StdTcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use colored::Colorize;
use eyre::{bail, eyre, Result};
use futures_util::{SinkExt, Stream, StreamExt};
use hyper::header::{CACHE_CONTROL, EXPIRES, PRAGMA};
use hyper::service::{make_service_fn, service_fn};
use hyper::{header::CONTENT_TYPE, Body, Request, Response, Server, StatusCode};
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use tera::{Context, Tera};
use tokio::sync::broadcast;
use tokio::{
    net::{TcpListener, TcpStream},
    runtime::Handle,
    sync::RwLock,
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_tungstenite::accept_async;
use tracing::{debug, error, info, instrument, warn};
use walkdir::WalkDir;

use crate::{config, fs, plugin, shared};

/// Represents the directory structure of a Norgolith site.
///
/// This struct defines the paths to key directories in a Norgolith site, including
/// content, assets, templates, and theme-specific assets and templates. It is used
/// to organize and manage the file structure of the site.
#[derive(Debug)]
struct SitePaths {
    config_file: PathBuf,
    content: PathBuf,
    assets: PathBuf,
    templates: PathBuf,
    theme_assets: PathBuf,
    theme_templates: PathBuf,
}

impl SitePaths {
    /// Creates a new `SitePaths` instance based on the provided root directory.
    ///
    /// This function initializes the paths for the content, assets, templates, and
    /// theme-specific directories by joining the root directory with the respective
    /// subdirectories.
    ///
    /// # Arguments
    /// * `root` - The root directory of the Norgolith site.
    ///
    /// # Returns
    /// * `SitePaths` - A new instance of `SitePaths` with the initialized directory paths.
    #[instrument(skip(root))]
    fn new(root: PathBuf) -> Self {
        debug!("Initializing site paths");
        let paths = Self {
            config_file: root.join("norgolith.toml"),
            content: root.join("content"),
            assets: root.join("assets"),
            theme_assets: root.join("theme/assets"),
            templates: root.join("templates"),
            theme_templates: root.join("theme/templates"),
        };
        debug!(?paths, "Configured site directories");
        paths
    }
}

/// Global state for the server, including reloading functionality.
///
/// This struct holds the shared state for the server, including the WebSocket reload
/// channel, Tera templates, site configuration, directory paths, and server settings.
/// It is used to manage the server's runtime state and facilitate communication
/// between components.
struct ServerState {
    reload_tx: Arc<broadcast::Sender<()>>,
    tera: Arc<RwLock<Tera>>,
    config: Arc<RwLock<config::SiteConfig>>,
    paths: SitePaths,
    build_drafts: bool,
    routes_url: String,
    posts: Arc<RwLock<Vec<toml::Value>>>,
    cache: Arc<RwLock<crate::cache::BuildCache>>,
    rendered_pages: Arc<RwLock<HashMap<String, String>>>,
    plugin_mgr: Arc<plugin::PluginManager>,
}

impl ServerState {
    /// Reloads the Tera templates.
    ///
    /// This function triggers a full reload of the Tera templates. It is called when
    /// changes to template files are detected. If the reload fails, an error is returned.
    ///
    /// # Returns
    /// * `Result<()>` - `Ok(())` if the templates are reloaded successfully, otherwise
    ///   an error is returned.
    #[instrument(level = "debug", skip(self))]
    async fn reload_templates(&self) -> Result<()> {
        debug!("Reloading templates");
        // XXX: for some reason Tera::full_reload is not working properly for us and thus we have to
        //      create a new Tera instance to be able to actually have the content reloaded.
        //      I think this may be a little inefficient if the templates are being constantly reloaded
        //      but who cares, it does the job and I am not willing to keep debugging this any longer right now.
        let new_tera = shared::init_tera(
            self.paths.templates.to_str().unwrap(),
            &self.paths.theme_templates,
        )?;
        let mut tera = self.tera.write().await;
        *tera = new_tera;

        info!("Templates reloaded successfully");
        let templates: Vec<&str> = tera.get_template_names().collect();
        debug!("There are {} templates loaded", templates.len());

        // Reload the page
        self.send_reload()?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    async fn reload_config(&self) -> Result<()> {
        debug!("Reloading config");
        let config_content = tokio::fs::read_to_string(&self.paths.config_file).await?;
        let new_config: config::SiteConfig = toml::from_str(&config_content)?;

        // Re-collect posts, collections config may have changed
        let new_posts = shared::collect_all_posts_metadata(
            &self.paths.content,
            &self.routes_url,
            &new_config.collections,
        )?;

        {
            let mut config = self.config.write().await;
            *config = new_config;
        }
        {
            let mut posts = self.posts.write().await;
            *posts = new_posts;
        }

        info!("Config reloaded successfully");
        self.send_reload()?;
        Ok(())
    }

    /// Rebuilds the in-memory rendered pages cache.
    ///
    /// Called when content, templates, or config change. Re-renders all pages
    /// and replaces the cache wholesale.
    #[instrument(level = "debug", skip(self))]
    async fn rebuild_rendered_pages(&self) {
        let tera = self.tera.read().await;
        let config = self.config.read().await.clone();
        let posts = self.posts.read().await.clone();
        let cache = self.cache.read().await;

        match render_all_pages(&tera, &self.paths, &config, &self.routes_url, &posts, &cache, &self.plugin_mgr) {
            Ok(new_pages) => {
                let mut pages = self.rendered_pages.write().await;
                *pages = new_pages;
                info!("Rendered pages cache rebuilt");
            }
            Err(e) => error!("Failed to rebuild rendered pages: {}", e),
        }
    }

    /// Sends a reload signal to connected WebSocket clients.
    ///
    /// This function sends a signal to all connected WebSocket clients to trigger
    /// a page reload. It is used when changes to assets, templates, or content are
    /// detected. If the signal fails to send, an error is returned.
    ///
    /// # Returns
    /// * `Result<()>` - `Ok(())` if the signal is sent successfully, otherwise
    ///   an error is returned.
    #[instrument(skip(self))]
    fn send_reload(&self) -> Result<()> {
        debug!("Sending reload signal to clients");
        if self.reload_tx.receiver_count() == 0 {
            debug!("No active receivers, skipping reload signal");
            return Ok(());
        }

        self.reload_tx
            .send(())
            .map(|_| {
                debug!(
                    "Reload signal sent to {} clients",
                    self.reload_tx.receiver_count()
                );
            })
            .map_err(|e| eyre!("Failed to send reload signal: {}", e))
    }
}

/// Represents actions to be taken based on file changes.
///
/// This struct defines the actions that should be performed when file system events
/// are detected. It includes flags for reloading templates and assets, as well as
/// lists of paths to rebuild or clean up.
#[derive(Default, Debug, Clone)]
struct FileActions {
    reload_templates: bool,
    reload_assets: bool,
    reload_content: bool,
    reload_config: bool,
}

/// LiveReload script to be injected into HTML pages.
///
/// The LiveReload script version in use is v4.0.2
const LIVE_RELOAD_SCRIPT: &str = include_str!("../resources/assets/livereload.js");
/// Port for the LiveReload WebSocket server
const LIVE_RELOAD_PORT: u16 = 35729;
/// WebSocket hello message for LiveReload protocol
const WS_HELLO_MESSAGE: &str = r#"{"command":"hello","protocols":["http://livereload.com/protocols/official-7"],"serverName":"norgolith"}"#;
/// WebSocket reload message for LiveReload protocol
const WS_RELOAD_MESSAGE: &str = r#"{"command":"reload","path":"/"}"#;

/// Checks if a file system event is relevant for triggering a reload.
///
/// This function determines whether a file system event should trigger a reload
/// based on its type. It considers events such as file creation, removal, and
/// data modification as relevant.
///
/// # Arguments
/// * `event` - The file system event to check.
///
/// # Returns
/// * `bool` - `true` if the event is relevant, `false` otherwise.
fn is_relevant_event(event: &notify::Event) -> bool {
    matches!(
        event.kind,
        notify::EventKind::Create(_)
            | notify::EventKind::Remove(_)
            | notify::EventKind::Modify(notify::event::ModifyKind::Data(_))
    )
}

/// Checks if a file system event corresponds to a template change.
///
/// This function determines whether the event is relevant to the templates directory
/// and whether it should trigger a template reload. It checks if the file has an
/// `.html` extension and is located within the templates directory.
///
/// # Arguments
/// * `event` - The file system event to check.
///
/// # Returns
/// * `bool` - `true` if the event is a template change, `false` otherwise.
#[instrument(level = "debug", skip(event))]
async fn is_template_change(event: &notify::Event) -> bool {
    let Some(path) = event.paths.first() else {
        return false;
    };
    let is_template = path
        .extension()
        .is_some_and(|ext| ext == "html" || ext == "xml");
    let Some(parent_dir) = path.parent() else {
        return false;
    };

    is_relevant_event(event)
        && is_template
        && fs::find_in_previous_dirs("dir", "templates", &mut parent_dir.to_path_buf())
            .is_ok()
}

/// Checks if a file system event corresponds to a content change.
///
/// This function determines whether the event is relevant to the content directory
/// and whether it should trigger a content rebuild. It does not check for specific
/// file types (e.g., `.norg` files) because the content directory may also contain
/// assets like images, and changes to these files should also trigger a reload.
///
/// # Arguments
/// * `event` - The file system event to check.
///
/// # Returns
/// * `bool` - `true` if the event is a content change, `false` otherwise.
#[instrument(level = "debug", skip(event))]
async fn is_content_change(event: &notify::Event) -> bool {
    // NOTE: we do not check for the norg filetype here because content directory
    // can also hold assets like images, and we want to also trigger a reload when
    // an asset file is created, modified or removed.
    let Some(path) = event.paths.first() else {
        return false;
    };
    let Some(parent_dir) = path.parent() else {
        return false;
    };

    is_relevant_event(event)
        && fs::find_in_previous_dirs("dir", "content", &mut parent_dir.to_path_buf())
            .is_ok()
}

/// Checks if a file system event corresponds to an asset change.
///
/// This function determines whether the event is relevant to the assets directory
/// and whether it should trigger an asset reload. It does not check for specific
/// file types because the assets directory can contain various file types (e.g., CSS, JS, images).
///
/// # Arguments
/// * `event` - The file system event to check.
///
/// # Returns
/// * `bool` - `true` if the event is an asset change, `false` otherwise.
#[instrument(level = "debug", skip(event))]
async fn is_asset_change(event: &notify::Event) -> bool {
    // NOTE: we do not check for any filetype here because assets directory
    // can hold assets like css, javascript, images, etc and we want to
    // trigger a reload when any asset file is created, modified or removed.
    let Some(path) = event.paths.first() else {
        return false;
    };
    let Some(parent_dir) = path.parent() else {
        return false;
    };

    // FIXME: find from given path instad of traversing file system
    is_relevant_event(event)
        && fs::find_in_previous_dirs("dir", "assets", &mut parent_dir.to_path_buf())
            .is_ok()
}

/// Processes debounced file system events and triggers appropriate actions.
///
/// This function handles the result of debounced file system events. If the events
/// are valid, it processes them to determine the necessary actions (e.g., reloading
/// templates, rebuilding content). If there are errors in the watcher, it logs them.
///
/// # Arguments
/// * `result` - The result of the debounced file system events.
/// * `state` - The shared server state.
#[instrument(name = "watcher", skip(result, state))]
async fn process_debounced_events(result: DebounceEventResult, state: Arc<ServerState>) {
    match result {
        DebounceEventResult::Ok(events) => {
            debug!("Processing {} file events", events.len());
            handle_file_events(events, state).await
        }
        DebounceEventResult::Err(errors) => {
            error!("Watcher errors: {:?}", errors);
        }
    }
}

/// Executes actions based on file changes, such as reloading assets or templates.
///
/// This function processes the actions determined by file system events, such as
/// reloading assets, reloading templates, cleaning up orphaned files, or rebuilding
/// content. It logs the results of these actions.
///
/// # Arguments
/// * `actions` - The actions to execute.
/// * `state` - The shared server state.
#[instrument(level = "debug", skip(actions, state))]
async fn execute_actions(actions: FileActions, state: Arc<ServerState>) {
    debug!(
        "Executing actions: templates={}, assets={}, reload={}, config={}",
        actions.reload_templates,
        actions.reload_assets,
        actions.reload_content,
        actions.reload_config,
    );

    // Config reload supersedes content/template/asset reloads since it re-collects posts too
    if actions.reload_config {
        match state.reload_config().await {
            Ok(_) => {}
            Err(e) => error!("Config reload failed: {}", e),
        }
        state.rebuild_rendered_pages().await;
        return;
    }

    // Handle asset reloads
    if actions.reload_assets {
        if let Err(e) = state.send_reload() {
            error!("Asset reload error: {}", e);
        }
    }

    // Handle template reloads
    if actions.reload_templates {
        match state.reload_templates().await {
            Ok(_) => {
                state.rebuild_rendered_pages().await;
                if let Err(e) = state.send_reload() {
                    error!("Template reload signal error: {}", e);
                }
            }
            Err(e) => error!("Template reload failed: {}", e),
        }
    }

    if actions.reload_content {
        let collections = state.config.read().await.collections.clone();
        match shared::collect_all_posts_metadata(
            &state.paths.content,
            &state.routes_url,
            &collections,
        )
        {
            Ok(new_posts) => {
                let mut posts_lock = state.posts.write().await;
                *posts_lock = new_posts;
            }
            Err(e) => error!("Failed to update pages metadata: {}", e),
        }

        state.rebuild_rendered_pages().await;

        if let Err(e) = state.send_reload() {
            error!("Reload signal error: {}", e);
        }
    }
}

/// Injects the LiveReload script into HTML content.
///
/// This function modifies the provided HTML string by inserting the LiveReload script
/// just before the closing `</body>` tag. The script enables automatic page reloading
/// when changes are detected.
///
/// # Arguments
/// * `html` - The HTML content to modify.
#[instrument(skip(html))]
fn inject_livereload_script(html: &mut String) {
    debug!("Injecting LiveReload script");

    if let Some(pos) = html.rfind("</body>") {
        html.insert_str(
            pos,
            &format!(
                r#"<script src="/livereload.js?port={}&amp;mindelay=10"></script>"#,
                LIVE_RELOAD_PORT
            ),
        );
    }
}

/// Reads an asset file and returns its content and MIME type.
///
/// This function reads the content of an asset file and determines its MIME type
/// based on the file extension. It is used to serve static assets like CSS, JS, and images.
///
/// # Arguments
/// * `path` - The path to the asset file.
///
/// # Returns
/// * `Result<(Vec<u8>, String)>` - A tuple containing the file content and its MIME type.
///   Returns an error if the file cannot be read.
#[instrument(skip(path))]
async fn read_asset(path: &Path) -> Result<(Vec<u8>, String)> {
    debug!(path = %path.display(), "Reading asset");

    let content = tokio::fs::read(path)
        .await
        .map_err(|e| eyre!("Failed to read asset: {}", e))?;
    let mime_type = mime_guess::from_path(path)
        .first_or_octet_stream()
        .as_ref()
        .to_string();

    debug!(mime_type = %mime_type, "Determined asset MIME type");
    Ok((content, mime_type))
}

/// Handles file system events and updates the server state accordingly.
///
/// This function processes a list of debounced file system events and determines the
/// necessary actions (e.g., reloading templates, rebuilding content). It updates the
/// server state based on the detected changes.
///
/// # Arguments
/// * `events` - The list of file system events to process.
/// * `state` - The shared server state.
async fn handle_file_events(
    events: Vec<notify_debouncer_full::DebouncedEvent>,
    state: Arc<ServerState>,
) {
    let mut actions = FileActions::default();

    for event in events {
        if let Some(path) = event.paths.first() {
            handle_single_event(&event, path, &mut actions, &state).await;
        }
    }

    execute_actions(actions, state).await;
}

/// Handles a single file system event and updates the actions to be taken.
///
/// This function processes a single file system event and updates the `FileActions`
/// struct to reflect the necessary actions (e.g., reloading templates, rebuilding content).
/// It ignores temporary backup files (e.g., from NeoVim) to avoid unnecessary reloads.
///
/// # Arguments
/// * `event` - The file system event to process.
/// * `path` - The path associated with the event.
/// * `actions` - The actions to update based on the event.
/// * `state` - The shared server state.
#[instrument(level = "debug", skip(event, path, actions, state))]
async fn handle_single_event(
    event: &notify::Event,
    path: &Path,
    actions: &mut FileActions,
    state: &Arc<ServerState>,
) {
    if !is_relevant_event(event) {
        return;
    }
    debug!(event = ?event.kind, path = %path.display(), "Processing file event");

    // We are excluding these fucking temp (Neo)vim backup files because they trigger
    // stupid bugs that I'm not willing to debug anymore.
    //
    // TODO: also ignore swap files, my mental health will thank me later.
    if path.to_string_lossy().ends_with('~') {
        debug!("Ignoring temporary editor backup file");
        return;
    }

    if path == state.paths.config_file {
        info!("Config modified: norgolith.toml");
        actions.reload_config = true;
        return;
    }

    if is_template_change(event).await {
        if let Ok(rel) = path.strip_prefix(&state.paths.theme_templates) {
            info!("Template modified: {}", rel.display());
            actions.reload_templates = true;
        } else if let Ok(rel) = path.strip_prefix(&state.paths.templates) {
            info!("Template modified: {}", rel.display());
            actions.reload_templates = true;
        }
    }

    if is_asset_change(event).await {
        if let Ok(rel) = path.strip_prefix(&state.paths.theme_assets) {
            info!("Asset modified: {}", rel.display());
            actions.reload_assets = true;
        } else if let Ok(rel) = path.strip_prefix(&state.paths.assets) {
            info!("Asset modified: {}", rel.display());
            actions.reload_assets = true;
        }
    }

    // PERF: don't check for other content files as we will reload all clients anyways
    debug!(?actions.reload_content, "reload_content");
    if !actions.reload_content
        && is_content_change(event).await
        && path.strip_prefix(&state.paths.content).is_ok()
    {
        debug!(path = %path.display(), "Content modified");
        actions.reload_content = true;
    }
}

/// Handles requests for static assets.
///
/// This function serves static assets from the assets directory or the theme assets directory
/// if the file is not found in the primary assets directory. It returns a `Response` with
/// the file content and appropriate MIME type.
///
/// # Arguments
/// * `request_path` - The path of the requested asset.
/// * `paths` - The site directory paths.
///
/// # Returns
/// * `Result<Response<Body>>` - A `Response` containing the asset content or a 404 error
///   if the asset is not found.
#[instrument(skip(request_path, paths))]
async fn handle_asset(request_path: &str, paths: &SitePaths) -> Result<Response<Body>> {
    let asset_path = request_path.trim_start_matches("/assets/");
    debug!(path = %asset_path, "Handling asset request");

    let site_path = paths.assets.join(asset_path);

    debug!(site_assets = %site_path.display(), "Checking site assets path");
    let (content, mime_type) = match read_asset(&site_path).await {
        Ok(asset) => {
            debug!("Asset found in site directory");
            asset
        }
        Err(_) => {
            // Fallback to theme assets
            debug!("Asset not found in site directory, checking theme assets");
            let theme_path = paths.theme_assets.join(asset_path);
            match read_asset(&theme_path).await {
                Ok(asset) => {
                    debug!("Asset found in theme directory");
                    asset
                }
                Err(_) => {
                    error!(asset_path = %request_path, "Asset not found in site or theme directories");
                    return Ok(handle_not_found());
                }
            }
        }
    };
    Ok(Response::builder()
        .header(CONTENT_TYPE, mime_type)
        .status(StatusCode::OK)
        .header(
            CACHE_CONTROL,
            "no-store, no-cache, must-revalidate, proxy-revalidate",
        )
        .header(PRAGMA, "no-cache")
        .header(EXPIRES, 0)
        .body(Body::from(content))?)
}

fn handle_not_found() -> Response<Body> {
    // TODO: try load from templates
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("not found"))
        .expect("Could not build Not Found response")
}

async fn resolve_url_norg_path(content_dir: &Path, path: &Path) -> std::io::Result<PathBuf> {
    use tokio::fs;
    let mut path = content_dir.join(path);
    debug!(?path);
    // try "{path}.norg"
    if path.file_name().is_some() {
        let path = path.with_extension("norg");
        debug!(?path);
        if fs::metadata(&path).await.is_ok_and(|m| m.is_file()) {
            return Ok(path);
        }
    }
    // try {path}/index.norg
    let metadata = fs::metadata(&path).await?;
    if metadata.is_dir() {
        path.push("index.norg");
    }
    Ok(path)
}

/// Renders an XML feed template for the requested path.
///
/// Strips the leading `/` from the request path to derive the template name
/// (e.g. `/rss.xml` → `rss.xml`, `/en/rss.xml` → `en/rss.xml`). Returns 404
/// if no matching XML template is registered.
async fn handle_xml_feed(request_path: &str, state: &Arc<ServerState>) -> Result<Response<Body>> {
    let template_name = request_path.trim_start_matches('/');
    debug!(template = %template_name, "Handling XML feed request");

    // Fast path: lookup in pre-rendered memory cache
    {
        let pages = state.rendered_pages.read().await;
        if let Some(html) = pages.get(request_path) {
            return Ok(Response::builder()
                .header(CONTENT_TYPE, "application/xml; charset=utf-8")
                .status(StatusCode::OK)
                .body(Body::from(html.clone()))?);
        }
    }

    // Slow path: render on demand
    let tera = state.tera.read().await;
    if !tera.get_template_names().any(|n| n == template_name) {
        return Ok(handle_not_found());
    }

    let config = state.config.read().await.clone();
    let posts = state.posts.read().await.clone();
    let collections = shared::precompute_collection_subsets(&posts, &config);
    let shared_context = shared::build_shared_context(&posts, &config, &collections);
    let mut context = shared_context;
    context.insert("now", &Utc::now());

    let content = tera
        .render(template_name, &context)
        .map_err(|e| eyre!("{}: {}", "Failed to render XML feed template".bold(), e))?;

    Ok(Response::builder()
        .header(CONTENT_TYPE, "application/xml; charset=utf-8")
        .status(StatusCode::OK)
        .body(Body::from(content))?)
}

/// Handles requests for content, either static or dynamic.
///
/// Serves content from the content directory. If the content is a static file
/// (e.g., an image), it serves it directly. Otherwise, it renders the content as HTML
/// using Tera templates.
async fn handle_content(request_path: &str, state: Arc<ServerState>) -> Result<Response<Body>> {
    let req_path = PathBuf::from(request_path.trim_start_matches('/'));
    debug!(?req_path);
    match resolve_url_norg_path(&state.paths.content, &req_path).await {
        Ok(path) => handle_norg_content(path, state).await,
        Err(io_err) => match io_err.kind() {
            std::io::ErrorKind::NotFound => Ok(handle_not_found()),
            std::io::ErrorKind::PermissionDenied => Ok(Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Body::empty())
                .unwrap()),
            _ => Err(eyre!("Error reading '{}': {}", req_path.display(), io_err)),
        },
    }
}

/// Handles requests for HTML content, rendering it with Tera templates.
///
/// This function renders HTML content using Tera templates and injects the LiveReload script
/// into the rendered HTML. It also loads metadata associated with the content and passes it
/// to the template context.
///
/// # Arguments
/// * `content` - The content to render.
/// * `path` - The path of the content.
/// * `state` - The shared server state.
///
/// # Returns
/// * `Result<Response<Body>>` - A `Response` containing the rendered HTML or an error if
///   rendering fails.
async fn handle_norg_content(path: PathBuf, state: Arc<ServerState>) -> Result<Response<Body>> {
    let rel_path = path.strip_prefix(&state.paths.content)?.to_path_buf();

    // Fast path: lookup in pre-rendered memory cache
    {
        let pages = state.rendered_pages.read().await;
        let url_path = format!("/{}", rel_path.with_extension("").display());
        if let Some(html) = pages.get(&url_path) {
            let mut body = html.clone();
            inject_livereload_script(&mut body);
            return Ok(Response::builder()
                .header(CONTENT_TYPE, "text/html; charset=utf-8")
                .status(StatusCode::OK)
                .body(Body::from(body))?);
        }
    }

    // Slow path: not in cache (e.g. file changed since last render), render on demand
    let tera = state.tera.read().await;

    let Ok(content) = tokio::fs::read_to_string(&path).await else {
        return Ok(handle_not_found());
    };

    let metadata = shared::extract_metadata_from_content(&content, &rel_path, &state.routes_url);
    let is_draft = metadata
        .get("draft")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if is_draft && !state.build_drafts {
        return Ok(handle_not_found());
    }

    let cache_key = rel_path.with_extension("");
    let metadata = {
        let cache_guard = state.cache.read().await;
        cache_guard.get(&cache_key, &content)
    };
    let metadata = if let Some(cached) = metadata {
        match serde_json::from_value(cached.clone()) {
            Ok(md) => md,
            Err(_) => shared::load_metadata_from_content(&content, &rel_path, &state.routes_url),
        }
    } else {
        let md = shared::load_metadata_from_content(&content, &rel_path, &state.routes_url);
        if let Ok(json_val) = serde_json::to_value(&md) {
            let mut cache_guard = state.cache.write().await;
            cache_guard.insert(&cache_key, &content, json_val);
        }
        md
    };

    let config = state.config.read().await.clone();
    let posts = state.posts.read().await.clone();
    let collections = shared::precompute_collection_subsets(&posts, &config);
    let shared_context = shared::build_shared_context(&posts, &config, &collections);
    let mut body = shared::render_norg_page(&tera, &metadata, &shared_context)?;

    body = body.replace(
        &config.root_url.replace("://", ":&#x2F;&#x2F;"),
        &state.routes_url,
    );

    inject_livereload_script(&mut body);
    Ok(Response::builder()
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .status(StatusCode::OK)
        .body(Body::from(body))?)
}

/// Handles WebSocket connections for LiveReload functionality.
///
/// This function manages WebSocket connections for LiveReload. It sends a hello message
/// to the client upon connection and listens for reload signals to send reload messages.
///
/// # Arguments
/// * `stream` - The TCP stream for the WebSocket connection.
/// * `reload_tx` - The broadcast sender for reload signals.
#[instrument(skip(stream, reload_tx))]
async fn handle_websocket(stream: TcpStream, reload_tx: Arc<broadcast::Sender<()>>) {
    let mut ws_stream = match accept_async(stream).await {
        Ok(ws) => {
            debug!("New WebSocket connection");
            ws
        }
        Err(e) => {
            error!("WebSocket error: {}", e);
            return;
        }
    };

    let mut rx = reload_tx.subscribe();
    if let Err(e) = ws_stream
        .send(tokio_tungstenite::tungstenite::Message::Text(
            WS_HELLO_MESSAGE.into(),
        ))
        .await
    {
        error!("Failed to send hello message: {}", e);
        return;
    }

    loop {
        tokio::select! {
            _ = rx.recv() => {
                if let Err(e) = ws_stream.send(tokio_tungstenite::tungstenite::Message::Text(WS_RELOAD_MESSAGE.into())).await {
                    error!("WebSocket send error: {}", e);
                    break;
                }
            }
            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => break,
                    Some(Err(e)) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn handle_category_index(state: &Arc<ServerState>) -> Result<Response<Body>> {
    let config = state.config.read().await.clone();
    let url_path = format!("/{}", config.categories_dir);

    // Fast path: lookup in pre-rendered memory cache
    {
        let pages = state.rendered_pages.read().await;
        if let Some(html) = pages.get(&url_path) {
            let mut body = html.clone();
            inject_livereload_script(&mut body);
            return Ok(Response::builder()
                .header(CONTENT_TYPE, "text/html; charset=utf-8")
                .status(StatusCode::OK)
                .body(Body::from(body))?);
        }
    }

    // Slow path: render on demand
    let posts = state.posts.read().await.clone();
    let categories = shared::collect_all_posts_categories(&posts);
    let collections = shared::precompute_collection_subsets(&posts, &config);
    let shared_context = shared::build_shared_context(&posts, &config, &collections);
    let mut context = shared_context;
    context.insert("categories", &categories.into_iter().collect::<Vec<_>>());

    let tera = state.tera.read().await;
    let mut body = tera.render("categories.html", &context).map_err(|e| {
        if e.source().is_some() {
            let internal_err = e.source().unwrap();
            eyre!(
                "{}: {}",
                "Failed to render 'categories.html' template".bold(),
                internal_err
            )
        } else {
            eyre!("{}", "Failed to render 'categories.html' template".bold())
        }
    })?;
    body = body.replace(
        &config.root_url.replace("://", ":&#x2F;&#x2F;"),
        &state.routes_url,
    );

    inject_livereload_script(&mut body);
    Ok(Response::builder()
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .status(StatusCode::OK)
        .body(Body::from(body))?)
}

async fn handle_category(path: &str, state: &Arc<ServerState>) -> Result<Response<Body>> {
    let config = state.config.read().await.clone();
    let cat_prefix = format!("/{}/", config.categories_dir);
    let category = path.strip_prefix(&*cat_prefix).unwrap_or(path);

    // Fast path: lookup in pre-rendered memory cache
    {
        let pages = state.rendered_pages.read().await;
        if let Some(html) = pages.get(path) {
            let mut body = html.clone();
            inject_livereload_script(&mut body);
            return Ok(Response::builder()
                .header(CONTENT_TYPE, "text/html; charset=utf-8")
                .status(StatusCode::OK)
                .body(Body::from(body))?);
        }
    }

    // Slow path: render on demand
    let posts = state.posts.read().await.clone();

    let category_posts: Vec<_> = posts
        .into_iter()
        .filter(|post| {
            post.get("categories")
                .and_then(|c| c.as_array())
                .map(|cats| cats.iter().any(|c| c.as_str() == Some(category)))
                .unwrap_or(false)
        })
        .collect();

    let mut context = Context::new();
    context.insert("config", &config);
    context.insert("category", &category);
    context.insert("posts", &category_posts);
    context.insert(
        "lith_version",
        option_env!("LITH_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")),
    );

    let tera = state.tera.read().await;
    let mut body = tera.render("category.html", &context).map_err(|e| {
        if e.source().is_some() {
            let internal_err = e.source().unwrap();
            eyre!(
                "{}: {}",
                "Failed to render 'category.html' template".bold(),
                internal_err
            )
        } else {
            eyre!("{}", "Failed to render 'category.html' template".bold())
        }
    })?;

    body = body.replace(
        &config.root_url.replace("://", ":&#x2F;&#x2F;"),
        &state.routes_url,
    );

    inject_livereload_script(&mut body);
    Ok(Response::builder()
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .status(StatusCode::OK)
        .body(Body::from(body))?)
}

/// Handles HTTP requests and routes them to the appropriate handler.
///
/// This function processes incoming HTTP requests and routes them to the appropriate
/// handler based on the request path. It serves LiveReload scripts, static assets, and
/// dynamic content.
///
/// # Arguments
/// * `req` - The incoming HTTP request.
/// * `state` - The shared server state.
///
/// # Returns
/// * `Result<Response<Body>>` - A `Response` containing the result of the request handling.
async fn handle_request(req: Request<Body>, state: Arc<ServerState>) -> Result<Response<Body>> {
    let request_path = req.uri().path();
    debug!(path = %request_path, "Handling request");

    let categories_dir = state.config.read().await.categories_dir.clone();
    match request_path {
        "/livereload.js" => Ok(Response::builder()
            .header(CONTENT_TYPE, "text/javascript")
            .body(LIVE_RELOAD_SCRIPT.into())?),
        path if path == format!("/{}", categories_dir) => handle_category_index(&state).await,
        path if path.starts_with(&format!("/{}/", categories_dir)) => {
            handle_category(path, &state).await
        }
        path if path.starts_with("/assets/") => handle_asset(path, &state.paths).await,
        path if path.ends_with(".xml") => handle_xml_feed(path, &state).await,
        _ => handle_content(request_path, state).await,
    }
}

/// Handles HTTP requests and logs the results.
///
/// This function wraps the request handling logic and logs the request method, path,
/// status code, and response time. It ensures that errors are handled gracefully and
/// returns an appropriate response.
///
/// # Arguments
/// * `req` - The incoming HTTP request.
/// * `state` - The shared server state.
///
/// # Returns
/// * `Result<Response<Body>, Infallible>` - A `Response` or an error if the request
///   cannot be handled.
#[instrument(name = "serve_request", skip(req, state))]
async fn handle_server_request(
    req: Request<Body>,
    state: Arc<ServerState>,
) -> Result<Response<Body>, Infallible> {
    let start = std::time::Instant::now();
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_owned();

    debug!(method = %method, path = %path, "Incoming request");

    let response = match handle_request(req, state).await {
        Ok(res) => res,
        Err(e) => {
            error!("{}", e);
            // Remove ANSI codes from error string as the colored crate clear method is stupid enough not to do anything
            let e_str = e.to_string().replace("\x1b[1m", "").replace("\x1b[0m", "");
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(format!(
                    "500 Internal Server Error\n\n{}",
                    e_str
                )))
                .unwrap()
        }
    };

    let duration = start.elapsed();
    let status = response.status();

    if path != "/livereload.js" {
        let method_str = method.to_string();
        let method_colored = if method_str == "GET" {
            method_str.green().bold()
        } else {
            method_str.yellow().bold()
        };
        let status_code = status.as_u16();
        let status_colored = match status_code {
            200..=299 => status_code.to_string().green(),
            300..=399 => status_code.to_string().cyan(),
            400..=499 => status_code.to_string().yellow(),
            _ => status_code.to_string().red(),
        };
        let duration_str = format!("{:.1?}", duration);
        let duration_colored = if duration.as_millis() >= 500 {
            duration_str.yellow()
        } else {
            duration_str.dimmed()
        };
        println!(
            "  {} {:<60}  {}  {}",
            method_colored, path, status_colored, duration_colored
        );
    }

    Ok(response)
}

/// Pre-renders all content pages into an in-memory HashMap for instant serving.
///
/// Walks the content directory, renders each .norg file through the Tera template
/// pipeline, and stores the HTML indexed by URL path. Also pre-renders category
/// pages and XML feed templates.
#[allow(clippy::too_many_arguments)]
fn render_all_pages(
    tera: &Tera,
    paths: &SitePaths,
    config: &config::SiteConfig,
    routes_url: &str,
    posts: &[toml::Value],
    cache: &crate::cache::BuildCache,
    plugin_mgr: &plugin::PluginManager,
) -> Result<HashMap<String, String>> {
    let mut pages = HashMap::new();

    let collections = shared::precompute_collection_subsets(posts, config);
    let shared_context = shared::build_shared_context(posts, config, &collections);

    // Render content pages
    for entry in WalkDir::new(&paths.content)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "norg"))
    {
        let path = entry.path();
        let rel_path = match path.strip_prefix(&paths.content) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };

        // Draft check
        let metadata = shared::extract_metadata_from_content(&content, rel_path, routes_url);
        let is_draft = metadata
            .get("draft")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_draft {
            continue;
        }

        // Full load with HTML conversion (reuse build_cache if available)
        let cache_key = rel_path.with_extension("");
        let mut metadata = if let Some(cached) = cache.get(&cache_key, &content) {
            serde_json::from_value(cached).unwrap_or_else(|_| {
                shared::load_metadata_from_content(&content, rel_path, routes_url)
            })
        } else {
            shared::load_metadata_from_content(&content, rel_path, routes_url)
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
                            if let toml::Value::Table(ref mut table) = metadata {
                                table.insert("raw".to_string(), toml::Value::String(new_html));
                            }
                        }
                    }
                }
            }
        }

        let mut body = shared::render_norg_page(tera, &metadata, &shared_context)?;

        // post_render hook: modify final HTML after Tera, before URL rewrite
        if plugin_mgr.has_hook(plugin::HOOK_POST_RENDER) {
            let input = serde_json::json!({
                "html": body,
                "metadata": metadata,
                "rel_path": rel_path.to_string_lossy(),
            })
            .to_string();
            for p in plugin_mgr.plugins() {
                if let Some(f) = p.hooks.post_render {
                    if let Some(new_html) = p.call_hook(f, &input) {
                        body = new_html;
                    }
                }
            }
        }

        // Always use the proper URL to the development server for template links that refers
        // to the local URL, this is useful when running the server exposed to LAN network
        body = body.replace(
            &config.root_url.replace("://", ":&#x2F;&#x2F;"),
            routes_url,
        );

        // URL path: /{rel_path_without_extension}
        let url_path = format!("/{}", rel_path.with_extension("").display());
        pages.insert(url_path, body);
    }

    // Pre-render category index
    if !posts.is_empty() {
        if let Ok(body) = shared::render_category_index(tera, posts, config, &collections) {
            let body = body.replace(
                &config.root_url.replace("://", ":&#x2F;&#x2F;"),
                routes_url,
            );
            pages.insert(format!("/{}", config.categories_dir), body);
        }

        // Pre-render individual category pages
        let categories = shared::collect_all_posts_categories(posts);
        for category in &categories {
            let category_posts: Vec<_> = posts
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
            context.insert("category", category);
            context.insert("posts", &category_posts);
            context.insert(
                "lith_version",
                option_env!("LITH_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")),
            );

            if let Ok(body) = tera.render("category.html", &context) {
                let body = body.replace(
                    &config.root_url.replace("://", ":&#x2F;&#x2F;"),
                    routes_url,
                );
                let url_path = format!("/{}/{}", config.categories_dir, category);
                pages.insert(url_path, body);
            }
        }
    }

    // Pre-render XML feed templates
    for template_name in tera.get_template_names() {
        if !template_name.ends_with(".xml") {
            continue;
        }
        let mut context = shared_context.clone();
        context.insert("now", &Utc::now());
        if let Ok(body) = tera.render(template_name, &context) {
            let url_path = format!("/{}", template_name);
            pages.insert(url_path, body);
        }
    }

    debug!(count = pages.len(), "Pre-rendered pages into memory");
    Ok(pages)
}

/// Sets up the server state with the necessary configurations.
///
/// This function initializes the server state, including loading the site configuration,
/// setting up Tera templates, and creating the WebSocket reload channel. It ensures that
/// all components required for the server to function are properly initialized.
///
/// # Arguments
/// * `root` - The root directory of the site.
/// * `drafts` - Whether to build draft content.
/// * `routes_url` - The local URL on which the server will run.
///
/// # Returns
/// * `Result<Arc<ServerState>>` - The initialized server state or an error if setup fails.
#[instrument(skip(root, drafts, routes_url))]
async fn setup_server_state(
    root: PathBuf,
    drafts: bool,
    routes_url: String,
) -> Result<Arc<ServerState>> {
    debug!("Setting up server state");

    let config_content = tokio::fs::read_to_string(&root).await?;
    let site_config: config::SiteConfig = toml::from_str(&config_content)?;

    let validation_errors = site_config.validate();
    if !validation_errors.is_empty() {
        for error in &validation_errors {
            eprintln!("{}", error);
        }
        bail!("Site configuration has validation errors");
    }

    let root_dir = root.parent().unwrap().to_path_buf();
    let mut paths = SitePaths::new(root_dir.clone());

    // Resolve symlinks for each watched path so that OS-level file events
    // from within a symlinked directory are correctly detected and matched.
    if let Ok(real) = tokio::fs::canonicalize(&paths.content).await {
        paths.content = real;
    }
    if let Ok(real) = tokio::fs::canonicalize(&paths.assets).await {
        paths.assets = real;
    }
    if let Ok(real) = tokio::fs::canonicalize(&paths.templates).await {
        paths.templates = real;
    }
    if let Ok(real) = tokio::fs::canonicalize(&paths.theme_assets).await {
        paths.theme_assets = real;
    }
    if let Ok(real) = tokio::fs::canonicalize(&paths.theme_templates).await {
        paths.theme_templates = real;
    }

    let tera = shared::init_tera(paths.templates.to_str().unwrap(), &paths.theme_templates)?;

    let (reload_tx, _) = broadcast::channel(16);

    let posts =
        shared::collect_all_posts_metadata(&paths.content, &routes_url, &site_config.collections)?;

    // Open build cache for incremental renders
    let cache = crate::cache::BuildCache::open(&root_dir)?;

    // Load plugins, apply sandbox, run pre_build hook
    let plugin_mgr = plugin::PluginManager::load(&root_dir);
    let _ = plugin::sandbox::apply_landlock(&root_dir);
    if plugin_mgr.has_hook(plugin::HOOK_PRE_BUILD) {
        let input = serde_json::json!({
            "site_config": site_config,
            "pages_dir": paths.content,
            "output_dir": root_dir.join("public"),
        })
        .to_string();
        for p in plugin_mgr.plugins() {
            if let Some(f) = p.hooks.pre_build {
                p.call_hook(f, &input);
            }
        }
    }

    // Pre-render all pages into memory for instant serving
    let rendered_pages = render_all_pages(
        &tera,
        &paths,
        &site_config,
        &routes_url,
        &posts,
        &cache,
        &plugin_mgr,
    )?;

    let tera = Arc::new(RwLock::new(tera));

    Ok(Arc::new(ServerState {
        reload_tx: Arc::new(reload_tx),
        tera,
        config: Arc::new(RwLock::new(site_config)),
        paths,
        build_drafts: drafts,
        routes_url,
        posts: Arc::new(RwLock::new(posts)),
        cache: Arc::new(RwLock::new(cache)),
        rendered_pages: Arc::new(RwLock::new(rendered_pages)),
        plugin_mgr: Arc::new(plugin_mgr),
    }))
}

/// Sets up the file watcher for detecting changes in the site directory.
///
/// This function initializes a debounced file watcher to monitor changes in the templates,
/// content, and assets directories. It also watches theme directories if they exist. The
/// watcher uses a debounce mechanism to avoid triggering multiple events for rapid changes.
///
/// # Arguments
/// * `state` - The shared server state.
/// * `rt` - The runtime handle for spawning tasks.
///
/// # Returns
/// * `Result<(Debouncer<RecommendedWatcher, RecommendedCache>, impl Stream<Item = DebounceEventResult>)>` -
///   A tuple containing the debouncer and a stream of debounced events.
#[instrument(skip(state, rt))]
async fn setup_file_watcher(
    state: Arc<ServerState>,
    rt: Handle,
) -> Result<(
    Debouncer<RecommendedWatcher, RecommendedCache>,
    impl Stream<Item = DebounceEventResult>,
)> {
    debug!("Setting up file watcher");

    let (debouncer_tx, debouncer_rx) = tokio::sync::mpsc::channel(16);

    // Create debouncer with 200ms delay, this should be enough to handle both the
    // (Neo)vim swap files and also the VSCode atomic saves
    let mut debouncer: Debouncer<RecommendedWatcher, RecommendedCache> = new_debouncer(
        Duration::from_millis(200),
        None,
        move |result: DebounceEventResult| {
            let tx = debouncer_tx.clone();
            rt.spawn(async move {
                if let Err(e) = tx.send(result).await {
                    error!("Debouncer error: {:?}", e);
                }
            });
        },
    )?;

    debouncer.watch(&state.paths.config_file, RecursiveMode::NonRecursive)?;
    debouncer.watch(&state.paths.templates, RecursiveMode::Recursive)?;
    debouncer.watch(&state.paths.content, RecursiveMode::Recursive)?;
    debouncer.watch(&state.paths.assets, RecursiveMode::Recursive)?;
    // Watch theme files only if they exist
    if state.paths.theme_assets.exists() {
        debouncer.watch(&state.paths.theme_assets, RecursiveMode::Recursive)?;
    }
    if state.paths.theme_templates.exists() {
        debouncer.watch(&state.paths.theme_templates, RecursiveMode::Recursive)?;
    }

    Ok((debouncer, ReceiverStream::new(debouncer_rx)))
}

/// Starts the development server.
///
/// This function initializes and runs the development server, including the HTTP server,
/// WebSocket server, and file watcher. It also performs an initial build of the site
/// content and opens the site in the browser if requested. The server listens for file
/// changes and triggers reloads or rebuilds as necessary.
///
/// # Arguments
/// * `port` - The port on which the server will run.
/// * `drafts` - Whether to serve draft content.
/// * `open` - Whether to open the site in the browser after starting the server.
///
/// # Returns
/// * `Result<()>` - `Ok(())` if the server runs successfully, otherwise an error.
#[instrument(skip(port, drafts, open, host))]
pub async fn dev(listener: StdTcpListener, port: u16, drafts: bool, open: bool, host: bool) -> Result<()> {
    println!("{} Starting development server...", "→".cyan().bold());

    let root = fs::find_config_file()?;
    let Some(root) = root else {
        bail!(
            "{}: not in a Norgolith site directory",
            "Could not initialize the development server".bold()
        );
    };

    debug!(path = %root.display(), "Found site root");

    // Early set the development URL to the site routes
    let local_ip = local_ip_address::local_ip().unwrap_or(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    let routes_url = if host {
        format!("http://{}:{}", local_ip, port)
    } else {
        format!("http://localhost:{}", port)
    };
    let state = setup_server_state(root, drafts, routes_url).await?;
    let server_start = std::time::Instant::now();
    let rt = Handle::current();

    // Create initial receiver to always keep channel alive, this way
    // any "channel closed" errors are prevented from happening
    let _guard_receiver = state.reload_tx.subscribe();

    // WebSocket server
    let reload_tx = state.reload_tx.clone();
    tokio::spawn(async move {
        let listener = match TcpListener::bind(format!("127.0.0.1:{}", LIVE_RELOAD_PORT)).await {
            Ok(l) => l,
            Err(e) => {
                error!(
                    "LiveReload disabled: failed to bind port {}: {}",
                    LIVE_RELOAD_PORT, e
                );
                return;
            }
        };
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(handle_websocket(stream, reload_tx.clone()));
        }
    });

    // File watcher and event processing
    let (debouncer, mut debouncer_rx) = setup_file_watcher(state.clone(), rt.clone()).await?;
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        // Move debouncer into the async block, otherwise the file watcher does not work at all.
        // I spent at least hour and a half debugging this and the solution was really this simple...
        let _debouncer = debouncer;

        while let Some(result) = debouncer_rx.next().await {
            process_debounced_events(result, state_clone.clone()).await;
        }
    });

    // HTTP server
    let state_clone = Arc::clone(&state);
    let make_svc = make_service_fn(move |_| {
        let state = state_clone.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                handle_server_request(req, state.clone())
            }))
        }
    });
    listener.set_nonblocking(true)?;
    // Graceful shutdown via Ctrl-D (stdin EOF)
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut stdin = tokio::io::stdin();
        let mut buf = [0u8; 1];
        loop {
            match stdin.read(&mut buf).await {
                Ok(0) | Err(_) => {
                    let _ = shutdown_tx.send(());
                    break;
                }
                _ => {}
            }
        }
    });

    let server = Server::from_tcp(listener)?
        .serve(make_svc)
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        });

    let localhost_address = format!(
        "{} {}   {}",
        "•".green(),
        "Local:".bold(),
        format!("http://localhost:{}/", port.to_string().cyan().bold()).blue()
    );
    let lan_address = if host {
        format!(
            "{} {} {}",
            "•".green(),
            "Network:".bold(),
            format!("http://{}:{}/", local_ip, port.to_string().cyan().bold()).blue()
        )
    } else {
        format!(
            "{} {} {} {} {}",
            "•".green().dimmed(),
            "Network:".bold().dimmed(),
            "use".dimmed(),
            "--host".bold(),
            "to expose".dimmed()
        )
    };
    println!(
        "Server started in {}\n{}\n{}\n\n{}\n",
        shared::get_elapsed_time(server_start),
        localhost_address,
        lan_address,
        "Press Ctrl-D to stop the server".dimmed()
    );

    if open {
        match open::that_detached(format!("http://localhost:{}/", port)) {
            Ok(()) => {
                info!("Opening the development server page using your browser ...");
            }
            Err(e) => warn!(
                "{}: {}",
                "Could not open the development server page".bold(),
                e
            ),
        };
    }

    if let Err(e) = server.await {
        bail!("{}: {}", "Server error".bold(), e);
    }

    println!("\n{} Development server stopped.", "→".cyan().bold());
    Ok(())
}
