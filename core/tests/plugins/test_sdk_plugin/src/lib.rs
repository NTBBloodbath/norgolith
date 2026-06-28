use norgolith_plugin_sdk::*;

fn post_render_handler(json: serde_json::Value) -> Result<Option<String>, String> {
    let ctx: TransformContext = serde_json::from_value(json).map_err(|e| e.to_string())?;
    // Echo back the HTML with a marker
    Ok(Some(format!("<!-- plugin-ok -->{}", ctx.html)))
}

register_plugin!("test-sdk-plugin", "0.1.0",
    hooks: [post_render: post_render_handler]
);
