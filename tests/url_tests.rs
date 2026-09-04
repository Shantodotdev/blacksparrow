//! # URL Normalization & Domain Models Test Suite
//!
//! Strict TDD tests for 8-stage URL normalization, relative resolution,
//! hashing, and domain models.

use compact_str::CompactString;
use seo_lens::core::config::CrawlConfig;
use seo_lens::core::models::{
    CrawlSummary, DiscoveredLink, HreflangTag, ImageResource, IssueCategory, IssueFinding,
    PageReport, RobotsFlags, SchemaRecord, Severity,
};
use seo_lens::core::url::{is_internal, normalize_url, resolve_relative, url_hash};
use seo_lens::SeoError;

// =========================================================================
// 1. SCHEME NORMALIZATION TESTS
// =========================================================================

#[test]
fn test_scheme_lowercase() {
    assert_eq!(
        normalize_url("HTTP://example.com/").unwrap(),
        "http://example.com/"
    );
    assert_eq!(
        normalize_url("HTTPS://EXAMPLE.COM/path").unwrap(),
        "https://example.com/path"
    );
    assert_eq!(
        normalize_url("hTtP://example.com/test").unwrap(),
        "http://example.com/test"
    );
}

// =========================================================================
// 2. HOSTNAME NORMALIZATION TESTS
// =========================================================================

#[test]
fn test_hostname_lowercase_and_root_dot() {
    assert_eq!(
        normalize_url("https://ExAmPLe.COM/").unwrap(),
        "https://example.com/"
    );
    assert_eq!(
        normalize_url("https://example.com./").unwrap(),
        "https://example.com/"
    );
    assert_eq!(
        normalize_url("https://SUB.DOMAIN.EXAMPLE.COM./path").unwrap(),
        "https://sub.domain.example.com/path"
    );
}

#[test]
fn test_punycode_idn_hostname() {
    let normalized = normalize_url("https://münchen.de/").unwrap();
    assert_eq!(normalized, "https://xn--mnchen-3ya.de/");
}

#[test]
fn test_ip_address_hostnames() {
    assert_eq!(
        normalize_url("http://192.168.1.1:8080/path").unwrap(),
        "http://192.168.1.1:8080/path"
    );
    assert_eq!(
        normalize_url("http://[2001:db8::1]:8080/path").unwrap(),
        "http://[2001:db8::1]:8080/path"
    );
}

// =========================================================================
// 3. PORT STRIPPING TESTS
// =========================================================================

#[test]
fn test_default_port_stripping() {
    // HTTP default port 80 stripped
    assert_eq!(
        normalize_url("http://example.com:80/").unwrap(),
        "http://example.com/"
    );
    assert_eq!(
        normalize_url("http://example.com:80/about").unwrap(),
        "http://example.com/about"
    );

    // HTTPS default port 443 stripped
    assert_eq!(
        normalize_url("https://example.com:443/").unwrap(),
        "https://example.com/"
    );
    assert_eq!(
        normalize_url("https://example.com:443/about").unwrap(),
        "https://example.com/about"
    );
}

#[test]
fn test_non_default_ports_preserved() {
    assert_eq!(
        normalize_url("http://example.com:8080/").unwrap(),
        "http://example.com:8080/"
    );
    assert_eq!(
        normalize_url("https://example.com:8443/api").unwrap(),
        "https://example.com:8443/api"
    );
    // Cross ports preserved (HTTP with 443, HTTPS with 80)
    assert_eq!(
        normalize_url("http://example.com:443/").unwrap(),
        "http://example.com:443/"
    );
    assert_eq!(
        normalize_url("https://example.com:80/").unwrap(),
        "https://example.com:80/"
    );
}

// =========================================================================
// 4. PATH NORMALIZATION & DOT SEGMENT RESOLUTION
// =========================================================================

#[test]
fn test_path_dot_segments() {
    assert_eq!(
        normalize_url("https://example.com/a/./b").unwrap(),
        "https://example.com/a/b"
    );
    assert_eq!(
        normalize_url("https://example.com/a/b/../c").unwrap(),
        "https://example.com/a/c"
    );
    assert_eq!(
        normalize_url("https://example.com/a/b/c/../../d").unwrap(),
        "https://example.com/a/d"
    );
    assert_eq!(
        normalize_url("https://example.com/../").unwrap(),
        "https://example.com/"
    );
}

