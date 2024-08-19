use std::convert::Infallible;

use eyre::{bail, Result};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use tera::{Context, Tera};

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

async fn handle_request(req: Request<Body>) -> Result<Response<Body>> {
    // HACK: we are supposed to compile templates only once, but it is nearly impossible to achieve that
    // because the one-time compilation has to happen during the norgolith compilation process and the
    // templates are generated during the runtime (and without taking into account user-made ones)
    let mut templates = match Tera::new("templates/**/*.html") {
        Ok(t) => t,
        Err(e) => bail!("Tera parsing error(s): {}", e),
    };

    let request_path = req.uri().path().to_owned();

    // FIXME: find a way to return an "error" log if the request path does not exist
    let (req_parts, _) = req.into_parts();
    // XXX: add headers here as well?
    println!("{:#?} - {} '{}'", req_parts.version, req_parts.method, req_parts.uri);

    if !request_path.contains('.') {
        let context = Context::new();
        // HACK: currently we are setting a custom hardcoded template during the runtime called 'current.html'
        // which extends site's 'base.html' template to be able to embed the norg->html document in the site.
        // Perhaps there is a better way to achieve this?
        let path_contents = get_content(&request_path).await?;
        let body = format!(
            r#"{{% extends "base.html" %}}
{{% block content %}}
{}
{{% endblock content %}}
"#,
            path_contents
        );
        templates.add_raw_template("current.html", &body)?;
        Ok(Response::new(Body::from(
            templates.render("current.html", &context)?,
        )))
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

    if let Some(_root) = found_site_root {
        // Create the server binding
        let make_svc =
            make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle_request)) });
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
