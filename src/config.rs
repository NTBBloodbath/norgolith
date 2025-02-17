use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SiteConfigHighlighter {
    pub enable: bool,
    pub engine: Option<String>, // fallbacks to prism if not defined
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SiteConfig {
    #[serde(rename = "rootUrl")]
    pub root_url: String,
    pub language: String,
    pub title: String,
    pub author: String,
    pub highlighter: Option<SiteConfigHighlighter>,
}
