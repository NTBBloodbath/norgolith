mod cli;
mod cmd;
mod config;
mod converter;
mod fs;
mod net;
mod schema;
mod shared;
mod tera_functions;
mod theme;

use eyre::Result;
use tracing_subscriber::{filter::EnvFilter, fmt::time::ChronoLocal, FmtSubscriber};

#[tokio::main]
async fn main() -> Result<()> {
    // XXX: junk to test the conversion tool, remove later
    //let norg_doc = tokio::fs::read_to_string("/home/amartin/notes/languages/elixir.norg").await?;
    //let norg_html = converter::convert(norg_doc.clone());
    //println!("Norg code:{}\n", norg_doc);
    //println!("HTML code:\n{}", norg_html);

    let logging_timer = ChronoLocal::new(String::from("%r %F"));
    let logging_env = EnvFilter::try_from_env("LITH_LOG")
        .or_else(|_| EnvFilter::try_new("info"))?;
    let subscriber = FmtSubscriber::builder()
        .with_target(false)
        .with_file(false)
        .with_ansi(true)
        .with_timer(logging_timer)
        .with_env_filter(logging_env)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    if let Err(e) = cli::start().await {
        tracing::error!("{}", e);
        std::process::exit(1);
    }

    Ok(())
}
