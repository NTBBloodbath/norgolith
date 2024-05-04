mod cli;
mod cmd;
mod net;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    match cli::start().await {
        Err(e) => eprintln!("Something went wrong while parsing command-line: {:?}", e),
        _ => (),
    }

    Ok(())
}
