//! # MCP Agent Tools
//!
//! Implements the 8 core Model Context Protocol tools for AI agent pair-programming:
//! 1. `seo_start_audit`: Non-blocking async crawl kickoff (< 1.0s).
//! 2. `seo_audit_status`: Live crawl telemetry & issue counter polling.
//! 3. `seo_get_markdown_report`: Token-efficient Markdown report generation (zero ANSI).
//! 4. `seo_quick_page_check`: Synchronous single-page audit (< 500ms).
//! 5. `seo_query_issues`: Filtered query over SQLite issues table.
//! 6. `seo_check_ai_readiness`: Standalone Generative Engine Optimization audit.
//! 7. `seo_validate_schema`: Google Rich Results structured data validation.
//! 8. `seo_cleanup_session`: Drops session and cascades records from SQLite.

use crate::core::config::CrawlConfig;
use crate::core::models::{IssueCategory, Severity};
use crate::core::url::normalize_url;
use crate::crawler::ai_check::audit_ai_readiness;
use crate::crawler::engine::run_crawl_with_options;
use crate::crawler::inspector::inspect_url_with_options;
use crate::error::SeoResult;
use crate::mcp::formatter::format_llm_markdown_report;
use crate::mcp::types::{CallToolResult, ToolDefinition};
use crate::rules::page::schema_val::validate_raw_schema;
use crate::storage::{CrawlSessionInit, Database, IssueFilterCriteria};
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Returns the static catalog of all 8 MCP tool definitions and their JSON schemas.
pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "seo_start_audit".to_string(),
            description: "Kicks off a background crawl and technical SEO audit for an entire website. Returns immediately with a session_id in < 1.0s.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "format": "uri",
                        "description": "The root or starting URL to crawl (e.g. 'https://example.com')."
                    },
                    "max_pages": {
                        "type": "integer",
                        "default": 500,
                        "minimum": 1,
                        "maximum": 50000,
                        "description": "Maximum number of pages to crawl."
                    },
                    "max_depth": {
                        "type": "integer",
                        "default": 5,
                        "minimum": 1,
                        "maximum": 20,
                        "description": "Maximum click depth from the start URL."
                    },
                    "render_js": {
                        "type": "boolean",
                        "default": false,
                        "description": "Enable headless Chrome CDP to render JavaScript and audit SPAs."
                    },
                    "respect_robots": {
                        "type": "boolean",
                        "default": true,
                        "description": "Whether to fetch and obey /robots.txt rules."
                    }
                },
                "required": ["url"]
            }),
        },
        ToolDefinition {
            name: "seo_audit_status".to_string(),
            description: "Polls the live progress and operational telemetry of an active or finished crawl session.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "The unique audit session ID returned by seo_start_audit."
                    }
                },
                "required": ["session_id"]
            }),
        },
        ToolDefinition {
            name: "seo_get_markdown_report".to_string(),
            description: "Generates a structured, token-efficient Markdown audit report engineered specifically for LLM context windows (zero ANSI codes, zero ASCII art, high signal-to-noise ratio).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "The audit session ID."
                    },
                    "top_issues_limit": {
                        "type": "integer",
                        "default": 20,
                        "description": "Maximum number of distinct issue types to summarize."
                    },
                    "include_urls": {
                        "type": "boolean",
                        "default": true,
                        "description": "Include sample affected URLs for each issue."
                    }
                },
                "required": ["session_id"]
            }),
        },
        ToolDefinition {
            name: "seo_quick_page_check".to_string(),
            description: "Performs an instant, synchronous audit of a single URL in < 500ms. Ideal for testing a specific landing page or verifying a code fix immediately.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "format": "uri",
                        "description": "Single URL to fetch and audit."
                    }
                },
                "required": ["url"]
            }),
        },
        ToolDefinition {
            name: "seo_query_issues".to_string(),
            description: "Queries specific issues from an audit database, filtered by severity, category, or URL pattern.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "The audit session ID."
                    },
                    "severity": {
                        "type": "string",
                        "enum": ["critical", "alert", "warning", "notice"],
                        "description": "Optional severity filter."
                    },
                    "category": {
                        "type": "string",
                        "description": "Optional category filter (e.g. 'canonicalization', 'security', 'indexability', 'titles')."
                    },
                    "url_pattern": {
                        "type": "string",
                        "description": "Optional URL substring or glob pattern (e.g. '/blog/')."
                    },
                    "limit": {
                        "type": "integer",
                        "default": 50,
                        "maximum": 500,
                        "description": "Number of records to return."
                    }
                },
                "required": ["session_id"]
            }),
        },
        ToolDefinition {
            name: "seo_check_ai_readiness".to_string(),
            description: "Audits whether a website is optimized for Generative Engine Optimization (GEO) and AI Search Engines (ChatGPT Search, Perplexity, Claude) and checks /llms.txt.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "format": "uri",
                        "description": "The website base URL."
                    }
                },
                "required": ["url"]
            }),
        },
        ToolDefinition {
            name: "seo_validate_schema".to_string(),
            description: "Validates a raw JSON-LD or Schema.org block against Google Rich Results eligibility rules.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "json_ld": {
                        "type": "string",
                        "description": "The raw JSON-LD string or object snippet."
                    },
                    "target_type": {
                        "type": "string",
                        "description": "Optional expected @type (e.g. 'Product', 'Article', 'FAQPage', 'BreadcrumbList')."
                    }
                },
                "required": ["json_ld"]
            }),
        },
        ToolDefinition {
            name: "seo_cleanup_session".to_string(),
            description: "Deletes audit records and artifacts from disk for a crawl session.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "The audit session ID to purge."
                    }
                },
                "required": ["session_id"]
            }),
        },
    ]
}

