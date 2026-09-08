//! # Screaming Frog Compatible CSV Suite Integration Tests (Phase 11)
//!
//! Validates generation of the four industry-standard CSV export files:
//! 1. `internal_all.csv` (Screaming Frog internal crawl table)
//! 2. `issues_all.csv` (Full defect triage log)
//! 3. `response_codes.csv` (URL routing & redirect map)
//! 4. `external_all.csv` (Outbound link audit)

use hashbrown::HashMap;
use seo_lens::core::models::{
    DiscoveredLink, IssueCategory, IssueFinding, PageReport, RobotsFlags, RuleId, Severity,
};
use seo_lens::crawler::engine::CrawlResult;
use seo_lens::graph::SiteGraph;
use seo_lens::report::csv::export_csv_suite;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static CSV_TEST_COUNTER: AtomicUsize = AtomicUsize::new(1);

fn unique_test_csv_dir() -> PathBuf {
    let id = CSV_TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut dir = std::env::temp_dir();
    dir.push(format!("seolens_csv_test_{}_{}", std::process::id(), id));
    dir
}

fn create_test_crawl_result() -> CrawlResult {
    let mut graph = SiteGraph::new();

    let url_home = "https://example.com/".to_string();
    let url_about = "https://example.com/about".to_string();
    let url_redirect = "https://example.com/old-page".to_string();
    let url_noindex = "https://example.com/privacy".to_string();

    graph.add_node(&url_home, 200, 0, true);
    graph.add_node(&url_about, 200, 1, true);
    graph.add_node(&url_redirect, 301, 1, false);
    graph.add_node(&url_noindex, 200, 1, false);

    // Links: home -> about, home -> old-page, home -> privacy, home -> external
    graph.add_edge(
        &url_home,
        &url_about,
        seo_lens::graph::LinkEdgeType::InternalHyperlink,
        false,
        "About Us",
    );
    graph.add_edge(
        &url_home,
        &url_redirect,
        seo_lens::graph::LinkEdgeType::Redirect,
        false,
        "Old Page",
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
        title: Some("Example Home, Company & Co.".to_string()),
        title_length: 29,
        meta_description: Some("Meta description with, commas and \"quotes\".".to_string()),
        meta_desc_length: 44,
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
                target_url: "https://external-partner.org/partners".to_string(),
                target_url_hash: 99,
                anchor_text: "External Partner".to_string(),
                is_internal: false,
                is_nofollow: true,
                is_image_link: false,
                is_target_blank: true,
                has_opener_or_referrer: true,
                status_code: Some(200),
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

    let page_redirect = PageReport {
        crawl_id: "crawl_test".into(),
        url: url_redirect.clone(),
        url_hash: 3,
        final_url: Some(url_home.clone()),
        status_code: 301,
        content_type: "text/html".into(),
        size_bytes: 300,
        ttfb_ms: 45,
        crawl_depth: 1,
        is_internal: true,
        ..Default::default()
    };

    let page_noindex = PageReport {
        crawl_id: "crawl_test".into(),
        url: url_noindex.clone(),
        url_hash: 4,
        final_url: Some(url_noindex.clone()),
        status_code: 200,
        content_type: "text/html".into(),
        size_bytes: 1500,
        ttfb_ms: 90,
        crawl_depth: 1,
        title: Some("Privacy Policy".to_string()),
        title_length: 14,
        robots_flags: RobotsFlags::NOINDEX,
        is_internal: true,
        ..Default::default()
    };

    let issues = vec![
        IssueFinding {
            code: RuleId::WarnMetaDescMissing,
            category: IssueCategory::TitleMetadata,
            severity: Severity::Warning,
            title: "Missing Meta Description".into(),
            message: "The page does not declare a meta description.".to_string(),
            target_url: url_about.clone(),
            source_page_url: Some(url_home.clone()),
        },
        IssueFinding {
            code: RuleId::AlertIndexingBlockedNoindex,
            category: IssueCategory::Indexability,
            severity: Severity::Alert,
            title: "Noindex Directive Detected".into(),
            message: "Page has noindex directive in robots meta.".to_string(),
            target_url: url_noindex.clone(),
            source_page_url: None,
        },
    ];

    CrawlResult {
        target_url: "https://example.com/".to_string(),
        pages: vec![page_home, page_about, page_redirect, page_noindex],
        graph,
        pagerank: HashMap::new(),
        issues,
        duration: Duration::from_secs(5),
        sitemap_urls: vec![url_home.clone(), url_about.clone()],
        aimd_delay_ms: 50,
        health_score: 85,
    }
}

#[test]
fn test_export_csv_suite_generates_all_four_files() {
    let result = create_test_crawl_result();
    let temp_dir = unique_test_csv_dir();

    let exported = export_csv_suite(&result, &temp_dir).expect("Export CSV suite should succeed");

    assert_eq!(
        exported.len(),
        4,
        "Should return 4 generated CSV file paths"
    );

    let internal_csv = temp_dir.join("csv").join("internal_all.csv");
    let issues_csv = temp_dir.join("csv").join("issues_all.csv");
    let response_codes_csv = temp_dir.join("csv").join("response_codes.csv");
    let external_csv = temp_dir.join("csv").join("external_all.csv");

    assert!(internal_csv.exists(), "internal_all.csv must exist");
    assert!(issues_csv.exists(), "issues_all.csv must exist");
    assert!(response_codes_csv.exists(), "response_codes.csv must exist");
    assert!(external_csv.exists(), "external_all.csv must exist");

    // 1. Validate internal_all.csv schema and contents
    let internal_content = fs::read_to_string(&internal_csv).unwrap();
    let mut internal_lines = internal_content.lines();
    let internal_header = internal_lines.next().expect("Header line");
    assert_eq!(
        internal_header,
        "Address,Status Code,Status,Content Type,Size (Bytes),Word Count,Title 1,Title 1 Length,Meta Description 1,Meta Description 1 Length,H1-1,H1-1 Length,Canonical Link Element 1,Indexability,Indexability Status,Inlinks,Outlinks,Crawl Depth,Response Time (ms)"
    );

    // Verify comma and quote escaping on home page
    assert!(internal_content.contains(r#""Example Home, Company & Co.""#));
    assert!(internal_content.contains(r#""Meta description with, commas and ""quotes"".""#));
    assert!(internal_content.contains("Indexable"));
    assert!(internal_content.contains("Non-Indexable"));

    // 2. Validate issues_all.csv schema and contents
    let issues_content = fs::read_to_string(&issues_csv).unwrap();
    let mut issues_lines = issues_content.lines();
    let issues_header = issues_lines.next().expect("Header line");
    assert_eq!(
        issues_header,
        "Issue Code,Issue Name,Severity,Category,URL,Source URL,Details,Recommendation"
    );
    assert!(issues_content.contains("WARN_META_DESC_MISSING"));
    assert!(issues_content.contains("ALERT_INDEXING_BLOCKED_NOINDEX"));

    // 3. Validate response_codes.csv schema and contents
    let response_codes_content = fs::read_to_string(&response_codes_csv).unwrap();
    let mut response_codes_lines = response_codes_content.lines();
    let response_codes_header = response_codes_lines.next().expect("Header line");
    assert_eq!(
        response_codes_header,
        "URL,Status Code,Status,Redirect URL,Redirect Type,Inlinks Count"
    );
    assert!(response_codes_content.contains("https://example.com/old-page"));
    assert!(response_codes_content.contains("301"));
    assert!(response_codes_content.contains("Permanent"));

    // 4. Validate external_all.csv schema and contents
    let external_content = fs::read_to_string(&external_csv).unwrap();
    let mut external_lines = external_content.lines();
    let external_header = external_lines.next().expect("Header line");
    assert_eq!(
        external_header,
        "Source URL,Destination URL,Anchor Text,Status Code,Is Nofollow"
    );
    assert!(external_content.contains("https://external-partner.org/partners"));
    assert!(external_content.contains("External Partner"));
    assert!(external_content.contains("true")); // is_nofollow

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_cli_report_command_generates_csv_suite() {
    use seo_lens::cli::args::{Cli, Commands, ReportArgs};
    use seo_lens::cli::commands::execute;
    use seo_lens::storage::{CrawlSessionInit, Database};

    let temp_dir = unique_test_csv_dir();
    let db_path = temp_dir.join("test.db");
    let reports_dir = temp_dir.join("reports");
    let session_id = "crawl_csv_test_1";

    let db = Database::open(&db_path).expect("Open test database");
    db.init_crawl_session(&CrawlSessionInit {
        session_id: session_id.to_string(),
        target_url: "https://example.com/".to_string(),
        max_pages: 10,
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
        h1_primary: Some("Welcome".to_string()),
        h1_count: 1,
        is_internal: true,
        links: vec![DiscoveredLink {
            source_url: "https://example.com/".to_string(),
            target_url: "https://external.org/test".to_string(),
            target_url_hash: 10,
            anchor_text: "External Link".to_string(),
            is_internal: false,
            is_nofollow: true,
            is_image_link: false,
            is_target_blank: false,
            has_opener_or_referrer: true,
            status_code: Some(200),
        }],
        ..Default::default()
    };

    writer_handle.save_page(test_page).await.expect("Save page");
    writer_handle.flush().await.expect("Flush");
    db.update_crawl_status(session_id, "completed", None, 1, 0, 0, 1, Some(90))
        .expect("Update status");
    writer_handle.shutdown().await.expect("Shutdown");
    writer_task.await.expect("Join").expect("Result");

    let report_cli = Cli {
        command: Commands::Report(ReportArgs {
            session: Some(session_id.to_string()),
            session_pos: None,
            format: Some("csv".to_string()),
            output_dir: Some(reports_dir.clone()),
            db_path: Some(db_path.clone()),
            ..Default::default()
        }),
    };

    execute(report_cli).await.expect("Report command execution");

    assert!(reports_dir.join("csv").join("internal_all.csv").exists());
    assert!(reports_dir.join("csv").join("issues_all.csv").exists());
    assert!(reports_dir.join("csv").join("response_codes.csv").exists());
    assert!(reports_dir.join("csv").join("external_all.csv").exists());

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_csv_formula_injection_neutralization() {
    let out_dir = unique_test_csv_dir();
    let mut crawl_result = create_test_crawl_result();

    // Inject malicious formula values into crawled fields
    crawl_result.pages[0].title = Some("=cmd|'/C calc'!A0".to_string());
    crawl_result.pages[0].h1_primary = Some("@SUM(1,2)".to_string());
    crawl_result.pages[0].links[0].anchor_text = "+12345678".to_string();

    let exported = export_csv_suite(&crawl_result, &out_dir).expect("Export CSV");
    assert_eq!(exported.len(), 4);

    let internal_csv = fs::read_to_string(out_dir.join("csv").join("internal_all.csv"))
        .expect("Read internal CSV");
    // Assert formula characters are prefixed with single quote
    assert!(
        internal_csv.contains("'=cmd|'/C calc'!A0"),
        "Title starting with '=' must be neutralized"
    );
    assert!(
        internal_csv.contains("'@SUM(1,2)"),
        "H1 starting with '@' must be neutralized"
    );

    let _ = fs::remove_dir_all(&out_dir);
}
