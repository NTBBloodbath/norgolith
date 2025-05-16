use std::env::set_current_dir;
use std::path::PathBuf;

use clap::{builder::PossibleValue, Parser, Subcommand};
use eyre::{bail, Result};

use crate::cmd;
use crate::net;

#[cfg(test)]
use mockall::{automock, predicate::*};

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
        #[arg(
            long,
            default_value_t = true,
            overrides_with = "_no_prompt",
            help = "Whether to prompt for site info"
        )]
        prompt: bool,

        #[arg(long = "no-prompt")]
        _no_prompt: bool,

        /// Site name
        name: String,
    },
    /// Theme management
    Theme {
        #[command(subcommand)]
        subcommand: cmd::ThemeCommands,
    },
    /// Run a site in development mode
    Dev {
        #[arg(short = 'p', long, default_value_t = 3030, help = "Port to be used")]
        port: u16,

        #[arg(
            long,
            default_value_t = true,
            overrides_with = "_no_drafts",
            help = "Whether to serve draft content"
        )]
        drafts: bool,

        #[arg(long = "no-drafts")]
        _no_drafts: bool,

        // TODO: add SocketAddr parsing if host is a String, similar to Vite
        #[arg(
            short = 'e',
            long,
            default_value_t = false,
            help = "Expose site to LAN network"
        )]
        host: bool,

        #[arg(
            short = 'o',
            long,
            default_value_t = false,
            help = "Open the development server in your browser"
        )]
        open: bool,
    },
    /// Create a new asset in the site and optionally open it using your preferred system editor.
    /// e.g. 'new -k norg post1.norg' -> 'content/post1.norg'
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
            default_value = "norg",
            help = "type of asset",
            value_parser = [
                PossibleValue::new("norg").help("New norg file"),
                PossibleValue::new("css").help("New CSS stylesheet"),
                PossibleValue::new("js").help("New JS script")
            ]
        )]
        kind: Option<String>,

        /// Asset name, e.g. 'post1.norg' or 'hello.js'
        name: Option<String>,
    },
    /// Build a site for production
    Build {
        #[arg(
            short = 'm',
            long,
            default_value_t = true,
            overrides_with = "_no_minify",
            help = "Minify the produced assets"
        )]
        minify: bool,

        #[arg(long = "no-minify")]
        _no_minify: bool,
    },
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

    match cli.command {
        Commands::Init {
            name,
            prompt: _,
            _no_prompt,
        } => init_site(name, !_no_prompt).await?,
        Commands::Theme { subcommand } => theme_handle(&subcommand).await?,
        Commands::Dev {
            port,
            drafts: _,
            _no_drafts,
            host,
            open,
        } => check_and_serve(port, !_no_drafts, open, host).await?,
        Commands::Build {
            minify: _,
            _no_minify,
        } => build_site(!_no_minify).await?,
        Commands::New { kind, name, open } => new_asset(kind.as_ref(), name.as_ref(), open).await?,
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
async fn init_site(name: String, prompt: bool) -> Result<()> {
    cmd::init(&name, prompt).await?;
    // if let Some(name) = name {
    // } else {
    //     bail!("Missing name for the site: could not initialize the new Norgolith site");
    // }
    Ok(())
}

/// Builds a Norgolith site for production.
///
/// # Arguments:
///   * minify: Whether to minify the produced artifacts. Defaults to `true`.
///
/// # Returns:
///   A `Result<()>` indicating success or error.
async fn build_site(minify: bool) -> Result<()> {
    let build_config = match crate::fs::find_config_file().await? {
        Some(config_path) => {
            let config_content = tokio::fs::read_to_string(config_path).await?;
            toml::from_str(&config_content)?
        }
        None => crate::config::SiteConfig::default(),
    }
    .build
    .unwrap_or_default();

    // Merge CLI and config values
    // CLI options have higher priority than config
    // config has higher priority than defaults
    let minify = minify || build_config.minify;
    cmd::build(minify).await
}

/// Checks port availability and starts the development server.
///
/// # Arguments:
///   * port: The port number to use for the server.
///   * drafts: Whether to serve draft content.
///   * open: Whether to open the development server in the system web browser.
///   * host: Whether to expose local server to LAN network.
///
/// # Returns:
///   A `Result<()>` indicating success or error. On error, the context message
///   will provide information on why the development server could not be initialized.
async fn check_and_serve(port: u16, drafts: bool, open: bool, host: bool) -> Result<()> {
    let serve_config = match crate::fs::find_config_file().await? {
        Some(config_path) => {
            let config_content = tokio::fs::read_to_string(config_path).await?;
            toml::from_str(&config_content)?
        }
        None => crate::config::SiteConfig::default(),
    }
    .serve
    .unwrap_or_default();

    // Merge CLI and config values
    // CLI options have higher priority than config
    // config has higher priority than defaults
    let port = if port != 3030 {
        port
    } else if serve_config.port == 0 {
        3030
    } else {
        serve_config.port
    };
    let drafts = drafts || serve_config.drafts;
    let host = host || serve_config.host;
    let open = open || serve_config.open;

    if !net::is_port_available(port) {
        let port_msg = if port == 3030 {
            "default Norgolith port (3030)".to_string()
        } else {
            format!("requested port ({})", port)
        };

        bail!("Could not initialize the development server: failed to open listener, perhaps the {} is busy?", port_msg);
    }

    cmd::dev(port, drafts, open, host).await
}

async fn theme_handle(subcommand: &cmd::ThemeCommands) -> Result<()> {
    cmd::theme(subcommand).await
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
        String::from("norg"),
    ]
    .contains(&asset_type)
    {
        bail!("Unable to create site asset: unknown asset type provided");
    }

    match name {
        Some(name) => cmd::new(&asset_type, name, open).await,
        None => bail!("Unable to create site asset: missing name for the asset"),
    }
}

#[cfg(test)]
mod tests {
    use serial_test::serial;
    use tempfile::tempdir;

    use super::*;

    // init_site tests
    #[tokio::test]
    #[serial]
    async fn test_init_site_with_name() -> Result<()> {
        let dir = tempdir()?;

        let origin = std::env::current_dir()?;
        std::env::set_current_dir(dir.path())?;

        let test_name = String::from("my-site");
        let result = init_site(test_name, false).await;
        assert!(result.is_ok());

        std::env::set_current_dir(origin)?;

        Ok(())
    }

    #[cfg_attr(test, automock)]
    trait NetTrait {
        fn is_port_available(&self, port: u16) -> bool;
    }

    // check_and_serve tests
    #[tokio::test]
    #[cfg_attr(feature = "ci", ignore)]
    #[serial]
    async fn test_is_port_available() {
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
    #[serial]
    async fn test_check_and_serve() -> Result<()> {
        let dir = tempdir()?;

        let origin = std::env::current_dir()?;
        std::env::set_current_dir(dir.path())?;

        // Bind port
        let temp_listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = temp_listener.local_addr()?.port();

        // Create temporal site
        let test_site_name = String::from("my-unavailable-site");
        init_site(test_site_name.clone(), false).await.unwrap();

        // Enter the test directory
        let path = dir.path().join(&test_site_name);

        std::env::set_current_dir(path)?;

        let result = check_and_serve(port, false, false, false).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .root_cause()
            .to_string()
            .contains("failed to open listener"));

        // Restore previous directory
        std::env::set_current_dir(origin)?;

        Ok(())
    }
}
