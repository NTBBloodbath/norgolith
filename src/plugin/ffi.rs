use std::os::raw::c_char;

/// C ABI PluginInfo struct returned by `norgolith_plugin_init`
#[repr(C)]
pub struct PluginInfo {
    /// Which version of plugin.h this .so was compiled against
    pub abi_version: u32,
    /// Human-readable plugin name for error messages
    pub name: *const c_char,
    /// Semantic version for debugging (not validation)
    pub version: *const c_char,
}

/// Function pointer type for plugin hooks
pub type PluginFn = extern "C" fn(*const c_char) -> *mut c_char;

/// Function pointer type for freeing plugin-allocated strings
pub type FreeStringFn = extern "C" fn(*mut c_char);
