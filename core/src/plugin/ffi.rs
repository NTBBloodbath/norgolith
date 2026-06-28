use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::AssertUnwindSafe;
use std::sync::mpsc;
use std::time::Duration;

use eyre::{bail, Result};

/// Wrapper to make raw pointer results Send-safe across threads
struct HookResult(Option<*mut c_char>);

// SAFETY: we only send the pointer across threads, actual deref happens on the receiving side
unsafe impl Send for HookResult {}

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

/// Call a plugin hook with catch_unwind + thread timeout
///
/// Returns `Ok(None)` if the plugin returned NULL (no change)
/// Returns `Ok(Some(json))` if the plugin returned modified content
/// Returns `Err(msg)` on panic, timeout, or invalid output
pub fn call_hook_safe(
    f: PluginFn,
    input: &str,
    timeout: Duration,
) -> Result<Option<String>> {
    let c_input = CString::new(input)
        .map_err(|e| eyre::eyre!("failed to create CString: {}", e))?;

    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            HookResult(Some(f(c_input.as_ptr())))
        }));
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(HookResult(ptr))) => {
            let ptr = ptr.unwrap();
            if ptr.is_null() {
                return Ok(None);
            }
            let result = unsafe { CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned();
            unsafe { libc::free(ptr as *mut libc::c_void) };
            Ok(Some(result))
        }
        Ok(Err(panic)) => {
            let msg = match panic.downcast_ref::<&str>() {
                Some(s) => s.to_string(),
                None => match panic.downcast_ref::<String>() {
                    Some(s) => s.clone(),
                    None => "unknown panic".to_string(),
                },
            };
            Err(eyre::eyre!("plugin panicked: {}", msg))
        }
        Err(_timeout) => {
            Err(eyre::eyre!(
                "plugin hook timed out after {}ms",
                timeout.as_millis()
            ))
        }
    }
}

/// Parse a hook response JSON and extract the HTML field
///
/// Returns `Ok(None)` if html is null (no change)
/// Returns `Ok(Some(html))` if html is present
/// Returns `Err` on error status or invalid JSON
pub fn parse_hook_response(json: &str) -> Result<Option<String>> {
    let val: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| eyre::eyre!("invalid JSON from plugin: {}", e))?;

    if let Some(status) = val.get("status").and_then(|v| v.as_str()) {
        if status == "error" {
            let msg = val
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            bail!("plugin error: {}", msg);
        }
    }

    match val.get("html").and_then(|v| v.as_str()) {
        Some(html) => Ok(Some(html.to_string())),
        None => Ok(None),
    }
}

/// Parse a hook response for pre_build/post_build (status only)
pub fn parse_status_response(json: &str) -> Result<()> {
    let val: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| eyre::eyre!("invalid JSON from plugin: {}", e))?;

    if let Some(status) = val.get("status").and_then(|v| v.as_str()) {
        if status == "error" {
            let msg = val
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            bail!("plugin error: {}", msg);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hook_response_with_html() {
        let json = r#"{"html": "<h1>Hello</h1>"}"#;
        assert_eq!(
            parse_hook_response(json).unwrap(),
            Some("<h1>Hello</h1>".to_string())
        );
    }

    #[test]
    fn test_parse_hook_response_null_html() {
        let json = r#"{"html": null}"#;
        assert_eq!(parse_hook_response(json).unwrap(), None);
    }

    #[test]
    fn test_parse_hook_response_error() {
        let json = r#"{"status": "error", "message": "something broke"}"#;
        assert!(parse_hook_response(json).is_err());
    }

    #[test]
    fn test_parse_status_response_ok() {
        assert!(parse_status_response(r#"{"status": "ok"}"#).is_ok());
    }

    #[test]
    fn test_parse_status_response_error() {
        assert!(parse_status_response(
            r#"{"status": "error", "message": "init failed"}"#
        )
        .is_err());
    }
}
