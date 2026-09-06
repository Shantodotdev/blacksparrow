//! # Technical SEO Rules Engine (Single-Page In-Flight) Tests
//!
//! Comprehensive integration test suite for Phase 5 in-flight document-level audit checks.
//! Verifies positive detections, negative non-detections, exact boundary conditions,
//! real-world production fixtures, and adversarial malformed inputs.

use compact_str::CompactString;
use reqwest::header::{HeaderMap, HeaderValue};
use seo_lens::core::models::{ImageResource, IssueCategory, RobotsFlags, SchemaRecord, Severity};
use seo_lens::crawler::client::FetchResult;
use seo_lens::parser::{parse_html, ParsedPage};
use seo_lens::rules::catalog::{get_rule, RuleId};
use seo_lens::rules::evaluate_page;

/// Creates a populated [`FetchResult`] with customizable status, headers, and latency.
fn make_mock_fetch_result(
    url: &str,
    final_url: &str,
    status_code: u16,
    body: &str,
    headers: HeaderMap,
    ttfb_ms: u32,
    waf_detected: Option<&'static str>,
) -> FetchResult {
    FetchResult {
        url: url.to_string(),
        final_url: final_url.to_string(),
        status_code,
        headers,
        content_type: CompactString::new("text/html; charset=utf-8"),
        body: body.to_string(),
        body_bytes: body.as_bytes().to_vec(),
        size_bytes: body.len() as u32,
        ttfb_ms,
        redirect_chain: Vec::new(),
        waf_detected,
    }
}

/// Creates a standard set of secure HTTP response headers.
fn make_default_clean_headers() -> HeaderMap {
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

/// Helper to construct an [`ImageResource`] with sensible test defaults.
fn make_test_image(src_url: &str, alt_text: Option<&str>, has_dimensions: bool) -> ImageResource {
    ImageResource {
        src_url: src_url.to_string(),
        alt_text: alt_text.map(|s| s.to_string()),
        width: if has_dimensions { Some(400) } else { None },
        height: if has_dimensions { Some(300) } else { None },
        size_bytes: Some(1024),
        has_dimensions,
        is_broken: false,
    }
}

// =========================================================================
// 1. Catalog Integrity & Typed Lookups
// =========================================================================

#[test]
fn test_rules_catalog_integrity() {
    // Strongly-typed enum lookup
    let title_missing = get_rule(RuleId::ErrTitleMissing);
    assert_eq!(title_missing.severity, Severity::Critical);
    assert_eq!(title_missing.category, IssueCategory::TitleMetadata);
    assert_eq!(title_missing.code(), "ERR_TITLE_MISSING");

    let h1_multiple = get_rule(RuleId::WarnH1Multiple);
    assert_eq!(h1_multiple.severity, Severity::Warning);
    assert_eq!(h1_multiple.category, IssueCategory::Headings);

    // String code resolution
    assert!(seo_lens::rules::catalog::get_rule_by_code("ERR_TITLE_MISSING").is_some());
    assert!(seo_lens::rules::catalog::get_rule_by_code("INVALID_CODE").is_none());

    // Enum from_code resolution
    assert_eq!(
        RuleId::from_code("ERR_TITLE_MISSING"),
        Some(RuleId::ErrTitleMissing)
    );
    assert_eq!(RuleId::from_code("NON_EXISTENT"), None);
}

// =========================================================================
// 2. Title Rules Exhaustive Boundaries
// =========================================================================

#[test]
fn test_titles_rules_exhaustive_boundaries() {
    let fetch = make_mock_fetch_result(
        "https://example.com/test",
        "https://example.com/test",
        200,
        "",
        make_default_clean_headers(),
        100,
        None,
    );

    // None -> ERR_TITLE_MISSING
    let mut page = ParsedPage::default();
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::ErrTitleMissing));

    // Empty string -> ERR_TITLE_MISSING
    page.title = Some("".to_string());
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::ErrTitleMissing));

    // Whitespace only -> ERR_TITLE_MISSING
    page.title = Some("   \t\n  ".to_string());
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::ErrTitleMissing));

    // Whitespace padded -> WARN_TITLE_WHITESPACE_PADDED
    page.title = Some("  Valid Length Document Title Here  ".to_string());
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnTitleWhitespacePadded));

    // 29 chars -> WARN_TITLE_TOO_SHORT
    page.title = Some("12345678901234567890123456789".to_string());
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::WarnTitleTooShort));

    // Exactly 30 chars -> OK (neither short nor long)
    page.title = Some("123456789012345678901234567890".to_string());
    let issues = evaluate_page(&page, &fetch);
    assert!(!issues.iter().any(|i| i.code == RuleId::WarnTitleTooShort));
    assert!(!issues.iter().any(|i| i.code == RuleId::WarnTitleTooLong));

    // Exactly 60 chars -> OK
    let title_60 = "A".repeat(60);
    page.title = Some(title_60);
    let issues = evaluate_page(&page, &fetch);
    assert!(!issues.iter().any(|i| i.code == RuleId::WarnTitleTooShort));
    assert!(!issues.iter().any(|i| i.code == RuleId::WarnTitleTooLong));

    // Exactly 61 chars -> WARN_TITLE_TOO_LONG
    let title_61 = "A".repeat(61);
    page.title = Some(title_61);
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::WarnTitleTooLong));

    // Optimal title (45 chars) -> 0 title issues
    page.title = Some("High Performance Rust Web Crawler & SEO Engine".to_string());
    let issues = evaluate_page(&page, &fetch);
    let title_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.category == IssueCategory::TitleMetadata && i.code.as_str().contains("TITLE"))
        .collect();
    assert!(
        title_issues.is_empty(),
        "Optimal title triggered unexpected issues: {:?}",
        title_issues
    );
}

