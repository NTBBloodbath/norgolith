#![allow(dead_code, unused_imports)]

pub mod ffi;
pub mod manifest;
pub mod sandbox;

use std::path::{Path, PathBuf};
use std::time::Duration;

pub use ffi::{FreeStringFn, PluginFn, PluginInfo};
pub use manifest::{
    Capabilities, FilesystemAccess, HookConfig, PluginManifest, CORE_ABI_VERSION,
    HOOK_POST_BUILD, HOOK_POST_CONVERT, HOOK_POST_RENDER, HOOK_PRE_BUILD,
};

use tracing::{info, warn};

/// Hooks a plugin can implement. Each is an optional C ABI function pointer
pub struct PluginHooks {
    pub pre_build: Option<PluginFn>,
    pub post_convert: Option<PluginFn>,
    pub post_render: Option<PluginFn>,
    pub post_build: Option<PluginFn>,
}

/// A loaded plugin instance
pub struct PluginInstance {
    pub name: String,
    pub version: String,
    /// Keeps the `.so` loaded in memory. Dropping this unloads the library
    _lib: libloading::Library,
    pub hooks: PluginHooks,
    pub manifest: PluginManifest,
    /// Function to free strings allocated by this plugin (defaults to libc::free)
    pub free_string: FreeStringFn,
}

impl PluginInstance {
    /// Call a hook on this plugin with safety wrappers (catch_unwind + timeout)
    pub fn call_hook(&self, f: PluginFn, input: &str) -> Option<String> {
        let timeout = Duration::from_millis(self.manifest.timeout_ms);
        match ffi::call_hook_safe(f, input, timeout) {
            Ok(Some(json)) => match ffi::parse_hook_response(&json) {
                Ok(Some(html)) => Some(html),
                Ok(None) => None,
                Err(e) => {
                    warn!("Plugin '{}' returned invalid response: {}", self.name, e);
                    None
                }
            },
            Ok(None) => None,
            Err(e) => {
                warn!("Plugin '{}' hook failed: {}", self.name, e);
                None
            }
        }
    }
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

    /// Scan `plugins/` under `site_dir` and load all valid plugins
    pub fn load(site_dir: &Path) -> Self {
        let mut manager = Self::new();
        let plugins_dir = site_dir.join("plugins");

        if !plugins_dir.is_dir() {
            return manager;
        }

        let entries = match std::fs::read_dir(&plugins_dir) {
            Ok(e) => e,
            Err(e) => {
                warn!("Failed to read plugins directory: {}", e);
                return manager;
            }
        };

        for entry in entries.filter_map(|e| e.ok()) {
            if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                continue;
            }
            let dir = entry.path();
            match load_plugin(&dir) {
                Ok(instance) => {
                    info!(
                        "Loaded plugin '{}' v{}",
                        instance.name, instance.version
                    );
                    manager.plugins.push(instance);
                }
                Err(e) => {
                    warn!(
                        "Plugin '{}' skipped: {}",
                        dir.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("?"),
                        e
                    );
                }
            }
        }

        manager
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

pub fn library_extension() -> &'static str {
    #[cfg(target_os = "linux")]
    { "so" }
    #[cfg(target_os = "macos")]
    { "dylib" }
    #[cfg(target_os = "windows")]
    { "dll" }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    { "so" }
}

pub fn library_filename(name: &str) -> String {
    // Linux/macOS convention: lib<name>.<ext>
    // Windows convention: <name>.dll
    #[cfg(target_os = "windows")]
    { format!("{}.{}", name, library_extension()) }
    #[cfg(not(target_os = "windows"))]
    { format!("lib{}.{}", name, library_extension()) }
}

/// Find the shared library file in a plugin directory
fn find_library(dir: &Path, name: &str) -> Option<PathBuf> {
    let expected = dir.join(library_filename(name));
    if expected.is_file() {
        return Some(expected);
    }
    // Fallback: scan for any .so/.dylib/.dll in the directory
    let ext = library_extension();
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .find(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s == ext)
                .unwrap_or(false)
        })
        .map(|e| e.path())
}

