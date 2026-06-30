use std::fmt::Write;

use tracing::warn;

use crate::config::{RobotsPreset, SiteConfig, SiteConfigRobots};

const ROBOTS_ALLOW_ALL: &str = "\
User-agent: *
Allow: /
";

const ROBOTS_BLOCK_ALL: &str = "\
User-agent: *
Disallow: /
";

// This const is updated by scripts/update-robots-presets.sh
// To update: run the script, which fetches from ai.robots.txt
const ROBOTS_NO_LLMS: &str = r"User-agent: AddSearchBot
User-agent: AgentTimes
User-agent: AI2Bot
User-agent: AI2Bot-DeepResearchEval
User-agent: Ai2Bot-Dolma
User-agent: aiHitBot
User-agent: amazon-kendra
User-agent: Amazonbot
User-agent: AmazonBuyForMe
User-agent: Amzn-SearchBot
User-agent: Amzn-User
User-agent: Andibot
User-agent: Anomura
User-agent: anthropic-ai
User-agent: ApifyBot
User-agent: ApifyWebsiteContentCrawler
User-agent: Applebot
User-agent: Applebot-Extended
User-agent: Aranet-SearchBot
User-agent: atlassian-bot
User-agent: Awario
User-agent: AzureAI-SearchBot
User-agent: bedrockbot
User-agent: bigsur.ai
User-agent: Bravebot
User-agent: Brightbot
User-agent: Brightbot 1.0
User-agent: BuddyBot
User-agent: Bytespider
User-agent: CCBot
User-agent: Channel3Bot
User-agent: ChatGLM-Spider
User-agent: ChatGPT Agent
User-agent: ChatGPT-User
User-agent: Claude-Code
User-agent: Claude-SearchBot
User-agent: Claude-User
User-agent: Claude-Web
User-agent: ClaudeBot
User-agent: Cloudflare-AutoRAG
User-agent: CloudVertexBot
User-agent: Code
User-agent: cohere-ai
User-agent: cohere-training-data-crawler
User-agent: Cotoyogi
User-agent: CragCrawler
User-agent: Crawl4AI
User-agent: Crawlspace
User-agent: Datenbank Crawler
User-agent: DeepSeekBot
User-agent: Devin
User-agent: Diffbot
User-agent: DuckAssistBot
User-agent: Echobot Bot
User-agent: EchoboxBot
User-agent: ExaBot
User-agent: FacebookBot
User-agent: facebookexternalhit
User-agent: Factset_spyderbot
User-agent: FirecrawlAgent
User-agent: FriendlyCrawler
User-agent: GeistHaus-PageFetcher
User-agent: Gemini-Deep-Research
User-agent: Google-Agent
User-agent: Google-CloudVertexBot
User-agent: Google-Extended
User-agent: Google-Firebase
User-agent: Google-Gemini-CLI
User-agent: Google-NotebookLM
User-agent: GoogleAgent-Mariner
User-agent: GoogleAgent-URLContext
User-agent: GoogleOther
User-agent: GoogleOther-Image
User-agent: GoogleOther-Video
User-agent: GPTBot
User-agent: HenkBot
User-agent: iAskBot
User-agent: iaskspider
User-agent: iaskspider/2.0
User-agent: IbouBot
User-agent: ICC-Crawler
User-agent: ImagesiftBot
User-agent: imageSpider
User-agent: img2dataset
User-agent: ISSCyberRiskCrawler
User-agent: kagi-fetcher
User-agent: Kangaroo Bot
User-agent: Kimi-User
User-agent: KlaviyoAIBot
User-agent: KunatoCrawler
User-agent: laion-huggingface-processor
User-agent: LAIONDownloader
User-agent: LCC
User-agent: LinerBot
User-agent: Linguee Bot
User-agent: LinkupBot
User-agent: Manus-User
User-agent: meta-externalagent
User-agent: Meta-ExternalAgent
User-agent: meta-externalfetcher
User-agent: Meta-ExternalFetcher
User-agent: meta-webindexer
User-agent: MistralAI-User
User-agent: MistralAI-User/1.0
User-agent: MyCentralAIScraperBot
User-agent: NagetBot
User-agent: netEstate Imprint Crawler
User-agent: newsai
User-agent: NotebookLM
User-agent: NovaAct
User-agent: OAI-SearchBot
User-agent: omgili
User-agent: omgilibot
User-agent: OpenAI
User-agent: opencode
User-agent: Operator
User-agent: PanguBot
User-agent: Panscient
User-agent: panscient.com
User-agent: Perplexity-User
User-agent: PerplexityBot
User-agent: PetalBot
User-agent: PhindBot
User-agent: Poggio-Citations
User-agent: Poseidon Research Crawler
User-agent: QualifiedBot
User-agent: Querit-SearchBot
User-agent: QueritBot
User-agent: QuillBot
User-agent: quillbot.com
User-agent: SBIntuitionsBot
User-agent: Scrapy
User-agent: SemrushBot-OCOB
User-agent: SemrushBot-SWA
User-agent: Shap-User
User-agent: ShapBot
User-agent: Sidetrade indexer bot
User-agent: Spider
User-agent: TavilyBot
User-agent: Terra Cotta
User-agent: TerraCotta
User-agent: Thinkbot
User-agent: TikTokSpider
User-agent: Timpibot
User-agent: Trae
User-agent: TwinAgent
User-agent: UseAI
User-agent: VelenPublicWebCrawler
User-agent: WARDBot
User-agent: Webzio-Extended
User-agent: webzio-extended
User-agent: wpbot
User-agent: WRTNBot
User-agent: YaK
User-agent: YandexAdditional
User-agent: YandexAdditionalBot
User-agent: YouBot
User-agent: ZanistaBot
Disallow: /";

