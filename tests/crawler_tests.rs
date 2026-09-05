//! # Asynchronous HTTP Fetcher & AIMD Politeness Integration Tests
//!
//! Strict TDD test suite validating:
//! - HTTP fetch execution with status code, body, and latency metrics.
//! - HTTP redirect chain tracking (301, 302, 307, 308) and destination resolution.
//! - Redirect loop prevention (max 10 hops limit).
//! - WAF / Bot challenge screen detection (Cloudflare, Akamai, DataDome, Imperva).
//! - AIMD congestion controller adaptive rate-tuning and backoff on 429 / 503 / latency spikes.

use seo_lens::crawler::aimd::AimdController;
use seo_lens::crawler::client::{FetchOptions, HttpClient};
use seo_lens::crawler::waf::detect_waf;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_http_fetch_success() {
    let mock_server = MockServer::start().await;

    let body_html = "<html><head><title>Test Page</title></head><body><h1>Hello</h1></body></html>";

    Mock::given(method("GET"))
        .and(path("/success"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(body_html.as_bytes().to_vec())
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .mount(&mock_server)
        .await;

    let client = HttpClient::new(FetchOptions {
        user_agent: "SEOLens/1.0".to_string(),
        timeout: Duration::from_secs(5),
        max_redirects: 10,
        ..Default::default()
    })
    .expect("Failed to create HTTP client");

    let url = format!("{}/success", mock_server.uri());
    let result = client.fetch(&url).await.expect("Fetch failed");

    assert_eq!(result.status_code, 200);
    assert_eq!(result.url, url);
    assert_eq!(result.final_url, url);
    assert_eq!(result.body, body_html);
    assert!(result.content_type.contains("text/html"));
    assert_eq!(result.size_bytes, body_html.len() as u32);
    assert!(result.redirect_chain.is_empty());
    assert!(result.waf_detected.is_none());
    assert!(result.ttfb_ms < 5000);
}

#[tokio::test]
async fn test_redirect_chain_tracking() {
    let mock_server = MockServer::start().await;

    // First redirect: /start -> /middle (301)
    Mock::given(method("GET"))
        .and(path("/start"))
        .respond_with(
            ResponseTemplate::new(301)
                .insert_header("location", format!("{}/middle", mock_server.uri())),
        )
        .mount(&mock_server)
        .await;

    // Second redirect: /middle -> /final (302)
    Mock::given(method("GET"))
        .and(path("/middle"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/final", mock_server.uri())),
        )
        .mount(&mock_server)
        .await;

    // Final destination: /final (200)
    Mock::given(method("GET"))
        .and(path("/final"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("<html><body>Final Destination</body></html>")
                .insert_header("content-type", "text/html"),
        )
        .mount(&mock_server)
        .await;

    let client = HttpClient::new(FetchOptions {
        user_agent: "SEOLens/1.0".to_string(),
        timeout: Duration::from_secs(5),
        max_redirects: 10,
        ..Default::default()
    })
    .expect("Failed to create HTTP client");

    let start_url = format!("{}/start", mock_server.uri());
    let result = client.fetch(&start_url).await.expect("Fetch failed");

    assert_eq!(result.status_code, 200);
    assert_eq!(result.url, start_url);
    assert_eq!(result.final_url, format!("{}/final", mock_server.uri()));
    assert_eq!(result.redirect_chain.len(), 2);
    assert_eq!(
        result.redirect_chain[0],
        format!("{}/start", mock_server.uri())
    );
    assert_eq!(
        result.redirect_chain[1],
        format!("{}/middle", mock_server.uri())
    );
}

#[tokio::test]
async fn test_redirect_loop_prevention() {
    let mock_server = MockServer::start().await;

    // /loop-a redirects to /loop-b
    Mock::given(method("GET"))
        .and(path("/loop-a"))
        .respond_with(
            ResponseTemplate::new(301)
                .insert_header("location", format!("{}/loop-b", mock_server.uri())),
        )
        .mount(&mock_server)
        .await;

    // /loop-b redirects back to /loop-a
    Mock::given(method("GET"))
        .and(path("/loop-b"))
        .respond_with(
            ResponseTemplate::new(301)
                .insert_header("location", format!("{}/loop-a", mock_server.uri())),
        )
        .mount(&mock_server)
        .await;

    let client = HttpClient::new(FetchOptions {
        user_agent: "SEOLens/1.0".to_string(),
        timeout: Duration::from_secs(5),
        max_redirects: 5,
        ..Default::default()
    })
    .expect("Failed to create HTTP client");

    let start_url = format!("{}/loop-a", mock_server.uri());
    let err = client.fetch(&start_url).await;
    assert!(err.is_err(), "Circular redirect should return an error");
}

#[tokio::test]
async fn test_waf_challenge_fingerprint_detection() {
    let mock_server = MockServer::start().await;

    let cf_challenge_html = r#"
        <!DOCTYPE html>
        <html>
        <head><title>Just a moment...</title></head>
        <body>
            <div id="cf-browser-verification">Checking your browser before accessing example.com.</div>
            <p>Cloudflare Ray ID: 8934273894723</p>
        </body>
        </html>
    "#;

    Mock::given(method("GET"))
        .and(path("/waf-blocked"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_string(cf_challenge_html)
                .insert_header("cf-ray", "8934273894723-DFW")
                .insert_header("server", "cloudflare"),
        )
        .mount(&mock_server)
        .await;

    let client = HttpClient::new(FetchOptions::default()).expect("Client creation failed");
    let url = format!("{}/waf-blocked", mock_server.uri());
    let result = client
        .fetch(&url)
        .await
        .expect("Should return fetch result even on 403");

    assert_eq!(result.status_code, 403);
    assert_eq!(result.waf_detected, Some("Cloudflare"));

    // Also verify detect_waf directly on other providers
    let akamai_body =
        "<html><body>Reference&#32;Number: 18.234.56 Akamai Bot Manager</body></html>";
    assert_eq!(
        detect_waf(403, &result.headers, akamai_body),
        Some("Akamai")
    );

    let datadome_body = "<script src=\"https://geo.captcha-delivery.com/captcha/\"></script>";
    assert_eq!(
        detect_waf(403, &result.headers, datadome_body),
        Some("DataDome")
    );

    let imperva_body = "<html>Incapsula Incident ID: 123456789</html>";
    assert_eq!(
        detect_waf(403, &result.headers, imperva_body),
        Some("Imperva")
    );
}

#[test]
fn test_aimd_congestion_controller() {
    let mut controller = AimdController::new(10, 0); // c_max = 10, delay_floor = 0ms

    assert_eq!(controller.current_concurrency(), 10);
    assert_eq!(controller.current_delay_ms(), 0);

    // 1. Record 20 successful low-latency requests (< 500ms)
    for _ in 0..20 {
        controller.record_success(120);
    }
    // Concurrency remains at max, delay remains at floor
    assert_eq!(controller.current_concurrency(), 10);
    assert_eq!(controller.current_delay_ms(), 0);

    // 2. Simulate origin distress (e.g. 429 Too Many Requests or 503 Service Unavailable)
    controller.record_failure(Some(429), true);

    // Multiplicative backoff triggers immediately: concurrency halved, delay increases
    assert!(controller.current_concurrency() <= 5);
    assert!(controller.current_delay_ms() >= 100);

    let distressed_delay = controller.current_delay_ms();
    let distressed_concurrency = controller.current_concurrency();

    // Another failure doubles delay again
    controller.record_failure(Some(503), false);
    assert!(controller.current_delay_ms() >= distressed_delay * 2);
    assert!(controller.current_concurrency() <= distressed_concurrency);

    // 3. Simulate origin recovery: 50 healthy fast requests
    for _ in 0..50 {
        controller.record_success(150);
    }

    // Delay should have decreased additively, concurrency should have recovered
    assert!(controller.current_delay_ms() < distressed_delay * 2);
    assert!(controller.current_concurrency() > 1);
}

#[tokio::test]
async fn test_crawl_sitemap_recursion_and_orphan_detection() {
    use seo_lens::core::config::CrawlConfig;
    use seo_lens::core::models::RuleId;
    use seo_lens::crawler::run_crawl;

    let mock_server = MockServer::start().await;
    let base_uri = mock_server.uri();

    // 1. robots.txt declares sitemap_index.xml
    let robots_txt = format!(
        "User-agent: *\nAllow: /\nSitemap: {}/sitemap_index.xml\n",
        base_uri
    );
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(robots_txt))
        .mount(&mock_server)
        .await;

    // 2. sitemap_index.xml declares sub-sitemap: /sitemap-pages.xml
    let sitemap_index = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sitemap>
    <loc>{}/sitemap-pages.xml</loc>
  </sitemap>
</sitemapindex>"#,
        base_uri
    );
    Mock::given(method("GET"))
        .and(path("/sitemap_index.xml"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sitemap_index)
                .insert_header("content-type", "application/xml"),
        )
        .mount(&mock_server)
        .await;

    // 3. /sitemap-pages.xml declares home page and an orphan page
    let sitemap_pages = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>{}/</loc>
  </url>
  <url>
    <loc>{}/orphan-article</loc>
  </url>
</urlset>"#,
        base_uri, base_uri
    );
    Mock::given(method("GET"))
        .and(path("/sitemap-pages.xml"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sitemap_pages)
                .insert_header("content-type", "application/xml"),
        )
        .mount(&mock_server)
        .await;

    // 4. Crawled HTML pages: / links to /linked-page, and /linked-page links to /
    let home_html = format!(
        r#"<!DOCTYPE html><html><head><title>Home</title></head><body><h1>Home</h1><a href="{}/linked-page">Linked</a></body></html>"#,
        base_uri
    );
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(home_html)
                .insert_header("content-type", "text/html"),
        )
        .mount(&mock_server)
        .await;

    let linked_html = format!(
        r#"<!DOCTYPE html><html><head><title>Linked</title></head><body><h1>Linked</h1><a href="{}/">Home</a></body></html>"#,
        base_uri
    );
    Mock::given(method("GET"))
        .and(path("/linked-page"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(linked_html)
                .insert_header("content-type", "text/html"),
        )
        .mount(&mock_server)
        .await;

    // Notice: /orphan-article is NOT linked from any page on the site!
    let orphan_html = r#"<!DOCTYPE html><html><head><title>Orphan</title></head><body><h1>Orphan</h1></body></html>"#;
    Mock::given(method("GET"))
        .and(path("/orphan-article"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(orphan_html)
                .insert_header("content-type", "text/html"),
        )
        .mount(&mock_server)
        .await;

    let mut config = CrawlConfig::new(&format!("{}/", base_uri)).expect("Valid config");
    config.max_pages = 50;
    config.max_depth = 5;
    config.concurrency = 2;
    config.delay_ms = 0;
    config.no_aimd = true;
    config.respect_robots = true;

    let result = run_crawl(&config, None)
        .await
        .expect("Crawl should succeed");

    // Sitemap URLs extracted must contain actual content page URLs, NOT XML sitemap files!
    for sm_url in &result.sitemap_urls {
        assert!(
            !sm_url.ends_with(".xml"),
            "Sitemap URL list must not contain XML feeds: {}",
            sm_url
        );
    }

    let expected_orphan_url = format!("{}/orphan-article", base_uri);
    assert!(
        result.sitemap_urls.contains(&expected_orphan_url),
        "Declared page /orphan-article must be in sitemap_urls: {:?}",
        result.sitemap_urls
    );

    // Orphan page finding must be generated for /orphan-article
    let orphan_findings: Vec<_> = result
        .issues
        .iter()
        .filter(|i| i.code == RuleId::AlertGraphOrphanPage)
        .collect();

    assert_eq!(
        orphan_findings.len(),
        1,
        "Expected exactly 1 orphan page finding, got: {:?}",
        orphan_findings
    );
    assert_eq!(orphan_findings[0].target_url, expected_orphan_url);

    // Sitemaps themselves and homepage must NOT have orphan findings
    for finding in &orphan_findings {
        assert!(
            !finding.target_url.ends_with(".xml"),
            "Sitemap XML file must never be flagged as orphan page: {}",
            finding.target_url
        );
        assert_ne!(
            finding.target_url,
            format!("{}/", base_uri),
            "Homepage must never be flagged as orphan page"
        );
    }
}