// =========================================================================
// 3. Meta Description Rules Exhaustive Boundaries
// =========================================================================

#[test]
fn test_descriptions_rules_exhaustive_boundaries() {
    let fetch = make_mock_fetch_result(
        "https://example.com/test",
        "https://example.com/test",
        200,
        "",
        make_default_clean_headers(),
        100,
        None,
    );

    // None -> WARN_META_DESC_MISSING
    let mut page = ParsedPage::default();
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::WarnMetaDescMissing));

    // Empty string -> WARN_META_DESC_MISSING
    page.meta_description = Some("".to_string());
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::WarnMetaDescMissing));

    // Whitespace only -> WARN_META_DESC_MISSING
    page.meta_description = Some("   \t  ".to_string());
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::WarnMetaDescMissing));

    // 69 chars -> WARN_META_DESC_TOO_SHORT
    page.meta_description = Some("A".repeat(69));
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnMetaDescTooShort));

    // Exactly 70 chars -> OK
    page.meta_description = Some("A".repeat(70));
    let issues = evaluate_page(&page, &fetch);
    assert!(!issues
        .iter()
        .any(|i| i.code == RuleId::WarnMetaDescTooShort));
    assert!(!issues.iter().any(|i| i.code == RuleId::WarnMetaDescTooLong));

    // Exactly 160 chars -> OK
    page.meta_description = Some("A".repeat(160));
    let issues = evaluate_page(&page, &fetch);
    assert!(!issues
        .iter()
        .any(|i| i.code == RuleId::WarnMetaDescTooShort));
    assert!(!issues.iter().any(|i| i.code == RuleId::WarnMetaDescTooLong));

    // Exactly 161 chars -> WARN_META_DESC_TOO_LONG
    page.meta_description = Some("A".repeat(161));
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::WarnMetaDescTooLong));

    // Clean description (120 chars) -> 0 description issues
    page.meta_description = Some("Discover how to build high-performance technical SEO audit tools using Rust, Tokio, and streaming HTML parser pipelines.".to_string());
    let issues = evaluate_page(&page, &fetch);
    let desc_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.code.as_str().contains("META_DESC"))
        .collect();
    assert!(
        desc_issues.is_empty(),
        "Optimal description triggered unexpected issues: {:?}",
        desc_issues
    );
}

// =========================================================================
// 4. Headings & Structural Hierarchy Rules
// =========================================================================

