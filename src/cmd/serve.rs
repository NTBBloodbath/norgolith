use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use eyre::{bail, eyre, Result};
use hyper::service::{make_service_fn, service_fn};
use hyper::{
    header::{HeaderValue, CONTENT_TYPE},
    Body, Request, Response, Server,
};
use indoc::formatdoc;
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use tera::{Context, Tera};
use tokio::{
    runtime::Handle,
    sync::{watch, RwLock},
};

use crate::converter;
use crate::fs;

// Global state for reloading
struct ServerState {
    reload_tx: watch::Sender<bool>,
    tera: Arc<RwLock<Tera>>,
}

async fn get_content(name: &str) -> Result<String> {
    let contents: String = if name == "/" {
        // '/' is always the index, fast return it
        tokio::fs::read_to_string(".build/index.html").await?
    } else {
        let content_file = format!("{}{}{}", ".build/", &name[1..], ".html");
        tokio::fs::read_to_string(content_file).await?
    };
    Ok(contents)
}

/// Recursively converts all the norg files in the content directory
async fn convert_content() -> Result<()> {
    async fn process_entry(entry: tokio::fs::DirEntry) -> Result<()> {
        let path = entry.path();
        if path.is_dir() {
            // Process directory recursively
            let mut content_stream = tokio::fs::read_dir(&path).await?;
            while let Some(entry) = content_stream.next_entry().await? {
                Box::pin(process_entry(entry)).await?;
            }
        } else {
            convert_document(&path).await?;
        }
        Ok(())
    }

    let mut content_stream = tokio::fs::read_dir("content").await?;
    while let Some(entry) = content_stream.next_entry().await? {
        Box::pin(process_entry(entry)).await?;
    }

    Ok(())
}

async fn convert_document(file_path: &Path) -> Result<()> {
    if file_path.extension().unwrap_or_default() == "norg"
        && tokio::fs::try_exists(file_path).await?
    {
        let mut should_convert = true;

        // Preserve directory structure relative to content directory
        let relative_path = file_path.strip_prefix("content")?;
        let html_file_path = Path::new(".build")
            .join(relative_path)
            .with_extension("html");

        let norg_document = tokio::fs::read_to_string(file_path).await?;
        let norg_html = converter::convert(norg_document);

        // Check existing content only if file exists
        if tokio::fs::try_exists(&html_file_path).await? {
            let html_content = tokio::fs::read_to_string(&html_file_path).await?;
            should_convert = norg_html != html_content;
        }

        if should_convert {
            println!("[server] Converting norg file: {}", file_path.display());

            // Create parent directories if needed
            if let Some(parent) = html_file_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            tokio::fs::write(&html_file_path, norg_html).await?;
        }
    }

    Ok(())
}

fn is_template_change(event: &notify::Event) -> Result<bool> {
    // Filter events to only trigger reloading on meaningful changes
    let is_template = event.paths.iter().any(|path| {
        path.parent().unwrap().ends_with("templates")
            && path.extension().map(|ext| ext == "html").unwrap_or(false)
    });

    let is_template_change = matches!(
        event.kind,
        notify::EventKind::Create(_)
            | notify::EventKind::Remove(_)
            | notify::EventKind::Modify(notify::event::ModifyKind::Data(_))
    );

    Ok(is_template && is_template_change)
}

fn is_content_change(event: &notify::Event) -> Result<bool> {
    // Filter events to only trigger reloading on meaningful changes
    let is_content_file = event.paths.iter().any(|path| {
        // NOTE: we do not check for the norg filetype here because content directory
        // can also hold assets like images, and we want to also trigger a reload when
        // an asset file is created, modified or removed
        path.parent().unwrap().ends_with("content")
    });

    let is_content_change = matches!(
        event.kind,
        notify::EventKind::Create(_)
            | notify::EventKind::Remove(_)
            | notify::EventKind::Modify(notify::event::ModifyKind::Data(_))
    );

    Ok(is_content_file && is_content_change)
}

