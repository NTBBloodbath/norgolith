use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// Current ABI version that plugins must target
pub const CORE_ABI_VERSION: u32 = 1;

/// Hook bit flags for the plugin hook mask
pub const HOOK_PRE_BUILD: u32 = 1;
pub const HOOK_POST_CONVERT: u32 = 2;
pub const HOOK_POST_RENDER: u32 = 4;
pub const HOOK_POST_BUILD: u32 = 8;

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

/// Context for the pre_build hook
#[derive(serde::Deserialize)]
pub struct PreBuildContext {
    pub site_config: serde_json::Value,
    pub pages_dir: String,
    pub output_dir: String,
}

/// Context for post_convert and post_render hooks
#[derive(serde::Deserialize)]
pub struct TransformContext {
    pub html: String,
    pub metadata: serde_json::Value,
    pub rel_path: String,
}

/// Context for the post_build hook
#[derive(serde::Deserialize)]
pub struct PostBuildContext {
    pub site_config: serde_json::Value,
    pub pages_dir: String,
    pub output_dir: String,
}

/// Universal bridge function: reads C input → calls handler → returns JSON/NULL/error
///
/// The handler receives a `serde_json::Value` and must return:
/// - `Ok(Some(html))` — modified content (returned as `{"html":"..."}`)
/// - `Ok(None)` — no change (returned as NULL)
/// - `Err(msg)` — error (returned as `{"error":"..."}`)
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn __bridge_json<F>(input: *const c_char, handler: F) -> *mut c_char
where
    F: FnOnce(serde_json::Value) -> Result<Option<String>, String>,
{
    let input_str = unsafe { CStr::from_ptr(input) }
        .to_str()
        .unwrap_or("{}");

    let value: serde_json::Value =
        serde_json::from_str(input_str).unwrap_or(serde_json::Value::Null);

    match handler(value) {
        Ok(Some(html)) => {
            let output = serde_json::json!({"html": html}).to_string();
            CString::new(output).unwrap().into_raw()
        }
        Ok(None) => std::ptr::null_mut(),
        Err(e) => {
            let output = serde_json::json!({"error": e}).to_string();
            CString::new(output).unwrap().into_raw()
        }
    }
}

/// Set a hook function pointer and mask bit by name
///
/// Hook names map to array indices: pre_build=0, post_convert=1, post_render=2, post_build=3
pub fn __set_hook(
    name: &str,
    mask: &mut u32,
    hooks: &mut [Option<PluginFn>; 4],
    func: PluginFn,
) {
    match name {
        "pre_build" => {
            *mask |= HOOK_PRE_BUILD;
            hooks[0] = Some(func);
        }
        "post_convert" => {
            *mask |= HOOK_POST_CONVERT;
            hooks[1] = Some(func);
        }
        "post_render" => {
            *mask |= HOOK_POST_RENDER;
            hooks[2] = Some(func);
        }
        "post_build" => {
            *mask |= HOOK_POST_BUILD;
            hooks[3] = Some(func);
        }
        _ => {}
    }
}

