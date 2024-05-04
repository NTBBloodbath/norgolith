use std::convert::Infallible;

use anyhow::{anyhow, Result};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};

async fn handle_request(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    Ok(Response::new(Body::from("Hello, Norgolith!")))
}

pub async fn serve(port: u16) -> Result<()> {
    let make_svc =
        make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle_request)) });
    let addr = ([127, 0, 0, 1], port).into();
    let server = Server::bind(&addr).serve(make_svc);

    println!("Serving site ...");
    println!("Web server is available at http://localhost:{:?}/", port);
    server
        .await
        .map_err(|err| anyhow!("Server error: {}", err))?;

    Ok(())
}