#[test]
fn test_consecutive_slashes_in_path() {
    assert_eq!(
        normalize_url("https://example.com/blog//post").unwrap(),
        "https://example.com/blog/post"
    );
    assert_eq!(
        normalize_url("https://example.com/a///b////c/").unwrap(),
        "https://example.com/a/b/c/"
    );
    assert_eq!(
        normalize_url("https://example.com//").unwrap(),
        "https://example.com/"
    );
}

// =========================================================================
// 5. ROOT PATH ENFORCEMENT & TRAILING SLASH HANDLING
// =========================================================================

#[test]
fn test_root_path_enforcement() {
    assert_eq!(
        normalize_url("https://example.com").unwrap(),
        "https://example.com/"
    );
    assert_eq!(
        normalize_url("http://example.com").unwrap(),
        "http://example.com/"
    );
}

#[test]
fn test_trailing_slash_consistency() {
    // Non-root paths preserve trailing slash intention
    assert_eq!(
        normalize_url("https://example.com/blog/").unwrap(),
        "https://example.com/blog/"
    );
    assert_eq!(
        normalize_url("https://example.com/blog").unwrap(),
        "https://example.com/blog"
    );
    assert_eq!(
        normalize_url("https://example.com/index.html").unwrap(),
        "https://example.com/index.html"
    );
}

// =========================================================================
// 6. FRAGMENT STRIPPING TESTS
// =========================================================================

#[test]
fn test_fragment_stripping() {
    assert_eq!(
        normalize_url("https://example.com/page#section").unwrap(),
        "https://example.com/page"
    );
    assert_eq!(
        normalize_url("https://example.com/page#").unwrap(),
        "https://example.com/page"
    );
    assert_eq!(
        normalize_url("https://example.com/#top").unwrap(),
        "https://example.com/"
    );
    assert_eq!(
        normalize_url("https://example.com/?q=1#reviews").unwrap(),
        "https://example.com/?q=1"
    );
}

// =========================================================================
// 7. TRACKING PARAMETER STRIPPING TESTS
// =========================================================================

#[test]
fn test_strip_utm_tracking_parameters() {
    assert_eq!(
        normalize_url("https://example.com/page?utm_source=google").unwrap(),
        "https://example.com/page"
    );
    assert_eq!(
        normalize_url("https://example.com/page?utm_source=tw&utm_medium=social&utm_campaign=spring&utm_term=shoes&utm_content=banner").unwrap(),
        "https://example.com/page"
    );
}

#[test]
fn test_strip_ad_and_analytics_parameters() {
    // Facebook click id
    assert_eq!(
        normalize_url("https://example.com/?fbclid=IwAR123456").unwrap(),
        "https://example.com/"
    );
    // Google click id
    assert_eq!(
        normalize_url("https://example.com/prod?gclid=xyz987").unwrap(),
        "https://example.com/prod"
    );
    // Microsoft / Bing click id
    assert_eq!(
        normalize_url("https://example.com/?msclkid=abc").unwrap(),
        "https://example.com/"
    );
    // Mailchimp id
    assert_eq!(
        normalize_url("https://example.com/?mc_eid=mc123").unwrap(),
        "https://example.com/"
    );
    // Google Analytics client linker ids
    assert_eq!(
        normalize_url("https://example.com/?_ga=1.2.3&_gl=4.5.6").unwrap(),
        "https://example.com/"
    );
    // Generic ref tracking
    assert_eq!(
        normalize_url("https://example.com/item?ref=homepage").unwrap(),
        "https://example.com/item"
    );
}

#[test]
fn test_preserve_legitimate_parameters_while_stripping_tracking() {
    assert_eq!(
        normalize_url("https://example.com/search?utm_source=newsletter&q=rust&gclid=999").unwrap(),
        "https://example.com/search?q=rust"
    );
}

// =========================================================================
// 8. DETERMINISTIC QUERY SORTING & CLEANUP
// =========================================================================

