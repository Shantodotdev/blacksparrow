//! # Internationalization & Schema Rules Integration Tests
//!
//! Integration test suite validating:
//! - Category 9: Hreflang ISO 639-1 / RFC 5646 validation, cross-domain warnings, missing x-default, missing self-reference, missing html lang.
//! - Category 10: Structured data multiple Product entities and invalid ISO 8601 date formats.
//! - Category 11: GEO & AI search bot blocking in robots.txt and missing /llms.txt file.
//! - Category 12: Low internal PageRank hub detection in internal graph topology.

use compact_str::CompactString;
use hashbrown::HashMap;
use seo_lens::core::models::{HreflangTag, PageReport, RuleId, SchemaRecord};
use seo_lens::graph::{LinkEdgeType, SiteGraph};

#[test]
fn test_rule_hreflang_invalid_lang_code() {
    use seo_lens::rules::page::international::check_international;

    let mut issues = Vec::new();
    let hreflangs = vec![
        HreflangTag {
            lang_code: CompactString::new("en-US"), // Valid
            target_url: "https://example.com/en-us".to_string(),
            is_reciprocal: false,
        },
        HreflangTag {
            lang_code: CompactString::new("x-default"), // Valid
            target_url: "https://example.com/".to_string(),
            is_reciprocal: false,
        },
        HreflangTag {
            lang_code: CompactString::new("en-UK"), // Invalid: UK is not ISO 3166-1 alpha-2 (GB is)
            target_url: "https://example.com/en-uk".to_string(),
            is_reciprocal: false,
        },
        HreflangTag {
            lang_code: CompactString::new("1234"), // Invalid: numeric
            target_url: "https://example.com/num".to_string(),
            is_reciprocal: false,
        },
        HreflangTag {
            lang_code: CompactString::new("english"), // Invalid: full word (> 3 chars)
            target_url: "https://example.com/eng".to_string(),
            is_reciprocal: false,
        },
    ];

    check_international(
        Some("en"),
        &hreflangs,
        "https://example.com/en-us",
        &mut issues,
    );

    let invalid_code_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::ErrHreflangInvalidLangCode)
        .collect();

    assert_eq!(
        invalid_code_issues.len(),
        3,
        "Expected 3 invalid lang code issues (en-UK, 1234, english), got: {:?}",
        invalid_code_issues
    );
}

#[test]
fn test_rule_hreflang_cross_domain_and_self_reference_and_x_default() {
    use seo_lens::rules::page::international::check_international;

    let mut issues = Vec::new();
    let hreflangs = vec![
        HreflangTag {
            lang_code: CompactString::new("es"),
            target_url: "https://external-domain.com/es".to_string(), // Cross-domain!
            is_reciprocal: false,
        },
        HreflangTag {
            lang_code: CompactString::new("fr"),
            target_url: "https://example.com/fr".to_string(),
            is_reciprocal: false,
        },
        // Missing self-reference (https://example.com/en)
        // Missing x-default
    ];

    check_international(
        Some("en"),
        &hreflangs,
        "https://example.com/en",
        &mut issues,
    );

    // 1. Cross-domain alternate warning
    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnHreflangCrossDomain),
        "Expected WarnHreflangCrossDomain for external-domain.com"
    );

    // 2. Missing self-reference error
    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::ErrHreflangMissingSelfReference),
        "Expected ErrHreflangMissingSelfReference when page URL is not in hreflang tags"
    );

    // 3. Missing x-default warning
    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnHreflangMissingXDefault),
        "Expected WarnHreflangMissingXDefault when x-default is absent in hreflang cluster"
    );
}

#[test]
fn test_rule_html_lang_missing() {
    use seo_lens::rules::page::international::check_international;

    let mut issues = Vec::new();
    let hreflangs = Vec::new();

    // html lang is None
    check_international(None, &hreflangs, "https://example.com/", &mut issues);

    assert!(
        issues.iter().any(|i| i.code == RuleId::WarnHtmlLangMissing),
        "Expected WarnHtmlLangMissing when html lang is None"
    );

    // html lang is empty string
    let mut issues2 = Vec::new();
    check_international(
        Some("   "),
        &hreflangs,
        "https://example.com/",
        &mut issues2,
    );
    assert!(
        issues2
            .iter()
            .any(|i| i.code == RuleId::WarnHtmlLangMissing),
        "Expected WarnHtmlLangMissing when html lang is empty whitespace"
    );
}

#[test]
fn test_rule_schema_multiple_product_entities() {
    use seo_lens::parser::ParsedPage;
    use seo_lens::rules::page::schema_val::check_schemas;

    let mut issues = Vec::new();
    let mut parsed = ParsedPage::default();

    parsed.schemas.push(SchemaRecord {
        schema_type: CompactString::new("Product"),
        raw_json: r#"{"@context": "https://schema.org", "@type": "Product", "name": "Laptop A"}"#
            .to_string(),
        is_valid_json: true,
        is_google_eligible: true,
        missing_required_fields: vec![],
    });
    parsed.schemas.push(SchemaRecord {
        schema_type: CompactString::new("Product"),
        raw_json: r#"{"@context": "https://schema.org", "@type": "Product", "name": "Laptop B"}"#
            .to_string(),
        is_valid_json: true,
        is_google_eligible: true,
        missing_required_fields: vec![],
    });

    check_schemas(&parsed, "https://example.com/product", &mut issues);

    let multiple_product_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::WarnSchemaMultipleProductEntities)
        .collect();

    assert_eq!(
        multiple_product_issues.len(),
        1,
        "Expected WarnSchemaMultipleProductEntities when multiple Product schemas exist on one page"
    );
}

