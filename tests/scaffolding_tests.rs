use seo_lens::{SeoError, SeoResult};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn test_fixtures_exist_and_readable() {
    let sample_html = include_str!("fixtures/sample_page.html");
    assert!(sample_html.contains("<title>SEO Lens Fixture Page</title>"));
    assert!(sample_html.contains("rel=\"canonical\""));

    let robots_txt = include_str!("fixtures/robots.txt");
    assert!(robots_txt.contains("User-agent: *"));
    assert!(robots_txt.contains("User-agent: GPTBot"));
    assert!(robots_txt.contains("User-agent: PerplexityBot"));

    let sitemap_xml = include_str!("fixtures/sitemap.xml");
    assert!(sitemap_xml.contains("<loc>https://example.com/</loc>"));

    let good_html = include_str!("fixtures/good_page.html");
    assert!(good_html.contains("Technical SEO Best Practices"));

    let broken_html = include_str!("fixtures/broken_seo_page.html");
    assert!(broken_html.contains("Broken SEO Test Page"));

    let product_html = include_str!("fixtures/ecommerce_product_page.html");
    assert!(product_html.contains("Rust Performance Engineering Handbook"));

    let intl_html = include_str!("fixtures/international_hreflang_page.html");
    assert!(intl_html.contains("hreflang=\"fr-FR\""));
}

#[test]
fn test_all_fixtures_parse_cleanly() {
    use seo_lens::parser::parse_html;

    let fixtures = [
        include_str!("fixtures/sample_page.html"),
        include_str!("fixtures/malformed_page.html"),
        include_str!("fixtures/good_page.html"),
        include_str!("fixtures/broken_seo_page.html"),
        include_str!("fixtures/ecommerce_product_page.html"),
        include_str!("fixtures/international_hreflang_page.html"),
    ];

    for fixture in fixtures {
        let result = parse_html(fixture, "https://example.com/test");
        assert!(
            result.is_ok(),
            "Fixture failed to stream-parse: {:?}",
            result.err()
        );
    }
}

#[tokio::test]
async fn test_wiremock_harness_integration() -> SeoResult<()> {
    let mock_server = MockServer::start().await;

    let fixture_html = include_str!("fixtures/sample_page.html");

    Mock::given(method("GET"))
        .and(path("/test-page"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(fixture_html)
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .mount(&mock_server)
        .await;

    // Use reqwest to verify mock server connection
    let response = reqwest::get(format!("{}/test-page", mock_server.uri()))
        .await
        .map_err(|e| SeoError::Network(e.to_string()))?;

    assert_eq!(response.status(), 200);
    let body = response
        .text()
        .await
        .map_err(|e| SeoError::Network(e.to_string()))?;
    assert!(body.contains("SEO Lens Fixture Page"));

    Ok(())
}
