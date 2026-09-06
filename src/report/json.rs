//! # JSON Report Exporter
//!
//! Generates comprehensive machine-readable JSON reports for CI/CD pipeline integration,
//! data warehouse ingestion, and programmatic processing.

use crate::core::models::{
    HreflangTag, ImageResource, IssueFinding, PageReport, RobotsFlags, SchemaRecord,
};
use crate::crawler::engine::CrawlResult;
use crate::error::{SeoError, SeoResult};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Compact machine-readable JSON representation of a crawled page report.
///
/// Excludes the unbounded raw vector of individual [`crate::core::models::DiscoveredLink`] records
/// (which are exported losslessly into CSVs and SQLite) in favor of aggregated link counts,
/// preventing multi-hundred megabyte JSON bloat on large websites with mega-navigation.
#[derive(Debug, Clone, Serialize)]
pub struct PageJsonReport<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub crawl_id: &'a str,
    pub url: &'a str,
    pub url_hash: u64,
    pub final_url: Option<&'a str>,
    pub status_code: u16,
    pub content_type: &'a str,
    pub size_bytes: u32,
    pub ttfb_ms: u32,
    pub crawl_depth: u16,

    pub title: Option<&'a str>,
    pub title_length: u16,
    pub meta_description: Option<&'a str>,
    pub meta_desc_length: u16,
    pub canonical_url: Option<&'a str>,
    pub html_lang: Option<&'a str>,
    pub charset: Option<&'a str>,
    pub viewport: Option<&'a str>,

    pub robots_flags: RobotsFlags,
    pub is_sitemap_url: bool,
    pub is_internal: bool,

    pub h1_primary: Option<&'a str>,
    pub h1_count: u16,
    pub h2_headings: &'a [String],
    pub h3_headings: &'a [String],

    pub word_count: u32,
    pub content_hash: u64,
    pub simhash: u64,
    pub is_soft_404: bool,
    pub has_lorem_ipsum: bool,

    pub is_https: bool,
    pub has_hsts: bool,
    pub has_csp: bool,
    pub has_x_frame: bool,
    pub has_x_content_type: bool,
    pub mixed_content_count: u16,

    pub links_count: usize,
    pub internal_links_count: usize,
    pub external_links_count: usize,

    pub images: &'a [ImageResource],
    pub schemas: &'a [SchemaRecord],
    pub hreflangs: &'a [HreflangTag],
    pub issues: &'a [IssueFinding],
}

impl<'a> From<&'a PageReport> for PageJsonReport<'a> {
    fn from(p: &'a PageReport) -> Self {
        let (internal_links, external_links) =
            p.links.iter().fold((0usize, 0usize), |(int, ext), link| {
                if link.is_internal {
                    (int + 1, ext)
                } else {
                    (int, ext + 1)
                }
            });

        Self {
            id: p.id,
            crawl_id: p.crawl_id.as_str(),
            url: &p.url,
            url_hash: p.url_hash,
            final_url: p.final_url.as_deref(),
            status_code: p.status_code,
            content_type: p.content_type.as_str(),
            size_bytes: p.size_bytes,
            ttfb_ms: p.ttfb_ms,
            crawl_depth: p.crawl_depth,
            title: p.title.as_deref(),
            title_length: p.title_length,
            meta_description: p.meta_description.as_deref(),
            meta_desc_length: p.meta_desc_length,
            canonical_url: p.canonical_url.as_deref(),
            html_lang: p.html_lang.as_deref(),
            charset: p.charset.as_deref(),
            viewport: p.viewport.as_deref(),
            robots_flags: p.robots_flags,
            is_sitemap_url: p.is_sitemap_url,
            is_internal: p.is_internal,
            h1_primary: p.h1_primary.as_deref(),
            h1_count: p.h1_count,
            h2_headings: &p.h2_headings,
            h3_headings: &p.h3_headings,
            word_count: p.word_count,
            content_hash: p.content_hash,
            simhash: p.simhash,
            is_soft_404: p.is_soft_404,
            has_lorem_ipsum: p.has_lorem_ipsum,
            is_https: p.is_https,
            has_hsts: p.has_hsts,
            has_csp: p.has_csp,
            has_x_frame: p.has_x_frame,
            has_x_content_type: p.has_x_content_type,
            mixed_content_count: p.mixed_content_count,
            links_count: p.links.len(),
            internal_links_count: internal_links,
            external_links_count: external_links,
            images: &p.images,
            schemas: &p.schemas,
            hreflangs: &p.hreflangs,
            issues: &p.issues,
        }
    }
}

#[derive(Serialize)]
struct JsonReportPayload<'a> {
    target_url: &'a str,
    health_score: u8,
    duration_secs: f64,
    pages_count: usize,
    internal_links_count: usize,
    aimd_delay_ms: u64,
    sitemap_urls_count: usize,
    issues_count: usize,
    issues: &'a [crate::core::models::IssueFinding],
    pages: Vec<PageJsonReport<'a>>,
}

/// Exports the complete crawl result as a structured JSON file.
pub fn export_json_report(result: &CrawlResult, output_dir: &Path) -> SeoResult<PathBuf> {
    fs::create_dir_all(output_dir)
        .map_err(|e| SeoError::Internal(format!("Failed to create output directory: {}", e)))?;

    let host = url::Url::parse(&result.target_url)
        .map(|u| u.host_str().unwrap_or("site").replace('.', "_"))
        .unwrap_or_else(|_| "site_audit".to_string());

    let filename = format!("{}_audit.json", host);
    let output_path = output_dir.join(filename);

    let pages: Vec<PageJsonReport> = result.pages.iter().map(PageJsonReport::from).collect();

    let payload = JsonReportPayload {
        target_url: &result.target_url,
        health_score: result.health_score,
        duration_secs: result.duration.as_secs_f64(),
        pages_count: result.pages.len(),
        internal_links_count: result.graph.edge_count(),
        aimd_delay_ms: result.aimd_delay_ms,
        sitemap_urls_count: result.sitemap_urls.len(),
        issues_count: result.issues.len(),
        issues: &result.issues,
        pages,
    };

    let json_bytes = serde_json::to_vec_pretty(&payload)
        .map_err(|e| SeoError::Internal(format!("Failed to serialize JSON report: {}", e)))?;

    fs::write(&output_path, json_bytes)
        .map_err(|e| SeoError::Internal(format!("Failed to write JSON report: {}", e)))?;

    Ok(output_path)
}
