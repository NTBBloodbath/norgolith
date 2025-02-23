mod cli;
mod cmd;
mod config;
mod converter;
mod fs;
mod net;
mod theme;
mod schema;
mod shared;
mod tera_functions;

use eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // XXX: junk to test the conversion tool, remove later
    //let norg_doc = tokio::fs::read_to_string("/home/amartin/notes/languages/elixir.norg").await?;
    //let norg_html = converter::convert(norg_doc.clone());
    //println!("Norg code:{}\n", norg_doc);
    //println!("HTML code:\n{}", norg_html);

    if let Err(e) = cli::start().await {
        eprintln!("Something went wrong:\n{:?}", e);
        std::process::exit(1);
    }

    Ok(())
}
