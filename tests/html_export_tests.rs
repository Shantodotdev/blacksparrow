//! # Standalone HTML Report Exporter Integration Tests (Phase 11)
//!
//! Validates generation of self-contained, offline-ready HTML visual audit reports:
//! 1. Single-file self-contained HTML (all CSS and JS inlined).
//! 2. Zero external dependencies (no CDNs, no external fonts or trackers).
//! 3. Interactive components: live search bar, severity filter chips, issue accordions, pages table.
//! 4. CLI report command integration (`--format html`).

use blacksparrow::core::models::{
    DiscoveredLink, IssueCategory, IssueFinding, PageReport, RobotsFlags, RuleId, Severity,
};
use blacksparrow::crawler::engine::CrawlResult;
use blacksparrow::graph::SiteGraph;
use blacksparrow::report::html::export_html_report;
use hashbrown::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static HTML_TEST_COUNTER: AtomicUsize = AtomicUsize::new(1);

fn unique_test_html_dir() -> PathBuf {
    let id = HTML_TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut dir = std::env::temp_dir();
    dir.push(format!("seolens_html_test_{}_{}", std::process::id(), id));
    dir
}

fn create_test_crawl_result() -> CrawlResult {
    let mut graph = SiteGraph::new();

    let url_home = "https://example.com/".to_string();
    let url_about = "https://example.com/about".to_string();
    let url_broken = "https://example.com/broken-page".to_string();

    graph.add_node(&url_home, 200, 0, true);
    graph.add_node(&url_about, 200, 1, true);
    graph.add_node(&url_broken, 404, 1, false);

    graph.add_edge(
        &url_home,
        &url_about,
        blacksparrow::graph::LinkEdgeType::InternalHyperlink,
        false,
        "About Us",
    );
    graph.add_edge(
        &url_home,
        &url_broken,
        blacksparrow::graph::LinkEdgeType::InternalHyperlink,
        false,
        "Broken Page",
    );

    let page_home = PageReport {
        crawl_id: "crawl_test".into(),
        url: url_home.clone(),
        url_hash: 1,
        final_url: Some(url_home.clone()),
        status_code: 200,
        content_type: "text/html; charset=utf-8".into(),
        size_bytes: 4096,
        ttfb_ms: 120,
        crawl_depth: 0,
        title: Some("Example Home | Best Products".to_string()),
        title_length: 28,
        meta_description: Some("Meta description for example home page.".to_string()),
        meta_desc_length: 39,
        canonical_url: Some(url_home.clone()),
        html_lang: Some("en".into()),
        robots_flags: RobotsFlags::NONE,
        is_sitemap_url: true,
        is_internal: true,
        h1_primary: Some("Welcome to Example".to_string()),
        h1_count: 1,
        word_count: 500,
        links: vec![
            DiscoveredLink {
                source_url: url_home.clone(),
                target_url: url_about.clone(),
                target_url_hash: 2,
                anchor_text: "About Us".to_string(),
                is_internal: true,
                is_nofollow: false,
                is_image_link: false,
                is_target_blank: false,
                has_opener_or_referrer: true,
                status_code: Some(200),
            },
            DiscoveredLink {
                source_url: url_home.clone(),
                target_url: url_broken.clone(),
                target_url_hash: 3,
                anchor_text: "Broken Page".to_string(),
                is_internal: true,
                is_nofollow: false,
                is_image_link: false,
                is_target_blank: false,
                has_opener_or_referrer: true,
                status_code: Some(404),
            },
        ],
        ..Default::default()
    };

    let page_about = PageReport {
        crawl_id: "crawl_test".into(),
        url: url_about.clone(),
        url_hash: 2,
        final_url: Some(url_about.clone()),
        status_code: 200,
        content_type: "text/html".into(),
        size_bytes: 2048,
        ttfb_ms: 85,
        crawl_depth: 1,
        title: Some("About Us - Example".to_string()),
        title_length: 18,
        meta_description: None,
        meta_desc_length: 0,
        canonical_url: Some(url_about.clone()),
        html_lang: Some("en".into()),
        robots_flags: RobotsFlags::NONE,
        is_sitemap_url: true,
        is_internal: true,
        h1_primary: Some("About Us".to_string()),
        h1_count: 1,
        word_count: 320,
        ..Default::default()
    };

    let page_broken = PageReport {
        crawl_id: "crawl_test".into(),
        url: url_broken.clone(),
        url_hash: 3,
        final_url: None,
        status_code: 404,
        content_type: "text/html".into(),
        size_bytes: 500,
        ttfb_ms: 60,
        crawl_depth: 1,
        is_internal: true,
        ..Default::default()
    };

    let issues = vec![
        IssueFinding {
            code: RuleId::ErrHttp4xxClientError,
            category: IssueCategory::HttpTransport,
            severity: Severity::Critical,
            title: "HTTP 404 Client Error".into(),
            message: "Page responded with 404 Not Found status code.".to_string(),
            target_url: url_broken.clone(),
            source_page_url: Some(url_home.clone()),
        },
        IssueFinding {
            code: RuleId::WarnMetaDescMissing,
            category: IssueCategory::TitleMetadata,
            severity: Severity::Warning,
            title: "Missing Meta Description".into(),
            message: "The page does not declare a meta description.".to_string(),
            target_url: url_about.clone(),
            source_page_url: Some(url_home.clone()),
        },
    ];

    CrawlResult {
        target_url: "https://example.com/".to_string(),
        pages: vec![page_home, page_about, page_broken],
        graph,
        pagerank: HashMap::new(),
        issues,
        duration: Duration::from_secs(3),
        sitemap_urls: vec![url_home.clone(), url_about.clone()],
        aimd_delay_ms: 50,
        health_score: 72,
    }
}