/// Represents a URL entry for the sitemap XML.
#[derive(Debug, Clone)]
pub struct SitemapUrl {
    pub loc: String,
    pub lastmod: Option<String>,
}

/// Generates a `robots.txt` content based on the site configuration.
///
/// If `custom_file` is set, reads that file and appends the Sitemap directive.
/// Otherwise, generates from the configured preset.
pub fn generate_robots_txt(
    site_config: &SiteConfig,
    robots_config: &SiteConfigRobots,
    sitemap_enabled: bool,
) -> String {
    let mut buf = String::with_capacity(512);

    if let Some(ref custom_path) = robots_config.custom {
        match std::fs::read_to_string(custom_path) {
            Ok(content) => buf.push_str(&content),
            Err(e) => {
                warn!(
                    "Failed to read custom robots file '{}': {}",
                    custom_path, e
                );
                // Fallback to a permissive robots.txt
                buf.push_str(ROBOTS_ALLOW_ALL);
            }
        }
    } else if let Some(ref preset) = robots_config.preset {
        match preset {
            RobotsPreset::AllowAll => buf.push_str(ROBOTS_ALLOW_ALL),
            RobotsPreset::BlockAll => buf.push_str(ROBOTS_BLOCK_ALL),
            RobotsPreset::NoLlms => buf.push_str(ROBOTS_NO_LLMS),
        }
    } else {
        // No preset and no custom file: default to permissive
        buf.push_str(ROBOTS_ALLOW_ALL);
    }

    // Append Sitemap directive if sitemap is enabled
    if sitemap_enabled {
        let _ = writeln!(buf);
        let _ = writeln!(buf, "Sitemap: {}/sitemap.xml", site_config.root_url);
    }

    buf
}

