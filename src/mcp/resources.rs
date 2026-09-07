//! # MCP Resource Handlers
//!
//! Exposes read-only Model Context Protocol URI resources for LLM agents:
//! - `seo://crawls`: Lists all recent crawl sessions in SQLite.
//! - `seo://crawls/{session_id}/summary`: JSON summary of crawl metrics and health score.
//! - `seo://crawls/{session_id}/report`: Full token-efficient Markdown report.
//! - `seo://crawls/{session_id}/issues`: Raw JSON array of all issues.

use crate::error::{SeoError, SeoResult};
use crate::mcp::formatter::format_llm_markdown_report;
use crate::mcp::types::{ReadResourceResult, ResourceContents, ResourceDefinition};
use crate::storage::Database;

/// Returns definitions for all available MCP resources.
pub fn get_resource_definitions(db: &Database) -> Vec<ResourceDefinition> {
    let mut defs = vec![ResourceDefinition {
        uri: "seo://crawls".to_string(),
        name: "Recent Crawl Sessions".to_string(),
        description: Some("Lists all historical and active crawl sessions in SQLite".to_string()),
        mime_type: Some("application/json".to_string()),
    }];

    // Also advertise resources for recent crawl sessions
    if let Ok(crawls) = db.list_crawls() {
        for crawl in crawls.iter().take(20) {
            defs.push(ResourceDefinition {
                uri: format!("seo://crawls/{}/summary", crawl.session_id),
                name: format!("Crawl Summary: {}", crawl.session_id),
                description: Some(format!("Summary metrics for {}", crawl.target_url)),
                mime_type: Some("application/json".to_string()),
            });
            defs.push(ResourceDefinition {
                uri: format!("seo://crawls/{}/report", crawl.session_id),
                name: format!("Markdown Audit Report: {}", crawl.session_id),
                description: Some(format!(
                    "LLM-optimized audit report for {}",
                    crawl.target_url
                )),
                mime_type: Some("text/markdown".to_string()),
            });
            defs.push(ResourceDefinition {
                uri: format!("seo://crawls/{}/issues", crawl.session_id),
                name: format!("Audit Issues: {}", crawl.session_id),
                description: Some(format!(
                    "All detected issue findings for {}",
                    crawl.target_url
                )),
                mime_type: Some("application/json".to_string()),
            });
        }
    }

    defs
}

/// Reads the resource content for a given URI.
pub fn read_resource(uri: &str, db: &Database) -> SeoResult<ReadResourceResult> {
    let trimmed = uri.trim();

    if trimmed == "seo://crawls" {
        let crawls = db.list_crawls()?;
        let text = serde_json::to_string_pretty(&crawls)
            .map_err(|e| SeoError::Internal(format!("Failed to serialize crawl sessions: {e}")))?;
        return Ok(ReadResourceResult {
            contents: vec![ResourceContents {
                uri: trimmed.to_string(),
                mime_type: Some("application/json".to_string()),
                text,
            }],
        });
    }

    if let Some(rest) = trimmed.strip_prefix("seo://crawls/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2 {
            let session_id = parts[0];
            let sub_resource = parts[1];

            let crawl = db.get_crawl(session_id)?.ok_or_else(|| {
                SeoError::Storage(format!("Crawl session '{session_id}' not found"))
            })?;

            match sub_resource {
                "summary" => {
                    let text = serde_json::to_string_pretty(&crawl).map_err(|e| {
                        SeoError::Internal(format!("Failed to serialize crawl summary: {e}"))
                    })?;
                    return Ok(ReadResourceResult {
                        contents: vec![ResourceContents {
                            uri: trimmed.to_string(),
                            mime_type: Some("application/json".to_string()),
                            text,
                        }],
                    });
                }
                "report" => {
                    let issues = db.get_crawl_issues(session_id, None, None)?;
                    let text = format_llm_markdown_report(&crawl, &issues, 50, true);
                    return Ok(ReadResourceResult {
                        contents: vec![ResourceContents {
                            uri: trimmed.to_string(),
                            mime_type: Some("text/markdown".to_string()),
                            text,
                        }],
                    });
                }
                "issues" => {
                    let issues = db.get_crawl_issues(session_id, None, None)?;
                    let text = serde_json::to_string_pretty(&issues).map_err(|e| {
                        SeoError::Internal(format!("Failed to serialize crawl issues: {e}"))
                    })?;
                    return Ok(ReadResourceResult {
                        contents: vec![ResourceContents {
                            uri: trimmed.to_string(),
                            mime_type: Some("application/json".to_string()),
                            text,
                        }],
                    });
                }
                _ => {}
            }
        }
    }

    Err(SeoError::Internal(format!("Resource not found: '{uri}'")))
}