#[test]
fn test_rule_schema_invalid_date_format() {
    use seo_lens::parser::ParsedPage;
    use seo_lens::rules::page::schema_val::check_schemas;

    let mut issues = Vec::new();
    let mut parsed = ParsedPage::default();

    // Invalid datePublished using slash format "2026/05/01"
    parsed.schemas.push(SchemaRecord {
        schema_type: CompactString::new("Article"),
        raw_json: r#"{"@context": "https://schema.org", "@type": "Article", "headline": "Test", "datePublished": "2026/05/01"}"#.to_string(),
        is_valid_json: true,
        is_google_eligible: true,
        missing_required_fields: vec![],
    });

    check_schemas(&parsed, "https://example.com/article", &mut issues);

    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnSchemaInvalidDateFormat),
        "Expected WarnSchemaInvalidDateFormat for datePublished: '2026/05/01'"
    );

    // Valid ISO 8601 date: "2026-05-01" or "2026-05-01T14:30:00Z"
    let mut issues_valid = Vec::new();
    let mut parsed_valid = ParsedPage::default();
    parsed_valid.schemas.push(SchemaRecord {
        schema_type: CompactString::new("Article"),
        raw_json: r#"{"@context": "https://schema.org", "@type": "Article", "headline": "Test", "datePublished": "2026-05-01T14:30:00Z"}"#.to_string(),
        is_valid_json: true,
        is_google_eligible: true,
        missing_required_fields: vec![],
    });

    check_schemas(
        &parsed_valid,
        "https://example.com/article",
        &mut issues_valid,
    );
    assert!(
        !issues_valid
            .iter()
            .any(|i| i.code == RuleId::WarnSchemaInvalidDateFormat),
        "Valid ISO 8601 date must not trigger WarnSchemaInvalidDateFormat"
    );
}

#[test]
fn test_rule_low_internal_pagerank_hub() {
    use seo_lens::rules::graph::architecture::evaluate_architecture;

    let mut graph = SiteGraph::new();
    let mut pages = Vec::new();
    let mut pagerank = HashMap::new();

    // Hub page: out_degree = 55 (high outlinks), in_degree = 1 (isolated), pagerank = 0.00001 (very low)
    let hub_url = "https://example.com/isolated-hub";
    graph.add_node(hub_url, 200, 1, false);
    pagerank.insert(seo_lens::core::url::url_hash(hub_url), 0.00001);

    let hub_page = PageReport {
        url: hub_url.to_string(),
        status_code: 200,
        crawl_depth: 1,
        ..Default::default()
    };

    // Single inlink to the hub
    graph.add_node("https://example.com/", 200, 0, false);
    graph.add_edge(
        "https://example.com/",
        hub_url,
        LinkEdgeType::InternalHyperlink,
        false,
        "Hub",
    );

    // 55 outlinks from the hub to product pages
    for i in 0..55 {
        let child_url = format!("https://example.com/item-{}", i);
        graph.add_node(&child_url, 200, 2, false);
        graph.add_edge(
            hub_url,
            &child_url,
            LinkEdgeType::InternalHyperlink,
            false,
            "Item",
        );
    }

    pages.push(hub_page);

    let issues = evaluate_architecture(&pages, &graph, &pagerank);

    let low_pr_hub_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::WarnLowInternalPagerankHub)
        .collect();

    assert_eq!(
        low_pr_hub_issues.len(),
        1,
        "Expected WarnLowInternalPagerankHub for hub with 55 outlinks and low PageRank, got: {:?}",
        low_pr_hub_issues
    );
}

#[tokio::test]
async fn test_rule_ai_search_bots_blocked_and_llms_txt_missing() {
    use seo_lens::core::config::CrawlConfig;
    use seo_lens::crawler::run_crawl;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;
    let base_uri = mock_server.uri();

    // robots.txt disallowing PerplexityBot and GPTBot
    let robots_txt = r#"
User-agent: PerplexityBot
Disallow: /

User-agent: GPTBot
Disallow: /

User-agent: *
Allow: /
"#;

    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(robots_txt))
        .mount(&mock_server)
        .await;

    // llms.txt returns 404 Not Found
    Mock::given(method("GET"))
        .and(path("/llms.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    // Homepage
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html")
                .set_body_string("<!DOCTYPE html><html><head><title>Home Page</title></head><body><h1>Home</h1></body></html>"),
        )
        .mount(&mock_server)
        .await;

    let mut config = CrawlConfig::new(&format!("{}/", base_uri)).expect("Valid config");
    config.max_pages = 1;
    config.respect_robots = true;

    let result = run_crawl(&config, None)
        .await
        .expect("Crawl should succeed");

    // 1. Check AlertAiSearchBotsBlocked is raised because PerplexityBot / GPTBot are blocked
    assert!(
        result
            .issues
            .iter()
            .any(|i| i.code == RuleId::AlertAiSearchBotsBlocked),
        "Expected AlertAiSearchBotsBlocked when AI search bots are disallowed in robots.txt"
    );

    // 2. Check WarnLlmsTxtMissing is raised because /llms.txt returned 404
    assert!(
        result
            .issues
            .iter()
            .any(|i| i.code == RuleId::WarnLlmsTxtMissing),
        "Expected WarnLlmsTxtMissing when /llms.txt returns 404"
    );
}
