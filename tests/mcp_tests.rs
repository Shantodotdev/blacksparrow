//! # Model Context Protocol (MCP) Integration Tests
//!
//! Validates the native JSON-RPC 2.0 stdio MCP server:
//! - Protocol negotiation (`initialize`, `ping`).
//! - Complete catalog of 8 core tools (`tools/list`).
//! - Synchronous single-page auditing (`seo_quick_page_check`).
//! - Non-blocking asynchronous audit start (`seo_start_audit`) & telemetry polling (`seo_audit_status`).
//! - Token-efficient Markdown report generation (`seo_get_markdown_report`) with zero ANSI codes.
//! - Filtered issue queries (`seo_query_issues`).
//! - Generative Engine Optimization readiness (`seo_check_ai_readiness`).
//! - Schema.org Rich Results validation (`seo_validate_schema`).
//! - Session cleanup (`seo_cleanup_session`).
//! - MCP Resources (`resources/list`, `resources/read`).

use blacksparrow::core::models::{IssueCategory, IssueFinding, RuleId, Severity};
use blacksparrow::mcp::protocol::{handle_jsonrpc_request, McpContext};
use blacksparrow::storage::{CrawlSessionInit, Database};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

static MCP_TEST_COUNTER: AtomicUsize = AtomicUsize::new(2000);

fn unique_test_db_path() -> PathBuf {
    let id = MCP_TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut p = std::env::temp_dir();
    p.push(format!("seolens_mcp_test_{}_{}.db", std::process::id(), id));
    p
}

#[tokio::test]
async fn test_mcp_initialize_and_ping() {
    let db_path = unique_test_db_path();
    let ctx = Arc::new(McpContext::new(Some(db_path)).expect("McpContext init"));

    // 1. Initialize request
    let init_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-agent",
                "version": "1.0.0"
            }
        }
    });

    let resp_str = handle_jsonrpc_request(&init_req.to_string(), &ctx).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_str).expect("Valid JSON response");

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(resp["result"]["serverInfo"]["name"], "blacksparrow");
    assert!(resp["result"]["capabilities"]["tools"].is_object());
    assert!(resp["result"]["capabilities"]["resources"].is_object());

    // 2. Ping request
    let ping_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "ping"
    });

    let ping_resp_str = handle_jsonrpc_request(&ping_req.to_string(), &ctx).await;
    let ping_resp: serde_json::Value =
        serde_json::from_str(&ping_resp_str).expect("Valid JSON response");
    assert_eq!(ping_resp["jsonrpc"], "2.0");
    assert_eq!(ping_resp["id"], 2);
    assert_eq!(ping_resp["result"], serde_json::json!({}));
}

#[tokio::test]
async fn test_mcp_tools_list_schema() {
    let db_path = unique_test_db_path();
    let ctx = Arc::new(McpContext::new(Some(db_path)).expect("McpContext init"));

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/list"
    });

    let resp_str = handle_jsonrpc_request(&req.to_string(), &ctx).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_str).expect("Valid JSON response");

    let tools = resp["result"]["tools"].as_array().expect("Tools array");
    assert_eq!(tools.len(), 8, "Must expose exactly 8 core MCP tools");

    let tool_names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().expect("Tool name"))
        .collect();

    assert!(tool_names.contains(&"seo_start_audit"));
    assert!(tool_names.contains(&"seo_audit_status"));
    assert!(tool_names.contains(&"seo_get_markdown_report"));
    assert!(tool_names.contains(&"seo_quick_page_check"));
    assert!(tool_names.contains(&"seo_query_issues"));
    assert!(tool_names.contains(&"seo_check_ai_readiness"));
    assert!(tool_names.contains(&"seo_validate_schema"));
    assert!(tool_names.contains(&"seo_cleanup_session"));

    // Verify schemas have inputSchema with properties
    for tool in tools {
        assert!(tool["inputSchema"]["type"] == "object");
        assert!(tool["description"].is_string());
    }
}