#[test]
fn test_query_parameter_lexicographical_sorting() {
    assert_eq!(
        normalize_url("https://example.com/search?z=3&a=1&m=2").unwrap(),
        "https://example.com/search?a=1&m=2&z=3"
    );
    assert_eq!(
        normalize_url("https://example.com/?category=books&author=tolkien&available=true").unwrap(),
        "https://example.com/?author=tolkien&available=true&category=books"
    );
}

#[test]
fn test_empty_query_cleanup() {
    assert_eq!(
        normalize_url("https://example.com/search?").unwrap(),
        "https://example.com/search"
    );
    assert_eq!(
        normalize_url("https://example.com/?").unwrap(),
        "https://example.com/"
    );
}

// =========================================================================
// 9. RELATIVE URL RESOLUTION TESTS
// =========================================================================

#[test]
fn test_resolve_relative_paths() {
    let base = "https://example.com/blog/tutorials/";

    // Sibling / child relative path
    assert_eq!(
        resolve_relative(base, "rust-intro.html").unwrap(),
        "https://example.com/blog/tutorials/rust-intro.html"
    );

    // Parent directory relative path
    assert_eq!(
        resolve_relative(base, "../news/update.html").unwrap(),
        "https://example.com/blog/news/update.html"
    );

    // Root-relative path
    assert_eq!(
        resolve_relative(base, "/contact").unwrap(),
        "https://example.com/contact"
    );

    // Protocol-relative URL
    assert_eq!(
        resolve_relative(base, "//cdn.example.com/img.png").unwrap(),
        "https://cdn.example.com/img.png"
    );

    // Query-only relative URL
    assert_eq!(
        resolve_relative("https://example.com/items?page=1", "?page=2").unwrap(),
        "https://example.com/items?page=2"
    );

    // Absolute URL passes through normalized
    assert_eq!(
        resolve_relative(base, "https://other.com/about#hash").unwrap(),
        "https://other.com/about"
    );
}

// =========================================================================
// 10. INTERNAL LINK CHECK TESTS
// =========================================================================

#[test]
fn test_is_internal_link() {
    let base = "https://example.com/section/page";

    // Exact host matches
    assert!(is_internal("https://example.com/other", base));
    assert!(is_internal("http://example.com/other", base));
    assert!(is_internal("https://example.com/", base));

    // External domain
    assert!(!is_internal("https://google.com/", base));
    assert!(!is_internal("https://example.org/", base));

    // Subdomain is considered external (or distinct scope)
    assert!(!is_internal("https://sub.example.com/", base));
}

// =========================================================================
// 11. HASH DETERMINISM TESTS
// =========================================================================

#[test]
fn test_url_hash_determinism() {
    let url1 = "https://example.com/page?a=1&b=2";
    let url2 = "https://example.com/page?b=2&a=1";

    let norm1 = normalize_url(url1).unwrap();
    let norm2 = normalize_url(url2).unwrap();

    assert_eq!(norm1, norm2);
    assert_eq!(url_hash(&norm1), url_hash(&norm2));
    assert_ne!(url_hash(&norm1), 0);
}

// =========================================================================
// 12. ERROR HANDLING TESTS
// =========================================================================

#[test]
fn test_invalid_urls_return_seo_error() {
    assert!(matches!(normalize_url("not-a-valid-url"), Err(SeoError::Url(_))));
    assert!(matches!(normalize_url("://missing-scheme"), Err(SeoError::Url(_))));
    assert!(matches!(normalize_url("ftp://unsupported.com"), Err(SeoError::Url(_))));
}

// =========================================================================
// 13. DOMAIN MODELS CREATION & SERIALIZATION TESTS
// =========================================================================

