//! # Single-Page In-Flight Rules Runner
//!
//! Evaluates all document-level rules against a parsed HTML page and its HTTP transport telemetry.

pub mod canonical;
pub mod descriptions;
pub mod directives;
pub mod geo;
pub mod headings;
pub mod images;
pub mod international;
pub mod links;
pub mod mobile;
pub mod schema_val;
pub mod security;
pub mod status;
pub mod titles;

use crate::core::models::IssueFinding;
use crate::crawler::client::FetchResult;
use crate::parser::ParsedPage;

/// Runs all single-page in-flight technical SEO checks on the target URL.
pub fn evaluate_page_rules(page: &ParsedPage, fetch: &FetchResult) -> Vec<IssueFinding> {
    let mut issues = Vec::with_capacity(8);
    let url = &fetch.final_url;

    status::check_status(page, fetch, &mut issues);
    titles::check_titles(page, url, &mut issues);
    descriptions::check_descriptions(page, url, &mut issues);
    headings::check_headings(page, url, &mut issues);
    canonical::check_canonical(page, url, &mut issues);
    directives::check_directives(page, fetch, url, &mut issues);
    security::check_security(page, fetch, url, &mut issues);
    images::check_images(page, url, &mut issues);
    mobile::check_mobile(page, url, &mut issues);
    links::check_links(page, url, &mut issues);
    schema_val::check_schemas(page, url, &mut issues);
    geo::check_content_and_ai(page, fetch, url, &mut issues);
    international::check_international(
        page.html_lang.as_deref(),
        &page.hreflangs,
        url,
        &mut issues,
    );

    issues
}
