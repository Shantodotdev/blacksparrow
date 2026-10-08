//! `interact`: open a page in Chrome, run steps (click, type, press, scroll, wait), then read
//! it. Every call returns an accessibility snapshot of the interactive elements with short
//! references (`e12: button "Load more"`) that later steps can target.

use crate::crawler::render_pool::{RenderRequest, SnapshotElement};
use crate::error::SeoResult;
use crate::extract::scrape::{rendered_to_document, Scraper};
use crate::extract::types::{BrowserAction, OutputFormat, PageDocument, ScrapeOptions, WaitUntil};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// An interact request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InteractRequest {
    /// Page to open.
    pub url: String,
    /// Steps, run in order; a failing step stops the rest and is reported.
    #[serde(alias = "steps")]
    pub actions: Vec<BrowserAction>,
    /// Return the snapshot of interactive elements.
    pub snapshot: bool,
    /// Capture a full-page PNG (base64 in the document).
    pub screenshot: bool,
    /// Readiness condition before the first step.
    #[serde(alias = "waitUntil")]
    pub wait_until: WaitUntil,
    /// Selector that must appear before the first step.
    #[serde(alias = "waitFor")]
    pub wait_for: Option<String>,
    /// Overall time limit in milliseconds.
    #[serde(alias = "timeout")]
    pub timeout_ms: Option<u64>,
    /// How the final page is converted.
    #[serde(alias = "scrapeOptions")]
    pub scrape: ScrapeOptions,
}

impl Default for InteractRequest {
    fn default() -> Self {
        Self {
            url: String::new(),
            actions: Vec::new(),
            snapshot: true,
            screenshot: false,
            wait_until: WaitUntil::default(),
            wait_for: None,
            timeout_ms: None,
            scrape: ScrapeOptions::default(),
        }
    }
}

/// Result of an interact call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InteractResult {
    /// The page after the steps, converted like a scrape.
    pub document: PageDocument,
    /// Interactive elements after the steps.
    pub snapshot: Vec<SnapshotElement>,
    /// Step failures.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub action_errors: Vec<String>,
    /// Requests Chrome tried that the network guard blocked.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocked_requests: Vec<String>,
}

/// Runs the steps and returns the resulting page.
///
/// # Errors
///
/// Returns an error when the URL is refused (network guard, robots.txt), Chrome is not
/// available, or the page cannot be loaded in time.
pub async fn interact(scraper: &Scraper, req: &InteractRequest) -> SeoResult<InteractResult> {
    let request = RenderRequest {
        wait_until: req.wait_until.clone(),
        wait_for_selector: req.wait_for.clone(),
        wait_ms: None,
        actions: req.actions.clone(),
        screenshot: req.screenshot,
        snapshot: req.snapshot,
        timeout: req.timeout_ms.map(Duration::from_millis),
    };
    let output = scraper.render_raw(&req.url, &request).await?;
    let mut opts = req.scrape.clone();
    if req.screenshot && !opts.wants(OutputFormat::Screenshot) {
        opts.formats.push(OutputFormat::Screenshot);
    }
    let mut document = rendered_to_document(&req.url, &output, &opts, None)?;
    document.apply_formats(&opts);
    Ok(InteractResult {
        document,
        snapshot: output.snapshot,
        action_errors: output.action_errors,
        blocked_requests: output.blocked_requests,
    })
}
