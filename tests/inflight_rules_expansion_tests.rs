//! # In-Flight Rules Expansion Tests (Micro-Phase 4.5)
//!
//! Integration tests validating remaining in-flight rules across Categories 1–8:
//! 1. Category 1: ERR_HTTP_SOFT_404
//! 2. Category 2: WARN_TITLE_SAME_AS_H1, WARN_META_KEYWORDS_PRESENT
//! 3. Category 3: WARN_DUPLICATE_HEADING_TEXT, WARN_EXCESSIVE_DOM_DEPTH
//! 4. Category 4: WARN_PAGINATION_MISSING_CANONICAL, ALERT_PAGINATION_NOINDEX, ALERT_UNRENDERED_SPA_HEURISTIC
//! 5. Category 5: ALERT_CANONICAL_CROSS_DOMAIN, WARN_CANONICAL_TO_UNVERIFIED_HTTP
//! 6. Category 6: WARN_LINKS_TOO_MANY_ON_PAGE, WARN_LINK_SUSPICIOUS_ANCHOR, WARN_LINK_EMPTY_ANCHOR
//! 7. Category 7: WARN_SECURITY_MISSING_REFERRER_POLICY, WARN_SECURITY_TARGET_BLANK_NO_OPENER, WARN_SECURITY_INSECURE_FORM
//! 8. Category 8: WARN_PERF_LARGE_HTML_PAYLOAD, ERR_PERF_EXCESSIVE_HTML_PAYLOAD, WARN_IMG_ALT_TOO_LONG

use compact_str::CompactString;
use reqwest::header::{HeaderMap, HeaderValue};
use seo_lens::core::models::{DiscoveredLink, ImageResource, RobotsFlags, RuleId};
use seo_lens::crawler::client::FetchResult;
use seo_lens::parser::{parse_html, ParsedPage};
use seo_lens::rules::evaluate_page;

fn make_test_fetch(
    url: &str,
    status_code: u16,
    body: &str,
    size_bytes: u32,
    headers: HeaderMap,
) -> FetchResult {
    FetchResult {
        url: url.to_string(),
        final_url: url.to_string(),
        status_code,
        headers,
        content_type: CompactString::new("text/html; charset=utf-8"),
        body: body.to_string(),
        body_bytes: body.as_bytes().to_vec(),
        size_bytes,
        ttfb_ms: 100,
        redirect_chain: Vec::new(),
        waf_detected: None,
    }
}

fn make_secure_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "strict-transport-security",
        HeaderValue::from_static("max-age=31536000; includeSubDomains"),
    );
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'self'"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "referrer-policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers
}

#[test]
fn test_rule_err_http_soft_404() {
    let url = "https://example.com/missing-item";
    let body = "<html><head><title>404 Not Found</title></head><body><h1>404 Not Found</h1><p>Sorry, the page you requested cannot be found.</p></body></html>";
    let fetch = make_test_fetch(url, 200, body, body.len() as u32, make_secure_headers());
    let page = parse_html(body, url).unwrap();

    let issues = evaluate_page(&page, &fetch);
    assert!(
        issues.iter().any(|i| i.code == RuleId::ErrHttpSoft404),
        "Expected ERR_HTTP_SOFT_404 when HTTP 200 OK has '404 Not Found' content with low word count"
    );
}

#[test]
fn test_rule_warn_title_same_as_h1() {
    let url = "https://example.com/products/shoes";
    let body = r#"<!DOCTYPE html><html><head><title>Men's Running Shoes</title></head><body><h1>Men's Running Shoes</h1></body></html>"#;
    let fetch = make_test_fetch(url, 200, body, body.len() as u32, make_secure_headers());
    let page = parse_html(body, url).unwrap();

    let issues = evaluate_page(&page, &fetch);
    assert!(
        issues.iter().any(|i| i.code == RuleId::WarnTitleSameAsH1),
        "Expected WARN_TITLE_SAME_AS_H1 when title exactly matches H1"
    );
}

#[test]
fn test_rule_warn_meta_keywords_present() {
    let url = "https://example.com/page";
    let body = r#"<!DOCTYPE html><html><head><title>Test Page</title><meta name="keywords" content="seo, audit, ranking"></head><body><h1>Test</h1></body></html>"#;
    let fetch = make_test_fetch(url, 200, body, body.len() as u32, make_secure_headers());
    let page = parse_html(body, url).unwrap();

    let issues = evaluate_page(&page, &fetch);
    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnMetaKeywordsPresent),
        "Expected WARN_META_KEYWORDS_PRESENT when <meta name=\"keywords\"> is present"
    );
}

