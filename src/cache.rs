use std::collections::HashMap;
use std::path::{Path, PathBuf};

use colored::Colorize;
use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Cached metadata entry with content hash for invalidation.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    content_hash: String,
    metadata: serde_json::Value,
}

/// Returns the XDG cache directory for a site: `~/.cache/norgolith/{site_name}/`
fn cache_dir_for_site(site_root: &Path) -> Result<PathBuf> {
    let cache_base = dirs::cache_dir()
        .ok_or_else(|| eyre!("{}: cannot determine cache directory", "Failed".bold()))?;
    let site_name = site_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    Ok(cache_base.join("norgolith").join(site_name))
}

/// Build cache for incremental builds.
///
/// Stores parsed metadata keyed by relative file path. Entries are invalidated when:
/// - File content changes (blake3 hash comparison)
/// - Templates, config, or theme change (global hash stored in `.global_hash` file)
#[derive(Debug)]
pub struct BuildCache {
    cache_dir: PathBuf,
    entries: HashMap<PathBuf, CacheEntry>,
    global_hash: String,
}

impl BuildCache {
    /// Creates or loads a build cache.
    ///
    /// `site_root` is the directory containing `norgolith.toml`.
    /// Cache is stored in `~/.cache/norgolith/{site_name}/` (XDG_CACHE_HOME).
    /// If the global state (templates + config + theme) changed since last build,
    /// the entire cache is cleared.
    pub fn open(site_root: &Path) -> Result<Self> {
        let cache_dir = cache_dir_for_site(site_root)?;

        let global_hash = compute_global_hash(site_root)?;

        // Load existing cache
        let (mut entries, stored_global) = if cache_dir.exists() {
            let stored = read_global_hash(&cache_dir);
            let entries = load_entries(&cache_dir)?;
            (entries, stored)
        } else {
            (HashMap::new(), None)
        };

        // If global hash changed, clear all entries
        if stored_global.as_deref() != Some(global_hash.as_str()) {
            debug!("Global state changed (or first build), clearing cache");
            entries.clear();
            let _ = std::fs::remove_dir_all(&cache_dir);
        }

        Ok(Self { cache_dir, entries, global_hash })
    }

    /// Looks up cached metadata for a file.
    ///
    /// Returns `Some(metadata)` if the cache hit (content unchanged).
    /// Returns `None` on miss (file changed or never cached).
    pub fn get(&self, rel_path: &Path, content: &str) -> Option<serde_json::Value> {
        let entry = self.entries.get(rel_path)?;
        let hash = blake3_hash(content);
        if entry.content_hash == hash {
            debug!(path = %rel_path.display(), "cache hit");
            Some(entry.metadata.clone())
        } else {
            debug!(path = %rel_path.display(), "cache miss (content changed)");
            None
        }
    }

    /// Stores metadata in the cache.
    pub fn insert(&mut self, rel_path: &Path, content: &str, metadata: serde_json::Value) {
        let hash = blake3_hash(content);
        self.entries.insert(
            rel_path.to_path_buf(),
            CacheEntry {
                content_hash: hash,
                metadata,
            },
        );
    }

    /// Saves cache entries and global hash to disk.
    pub fn save(&self) -> Result<()> {
        if !self.cache_dir.exists() {
            std::fs::create_dir_all(&self.cache_dir).map_err(|e| {
                eyre!("{}: {}", "Failed to create cache directory".bold(), e)
            })?;
        }

        // Write global hash
        let global_path = self.cache_dir.join(".global_hash");
        std::fs::write(&global_path, &self.global_hash).map_err(|e| {
            eyre!("{}: {}", "Failed to write global hash".bold(), e)
        })?;

        // Write each entry
        for (rel_path, entry) in &self.entries {
            let cache_path = self.cache_dir.join(rel_path).with_extension("json");
            if let Some(parent) = cache_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let json = serde_json::to_string_pretty(entry).map_err(|e| {
                eyre!("{}: {}", "Failed to serialize cache entry".bold(), e)
            })?;
            std::fs::write(&cache_path, json).unwrap_or_else(|e| {
                warn!(
                    path = %cache_path.display(),
                    "Failed to write cache entry: {}", e
                );
            });
        }

        debug!(count = self.entries.len(), "Cache saved");
        Ok(())
    }
}

/// Computes a blake3 hash of the file content.
fn blake3_hash(content: &str) -> String {
    blake3::hash(content.as_bytes()).to_hex().to_string()
}

/// Reads the stored global hash from cache.
fn read_global_hash(cache_dir: &Path) -> Option<String> {
    let path = cache_dir.join(".global_hash");
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Computes a global hash from templates, config, and theme directories.
fn compute_global_hash(site_root: &Path) -> Result<String> {
    let mut hasher = blake3::Hasher::new();

    // Hash config file
    let config_path = site_root.join("norgolith.toml");
    if config_path.exists() {
        if let Ok(content) = std::fs::read(&config_path) {
            hasher.update(&content);
        }
    }

    // Hash templates directory
    let templates_dir = site_root.join("templates");
    if templates_dir.exists() {
        hash_dir(&templates_dir, &mut hasher)?;
    }

    // Hash theme templates directory
    let theme_templates_dir = site_root.join("theme").join("templates");
    if theme_templates_dir.exists() {
        hash_dir(&theme_templates_dir, &mut hasher)?;
    }

    Ok(hasher.finalize().to_hex().to_string())
}

/// Hashes all files in a directory recursively.
fn hash_dir(dir: &Path, hasher: &mut blake3::Hasher) -> Result<()> {
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
    {
        if let Ok(content) = std::fs::read(entry.path()) {
            if let Ok(rel) = entry.path().strip_prefix(dir) {
                hasher.update(rel.to_string_lossy().as_bytes());
            }
            hasher.update(&content);
        }
    }
    Ok(())
}

/// Loads cache entries from disk.
fn load_entries(cache_dir: &Path) -> Result<HashMap<PathBuf, CacheEntry>> {
    let mut entries = HashMap::new();

    for entry in walkdir::WalkDir::new(cache_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
    {
        let path = entry.path();
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(cache_entry) = serde_json::from_str::<CacheEntry>(&content) {
                if let Ok(rel) = path.strip_prefix(cache_dir) {
                    let rel_no_ext = rel.with_extension("");
                    entries.insert(rel_no_ext, cache_entry);
                }
            }
        }
    }

    debug!(count = entries.len(), "Cache loaded");
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake3_hash_deterministic() {
        let h1 = blake3_hash("hello world");
        let h2 = blake3_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_blake3_hash_different_inputs() {
        let h1 = blake3_hash("hello");
        let h2 = blake3_hash("world");
        assert_ne!(h1, h2);
    }
}