#[tokio::test]
async fn test_mcp_quick_page_check_sync() {
    let server = MockServer::start().await;
    let html = r#"<!DOCTYPE html>
<html>
<head>
    <title>Pricing Plans - Fast SaaS</title>
    <meta name="description" content="Transparent pricing for engineering teams.">
    <link rel="canonical" href="https://example.com/pricing">
</head>
<body>
    <h1>Pricing</h1>
    <p>Sign up today and scale with us.</p>
    <img src="/logo.png">
</body>
</html>"#;

    Mock::given(method("GET"))
        .and(path("/pricing"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(html)
                .insert_header("content-type", "text/html"),
        )
        .mount(&server)
        .await;

    let db_path = unique_test_db_path();
    let ctx = Arc::new(McpContext::new(Some(db_path)).expect("McpContext init"));

    let test_url = format!("{}/pricing", server.uri());
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 20,
        "method": "tools/call",
        "params": {
            "name": "seo_quick_page_check",
            "arguments": {
                "url": test_url
            }
        }
    });

    let resp_str = handle_jsonrpc_request(&req.to_string(), &ctx).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_str).expect("Valid JSON response");
    assert_eq!(resp["id"], 20);

    let content_text = resp["result"]["content"][0]["text"]
        .as_str()
        .expect("Content text");
    let result_obj: serde_json::Value =
        serde_json::from_str(content_text).expect("Parsed tool JSON");

    assert_eq!(result_obj["status_code"], 200);
    assert_eq!(result_obj["title"], "Pricing Plans - Fast SaaS");
    assert_eq!(
        result_obj["meta_description"],
        "Transparent pricing for engineering teams."
    );
    assert_eq!(result_obj["h1"], "Pricing");
    assert_eq!(result_obj["canonical_url"], "https://example.com/pricing");
    assert_eq!(result_obj["is_indexable"], true);

    // Image missing alt attribute should be flagged
    let issues = result_obj["issues_detected"]
        .as_array()
        .expect("Issues array");
    let has_img_alt_issue = issues
        .iter()
        .any(|i| i["code"].as_str() == Some("WARN_IMAGE_MISSING_ALT"));
    assert!(
        has_img_alt_issue,
        "Should detect missing alt tag on /logo.png"
    );
}

#[tokio::test]
async fn test_mcp_start_audit_non_blocking_and_status() {
    let server = MockServer::start().await;
    let html =
        r#"<!DOCTYPE html><html><head><title>Root</title></head><body><h1>Root</h1></body></html>"#;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(html))
        .mount(&server)
        .await;

    let db_path = unique_test_db_path();
    let ctx = Arc::new(McpContext::new(Some(db_path.clone())).expect("McpContext init"));

    // 1. Start audit
    let start_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 30,
        "method": "tools/call",
        "params": {
            "name": "seo_start_audit",
            "arguments": {
                "url": server.uri(),
                "max_pages": 10,
                "max_depth": 2
            }
        }
    });

    let start_time = std::time::Instant::now();
    let resp_str = handle_jsonrpc_request(&start_req.to_string(), &ctx).await;
    let elapsed = start_time.elapsed();

    // Must return in under 1.0 second (non-blocking async guarantee)
    assert!(
        elapsed.as_millis() < 1000,
        "seo_start_audit took {}ms, exceeding 1000ms SLA",
        elapsed.as_millis()
    );

    let resp: serde_json::Value = serde_json::from_str(&resp_str).expect("Valid JSON response");
    let content_text = resp["result"]["content"][0]["text"]
        .as_str()
        .expect("Content text");
    let start_res: serde_json::Value =
        serde_json::from_str(content_text).expect("Parsed tool JSON");

    let session_id = start_res["session_id"]
        .as_str()
        .expect("Session ID string")
        .to_string();
    assert_eq!(start_res["status"], "queued");
    assert_eq!(start_res["target_url"], server.uri());
    assert_eq!(start_res["poll_interval_seconds"], 15);

    // 2. Poll audit status
    let status_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 31,
        "method": "tools/call",
        "params": {
            "name": "seo_audit_status",
            "arguments": {
                "session_id": session_id
            }
        }
    });

    let status_resp_str = handle_jsonrpc_request(&status_req.to_string(), &ctx).await;
    let status_resp: serde_json::Value =
        serde_json::from_str(&status_resp_str).expect("Valid JSON response");
    let status_text = status_resp["result"]["content"][0]["text"]
        .as_str()
        .expect("Content text");
    let status_res: serde_json::Value =
        serde_json::from_str(status_text).expect("Parsed status JSON");

    assert_eq!(status_res["session_id"], session_id);
    assert!(status_res["status"].is_string());
    assert!(status_res["issues_count"].is_object());
}

