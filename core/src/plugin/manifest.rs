use std::path::Path;

use eyre::{bail, eyre, Result};
use serde::Deserialize;

/// Current ABI version that this norgolith core provides
pub const CORE_ABI_VERSION: u32 = 1;

/// Default hook timeout in milliseconds
const DEFAULT_TIMEOUT_MS: u64 = 10_000;

/// Parsed representation of a `plugin.toml` manifest
#[derive(Debug, Clone, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMetadata,
    pub hooks: HookConfig,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginMetadata {
    /// Name of the plugin (e.g. "my-plugin")
    pub name: String,
    /// Version of the plugin (e.g. "0.1.0")
    pub version: String,
    /// Semver requirement for norgolith compatibility (e.g. ">=0.4.0")
    pub norgolith: String,
    /// ABI version this plugin was compiled against
    pub abi: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HookConfig {
    #[serde(default)]
    pub pre_build: bool,
    #[serde(default)]
    pub post_convert: bool,
    #[serde(default)]
    pub post_render: bool,
    #[serde(default)]
    pub post_build: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Capabilities {
    #[serde(default)]
    pub filesystem: FilesystemAccess,
    #[serde(default)]
    pub network: bool,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FilesystemAccess {
    #[default]
    None,
    Read,
    Write,
    #[serde(rename = "read-write")]
    ReadWrite,
}

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

impl HookConfig {
    /// Returns a bitmask of declared hooks
    /// Bits: PRE_BUILD=1, POST_CONVERT=2, POST_RENDER=4, POST_BUILD=8
    pub fn to_mask(&self) -> u32 {
        let mut mask = 0u32;
        if self.pre_build {
            mask |= HOOK_PRE_BUILD;
        }
        if self.post_convert {
            mask |= HOOK_POST_CONVERT;
        }
        if self.post_render {
            mask |= HOOK_POST_RENDER;
        }
        if self.post_build {
            mask |= HOOK_POST_BUILD;
        }
        mask
    }

    pub fn declared_hooks(&self) -> Vec<&'static str> {
        let mut hooks = Vec::new();
        if self.pre_build {
            hooks.push("pre_build");
        }
        if self.post_convert {
            hooks.push("post_convert");
        }
        if self.post_render {
            hooks.push("post_render");
        }
        if self.post_build {
            hooks.push("post_build");
        }
        hooks
    }
}

/// Plugin needs to configure itself before any file processing
pub const HOOK_PRE_BUILD: u32 = 1;
/// Plugin needs to modify HTML after Norg->HTML conversion but before Tera templating
pub const HOOK_POST_CONVERT: u32 = 2;
/// Plugin needs to modify final HTML after Tera layout is applied
pub const HOOK_POST_RENDER: u32 = 4;
/// Plugin needs to generate additional output files after all pages are rendered
pub const HOOK_POST_BUILD: u32 = 8;

impl PluginManifest {
    /// Parse a `plugin.toml` file at the given path
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| eyre!("Failed to read {}: {}", path.display(), e))?;
        let manifest: PluginManifest = toml::from_str(&content)
            .map_err(|e| eyre!("Failed to parse {}: {}", path.display(), e))?;
        Ok(manifest)
    }

    /// Validate ABI compatibility. Returns Ok(()) if compatible
    pub fn validate_abi(&self) -> Result<()> {
        if self.plugin.abi != CORE_ABI_VERSION {
            bail!(
                "ABI mismatch: plugin '{}' requires abi={}, core provides abi={}",
                self.plugin.name,
                self.plugin.abi,
                CORE_ABI_VERSION
            );
        }
        Ok(())
    }

    /// Validate semver compatibility with the running norgolith version
    pub fn validate_semver(&self) -> Result<()> {
        let req = semver::VersionReq::parse(&self.plugin.norgolith)
            .map_err(|e| eyre!("Invalid semver requirement '{}': {}", self.plugin.norgolith, e))?;
        let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
            .map_err(|e| eyre!("Invalid core version: {}", e))?;
        if !req.matches(&current) {
            bail!(
                "Version mismatch: plugin '{}' requires norgolith {}, installed is {}",
                self.plugin.name,
                self.plugin.norgolith,
                current
            );
        }
        Ok(())
    }
}
