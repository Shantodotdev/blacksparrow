//! # Page Inspector Integration Tests
//!
//! Validates the Developer X-Ray / Page Inspector visualization:
//! - Comprehensive extraction of Title, Meta, Canonical, Robots, Viewport.
//! - OpenGraph and Twitter Card social preview key-value pairs.
//! - JSON-LD structured data schema blocks and eligibility.
//! - Heading hierarchy (H1 -> H2 -> H3) tree visualization.
//! - Link and asset counts (internal, external, images, missing alt).
//! - Single-page audit defect reporting with code, title, and remedy.

use compact_str::CompactString;
use reqwest::header::HeaderMap;
use seo_lens::core::models::{
    DiscoveredLink, ImageResource, IssueCategory, IssueFinding, RobotsFlags, RuleId, SchemaRecord,
    Severity,
};
use seo_lens::crawler::client::FetchResult;
use seo_lens::crawler::inspector::inspect_url;
use seo_lens::parser::ParsedPage;
use seo_lens::report::inspector::format_page_inspection;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn test_format_page_inspection_with_rich_metadata() {
    let page = ParsedPage {
        title: Some("Rust Programming Language - Fast & Reliable".to_string()),
        meta_description: Some(
            "A language empowering everyone to build reliable and efficient software.".to_string(),
        ),
        meta_keywords: None,
        canonical_url: Some("https://www.rust-lang.org/".to_string()),
        is_canonical_relative: false,
        html_lang: Some(CompactString::new("en")),
        charset: Some(CompactString::new("utf-8")),
        viewport: Some(CompactString::new("width=device-width, initial-scale=1.0")),
        robots_flags: RobotsFlags::NONE,
        h1_primary: Some("Rust: Empowering Everyone".to_string()),
        h1_count: 1,
        h2_headings: vec!["Why Rust?".to_string(), "Get Started".to_string()],
        h3_headings: vec!["Performance".to_string(), "Reliability".to_string()],
        dom_element_count: 50,
        word_count: 1450,
        content_hash: 0x12345678,
        simhash: 0x87654321,
        links: vec![
            DiscoveredLink {
                source_url: "https://www.rust-lang.org/".to_string(),
                target_url: "https://www.rust-lang.org/learn".to_string(),
                target_url_hash: 1,
                anchor_text: "Learn".to_string(),
                is_internal: true,
                is_nofollow: false,
                is_image_link: false,
                status_code: Some(200),
                is_target_blank: false,
                has_opener_or_referrer: true,
            },
            DiscoveredLink {
                source_url: "https://www.rust-lang.org/".to_string(),
                target_url: "https://github.com/rust-lang/rust".to_string(),
                target_url_hash: 2,
                anchor_text: "GitHub".to_string(),
                is_internal: false,
                is_nofollow: false,
                is_image_link: false,
                status_code: Some(200),
                is_target_blank: false,
                has_opener_or_referrer: true,
            },
        ],
        images: vec![
            ImageResource {
                src_url: "https://www.rust-lang.org/static/images/rust-logo-blk.svg".to_string(),
                alt_text: Some("Rust Logo".to_string()),
                width: Some(120),
                height: Some(120),
                has_dimensions: true,
                size_bytes: Some(1024),
                is_broken: false,
            },
            ImageResource {
                src_url: "https://www.rust-lang.org/static/images/banner.png".to_string(),
                alt_text: None,
                width: None,
                height: None,
                has_dimensions: false,
                size_bytes: Some(2048),
                is_broken: false,
            },
        ],
        schemas: vec![SchemaRecord {
            schema_type: CompactString::new("Organization"),
            raw_json: r#"{"@context":"https://schema.org","@type":"Organization","name":"Rust Foundation"}"#.to_string(),
            is_valid_json: true,
            is_google_eligible: true,
            missing_required_fields: vec![],
        }],
        hreflangs: vec![],
        open_graph: vec![
            (CompactString::new("og:title"), "Rust Programming Language".to_string()),
            (CompactString::new("og:description"), "A language empowering everyone...".to_string()),
            (CompactString::new("og:image"), "https://www.rust-lang.org/static/images/og.png".to_string()),
            (CompactString::new("og:type"), "website".to_string()),
        ],
        twitter_cards: vec![
            (CompactString::new("twitter:card"), "summary_large_image".to_string()),
            (CompactString::new("twitter:site"), "@rustlang".to_string()),
        ],
    };

    let fetch = FetchResult {
        url: "https://www.rust-lang.org/".to_string(),
        final_url: "https://www.rust-lang.org/".to_string(),
        status_code: 200,
        headers: HeaderMap::new(),
        content_type: CompactString::new("text/html; charset=utf-8"),
        body: "<html>...</html>".to_string(),
        body_bytes: vec![],
        size_bytes: 24800,
        ttfb_ms: 32,
        redirect_chain: vec![],
        waf_detected: None,
    };

    let issues = vec![IssueFinding {
        code: RuleId::WarnSecurityMissingCsp,
        category: IssueCategory::Security,
        severity: Severity::Warning,
        title: CompactString::new("Missing Content-Security-Policy (CSP)"),
        message: "Missing Content-Security-Policy header.".to_string(),
        target_url: "https://www.rust-lang.org/".to_string(),
        source_page_url: None,
    }];

    let output = format_page_inspection(&page, &fetch, &issues);

    assert!(output.contains("TARGET & PROTOCOL TELEMETRY"));
    assert!(output.contains("Rust Programming Language - Fast & Reliable"));
    assert!(output.contains("A language empowering everyone"));
    assert!(output.contains("https://www.rust-lang.org/"));
    assert!(output.contains("og:title"));
    assert!(output.contains("https://www.rust-lang.org/static/images/og.png"));
    assert!(output.contains("twitter:card"));
    assert!(output.contains("@rustlang"));
    assert!(output.contains("@type: Organization"));
    assert!(output.contains("Rust: Empowering Everyone"));
    assert!(!output.contains("PAGE TECHNICAL AUDIT"));
}