async fn handle_request(req: Request<Body>, state: Arc<ServerState>) -> Result<Response<Body>> {
    let request_path = req.uri().path().to_owned();

    if request_path == "/_norgolith_reload" {
        let mut reload_rx = state.reload_tx.subscribe();
        reload_rx.changed().await?;
        return Ok(Response::new(Body::from("reload")));
    }

    // FIXME: find a way to return an "error" log if the request path does not exist
    let (req_parts, _) = req.into_parts();
    // XXX: add headers here as well?
    println!(
        "[server] {:#?} - {} '{}'",
        req_parts.version, req_parts.method, req_parts.uri
    );

    let mut response = if !request_path.contains('.') {
        // HTML content handling
        let mut context = Context::new();
        let path_contents = get_content(&request_path).await?;
        context.insert("content", &path_contents);
        // TODO: convert the template title into a variable and add it to the context

        let tera = state.tera.read().await;
        let body = tera
            .render("base.html", &context)
            .map_err(|e| eyre!("[server] Template rendering error: {}", e))?;

        // Create response with proper headers
        let mut response = Response::new(Body::from(body));
        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        Ok(response)
    } else {
        // Static assets handling
        let path_contents = get_content(&request_path).await?;
        let mut response = Response::new(Body::from(path_contents));

        // Set content type based on file extension
        // XXX: replace with the mimetype crate that does the job for us
        let mime_type = match request_path.split('.').last() {
            Some("css") => "text/css",
            Some("js") => "application/javascript",
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("svg") => "image/svg+xml",
            _ => "text/plain",
        };

        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_str(mime_type)
                .unwrap_or_else(|_| HeaderValue::from_static("text/plain")),
        );
        Ok(response)
    };

    // Inject reload script into HTML responses
    if let Ok(ref mut response) = response {
        if let Some(content_type) = response.headers().get(CONTENT_TYPE) {
            if content_type.to_str().unwrap() == "text/html; charset=utf-8" {
                let body = hyper::body::to_bytes(response.body_mut()).await?;
                let mut html = String::from_utf8(body.to_vec())?;

                // Inject reload script before closing body tag, it does reload every second
                let reload_script = formatdoc!(
                    r#"
                    <script>
                        (function() {{
                            function checkReload() {{
                                fetch('/_norgolith_reload')
                                    .then(r => r.text())
                                    .then(t => {{
                                        if(t === 'reload') location.reload();
                                        else setTimeout(checkReload, 1000);
                                    }})
                                    .catch(() => setTimeout(checkReload, 1000));
                            }}
                            checkReload();
                        }})();
                    </script>
                "#
                );

                if let Some(pos) = html.rfind("</body>") {
                    html.insert_str(pos, &reload_script);
                }
                *response.body_mut() = Body::from(html);
            }
        }
    }

    response
}

pub async fn serve(port: u16, open: bool) -> Result<()> {
    // Try to find a 'norgolith.toml' file in the current working directory and its parents
    let found_site_root = fs::find_in_previous_dirs("file", "norgolith.toml").await?;

    if let Some(mut root) = found_site_root {
        // Remove the `/norgolith.toml` from the root path
        root.pop();
        let root_dir = root.into_os_string().into_string().unwrap();

        // Tera wants a `dir: &str` parameter for some reason instead of asking for a `&Path` or `&PathBuf`...
        let templates_dir = root_dir.clone() + "/templates";
        let content_dir = root_dir.clone() + "/content";

        // Async runtime handle
        let rt = Handle::current();

        // Initialize Tera once
        let tera = match Tera::new(&(templates_dir.clone() + "/**/*.html")) {
            Ok(t) => t,
            Err(e) => bail!("[server] Tera parsing error(s): {}", e),
        };
        let tera = Arc::new(RwLock::new(tera));

        // Create reload channel
        let (reload_tx, _) = watch::channel(false);

        // Initialize server state
        let state = Arc::new(ServerState { reload_tx, tera });

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

        tokio::spawn(async move {
            while let Some(result) = debouncer_rx.recv().await {
                match result {
                    DebounceEventResult::Ok(events) => {
                        let mut reload_needed = false;
                        let mut rebuild_needed = false;
                        let mut rebuild_document_path = PathBuf::new();

                        for event in events {
                            if is_template_change(&event).unwrap_or(false) {
                                println!(
                                    "[server] Detected template change: {}",
                                    event
                                        .paths
                                        .first()
                                        .unwrap()
                                        .file_name()
                                        .unwrap()
                                        .to_str()
                                        .unwrap()
                                );
                                reload_needed = true;
                            }

                            if is_content_change(&event).unwrap_or(false) {
                                let file_path = event.paths.first().unwrap();
                                println!(
                                    "[server] Detected content change: {}",
                                    file_path.file_name().unwrap().to_str().unwrap()
                                );
                                rebuild_needed = true;
                                rebuild_document_path = file_path.to_owned();
                            }
                        }

                        if reload_needed {
                            let mut tera = state_watcher.tera.write().await;
                            match tera.full_reload() {
                                Ok(_) => {
                                    println!("[server] Templates successfully reloaded");
                                    let _ = state_watcher.reload_tx.send(true);
                                    // Reset
                                    let _ = state_watcher.reload_tx.send(false);
                                }
                                Err(e) => eprintln!("[server] Failed to reload templates: {}", e),
                            }
                        }

                        if rebuild_needed {
                            let state = Arc::clone(&state_watcher);
                            tokio::task::spawn(async move {
                                match convert_document(&rebuild_document_path).await {
                                    Ok(_) => {
                                        println!("[server] Content successfully regenerated");
                                        let _ = state.reload_tx.send(true);
                                        // Reset
                                        let _ = state.reload_tx.send(false);
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
            async { Ok::<_, Infallible>(service_fn(move |req| handle_request(req, state.clone()))) }
        });
        let addr = ([127, 0, 0, 1], port).into();
        let server = Server::bind(&addr).serve(make_svc);
        let uri = format!("http://localhost:{}/", port);

        // Convert the norg documents to html
        convert_content().await?;

        println!("[server] Serving site ...");
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
