use std::path::{Path, PathBuf};

use clap::Subcommand;
use colored::Colorize;
use eyre::{bail, Result};

use crate::plugin::{self, PluginManifest, CORE_ABI_VERSION};

#[derive(Subcommand, Clone)]
pub enum PluginCommands {
    /// List installed plugins and their status
    List,
    /// Scaffold a new plugin project
    New {
        /// Plugin name (used for directory and crate name)
        name: String,
    },
    /// Build and install a plugin from a local path
    Install {
        /// Path to the plugin directory (containing Cargo.toml)
        path: PathBuf,
    },
    /// Remove an installed plugin
    Uninstall {
        /// Plugin name to remove
        name: String,
    },
}

pub fn handle(subcommand: &PluginCommands) -> Result<()> {
    match subcommand {
        PluginCommands::List => list_plugins(),
        PluginCommands::New { name } => new_plugin(name),
        PluginCommands::Install { path } => install_plugin(path),
        PluginCommands::Uninstall { name } => uninstall_plugin(name),
    }
}

fn list_plugins() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let mgr = plugin::PluginManager::load(&cwd);

    if mgr.is_empty() {
        println!("{}", "No plugins installed.".dimmed());
        println!(
            "Install one with {} or scaffold a new one with {}",
            "lith plugin install <path>".cyan(),
            "lith plugin new <name>".cyan()
        );
        return Ok(());
    }

    for p in mgr.plugins() {
        let hooks = p.manifest.hooks.declared_hooks();
        let hooks_str = if hooks.is_empty() {
            "none".dimmed().to_string()
        } else {
            hooks.join(", ")
        };

        let status = if hooks.is_empty() {
            "no hooks".yellow().to_string()
        } else {
            "ok".green().to_string()
        };

        let fs = match p.manifest.capabilities.filesystem {
            plugin::FilesystemAccess::None => "none".dimmed().to_string(),
            plugin::FilesystemAccess::Read => "read".to_string(),
            plugin::FilesystemAccess::Write => "write".to_string(),
            plugin::FilesystemAccess::ReadWrite => "read-write".to_string(),
        };
        let net = if p.manifest.capabilities.network {
            "yes".to_string()
        } else {
            "no".dimmed().to_string()
        };

        println!("{}", p.name.bold());
        println!(
            "   version:  {:<10}  hooks:      {}",
            p.version, hooks_str
        );
        println!(
            "   status:   {:<19}  priority:   {}",
            status, p.manifest.priority
        );
        println!(
            "   abi:      {:<10}  norgolith:  {}",
            p.manifest.plugin.abi, p.manifest.plugin.norgolith
        );
        println!(
            "   timeout:  {:<10}  fs:         {}     net: {}",
            format!("{}s", p.manifest.timeout_ms / 1000),
            fs,
            net
        );
    }

    println!("\n{}", format!("{} plugin(s) loaded", mgr.len()).bold());
    Ok(())
}

fn new_plugin(name: &str) -> Result<()> {
    validate_plugin_name(name)?;

    let cwd = std::env::current_dir()?;
    let plugins_dir = cwd.join("plugins").join(name);

    if plugins_dir.exists() {
        bail!("Plugin '{}' already exists at {}", name, plugins_dir.display());
    }

    std::fs::create_dir_all(plugins_dir.join("src"))?;

    // NOTE: I still need to handle the case where the plugin requires a dev norgolith version, e.g. (>=0.4.0-COMMIT_HASH)
    // For now, just use the current version of norgolith from Cargo.toml. I'll need to figure out if semver crate can
    // handle this case, or if I need to implement a custom version comparison for dev versions.
    const NORGOLITH_VERSION: &str = env!("CARGO_PKG_VERSION");

    // plugin.toml
    let manifest = format!(
        r#"[plugin]
name = "{name}"
version = "0.1.0"
norgolith = ">={NORGOLITH_VERSION}"
abi = {CORE_ABI_VERSION}

[hooks]
pre_build = false
post_convert = false
post_render = false
post_build = false

[capabilities]
filesystem = "none"
network = false

timeout_ms = 10000
priority = 100
"#
    );
    std::fs::write(plugins_dir.join("plugin.toml"), manifest)?;

    // Cargo.toml
    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
norgolith-plugin-sdk = "0.1"
"#
    );
    std::fs::write(plugins_dir.join("Cargo.toml"), cargo_toml)?;

    // src/lib.rs
    let lib_rs = format!(
        r#"use norgolith_plugin_sdk::*;

register_plugin!("{name}", "0.1.0")
    .on_post_render(|ctx| {{
        Ok(Some(ctx.html))
    }})
    .register();
"#
    );
    std::fs::write(plugins_dir.join("src").join("lib.rs"), lib_rs)?;

    println!(
        "Plugin '{}' created at {}",
        name.bold(),
        plugins_dir.display()
    );
    println!("\nNext steps:");
    println!("  1. cd plugins/{}", name);
    println!("  2. Implement your hooks in src/lib.rs");
    println!("  3. Build with `cargo build`");
    println!("  4. Test with `lith plugin install plugins/{}'", name);

    Ok(())
}

