//! # Screaming Frog Compatible CSV Report Exporter
//!
//! Generates four industry-standard CSV reports under `--format csv` for immediate
//! compatibility with SEO agency spreadsheet and data analysis workflows:
//! 1. `internal_all.csv`: Full internal crawled pages table with headings, metadata, and link counts.
//! 2. `issues_all.csv`: Defect triage log containing rule IDs, severities, affected URLs, and fix advice.
//! 3. `response_codes.csv`: HTTP status code distribution and redirect routing map.
//! 4. `external_all.csv`: Outbound hyperlink audit with anchor text, destination URLs, and nofollow directives.

use crate::core::models::RobotsFlags;
use crate::crawler::engine::CrawlResult;
use crate::error::{SeoError, SeoResult};
use crate::rules::catalog::get_rule;
use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};

/// Sanitizes text to prevent CSV/formula injection (CWE-1236) when opened in spreadsheet software.
///
/// If the text starts with formula triggers (`=`, `+`, `-`, `@`, `\t`, `\r`), prepends `'`.
pub fn sanitize_csv_cell(value: &str) -> Cow<'_, str> {
    let trimmed = value.trim_start();
    if trimmed.starts_with('=')
        || trimmed.starts_with('+')
        || trimmed.starts_with('-')
        || trimmed.starts_with('@')
        || trimmed.starts_with('\t')
        || trimmed.starts_with('\r')
    {
        Cow::Owned(format!("'{}", value))
    } else {
        Cow::Borrowed(value)
    }
}

/// Maps standard HTTP status codes to conventional reason phrases.
pub fn http_status_reason(status_code: u16) -> &'static str {
    match status_code {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        410 => "Gone",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => match status_code {
            200..=299 => "OK",
            300..=399 => "Redirect",
            400..=499 => "Client Error",
            500..=599 => "Server Error",
            _ => "Unknown",
        },
    }
}

/// Evaluates the indexability status of a crawled page in compliance with Screaming Frog standards.
pub fn evaluate_indexability(
    status_code: u16,
    robots_flags: RobotsFlags,
    url: &str,
    canonical_url: Option<&str>,
) -> (&'static str, String) {
    if !(200..=299).contains(&status_code) {
        let reason = http_status_reason(status_code);
        return ("Non-Indexable", format!("{reason} ({status_code})"));
    }

    if robots_flags.contains(RobotsFlags::NOINDEX) {
        return ("Non-Indexable", "noindex".to_string());
    }

    if let Some(canonical) = canonical_url {
        let trimmed_canonical = canonical.trim();
        let trimmed_url = url.trim();
        if !trimmed_canonical.is_empty() && trimmed_canonical != trimmed_url {
            return ("Non-Indexable", "Canonicalised".to_string());
        }
    }

    ("Indexable", String::new())
}

/// Exports the complete suite of four Screaming Frog compatible CSV files into `<output_dir>/csv/`.
///
/// Returns the list of generated file paths:
/// `[internal_all.csv, issues_all.csv, response_codes.csv, external_all.csv]`.
pub fn export_csv_suite(result: &CrawlResult, output_dir: &Path) -> SeoResult<Vec<PathBuf>> {
    let csv_dir = output_dir.join("csv");
    fs::create_dir_all(&csv_dir)
        .map_err(|e| SeoError::Internal(format!("Failed to create CSV export directory: {e}")))?;

    let internal_path = csv_dir.join("internal_all.csv");
    let issues_path = csv_dir.join("issues_all.csv");
    let response_codes_path = csv_dir.join("response_codes.csv");
    let external_path = csv_dir.join("external_all.csv");

    export_internal_all(result, &internal_path)?;
    export_issues_all(result, &issues_path)?;
    export_response_codes(result, &response_codes_path)?;
    export_external_all(result, &external_path)?;

    Ok(vec![
        internal_path,
        issues_path,
        response_codes_path,
        external_path,
    ])
}

/// Writes `internal_all.csv` containing all crawled pages and metadata.
fn export_internal_all(result: &CrawlResult, path: &Path) -> SeoResult<()> {
    let mut writer = csv::Writer::from_path(path)
        .map_err(|e| SeoError::Internal(format!("Failed to initialize internal_all.csv: {e}")))?;

    // 19 Screaming Frog standard columns
    writer
        .write_record([
            "Address",
            "Status Code",
            "Status",
            "Content Type",
            "Size (Bytes)",
            "Word Count",
            "Title 1",
            "Title 1 Length",
            "Meta Description 1",
            "Meta Description 1 Length",
            "H1-1",
            "H1-1 Length",
            "Canonical Link Element 1",
            "Indexability",
            "Indexability Status",
            "Inlinks",
            "Outlinks",
            "Crawl Depth",
            "Response Time (ms)",
        ])
        .map_err(|e| {
            SeoError::Internal(format!("Failed to write header to internal_all.csv: {e}"))
        })?;

    for page in &result.pages {
        let status_reason = http_status_reason(page.status_code);
        let (indexability, indexability_status) = evaluate_indexability(
            page.status_code,
            page.robots_flags,
            &page.url,
            page.canonical_url.as_deref(),
        );

        let inlinks = result.graph.in_degree(&page.url);
        let outlinks = result.graph.out_degree(&page.url);
        let h1_length = page
            .h1_primary
            .as_ref()
            .map(|h| h.chars().count())
            .unwrap_or(0);

        writer
            .write_record([
                page.url.as_str(),
                &page.status_code.to_string(),
                status_reason,
                page.content_type.as_str(),
                &page.size_bytes.to_string(),
                &page.word_count.to_string(),
                sanitize_csv_cell(page.title.as_deref().unwrap_or("")).as_ref(),
                &page.title_length.to_string(),
                sanitize_csv_cell(page.meta_description.as_deref().unwrap_or("")).as_ref(),
                &page.meta_desc_length.to_string(),
                sanitize_csv_cell(page.h1_primary.as_deref().unwrap_or("")).as_ref(),
                &h1_length.to_string(),
                page.canonical_url.as_deref().unwrap_or(""),
                indexability,
                &indexability_status,
                &inlinks.to_string(),
                &outlinks.to_string(),
                &page.crawl_depth.to_string(),
                &page.ttfb_ms.to_string(),
            ])
            .map_err(|e| {
                SeoError::Internal(format!("Failed to write row to internal_all.csv: {e}"))
            })?;
    }

    writer
        .flush()
        .map_err(|e| SeoError::Internal(format!("Failed to flush internal_all.csv: {e}")))?;

    Ok(())
}

