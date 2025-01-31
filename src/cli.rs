use std::env::set_current_dir;
use std::path::PathBuf;

use clap::{builder::PossibleValue, Parser, Subcommand};
use eyre::{bail, Result};

use crate::cmd;
use crate::net;

#[cfg(test)]
use mockall::{automock, predicate::*};
#[cfg(test)]
use tokio::{fs::canonicalize, fs::remove_dir_all};

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

    /// Operate on the project in the given directory.
    #[arg(short = 'd', long = "dir", global = true)]
    project_dir: Option<PathBuf>,

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
        #[arg(short = 'p', long, default_value_t = 3030, help = "Port to be used")]
        port: u16,

        #[arg(
            short = 'o',
            long,
            default_value_t = false,
            help = "Open the development server in your browser"
        )]
        open: bool,
    },
    /// Create a new asset in the site and optionally open it using your preferred system editor.
    /// e.g. 'new -k content post1.norg' -> 'content/post1.norg'
    New {
        #[arg(
            short = 'o',
            long,
            default_value_t = false,
            help = "Open the new file using your preferred system editor"
        )]
        open: bool,

        #[arg(
            short = 'k',
            long,
            default_value = "content",
            help = "type of asset",
            value_parser = [
                PossibleValue::new("content").help("New norg file"),
                PossibleValue::new("css").help("New CSS stylesheet"),
                PossibleValue::new("js").help("New JS script")
            ]
        )]
        kind: Option<String>,

        /// Asset name, e.g. 'post1.norg' or 'hello.js'
        name: Option<String>,
    },
    /// Build a site for production
    Build,
}

/// Asynchronously parse the command-line arguments and executes the corresponding subcommand
///
/// # Returns:
///   A `Result<()>` indicating success or error. On error, the context message will provide information on why the subcommand failed.
pub async fn start() -> Result<()> {
    let cli = Cli::parse();

    if let Some(dir) = cli.project_dir {
        set_current_dir(dir)?;
    }

    match &cli.command {
        Commands::Init { name } => init_site(name.as_ref()).await?,
        Commands::Serve { port, open } => check_and_serve(*port, *open).await?,
        Commands::New { kind, name, open } => {
            new_asset(kind.as_ref(), name.as_ref(), *open).await?
        }
        _ => bail!("Unsupported command"),
    }

    Ok(())
}

/// Initializes a new Norgolith site.
///
/// # Arguments:
///   * name: An optional name for the site. If `None` is provided, an error will be returned.
///
/// # Returns:
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
///   * open: Whether to open the development server in the system web browser.
///
/// # Returns:
///   A `Result<()>` indicating success or error. On error, the context message will provide information on why the development server could not be initialized.
async fn check_and_serve(port: u16, open: bool) -> Result<()> {
    if !net::is_port_available(port) {
        let port_msg = if port == 3030 {
            "default Norgolith port (3030)".to_string()
        } else {
            format!("requested port ({})", port)
        };

        bail!("Could not initialize the development server: failed to open listener, perhaps the {} is busy?", port_msg);
    }

    cmd::serve(port, open).await?;
    Ok(())
}

/// Creates a new asset with the given kind and name.
///
/// # Arguments
///
/// * `kind`: Optional asset type. Defaults to "content". Valid values are "content", "css", and "js".
/// * `name`: Required asset name including the extension.
///
/// # Errors
///
/// Returns an error if the asset name is missing.
///
/// # Example
///
/// ```rust
/// use crate::new_asset;
///
/// async fn main() -> Result<()> {
///     new_asset(Some(&String::from("css")), Some(&String::from("style.css"))).await?;
/// Ok(())
/// }
/// ```
async fn new_asset(kind: Option<&String>, name: Option<&String>, open: bool) -> Result<()> {
    let asset_type = kind.unwrap_or(&String::from("content")).to_owned();

    if ![
        String::from("js"),
        String::from("css"),
        String::from("content"),
    ]
    .contains(&asset_type)
    {
        bail!("Unable to create site asset: unknown asset type provided");
    }

    match name {
        Some(name) => cmd::new(&asset_type, name, open).await?,
        None => bail!("Unable to create site asset: missing name for the asset"),
    }
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
        remove_dir_all(test_name).await.unwrap();
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
    async fn test_check_and_serve_unavailable_port() -> Result<()> {
        // Bind port
        let temp_listener = std::net::TcpListener::bind(("127.0.0.1", 3030))?;
        let port = temp_listener.local_addr()?.port();

        // Create temporal site
        let test_site_dir = String::from("my-unavailable-site");
        init_site(Some(&test_site_dir)).await?;

        // Save current directory as the previous directory to restore it later
        let previous_dir = std::env::current_dir()?;

        // Enter the test directory
        std::env::set_current_dir(canonicalize(test_site_dir.clone()).await?)?;

        let result = check_and_serve(port, false).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .root_cause()
            .to_string()
            .contains("failed to open listener"));

        // Restore previous directory
        std::env::set_current_dir(previous_dir)?;

        // Cleanup test directory and test file
        remove_dir_all(test_site_dir).await?;

        Ok(())
    }
}
