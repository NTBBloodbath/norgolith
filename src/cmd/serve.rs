use std::convert::Infallible;

use anyhow::{anyhow, Result};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use tokio::fs;
// use tokio::process::Command;

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
    // TODO(ntbbloodbath): better look recursively from parent directories for the norgolith configuration file
    let path_exists = fs::metadata("norgolith.toml").await.is_ok();

    if path_exists {
        eprintln!("Not in a norgolith site directory");
        std::process::exit(1);
    } else {
        // Create the server binding
        let make_svc =
            make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle_request)) });
        let addr = ([127, 0, 0, 1], port).into();
        let server = Server::bind(&addr).serve(make_svc);

        // Convert the norg documents to html
        // convert_document().await?;

        println!("Serving site ...");
        println!("Web server is available at http://localhost:{:?}/", port);
        server
            .await
            .map_err(|err| anyhow!("Server error: {}", err))?;
    }
    Ok(())
}