#[test]
fn test_headings_rules_exhaustive_scenarios() {
    let fetch = make_mock_fetch_result(
        "https://example.com/test",
        "https://example.com/test",
        200,
        "",
        make_default_clean_headers(),
        100,
        None,
    );

    // 1. Missing H1
    let mut page = ParsedPage {
        h1_count: 0,
        ..Default::default()
    };
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::ErrH1Missing));

    // 2. Empty H1
    page.h1_count = 1;
    page.h1_primary = Some("   ".to_string());
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::WarnH1Empty));

    // 3. Multiple H1 tags
    page.h1_count = 2;
    page.h1_primary = Some("First Primary Headline".to_string());
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::WarnH1Multiple));

    // 4. Boundary H1 Length: 70 chars (OK) vs 71 chars (Too Long)
    page.h1_count = 1;
    page.h1_primary = Some("A".repeat(70));
    let issues = evaluate_page(&page, &fetch);
    assert!(!issues.iter().any(|i| i.code == RuleId::WarnH1TooLong));

    page.h1_primary = Some("A".repeat(71));
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::WarnH1TooLong));

    // 5. Skipped Hierarchy: H1 -> H3 without any H2
    page.h1_primary = Some("Valid Headline".to_string());
    page.h2_headings = Vec::new();
    page.h3_headings = vec!["Skipped Subtopic".to_string()];
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnHeadingHierarchySkipped));

    // 6. Valid Sequential Hierarchy: H1 -> H2 -> H3
    page.h2_headings = vec!["Intermediate Section".to_string()];
    let issues = evaluate_page(&page, &fetch);
    let heading_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.category == IssueCategory::Headings)
        .collect();
    assert!(
        heading_issues.is_empty(),
        "Valid heading hierarchy triggered unexpected issues: {:?}",
        heading_issues
    );
}

// =========================================================================
// 5. Canonicalization Rules
// =========================================================================

#[test]
fn test_canonical_rules_exhaustive_scenarios() {
    let fetch = make_mock_fetch_result(
        "https://example.com/page",
        "https://example.com/page",
        200,
        "",
        make_default_clean_headers(),
        100,
        None,
    );

    // Missing canonical
    let mut page = ParsedPage {
        canonical_url: None,
        ..Default::default()
    };
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnCanonicalMissing));

    // Relative canonical
    page.canonical_url = Some("https://example.com/subpath".to_string());
    page.is_canonical_relative = true;
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::ErrCanonicalRelative));

    // Canonical URL mismatch
    page.is_canonical_relative = false;
    page.canonical_url = Some("https://example.com/different-page".to_string());
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::AlertCanonicalMismatch));

    // Clean self-referencing absolute canonical
    page.canonical_url = Some("https://example.com/page".to_string());
    let issues = evaluate_page(&page, &fetch);
    assert!(!issues
        .iter()
        .any(|i| i.code == RuleId::AlertCanonicalMismatch));
    assert!(!issues
        .iter()
        .any(|i| i.code == RuleId::ErrCanonicalRelative));

    // Trailing slash tolerance: /page/ matches /page
    page.canonical_url = Some("https://example.com/page/".to_string());
    let issues = evaluate_page(&page, &fetch);
    assert!(!issues
        .iter()
        .any(|i| i.code == RuleId::AlertCanonicalMismatch));

    // Case-insensitive domain tolerance
    page.canonical_url = Some("https://EXAMPLE.COM/page".to_string());
    let issues = evaluate_page(&page, &fetch);
    assert!(!issues
        .iter()
        .any(|i| i.code == RuleId::AlertCanonicalMismatch));
}

// =========================================================================
// 6. Directives & Robots Directives Rules
// =========================================================================

#[test]
fn test_directives_and_robots_exhaustive() {
    let fetch = make_mock_fetch_result(
        "https://example.com/page",
        "https://example.com/page",
        200,
        "",
        make_default_clean_headers(),
        100,
        None,
    );

    // Individual flags
    let mut page = ParsedPage {
        robots_flags: RobotsFlags::NOINDEX,
        ..Default::default()
    };
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::AlertIndexingBlockedNoindex));
    assert!(!issues
        .iter()
        .any(|i| i.code == RuleId::WarnLinkEquityBlockedNofollow));

    page.robots_flags = RobotsFlags::NOFOLLOW;
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnLinkEquityBlockedNofollow));

    page.robots_flags = RobotsFlags::NOARCHIVE;
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnNoarchivePresent));

    page.robots_flags = RobotsFlags::NOSNIPPET;
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnNosnippetPresent));

    // Combined directives
    page.robots_flags = RobotsFlags::NOINDEX
        | RobotsFlags::NOFOLLOW
        | RobotsFlags::NOARCHIVE
        | RobotsFlags::NOSNIPPET;
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::AlertIndexingBlockedNoindex));
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnLinkEquityBlockedNofollow));
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnNoarchivePresent));
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnNosnippetPresent));

    // Permissive directives: empty or index,follow
    page.robots_flags = RobotsFlags::empty();
    let issues = evaluate_page(&page, &fetch);
    let directive_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.category == IssueCategory::Indexability)
        .collect();
    assert!(
        directive_issues.is_empty(),
        "Permissive robots triggered unexpected issues: {:?}",
        directive_issues
    );
}

