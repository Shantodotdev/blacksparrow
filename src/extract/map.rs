//! `map`: list a site's URLs fast, from robots.txt sitemaps and the links on the start page,
//! without scraping every page.

use crate::core::url::{is_static_asset_url, normalize_url};
use crate::error::{SeoError, SeoResult};
use crate::extract::find::{query_terms, tokenize};
use crate::extract::paths::PathFilter;
use crate::extract::scrape::Scraper;
use crate::extract::types::{PageStatus, ScrapeOptions};
use hashbrown::HashSet;
use serde::{Deserialize, Serialize};

/// How sitemaps are used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SitemapMode {
    /// Sitemap URLs plus links found on pages.
    #[default]
    Include,
    /// Ignore sitemaps.
    Skip,
    /// Only sitemap URLs (and the start URL).
    Only,
}

/// Options for [`map_site`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MapOptions {
    /// Rank URLs by relevance to these words (URL path and link text).
    pub search: Option<String>,
    /// Only keep URLs whose path matches one of these patterns (see [`PathFilter`]).
    #[serde(alias = "includePaths")]
    pub include_paths: Vec<String>,
    /// Drop URLs whose path matches one of these patterns.
    #[serde(alias = "excludePaths")]
    pub exclude_paths: Vec<String>,
    /// Maximum URLs returned.
    pub limit: usize,
    /// Sitemap usage.
    pub sitemap: SitemapMode,
    /// Keep URLs on subdomains of the start host.
    #[serde(alias = "includeSubdomains")]
    pub include_subdomains: bool,
    /// Treat URLs that differ only in their query string as one.
    #[serde(alias = "ignoreQueryParameters")]
    pub ignore_query: bool,
}

impl Default for MapOptions {
    fn default() -> Self {
        Self {
            search: None,
            include_paths: Vec::new(),
            exclude_paths: Vec::new(),
            limit: 5000,
            sitemap: SitemapMode::Include,
            include_subdomains: false,
            ignore_query: true,
        }
    }
}

/// One URL found by [`map_site`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapLink {
    /// Normalized absolute URL.
    pub url: String,
    /// Link text or page title, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Where the URL came from: `start`, `sitemap` or `page`.
    pub source: String,
    /// Relevance to `search` (only set when searching).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

/// Result of [`map_site`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MapResult {
    /// The start URL.
    pub url: String,
    /// URLs found, start URL first unless a search reordered them.
    pub links: Vec<MapLink>,
    /// URLs that came from sitemaps (before filtering).
    pub sitemap_urls: usize,
    /// URLs skipped because robots.txt disallows them.
    pub robots_skipped: usize,
}

/// Decides whether a URL belongs to the crawl's site.
#[derive(Debug, Clone)]
pub(crate) struct SiteScope {
    host: String,
    base_domain: String,
    include_subdomains: bool,
}

impl SiteScope {
    pub(crate) fn new(seed: &url::Url, include_subdomains: bool) -> Self {
        let host = seed.host_str().unwrap_or("").to_ascii_lowercase();
        let base_domain = host.strip_prefix("www.").unwrap_or(&host).to_string();
        Self {
            host,
            base_domain,
            include_subdomains,
        }
    }

    pub(crate) fn contains(&self, url: &str) -> bool {
        let Ok(parsed) = url::Url::parse(url) else {
            return false;
        };
        if !matches!(parsed.scheme(), "http" | "https") {
            return false;
        }
        let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
        let www_twin = host.strip_prefix("www.").unwrap_or(&host) == self.base_domain;
        host == self.host
            || www_twin
            || (self.include_subdomains && host.ends_with(&format!(".{}", self.base_domain)))
    }
}

/// Normalizes a URL for deduplication, optionally dropping its query string.
pub(crate) fn canonical_key(url: &str, ignore_query: bool) -> Option<String> {
    let normalized = normalize_url(url).ok()?;
    if !ignore_query {
        return Some(normalized);
    }
    let mut parsed = url::Url::parse(&normalized).ok()?;
    parsed.set_query(None);
    Some(parsed.to_string())
}