#[tokio::test]
async fn test_mcp_get_markdown_report_token_efficiency_zero_ansi() {
    let db_path = unique_test_db_path();
    let db = Database::open(&db_path).expect("Open test database");
    let session = "crawl_mcp_token_test";

    // Setup crawl session and issues
    db.init_crawl_session(&CrawlSessionInit {
        session_id: session.to_string(),
        target_url: "https://agent-efficiency.test".to_string(),
        max_pages: 50,
        max_depth: 3,
        respect_robots: true,
        render_js: false,
    })
    .expect("Init session");

    let issues = vec![
        IssueFinding {
            code: RuleId::ErrHttp4xxClientError,
            category: IssueCategory::Indexability,
            severity: Severity::Critical,
            title: "Dead Internal 404 Page".into(),
            message: "HTTP 404 Not Found returned".into(),
            target_url: "https://agent-efficiency.test/dead-link".into(),
            source_page_url: Some("https://agent-efficiency.test/home".into()),
        },
        IssueFinding {
            code: RuleId::WarnTitleTooLong,
            category: IssueCategory::TitleMetadata,
            severity: Severity::Warning,
            title: "Title Exceeds 60 Characters".into(),
            message: "Length is 82 chars".into(),
            target_url: "https://agent-efficiency.test/product".into(),
            source_page_url: None,
        },
    ];
    db.save_issue_batch(session, &issues).expect("Save issues");

    let ctx = Arc::new(McpContext::new(Some(db_path)).expect("McpContext init"));

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 40,
        "method": "tools/call",
        "params": {
            "name": "seo_get_markdown_report",
            "arguments": {
                "session_id": session,
                "top_issues_limit": 10,
                "include_urls": true
            }
        }
    });

    let resp_str = handle_jsonrpc_request(&req.to_string(), &ctx).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_str).expect("Valid JSON response");
    let md_report = resp["result"]["content"][0]["text"]
        .as_str()
        .expect("Report markdown");

    // Strict LLM Token-Efficiency Guarantees:
    // 1. Zero ANSI escape sequences
    assert!(
        !md_report.contains("\x1b["),
        "LLM report must contain ZERO ANSI escape codes"
    );
    // 2. Zero ASCII-art banners
    assert!(
        !md_report.contains("██"),
        "LLM report must not contain decorative ASCII art banners"
    );
    // 3. Clear Markdown structure
    assert!(md_report.contains("# Technical SEO Audit"));
    assert!(md_report.contains("ERR_HTTP_4XX_CLIENT_ERROR"));
    assert!(md_report.contains("WARN_TITLE_TOO_LONG"));
    assert!(md_report.contains("Action for Agent"));
}

#[tokio::test]
async fn test_mcp_query_issues_and_cleanup() {
    let db_path = unique_test_db_path();
    let db = Database::open(&db_path).expect("Open test database");
    let session = "crawl_mcp_cleanup_test";

    db.init_crawl_session(&CrawlSessionInit {
        session_id: session.to_string(),
        target_url: "https://cleanup.test".to_string(),
        max_pages: 10,
        max_depth: 2,
        respect_robots: true,
        render_js: false,
    })
    .expect("Init session");

    let issues = vec![IssueFinding {
        code: RuleId::ErrHttp5xxServerError,
        category: IssueCategory::HttpTransport,
        severity: Severity::Critical,
        title: "Server 500 Error".into(),
        message: "Internal Error".into(),
        target_url: "https://cleanup.test/api/fail".into(),
        source_page_url: None,
    }];
    db.save_issue_batch(session, &issues).expect("Save issues");

    let ctx = Arc::new(McpContext::new(Some(db_path.clone())).expect("McpContext init"));

    // 1. Query issues
    let query_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 50,
        "method": "tools/call",
        "params": {
            "name": "seo_query_issues",
            "arguments": {
                "session_id": session,
                "severity": "critical"
            }
        }
    });

    let resp_str = handle_jsonrpc_request(&query_req.to_string(), &ctx).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_str).expect("Valid JSON response");
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .expect("Content text");
    let query_res: serde_json::Value = serde_json::from_str(text).expect("Parsed query JSON");

    assert_eq!(query_res["total_matching"], 1);
    assert_eq!(query_res["issues"][0]["code"], "ERR_HTTP_5XX_SERVER_ERROR");

    // 2. Cleanup session
    let cleanup_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 51,
        "method": "tools/call",
        "params": {
            "name": "seo_cleanup_session",
            "arguments": {
                "session_id": session
            }
        }
    });

    let clean_resp_str = handle_jsonrpc_request(&cleanup_req.to_string(), &ctx).await;
    let clean_resp: serde_json::Value =
        serde_json::from_str(&clean_resp_str).expect("Valid JSON response");
    let clean_text = clean_resp["result"]["content"][0]["text"]
        .as_str()
        .expect("Content text");
    let clean_res: serde_json::Value = serde_json::from_str(clean_text).expect("Parsed clean JSON");

    assert_eq!(clean_res["session_id"], session);
    assert_eq!(clean_res["purged"], true);

    // Verify session no longer exists in DB
    let verify_db = Database::open(&db_path).expect("Reopen db");
    let crawl = verify_db.get_crawl(session).expect("Query deleted crawl");
    assert!(
        crawl.is_none(),
        "Crawl session should be purged from SQLite"
    );
}