// =========================================================================
// 7. Security & Transport Encryption Rules
// =========================================================================

#[test]
fn test_security_and_transport_exhaustive() {
    let page = ParsedPage::default();

    // 1. Insecure HTTP scheme
    let fetch_http = make_mock_fetch_result(
        "http://example.com/insecure",
        "http://example.com/insecure",
        200,
        "",
        make_default_clean_headers(),
        100,
        None,
    );
    let issues = evaluate_page(&page, &fetch_http);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::ErrSecurityInsecureHttp && i.severity == Severity::Critical));

    // 2. Missing Individual Security Headers on HTTPS
    let mut incomplete_headers = HeaderMap::new();
    // Missing HSTS, CSP, X-Frame-Options, X-Content-Type-Options
    let fetch_no_headers = make_mock_fetch_result(
        "https://example.com/secure",
        "https://example.com/secure",
        200,
        "",
        incomplete_headers.clone(),
        100,
        None,
    );
    let issues = evaluate_page(&page, &fetch_no_headers);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnSecurityMissingHsts));
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnSecurityMissingCsp));
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnSecurityMissingXFrameOptions));
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnSecurityMissingXContentType));
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnSecurityMissingReferrerPolicy));

    // 3. Mixed Content Subresources (images, forms, iframes)
    incomplete_headers = make_default_clean_headers();
    let mut page_mixed_image = ParsedPage::default();
    page_mixed_image.images.push(make_test_image(
        "http://insecure.example.com/tracking.png",
        Some("Insecure pixel"),
        true,
    ));
    let fetch_clean_headers = make_mock_fetch_result(
        "https://example.com/page",
        "https://example.com/page",
        200,
        "",
        incomplete_headers.clone(),
        100,
        None,
    );
    let issues = evaluate_page(&page_mixed_image, &fetch_clean_headers);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::ErrSecurityMixedContent && i.severity == Severity::Critical));

    // Mixed content iframe in HTML body
    let fetch_mixed_iframe = make_mock_fetch_result(
        "https://example.com/page",
        "https://example.com/page",
        200,
        "<iframe src=\"http://insecure-domain.com/widget\"></iframe>",
        incomplete_headers.clone(),
        100,
        None,
    );
    let issues = evaluate_page(&page, &fetch_mixed_iframe);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::ErrSecurityMixedContent));

    // Mixed content form action in HTML body
    let fetch_mixed_form = make_mock_fetch_result(
        "https://example.com/page",
        "https://example.com/page",
        200,
        "<form action=\"http://insecure-backend.com/login\"></form>",
        incomplete_headers.clone(),
        100,
        None,
    );
    let issues = evaluate_page(&page, &fetch_mixed_form);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::ErrSecurityMixedContent));

    // Fully secure HTTPS page -> 0 security issues
    let issues = evaluate_page(&page, &fetch_clean_headers);
    let sec_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.category == IssueCategory::Security)
        .collect();
    assert!(
        sec_issues.is_empty(),
        "Fully secured page triggered unexpected security issues: {:?}",
        sec_issues
    );
}

// =========================================================================
// 8. Image Optimization & Layout Stability (CLS) Rules
// =========================================================================

