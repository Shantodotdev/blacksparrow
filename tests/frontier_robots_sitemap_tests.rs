//! # Frontier, Robots & Sitemap Integration Tests
//!
//! Comprehensive test suite validating:
//! - SwissTable hash-based deduplication and queue boundary enforcement (BFS vs DFS).
//! - Strict RFC 9309 robots.txt compliance (longest match, precedence, wildcards, crawl-delay).
//! - Zero-copy streaming XML sitemap parser with alternates and gzip decompression.

use blacksparrow::crawler::frontier::Frontier;
use blacksparrow::crawler::robots::RobotsTxt;
use blacksparrow::crawler::sitemap::{parse_sitemap, SitemapDocument};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;
use std::time::Duration;

// =========================================================================
// 1. Crawl Frontier & Boundary Tests
// =========================================================================

#[test]
fn test_frontier_fifo_tie_breaking_order() {
    let mut frontier = Frontier::new(100, 5);

    assert!(frontier.push("https://example.com/", 0, None).unwrap());
    assert!(frontier
        .push("https://example.com/a", 1, Some("https://example.com/"))
        .unwrap());
    assert!(frontier
        .push("https://example.com/b", 1, Some("https://example.com/"))
        .unwrap());

    // Priority queue pops root first, then tie-breaks identical score pages in FIFO order
    let first = frontier.pop().unwrap();
    assert_eq!(first.url, "https://example.com/");
    assert_eq!(first.depth, 0);

    let second = frontier.pop().unwrap();
    assert_eq!(second.url, "https://example.com/a");
    assert_eq!(second.depth, 1);

    let third = frontier.pop().unwrap();
    assert_eq!(third.url, "https://example.com/b");
    assert_eq!(third.depth, 1);

    assert!(frontier.is_empty());
}

#[test]
fn test_frontier_swisstable_deduplication() {
    let mut frontier = Frontier::new(100, 5);

    // Initial push
    assert!(frontier
        .push("https://example.com/blog?b=2&a=1", 0, None)
        .unwrap());

    // Duplicate variants that normalize to the exact same URL:
    // - hostname case insensitivity
    // - marketing tracking parameters stripped
    // - query param lexicographical sorting
    // - default port 443 stripping
    // - fragment stripping
    assert!(!frontier
        .push("https://EXAMPLE.COM/blog?a=1&b=2", 1, None)
        .unwrap());
    assert!(!frontier
        .push(
            "https://example.com/blog?b=2&a=1&utm_source=facebook&utm_medium=cpc",
            1,
            None
        )
        .unwrap());
    assert!(!frontier
        .push("https://example.com:443/blog?a=1&b=2#section", 1, None)
        .unwrap());

    assert_eq!(frontier.len(), 1);
    assert_eq!(frontier.visited_count(), 1);
}

#[test]
fn test_frontier_max_depth_enforcement() {
    let mut frontier = Frontier::new(100, 2);

    assert!(frontier
        .push("https://example.com/depth-0", 0, None)
        .unwrap());
    assert!(frontier
        .push("https://example.com/depth-1", 1, None)
        .unwrap());
    assert!(frontier
        .push("https://example.com/depth-2", 2, None)
        .unwrap());

    // Exceeds max_depth of 2 -> must be rejected (returns false and not enqueued)
    assert!(!frontier
        .push("https://example.com/depth-3", 3, None)
        .unwrap());
    assert!(!frontier
        .push("https://example.com/depth-4", 4, None)
        .unwrap());

    assert_eq!(frontier.len(), 3);
}

#[test]
fn test_frontier_max_pages_ceiling() {
    let mut frontier = Frontier::new(3, 10);

    assert!(frontier
        .push("https://example.com/page-1", 0, None)
        .unwrap());
    assert!(frontier
        .push("https://example.com/page-2", 1, None)
        .unwrap());
    assert!(frontier
        .push("https://example.com/page-3", 1, None)
        .unwrap());

    // 4th page exceeds max_pages ceiling (3) -> rejected
    assert!(!frontier
        .push("https://example.com/page-4", 1, None)
        .unwrap());
    assert_eq!(frontier.len(), 3);
}

