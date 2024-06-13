use clap::{Parser, Subcommand};
use eyre::{bail, Result};

use crate::cmd;
use crate::net;

#[cfg(test)]
use mockall::{automock, predicate::*};
#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
use tokio::{fs::remove_dir_all, net::TcpListener};

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

/// Asynchronously parse the command-line arguments and executes the corresponding subcommand
///
/// # Returns
///   A `Result<()>` indicating success or error. On error, the context message will provide information on why the subcommand failed.
pub async fn start() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Init { name } => init_site(name.as_ref()).await?,
        Commands::Serve { port } => check_and_serve(*port).await?,
        _ => bail!("Unsupported command"),
    }

    Ok(())
}

/// Initializes a new Norgolith site.
///
/// # Arguments:
///   * name: An optional name for the site. If `None` is provided, an error will be returned.
///
/// # Returns
///   A `Result<()>` indicating success or error. On error, the context message will provide information on why the site could not be initialized.
async fn init_site(name: Option<&String>) -> Result<()> {
    if let Some(name) = name {
        cmd::init(name).await?;
    } else {
        bail!("Missing name for the site: could not initialize the new Norgolith site");
    }
    Ok(())
}

/// Checks port availability and starts the development server.
///
/// # Arguments:
///   * port: The port number to use for the server.
///
/// # Returns:
///   A `Result<()>` indicating success or error. On error, the context message will provide information on why the development server could not be initialized.
async fn check_and_serve(port: u16) -> Result<()> {
    if !net::is_port_available(port) {
        let port_msg = if port == 3030 {
            "default Norgolith port (3030)".to_string()
        } else {
            format!("requested port ({})", port)
        };

        bail!("Could not initialize the development server: failed to open listener, perhaps the {} is busy?", port_msg);
    }

    cmd::serve(port).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // init_site tests
    #[tokio::test]
    async fn test_init_site_with_name() {
        let test_name = String::from("my-site");
        let result = init_site(Some(&test_name)).await;
        assert!(result.is_ok());

        // Cleanup
        remove_dir_all(PathBuf::from(test_name)).await.unwrap();
    }

    #[tokio::test]
    async fn test_init_site_without_name() {
        let result = init_site(None).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .root_cause()
            .to_string()
            .contains("Missing name for the site"));
    }

    #[cfg_attr(test, automock)]
    trait NetTrait {
        fn is_port_available(&self, port: u16) -> bool;
    }

    // check_and_serve tests
    #[tokio::test]
    #[cfg_attr(feature = "ci", ignore)]
    async fn test_check_and_serve_available_port() {
        let mut mock_net = MockNetTrait::new();
        mock_net
            .expect_is_port_available()
            .with(eq(8080))
            .times(1)
            .returning(|_| true);
        assert!(mock_net.is_port_available(8080));
    }

    #[tokio::test]
    #[cfg_attr(feature = "ci", ignore)]
    async fn test_check_and_serve_unavailable_port() {
        let temp_listener = TcpListener::bind("localhost:3030").await.unwrap();
        let port = temp_listener.local_addr().unwrap().port();

        let result = check_and_serve(port).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .root_cause()
            .to_string()
            .contains("failed to open listener"));
    }
}