#[test]
fn test_image_and_layout_stability_rules() {
    let fetch = make_mock_fetch_result(
        "https://example.com/page",
        "https://example.com/page",
        200,
        "",
        make_default_clean_headers(),
        100,
        None,
    );

    let mut page = ParsedPage::default();

    // 1. Missing alt text
    page.images
        .push(make_test_image("https://example.com/photo.jpg", None, true));
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::WarnImageMissingAlt));

    // 2. Empty/whitespace alt text
    page.images.clear();
    page.images.push(make_test_image(
        "https://example.com/photo.jpg",
        Some("   "),
        true,
    ));
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::WarnImageMissingAlt));

    // 3. Missing dimensions (CLS risk)
    page.images.clear();
    page.images.push(make_test_image(
        "https://example.com/photo.jpg",
        Some("Valid description"),
        false,
    ));
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnImageMissingDimensions));

    // 4. Large inline Data URI
    page.images.clear();
    page.images.push(make_test_image(
        "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk",
        Some("Data URI"),
        true,
    ));
    let issues = evaluate_page(&page, &fetch);
    assert!(issues.iter().any(|i| i.code == RuleId::WarnImageDataUri));

    // 5. Clean image -> 0 image issues
    page.images.clear();
    page.images.push(make_test_image(
        "https://example.com/photo.webp",
        Some("High-performance server diagram"),
        true,
    ));
    let issues = evaluate_page(&page, &fetch);
    let image_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.code.as_str().starts_with("WARN_IMAGE_"))
        .collect();
    assert!(
        image_issues.is_empty(),
        "Optimized image triggered unexpected issues: {:?}",
        image_issues
    );
}

// =========================================================================
// 9. Mobile UX & Viewport Configuration Rules
// =========================================================================

#[test]
fn test_mobile_ux_and_viewport_rules() {
    let fetch = make_mock_fetch_result(
        "https://example.com/page",
        "https://example.com/page",
        200,
        "",
        make_default_clean_headers(),
        100,
        None,
    );

    let mut page = ParsedPage {
        viewport: None,
        ..Default::default()
    };

    // 1. Missing viewport
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::ErrMobileNoViewport && i.severity == Severity::Critical));

    // 2. Viewport disables user scaling: user-scalable=no
    page.viewport = Some(CompactString::new(
        "width=device-width, initial-scale=1.0, user-scalable=no",
    ));
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnMobileViewportNonScalable));

    // Viewport disables user scaling: user-scalable=0
    page.viewport = Some(CompactString::new(
        "width=device-width, initial-scale=1.0, user-scalable=0",
    ));
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnMobileViewportNonScalable));

    // Viewport disables user scaling: maximum-scale=1.0
    page.viewport = Some(CompactString::new(
        "width=device-width, initial-scale=1.0, maximum-scale=1.0",
    ));
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnMobileViewportNonScalable));

    // 3. Clean responsive viewport
    page.viewport = Some(CompactString::new("width=device-width, initial-scale=1.0"));
    let issues = evaluate_page(&page, &fetch);
    let mobile_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.category == IssueCategory::MobileUx && i.code.as_str().contains("VIEWPORT"))
        .collect();
    assert!(
        mobile_issues.is_empty(),
        "Clean viewport triggered unexpected mobile issues: {:?}",
        mobile_issues
    );
}

// =========================================================================
// 10. Structured Data & Schema.org Validation Rules
// =========================================================================

#[test]
fn test_structured_data_and_schema_rules() {
    let fetch = make_mock_fetch_result(
        "https://example.com/page",
        "https://example.com/page",
        200,
        "",
        make_default_clean_headers(),
        100,
        None,
    );

    let mut page = ParsedPage::default();

    // 1. JSON-LD syntax error
    page.schemas.push(SchemaRecord {
        schema_type: CompactString::new("InvalidJson"),
        raw_json: "{ malformed json".to_string(),
        is_valid_json: false,
        is_google_eligible: false,
        missing_required_fields: Vec::new(),
    });
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::ErrSchemaSyntaxError && i.severity == Severity::Alert));

    // 2. Schema missing required fields for Rich Results
    page.schemas.clear();
    page.schemas.push(SchemaRecord {
        schema_type: CompactString::new("Product"),
        raw_json: "{}".to_string(),
        is_valid_json: true,
        is_google_eligible: true,
        missing_required_fields: vec![CompactString::new("name"), CompactString::new("offers")],
    });
    let issues = evaluate_page(&page, &fetch);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnSchemaMissingRequiredFields));

    // 3. Valid complete schema
    page.schemas.clear();
    page.schemas.push(SchemaRecord {
        schema_type: CompactString::new("Article"),
        raw_json: r#"{"@type":"Article","headline":"Clean"}"#.to_string(),
        is_valid_json: true,
        is_google_eligible: true,
        missing_required_fields: Vec::new(),
    });
    let issues = evaluate_page(&page, &fetch);
    let schema_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.category == IssueCategory::StructuredData)
        .collect();
    assert!(
        schema_issues.is_empty(),
        "Valid schema triggered unexpected issues: {:?}",
        schema_issues
    );
}