#[tokio::test]
async fn test_mcp_check_ai_and_validate_schema_tools() {
    let server = MockServer::start().await;
    let robots_txt = "User-agent: PerplexityBot\nDisallow: /\n\nUser-agent: GPTBot\nDisallow: /";
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(robots_txt))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/llms.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("# LLMS.txt content"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/llms-full.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let db_path = unique_test_db_path();
    let ctx = Arc::new(McpContext::new(Some(db_path)).expect("McpContext init"));

    // 1. Test seo_check_ai_readiness
    let ai_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 60,
        "method": "tools/call",
        "params": {
            "name": "seo_check_ai_readiness",
            "arguments": {
                "url": server.uri()
            }
        }
    });

    let resp_str = handle_jsonrpc_request(&ai_req.to_string(), &ctx).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_str).expect("Valid JSON response");
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .expect("Content text");
    let ai_res: serde_json::Value = serde_json::from_str(text).expect("Parsed AI check JSON");

    assert_eq!(ai_res["llms_txt_found"], true);
    assert_eq!(ai_res["llms_full_txt_found"], false);
    assert_eq!(
        ai_res["ai_crawler_access"]["retrieval_citation_bots"]["PerplexityBot"],
        "DISALLOWED"
    );

    // 2. Test seo_validate_schema
    let valid_json_ld = serde_json::json!({
        "@context": "https://schema.org",
        "@type": "Product",
        "name": "Developer Mechanical Keyboard",
        "offers": {
            "@type": "Offer",
            "price": "149.99",
            "priceCurrency": "USD"
        }
    });

    let schema_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 61,
        "method": "tools/call",
        "params": {
            "name": "seo_validate_schema",
            "arguments": {
                "json_ld": valid_json_ld.to_string(),
                "target_type": "Product"
            }
        }
    });

    let schema_resp_str = handle_jsonrpc_request(&schema_req.to_string(), &ctx).await;
    let schema_resp: serde_json::Value =
        serde_json::from_str(&schema_resp_str).expect("Valid JSON response");
    let schema_text = schema_resp["result"]["content"][0]["text"]
        .as_str()
        .expect("Content text");
    let schema_res: serde_json::Value =
        serde_json::from_str(schema_text).expect("Parsed schema JSON");

    assert_eq!(schema_res["is_valid_json"], true);
    assert_eq!(schema_res["detected_type"], "Product");
    assert_eq!(schema_res["is_rich_result_eligible"], true);
}

#[tokio::test]
async fn test_mcp_resources_list_and_read() {
    let db_path = unique_test_db_path();
    let db = Database::open(&db_path).expect("Open test database");
    let session = "crawl_mcp_resource_test";

    db.init_crawl_session(&CrawlSessionInit {
        session_id: session.to_string(),
        target_url: "https://resource-test.com".to_string(),
        max_pages: 5,
        max_depth: 2,
        respect_robots: true,
        render_js: false,
    })
    .expect("Init session");

    let ctx = Arc::new(McpContext::new(Some(db_path)).expect("McpContext init"));

    // 1. resources/list
    let list_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 70,
        "method": "resources/list"
    });

    let resp_str = handle_jsonrpc_request(&list_req.to_string(), &ctx).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_str).expect("Valid JSON response");
    let resources = resp["result"]["resources"]
        .as_array()
        .expect("Resources array");

    let uris: Vec<&str> = resources
        .iter()
        .map(|r| r["uri"].as_str().expect("URI"))
        .collect();
    assert!(uris.contains(&"seo://crawls"));

    // 2. resources/read seo://crawls
    let read_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 71,
        "method": "resources/read",
        "params": {
            "uri": "seo://crawls"
        }
    });

    let read_resp_str = handle_jsonrpc_request(&read_req.to_string(), &ctx).await;
    let read_resp: serde_json::Value =
        serde_json::from_str(&read_resp_str).expect("Valid JSON response");
    let content = read_resp["result"]["contents"][0]["text"]
        .as_str()
        .expect("Content text");
    let sessions: Vec<serde_json::Value> =
        serde_json::from_str(content).expect("Parsed sessions array");
    assert!(sessions.iter().any(|s| s["session_id"] == session));

    // 3. resources/read seo://crawls/{id}/summary
    let summary_uri = format!("seo://crawls/{session}/summary");
    let summary_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 72,
        "method": "resources/read",
        "params": {
            "uri": summary_uri
        }
    });

    let summary_resp_str = handle_jsonrpc_request(&summary_req.to_string(), &ctx).await;
    let summary_resp: serde_json::Value =
        serde_json::from_str(&summary_resp_str).expect("Valid JSON response");
    let summary_content = summary_resp["result"]["contents"][0]["text"]
        .as_str()
        .expect("Summary text");
    let summary_obj: serde_json::Value =
        serde_json::from_str(summary_content).expect("Parsed summary");
    assert_eq!(summary_obj["session_id"], session);
    assert_eq!(summary_obj["target_url"], "https://resource-test.com");
}

