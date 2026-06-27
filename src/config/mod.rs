use std::collections::HashMap;

use colored::Colorize;
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SiteConfigSeo {
    #[serde(default = "default_true")]
    pub sitemap: bool,
    #[serde(default = "default_true")]
    pub open_graph: bool,
    #[serde(default, rename = "default_image")]
    pub default_image: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SiteConfigRobots {
    #[serde(default = "default_true")]
    pub enable: bool,
    pub preset: Option<RobotsPreset>,
    #[serde(default, rename = "custom_file")]
    pub custom: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum RobotsPreset {
    #[serde(rename = "allow_all")]
    AllowAll,
    #[serde(rename = "no_llms")]
    NoLlms,
    #[serde(rename = "block_all")]
    BlockAll,
}

fn default_true() -> bool {
    true
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
    #[serde(default)]
    pub seo: Option<SiteConfigSeo>,
    pub robots: Option<SiteConfigRobots>,
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
            seo: None,
            robots: None,
        }
    }
}

impl SiteConfig {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.root_url.is_empty() {
            errors.push(format!(
                "{}: 'rootUrl' must not be empty",
                "Validation failed".bold()
            ));
        }
        if self.title.is_empty() {
            errors.push(format!(
                "{}: 'title' must not be empty",
                "Validation failed".bold()
            ));
        }
        if self.author.is_empty() {
            errors.push(format!(
                "{}: 'author' must not be empty",
                "Validation failed".bold()
            ));
        }
        if self.language.is_empty() {
            errors.push(format!(
                "{}: 'language' must not be empty",
                "Validation failed".bold()
            ));
        }

        errors
    }
}