// =========================================================================
// 11. Content Quality & AI Search (GEO) Rules
// =========================================================================

#[test]
fn test_content_quality_and_geo_rules() {
    let mut page = ParsedPage {
        word_count: 199,
        ..Default::default()
    };

    // 1. Thin Content Boundary: 199 words vs 200 words on 200 OK
    let fetch_200 = make_mock_fetch_result(
        "https://example.com/page",
        "https://example.com/page",
        200,
        "clean body",
        make_default_clean_headers(),
        100,
        None,
    );
    let issues = evaluate_page(&page, &fetch_200);
    assert!(issues.iter().any(|i| i.code == RuleId::WarnContentThin));

    page.word_count = 200;
    let issues = evaluate_page(&page, &fetch_200);
    assert!(!issues.iter().any(|i| i.code == RuleId::WarnContentThin));

    // Non-200 responses (e.g. 404, 301) should NOT flag thin content
    page.word_count = 50;
    let fetch_404 = make_mock_fetch_result(
        "https://example.com/missing",
        "https://example.com/missing",
        404,
        "not found",
        make_default_clean_headers(),
        100,
        None,
    );
    let issues = evaluate_page(&page, &fetch_404);
    assert!(!issues.iter().any(|i| i.code == RuleId::WarnContentThin));

    // 2. Placeholder Lorem Ipsum Text
    let fetch_lorem = make_mock_fetch_result(
        "https://example.com/page",
        "https://example.com/page",
        200,
        "<p>Lorem Ipsum dolor sit amet, consectetur adipiscing elit.</p>",
        make_default_clean_headers(),
        100,
        None,
    );
    page.word_count = 500;
    let issues = evaluate_page(&page, &fetch_lorem);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::WarnLoremIpsumDetected));
}

// =========================================================================
// 12. HTTP Transport & Telemetry Exhaustive Rules
// =========================================================================

#[test]
fn test_http_transport_and_telemetry_exhaustive() {
    let parsed = ParsedPage::default();

    // 4xx Client Errors: 400, 401, 403, 404, 410, 429
    for code in [400, 401, 403, 404, 410, 429] {
        let fetch = make_mock_fetch_result(
            "https://example.com/test",
            "https://example.com/test",
            code,
            "",
            HeaderMap::new(),
            100,
            None,
        );
        let issues = evaluate_page(&parsed, &fetch);
        assert!(
            issues.iter().any(
                |i| i.code == RuleId::ErrHttp4xxClientError && i.severity == Severity::Critical
            ),
            "Failed to flag HTTP 4xx for status code {code}"
        );
    }

    // 5xx Server Errors: 500, 502, 503, 504
    for code in [500, 502, 503, 504] {
        let fetch = make_mock_fetch_result(
            "https://example.com/test",
            "https://example.com/test",
            code,
            "",
            HeaderMap::new(),
            100,
            None,
        );
        let issues = evaluate_page(&parsed, &fetch);
        assert!(
            issues.iter().any(
                |i| i.code == RuleId::ErrHttp5xxServerError && i.severity == Severity::Critical
            ),
            "Failed to flag HTTP 5xx for status code {code}"
        );
    }

    // 301 Permanent Redirect
    let fetch_301 = make_mock_fetch_result(
        "https://example.com/old",
        "https://example.com/new",
        301,
        "",
        HeaderMap::new(),
        100,
        None,
    );
    let issues = evaluate_page(&parsed, &fetch_301);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::InfoHttp301PermanentRedirect && i.severity == Severity::Notice));

    // 302 Temporary Redirect
    let fetch_302 = make_mock_fetch_result(
        "https://example.com/old",
        "https://example.com/new",
        302,
        "",
        HeaderMap::new(),
        100,
        None,
    );
    let issues = evaluate_page(&parsed, &fetch_302);
    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::InfoHttp302TemporaryRedirect
                && i.severity == Severity::Warning)
    );

    // 307 & 308 Redirects
    for code in [307, 308] {
        let fetch = make_mock_fetch_result(
            "https://example.com/old",
            "https://example.com/new",
            code,
            "",
            HeaderMap::new(),
            100,
            None,
        );
        let issues = evaluate_page(&parsed, &fetch);
        assert!(
            issues.iter().any(
                |i| i.code == RuleId::InfoHttp307_308Redirect && i.severity == Severity::Notice
            ),
            "Failed to flag 307/308 redirect for status code {code}"
        );
    }

    // WAF Bot Challenge
    let fetch_waf = make_mock_fetch_result(
        "https://example.com/protected",
        "https://example.com/protected",
        403,
        "Checking browser...",
        HeaderMap::new(),
        100,
        Some("Cloudflare Managed Challenge"),
    );
    let issues = evaluate_page(&parsed, &fetch_waf);
    assert!(issues
        .iter()
        .any(|i| i.code == RuleId::AlertWafBotChallenge && i.severity == Severity::Alert));

    // TTFB Latency Boundary: 1800ms (OK) vs 1801ms (Slow TTFB)
    let fetch_1800 = make_mock_fetch_result(
        "https://example.com/page",
        "https://example.com/page",
        200,
        "",
        make_default_clean_headers(),
        1800,
        None,
    );
    let issues = evaluate_page(&parsed, &fetch_1800);
    assert!(!issues.iter().any(|i| i.code == RuleId::WarnSlowTtfb));

    let fetch_1801 = make_mock_fetch_result(
        "https://example.com/page",
        "https://example.com/page",
        200,
        "",
        make_default_clean_headers(),
        1801,
        None,
    );
    let issues = evaluate_page(&parsed, &fetch_1801);
    assert!(issues.iter().any(|i| i.code == RuleId::WarnSlowTtfb));
}

