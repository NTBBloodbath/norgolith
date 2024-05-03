mod cli;

use anyhow::Result;

fn main() -> Result<()> {
    cli::start()?;

    Ok(())
}