#[test]
fn test_export_html_report_creates_standalone_offline_report() {
    let result = create_test_crawl_result();
    let temp_dir = unique_test_html_dir();

    let output_path =
        export_html_report(&result, &temp_dir).expect("Export HTML report should succeed");

    assert!(output_path.exists(), "HTML report file must exist");
    assert!(output_path
        .extension()
        .map(|s| s == "html")
        .unwrap_or(false));

    let html_content = fs::read_to_string(&output_path).expect("Read generated HTML");

    // 1. Validate proper HTML5 structure
    assert!(html_content.contains("<!DOCTYPE html>"));
    assert!(html_content.contains("<html"));
    assert!(html_content.contains("<head>"));
    assert!(html_content.contains("</head>"));
    assert!(html_content.contains("<body"));
    assert!(html_content.contains("</body>"));
    assert!(html_content.contains("</html>"));

    // 2. Validate zero external CDN calls or tracking scripts
    assert!(!html_content.contains("fonts.googleapis.com"));
    assert!(!html_content.contains("cdnjs.cloudflare.com"));
    assert!(!html_content.contains("unpkg.com"));
    assert!(!html_content.contains("cdn.jsdelivr.net"));
    assert!(!html_content.contains("google-analytics.com"));

    // 3. Validate embedded CSS and JavaScript
    assert!(html_content.contains("<style>"));
    assert!(html_content.contains("</style>"));
    assert!(html_content.contains("<script>"));
    assert!(html_content.contains("</script>"));

    // 4. Validate Audit Data Content
    assert!(html_content.contains("https://example.com/"));
    assert!(html_content.contains("72/100") || html_content.contains("72"));
    assert!(html_content.contains("ERR_HTTP_4XX_CLIENT_ERROR"));
    assert!(html_content.contains("WARN_META_DESC_MISSING"));
    assert!(html_content.contains("https://example.com/broken-page"));
    assert!(html_content.contains("https://example.com/about"));

    // 5. Validate Interactive UI components
    assert!(
        html_content.contains("id=\"issueSearch\"") || html_content.contains("id=\"pageSearch\"")
    );
    assert!(html_content.contains("filterIssues") || html_content.contains("data-severity"));
    assert!(html_content.contains("copyRemediation") || html_content.contains("Copy"));

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_cli_report_command_generates_html_report() {
    use blacksparrow::cli::args::{Cli, Commands, ReportArgs};
    use blacksparrow::cli::commands::execute;
    use blacksparrow::storage::{CrawlSessionInit, Database};

    let temp_dir = unique_test_html_dir();
    let db_path = temp_dir.join("test.db");
    let reports_dir = temp_dir.join("reports");
    let session_id = "crawl_html_test_1";

    let db = Database::open(&db_path).expect("Open test database");
    db.init_crawl_session(&CrawlSessionInit {
        session_id: session_id.to_string(),
        target_url: "https://example.com/".to_string(),
        max_pages: 5,
        max_depth: 2,
        respect_robots: true,
        render_js: false,
    })
    .expect("Init session");

    let (writer_handle, writer_task) = db
        .spawn_writer(session_id, 1, Duration::from_millis(50))
        .expect("Spawn writer");

    let test_page = PageReport {
        crawl_id: session_id.into(),
        url: "https://example.com/".to_string(),
        url_hash: 1,
        final_url: Some("https://example.com/".to_string()),
        status_code: 200,
        content_type: "text/html".into(),
        size_bytes: 1024,
        ttfb_ms: 50,
        crawl_depth: 0,
        title: Some("Example Home".to_string()),
        title_length: 12,
        is_internal: true,
        ..Default::default()
    };

    writer_handle.save_page(test_page).await.expect("Save page");
    writer_handle.flush().await.expect("Flush");
    db.update_crawl_status(session_id, "completed", None, 1, 0, 0, 1, Some(85))
        .expect("Update status");
    writer_handle.shutdown().await.expect("Shutdown");
    writer_task.await.expect("Join").expect("Result");

    let report_cli = Cli {
        command: Commands::Report(ReportArgs {
            session: Some(session_id.to_string()),
            session_pos: None,
            format: Some("html".to_string()),
            output_dir: Some(reports_dir.clone()),
            db_path: Some(db_path.clone()),
            ..Default::default()
        }),
    };

    execute(report_cli).await.expect("Report command execution");

    let expected_file = reports_dir.join("example_com_audit.html");
    assert!(
        expected_file.exists(),
        "example_com_audit.html must be generated"
    );

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}
