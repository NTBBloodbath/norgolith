# Norgolith Plugin SDK

SDK for building Norgolith plugins. Write a hook function, register it with a macro, and the plugin runs during the build process.

For the full plugin guide, see [Plugins documentation](https://norgolith.dev/docs/plugins).

## Adding the SDK

Add the SDK to your `Cargo.toml`:

```toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
norgolith-plugin-sdk = "0.1"
```

The `cdylib` crate type is required so the output is a shared library (`.so` on Linux, `.dylib` on macOS, `.dll` on Windows).

## A Minimal Plugin

The quickest way to start is `lith plugin new my-plugin`, which generates:

```
plugins/my-plugin/
  plugin.toml      # Plugin manifest
  Cargo.toml       # Rust package config
  src/lib.rs       # Your hook code
```

The generated `src/lib.rs` looks like this:

```rust
use norgolith_plugin_sdk::*;

register_plugin!("my-plugin", "0.1.0",
    hooks: [post_render: my_hook]
);

fn my_hook(json: serde_json::Value) -> Result<Option<String>, String> {
    let ctx: TransformContext = serde_json::from_value(json)
        .map_err(|e| e.to_string())?;
    Ok(Some(ctx.html))
}
```

Build and install it:

```bash
cd plugins/my-plugin
cargo build --release
cd ../..
lith plugin install plugins/my-plugin
```

Now `lith build` runs your plugin on every page.

## The `register_plugin!` Macro

The macro generates two things: the shared library entry point and bridge functions for each hook.

```rust
register_plugin!("plugin-name", "0.1.0",
    hooks: [hook_name: handler_function, ...]
);
```

- First argument: plugin name (must match `plugin.toml`)
- Second argument: version string
- `hooks:` list: maps hook names to your handler functions

Valid hook names are `pre_build`, `post_convert`, `post_render`, and `post_build`.

## Hook Types

Each hook runs at a different point in the build process:

| Hook           | When it runs                                 | Input                    | Use case                                        |
| -------------- | -------------------------------------------- | ------------------------ | ----------------------------------------------- |
| `pre_build`    | Before any content is processed              | Site config              | Initialize plugin state, load external data     |
| `post_convert` | After Norg-to-HTML, before Tera templating   | HTML fragment + metadata | Modify page content (e.g., syntax highlighting) |
| `post_render`  | After Tera layout is applied, before writing | Final HTML page          | Inject into `<head>` or `<body>`                |
| `post_build`   | After all pages are written to disk          | Site config              | Generate extra files (e.g., sitemaps)           |

## Context Types

Each hook receives a JSON string. Deserialize it into the matching context type:

### `PreBuildContext`

Available in `pre_build`:

```rust
#[derive(serde::Deserialize)]
pub struct PreBuildContext {
    pub site_config: serde_json::Value,  // Full site config as JSON
    pub pages_dir: String,               // Path to content/ directory
    pub output_dir: String,              // Path to public/ directory
}
```

### `TransformContext`

Available in `post_convert` and `post_render`:

```rust
#[derive(serde::Deserialize)]
pub struct TransformContext {
    pub html: String,                 // The HTML content
    pub metadata: serde_json::Value,  // Page metadata (title, date, etc.)
    pub rel_path: String,             // Relative path (e.g., "posts/hello.norg")
}
```

### `PostBuildContext`

Available in `post_build`:

```rust
#[derive(serde::Deserialize)]
pub struct PostBuildContext {
    pub site_config: serde_json::Value,
    pub pages_dir: String,
    pub output_dir: String,
}
```

## Return Values

Your handler function returns `Result<Option<String>, String>`:

- `Ok(Some(html))`: return modified content. The new HTML replaces the original.
- `Ok(None)`: no change. The page passes through unmodified.
- `Err(message)`: an error occurred. The error is logged and the page passes through unchanged.

```rust
fn my_hook(json: serde_json::Value) -> Result<Option<String>, String> {
    let ctx: TransformContext = serde_json::from_value(json)
        .map_err(|e| e.to_string())?;

    // Only modify pages with a specific tag
    if ctx.metadata.get("tags")
        .and_then(|v| v.as_array())
        .map(|tags| tags.iter().any(|t| t.as_str() == Some("special")))
        .unwrap_or(false)
    {
        Ok(Some(format!("<!-- special page -->\n{}", ctx.html)))
    } else {
        Ok(None)  // Leave other pages alone
    }
}
```

## Building

Build your plugin in release mode:

```bash
cargo build --release
```

The output shared library is in `target/release/`:

- Linux: `libmy-plugin.so`
- macOS: `libmy-plugin.dylib`
- Windows: `my-plugin.dll`

Install it:

```bash
lith plugin install plugins/my-plugin
```

This copies the `.so` and `plugin.toml` into your site's `plugins/` directory.

## Plugin Manifest (`plugin.toml`)

Every plugin needs a `plugin.toml` alongside the shared library:

```toml
[plugin]
name = "my-plugin"
version = "0.1.0"
norgolith = ">=0.4.0"   # Semver requirement for norgolith version
abi = 1                 # Must match CORE_ABI_VERSION

[hooks]
pre_build = false
post_convert = false
post_render = true      # Set to true for hooks you implement
post_build = false

[capabilities]
filesystem = "none"     # "none", "read", "write", "read-write"
network = false         # Whether plugin needs network access

timeout_ms = 10000      # Max time per hook call (default: 10000ms)
priority = 100          # Lower numbers run first (default: 100)
```

## Priority

Use the `priority` field to control execution order when you have multiple plugins. Lower numbers run first:

```toml
priority = 50   # Runs early
priority = 100  # Default, runs after priority 50
priority = 200  # Runs last
```

## Full Example

Here is a complete plugin that wraps every `<h1>` heading in a styled div:

```rust
use norgolith_plugin_sdk::*;

register_plugin!("heading-wrapper", "0.1.0",
    hooks: [post_render: wrap_headings]
);

fn wrap_headings(json: serde_json::Value) -> Result<Option<String>, String> {
    let ctx: TransformContext = serde_json::from_value(json)
        .map_err(|e| e.to_string())?;

    let html = ctx.html.replace(
        "<h1>",
        r#"<h1 style="border-bottom: 2px solid #333;">"#,
    );

    Ok(Some(html))
}
```

## See Also

- [Plugin system guide](https://norgolith.dev/docs/plugins): full documentation for users and plugin authors
- [Norgolith repository](https://github.com/NTBBloodbath/norgolith): source code and issue tracker
