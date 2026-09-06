//! # Faceted Navigation & Parameter Spider Trap Tests
//!
//! Integration and unit tests validating:
//! 1. E-Commerce tracking parameter stripping (`scm`, `spm`, `pvid`, `clickTrackInfo`, etc.).
//! 2. Parameter classification into Tracking, Sorting/Display, and Content facets.
//! 3. Detection and firing of `ALERT_FACETED_SPIDER_TRAP`.
//! 4. Sorting facet pruning and parameter threshold enforcement in crawler engine.
//! 5. Canonical facet pruning defense against spider traps.

use seo_lens::core::config::CrawlConfig;
use seo_lens::core::models::{IssueCategory, RuleId, Severity};
use seo_lens::core::url::{
    classify_parameter, count_content_facets, has_sorting_facets, normalize_url, QueryParamCategory,
};
use seo_lens::crawler::client::FetchResult;
use seo_lens::crawler::engine::run_crawl;
use seo_lens::crawler::priority::calculate_url_importance;
use seo_lens::parser::streaming::parse_html;
use seo_lens::rules::page::evaluate_page_rules;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn test_ecommerce_tracking_parameter_stripping() {
    let dirty_url = "https://example.com/product/laptop-123?\
        spm=a2a0e.home.flashSale.1\
        &scm=1007.17743.123456.0\
        &pvid=89abcde-f012-3456-789a\
        &clickTrackInfo=ab_version%3D1\
        &wh_pid=12345\
        &hybrid=1\
        &data_prefetch=true\
        &fbclid=IwAR123456\
        &gclid=Cj0KCQj\
        &utm_custom_tag=promo\
        &category=laptops\
        &id=9988";

    let clean = normalize_url(dirty_url).expect("Normalization should succeed");

    // Legitimate content parameters must be preserved and sorted
    assert_eq!(
        clean,
        "https://example.com/product/laptop-123?category=laptops&id=9988"
    );
}

#[test]
fn test_query_parameter_classification() {
    // Tracking parameters
    assert_eq!(
        classify_parameter("utm_source"),
        QueryParamCategory::Tracking
    );
    assert_eq!(
        classify_parameter("utm_custom_id"),
        QueryParamCategory::Tracking
    );
    assert_eq!(classify_parameter("spm"), QueryParamCategory::Tracking);
    assert_eq!(classify_parameter("scm"), QueryParamCategory::Tracking);
    assert_eq!(classify_parameter("pvid"), QueryParamCategory::Tracking);
    assert_eq!(
        classify_parameter("clickTrackInfo"),
        QueryParamCategory::Tracking
    );
    assert_eq!(classify_parameter("wh_pid"), QueryParamCategory::Tracking);
    assert_eq!(classify_parameter("hybrid"), QueryParamCategory::Tracking);
    assert_eq!(
        classify_parameter("data_prefetch"),
        QueryParamCategory::Tracking
    );
    assert_eq!(classify_parameter("fbclid"), QueryParamCategory::Tracking);
    assert_eq!(classify_parameter("gclid"), QueryParamCategory::Tracking);
    assert_eq!(classify_parameter("source"), QueryParamCategory::Tracking);
    assert_eq!(
        classify_parameter("affiliate"),
        QueryParamCategory::Tracking
    );

    // Sorting & Display parameters
    assert_eq!(
        classify_parameter("sort"),
        QueryParamCategory::SortingOrDisplay
    );
    assert_eq!(
        classify_parameter("sort_by"),
        QueryParamCategory::SortingOrDisplay
    );
    assert_eq!(
        classify_parameter("order"),
        QueryParamCategory::SortingOrDisplay
    );
    assert_eq!(
        classify_parameter("orderby"),
        QueryParamCategory::SortingOrDisplay
    );
    assert_eq!(
        classify_parameter("dir"),
        QueryParamCategory::SortingOrDisplay
    );
    assert_eq!(
        classify_parameter("limit"),
        QueryParamCategory::SortingOrDisplay
    );
    assert_eq!(
        classify_parameter("per_page"),
        QueryParamCategory::SortingOrDisplay
    );
    assert_eq!(
        classify_parameter("view"),
        QueryParamCategory::SortingOrDisplay
    );
    assert_eq!(
        classify_parameter("layout"),
        QueryParamCategory::SortingOrDisplay
    );

    // Content Facets
    assert_eq!(
        classify_parameter("category"),
        QueryParamCategory::ContentFacet
    );
    assert_eq!(
        classify_parameter("brand"),
        QueryParamCategory::ContentFacet
    );
    assert_eq!(classify_parameter("tag"), QueryParamCategory::ContentFacet);
    assert_eq!(
        classify_parameter("color"),
        QueryParamCategory::ContentFacet
    );
    assert_eq!(
        classify_parameter("price_min"),
        QueryParamCategory::ContentFacet
    );
    assert_eq!(classify_parameter("q"), QueryParamCategory::ContentFacet);
}

