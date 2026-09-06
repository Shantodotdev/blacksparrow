//! # CLI & Multi-Page Crawler Integration Tests
//!
//! Validates concurrent multi-page BFS crawl loops, depth limits, robots exclusion,
//! site graph generation, health score calculation, and file export formatting.

use seo_lens::core::config::CrawlConfig;
use seo_lens::core::models::RuleId;
use seo_lens::crawler::engine::run_crawl;
use seo_lens::report::json::export_json_report;
use seo_lens::report::markdown::export_markdown_report;
use seo_lens::report::score::calculate_health_score;
use std::fs;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_multi_page_crawl_bfs_and_max_pages() {
    let server = MockServer::start().await;
    let base = server.uri();

    // / (root) -> /page1, /page2, /page3
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<!DOCTYPE html><html><head><title>Root</title></head>
            <body>
                <a href="{base}/page1">Page 1</a>
                <a href="{base}/page2">Page 2</a>
                <a href="{base}/page3">Page 3</a>
            </body></html>"#
        )))
        .mount(&server)
        .await;

    // /page1 -> /page4, /page5
    Mock::given(method("GET"))
        .and(path("/page1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<!DOCTYPE html><html><head><title>Page 1</title></head>
            <body>
                <a href="{base}/page4">Page 4</a>
                <a href="{base}/page5">Page 5</a>
            </body></html>"#
        )))
        .mount(&server)
        .await;

    // /page2, /page3, /page4, /page5
    for p in ["/page2", "/page3", "/page4", "/page5"] {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                r#"<!DOCTYPE html><html><head><title>{p}</title></head><body><p>Content for {p}</p></body></html>"#
            )))
            .mount(&server)
            .await;
    }

    let mut config = CrawlConfig::new(&base).unwrap();
    config.max_pages = 3; // Cap at 3 pages
    config.concurrency = 2;
    config.no_aimd = true;
    config.respect_robots = false;

    let result = run_crawl(&config, None).await.unwrap();

    assert_eq!(
        result.pages.len(),
        3,
        "Crawl must strictly respect max_pages limit"
    );
    assert!(result.pages.iter().any(|p| p.url == format!("{base}/")));
}

#[tokio::test]
async fn test_crawl_max_depth_enforcement() {
    let server = MockServer::start().await;
    let base = server.uri();

    // Depth 0: / -> /d1
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<!DOCTYPE html><html><head><title>Root</title></head><body><a href="{base}/d1">D1</a></body></html>"#
        )))
        .mount(&server)
        .await;

    // Depth 1: /d1 -> /d2
    Mock::given(method("GET"))
        .and(path("/d1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<!DOCTYPE html><html><head><title>D1</title></head><body><a href="{base}/d2">D2</a></body></html>"#
        )))
        .mount(&server)
        .await;

    // Depth 2: /d2 -> /d3
    Mock::given(method("GET"))
        .and(path("/d2"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<!DOCTYPE html><html><head><title>D2</title></head><body><a href="{base}/d3">D3</a></body></html>"#
        )))
        .mount(&server)
        .await;

    // Depth 3: /d3
    Mock::given(method("GET"))
        .and(path("/d3"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<!DOCTYPE html><html><head><title>D3</title></head><body>D3 content</body></html>"#,
        ))
        .mount(&server)
        .await;

    let mut config = CrawlConfig::new(&base).unwrap();
    config.max_pages = 10;
    config.max_depth = 1; // Only crawl root (depth 0) and depth 1 (/d1)
    config.concurrency = 2;
    config.no_aimd = true;
    config.respect_robots = false;

    let result = run_crawl(&config, None).await.unwrap();

    assert_eq!(
        result.pages.len(),
        2,
        "Should only crawl depth 0 and depth 1"
    );
    let crawled_urls: Vec<_> = result.pages.iter().map(|p| p.url.as_str()).collect();
    assert!(crawled_urls.contains(&format!("{base}/").as_str()));
    assert!(crawled_urls.contains(&format!("{base}/d1").as_str()));
    assert!(!crawled_urls.contains(&format!("{base}/d2").as_str()));
}