/// Generates a `sitemap.xml` content from the given list of URLs.
pub fn generate_sitemap_xml(urls: &[SitemapUrl], root_url: &str) -> String {
    // Estimate: ~200 bytes per URL entry
    let mut buf = String::with_capacity(urls.len() * 200 + 128);

    buf.push_str(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
"#,
    );

    for url in urls {
        let _ = writeln!(buf, "  <url>");
        let _ = writeln!(
            buf,
            "    <loc>{}/{}</loc>",
            root_url.trim_end_matches('/'),
            url.loc.trim_start_matches('/')
        );
        if let Some(ref lastmod) = url.lastmod {
            let _ = writeln!(buf, "    <lastmod>{}</lastmod>", lastmod);
        }
        let _ = writeln!(buf, "    <changefreq>weekly</changefreq>");
        let _ = writeln!(buf, "    <priority>0.5</priority>");
        let _ = writeln!(buf, "  </url>");
    }

    buf.push_str("</urlset>\n");
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- generate_sitemap_xml ---

    #[test]
    fn sitemap_empty() {
        let xml = generate_sitemap_xml(&[], "https://example.com");
        assert!(xml.contains("<urlset"));
        assert!(xml.contains("</urlset>"));
        assert!(!xml.contains("<url>"));
    }

    #[test]
    fn sitemap_single_url() {
        let urls = vec![SitemapUrl {
            loc: "/about/".into(),
            lastmod: None,
        }];
        let xml = generate_sitemap_xml(&urls, "https://example.com");
        assert!(xml.contains("<loc>https://example.com/about/</loc>"));
        assert!(xml.contains("<changefreq>weekly</changefreq>"));
        assert!(xml.contains("<priority>0.5</priority>"));
        assert!(!xml.contains("<lastmod>"));
    }

    #[test]
    fn sitemap_with_lastmod() {
        let urls = vec![SitemapUrl {
            loc: "/posts/hello/".into(),
            lastmod: Some("2026-01-15T12:00:00Z".into()),
        }];
        let xml = generate_sitemap_xml(&urls, "https://example.com");
        assert!(xml.contains("<lastmod>2026-01-15T12:00:00Z</lastmod>"));
    }

    #[test]
    fn sitemap_multiple_urls() {
        let urls = vec![
            SitemapUrl { loc: "/".into(), lastmod: None },
            SitemapUrl { loc: "/about/".into(), lastmod: None },
            SitemapUrl { loc: "/posts/".into(), lastmod: None },
        ];
        let xml = generate_sitemap_xml(&urls, "https://example.com");
        assert!(xml.contains("<loc>https://example.com/</loc>"));
        assert!(xml.contains("<loc>https://example.com/about/</loc>"));
        assert!(xml.contains("<loc>https://example.com/posts/</loc>"));
    }

    // --- generate_robots_txt ---

    fn test_config(root_url: &str) -> SiteConfig {
        SiteConfig {
            root_url: root_url.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn robots_allow_all() {
        let config = test_config("https://example.com");
        let robots = SiteConfigRobots {
            enable: true,
            preset: Some(RobotsPreset::AllowAll),
            custom: None,
        };
        let txt = generate_robots_txt(&config, &robots, false);
        assert!(txt.contains("User-agent: *"));
        assert!(txt.contains("Allow: /"));
    }

    #[test]
    fn robots_block_all() {
        let config = test_config("https://example.com");
        let robots = SiteConfigRobots {
            enable: true,
            preset: Some(RobotsPreset::BlockAll),
            custom: None,
        };
        let txt = generate_robots_txt(&config, &robots, false);
        assert!(txt.contains("User-agent: *"));
        assert!(txt.contains("Disallow: /"));
    }

    #[test]
    fn robots_no_llms() {
        let config = test_config("https://example.com");
        let robots = SiteConfigRobots {
            enable: true,
            preset: Some(RobotsPreset::NoLlms),
            custom: None,
        };
        let txt = generate_robots_txt(&config, &robots, false);
        assert!(txt.contains("User-agent: GPTBot"));
        assert!(txt.contains("User-agent: ClaudeBot"));
        assert!(txt.contains("Disallow: /"));
    }

    #[test]
    fn robots_sitemap_line_when_enabled() {
        let config = test_config("https://example.com");
        let robots = SiteConfigRobots {
            enable: true,
            preset: Some(RobotsPreset::AllowAll),
            custom: None,
        };
        let txt = generate_robots_txt(&config, &robots, true);
        assert!(txt.contains("Sitemap: https://example.com/sitemap.xml"));
    }

    #[test]
    fn robots_no_sitemap_line_when_disabled() {
        let config = test_config("https://example.com");
        let robots = SiteConfigRobots {
            enable: true,
            preset: Some(RobotsPreset::AllowAll),
            custom: None,
        };
        let txt = generate_robots_txt(&config, &robots, false);
        assert!(!txt.contains("Sitemap:"));
    }

    #[test]
    fn robots_custom_file_fallback() {
        let config = test_config("https://example.com");
        let robots = SiteConfigRobots {
            enable: true,
            preset: None,
            custom: Some("/nonexistent/robots.txt".into()),
        };
        // Should warn and fall back to allow_all
        let txt = generate_robots_txt(&config, &robots, false);
        assert!(txt.contains("User-agent: *"));
        assert!(txt.contains("Allow: /"));
    }

    #[test]
    fn robots_no_preset_no_custom_defaults_to_allow() {
        let config = test_config("https://example.com");
        let robots = SiteConfigRobots {
            enable: true,
            preset: None,
            custom: None,
        };
        let txt = generate_robots_txt(&config, &robots, false);
        assert!(txt.contains("User-agent: *"));
        assert!(txt.contains("Allow: /"));
    }
}