// =========================================================================
// 2. RFC 9309 Robots Exclusion Protocol Tests
// =========================================================================

#[test]
fn test_robots_longest_match_precedence() {
    let robots_txt = r#"
User-agent: *
Allow: /products/
Disallow: /products/archived/
Allow: /products/archived/public/
"#;

    let robots = RobotsTxt::parse(robots_txt);

    // /products/catalog -> matches /products/ (10 chars) -> ALLOWED
    assert!(robots.is_allowed("SEOLens", "/products/catalog"));

    // /products/archived/item-1 -> matches /products/archived/ (19 chars) vs /products/ (10 chars) -> DISALLOWED
    assert!(!robots.is_allowed("SEOLens", "/products/archived/item-1"));

    // /products/archived/public/item-2 -> matches /products/archived/public/ (26 chars) -> ALLOWED
    assert!(robots.is_allowed("SEOLens", "/products/archived/public/item-2"));
}

#[test]
fn test_robots_equal_length_allow_precedence() {
    // Per RFC 9309 Section 2.2.2:
    // If Allow and Disallow directives match with equal character length, Allow takes precedence.
    let robots_txt = r#"
User-agent: *
Disallow: /blog
Allow: /blog
"#;

    let robots = RobotsTxt::parse(robots_txt);
    assert!(robots.is_allowed("SEOLens", "/blog"));
    assert!(robots.is_allowed("SEOLens", "/blog/post-1"));
}

#[test]
fn test_robots_user_agent_priority() {
    let robots_txt = r#"
User-agent: *
Disallow: /

User-agent: Googlebot
Disallow: /private/
Allow: /

User-agent: SEOLens
Allow: /
Disallow: /admin/
"#;

    let robots = RobotsTxt::parse(robots_txt);

    // SEOLens matches its specific group:
    assert!(robots.is_allowed("SEOLens", "/public"));
    assert!(!robots.is_allowed("SEOLens", "/admin/settings"));

    // Googlebot matches Googlebot group:
    assert!(robots.is_allowed("Googlebot", "/public"));
    assert!(!robots.is_allowed("Googlebot", "/private/secret"));

    // Unmatched bot falls back to wildcard `*` (Disallow: /):
    assert!(!robots.is_allowed("Bingbot", "/public"));
}

