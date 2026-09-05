//! # JSON Report Exporter
//!
//! Generates comprehensive machine-readable JSON reports for CI/CD pipeline integration,
//! data warehouse ingestion, and programmatic processing.

use crate::crawler::engine::CrawlResult;
use crate::error::{SeoError, SeoResult};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

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
    pages: &'a [crate::core::models::PageReport],
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
        pages: &result.pages,
    };

    let json_bytes = serde_json::to_vec_pretty(&payload)
        .map_err(|e| SeoError::Internal(format!("Failed to serialize JSON report: {}", e)))?;

    fs::write(&output_path, json_bytes)
        .map_err(|e| SeoError::Internal(format!("Failed to write JSON report: {}", e)))?;

    Ok(output_path)
}
