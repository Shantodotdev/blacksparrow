//! # Streaming HTML Parser Integration Tests
//!
//! Strict TDD test suite validating metadata extraction, link discovery,
//! image resource scanning, heading hierarchy, JSON-LD schemas,
//! and content text extraction using lol_html streaming engine.

use seo_lens::core::models::RobotsFlags;
use seo_lens::parser::parse_html;

#[test]
fn test_parse_sample_fixture_metadata() {
    let html = include_str!("fixtures/sample_page.html");
    let base_url = "https://example.com/";

    let parsed = parse_html(html, base_url).expect("Failed to parse sample HTML fixture");

    assert_eq!(parsed.title.as_deref(), Some("SEO Lens Fixture Page"));
    assert_eq!(
        parsed.meta_description.as_deref(),
        Some("A sample fixture page used for technical SEO audit verification.")
    );
    assert_eq!(
        parsed.canonical_url.as_deref(),
        Some("https://example.com/fixture")
    );
    assert_eq!(parsed.html_lang.as_deref(), Some("en"));
    assert_eq!(parsed.charset.as_deref(), Some("utf-8"));
    assert_eq!(
        parsed.viewport.as_deref(),
        Some("width=device-width, initial-scale=1")
    );

    // Robots directives
    assert!(parsed.robots_flags.contains(RobotsFlags::NOINDEX));
    assert!(parsed.robots_flags.contains(RobotsFlags::NOFOLLOW));
    assert!(!parsed.robots_flags.contains(RobotsFlags::NOARCHIVE));

    // OpenGraph & Twitter
    assert_eq!(
        parsed.get_open_graph("og:title").map(|s| s.as_str()),
        Some("SEO Lens Sample Fixture")
    );
    assert_eq!(
        parsed.get_twitter_card("twitter:card").map(|s| s.as_str()),
        Some("summary_large_image")
    );
}

#[test]
fn test_parse_headings_hierarchy() {
    let html = include_str!("fixtures/malformed_page.html");
    let base_url = "https://example.com/malformed";

    let parsed = parse_html(html, base_url).expect("Failed to parse malformed HTML fixture");

    assert_eq!(parsed.h1_primary.as_deref(), Some("Primary H1 Headline"));
    assert_eq!(parsed.h1_count, 2);
    assert_eq!(parsed.h2_headings, vec!["Section Subheading"]);
    assert_eq!(parsed.h3_headings, vec!["Subsection Subheading"]);
}

#[test]
fn test_parse_links_discovery() {
    let html = include_str!("fixtures/malformed_page.html");
    let base_url = "https://example.com/malformed";

    let parsed = parse_html(html, base_url).expect("Failed to parse HTML for links");

    // Check internal relative link
    let internal_link = parsed
        .links
        .iter()
        .find(|l| l.target_url == "https://example.com/relative/page");
    assert!(
        internal_link.is_some(),
        "Should discover internal relative link"
    );
    let internal_link = internal_link.unwrap();
    assert!(internal_link.is_internal);
    assert!(!internal_link.is_nofollow);
    assert_eq!(internal_link.anchor_text, "Internal Relative Link");

    // Check outbound nofollow link
    let outbound_link = parsed
        .links
        .iter()
        .find(|l| l.target_url == "https://external.com/outbound");
    assert!(outbound_link.is_some(), "Should discover outbound link");
    let outbound_link = outbound_link.unwrap();
    assert!(!outbound_link.is_internal);
    assert!(outbound_link.is_nofollow);

    // Check image link
    let image_link = parsed
        .links
        .iter()
        .find(|l| l.target_url == "https://example.com/image-link");
    assert!(image_link.is_some(), "Should discover image link");
    assert!(image_link.unwrap().is_image_link);
}

