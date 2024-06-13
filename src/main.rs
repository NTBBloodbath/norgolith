mod cli;
mod cmd;
mod fs;
mod net;

use eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(e) = cli::start().await {
        eprintln!("Something went wrong: {:?}", e);
        std::process::exit(1);
    }

    Ok(())
}