#[tokio::test]
async fn test_mcp_server_io_stream() {
    let db_path = unique_test_db_path();
    let ping_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "ping"
    });
    let input_bytes = format!("{}\n", ping_req);
    let reader = tokio::io::BufReader::new(input_bytes.as_bytes());
    let mut output_bytes = Vec::new();

    blacksparrow::mcp::run_mcp_server_io(reader, &mut output_bytes, Some(db_path))
        .await
        .expect("Run MCP server IO");

    let output_str = String::from_utf8(output_bytes).expect("Valid UTF-8 output");
    let resp: serde_json::Value =
        serde_json::from_str(output_str.trim()).expect("Valid JSON response");
    assert_eq!(resp["id"], 99);
    assert_eq!(resp["result"], serde_json::json!({}));
}

#[test]
fn test_mcp_fails_fast_on_invalid_db_path() {
    let temp = std::env::temp_dir();
    let blocker = temp.join(format!("seolens_blocker_{}.tmp", std::process::id()));
    let _ = std::fs::write(&blocker, b"blocker");
    let invalid_path = blocker.join("forbidden_child_dir").join("never_allowed.db");
    let res = McpContext::new(Some(invalid_path));
    let _ = std::fs::remove_file(&blocker);
    assert!(
        res.is_err(),
        "McpContext must fail fast when database path cannot be created/opened"
    );
}

#[tokio::test]
async fn test_mcp_background_crawl_failure_persists_failed_status() {
    let db_path = unique_test_db_path();
    let ctx = Arc::new(McpContext::new(Some(db_path.clone())).expect("McpContext init"));

    let start_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 88,
        "method": "tools/call",
        "params": {
            "name": "seo_start_audit",
            "arguments": {
                "url": "http://this-target-does-not-exist.invalid"
            }
        }
    });

    let start_resp_str = handle_jsonrpc_request(&start_req.to_string(), &ctx).await;
    let start_resp: serde_json::Value =
        serde_json::from_str(&start_resp_str).expect("Valid JSON response");
    let content_text = start_resp["result"]["content"][0]["text"]
        .as_str()
        .expect("Content text");
    let start_res: serde_json::Value =
        serde_json::from_str(content_text).expect("Parsed start JSON");
    let session_id = start_res["session_id"].as_str().expect("Session ID");

    let status_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 89,
        "method": "tools/call",
        "params": {
            "name": "seo_audit_status",
            "arguments": {
                "session_id": session_id
            }
        }
    });

    for _ in 0..80 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let status_resp_str = handle_jsonrpc_request(&status_req.to_string(), &ctx).await;
        let status_resp: serde_json::Value =
            serde_json::from_str(&status_resp_str).expect("Valid JSON response");
        let status_text = status_resp["result"]["content"][0]["text"]
            .as_str()
            .expect("Content text");
        let status_res: serde_json::Value =
            serde_json::from_str(status_text).expect("Parsed status JSON");
        if status_res["is_complete"].as_bool() == Some(true) {
            assert_eq!(
                status_res["status"], "failed",
                "Terminal state on error must be 'failed'"
            );
            return;
        }
    }
    panic!("Audit status did not report complete within timeout");
}