#[test]
fn test_domain_models_integrity() {
    let mut flags = RobotsFlags::NONE;
    flags.insert(RobotsFlags::NOINDEX);
    flags.insert(RobotsFlags::NOFOLLOW);
    assert!(flags.contains(RobotsFlags::NOINDEX));
    assert!(flags.contains(RobotsFlags::NOFOLLOW));
    assert!(!flags.contains(RobotsFlags::NOARCHIVE));

    let issue = IssueFinding {
        code: CompactString::new("ERR_TITLE_MISSING"),
        category: IssueCategory::TitleMetadata,
        severity: Severity::Critical,
        title: CompactString::new("Missing Document Title"),
        message: "Page lacks a <title> tag.".to_string(),
        target_url: "https://example.com/missing-title".to_string(),
        source_page_url: None,
    };
    assert_eq!(issue.severity, Severity::Critical);
    assert_eq!(issue.category, IssueCategory::TitleMetadata);

    let link = DiscoveredLink {
        source_url: "https://example.com/".to_string(),
        target_url: "https://example.com/about".to_string(),
        target_url_hash: url_hash("https://example.com/about"),
        anchor_text: "About Us".to_string(),
        is_internal: true,
        is_nofollow: false,
        is_image_link: false,
        status_code: Some(200),
    };
    assert!(link.is_internal);

    let image = ImageResource {
        src_url: "https://example.com/logo.png".to_string(),
        alt_text: Some("Company Logo".to_string()),
        width: Some(200),
        height: Some(50),
        size_bytes: Some(15000),
        has_dimensions: true,
        is_broken: false,
    };
    assert!(image.has_dimensions);

    let schema = SchemaRecord {
        schema_type: CompactString::new("Organization"),
        raw_json: r#"{"@type":"Organization"}"#.to_string(),
        is_valid_json: true,
        is_google_eligible: true,
        missing_required_fields: vec![],
    };
    assert!(schema.is_valid_json);

    let hreflang = HreflangTag {
        lang_code: CompactString::new("en-US"),
        target_url: "https://example.com/en-us/".to_string(),
        is_reciprocal: true,
    };
    assert_eq!(hreflang.lang_code.as_str(), "en-US");

    let page_report = PageReport {
        id: None,
        crawl_id: CompactString::new("crawl-test-1"),
        url: "https://example.com/".to_string(),
        url_hash: url_hash("https://example.com/"),
        final_url: None,
        status_code: 200,
        content_type: CompactString::new("text/html; charset=utf-8"),
        size_bytes: 4096,
        ttfb_ms: 120,
        crawl_depth: 0,
        title: Some("Homepage".to_string()),
        title_length: 8,
        meta_description: Some("Homepage description".to_string()),
        meta_desc_length: 20,
        canonical_url: Some("https://example.com/".to_string()),
        html_lang: Some(CompactString::new("en")),
        charset: Some(CompactString::new("utf-8")),
        viewport: Some(CompactString::new("width=device-width, initial-scale=1")),
        robots_flags: flags,
        is_sitemap_url: true,
        is_internal: true,
        h1_primary: Some("Welcome".to_string()),
        h1_count: 1,
        h2_headings: vec!["Features".to_string()],
        h3_headings: vec![],
        word_count: 350,
        content_hash: 12345,
        simhash: 67890,
        is_soft_404: false,
        has_lorem_ipsum: false,
        is_https: true,
        has_hsts: true,
        has_csp: false,
        has_x_frame: true,
        has_x_content_type: true,
        mixed_content_count: 0,
        links: vec![link],
        images: vec![image],
        schemas: vec![schema],
        hreflangs: vec![hreflang],
        issues: vec![issue],
    };

    assert_eq!(page_report.status_code, 200);
    assert_eq!(page_report.links.len(), 1);

    // Serialization test
    let json = serde_json::to_string(&page_report).expect("Failed to serialize PageReport");
    assert!(json.contains("crawl-test-1"));
    assert!(json.contains("Homepage"));

    let config = CrawlConfig::new("https://example.com/").expect("Failed to create CrawlConfig");
    assert_eq!(config.start_url, "https://example.com/");
    assert_eq!(config.max_pages, 500);
    assert_eq!(config.max_depth, 5);
    assert_eq!(config.concurrency, 10);

    let summary = CrawlSummary {
        session_id: "test-session".to_string(),
        target_url: "https://example.com/".to_string(),
        started_at: "2026-09-04T12:00:00Z".to_string(),
        finished_at: None,
        total_pages_crawled: 1,
        total_links_discovered: 1,
        total_errors: 0,
        total_alerts: 0,
        total_warnings: 0,
        total_notices: 0,
        average_ttfb_ms: 120,
        p95_ttfb_ms: 120,
        health_score: 100,
    };
    assert_eq!(summary.health_score, 100);
}
