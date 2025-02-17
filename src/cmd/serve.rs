use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use eyre::{bail, eyre, Result};
use futures_util::{SinkExt, StreamExt};
use hyper::service::{make_service_fn, service_fn};
use hyper::{
    header::{HeaderValue, CONTENT_TYPE},
    Body, Request, Response, Server, StatusCode,
};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use tera::{Context, Tera};
use tokio::sync::broadcast;
use tokio::{
    net::{TcpListener, TcpStream},
    runtime::Handle,
    sync::RwLock,
};
use tokio_tungstenite::accept_async;

use crate::{config, fs, shared};

// Global state for reloading
struct ServerState {
    reload_tx: broadcast::Sender<()>,
    tera: Arc<RwLock<Tera>>,
    config: config::SiteConfig,
    content_dir: PathBuf,
    assets_dir: PathBuf,
}

// https//github.com/livereload/livereload-js dist/livereload.min.js v4.0.2
const LIVE_RELOAD: &str = include_str!("../resources/assets/livereload.js");

async fn is_template_change(event: &notify::Event) -> Result<bool> {
    let mut parent_dir = event
        .paths
        .first()
        .unwrap()
        .parent()
        .as_mut()
        .unwrap()
        .to_path_buf();
    let is_template_dir = fs::find_in_previous_dirs("dir", "templates", &mut parent_dir)
        .await
        .is_ok();

    // Filter events to only trigger reloading on meaningful changes
    let is_template = event
        .paths
        .first()
        .unwrap()
        .extension()
        .map(|ext| ext == "html")
        .unwrap_or(false);

    let is_template_change = matches!(
        event.kind,
        notify::EventKind::Create(_)
            | notify::EventKind::Remove(_)
            | notify::EventKind::Modify(notify::event::ModifyKind::Data(_))
    );

    Ok(is_template_dir && is_template && is_template_change)
}

async fn is_content_change(event: &notify::Event) -> Result<bool> {
    let event_path = event.paths.first().as_mut().unwrap().to_path_buf();
    let mut parent_dir = event_path.parent().as_mut().unwrap().to_path_buf();
    let is_content_dir = fs::find_in_previous_dirs("dir", "content", &mut parent_dir)
        .await
        .is_ok();

    // Filter events to only trigger reloading on meaningful changes
    // NOTE: we do not check for the norg filetype here because content directory
    // can also hold assets like images, and we want to also trigger a reload when
    // an asset file is created, modified or removed.
    let is_content_change = matches!(
        event.kind,
        notify::EventKind::Create(_)
            | notify::EventKind::Remove(_)
            | notify::EventKind::Modify(notify::event::ModifyKind::Data(_))
    );

    Ok(is_content_dir && is_content_change)
}

async fn is_asset_change(event: &notify::Event) -> Result<bool> {
    let event_path = event.paths.first().unwrap();
    let mut parent_dir = event_path.parent().as_mut().unwrap().to_path_buf();
    let is_assets_dir = fs::find_in_previous_dirs("dir", "assets", &mut parent_dir)
        .await
        .is_ok();

    // Filter events to only trigger reloading on meaningful changes
    // NOTE: we do not check for any filetype here because assets directory
    // can hold assets like css, javascript, images, etc and we want to
    // trigger a reload when any asset file is created, modified or removed.
    let is_asset_change = matches!(
        event.kind,
        notify::EventKind::Create(_)
            | notify::EventKind::Remove(_)
            | notify::EventKind::Modify(notify::event::ModifyKind::Data(_))
    );

    Ok(is_assets_dir && is_asset_change)
}

async fn handle_asset(request_path: &str, assets_dir: &Path) -> Result<Response<Body>> {
    let asset_path = request_path.trim_start_matches("/assets/");
    let full_path = assets_dir.join(asset_path);

    match tokio::fs::read(&full_path).await {
        Ok(content) => {
            let mime_type = mime_guess::from_path(asset_path).first_or_octet_stream();

            Response::builder()
                .header(CONTENT_TYPE, mime_type.as_ref())
                .status(StatusCode::OK)
                .body(Body::from(content))
                .map_err(Into::into)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("[server] Asset not found: {}", asset_path);
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("404 Asset Not Found"))?)
        }
        Err(e) => {
            eprintln!("[server] Error reading asset: {}", e);
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("500 Internal Server Error"))?)
        }
    }
}