#[test]
fn test_rule_warn_duplicate_heading_text() {
    let url = "https://example.com/guide";
    let body = r#"<!DOCTYPE html><html><head><title>Guide</title></head><body>
        <h1>Main Guide</h1>
        <h2>Overview</h2>
        <p>Text 1</p>
        <h2>Overview</h2>
        <p>Text 2</p>
    </body></html>"#;
    let fetch = make_test_fetch(url, 200, body, body.len() as u32, make_secure_headers());
    let page = parse_html(body, url).unwrap();

    let issues = evaluate_page(&page, &fetch);
    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnDuplicateHeadingText),
        "Expected WARN_DUPLICATE_HEADING_TEXT when multiple H2s have duplicate text"
    );
}

#[test]
fn test_rule_warn_excessive_dom_depth() {
    let url = "https://example.com/bloated";
    let page = ParsedPage {
        dom_element_count: 1800, // > 1500 threshold
        ..Default::default()
    };
    let fetch = make_test_fetch(url, 200, "<html></html>", 100, make_secure_headers());

    let issues = evaluate_page(&page, &fetch);
    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnExcessiveDomDepth),
        "Expected WARN_EXCESSIVE_DOM_DEPTH when dom_element_count > 1500"
    );
}

#[test]
fn test_rule_pagination_missing_canonical_and_noindex() {
    let url = "https://example.com/shop?page=3";
    let mut page = ParsedPage::default();
    page.robots_flags.insert(RobotsFlags::NOINDEX);
    let fetch = make_test_fetch(url, 200, "<html></html>", 100, make_secure_headers());

    let issues = evaluate_page(&page, &fetch);
    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnPaginationMissingCanonical),
        "Expected WARN_PAGINATION_MISSING_CANONICAL on paginated page without canonical"
    );
    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::AlertPaginationNoindex),
        "Expected ALERT_PAGINATION_NOINDEX on paginated page with noindex"
    );
}

#[test]
fn test_rule_alert_unrendered_spa_heuristic() {
    let url = "https://example.com/app";
    let body = r#"<!DOCTYPE html><html><head><title>App</title></head><body><div id="root"></div><noscript>You need JavaScript to run this app.</noscript></body></html>"#;
    let fetch = make_test_fetch(url, 200, body, body.len() as u32, make_secure_headers());
    let page = parse_html(body, url).unwrap();

    let issues = evaluate_page(&page, &fetch);
    assert!(
        issues.iter().any(|i| i.code == RuleId::AlertUnrenderedSpaHeuristic),
        "Expected ALERT_UNRENDERED_SPA_HEURISTIC on empty React/Vue root container with 0 links and low word count"
    );
}

#[test]
fn test_rule_canonical_cross_domain_and_unverified_http() {
    let url = "https://example.com/post";

    // 1. Cross domain canonical
    let page1 = ParsedPage {
        canonical_url: Some("https://other-domain.com/post".to_string()),
        ..Default::default()
    };
    let fetch1 = make_test_fetch(url, 200, "<html></html>", 100, make_secure_headers());
    let issues1 = evaluate_page(&page1, &fetch1);
    assert!(
        issues1
            .iter()
            .any(|i| i.code == RuleId::AlertCanonicalCrossDomain),
        "Expected ALERT_CANONICAL_CROSS_DOMAIN when canonical points to another domain"
    );

    // 2. HTTPS page canonicalizing to HTTP
    let page2 = ParsedPage {
        canonical_url: Some("http://example.com/post".to_string()),
        ..Default::default()
    };
    let fetch2 = make_test_fetch(url, 200, "<html></html>", 100, make_secure_headers());
    let issues2 = evaluate_page(&page2, &fetch2);
    assert!(
        issues2
            .iter()
            .any(|i| i.code == RuleId::WarnCanonicalToUnverifiedHttp),
        "Expected WARN_CANONICAL_TO_UNVERIFIED_HTTP when HTTPS page canonicalizes to HTTP"
    );
}