fn install_plugin(source_dir: &Path) -> Result<()> {
    if !source_dir.is_dir() {
        bail!("Not a directory: {}", source_dir.display());
    }

    let manifest_path = source_dir.join("plugin.toml");
    if !manifest_path.is_file() {
        bail!(
            "No plugin.toml found in {}",
            source_dir.display()
        );
    }

    let manifest = PluginManifest::load(&manifest_path)?;
    validate_plugin_name(&manifest.plugin.name)?;
    manifest.validate_abi()?;
    manifest.validate_semver()?;

    // Build the plugin
    println!("{}", "Building plugin...".dimmed());
    let status = std::process::Command::new("cargo")
        .arg("build")
        .arg("--release")
        .current_dir(source_dir)
        .status()?;

    if !status.success() {
        bail!("cargo build failed");
    }

    // Find the built library
    let target_dir = source_dir.join("target").join("release");
    let lib_name = plugin::library_filename(&manifest.plugin.name);
    let lib_path = target_dir.join(&lib_name);

    if !lib_path.is_file() {
        // Fallback: scan for any matching library
        let ext = plugin::library_extension();
        let found = std::fs::read_dir(&target_dir)
            .ok()
            .and_then(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .find(|e| {
                        e.path()
                            .extension()
                            .and_then(|s| s.to_str())
                            .map(|s| s == ext)
                            .unwrap_or(false)
                    })
                    .map(|e| e.path())
            });

        match found {
            Some(path) => {
                let cwd = std::env::current_dir()?;
                let dest_dir = cwd.join("plugins").join(&manifest.plugin.name);
                std::fs::create_dir_all(&dest_dir)?;
                std::fs::copy(&path, dest_dir.join(path.file_name().unwrap()))?;
                std::fs::copy(&manifest_path, dest_dir.join("plugin.toml"))?;
            }
            None => {
                bail!(
                    "Built library not found in {}",
                    target_dir.display()
                );
            }
        }
    } else {
        let cwd = std::env::current_dir()?;
        let dest_dir = cwd.join("plugins").join(&manifest.plugin.name);
        std::fs::create_dir_all(&dest_dir)?;
        std::fs::copy(&lib_path, dest_dir.join(&lib_name))?;
        std::fs::copy(&manifest_path, dest_dir.join("plugin.toml"))?;
    }

    println!(
        "Plugin '{}' v{} installed",
        manifest.plugin.name.bold(),
        manifest.plugin.version
    );
    Ok(())
}

fn validate_plugin_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Plugin name cannot be empty");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") || name.contains(':') {
        bail!("Invalid plugin name: '{}' (no path separators or '..' allowed)", name);
    }
    Ok(())
}

fn uninstall_plugin(name: &str) -> Result<()> {
    validate_plugin_name(name)?;

    let cwd = std::env::current_dir()?;
    let plugin_dir = cwd.join("plugins").join(name);

    if !plugin_dir.is_dir() {
        bail!("Plugin '{}' is not installed", name);
    }

    std::fs::remove_dir_all(&plugin_dir)?;
    println!("Plugin '{}' uninstalled", name.bold());
    Ok(())
}