/// Lists the URLs of the site at `url`.
///
/// # Errors
///
/// Returns an error for an invalid start URL or invalid path patterns.
pub async fn map_site(scraper: &Scraper, url: &str, opts: &MapOptions) -> SeoResult<MapResult> {
    let seed =
        url::Url::parse(url).map_err(|e| SeoError::Url(format!("Invalid URL '{url}': {e}")))?;
    let filter = PathFilter::new(&opts.include_paths, &opts.exclude_paths)?;
    let scope = SiteScope::new(&seed, opts.include_subdomains);
    let mut result = MapResult {
        url: url.to_string(),
        ..Default::default()
    };

    let mut candidates: Vec<MapLink> = Vec::new();
    let page = scraper
        .scrape_unsaved(
            url,
            &ScrapeOptions {
                only_main_content: false,
                ..Default::default()
            },
        )
        .await?;
    candidates.push(MapLink {
        url: url.to_string(),
        title: page.metadata.title.clone(),
        source: "start".to_string(),
        score: None,
    });
    if opts.sitemap != SitemapMode::Only && page.status == PageStatus::Ok {
        candidates.extend(page.outlinks.iter().map(|l| MapLink {
            url: l.url.clone(),
            title: (!l.text.is_empty()).then(|| l.text.clone()),
            source: "page".to_string(),
            score: None,
        }));
    }
    if opts.sitemap != SitemapMode::Skip {
        let (_, sitemap_urls, _) = crate::crawler::engine::discover_robots_and_sitemaps(
            scraper.client(),
            url,
            false,
            &[],
            opts.limit.min(u32::MAX as usize) as u32,
        )
        .await;
        result.sitemap_urls = sitemap_urls.len();
        candidates.extend(sitemap_urls.into_iter().map(|u| MapLink {
            url: u,
            title: None,
            source: "sitemap".to_string(),
            score: None,
        }));
    }

    let mut seen = HashSet::new();
    let mut links = Vec::new();
    for mut link in candidates {
        if !scope.contains(&link.url) || is_static_asset_url(&link.url) {
            continue;
        }
        let Some(key) = canonical_key(&link.url, opts.ignore_query) else {
            continue;
        };
        if link.source != "start" && !filter.allows(&key) {
            continue;
        }
        if !seen.insert(key.clone()) {
            // Fill in a missing title from a later sighting.
            if let (Some(title), Some(existing)) = (
                link.title.take(),
                links.iter_mut().find(|l: &&mut MapLink| l.url == key),
            ) {
                existing.title.get_or_insert(title);
            }
            continue;
        }
        if scraper.config().respect_robots && !scraper.robots_allows(&key).await {
            result.robots_skipped += 1;
            continue;
        }
        link.url = key;
        links.push(link);
    }
    if !filter.allows(url) && !opts.include_paths.is_empty() {
        links.retain(|l| l.source != "start");
    }

    if let Some(search) = opts.search.as_deref().filter(|s| !s.trim().is_empty()) {
        rank(&mut links, search);
    }
    links.truncate(opts.limit);
    result.links = links;
    Ok(result)
}

/// Orders links by how many search terms appear in their path (weight 2) and title (weight 1).
fn rank(links: &mut [MapLink], search: &str) {
    let terms = query_terms(search, true);
    if terms.is_empty() {
        return;
    }
    for link in links.iter_mut() {
        let path_tokens = url::Url::parse(&link.url)
            .map(|u| tokenize(u.path()))
            .unwrap_or_default();
        let title_tokens = link.title.as_deref().map(tokenize).unwrap_or_default();
        let mut score = 0.0;
        for term in &terms {
            if path_tokens.iter().any(|t| t.starts_with(term.as_str())) {
                score += 2.0;
            }
            if title_tokens.iter().any(|t| t.starts_with(term.as_str())) {
                score += 1.0;
            }
        }
        link.score = Some(score / (3.0 * terms.len() as f64));
    }
    links.sort_by(|a, b| {
        b.score
            .unwrap_or(0.0)
            .partial_cmp(&a.score.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}
