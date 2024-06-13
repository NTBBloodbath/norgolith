use std::convert::Infallible;

use eyre::{bail, Result};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
// use tokio::process::Command;

use crate::fs;

async fn handle_request(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    Ok(Response::new(Body::from("Hello, Norgolith!")))
}

// NOTE: we are going to replace pandoc with a native rust parser later on :)
// async fn convert_document() -> Result<()> {
//     let mut content_stream = fs::read_dir("content").await?;
//     while let Some(entry) = content_stream.next_entry().await? {
//         let file_path = entry.path();
//
//         if file_path.extension().unwrap_or_default() == "norg" {
//             println!("Processing norg file: {}", file_path.display());
//             let pandoc = Command::new("pandoc")
//                 .arg("--from=../norg-pandoc/init.lua")
//                 .arg(file_path)
//                 .arg("--output=content/index.html")
//                 .output().await.unwrap();
//             println!("{:?}", pandoc);
//         }
//     }
//
//     Ok(())
// }

pub async fn serve(port: u16) -> Result<()> {
    // Try to find a 'norgolith.toml' file in the current working directory and its parents
    let found_site_root = fs::find_file_in_previous_dirs("norgolith.toml").await?;

    if let Some(_root) = found_site_root {
        // Create the server binding
        let make_svc =
            make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle_request)) });
        let addr = ([127, 0, 0, 1], port).into();
        let server = Server::bind(&addr).serve(make_svc);

        // Convert the norg documents to html
        // convert_document().await?;

        println!("Serving site ...");
        println!("Web server is available at http://localhost:{:?}/", port);
        if let Err(err) = server.await {
            bail!("Server error: {}", err)
        }
    } else {
        bail!("Could not initialize the development server: not in a Norgolith site directory");
    }

    Ok(())
}