#[tokio::test]
async fn test_robots_txt_exclusion_during_crawl() {
    let server = MockServer::start().await;
    let base = server.uri();

    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("User-agent: *\nDisallow: /blocked/\n"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<!DOCTYPE html><html><head><title>Root</title></head>
            <body>
                <a href="{base}/allowed">Allowed</a>
                <a href="{base}/blocked/secret">Secret</a>
            </body></html>"#
        )))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/allowed"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<!DOCTYPE html><html><head><title>Allowed</title></head><body>Allowed content</body></html>"#,
        ))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/blocked/secret"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<!DOCTYPE html><html><head><title>Secret</title></head><body>Secret content</body></html>"#,
        ))
        .mount(&server)
        .await;

    let mut config = CrawlConfig::new(&base).unwrap();
    config.max_pages = 10;
    config.concurrency = 2;
    config.no_aimd = true;
    config.respect_robots = true; // Strict robots compliance

    let result = run_crawl(&config, None).await.unwrap();

    let crawled_urls: Vec<_> = result.pages.iter().map(|p| p.url.as_str()).collect();
    assert!(crawled_urls.contains(&format!("{base}/").as_str()));
    assert!(crawled_urls.contains(&format!("{base}/allowed").as_str()));
    assert!(
        !crawled_urls.contains(&format!("{base}/blocked/secret").as_str()),
        "Disallowed URL must not be crawled"
    );
}

#[tokio::test]
async fn test_multi_page_site_graph_and_pagerank() {
    let server = MockServer::start().await;
    let base = server.uri();

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<!DOCTYPE html><html><head><title>Home</title></head>
            <body><a href="{base}/about">About</a></body></html>"#
        )))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/about"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<!DOCTYPE html><html><head><title>About</title></head>
            <body><a href="{base}/">Home</a></body></html>"#
        )))
        .mount(&server)
        .await;

    let mut config = CrawlConfig::new(&base).unwrap();
    config.max_pages = 10;
    config.concurrency = 2;
    config.no_aimd = true;
    config.respect_robots = false;

    let result = run_crawl(&config, None).await.unwrap();

    assert_eq!(result.pages.len(), 2);
    assert_eq!(result.graph.node_count(), 2);
    assert_eq!(result.graph.edge_count(), 2);

    // Sum of PageRank scores must be approximately 1.0
    let pr_sum: f64 = result.pagerank.values().sum();
    assert!((pr_sum - 1.0).abs() < 1e-3, "PageRank sum must be 1.0");
}

#[test]
fn test_health_score_calculation() {
    use compact_str::CompactString;
    use seo_lens::core::models::{IssueCategory, IssueFinding, Severity};

    // Clean crawl with 10 pages and 0 issues = 100/100
    let clean_score = calculate_health_score(10, &[]);
    assert_eq!(clean_score, 100);

    // 10 pages with 1 Critical, 2 Alerts, 3 Warnings
    let mock_issue = |code, severity| IssueFinding {
        code,
        category: IssueCategory::TitleMetadata,
        severity,
        title: CompactString::new("Test Issue"),
        message: "Details".to_string(),
        target_url: "https://example.com/".to_string(),
        source_page_url: None,
    };

    let issues = vec![
        mock_issue(RuleId::ErrTitleMissing, Severity::Critical),
        mock_issue(RuleId::AlertCanonicalMismatch, Severity::Alert),
        mock_issue(RuleId::AlertWafBotChallenge, Severity::Alert),
        mock_issue(RuleId::WarnTitleTooShort, Severity::Warning),
        mock_issue(RuleId::WarnMetaDescMissing, Severity::Warning),
        mock_issue(RuleId::WarnContentThin, Severity::Warning),
    ];

    let degraded_score = calculate_health_score(10, &issues);
    assert!(degraded_score < 100);
    assert!(degraded_score > 0);
}