// =========================================================================
// 13. Pristine Good Benchmark Fixture Verification
// =========================================================================

#[test]
fn test_good_page_has_zero_critical_or_alert_issues() {
    let html = include_str!("fixtures/good_page.html");
    let parsed = parse_html(html, "https://example.com/blog/technical-seo-guide")
        .expect("Failed to parse good_page fixture");

    let fetch = make_mock_fetch_result(
        "https://example.com/blog/technical-seo-guide",
        "https://example.com/blog/technical-seo-guide",
        200,
        html,
        make_default_clean_headers(),
        250,
        None,
    );

    let issues = evaluate_page(&parsed, &fetch);

    let critical_or_alert: Vec<_> = issues
        .iter()
        .filter(|i| i.severity == Severity::Critical || i.severity == Severity::Alert)
        .collect();

    assert!(
        critical_or_alert.is_empty(),
        "Good page unexpectedly triggered critical/alert issues: {:#?}",
        critical_or_alert
    );
}

// =========================================================================
// 14. Broken SEO Benchmark Fixture Defect Matrix
// =========================================================================

#[test]
fn test_broken_page_detects_all_intentional_issues() {
    let html = include_str!("fixtures/broken_seo_page.html");
    let parsed = parse_html(html, "https://example.com/broken-page")
        .expect("Failed to parse broken_seo_page fixture");

    let fetch = make_mock_fetch_result(
        "https://example.com/broken-page",
        "https://example.com/broken-page",
        200,
        html,
        HeaderMap::new(), // Intentionally missing all security headers
        400,
        None,
    );

    let issues = evaluate_page(&parsed, &fetch);
    let codes: Vec<RuleId> = issues.iter().map(|i| i.code).collect();

    // Verify all 12 intentional defects in the fixture are identified
    let expected_defects = [
        RuleId::ErrMobileNoViewport,
        RuleId::WarnMetaDescMissing,
        RuleId::AlertIndexingBlockedNoindex,
        RuleId::WarnLinkEquityBlockedNofollow,
        RuleId::ErrCanonicalRelative,
        RuleId::WarnH1Multiple,
        RuleId::WarnHeadingHierarchySkipped,
        RuleId::ErrSchemaSyntaxError,
        RuleId::WarnImageMissingAlt,
        RuleId::WarnImageMissingDimensions,
        RuleId::WarnSecurityMissingHsts,
        RuleId::WarnLoremIpsumDetected,
    ];

    for defect in expected_defects {
        assert!(
            codes.contains(&defect),
            "Broken fixture failed to detect expected issue '{defect}'"
        );
    }
}