#[test]
fn test_has_sorting_facets_and_count_content_facets() {
    let sorting_url = "https://example.com/shop?category=shoes&sort=price_asc";
    assert!(has_sorting_facets(sorting_url));
    assert_eq!(count_content_facets(sorting_url), 1); // only `category` is content facet

    let non_sorting_url = "https://example.com/shop?category=shoes&brand=nike&color=black";
    assert!(!has_sorting_facets(non_sorting_url));
    assert_eq!(count_content_facets(non_sorting_url), 3);

    let clean_url = "https://example.com/shop/shoes";
    assert!(!has_sorting_facets(clean_url));
    assert_eq!(count_content_facets(clean_url), 0);
}

#[test]
fn test_rule_alert_faceted_spider_trap_detection() {
    // 3 content facets > threshold of 2 -> triggers ALERT_FACETED_SPIDER_TRAP
    let trap_url = "https://example.com/catalog?cat=shoes&brand=nike&color=black";
    let html = r#"<!DOCTYPE html>
    <html lang="en">
    <head>
        <title>Shoes Filtered Catalog</title>
        <meta name="description" content="Find the best shoes in our huge filtered catalog.">
        <link rel="canonical" href="https://example.com/catalog?cat=shoes&brand=nike&color=black">
    </head>
    <body>
        <h1>Shoes Catalog</h1>
        <p>Filtered products listing...</p>
    </body>
    </html>"#;

    let parsed = parse_html(html, trap_url).expect("HTML should parse");
    let fetch = FetchResult {
        url: trap_url.to_string(),
        final_url: trap_url.to_string(),
        status_code: 200,
        headers: reqwest::header::HeaderMap::new(),
        body: html.to_string(),
        body_bytes: html.as_bytes().to_vec(),
        size_bytes: html.len() as u32,
        ttfb_ms: 50,
        redirect_chain: Vec::new(),
        content_type: "text/html".into(),
        waf_detected: None,
    };

    let issues = evaluate_page_rules(&parsed, &fetch);
    let trap_issue = issues
        .iter()
        .find(|i| i.code == RuleId::AlertFacetedSpiderTrap);

    assert!(
        trap_issue.is_some(),
        "Expected ALERT_FACETED_SPIDER_TRAP on URL with 3 content facet parameters"
    );
    let issue = trap_issue.unwrap();
    assert_eq!(issue.severity, Severity::Alert);
    assert_eq!(issue.category, IssueCategory::HttpTransport);

    // 2 content facets <= threshold of 2 -> must NOT trigger ALERT_FACETED_SPIDER_TRAP
    let benign_url = "https://example.com/catalog?cat=shoes&brand=nike";
    let benign_parsed = parse_html(html, benign_url).expect("HTML should parse");
    let mut benign_fetch = fetch.clone();
    benign_fetch.url = benign_url.to_string();
    benign_fetch.final_url = benign_url.to_string();

    let benign_issues = evaluate_page_rules(&benign_parsed, &benign_fetch);
    assert!(
        benign_issues
            .iter()
            .all(|i| i.code != RuleId::AlertFacetedSpiderTrap),
        "URL with <= 2 content facet parameters must NOT trigger ALERT_FACETED_SPIDER_TRAP"
    );
}

#[test]
fn test_crawl_config_faceted_navigation_defaults() {
    let config = CrawlConfig::new("https://example.com").unwrap();
    assert_eq!(config.max_query_params, 2);
    assert!(config.ignore_sorting_facets);
}

