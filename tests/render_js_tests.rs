//! Integration tests for JavaScript-rendered DOM diff auditing.

use blacksparrow::core::config::CrawlConfig;
use blacksparrow::core::models::{RobotsFlags, RuleId};
use blacksparrow::crawler::engine::run_crawl;
use blacksparrow::extract::render::{RenderOptions, Renderer};
use blacksparrow::parser::parse_html;
use blacksparrow::rules::evaluate_js_diff;
use std::path::PathBuf;
use std::time::Duration;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const RAW_SPA_HTML: &str = include_str!("fixtures/render_js_spa.html");

/// The Chromium executable for the tests that render; `None` (after a notice) when unset.
fn test_chrome() -> Option<PathBuf> {
    let chrome = std::env::var_os("BLACKSPARROW_TEST_CHROME").map(PathBuf::from);
    if chrome.is_none() {
        eprintln!("skipped: BLACKSPARROW_TEST_CHROME is not set");
    }
    chrome
}

#[test]
fn rendered_spa_diffs_are_reported_from_a_meaningful_html_fixture() {
    let raw = parse_html(RAW_SPA_HTML, "https://example.test/products/widget")
        .expect("fixture raw HTML should parse");
    let rendered = parse_html(
        r#"<!doctype html><html lang="en"><head>
            <title>SPA route metadata</title>
            <meta name="description" content="This server-rendered description should remain accurate after hydration.">
            <link rel="canonical" href="https://example.test/products/widget/client-route">
            <meta name="robots" content="noindex">
        </head><body><main id="root"><a href="/products">Browse products</a></main></body></html>"#,
        "https://example.test/products/widget/client-route",
    )
    .expect("fixture rendered DOM should parse");

    let issues = evaluate_js_diff(
        &raw,
        &rendered,
        "https://example.test/products/widget",
        "https://example.test/products/widget/client-route",
        &["Uncaught Error: Hydration failed".to_string()],
    );

    for expected in [
        RuleId::ErrJsDiffCanonicalAltered,
        RuleId::ErrJsDiffNoindexInjected,
        RuleId::WarnJsDiffTitleMetaDesync,
        RuleId::AlertJsDiffVanishingContent,
        RuleId::AlertJsDiffLateRenderedLinks,
        RuleId::ErrJsDiffHydrationCrash,
    ] {
        assert!(
            issues.iter().any(|issue| issue.code == expected),
            "expected {} from the rendered SPA diff",
            expected.as_str()
        );
    }
}

#[tokio::test]
async fn chrome_renderer_executes_fixture_javascript_and_collects_runtime_errors() {
    let Some(chrome) = test_chrome() else {
        return;
    };
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/products/widget"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(RAW_SPA_HTML, "text/html"))
        .mount(&server)
        .await;

    let url = Url::parse(&format!("{}/products/widget", server.uri())).unwrap();
    let renderer = Renderer::launch(RenderOptions {
        chrome_path: Some(chrome),
        no_sandbox: true,
        page_timeout: Duration::from_secs(25),
        user_agent: Some("BlackSparrow render-js test".to_string()),
        ..RenderOptions::default()
    })
    .await
    .expect("an installed Chrome should launch for the render-js test");
    let rendered = renderer
        .render(&url)
        .await
        .expect("fixture JavaScript should render in Chrome");
    renderer.close().await;

    let parsed =
        parse_html(&rendered.html, &rendered.final_url).expect("rendered DOM should parse");
    assert!(parsed.robots_flags.contains(RobotsFlags::NOINDEX));
    assert!(parsed
        .links
        .iter()
        .any(|link| link.anchor_text == "Browse products"));
    assert!(rendered
        .final_url
        .ends_with("/products/widget/client-route"));
    assert!(
        rendered
            .runtime_errors
            .iter()
            .any(|error| error.contains("Hydration failed")),
        "uncaught fixture hydration error should be captured: {:?}",
        rendered.runtime_errors
    );
}

#[tokio::test]
async fn crawl_uses_rendered_dom_for_reports_frontier_and_js_diff_issues() {
    let Some(chrome) = test_chrome() else {
        return;
    };
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/products/widget"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(RAW_SPA_HTML, "text/html"))
        .mount(&server)
        .await;

    let url = format!("{}/products/widget", server.uri());
    let mut config = CrawlConfig::new(&url).expect("mock URL should be a valid crawl target");
    config.max_pages = 1;
    config.respect_robots = false;
    config.no_aimd = true;
    config.render_js = true;
    config.chrome_path = Some(chrome);
    config.chrome_no_sandbox = true;

    let result = run_crawl(&config, None)
        .await
        .expect("render-js crawl should complete");
    let page = result
        .pages
        .first()
        .expect("crawl should report the seed page");

    assert_eq!(
        page.canonical_url.as_deref(),
        Some("https://example.test/products/widget/client-route")
    );
    assert!(page
        .links
        .iter()
        .any(|link| link.anchor_text == "Browse products"));
    assert!(page
        .issues
        .iter()
        .any(|issue| issue.code == RuleId::ErrJsDiffHydrationCrash));
    assert!(result
        .issues
        .iter()
        .any(|issue| issue.code == RuleId::ErrJsDiffCanonicalAltered));
}