/// Dispatches an MCP tool call to its corresponding handler.
pub async fn execute_tool(
    name: &str,
    args: Option<&Value>,
    db: &Database,
) -> SeoResult<CallToolResult> {
    match name {
        "seo_start_audit" => tool_start_audit(args, db).await,
        "seo_audit_status" => tool_audit_status(args, db).await,
        "seo_get_markdown_report" => tool_get_markdown_report(args, db).await,
        "seo_quick_page_check" => tool_quick_page_check(args).await,
        "seo_query_issues" => tool_query_issues(args, db).await,
        "seo_check_ai_readiness" => tool_check_ai_readiness(args).await,
        "seo_validate_schema" => tool_validate_schema(args).await,
        "seo_cleanup_session" => tool_cleanup_session(args, db).await,
        _ => Ok(CallToolResult::error(format!("Unknown tool: '{name}'"))),
    }
}

async fn tool_start_audit(args: Option<&Value>, db: &Database) -> SeoResult<CallToolResult> {
    let args_val = args.cloned().unwrap_or_else(|| json!({}));
    let raw_url = match args_val["url"].as_str() {
        Some(u) if !u.trim().is_empty() => u.trim(),
        _ => return Ok(CallToolResult::error("Missing required parameter 'url'")),
    };

    let target_url = match normalize_url(raw_url) {
        Ok(u) => u,
        Err(e) => return Ok(CallToolResult::error(format!("Invalid URL: {e}"))),
    };

    let max_pages = args_val["max_pages"].as_u64().unwrap_or(500) as u32;
    let max_depth = args_val["max_depth"].as_u64().unwrap_or(5) as u16;
    let respect_robots = args_val["respect_robots"].as_bool().unwrap_or(true);
    let render_js = args_val["render_js"].as_bool().unwrap_or(false);

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let session_id = format!("crawl_{ts}");

    // Register initial session record in SQLite
    let init = CrawlSessionInit {
        session_id: session_id.clone(),
        target_url: target_url.clone(),
        max_pages,
        max_depth,
        respect_robots,
        render_js,
    };
    db.init_crawl_session(&init)?;

    // Spawn non-blocking background Tokio task
    let background_db = db.clone();
    let background_url = target_url.clone();
    let background_session_id = session_id.clone();
    tokio::spawn(async move {
        let run_result: SeoResult<crate::crawler::engine::CrawlResult> = async {
            let mut config = CrawlConfig::new(&background_url)?;
            config.session_id = Some(background_session_id.clone());
            config.max_pages = max_pages;
            config.max_depth = max_depth;
            config.respect_robots = respect_robots;
            config.render_js = render_js;
            config.quiet = true; // headless background task

            let (writer_handle, writer_task) = background_db.spawn_writer(
                &background_session_id,
                20,
                Duration::from_millis(500),
            )?;

            let crawl_res = run_crawl_with_options(&config, None, Some(writer_handle), None).await;
            let _ = writer_task.await;

            crawl_res
        }
        .await;

        match run_result {
            Ok(res) if !res.pages.is_empty() => {
                let errors = res
                    .issues
                    .iter()
                    .filter(|i| i.severity == Severity::Critical)
                    .count() as u32;
                let alerts = res
                    .issues
                    .iter()
                    .filter(|i| i.severity == Severity::Alert)
                    .count() as u32;
                let warnings = res
                    .issues
                    .iter()
                    .filter(|i| i.severity == Severity::Warning)
                    .count() as u32;

                let _ = background_db.update_crawl_status(
                    &background_session_id,
                    "completed",
                    None,
                    res.pages.len() as u32,
                    errors,
                    alerts,
                    warnings,
                    Some(res.health_score),
                );
            }
            _ => {
                let _ = background_db.update_crawl_status(
                    &background_session_id,
                    "failed",
                    None,
                    0,
                    0,
                    0,
                    0,
                    None,
                );
            }
        }
    });

    let res = json!({
        "session_id": session_id,
        "status": "queued",
        "target_url": raw_url,
        "message": "Audit started in background. Poll 'seo_audit_status' with session_id to monitor progress.",
        "poll_interval_seconds": 15
    });

    Ok(CallToolResult::success_json(&res))
}