#[test]
fn test_format_page_inspection_wraps_long_descriptions() {
    let long_desc = "We turn climate realities into stories people understand, care about, and act on through communication, advocacy, and powerful storytelling.";
    let long_og_desc = "Climate stories can move people. We help communities and organizations communicate what is changing, why it matters, and how people can act.";

    let page = ParsedPage {
        title: Some("GTD Media | Climate Communication, Advocacy & Storytelling".to_string()),
        meta_description: Some(long_desc.to_string()),
        meta_keywords: None,
        canonical_url: Some("https://gtdnet.online/".to_string()),
        is_canonical_relative: false,
        html_lang: Some(CompactString::new("en")),
        charset: Some(CompactString::new("utf-8")),
        viewport: Some(CompactString::new("width=device-width, initial-scale=1")),
        robots_flags: RobotsFlags::NONE,
        h1_primary: Some("GTD Media".to_string()),
        h1_count: 1,
        h2_headings: vec![],
        h3_headings: vec![],
        dom_element_count: 50,
        word_count: 1567,
        content_hash: 1,
        simhash: 2,
        links: vec![],
        images: vec![],
        schemas: vec![],
        hreflangs: vec![],
        open_graph: vec![
            (CompactString::new("og:title"), "GTD Media".to_string()),
            (
                CompactString::new("og:description"),
                long_og_desc.to_string(),
            ),
        ],
        twitter_cards: vec![(
            CompactString::new("twitter:description"),
            long_og_desc.to_string(),
        )],
    };

    let fetch = FetchResult {
        url: "http://localhost:3000/".to_string(),
        final_url: "http://localhost:3000/".to_string(),
        status_code: 200,
        headers: HeaderMap::new(),
        content_type: CompactString::new("text/html; charset=utf-8"),
        body: "".to_string(),
        body_bytes: vec![],
        size_bytes: 230116,
        ttfb_ms: 6,
        redirect_chain: vec![],
        waf_detected: None,
    };

    let output = format_page_inspection(&page, &fetch, &[]);

    // Description must be wrapped across multiple lines
    assert!(output.contains("Description"));
    assert!(output.contains("We turn climate realities into stories"));
    assert!(output.contains("care about,"));
    assert!(output.contains("and act on through"));
    assert!(output.contains("140 chars"));

    // OpenGraph description must be wrapped with hanging indent
    assert!(output.contains("og:description"));
    assert!(output.contains("organizations communicate"));

    // Twitter description must be wrapped with hanging indent
    assert!(output.contains("twitter:description"));
    assert!(output.contains("what is changing"));
}

#[tokio::test]
async fn test_inspect_url_live_mock_server() {
    let mock_server = MockServer::start().await;
    let base_uri = mock_server.uri();

    let html_content = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Inspect Test Page</title>
  <meta name="description" content="Detailed meta description for testing the inspector.">
  <link rel="canonical" href="{}/page">
  <meta property="og:title" content="OG Test Title">
  <meta property="og:image" content="{}/hero.jpg">
  <meta name="twitter:card" content="summary">
  <script type="application/ld+json">
  {{
    "@context": "https://schema.org",
    "@type": "Article",
    "headline": "Inspect Test Page"
  }}
  </script>
</head>
<body>
  <h1>Main Heading</h1>
  <h2>Sub Heading 1</h2>
  <h3>Detail Item</h3>
  <a href="{}/other">Other Link</a>
  <img src="{}/logo.png" alt="Test Logo" width="100" height="50">
</body>
</html>"#,
        base_uri, base_uri, base_uri, base_uri
    );

    Mock::given(method("GET"))
        .and(path("/page"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(html_content.into_bytes())
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .mount(&mock_server)
        .await;

    let target_url = format!("{}/page", base_uri);
    let (parsed, fetch, issues) = inspect_url(&target_url, "SEOLens/1.0", Duration::from_secs(5))
        .await
        .expect("inspect_url should succeed");

    assert_eq!(fetch.status_code, 200);
    assert_eq!(parsed.title.as_deref(), Some("Inspect Test Page"));
    assert_eq!(parsed.h1_primary.as_deref(), Some("Main Heading"));
    assert_eq!(parsed.schemas.len(), 1);
    assert_eq!(parsed.schemas[0].schema_type, "Article");

    let formatted = format_page_inspection(&parsed, &fetch, &issues);
    assert!(formatted.contains("Inspect Test Page"));
    assert!(formatted.contains("OG Test Title"));
    assert!(formatted.contains("@type: Article"));
    assert!(formatted.contains("Main Heading"));
}