#[test]
fn test_sorting_facet_penalization_in_priority_heap() {
    let clean_category = "https://example.com/catalog/shoes";
    let sorted_category = "https://example.com/catalog/shoes?sort=price_asc";

    let score_clean = calculate_url_importance(clean_category, 1, 1, false);
    let score_sorted = calculate_url_importance(sorted_category, 1, 1, false);

    // Sorting facet parameter must incur standard query penalty + additional -500 sorting penalty
    assert!(
        score_clean - score_sorted >= 650,
        "Clean URL score ({}) should strongly exceed sorting facet URL score ({})",
        score_clean,
        score_sorted
    );
}

#[tokio::test]
async fn test_crawler_engine_faceted_pruning_and_canonical_defense() {
    let server = MockServer::start().await;
    let base_url = server.uri();

    let robots_txt = "User-agent: *\nAllow: /\n";
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(robots_txt))
        .mount(&server)
        .await;

    // Root page links to:
    // 1. /category?type=shoes (1 content facet -> permitted)
    // 2. /category?type=shoes&sort=price_asc (sorting facet -> must be pruned)
    // 3. /category?a=1&b=2&c=3 (3 content facets > max_query_params 2 -> must be pruned)
    // 4. /about (clean link -> permitted)
    let home_html = r#"<!DOCTYPE html>
        <html>
        <head><title>Home</title></head>
        <body>
            <h1>Home</h1>
            <a href="/category?type=shoes">Valid Category</a>
            <a href="/category?type=shoes&sort=price_asc">Sorting Trap</a>
            <a href="/category?a=1&b=2&c=3">Excessive Facet Trap</a>
            <a href="/about">About Us</a>
        </body>
        </html>"#;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(home_html))
        .mount(&server)
        .await;

    // Valid category page contains links to a deeper parameterized page, but is canonicalized to self
    let cat_html = format!(
        r#"<!DOCTYPE html>
        <html>
        <head>
            <title>Shoes</title>
            <link rel="canonical" href="{base_url}/category?type=shoes">
        </head>
        <body>
            <h1>Shoes</h1>
            <a href="/product/shoe-1">Shoe 1</a>
        </body>
        </html>"#
    );
    Mock::given(method("GET"))
        .and(path("/category"))
        .respond_with(ResponseTemplate::new(200).set_body_string(cat_html))
        .mount(&server)
        .await;

    let about_html = r#"<!DOCTYPE html>
        <html>
        <head><title>About</title></head>
        <body><h1>About</h1></body>
        </html>"#;
    Mock::given(method("GET"))
        .and(path("/about"))
        .respond_with(ResponseTemplate::new(200).set_body_string(about_html))
        .mount(&server)
        .await;

    let product_html = r#"<!DOCTYPE html>
        <html>
        <head><title>Shoe 1</title></head>
        <body><h1>Shoe 1</h1></body>
        </html>"#;
    Mock::given(method("GET"))
        .and(path("/product/shoe-1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(product_html))
        .mount(&server)
        .await;

    let mut config = CrawlConfig::new(&format!("{base_url}/")).unwrap();
    config.max_pages = 20;
    config.max_depth = 5;
    config.max_query_params = 2;
    config.ignore_sorting_facets = true;
    config.no_aimd = true;

    let result = run_crawl(&config, None).await.unwrap();

    let crawled_urls: Vec<&str> = result.pages.iter().map(|p| p.url.as_str()).collect();

    // Verify /category?type=shoes, /about, /product/shoe-1 were crawled
    assert!(crawled_urls
        .iter()
        .any(|u| u.contains("/category?type=shoes")));
    assert!(crawled_urls.iter().any(|u| u.ends_with("/about")));
    assert!(crawled_urls.iter().any(|u| u.contains("/product/shoe-1")));

    // Verify sorting facet URL was NEVER crawled
    assert!(
        !crawled_urls.iter().any(|u| u.contains("sort=")),
        "Sorting facet URL should be pruned by ignore_sorting_facets defense"
    );

    // Verify excessive facet URL was NEVER crawled
    assert!(
        !crawled_urls.iter().any(|u| u.contains("a=1&b=2&c=3")),
        "URL with 3 content facets should be pruned by max_query_params ceiling"
    );
}