/// Writes `issues_all.csv` summarizing all detected SEO findings.
fn export_issues_all(result: &CrawlResult, path: &Path) -> SeoResult<()> {
    let mut writer = csv::Writer::from_path(path)
        .map_err(|e| SeoError::Internal(format!("Failed to initialize issues_all.csv: {e}")))?;

    writer
        .write_record([
            "Issue Code",
            "Issue Name",
            "Severity",
            "Category",
            "URL",
            "Source URL",
            "Details",
            "Recommendation",
        ])
        .map_err(|e| {
            SeoError::Internal(format!("Failed to write header to issues_all.csv: {e}"))
        })?;

    for issue in &result.issues {
        let rule = get_rule(issue.code);

        writer
            .write_record([
                issue.code.as_str(),
                sanitize_csv_cell(issue.title.as_str()).as_ref(),
                issue.severity.as_str(),
                issue.category.as_str(),
                issue.target_url.as_str(),
                issue.source_page_url.as_deref().unwrap_or(""),
                sanitize_csv_cell(issue.message.as_str()).as_ref(),
                sanitize_csv_cell(rule.fix_advice).as_ref(),
            ])
            .map_err(|e| {
                SeoError::Internal(format!("Failed to write row to issues_all.csv: {e}"))
            })?;
    }

    writer
        .flush()
        .map_err(|e| SeoError::Internal(format!("Failed to flush issues_all.csv: {e}")))?;

    Ok(())
}

/// Writes `response_codes.csv` listing status codes, redirect destinations, and inlinks.
fn export_response_codes(result: &CrawlResult, path: &Path) -> SeoResult<()> {
    let mut writer = csv::Writer::from_path(path)
        .map_err(|e| SeoError::Internal(format!("Failed to initialize response_codes.csv: {e}")))?;

    writer
        .write_record([
            "URL",
            "Status Code",
            "Status",
            "Redirect URL",
            "Redirect Type",
            "Inlinks Count",
        ])
        .map_err(|e| {
            SeoError::Internal(format!("Failed to write header to response_codes.csv: {e}"))
        })?;

    for page in &result.pages {
        let status_reason = http_status_reason(page.status_code);
        let inlinks = result.graph.in_degree(&page.url);

        let (redirect_url, redirect_type) = match page.status_code {
            301 | 308 => (
                page.final_url
                    .as_deref()
                    .filter(|&u| u != page.url)
                    .unwrap_or(""),
                "Permanent",
            ),
            302 | 303 | 307 => (
                page.final_url
                    .as_deref()
                    .filter(|&u| u != page.url)
                    .unwrap_or(""),
                "Temporary",
            ),
            _ => ("", ""),
        };

        writer
            .write_record([
                page.url.as_str(),
                &page.status_code.to_string(),
                status_reason,
                redirect_url,
                redirect_type,
                &inlinks.to_string(),
            ])
            .map_err(|e| {
                SeoError::Internal(format!("Failed to write row to response_codes.csv: {e}"))
            })?;
    }

    writer
        .flush()
        .map_err(|e| SeoError::Internal(format!("Failed to flush response_codes.csv: {e}")))?;

    Ok(())
}

/// Writes `external_all.csv` auditing all external hyperlinks across the site.
fn export_external_all(result: &CrawlResult, path: &Path) -> SeoResult<()> {
    let mut writer = csv::Writer::from_path(path)
        .map_err(|e| SeoError::Internal(format!("Failed to initialize external_all.csv: {e}")))?;

    writer
        .write_record([
            "Source URL",
            "Destination URL",
            "Anchor Text",
            "Status Code",
            "Is Nofollow",
        ])
        .map_err(|e| {
            SeoError::Internal(format!("Failed to write header to external_all.csv: {e}"))
        })?;

    for page in &result.pages {
        for link in &page.links {
            if !link.is_internal {
                let status_str = link.status_code.map(|s| s.to_string()).unwrap_or_default();

                writer
                    .write_record([
                        link.source_url.as_str(),
                        link.target_url.as_str(),
                        sanitize_csv_cell(link.anchor_text.as_str()).as_ref(),
                        &status_str,
                        if link.is_nofollow { "true" } else { "false" },
                    ])
                    .map_err(|e| {
                        SeoError::Internal(format!("Failed to write row to external_all.csv: {e}"))
                    })?;
            }
        }
    }

    writer
        .flush()
        .map_err(|e| SeoError::Internal(format!("Failed to flush external_all.csv: {e}")))?;

    Ok(())
}