// =========================================================================
// 15. Real-World E-commerce Fixture Verification
// =========================================================================

#[test]
fn test_ecommerce_product_page_audit() {
    let html = include_str!("fixtures/ecommerce_product_page.html");
    let parsed = parse_html(
        html,
        "https://example.com/books/rust-performance-engineering",
    )
    .expect("Failed to parse ecommerce product fixture");

    let fetch = make_mock_fetch_result(
        "https://example.com/books/rust-performance-engineering",
        "https://example.com/books/rust-performance-engineering",
        200,
        html,
        make_default_clean_headers(),
        180,
        None,
    );

    let issues = evaluate_page(&parsed, &fetch);

    // E-commerce fixture has valid Product schema, self-referencing canonical,
    // responsive viewport, and optimized images. Should have 0 Critical and 0 Alert.
    let critical_or_alert: Vec<_> = issues
        .iter()
        .filter(|i| i.severity == Severity::Critical || i.severity == Severity::Alert)
        .collect();

    assert!(
        critical_or_alert.is_empty(),
        "E-commerce product fixture triggered critical/alert issues: {:#?}",
        critical_or_alert
    );

    // Verify schema is recognized as valid
    assert!(!parsed.schemas.is_empty());
    assert!(parsed.schemas.iter().any(|s| s.is_valid_json));
}

// =========================================================================
// 16. Multilingual Hreflang Fixture Verification
// =========================================================================

#[test]
fn test_international_hreflang_page_audit() {
    let html = include_str!("fixtures/international_hreflang_page.html");
    let parsed = parse_html(html, "https://example.com/fr/audit-seo")
        .expect("Failed to parse international hreflang fixture");

    let fetch = make_mock_fetch_result(
        "https://example.com/fr/audit-seo",
        "https://example.com/fr/audit-seo",
        200,
        html,
        make_default_clean_headers(),
        220,
        None,
    );

    let issues = evaluate_page(&parsed, &fetch);

    // Multilingual page has proper canonical, UTF-8 French text, valid title & description.
    let critical_or_alert: Vec<_> = issues
        .iter()
        .filter(|i| i.severity == Severity::Critical || i.severity == Severity::Alert)
        .collect();

    assert!(
        critical_or_alert.is_empty(),
        "International fixture triggered critical/alert issues: {:#?}",
        critical_or_alert
    );

    // Verify canonical matches
    assert_eq!(
        parsed.canonical_url.as_deref(),
        Some("https://example.com/fr/audit-seo")
    );
    assert!(!parsed.is_canonical_relative);
}

// =========================================================================
// 17. Adversarial & Malformed HTML Stress Tests (Zero-Panic Guarantee)
// =========================================================================

#[test]
fn test_adversarial_malformed_html_zero_panics() {
    let fetch = make_mock_fetch_result(
        "https://example.com/chaos",
        "https://example.com/chaos",
        200,
        "",
        make_default_clean_headers(),
        100,
        None,
    );

    let adversarial_inputs = [
        "",                                         // Completely empty string
        "   \t\r\n   ",                             // Whitespace only
        "<html><head><title>Unclosed",              // Truncated head
        "<div class=\"unclosed\"><span>No root",    // No html or head tags
        "<!DOCTYPE html><title></title><body>",     // Empty title tag
        "<img src=\"\">",                           // Empty image tag
        "<script type=\"application/ld+json\">",    // Unclosed script
        "<h1>One</h1><h1>Two</h1><h1>Three</h1>",   // Multiple H1s with no body
        "<h3>Skipped</h3>",                         // Only H3, no H1 or H2
        "<html><body><!-- comment only --></body>", // Comment only
    ];

    for (idx, raw_html) in adversarial_inputs.iter().enumerate() {
        let parsed = parse_html(raw_html, "https://example.com/chaos")
            .unwrap_or_else(|_| ParsedPage::default());

        // Zero-panic assertion: evaluate_page must execute safely under all pathological inputs
        let issues = evaluate_page(&parsed, &fetch);
        assert!(
            !issues.is_empty(),
            "Adversarial case #{idx} should report audit findings"
        );
    }
}