#[test]
fn test_robots_wildcards_and_anchors() {
    let robots_txt = r#"
User-agent: *
Disallow: /*.php$
Disallow: /temp*preview/
"#;

    let robots = RobotsTxt::parse(robots_txt);

    // Matches /*.php$
    assert!(!robots.is_allowed("SEOLens", "/index.php"));
    assert!(!robots.is_allowed("SEOLens", "/path/sub/script.php"));
    // Does NOT end with .php
    assert!(robots.is_allowed("SEOLens", "/index.php?query=1"));
    assert!(robots.is_allowed("SEOLens", "/index.phps"));

    // Matches /temp*preview/
    assert!(!robots.is_allowed("SEOLens", "/temp-test-preview/item"));
    assert!(!robots.is_allowed("SEOLens", "/temppreview/"));
}

#[test]
fn test_robots_crawl_delay_and_sitemaps() {
    let robots_txt = r#"
User-agent: SEOLens
Crawl-delay: 2.5

User-agent: *
Crawl-delay: 10
Sitemap: https://example.com/sitemap.xml
Sitemap: https://example.com/sitemap-news.xml
"#;

    let robots = RobotsTxt::parse(robots_txt);

    // Crawl delay for SEOLens
    assert_eq!(
        robots.crawl_delay("SEOLens"),
        Some(Duration::from_millis(2500))
    );

    // Crawl delay for other bots falls back to *
    assert_eq!(
        robots.crawl_delay("OtherBot"),
        Some(Duration::from_millis(10000))
    );

    // Sitemaps extracted globally
    assert_eq!(
        robots.sitemaps(),
        &[
            "https://example.com/sitemap.xml",
            "https://example.com/sitemap-news.xml"
        ]
    );
}

#[test]
fn test_robots_empty_disallow_permits_all() {
    let robots_txt = r#"
User-agent: *
Disallow:
"#;

    let robots = RobotsTxt::parse(robots_txt);
    assert!(robots.is_allowed("SEOLens", "/any/path"));
}

// =========================================================================
// 3. Streaming XML Sitemap Tests
// =========================================================================

#[test]
fn test_sitemap_parse_urlset_with_attributes_and_alternates() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"
        xmlns:xhtml="http://www.w3.org/1999/xhtml">
  <url>
    <loc>https://example.com/page-1</loc>
    <lastmod>2026-09-01T12:00:00Z</lastmod>
    <changefreq>daily</changefreq>
    <priority>0.8</priority>
    <xhtml:link rel="alternate" hreflang="es" href="https://example.com/es/page-1" />
    <xhtml:link rel="alternate" hreflang="fr" href="https://example.com/fr/page-1" />
  </url>
  <url>
    <loc>https://example.com/page-2</loc>
  </url>
</urlset>"#;

    let doc = parse_sitemap(xml.as_bytes()).expect("Failed to parse standard sitemap urlset");

    match doc {
        SitemapDocument::UrlSet(entries) => {
            assert_eq!(entries.len(), 2);

            let first = &entries[0];
            assert_eq!(first.loc, "https://example.com/page-1");
            assert_eq!(first.lastmod.as_deref(), Some("2026-09-01T12:00:00Z"));
            assert_eq!(first.changefreq.as_deref(), Some("daily"));
            assert_eq!(first.priority, Some(0.8));
            assert_eq!(first.alternates.len(), 2);
            assert_eq!(first.alternates[0].hreflang, "es");
            assert_eq!(first.alternates[0].href, "https://example.com/es/page-1");

            let second = &entries[1];
            assert_eq!(second.loc, "https://example.com/page-2");
            assert!(second.lastmod.is_none());
            assert!(second.changefreq.is_none());
            assert!(second.priority.is_none());
            assert!(second.alternates.is_empty());
        }
        SitemapDocument::Index(_) => panic!("Expected UrlSet, got Index"),
    }
}

#[test]
fn test_sitemap_parse_sitemapindex() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sitemap>
    <loc>https://example.com/sitemap-posts.xml</loc>
    <lastmod>2026-09-04T00:00:00Z</lastmod>
  </sitemap>
  <sitemap>
    <loc>https://example.com/sitemap-products.xml.gz</loc>
  </sitemap>
</sitemapindex>"#;

    let doc = parse_sitemap(xml.as_bytes()).expect("Failed to parse sitemap index");

    match doc {
        SitemapDocument::Index(sub_sitemaps) => {
            assert_eq!(sub_sitemaps.len(), 2);
            assert_eq!(sub_sitemaps[0].loc, "https://example.com/sitemap-posts.xml");
            assert_eq!(
                sub_sitemaps[0].lastmod.as_deref(),
                Some("2026-09-04T00:00:00Z")
            );
            assert_eq!(
                sub_sitemaps[1].loc,
                "https://example.com/sitemap-products.xml.gz"
            );
        }
        SitemapDocument::UrlSet(_) => panic!("Expected Index, got UrlSet"),
    }
}

#[test]
fn test_sitemap_gzip_decompression() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>https://example.com/compressed-page</loc>
    <priority>1.0</priority>
  </url>
</urlset>"#;

    // Compress with GzEncoder
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(xml.as_bytes())
        .expect("Failed to compress XML");
    let compressed_bytes = encoder.finish().expect("Failed to finish gzip compression");

    // parse_sitemap should automatically decompress gzip magic bytes
    let doc = parse_sitemap(&compressed_bytes).expect("Failed to parse compressed sitemap");

    match doc {
        SitemapDocument::UrlSet(entries) => {
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].loc, "https://example.com/compressed-page");
            assert_eq!(entries[0].priority, Some(1.0));
        }
        SitemapDocument::Index(_) => panic!("Expected UrlSet, got Index"),
    }
}