/// Default free function matching libc::free signature
extern "C" fn default_free(ptr: *mut std::os::raw::c_char) {
    unsafe { libc::free(ptr as *mut libc::c_void) }
}

/// Load a single plugin from a directory containing `plugin.toml` + shared library
fn load_plugin(dir: &Path) -> eyre::Result<PluginInstance> {
    let manifest_path = dir.join("plugin.toml");
    if !manifest_path.is_file() {
        eyre::bail!("no plugin.toml found");
    }

    let manifest = PluginManifest::load(&manifest_path)?;
    manifest.validate_abi()?;
    manifest.validate_semver()?;

    let lib_path = find_library(dir, &manifest.plugin.name)
        .ok_or_else(|| eyre::eyre!("shared library not found"))?;

    // SAFETY: we validate ABI before loading, and the init function is the only symbol we look up
    let lib = unsafe { libloading::Library::new(&lib_path) }
        .map_err(|e| eyre::eyre!("failed to load {}: {}", lib_path.display(), e))?;

    type InitFn = unsafe extern "C" fn(
        *mut PluginInfo,
        *mut u32,
        *mut [Option<PluginFn>; 4],
    );

    let init: libloading::Symbol<InitFn> = unsafe { lib.get(b"norgolith_plugin_init") }
        .map_err(|e| eyre::eyre!("missing symbol norgolith_plugin_init: {}", e))?;

    let mut info = PluginInfo {
        abi_version: 0,
        name: std::ptr::null(),
        version: std::ptr::null(),
    };
    let mut hook_mask = 0u32;
    let mut hooks: [Option<PluginFn>; 4] = [None, None, None, None];

    unsafe { init(&mut info, &mut hook_mask, &mut hooks) };

    // Validate that the returned ABI matches what the manifest claims
    if info.abi_version != manifest.plugin.abi {
        warn!(
            "Plugin '{}' returned abi={} but manifest declares abi={}",
            manifest.plugin.name, info.abi_version, manifest.plugin.abi
        );
    }

    // Validate hook mask matches manifest declarations
    let declared_mask = manifest.hooks.to_mask();
    if hook_mask != declared_mask {
        warn!(
            "Plugin '{}' hook mask mismatch: manifest declares {:#x}, plugin returned {:#x}",
            manifest.plugin.name, declared_mask, hook_mask
        );
    }

    let plugin_hooks = PluginHooks {
        pre_build: if hook_mask & HOOK_PRE_BUILD != 0 {
            hooks[0]
        } else {
            None
        },
        post_convert: if hook_mask & HOOK_POST_CONVERT != 0 {
            hooks[1]
        } else {
            None
        },
        post_render: if hook_mask & HOOK_POST_RENDER != 0 {
            hooks[2]
        } else {
            None
        },
        post_build: if hook_mask & HOOK_POST_BUILD != 0 {
            hooks[3]
        } else {
            None
        },
    };

    Ok(PluginInstance {
        name: manifest.plugin.name.clone(),
        version: manifest.plugin.version.clone(),
        _lib: lib,
        hooks: plugin_hooks,
        manifest,
        free_string: default_free,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_debug() -> PathBuf {
        let out_dir = PathBuf::from(env!("OUT_DIR"));
        out_dir.parent().unwrap().parent().unwrap().to_path_buf()
    }

    fn write_test_manifest(dir: &Path, name: &str) {
        let manifest = format!(
            r#"[plugin]
name = "{name}"
version = "0.1.0"
norgolith = ">=0.4.0"
abi = 1

[hooks]
pre_build = false
post_convert = false
post_render = true
post_build = false

[capabilities]
filesystem = "none"
network = false

timeout_ms = 5000
"#
        );
        std::fs::write(dir.join("plugin.toml"), manifest).unwrap();
    }

    #[test]
    fn test_empty_plugins_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = PluginManager::load(tmp.path());
        assert!(mgr.is_empty());
    }

    #[test]
    fn test_missing_plugins_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = PluginManager::load(&tmp.path().join("nonexistent"));
        assert!(mgr.is_empty());
    }

    #[test]
    fn test_plugin_dir_without_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("plugins").join("broken");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        let mgr = PluginManager::load(tmp.path());
        assert!(mgr.is_empty());
    }

    #[test]
    fn test_load_ok_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("plugins").join("test-ok");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_test_manifest(&plugin_dir, "test-ok");

        let lib_name = library_filename("test-ok");
        let src = target_debug().join(&lib_name);
        if !src.is_file() {
            eprintln!("test plugin not compiled, skipping");
            return;
        }
        std::fs::copy(&src, plugin_dir.join(&lib_name)).unwrap();

        let mgr = PluginManager::load(tmp.path());
        assert_eq!(mgr.len(), 1);
        let p = mgr.plugins().next().unwrap();
        assert_eq!(p.name, "test-ok");
        assert!(p.hooks.post_render.is_some());
    }

    #[test]
    fn test_hook_ok_transform() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("plugins").join("test-ok");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_test_manifest(&plugin_dir, "test-ok");

        let lib_name = library_filename("test-ok");
        let src = target_debug().join(&lib_name);
        if !src.is_file() {
            eprintln!("test plugin not compiled, skipping");
            return;
        }
        std::fs::copy(&src, plugin_dir.join(&lib_name)).unwrap();

        let mgr = PluginManager::load(tmp.path());
        let p = mgr.plugins().next().unwrap();
        let input = r#"{"html":"<p>hello</p>","metadata":{},"rel_path":"test.norg"}"#;
        let result = p.call_hook(p.hooks.post_render.unwrap(), input);
        assert!(result.is_some());
        assert!(result.unwrap().contains("[transformed]"));
    }

    #[test]
    fn test_hook_null_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("plugins").join("test-null");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_test_manifest(&plugin_dir, "test-null");

        let lib_name = library_filename("test-null");
        let src = target_debug().join(&lib_name);
        if !src.is_file() {
            eprintln!("test plugin not compiled, skipping");
            return;
        }
        std::fs::copy(&src, plugin_dir.join(&lib_name)).unwrap();

        let mgr = PluginManager::load(tmp.path());
        let p = mgr.plugins().next().unwrap();
        let input = r#"{"html":"<p>hello</p>","metadata":{},"rel_path":"test.norg"}"#;
        let result = p.call_hook(p.hooks.post_render.unwrap(), input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_hook_timeout() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("plugins").join("test-timeout");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_test_manifest(&plugin_dir, "test-timeout");

        let lib_name = library_filename("test-timeout");
        let src = target_debug().join(&lib_name);
        if !src.is_file() {
            eprintln!("test plugin not compiled, skipping");
            return;
        }
        std::fs::copy(&src, plugin_dir.join(&lib_name)).unwrap();

        let mgr = PluginManager::load(tmp.path());
        let p = mgr.plugins().next().unwrap();
        let input = r#"{"html":"<p>hello</p>","metadata":{},"rel_path":"test.norg"}"#;
        // Should return None (timeout is logged as warning, hook returns None)
        let result = p.call_hook(p.hooks.post_render.unwrap(), input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_hook_error_response() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("plugins").join("test-error");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_test_manifest(&plugin_dir, "test-error");

        let lib_name = library_filename("test-error");
        let src = target_debug().join(&lib_name);
        if !src.is_file() {
            eprintln!("test plugin not compiled, skipping");
            return;
        }
        std::fs::copy(&src, plugin_dir.join(&lib_name)).unwrap();

        let mgr = PluginManager::load(tmp.path());
        let p = mgr.plugins().next().unwrap();
        let input = r#"{"html":"<p>hello</p>","metadata":{},"rel_path":"test.norg"}"#;
        // Error response -> call_hook returns None (warning logged)
        let result = p.call_hook(p.hooks.post_render.unwrap(), input);
        assert_eq!(result, None);
    }
}
