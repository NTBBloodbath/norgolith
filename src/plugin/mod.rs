#![allow(dead_code, unused_imports)]

pub mod ffi;
pub mod manifest;

pub use ffi::{FreeStringFn, PluginFn, PluginInfo};
pub use manifest::{
    Capabilities, FilesystemAccess, HookConfig, PluginManifest, CORE_ABI_VERSION,
    HOOK_POST_BUILD, HOOK_POST_CONVERT, HOOK_POST_RENDER, HOOK_PRE_BUILD,
};

/// Hooks a plugin can implement. Each is an optional C ABI function pointer
pub struct PluginHooks {
    pub pre_build: Option<PluginFn>,
    pub post_convert: Option<PluginFn>,
    pub post_render: Option<PluginFn>,
    pub post_build: Option<PluginFn>,
}

/// A loaded plugin instance.
pub struct PluginInstance {
    pub name: String,
    pub version: String,
    /// Keeps the `.so` loaded in memory. Dropping this unloads the library
    _lib: libloading::Library,
    pub hooks: PluginHooks,
    pub manifest: PluginManifest,
}

/// Manages loaded plugins and dispatches hook calls
pub struct PluginManager {
    plugins: Vec<PluginInstance>,
}

impl PluginManager {
    /// Create an empty plugin manager (no plugins loaded)
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Iterate over loaded plugins
    pub fn plugins(&self) -> impl Iterator<Item = &PluginInstance> {
        self.plugins.iter()
    }

    /// Number of loaded plugins
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Whether no plugins are loaded
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Check if any plugin declares a given hook bit
    pub fn has_hook(&self, hook_bit: u32) -> bool {
        self.plugins
            .iter()
            .any(|p| p.manifest.hooks.to_mask() & hook_bit != 0)
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}