#[test]
fn test_parse_images_resource_extraction() {
    let html = include_str!("fixtures/malformed_page.html");
    let base_url = "https://example.com/malformed";

    let parsed = parse_html(html, base_url).expect("Failed to parse HTML for images");

    let banner = parsed
        .images
        .iter()
        .find(|img| img.src_url == "https://example.com/banner.png");
    assert!(banner.is_some(), "Should find banner image");
    let banner = banner.unwrap();
    assert_eq!(banner.alt_text.as_deref(), Some("Banner Image"));
    assert_eq!(banner.width, Some(800));
    assert_eq!(banner.height, Some(400));
    assert!(banner.has_dimensions);

    let missing_alt = parsed
        .images
        .iter()
        .find(|img| img.src_url == "https://example.com/missing-alt.png");
    assert!(missing_alt.is_some(), "Should find missing alt image");
    let missing_alt = missing_alt.unwrap();
    assert_eq!(missing_alt.alt_text, None);
    assert!(!missing_alt.has_dimensions);
}

#[test]
fn test_parse_json_ld_schema() {
    let html = include_str!("fixtures/malformed_page.html");
    let base_url = "https://example.com/malformed";

    let parsed = parse_html(html, base_url).expect("Failed to parse HTML for JSON-LD");

    assert_eq!(parsed.schemas.len(), 1);
    let schema = &parsed.schemas[0];
    assert_eq!(schema.schema_type.as_str(), "Article");
    assert!(schema.is_valid_json);
    assert!(schema.raw_json.contains("Testing Malformed Parsing"));
}

#[test]
fn test_editorial_content_and_word_count() {
    let html = include_str!("fixtures/malformed_page.html");
    let base_url = "https://example.com/malformed";

    let parsed = parse_html(html, base_url).expect("Failed to parse content");

    // Word count should only include editorial body text, ignoring nav, scripts, footer
    assert!(parsed.word_count > 10);
    assert_ne!(parsed.content_hash, 0);
    assert_ne!(parsed.simhash, 0);

    // Verify SimHash near-duplicate behavior
    let html_variation = html.replace(
        "explaining more details about the topic",
        "explaining a few details about the topic",
    );
    let parsed_variation = parse_html(&html_variation, base_url).unwrap();

    // Hamming distance between near-duplicates should be small (<= 10 bits difference out of 64, corresponding to >85% similarity)
    let hamming_distance = (parsed.simhash ^ parsed_variation.simhash).count_ones();
    assert!(
        hamming_distance <= 10,
        "Hamming distance {hamming_distance} should be <= 10 for near duplicates"
    );
}

#[test]
fn test_deeply_nested_html_and_json_serialization() {
    let mut html =
        String::from("<!DOCTYPE html><html><head><title>Nested Page</title></head><body>");
    for _ in 0..50 {
        html.push_str("<div><section><article>");
    }
    html.push_str("<h1>Deep Heading</h1><p>Deep editorial content inside nested tree.</p>");
    html.push_str("<a href=\"/nested-link\">Nested Link</a>");
    for _ in 0..50 {
        html.push_str("</article></section></div>");
    }
    html.push_str("</body></html>");

    let parsed =
        parse_html(&html, "https://example.com/deep").expect("Failed to parse deeply nested HTML");

    assert_eq!(parsed.h1_primary.as_deref(), Some("Deep Heading"));
    assert_eq!(parsed.links.len(), 1);
    assert_eq!(
        parsed.links[0].target_url,
        "https://example.com/nested-link"
    );

    // Verify JSON serialization of ParsedPage
    let json =
        serde_json::to_string_pretty(&parsed).expect("Failed to serialize ParsedPage to JSON");
    assert!(json.contains("\"title\": \"Nested Page\""));
    assert!(json.contains("\"h1_primary\": \"Deep Heading\""));
}

#[test]
fn test_manual_verification_gate_json_export() {
    let html = include_str!("fixtures/malformed_page.html");
    let parsed = parse_html(html, "https://example.com/fixture").expect("Failed to parse fixture");
    let json = serde_json::to_string_pretty(&parsed).expect("Failed to serialize ParsedPage");
    println!(
        "=== MANUAL VERIFICATION GATE: EXTRACTED METADATA JSON ===\n{}",
        json
    );
    assert!(!json.is_empty());
}