/// Register a plugin with the given name and version.
///
/// Generates the `norgolith_plugin_init` function and bridge functions for each hook.
///
/// # Example
///
/// ```rust
/// use norgolith_plugin_sdk::*;
///
/// fn highlight(json: serde_json::Value) -> Result<Option<String>, String> {
///     let ctx: TransformContext = serde_json::from_value(json).map_err(|e| e.to_string())?;
///     Ok(Some(ctx.html))
/// }
///
/// register_plugin!("my-plugin", "0.1.0",
///     hooks: [post_render: highlight]
/// );
/// ```
#[macro_export]
macro_rules! register_plugin {
    ($name:expr, $version:expr, hooks: [$($hook:ident : $handler:ident),* $(,)?]) => {
        // Generate one bridge function per hook
        $(
            #[no_mangle]
            pub extern "C" fn $hook(input: *const ::std::os::raw::c_char) -> *mut ::std::os::raw::c_char {
                $crate::__bridge_json(input, $handler)
            }
        )*

        // Generate the init function
        #[no_mangle]
        pub extern "C" fn norgolith_plugin_init(
            info: &mut $crate::PluginInfo,
            mask: &mut u32,
            hooks: &mut [Option<$crate::PluginFn>; 4],
        ) {
            *info = $crate::PluginInfo {
                abi_version: $crate::CORE_ABI_VERSION,
                name: concat!($name, "\0").as_ptr() as *const ::std::os::raw::c_char,
                version: concat!($version, "\0").as_ptr() as *const ::std::os::raw::c_char,
            };
            *mask = 0;
            *hooks = [None, None, None, None];
            $(
                $crate::__set_hook(stringify!($hook), mask, hooks, $hook as $crate::PluginFn);
            )*
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_bridge_json_success() {
        let input = CString::new(r#"{"html":"hello","metadata":{},"rel_path":"test.norg"}"#).unwrap();

        fn handler(json: serde_json::Value) -> Result<Option<String>, String> {
            let html = json.get("html").and_then(|v| v.as_str()).unwrap();
            Ok(Some(format!("{} world", html)))
        }

        let result = __bridge_json(input.as_ptr(), handler);
        assert!(!result.is_null());

        let output = unsafe { CStr::from_ptr(result) }.to_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(output).unwrap();
        assert_eq!(parsed.get("html").and_then(|v| v.as_str()).unwrap(), "hello world");
    }

    #[test]
    fn test_bridge_json_no_change() {
        let input = CString::new(r#"{"html":"keep","metadata":{},"rel_path":"test.norg"}"#).unwrap();

        fn handler(_json: serde_json::Value) -> Result<Option<String>, String> {
            Ok(None)
        }

        let result = __bridge_json(input.as_ptr(), handler);
        assert!(result.is_null());
    }

    #[test]
    fn test_bridge_json_error() {
        let input = CString::new(r#"{"html":"fail","metadata":{},"rel_path":"test.norg"}"#).unwrap();

        fn handler(_json: serde_json::Value) -> Result<Option<String>, String> {
            Err("something went wrong".to_string())
        }

        let result = __bridge_json(input.as_ptr(), handler);
        assert!(!result.is_null());

        let output = unsafe { CStr::from_ptr(result) }.to_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(output).unwrap();
        assert_eq!(parsed.get("error").and_then(|v| v.as_str()).unwrap(), "something went wrong");
    }

    #[test]
    fn test_set_hook_all_names() {
        let mut mask = 0u32;
        let mut hooks: [Option<PluginFn>; 4] = [None, None, None, None];

        extern "C" fn dummy(_input: *const c_char) -> *mut c_char {
            std::ptr::null_mut()
        }

        __set_hook("pre_build", &mut mask, &mut hooks, dummy);
        assert_eq!(mask, HOOK_PRE_BUILD);
        assert!(hooks[0].is_some());

        __set_hook("post_convert", &mut mask, &mut hooks, dummy);
        assert_eq!(mask, HOOK_PRE_BUILD | HOOK_POST_CONVERT);
        assert!(hooks[1].is_some());

        __set_hook("post_render", &mut mask, &mut hooks, dummy);
        assert_eq!(mask, HOOK_PRE_BUILD | HOOK_POST_CONVERT | HOOK_POST_RENDER);
        assert!(hooks[2].is_some());

        __set_hook("post_build", &mut mask, &mut hooks, dummy);
        assert_eq!(mask, HOOK_PRE_BUILD | HOOK_POST_CONVERT | HOOK_POST_RENDER | HOOK_POST_BUILD);
        assert!(hooks[3].is_some());
    }

    #[test]
    fn test_set_hook_unknown_name() {
        let mut mask = 0u32;
        let mut hooks: [Option<PluginFn>; 4] = [None, None, None, None];

        extern "C" fn dummy(_input: *const c_char) -> *mut c_char {
            std::ptr::null_mut()
        }

        __set_hook("unknown_hook", &mut mask, &mut hooks, dummy);
        assert_eq!(mask, 0);
        assert!(hooks.iter().all(|h| h.is_none()));
    }
}