async fn handle_websocket(stream: TcpStream, reload_tx: broadcast::Sender<()>) {
    let mut ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("[server] WebSocket error: {}", e);
            return;
        }
    };

    let mut rx = reload_tx.subscribe();

    let _ = ws_stream
        .send(tokio_tungstenite::tungstenite::Message::Text(
            r#"{
        "command": "hello",
        "protocols": ["http://livereload.com/protocols/official-7"],
        "serverName": "norgolith"
    }"#
            .to_string()
            .into(),
        ))
        .await;

    loop {
        tokio::select! {
            _ = rx.recv() => {
                if let Err(e) = ws_stream.send(tokio_tungstenite::tungstenite::Message::Text(r#"{
                    "command": "reload",
                    "path": "/"
                }"#.to_string().into())).await {
                    eprintln!("WebSocket send error: {}", e);
                    break;
                }
            }
            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => break,
                    Some(Err(e)) => {
                        eprintln!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn handle_request(req: Request<Body>, state: Arc<ServerState>) -> Result<Response<Body>> {
    let request_path = req.uri().path().to_owned();

    // Handle assets first
    if request_path.starts_with("/assets/") {
        return handle_asset(&request_path, &state.assets_dir).await;
    }

    // Handle reload endpoint
    if request_path == "/livereload.js" {
        return Ok(Response::builder()
            .header(CONTENT_TYPE, "text/javascript")
            .status(StatusCode::OK)
            .body(LIVE_RELOAD.into())?);
    }

    // Helper function to handle content retrieval errors
    async fn get_content_or_error(request_path: &str) -> Result<(String, PathBuf)> {
        shared::get_content(request_path).await.map_err(|e| {
            if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
                if io_err.kind() == std::io::ErrorKind::NotFound {
                    eyre!("Path not found: {}", request_path)
                } else {
                    eyre!("Error reading '{}': {}", request_path, io_err)
                }
            } else {
                eyre!("Unexpected error for '{}': {}", request_path, e)
            }
        })
    }

    // Normalize path for content handling
    let normalized_path = if request_path.ends_with('/') {
        request_path.trim_end_matches('/').to_owned()
    } else {
        request_path.clone()
    };

    let response = if !normalized_path.contains('.') {
        // HTML content handling
        match get_content_or_error(&normalized_path).await {
            Ok((path_contents, html_path)) => {
                // Get metadata path, derive it from actual HTML file path
                let meta_path = html_path.with_extension("meta.toml");

                // Handle metadata loading with proper error fallback
                let metadata: toml::Value = match tokio::fs::read_to_string(meta_path.clone()).await
                {
                    Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
                        // Fallback to empty table on parse errors
                        eprintln!("[server] Failed to parse metadata: {}", e);
                        toml::Value::Table(toml::map::Map::new())
                    }),
                    Err(e) => {
                        // Fallback to empty table if file not found
                        eprintln!("[server] Metadata file not found: {}", e);
                        toml::Value::Table(toml::map::Map::new())
                    }
                };

                // Get the layout (template) to render the content, fallback to default if the metadata field was not found.
                let layout = metadata
                    .get("layout")
                    .unwrap_or(&toml::Value::from("default"))
                    .as_str()
                    .unwrap()
                    .to_owned();

                // Build template context
                let mut context = Context::new();
                context.insert("content", &path_contents);
                context.insert("config", &state.config);
                context.insert("metadata", &metadata);

                // Get the template to use for rendering
                let tera = state.tera.read().await;
                tera.render(&(layout + ".html"), &context)
                    .map(|body| {
                        Response::builder()
                            .header(CONTENT_TYPE, "text/html; charset=utf-8")
                            .status(StatusCode::OK)
                            .body(Body::from(body))
                            .unwrap()
                    })
                    .map_err(|e| eyre!("Template rendering error for '{}': {}", normalized_path, e))
            }
            Err(e) => Err(e),
        }
    } else {
        match get_content_or_error(&normalized_path).await {
            Ok((path_contents, asset_path)) => {
                // Static assets handling
                //
                // Set content type based on file extension
                let mime_type = mime_guess::from_path(asset_path).first_or_octet_stream();
                Ok(Response::builder()
                    .header(
                        CONTENT_TYPE,
                        HeaderValue::from_str(mime_type.as_ref())
                            .unwrap_or_else(|_| HeaderValue::from_static("text/plain")),
                    )
                    .status(StatusCode::OK)
                    .body(Body::from(path_contents))?)
            }
            Err(e) => Err(e),
        }
    };

    // Inject livereload script into HTML responses
    match response {
        Ok(mut response) => {
            if let Some(content_type) = response.headers().get(CONTENT_TYPE) {
                if content_type.to_str().unwrap() == "text/html; charset=utf-8" {
                    let body = hyper::body::to_bytes(response.body_mut()).await?;
                    let mut html = String::from_utf8(body.to_vec())?;

                    // Inject reload script before closing body tag, it does reload every second
                    if let Some(pos) = html.rfind("</body>") {
                        html.insert_str(
                            pos,
                            r#"<script src="/livereload.js?port=35729&amp;mindelay=10"></script>"#,
                        );
                    }
                    *response.body_mut() = Body::from(html);
                }
            }
            Ok(response)
        }
        Err(e) => {
            // Single error logging point
            eprintln!("[server] {}", e);
            if e.to_string().contains("Path not found") {
                // TODO: add a 404 template using Tera
                Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("404 Not Found"))?)
            } else {
                // TODO: add a 500 template using Tera
                Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("500 Internal Server Error"))?)
            }
        }
    }
}