#[test]
fn test_markdown_and_json_report_exporters() {
    use compact_str::CompactString;
    use seo_lens::core::models::{PageReport, RobotsFlags};
    use seo_lens::crawler::engine::CrawlResult;
    use seo_lens::graph::SiteGraph;
    use std::time::Duration;

    let temp_dir = std::env::temp_dir().join(format!("seolens_test_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    let page = PageReport {
        id: None,
        crawl_id: CompactString::new("test"),
        url: "https://example.com/".to_string(),
        url_hash: 12345,
        final_url: None,
        status_code: 200,
        content_type: CompactString::new("text/html"),
        size_bytes: 1200,
        ttfb_ms: 50,
        crawl_depth: 0,
        title: Some("Example Page".to_string()),
        title_length: 12,
        meta_description: Some("Example description".to_string()),
        meta_desc_length: 19,
        canonical_url: Some("https://example.com/".to_string()),
        html_lang: Some(CompactString::new("en")),
        charset: Some(CompactString::new("utf-8")),
        viewport: Some(CompactString::new("width=device-width")),
        robots_flags: RobotsFlags::NONE,
        is_sitemap_url: false,
        is_internal: true,
        h1_primary: Some("Example".to_string()),
        h1_count: 1,
        h2_headings: vec![],
        h3_headings: vec![],
        word_count: 250,
        content_hash: 999,
        simhash: 888,
        is_soft_404: false,
        has_lorem_ipsum: false,
        is_https: true,
        has_hsts: true,
        has_csp: true,
        has_x_frame: true,
        has_x_content_type: true,
        mixed_content_count: 0,
        links: vec![],
        images: vec![],
        schemas: vec![],
        hreflangs: vec![],
        issues: vec![],
        page_intent: Default::default(),
    };

    let mut graph = SiteGraph::new();
    graph.add_node("https://example.com/", 200, 0, false);
    let mut pagerank = hashbrown::HashMap::new();
    pagerank.insert(12345, 1.0);

    let crawl_result = CrawlResult {
        target_url: "https://example.com/".to_string(),
        pages: vec![page],
        graph,
        pagerank,
        issues: vec![],
        duration: Duration::from_secs(2),
        sitemap_urls: vec![],
        aimd_delay_ms: 50,
        health_score: 100,
    };

    let md_path = export_markdown_report(&crawl_result, &temp_dir).unwrap();
    assert!(md_path.exists());
    let md_content = fs::read_to_string(&md_path).unwrap();
    assert!(md_content.contains("SEO LENS AUDIT REPORT"));
    assert!(md_content.contains("https://example.com/"));

    let json_path = export_json_report(&crawl_result, &temp_dir).unwrap();
    assert!(json_path.exists());
    let json_content = fs::read_to_string(&json_path).unwrap();
    let json_val: serde_json::Value = serde_json::from_str(&json_content).unwrap();
    assert_eq!(json_val["target_url"], "https://example.com/");
    assert_eq!(json_val["health_score"], 100);

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_crawl_ignores_non_html_assets_and_content_type() {
    let server = MockServer::start().await;
    let base = server.uri();

    // Root page links to static assets (.pdf, .gpg.ascii, .png, .zip) and a valid HTML page
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<!DOCTYPE html><html><head><title>Root</title><meta name="viewport" content="width=device-width"></head>
            <body>
                <h1>Root Title</h1>
                <a href="{base}/team-key.gpg.ascii">GPG Key</a>
                <a href="{base}/manual.pdf">User Manual</a>
                <a href="{base}/diagram.png">Architecture Diagram</a>
                <a href="{base}/release.zip">Release Archive</a>
                <a href="{base}/plain-api">Plain API</a>
                <a href="{base}/subpage">Subpage</a>
            </body></html>"#
        )))
        .mount(&server)
        .await;

    // Plain API without HTML extension, returning text/plain
    Mock::given(method("GET"))
        .and(path("/plain-api"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("content-type", "text/plain")
                .set_body_string("status=ok\nversion=1.0"),
        )
        .mount(&server)
        .await;

    // Subpage: valid HTML
    Mock::given(method("GET"))
        .and(path("/subpage"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<!DOCTYPE html><html><head><title>Subpage</title><meta name="viewport" content="width=device-width"></head>
            <body><h1>Subpage Title</h1><p>Welcome to the subpage</p></body></html>"#
        ))
        .mount(&server)
        .await;

    let mut config = CrawlConfig::new(&base).unwrap();
    config.max_pages = 10;
    config.concurrency = 2;
    config.no_aimd = true;
    config.respect_robots = false;

    let result = run_crawl(&config, None).await.unwrap();

    let crawled_urls: Vec<_> = result.pages.iter().map(|p| p.url.as_str()).collect();

    // The crawler must ignore non-HTML static assets in the frontier
    assert!(
        !crawled_urls.iter().any(|u| u.ends_with(".gpg.ascii")),
        ".gpg.ascii must not be crawled"
    );
    assert!(
        !crawled_urls.iter().any(|u| u.ends_with(".pdf")),
        ".pdf must not be crawled"
    );
    assert!(
        !crawled_urls.iter().any(|u| u.ends_with(".png")),
        ".png must not be crawled"
    );
    assert!(
        !crawled_urls.iter().any(|u| u.ends_with(".zip")),
        ".zip must not be crawled"
    );

    // Root, subpage, and plain-api should be crawled (assets skipped)
    assert_eq!(result.pages.len(), 3);
    assert!(crawled_urls.iter().any(|u| u.ends_with("/plain-api")));

    // No false positive ERR_TITLE_MISSING, ERR_H1_MISSING, or ERR_MOBILE_NO_VIEWPORT for plain-api
    assert!(!result
        .issues
        .iter()
        .any(|i| i.code == RuleId::ErrTitleMissing));
    assert!(!result.issues.iter().any(|i| i.code == RuleId::ErrH1Missing));
    assert!(!result
        .issues
        .iter()
        .any(|i| i.code == RuleId::ErrMobileNoViewport));
}

