use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::schema::ContentSchema;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SiteConfigHighlighter {
    pub enable: bool,
    pub engine: Option<String>, // fallbacks to prism if not defined
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SiteConfigRss {
    pub enable: bool,
    pub ttl: i32,
    pub description: String,
    pub image: String,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct SiteConfig {
    #[serde(rename = "rootUrl")]
    pub root_url: String,
    pub language: String,
    pub title: String,
    pub author: String,
    #[serde(default)]
    pub content_schema: Option<ContentSchema>,
    pub highlighter: Option<SiteConfigHighlighter>,
    pub rss: Option<SiteConfigRss>,
    pub extra: Option<HashMap<String, toml::Value>>,
}