async fn tool_audit_status(args: Option<&Value>, db: &Database) -> SeoResult<CallToolResult> {
    let args_val = args.cloned().unwrap_or_else(|| json!({}));
    let session_id = match args_val["session_id"].as_str() {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => {
            return Ok(CallToolResult::error(
                "Missing required parameter 'session_id'",
            ))
        }
    };

    let crawl = match db.get_crawl(session_id)? {
        Some(c) => c,
        None => {
            return Ok(CallToolResult::error(format!(
                "Session '{session_id}' not found in database"
            )))
        }
    };

    let is_complete =
        crawl.status == "completed" || crawl.status == "interrupted" || crawl.status == "failed";

    let res = json!({
        "session_id": session_id,
        "status": crawl.status,
        "pages_crawled": crawl.total_pages_crawled,
        "pages_discovered": crawl.total_links_discovered,
        "current_delay_ms": 0,
        "p95_ttfb_ms": crawl.p95_ttfb_ms,
        "error_rate_pct": 0.0,
        "issues_count": {
            "critical": crawl.total_errors,
            "alert": crawl.total_alerts,
            "warning": crawl.total_warnings,
            "notice": crawl.total_notices
        },
        "health_score": crawl.health_score,
        "is_complete": is_complete
    });

    Ok(CallToolResult::success_json(&res))
}

async fn tool_get_markdown_report(
    args: Option<&Value>,
    db: &Database,
) -> SeoResult<CallToolResult> {
    let args_val = args.cloned().unwrap_or_else(|| json!({}));
    let session_id = match args_val["session_id"].as_str() {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => {
            return Ok(CallToolResult::error(
                "Missing required parameter 'session_id'",
            ))
        }
    };

    let crawl = match db.get_crawl(session_id)? {
        Some(c) => c,
        None => {
            return Ok(CallToolResult::error(format!(
                "Session '{session_id}' not found in database"
            )))
        }
    };

    let top_limit = args_val["top_issues_limit"].as_u64().unwrap_or(20) as usize;
    let include_urls = args_val["include_urls"].as_bool().unwrap_or(true);

    let issues = db.get_crawl_issues(session_id, None, None)?;
    let report_md = format_llm_markdown_report(&crawl, &issues, top_limit, include_urls);

    Ok(CallToolResult::success_text(report_md))
}

async fn tool_quick_page_check(args: Option<&Value>) -> SeoResult<CallToolResult> {
    let args_val = args.cloned().unwrap_or_else(|| json!({}));
    let raw_url = match args_val["url"].as_str() {
        Some(u) if !u.trim().is_empty() => u.trim(),
        _ => return Ok(CallToolResult::error("Missing required parameter 'url'")),
    };

    let (parsed_page, fetch_result, issues) = match inspect_url_with_options(
        raw_url,
        crate::core::branding::MCP_BOT_USER_AGENT,
        Duration::from_secs(15),
        vec![],
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            return Ok(CallToolResult::error(format!(
                "Failed to fetch '{raw_url}': {e}"
            )))
        }
    };

    let issues_json: Vec<Value> = issues
        .iter()
        .map(|i| {
            json!({
                "code": i.code.as_str(),
                "severity": i.severity.as_str(),
                "message": i.message
            })
        })
        .collect();

    let is_indexable = !parsed_page
        .robots_flags
        .contains(crate::core::models::RobotsFlags::NOINDEX)
        && fetch_result.status_code == 200;

    let res = json!({
        "url": fetch_result.final_url,
        "status_code": fetch_result.status_code,
        "ttfb_ms": fetch_result.ttfb_ms,
        "title": parsed_page.title.as_deref().unwrap_or(""),
        "meta_description": parsed_page.meta_description.as_deref().unwrap_or(""),
        "h1": parsed_page.h1_primary.as_deref().unwrap_or(""),
        "canonical_url": parsed_page.canonical_url.as_deref().unwrap_or(""),
        "word_count": parsed_page.word_count,
        "is_indexable": is_indexable,
        "issues_detected": issues_json
    });

    Ok(CallToolResult::success_json(&res))
}