#[test]
fn test_json_report_export_compact_link_metrics() {
    use hashbrown::HashMap;
    use seo_lens::core::models::{DiscoveredLink, PageReport};
    use seo_lens::crawler::engine::CrawlResult;
    use seo_lens::graph::SiteGraph;
    use std::time::Duration;

    let temp_dir = std::env::temp_dir().join(format!(
        "seolens_json_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let mut page = PageReport {
        url: "https://example.com/catalog".to_string(),
        status_code: 200,
        title: Some("Catalog".to_string()),
        ..Default::default()
    };

    // Add 150 internal links and 50 external links
    for i in 0..150 {
        page.links.push(DiscoveredLink {
            source_url: page.url.clone(),
            target_url: format!("https://example.com/item-{}", i),
            target_url_hash: i as u64,
            anchor_text: format!("Item {}", i),
            is_internal: true,
            is_nofollow: false,
            is_image_link: false,
            is_target_blank: false,
            has_opener_or_referrer: false,
            status_code: None,
        });
    }
    for i in 0..50 {
        page.links.push(DiscoveredLink {
            source_url: page.url.clone(),
            target_url: format!("https://external.com/partner-{}", i),
            target_url_hash: (1000 + i) as u64,
            anchor_text: format!("Partner {}", i),
            is_internal: false,
            is_nofollow: true,
            is_image_link: false,
            is_target_blank: true,
            has_opener_or_referrer: true,
            status_code: None,
        });
    }

    let crawl_result = CrawlResult {
        target_url: "https://example.com/".to_string(),
        pages: vec![page],
        graph: SiteGraph::new(),
        pagerank: HashMap::new(),
        issues: vec![],
        duration: Duration::from_secs(1),
        sitemap_urls: vec![],
        aimd_delay_ms: 0,
        health_score: 100,
    };

    let json_path = export_json_report(&crawl_result, &temp_dir).unwrap();
    assert!(json_path.exists());
    let json_content = fs::read_to_string(&json_path).unwrap();
    let json_val: serde_json::Value = serde_json::from_str(&json_content).unwrap();

    let page_json = &json_val["pages"][0];
    assert_eq!(page_json["url"], "https://example.com/catalog");
    assert_eq!(page_json["links_count"], 200);
    assert_eq!(page_json["internal_links_count"], 150);
    assert_eq!(page_json["external_links_count"], 50);

    // Verify raw links array is omitted to keep JSON reports compact and fast
    assert!(page_json.get("links").is_none());

    // File size must remain very small (< 10 KB for 1 page)
    let metadata = fs::metadata(&json_path).unwrap();
    assert!(
        metadata.len() < 10_000,
        "JSON file must remain compact, got {} bytes",
        metadata.len()
    );

    let _ = fs::remove_dir_all(&temp_dir);
}