pub async fn serve(port: u16, drafts: bool, open: bool) -> Result<()> {
    // Try to find a 'norgolith.toml' file in the current working directory and its parents
    let mut current_dir = std::env::current_dir()?;
    let found_site_root =
        fs::find_in_previous_dirs("file", "norgolith.toml", &mut current_dir).await?;

    if let Some(mut root) = found_site_root {
        let server_start = std::time::Instant::now();

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

        // Async runtime handle
        let rt = Handle::current();

        // Initialize Tera once
        let tera = Arc::new(RwLock::new(shared::init_tera(&templates_dir).await?));

        // Create reload channel
        let (reload_tx, _) = broadcast::channel(16);

        // Start WebSocket server for livereload
        let reload_tx_clone = reload_tx.clone();
        tokio::spawn(async move {
            let listener = TcpListener::bind("127.0.0.1:35729").await.unwrap();
            while let Ok((stream, _)) = listener.accept().await {
                let reload_tx = reload_tx_clone.clone();
                tokio::spawn(handle_websocket(stream, reload_tx));
            }
        });

        // Initialize server state
        let state = Arc::new(ServerState {
            reload_tx,
            tera,
            config: site_config,
            content_dir: content_dir.clone(),
            assets_dir: assets_dir.clone(),
        });
        let root_url = state.config.root_url.clone();

        // Create debouncer with 200ms delay, this should be enough to handle both the
        // (Neo)vim swap files and also the VSCode atomic saves
        let (debouncer_tx, mut debouncer_rx) = tokio::sync::mpsc::channel(16);
        let state_watcher = Arc::clone(&state);
        let mut debouncer = new_debouncer(
            Duration::from_millis(200),
            None,
            move |result: DebounceEventResult| {
                let tx = debouncer_tx.clone();
                rt.spawn(async move {
                    if let Err(e) = tx.send(result).await {
                        eprintln!("[server] Error sending debouncer result: {:?}", e);
                    }
                });
            },
        )?;

        // Set up watchers
        debouncer.watch(Path::new(&templates_dir.clone()), RecursiveMode::Recursive)?;
        debouncer.watch(Path::new(&content_dir.clone()), RecursiveMode::Recursive)?;
        debouncer.watch(Path::new(&assets_dir.clone()), RecursiveMode::Recursive)?;

        tokio::spawn(async move {
            while let Some(result) = debouncer_rx.recv().await {
                match result {
                    DebounceEventResult::Ok(events) => {
                        let mut reload_templates_needed = false;
                        let mut reload_assets_needed = false;
                        let mut rebuild_needed = false;
                        let mut rebuild_document_path = PathBuf::new();

                        for event in events {
                            if let notify::EventKind::Remove(_) = &event.kind {
                                // I hate duplicating code but it makes the borrow checker happy so who cares!
                                let file_path = event.paths.first().unwrap().clone();

                                // Spawn cleanup task with owned data
                                let state = Arc::clone(&state_watcher);
                                let content_dir = state_watcher.content_dir.clone();
                                tokio::task::spawn(async move {
                                    if let Ok(relative_path) = file_path.strip_prefix(content_dir) {
                                        if relative_path
                                            .extension()
                                            .map(|e| e == "norg")
                                            .unwrap_or(false)
                                        {
                                            // Create owned paths for async task
                                            let html_path = Path::new(".build")
                                                .join(relative_path)
                                                .with_extension("html");
                                            let meta_path = html_path.with_extension("meta.toml");

                                            let _ = tokio::fs::remove_file(&html_path).await;
                                            let _ = tokio::fs::remove_file(&meta_path).await;
                                            println!("[server] Removed build files for deleted content: {}", relative_path.display());

                                            let _ = state.reload_tx.send(());
                                        }
                                    }
                                });
                            }

                            let file_path = event.paths.first().unwrap();
                            let file_name = event
                                .paths
                                .first()
                                .unwrap()
                                .file_name()
                                .unwrap()
                                .to_str()
                                .unwrap();
                            if is_template_change(&event).await.unwrap_or(false) {
                                println!("[server] Detected template change: {}", file_name);
                                reload_templates_needed = true;
                            }

                            // We are excluding these fucking temp (Neo)vim backup files because they trigger
                            // stupid bugs that I'm not willing to debug anymore.
                            //
                            // TODO: also ignore swap files, my mental health will thank me later.
                            if !file_name.ends_with('~') {
                                if file_path.strip_prefix(&state_watcher.content_dir).is_ok()
                                    && is_content_change(&event).await.unwrap_or(false)
                                {
                                    println!("[server] Detected content change: {}", file_name);
                                    rebuild_needed = true;
                                    rebuild_document_path = file_path.to_owned();
                                }

                                if file_path.strip_prefix(&state_watcher.assets_dir).is_ok()
                                    && is_asset_change(&event).await.unwrap_or(false)
                                {
                                    println!("[server] Detected asset change: {}", file_name);
                                    reload_assets_needed = true;
                                }
                            }
                        }

                        if reload_assets_needed {
                            let _ = state_watcher.reload_tx.send(());
                        }

                        if reload_templates_needed {
                            let mut tera = state_watcher.tera.write().await;
                            match tera.full_reload() {
                                Ok(_) => {
                                    println!("[server] Templates successfully reloaded");
                                    let _ = state_watcher.reload_tx.send(());
                                }
                                Err(e) => eprintln!("[server] Failed to reload templates: {}", e),
                            }
                        }

                        if rebuild_needed {
                            let state = Arc::clone(&state_watcher);
                            let root_url = state.config.root_url.clone();
                            tokio::task::spawn(async move {
                                match shared::convert_document(
                                    &rebuild_document_path,
                                    &state.content_dir,
                                    drafts,
                                    &root_url,
                                )
                                .await
                                {
                                    Ok(_) => {
                                        let stripped_path = rebuild_document_path
                                            .strip_prefix(&state.content_dir)
                                            .unwrap();
                                        println!(
                                            "[server] Content successfully regenerated: {}",
                                            stripped_path.display()
                                        );
                                        let _ = state.reload_tx.send(());
                                    }
                                    Err(e) => eprintln!("[server] Content conversion error: {}", e),
                                }
                            });
                        }
                    }
                    DebounceEventResult::Err(errors) => {
                        eprintln!("[server] Watcher errors: {:?}", errors);
                    }
                }
            }
        });

        // Create the server binding
        let make_svc = make_service_fn(move |_conn| {
            let state = Arc::clone(&state);
            async move {
                Ok::<_, Infallible>(service_fn(move |req: Request<Body>| {
                    let start = std::time::Instant::now();
                    let method = req.method().clone();
                    let uri = req.uri().clone();
                    let state = state.clone();

                    async move {
                        let path = uri.path().to_owned();
                        let response = handle_request(req, state).await.unwrap();
                        let status = response.status();
                        let duration = start.elapsed();

                        if path != "/livereload.js" {
                            println!(
                                "[server] {} {} => {} {} in {:.1?}",
                                method,
                                path,
                                status.as_u16(),
                                status.canonical_reason().unwrap_or("Unknown"),
                                duration
                            );
                        }

                        Ok::<_, Infallible>(response)
                    }
                }))
            }
        });
        let addr = ([127, 0, 0, 1], port).into();
        let server = Server::bind(&addr).serve(make_svc);
        let uri = format!("http://localhost:{}/", port);

        // Convert the norg documents to html
        shared::convert_content(&content_dir, drafts, &root_url).await?;

        // Clean up orphaned files before starting server
        shared::cleanup_orphaned_build_files(&content_dir).await?;

        println!(
            "[server] Serving site... Done in {}",
            shared::get_elapsed_time(server_start)
        );
        println!("[server] Web server is available at {}", uri);
        if open {
            match open::that_detached(uri) {
                Ok(()) => {
                    println!("[server] Opening the development server page using your browser ...")
                }
                Err(e) => bail!("[server] Could not open the development server page: {}", e),
            }
        }
        if let Err(err) = server.await {
            bail!("[server] Server error: {}", err)
        }
    } else {
        bail!("[server] Could not initialize the development server: not in a Norgolith site directory");
    }

    Ok(())
}