#[test]
fn test_rule_links_rules() {
    let url = "https://example.com/blog";

    let mut page = ParsedPage::default();
    // 1. Non-descriptive anchor
    page.links.push(DiscoveredLink {
        source_url: url.to_string(),
        target_url: "https://example.com/more".to_string(),
        target_url_hash: 1,
        anchor_text: "click here".to_string(),
        is_internal: true,
        is_nofollow: false,
        is_image_link: false,
        status_code: None,
        is_target_blank: false,
        has_opener_or_referrer: true,
    });

    // 2. Empty anchor
    page.links.push(DiscoveredLink {
        source_url: url.to_string(),
        target_url: "https://example.com/empty".to_string(),
        target_url_hash: 2,
        anchor_text: "   ".to_string(),
        is_internal: true,
        is_nofollow: false,
        is_image_link: false,
        status_code: None,
        is_target_blank: false,
        has_opener_or_referrer: true,
    });

    // 3. Excessive links (> 250)
    for i in 0..260 {
        page.links.push(DiscoveredLink {
            source_url: url.to_string(),
            target_url: format!("https://example.com/page-{}", i),
            target_url_hash: 100 + i,
            anchor_text: format!("Link {}", i),
            is_internal: true,
            is_nofollow: false,
            is_image_link: false,
            status_code: None,
            is_target_blank: false,
            has_opener_or_referrer: true,
        });
    }

    let fetch = make_test_fetch(url, 200, "<html></html>", 100, make_secure_headers());
    let issues = evaluate_page(&page, &fetch);

    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnLinkSuspiciousAnchor),
        "Expected WARN_LINK_SUSPICIOUS_ANCHOR on 'click here'"
    );
    assert!(
        issues.iter().any(|i| i.code == RuleId::WarnLinkEmptyAnchor),
        "Expected WARN_LINK_EMPTY_ANCHOR on empty anchor text"
    );
    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnLinksTooManyOnPage),
        "Expected WARN_LINKS_TOO_MANY_ON_PAGE on > 250 links"
    );
}

#[test]
fn test_rule_security_rules() {
    let url = "https://example.com/contact";

    // 1. Missing Referrer-Policy
    let mut headers = make_secure_headers();
    headers.remove("referrer-policy");

    // 2. Target blank without noopener
    let mut page = ParsedPage::default();
    page.links.push(DiscoveredLink {
        source_url: url.to_string(),
        target_url: "https://external.com/partner".to_string(),
        target_url_hash: 1,
        anchor_text: "Partner".to_string(),
        is_internal: false,
        is_nofollow: false,
        is_image_link: false,
        status_code: None,
        is_target_blank: true,
        has_opener_or_referrer: false,
    });

    // 3. Insecure form action
    let body = r#"<html><body><form action="http://insecure-api.com/submit"><input type="text"/></form></body></html>"#;

    let fetch = make_test_fetch(url, 200, body, body.len() as u32, headers);
    let issues = evaluate_page(&page, &fetch);

    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnSecurityMissingReferrerPolicy),
        "Expected WARN_SECURITY_MISSING_REFERRER_POLICY when Referrer-Policy header is missing"
    );
    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnSecurityTargetBlankNoOpener),
        "Expected WARN_SECURITY_TARGET_BLANK_NO_OPENER on target=_blank without noopener"
    );
    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnSecurityInsecureForm),
        "Expected WARN_SECURITY_INSECURE_FORM on form action='http://...'"
    );
}

#[test]
fn test_rule_performance_and_images_rules() {
    let url = "https://example.com/gallery";

    // 1. Large HTML Payload (> 1.5 MB)
    let fetch_large = make_test_fetch(url, 200, "<html></html>", 1_800_000, make_secure_headers());
    let page = ParsedPage::default();
    let issues_large = evaluate_page(&page, &fetch_large);
    assert!(
        issues_large
            .iter()
            .any(|i| i.code == RuleId::WarnPerfLargeHtmlPayload),
        "Expected WARN_PERF_LARGE_HTML_PAYLOAD on payload > 1.5MB"
    );

    // 2. Excessive HTML Payload (> 3.0 MB)
    let fetch_excessive =
        make_test_fetch(url, 200, "<html></html>", 3_500_000, make_secure_headers());
    let issues_excessive = evaluate_page(&page, &fetch_excessive);
    assert!(
        issues_excessive
            .iter()
            .any(|i| i.code == RuleId::ErrPerfExcessiveHtmlPayload),
        "Expected ERR_PERF_EXCESSIVE_HTML_PAYLOAD on payload > 3.0MB"
    );

    // 3. Alt text > 125 chars
    let mut page_img = ParsedPage::default();
    page_img.images.push(ImageResource {
        src_url: "https://example.com/img.jpg".to_string(),
        alt_text: Some("A".repeat(130)),
        width: Some(100),
        height: Some(100),
        size_bytes: Some(1000),
        has_dimensions: true,
        is_broken: false,
    });
    let fetch_clean = make_test_fetch(url, 200, "<html></html>", 100, make_secure_headers());
    let issues_img = evaluate_page(&page_img, &fetch_clean);
    assert!(
        issues_img
            .iter()
            .any(|i| i.code == RuleId::WarnImgAltTooLong),
        "Expected WARN_IMG_ALT_TOO_LONG on image alt text > 125 characters"
    );
}
