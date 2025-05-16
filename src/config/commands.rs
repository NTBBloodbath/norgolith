use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct BuildConfig {
    #[serde(default = "default_minify")]
    pub minify: bool,
}

fn default_minify() -> bool {
    true
}

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct DevConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_drafts")]
    pub drafts: bool,
    #[serde(default)]
    pub host: bool,
    #[serde(default)]
    pub open: bool,
}

fn default_port() -> u16 {
    3030
}
fn default_drafts() -> bool {
    true
}
