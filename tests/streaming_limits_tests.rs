//! # Streaming Limits & Decompression Defense Tests (P0-5)
//!
//! Automated tests validating:
//! 1. Responses exceeding 15 MB are truncated at 15 MB and flagged as truncated.
//! 2. Normal responses under 15 MB are completely received with is_truncated == false.
//! 3. Sitemap gzip decompression bombs exceeding 50 MB are safely rejected.
//! 4. Rule evaluation emits ErrPerfExcessiveHtmlPayload warning about Google's 15 MB limit.

use std::io::Write;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use blacksparrow::crawler::client::{FetchOptions, HttpClient, DEFAULT_MAX_RESPONSE_BYTES};
use blacksparrow::crawler::sitemap::parse_sitemap;
use blacksparrow::parser::ParsedPage;
use blacksparrow::rules::page::status::check_status;
use blacksparrow::SeoResult;
use flate2::write::GzEncoder;
use flate2::Compression;

#[tokio::test]
async fn test_response_exceeding_15mb_is_truncated() -> SeoResult<()> {
    let mock_server = MockServer::start().await;

    // Create a 16 MiB payload (16 * 1024 * 1024 bytes)
    let payload_size = 16 * 1024 * 1024;
    let payload = vec![b'x'; payload_size];

    Mock::given(method("GET"))
        .and(path("/large-page"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(payload)
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .mount(&mock_server)
        .await;

    let options = FetchOptions {
        allow_all_private_ips: true,
        ..Default::default()
    };
    let client = HttpClient::new(options)?;

    let url = format!("{}/large-page", mock_server.uri());
    let result = client.fetch(&url).await?;

    assert!(
        result.is_truncated,
        "Fetch result should be marked as truncated when exceeding 15 MB"
    );
    assert_eq!(
        result.size_bytes as usize, DEFAULT_MAX_RESPONSE_BYTES,
        "Size bytes should be capped at exactly 15 MB"
    );
    assert_eq!(
        result.body.len(),
        DEFAULT_MAX_RESPONSE_BYTES,
        "Decoded body length should equal 15 MB limit"
    );

    Ok(())
}

#[tokio::test]
async fn test_response_under_15mb_not_truncated() -> SeoResult<()> {
    let mock_server = MockServer::start().await;

    let small_html =
        "<html><head><title>Under Limit</title></head><body>Normal Content</body></html>";

    Mock::given(method("GET"))
        .and(path("/normal-page"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(small_html)
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .mount(&mock_server)
        .await;

    let options = FetchOptions {
        allow_all_private_ips: true,
        ..Default::default()
    };
    let client = HttpClient::new(options)?;

    let url = format!("{}/normal-page", mock_server.uri());
    let result = client.fetch(&url).await?;

    assert!(
        !result.is_truncated,
        "Fetch result should not be marked as truncated for normal pages"
    );
    assert_eq!(result.body, small_html);
    assert_eq!(result.size_bytes as usize, small_html.len());

    Ok(())
}

#[test]
fn test_sitemap_gzip_decompression_bomb_rejected() {
    // Generate a 55 MiB stream of zeros that compresses to a tiny gzip payload (< 60 KB)
    let uncompressed_size = 55 * 1024 * 1024; // 55 MiB (> 50 MiB limit)
    let zeros = vec![b' '; uncompressed_size];

    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(&zeros)
        .expect("Gzip compression must succeed");
    let compressed_bomb = encoder.finish().expect("Gzip finish must succeed");

    assert!(
        compressed_bomb.len() < 500_000,
        "Bomb must be small when compressed: {}",
        compressed_bomb.len()
    );

    // parse_sitemap must reject with an error instead of decompressing > 50 MiB
    let result = parse_sitemap(&compressed_bomb);
    assert!(
        result.is_err(),
        "parse_sitemap should reject decompression bomb"
    );

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("50 MB") || err_msg.contains("decompression limit"),
        "Error message should mention the 50 MB limit, got: {err_msg}"
    );
}

#[tokio::test]
async fn test_rule_excessive_html_payload_emitted_on_truncation() -> SeoResult<()> {
    let mock_server = MockServer::start().await;

    // Simulate truncated 15MB fetch result
    let payload = vec![b'a'; DEFAULT_MAX_RESPONSE_BYTES];
    Mock::given(method("GET"))
        .and(path("/truncated-page"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(payload)
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .mount(&mock_server)
        .await;

    let options = FetchOptions {
        allow_all_private_ips: true,
        ..Default::default()
    };
    let client = HttpClient::new(options)?;

    let url = format!("{}/truncated-page", mock_server.uri());
    let fetch_result = client.fetch(&url).await?;

    let page = ParsedPage::default();
    let mut issues = Vec::new();
    check_status(&page, &fetch_result, &mut issues);

    let has_excessive_issue = issues.iter().any(|i| {
        i.code == blacksparrow::core::models::RuleId::ErrPerfExcessiveHtmlPayload
            && (i.message.contains("15 MB")
                || i.message.contains("15.0 MB")
                || i.message.contains("Google"))
    });

    assert!(
        has_excessive_issue,
        "Should emit ErrPerfExcessiveHtmlPayload with 15MB Googlebot guidance, found issues: {:?}",
        issues
    );

    Ok(())
}
