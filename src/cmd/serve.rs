use std::convert::Infallible;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use eyre::{bail, eyre, Result};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use tera::{Context, Tera};
use tokio::sync::RwLock;

use crate::converter;
use crate::fs;

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

async fn handle_request(req: Request<Body>, tera: Arc<RwLock<Tera>>) -> Result<Response<Body>> {
    let request_path = req.uri().path().to_owned();

    // FIXME: find a way to return an "error" log if the request path does not exist
    let (req_parts, _) = req.into_parts();
    // XXX: add headers here as well?
    println!(
        "{:#?} - {} '{}'",
        req_parts.version, req_parts.method, req_parts.uri
    );

    if !request_path.contains('.') {
        let path_contents = get_content(&request_path).await?;
        let mut context = Context::new();
        context.insert("content", &path_contents);

        let tera = tera.read().await;
        match tera.render("base.html", &context) {
            Ok(rendered) => Ok(Response::new(Body::from(rendered))),
            Err(e) => {
                eprintln!("Template rendering error: {}", e);
                Ok(Response::new(Body::from("Internal Server Error")))
            }
        }
    } else {
        Ok(Response::new(Body::from("<h1>Hello, Norgolith</h1>")))
    }
}

async fn convert_document() -> Result<()> {
    let mut content_stream = tokio::fs::read_dir("content").await?;
    while let Some(entry) = content_stream.next_entry().await? {
        let file_path = entry.path();

        if file_path.extension().unwrap_or_default() == "norg" {
            println!("Processing norg file: {}", file_path.display());
            let norg_document = tokio::fs::read_to_string(file_path.clone()).await?;
            let html = converter::convert(norg_document);
            // FIXME: this will produce an unexpected output for nested content like 'content/foo-post/bar.norg'
            let file = file_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .replace("norg", "html");
            tokio::fs::write(".build/".to_owned() + &file, html).await?;
        }
    }

    Ok(())
}

pub async fn serve(port: u16, open: bool) -> Result<()> {
    // Try to find a 'norgolith.toml' file in the current working directory and its parents
    let found_site_root = fs::find_in_previous_dirs("file", "norgolith.toml").await?;

    if let Some(mut root) = found_site_root {
        // Remove the `/norgolith.toml` from the root path
        root.pop();
        // Tera wants a `dir: &str` parameter for some reason instead of asking for a `&Path` or `&PathBuf`...
        let templates_dir = root.into_os_string().into_string().unwrap() + "/templates";

        // Initialize Tera once
        let tera = match Tera::new(&(templates_dir.clone() + "/**/*.html")) {
            Ok(t) => t,
            Err(e) => bail!("Tera parsing error(s): {}", e),
        };
        let tera = Arc::new(RwLock::new(tera));

        // Create debouncer with 100ms delay, this should be enough to handle both the
        // (Neo)vim swap files and also the VSCode atomic saves
        let (tx, rx) = std::sync::mpsc::channel();
        let mut debouncer = new_debouncer(
            Duration::from_millis(100),
            None,
            move |result: DebounceEventResult| {
                tx.send(result).unwrap();
            },
        )
        .map_err(|e| eyre!("Watcher error: {}", e))?;

        debouncer
            .watch(Path::new(&templates_dir.clone()), RecursiveMode::Recursive)
            .map_err(|e| eyre!("Watcher error: {}", e))?;

        let tera_watcher = Arc::clone(&tera);
        std::thread::spawn(move || {
            for result in rx {
                match result {
                    DebounceEventResult::Ok(events) => {
                        let mut reload_needed = false;

                        // Analyze events using FileIdMap
                        for event in events {
                            // Filter events to only trigger reloading on meaningful changes
                            let is_template = event.paths.iter().any(|path| {
                                path.parent().unwrap().ends_with("templates")
                                    && path.extension().map(|ext| ext == "html").unwrap_or(false)
                            });

                            let is_content_change = matches!(
                                event.kind,
                                notify::EventKind::Create(_)
                                    | notify::EventKind::Remove(_)
                                    | notify::EventKind::Modify(notify::event::ModifyKind::Data(_))
                            );

                            if is_template && is_content_change {
                                println!(
                                    "Detected template change: {:?}",
                                    event.paths.first().unwrap().file_name()
                                );
                                reload_needed = true;
                            }
                        }

                        if reload_needed {
                            let mut tera = tera_watcher.blocking_write();
                            match tera.full_reload() {
                                Ok(_) => println!("Templates successfully reloaded"),
                                Err(e) => eprintln!("Failed to reload templates: {}", e),
                            }
                        }
                    }
                    DebounceEventResult::Err(errors) => {
                        eprintln!("Watcher errors: {:?}", errors);
                    }
                }
            }
        });

        // Create the server binding
        let make_svc = make_service_fn(move |_conn| {
            let tera = Arc::clone(&tera);
            async { Ok::<_, Infallible>(service_fn(move |req| handle_request(req, tera.clone()))) }
        });
        let addr = ([127, 0, 0, 1], port).into();
        let server = Server::bind(&addr).serve(make_svc);
        let uri = format!("http://localhost:{}/", port);

        // Convert the norg documents to html
        convert_document().await?;

        println!("Serving site ...");
        println!("Web server is available at {}", uri);
        if open {
            match open::that_detached(uri) {
                Ok(()) => println!("Opening the development server page using your browser ..."),
                Err(e) => bail!("Could not open the development server page: {}", e),
            }
        }
        if let Err(err) = server.await {
            bail!("Server error: {}", err)
        }
    } else {
        bail!("Could not initialize the development server: not in a Norgolith site directory");
    }

    Ok(())
}