async fn tool_query_issues(args: Option<&Value>, db: &Database) -> SeoResult<CallToolResult> {
    let args_val = args.cloned().unwrap_or_else(|| json!({}));
    let session_id = match args_val["session_id"].as_str() {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => {
            return Ok(CallToolResult::error(
                "Missing required parameter 'session_id'",
            ))
        }
    };

    let sev = args_val["severity"]
        .as_str()
        .and_then(|s| match s.to_lowercase().as_str() {
            "critical" => Some(Severity::Critical),
            "alert" => Some(Severity::Alert),
            "warning" => Some(Severity::Warning),
            "notice" => Some(Severity::Notice),
            _ => None,
        });

    let cat = args_val["category"]
        .as_str()
        .and_then(IssueCategory::from_str_name);
    let url_pattern = args_val["url_pattern"].as_str();
    let limit = args_val["limit"].as_u64().unwrap_or(50) as usize;

    let criteria = IssueFilterCriteria {
        severity: sev,
        category: cat,
        code: None,
        url_substring: url_pattern,
        limit,
        offset: 0,
    };

    let total = db.count_issues_filtered(session_id, &criteria)?;
    let issues = db.query_issues_filtered(session_id, &criteria)?;

    let issues_json: Vec<Value> = issues
        .iter()
        .map(|i| {
            json!({
                "code": i.code.as_str(),
                "severity": i.severity.as_str(),
                "category": i.category.as_str(),
                "target_url": i.target_url,
                "message": i.message,
                "source_page_url": i.source_page_url
            })
        })
        .collect();

    let res = json!({
        "total_matching": total,
        "issues": issues_json
    });

    Ok(CallToolResult::success_json(&res))
}

async fn tool_check_ai_readiness(args: Option<&Value>) -> SeoResult<CallToolResult> {
    let args_val = args.cloned().unwrap_or_else(|| json!({}));
    let raw_url = match args_val["url"].as_str() {
        Some(u) if !u.trim().is_empty() => u.trim(),
        _ => return Ok(CallToolResult::error("Missing required parameter 'url'")),
    };

    let report = match audit_ai_readiness(
        raw_url,
        crate::core::branding::MCP_BOT_USER_AGENT,
        Duration::from_secs(15),
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            return Ok(CallToolResult::error(format!(
                "AI readiness check failed: {e}"
            )))
        }
    };

    let res = json!({
        "target_url": report.base_url,
        "robots_found": report.robots_found,
        "llms_txt_found": report.llms_txt_found,
        "llms_txt_summary": report.llms_txt_summary,
        "llms_full_txt_found": report.llms_full_txt_found,
        "ai_crawler_access": {
            "retrieval_citation_bots": report.retrieval_bots,
            "foundation_training_bots": report.training_bots,
        },
        "citation_risk": report.citation_search_risk.to_string(),
        "recommendations": report.recommendations,
    });

    Ok(CallToolResult::success_json(&res))
}

async fn tool_validate_schema(args: Option<&Value>) -> SeoResult<CallToolResult> {
    let args_val = args.cloned().unwrap_or_else(|| json!({}));
    let json_ld = match args_val["json_ld"].as_str() {
        Some(j) if !j.trim().is_empty() => j.trim(),
        _ => {
            return Ok(CallToolResult::error(
                "Missing required parameter 'json_ld'",
            ))
        }
    };

    let target_type = args_val["target_type"].as_str();
    let outcome = validate_raw_schema(json_ld, target_type)?;

    let res = json!({
        "is_valid_json": outcome.is_valid_json,
        "detected_type": outcome.detected_type,
        "is_rich_result_eligible": outcome.is_rich_result_eligible,
        "missing_required_fields": outcome.missing_required_fields,
        "missing_recommended_fields": outcome.missing_recommended_fields,
        "error_message": outcome.error_message
    });

    Ok(CallToolResult::success_json(&res))
}

async fn tool_cleanup_session(args: Option<&Value>, db: &Database) -> SeoResult<CallToolResult> {
    let args_val = args.cloned().unwrap_or_else(|| json!({}));
    let session_id = match args_val["session_id"].as_str() {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => {
            return Ok(CallToolResult::error(
                "Missing required parameter 'session_id'",
            ))
        }
    };

    let purged = db.delete_crawl(session_id)?;

    let res = json!({
        "session_id": session_id,
        "purged": purged,
        "records_freed": if purged { 1 } else { 0 }
    });

    Ok(CallToolResult::success_json(&res))
}
