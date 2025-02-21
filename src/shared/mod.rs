use std::path::{Path, PathBuf};
use std::time::Instant;

use eyre::{bail, eyre, Context, Result};
use tera::Tera;

use crate::converter;

pub async fn get_content(name: &str) -> Result<(String, PathBuf)> {
    let build_path = Path::new(".build");
    let mut candidates: Vec<PathBuf> = Vec::new();

    // Normalize path by trimming slashes
    let clean_name = name.trim_matches('/');

    if clean_name.is_empty() {
        // Root path
        candidates.push(build_path.join("index.html"));
    } else {
        // Generate potential file paths
        candidates.push(build_path.join(format!("{}.html", clean_name))); // /docs -> docs.html
        candidates.push(build_path.join(clean_name).join("index.html")); // /docs -> docs/index.html
    }

    // Try candidates in order
    for path in &candidates {
        if tokio::fs::try_exists(path).await? {
            return Ok((tokio::fs::read_to_string(path).await?, path.to_path_buf()));
        }
    }

    Err(eyre::eyre!("Content not found for path: {}", name))
}

/// Recursively converts all the norg files in the content directory
pub async fn convert_content(content_dir: &Path, convert_drafts: bool, root_url: &str) -> Result<()> {
    async fn process_entry(
        entry: tokio::fs::DirEntry,
        content_dir: &Path,
        convert_drafts: bool,
        root_url: &str
    ) -> Result<()> {
        let path = entry.path();
        if path.is_dir() {
            // Process directory recursively
            let mut content_stream = tokio::fs::read_dir(&path).await?;
            while let Some(entry) = content_stream.next_entry().await? {
                Box::pin(process_entry(entry, content_dir, convert_drafts, root_url)).await?;
            }
        } else {
            convert_document(&path, content_dir, convert_drafts, root_url).await?;
        }
        Ok(())
    }

    let mut content_stream = tokio::fs::read_dir(content_dir).await?;
    while let Some(entry) = content_stream.next_entry().await? {
        Box::pin(process_entry(entry, content_dir, convert_drafts, root_url)).await?;
    }

    Ok(())
}

pub async fn convert_document(
    file_path: &Path,
    content_dir: &Path,
    convert_drafts: bool,
    root_url: &str
) -> Result<()> {
    if file_path.extension().unwrap_or_default() == "norg"
        && tokio::fs::try_exists(file_path).await?
    {
        let mut should_convert = true;
        let mut should_write_meta = true;

        // Preserve directory structure relative to content directory
        let relative_path = file_path.strip_prefix(content_dir).map_err(|_| {
            eyre!(
                "File {:?} is not in content directory {:?}",
                file_path,
                content_dir
            )
        })?;

        let html_file_path = Path::new(".build")
            .join(relative_path)
            .with_extension("html");
        let meta_file_path = html_file_path.with_extension("meta.toml");

        // Convert html content
        let norg_document = tokio::fs::read_to_string(file_path).await?;
        let norg_html = converter::html::convert(norg_document.clone(), root_url);

        // Convert metadata
        let norg_meta = converter::meta::convert(&norg_document)?;
        let meta_toml = toml::to_string_pretty(&norg_meta)?;

        // Check if the current document is a draft post and also whether we should finish the conversion
        // NOTE: content is not marked as draft by default
        if toml::Value::as_bool(norg_meta.get("draft").unwrap_or(&toml::Value::from(false)))
            .expect("draft metadata field should be a boolean") && !convert_drafts
        {
            return Ok(());
        }

        // Check existing metadata only if file exists
        if tokio::fs::try_exists(&meta_file_path).await? {
            let meta_content = tokio::fs::read_to_string(&meta_file_path).await?;
            should_write_meta = meta_toml != meta_content;
        }

        // Check existing content only if file exists
        if tokio::fs::try_exists(&html_file_path).await? {
            let html_content = tokio::fs::read_to_string(&html_file_path).await?;
            should_convert = norg_html != html_content;
        }

        if should_convert || should_write_meta {
            // Create parent directories if needed
            if let Some(parent) = html_file_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            // XXX: maybe these println makes stuff too verbose? Modifying a norg file already triggers two stdout messages
            if should_convert {
                // println!("[server] Converting norg file: {}", relative_path.display());
                tokio::fs::write(&html_file_path, norg_html).await?;
            }
            if should_write_meta {
                // println!("[server] Converting norg meta: {}", relative_path.display());
                tokio::fs::write(&meta_file_path, meta_toml).await?;
            }
        }
    }

    Ok(())
}

pub async fn cleanup_orphaned_build_files(content_dir: &Path) -> Result<()> {
    let build_dir = Path::new(".build");
    if !build_dir.exists() {
        return Ok(());
    }

    let mut stack = vec![build_dir.to_path_buf()];

    while let Some(current_dir) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&current_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_dir() {
                stack.push(path);
            } else if path.extension().map(|e| e == "html").unwrap_or(false) {
                let relative_path = path.strip_prefix(build_dir)?;
                let norg_path = content_dir.join(relative_path).with_extension("norg");

                if !norg_path.exists() {
                    // Delete HTML and meta files
                    let meta_path = path.with_extension("meta.toml");

                    tokio::fs::remove_file(&path).await?;
                    if tokio::fs::try_exists(&meta_path).await? {
                        tokio::fs::remove_file(&meta_path).await?;
                    }

                    println!("[server] Cleaned orphaned build file: {}", path.display());
                }
            }
        }
    }

    Ok(())
}

pub fn get_elapsed_time(instant: Instant) -> String {
    let duration = instant.elapsed();
    let secs = duration.as_secs_f64();

    if secs < 1.0 {
        format!("{:.0}ms", secs * 1000.0)
    } else {
        format!("{:.1}s", secs)
    }
}

pub async fn init_tera(templates_dir: &str, theme_dir: &Path) -> Result<Tera> {
    // Initialize Tera with the user-defined templates first
    let mut tera = match Tera::new(&(templates_dir.to_owned() + "/**/*.html")) {
        Ok(t) => t,
        Err(e) => bail!("Tera parsing error(s): {}", e),
    };

    // Theme templates will override the user-defined templates by design if they are named exactly
    // the same in both the user's templates directory and the theme templates directory
    if tokio::fs::try_exists(theme_dir.join("templates")).await? {
        let mut theme_templates: Vec<(String, Option<String>)> = Vec::new();

        let mut entries = tokio::fs::read_dir(&theme_dir.join("templates")).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_file() && path.extension().map(|e| e == "html").unwrap_or(false) {
                theme_templates.push((path.into_os_string().into_string().unwrap(), Some(entry.file_name().into_string().unwrap())))
            }
        }
        tera.add_template_files(theme_templates)
            .wrap_err("Failed to load theme templates")?;
    }

    // Register functions
    tera.register_function("now", crate::tera_functions::NowFunction);

    Ok(tera)
}
