use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::cmd;
use crate::net;

#[derive(Parser)]
#[command(
    author = "NTBBloodbath",
    version,
    disable_version_flag = true,
    about = "The monolithic Norg static site generator"
)]
struct Cli {
    /// Print version
    #[arg(short = 'v', long, action = clap::builder::ArgAction::Version)]
    version: (),

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Clone)]
enum Commands {
    /// Initialize a new Norgolith site (WIP)
    Init {
        /// Site name
        name: Option<String>,
    },
    /// Build a site for development (WIP)
    Serve {
        #[arg(short = 'p', long, default_value_t = 3030)]
        port: u16,
    },
    /// Build a site for production (WIP)
    Build,
}

pub async fn start() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Init { name } => {
            if let Some(value) = name {
                cmd::init(value).await?;
            } else {
                eprintln!("Missing name for the site");
                std::process::exit(1);
            }
        }
        Commands::Serve { port } => {
            // If not using the default port and the requested port is busy
            if *port != 3030 && !net::is_port_available(*port) {
                eprintln!("The requested port ({}) is not available", port);
                std::process::exit(1);
            }

            // If the default Norgolith port is busy
            if !net::is_port_available(*port) {
                eprintln!(
                    "Failed to open listener, perhaps the port {} is busy?",
                    port
                );
                std::process::exit(1);
            }

            cmd::serve(*port).await?;
        }
        _ => {
            println!("TBD");
        }
    }

    Ok(())
}
