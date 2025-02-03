use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SiteConfigHighlighter {
    enable: bool,
    engine: Option<String>, // fallbacks to prism if not defined
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SiteConfig {
    #[serde(rename = "rootUrl")]
    root_url: String,
    language: String,
    title: String,
    author: String,
    highlighter: Option<SiteConfigHighlighter>,
}
