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
    let response = reqwest::get(format!("{}/test-page", &mock_server.uri()))
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
