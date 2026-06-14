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
    pub ttl: u32,
    pub description: String,
    pub image: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CollectionConfig {
    pub name: String,
    pub dir: String,
}

fn default_collections() -> Vec<CollectionConfig> {
    vec![CollectionConfig {
        name: "posts".into(),
        dir: "posts".into(),
    }]
}

fn default_categories_dir() -> String {
    "categories".into()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
    #[serde(default = "default_collections", rename = "collections")]
    pub collections: Vec<CollectionConfig>,
    #[serde(default = "default_categories_dir", rename = "categoriesDir")]
    pub categories_dir: String,
}

impl Default for SiteConfig {
    fn default() -> Self {
        Self {
            root_url: String::new(),
            language: String::new(),
            title: String::new(),
            author: String::new(),
            content_schema: None,
            highlighter: None,
            rss: None,
            extra: None,
            collections: default_collections(),
            categories_dir: default_categories_dir(),
        }
    }
}
