use std::{convert::Infallible, path::{Path, PathBuf}};

use colored::Colorize as _;
use eyre::{bail, Result};
use hyper::{
    header::CONTENT_TYPE,
    service::{make_service_fn, service_fn},
    Body, Request, Response, Server, StatusCode,
};
use tracing::{debug, info};

use crate::fs;

async fn handle_request(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let request_path = req.uri().path();
    debug!(path = %request_path, "Handling request");
    let mut file_path = sanitize_path(request_path);
    debug!(?file_path);
    if file_path.is_dir() {
        file_path.push("index.html")
    }
    debug!(?file_path);
    let Ok(content) = tokio::fs::read(&file_path).await else {
        return Ok(handle_not_found());
    };
    let mime_type = mime_guess::from_path(&file_path).first_or_octet_stream();
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, mime_type.as_ref())
        .body(Body::from(content))
        .unwrap())
}

fn handle_not_found() -> Response<Body> {
    // TODO: try load 404.html
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("not found"))
        .expect("Could not build Not Found response")
}

fn sanitize_path(uri_path: &str) -> PathBuf {
    // TODO: decode percent signs (url-encoded path)
    let rel_path = uri_path.trim_start_matches('/');
    let mut base = PathBuf::from("./public");
    for comp in Path::new(rel_path) {
        if comp == ".." {
            continue
        }
        base.push(comp);
    }
    base
}

pub async fn preview(port: u16, open: bool, host: bool) -> Result<()> {
    info!("Starting preview server...");

    let root = fs::find_config_file().await?;
    let Some(root) = root else {
        bail!(
            "{}: not in a Norgolith site directory",
            "Could not initialize the development server".bold()
        );
    };

    debug!(path = %root.display(), "Found site root");

    let addr = if host {
        ([0, 0, 0, 0], port).into()
    } else {
        ([127, 0, 0, 1], port).into()
    };
    let make_svc = make_service_fn(|_| async { Ok::<_, Infallible>(service_fn(handle_request)) });
    let server = Server::bind(&addr).serve(make_svc);

    if open {
        match open::that_detached(format!("http://localhost:{}/", port)) {
            Ok(()) => info!("Opening the preview page using your browser ..."),
            Err(e) => bail!("{}: {}", "Could not open the preview page".bold(), e),
        };
    }

    if let Err(e) = server.await {
        bail!("{}: {}", "Server error".bold(), e);
    }

    Ok(())
}
