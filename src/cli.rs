use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author = "NTBBloodbath", version, disable_version_flag = true, about = "The monolithic Norg static site generator")]
struct Cli {
    /// Print version
    #[arg(short = 'v', long, action = clap::builder::ArgAction::Version)]
    version: (),

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Clone)]
enum Commands {
    /// Initialize a new Norgolith site
    Init {
        /// Site name
        name: Option<String>,
    },
    /// Build a site for development
    Serve {
        #[arg(short = 'p', long, default_value_t = 3030)]
        port: u16,
    },
    /// Build a site for production
    Build,
}

pub fn start() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Init { name } => {
            if name.is_none() {
                eprintln!("Missing name for the site");
            } else {
                println!("Creating {name:?} site ...");
            }
        },
        Commands::Serve { port } => {
            // NOTE: TBD
            println!("Serving site ...");
            println!("Web server is available at http://localhost:{:?}/", port);
        },
        _ => {
            println!("TBD");
        },
    }

    Ok(())
}
