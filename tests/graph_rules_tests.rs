//! # Site Graph Topology & Multi-Page Graph Rules Tests
//!
//! Integration test suite validating `SiteGraph` construction, power-iteration PageRank,
//! cycle and chain detection, content deduplication, and multi-page technical SEO rules.

use compact_str::CompactString;
use hashbrown::HashMap;
use seo_lens::core::models::{
    DiscoveredLink, HreflangTag, IssueCategory, IssueFinding, PageReport, RobotsFlags, RuleId,
    Severity,
};
use seo_lens::crawler::sitemap::{parse_sitemap, SitemapDocument};
use seo_lens::graph::{compute_pagerank, LinkEdgeType, SiteGraph};
use seo_lens::report::score::calculate_health_score;
use seo_lens::rules::graph::evaluate_graph_rules;

/// Helper to create a minimal dummy PageReport for graph tests.
#[allow(clippy::too_many_arguments)]
fn mock_page(
    url: &str,
    status_code: u16,
    crawl_depth: u16,
    title: Option<&str>,
    meta_desc: Option<&str>,
    canonical: Option<&str>,
    content_hash: u64,
    simhash: u64,
    links: Vec<DiscoveredLink>,
    hreflangs: Vec<HreflangTag>,
) -> PageReport {
    PageReport {
        id: None,
        crawl_id: CompactString::new("test-session"),
        url: url.to_string(),
        url_hash: seo_lens::core::url::url_hash(url),
        final_url: None,
        status_code,
        content_type: CompactString::new("text/html"),
        size_bytes: 1024,
        ttfb_ms: 120,
        crawl_depth,
        title: title.map(String::from),
        title_length: title.map(|t| t.len() as u16).unwrap_or(0),
        meta_description: meta_desc.map(String::from),
        meta_desc_length: meta_desc.map(|d| d.len() as u16).unwrap_or(0),
        canonical_url: canonical.map(String::from),
        html_lang: Some(CompactString::new("en")),
        charset: Some(CompactString::new("utf-8")),
        viewport: Some(CompactString::new("width=device-width, initial-scale=1.0")),
        robots_flags: RobotsFlags::NONE,
        is_sitemap_url: false,
        is_internal: true,
        h1_primary: Some(String::from("Main Headline")),
        h1_count: 1,
        h2_headings: vec![],
        h3_headings: vec![],
        word_count: 500,
        content_hash,
        simhash,
        is_soft_404: false,
        has_lorem_ipsum: false,
        is_https: true,
        has_hsts: true,
        has_csp: true,
        has_x_frame: true,
        has_x_content_type: true,
        mixed_content_count: 0,
        links,
        images: vec![],
        schemas: vec![],
        hreflangs,
        issues: vec![],
        page_intent: Default::default(),
    }
}

fn mock_link(source: &str, target: &str, is_nofollow: bool) -> DiscoveredLink {
    DiscoveredLink {
        source_url: source.to_string(),
        target_url: target.to_string(),
        target_url_hash: seo_lens::core::url::url_hash(target),
        anchor_text: "Test Link".to_string(),
        is_internal: true,
        is_nofollow,
        is_image_link: false,
        status_code: Some(200),
        is_target_blank: false,
        has_opener_or_referrer: true,
    }
}

#[test]
fn test_site_graph_construction_and_degrees() {
    let mut graph = SiteGraph::new();
    let idx_home = graph.add_node("https://example.com/", 200, 0, true);
    let _idx_about = graph.add_node("https://example.com/about", 200, 1, true);
    let _idx_contact = graph.add_node("https://example.com/contact", 200, 1, false);

    assert_eq!(graph.node_count(), 3);
    assert_eq!(
        idx_home,
        graph.get_node_index("https://example.com/").unwrap()
    );

    // Add edges: home -> about, home -> contact, about -> home
    graph.add_edge(
        "https://example.com/",
        "https://example.com/about",
        LinkEdgeType::InternalHyperlink,
        false,
        "About Us",
    );
    graph.add_edge(
        "https://example.com/",
        "https://example.com/contact",
        LinkEdgeType::InternalHyperlink,
        false,
        "Contact",
    );
    graph.add_edge(
        "https://example.com/about",
        "https://example.com/",
        LinkEdgeType::InternalHyperlink,
        false,
        "Home",
    );

    assert_eq!(graph.out_degree("https://example.com/"), 2);
    assert_eq!(graph.in_degree("https://example.com/"), 1);
    assert_eq!(graph.in_degree("https://example.com/about"), 1);
    assert_eq!(graph.out_degree("https://example.com/contact"), 0);
}

#[test]
fn test_pagerank_uniform_ring() {
    // In a symmetric ring graph (A -> B -> C -> A), all nodes must receive equal PageRank equity.
    let mut graph = SiteGraph::new();
    graph.add_node("https://example.com/a", 200, 0, false);
    graph.add_node("https://example.com/b", 200, 1, false);
    graph.add_node("https://example.com/c", 200, 2, false);

    graph.add_edge(
        "https://example.com/a",
        "https://example.com/b",
        LinkEdgeType::InternalHyperlink,
        false,
        "",
    );
    graph.add_edge(
        "https://example.com/b",
        "https://example.com/c",
        LinkEdgeType::InternalHyperlink,
        false,
        "",
    );
    graph.add_edge(
        "https://example.com/c",
        "https://example.com/a",
        LinkEdgeType::InternalHyperlink,
        false,
        "",
    );

    let pr = compute_pagerank(&graph, 0.85, 100, 1e-7);
    assert_eq!(pr.len(), 3);

    let pr_a = pr
        .get(&seo_lens::core::url::url_hash("https://example.com/a"))
        .copied()
        .unwrap_or(0.0);
    let pr_b = pr
        .get(&seo_lens::core::url::url_hash("https://example.com/b"))
        .copied()
        .unwrap_or(0.0);
    let pr_c = pr
        .get(&seo_lens::core::url::url_hash("https://example.com/c"))
        .copied()
        .unwrap_or(0.0);

    // Sum of PageRank should equal 1.0
    let total_sum = pr_a + pr_b + pr_c;
    assert!(
        (total_sum - 1.0).abs() < 1e-4,
        "PageRank must sum to 1.0, got {}",
        total_sum
    );

    // Symmetric ring should give equal scores (approx 1/3 each)
    assert!((pr_a - pr_b).abs() < 1e-4);
    assert!((pr_b - pr_c).abs() < 1e-4);
}

#[test]
fn test_pagerank_star_graph_centrality() {
    // Star graph: leaves (A, B, C) all point to Hub (H).
    // Hub (H) points back to A.
    // Hub should have significantly higher PageRank equity than leaves.
    let mut graph = SiteGraph::new();
    graph.add_node("https://example.com/hub", 200, 0, false);
    graph.add_node("https://example.com/leaf1", 200, 1, false);
    graph.add_node("https://example.com/leaf2", 200, 1, false);
    graph.add_node("https://example.com/leaf3", 200, 1, false);

    graph.add_edge(
        "https://example.com/leaf1",
        "https://example.com/hub",
        LinkEdgeType::InternalHyperlink,
        false,
        "",
    );
    graph.add_edge(
        "https://example.com/leaf2",
        "https://example.com/hub",
        LinkEdgeType::InternalHyperlink,
        false,
        "",
    );
    graph.add_edge(
        "https://example.com/leaf3",
        "https://example.com/hub",
        LinkEdgeType::InternalHyperlink,
        false,
        "",
    );
    graph.add_edge(
        "https://example.com/hub",
        "https://example.com/leaf1",
        LinkEdgeType::InternalHyperlink,
        false,
        "",
    );

    let pr = compute_pagerank(&graph, 0.85, 100, 1e-7);
    let pr_hub = pr
        .get(&seo_lens::core::url::url_hash("https://example.com/hub"))
        .copied()
        .unwrap_or(0.0);
    let pr_leaf2 = pr
        .get(&seo_lens::core::url::url_hash("https://example.com/leaf2"))
        .copied()
        .unwrap_or(0.0);

    assert!(
        pr_hub > pr_leaf2 * 2.0,
        "Hub PageRank ({}) must be much higher than leaf ({})",
        pr_hub,
        pr_leaf2
    );
}

#[test]
fn test_rule_orphan_page_detection() {
    // Page A links to Page B.
    // Sitemap declares Page A, Page B, and Page C.
    // Page C has 0 inlinks -> ALERT_GRAPH_ORPHAN_PAGE.
    let page_a = mock_page(
        "https://example.com/",
        200,
        0,
        Some("Home"),
        Some("Home Desc"),
        None,
        1001,
        2001,
        vec![mock_link(
            "https://example.com/",
            "https://example.com/b",
            false,
        )],
        vec![],
    );
    let page_b = mock_page(
        "https://example.com/b",
        200,
        1,
        Some("Page B"),
        Some("Desc B"),
        None,
        1002,
        2002,
        vec![mock_link(
            "https://example.com/b",
            "https://example.com/",
            false,
        )],
        vec![],
    );
    let page_c = mock_page(
        "https://example.com/c",
        200,
        1,
        Some("Page C (Orphan)"),
        Some("Desc C"),
        None,
        1003,
        2003,
        vec![], // No links to or from
        vec![],
    );

    let pages = vec![page_a, page_b, page_c];
    let sitemap_urls = vec![
        "https://example.com/".to_string(),
        "https://example.com/b".to_string(),
        "https://example.com/c".to_string(),
    ];

    let graph = SiteGraph::from_pages(&pages, &sitemap_urls);
    let issues = evaluate_graph_rules(&pages, &graph, &sitemap_urls, true, &HashMap::new());

    let orphan_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::AlertGraphOrphanPage)
        .collect();

    assert_eq!(
        orphan_issues.len(),
        1,
        "Exactly one orphan page should be flagged"
    );
    assert_eq!(orphan_issues[0].target_url, "https://example.com/c");
    assert_eq!(orphan_issues[0].severity, Severity::Alert);
    assert_eq!(orphan_issues[0].category, IssueCategory::SiteGraph);
}

#[test]
fn test_sitemap_xml_and_static_assets_never_flagged_as_orphans() {
    let page_home = mock_page(
        "https://example.com/",
        200,
        0,
        Some("Home"),
        Some("Home Desc"),
        None,
        1000,
        2000,
        vec![mock_link(
            "https://example.com/",
            "https://example.com/about",
            false,
        )],
        vec![],
    );
    let page_about = mock_page(
        "https://example.com/about",
        200,
        1,
        Some("About"),
        Some("About Desc"),
        None,
        1000,
        2000,
        vec![mock_link(
            "https://example.com/about",
            "https://example.com/",
            false,
        )],
        vec![],
    );

    let pages = vec![page_home, page_about];
    // Include XML sitemaps and static assets in sitemap_urls
    let sitemap_urls = vec![
        "https://example.com/".to_string(),
        "https://example.com/about".to_string(),
        "https://example.com/sitemap.xml".to_string(),
        "https://example.com/sitemap_index.xml".to_string(),
        "https://example.com/sitemap.xml.gz".to_string(),
        "https://example.com/robots.txt".to_string(),
        "https://example.com/image.png".to_string(),
    ];

    let graph = SiteGraph::from_pages(&pages, &sitemap_urls);
    let issues = evaluate_graph_rules(&pages, &graph, &sitemap_urls, true, &HashMap::new());

    let orphan_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::AlertGraphOrphanPage)
        .collect();

    assert!(
        orphan_issues.is_empty(),
        "Sitemap XML files, static assets, and homepage must never be flagged as orphan pages. Found: {:?}",
        orphan_issues
    );
}

#[test]
fn test_rule_circular_redirect_loop() {
    // Page A redirects to Page B. Page B redirects back to Page A.
    // Must trigger ERR_GRAPH_REDIRECT_LOOP.
    let mut page_a = mock_page(
        "https://example.com/a",
        301,
        0,
        None,
        None,
        None,
        0,
        0,
        vec![],
        vec![],
    );
    page_a.final_url = Some("https://example.com/b".to_string());

    let mut page_b = mock_page(
        "https://example.com/b",
        301,
        1,
        None,
        None,
        None,
        0,
        0,
        vec![],
        vec![],
    );
    page_b.final_url = Some("https://example.com/a".to_string());

    let pages = vec![page_a, page_b];
    let graph = SiteGraph::from_pages(&pages, &[]);
    let issues = evaluate_graph_rules(&pages, &graph, &[], true, &HashMap::new());

    let loop_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::ErrGraphRedirectLoop)
        .collect();

    assert!(
        !loop_issues.is_empty(),
        "Redirect loop must be flagged with ERR_GRAPH_REDIRECT_LOOP"
    );
    assert_eq!(loop_issues[0].severity, Severity::Critical);
}

#[test]
fn test_rule_multi_hop_redirect_chain() {
    // Page A (301) -> Page B (301) -> Page C (200)
    // Must trigger WARN_GRAPH_REDIRECT_CHAIN on Page A.
    let mut page_a = mock_page(
        "https://example.com/a",
        301,
        0,
        None,
        None,
        None,
        0,
        0,
        vec![],
        vec![],
    );
    page_a.final_url = Some("https://example.com/b".to_string());

    let mut page_b = mock_page(
        "https://example.com/b",
        301,
        1,
        None,
        None,
        None,
        0,
        0,
        vec![],
        vec![],
    );
    page_b.final_url = Some("https://example.com/c".to_string());

    let page_c = mock_page(
        "https://example.com/c",
        200,
        2,
        Some("Target"),
        None,
        None,
        1,
        1,
        vec![],
        vec![],
    );

    let pages = vec![page_a, page_b, page_c];
    let graph = SiteGraph::from_pages(&pages, &[]);
    let issues = evaluate_graph_rules(&pages, &graph, &[], true, &HashMap::new());

    let chain_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::WarnGraphRedirectChain)
        .collect();

    assert_eq!(
        chain_issues.len(),
        1,
        "Multi-hop redirect chain must be flagged"
    );
    assert_eq!(chain_issues[0].target_url, "https://example.com/a");
    assert_eq!(chain_issues[0].severity, Severity::Warning);
}

#[test]
fn test_rule_canonical_loop() {
    // Page A canonicalizes to Page B. Page B canonicalizes to Page A.
    // Must trigger ERR_GRAPH_CANONICAL_LOOP.
    let page_a = mock_page(
        "https://example.com/a",
        200,
        0,
        Some("A"),
        None,
        Some("https://example.com/b"),
        10,
        10,
        vec![],
        vec![],
    );
    let page_b = mock_page(
        "https://example.com/b",
        200,
        1,
        Some("B"),
        None,
        Some("https://example.com/a"),
        20,
        20,
        vec![],
        vec![],
    );

    let pages = vec![page_a, page_b];
    let graph = SiteGraph::from_pages(&pages, &[]);
    let issues = evaluate_graph_rules(&pages, &graph, &[], true, &HashMap::new());

    let canon_loop_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::ErrGraphCanonicalLoop)
        .collect();

    assert!(
        !canon_loop_issues.is_empty(),
        "Canonical loop must be flagged"
    );
    assert_eq!(canon_loop_issues[0].severity, Severity::Critical);
}

#[test]
fn test_rule_exact_and_near_duplicate_content() {
    // Page 1 & Page 2 have identical content_hash (Exact Duplicate)
    // Page 3 has small SimHash Hamming distance <= 3 from Page 1 (Near Duplicate)
    let page_1 = mock_page(
        "https://example.com/article",
        200,
        0,
        Some("Original Article"),
        Some("Desc 1"),
        None,
        99999,                 // Exact hash
        0x0000_0000_0000_000F, // SimHash
        vec![],
        vec![],
    );
    let page_2 = mock_page(
        "https://example.com/article-copy",
        200,
        1,
        Some("Copy Article"),
        Some("Desc 2"),
        None,
        99999, // Exact hash match with page 1!
        0x0000_0000_0000_000F,
        vec![],
        vec![],
    );
    let page_3 = mock_page(
        "https://example.com/article-near",
        200,
        1,
        Some("Near Article"),
        Some("Desc 3"),
        None,
        88888,                 // Different exact hash
        0x0000_0000_0000_000E, // Differs by 1 bit (Hamming distance 1 <= 3)
        vec![],
        vec![],
    );

    let pages = vec![page_1, page_2, page_3];
    let graph = SiteGraph::from_pages(&pages, &[]);
    let issues = evaluate_graph_rules(&pages, &graph, &[], true, &HashMap::new());

    let exact_dups: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::WarnGraphExactDuplicateContent)
        .collect();
    let near_dups: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::WarnGraphNearDuplicateContent)
        .collect();

    assert!(
        !exact_dups.is_empty(),
        "Exact duplicate content must be detected"
    );
    assert!(
        !near_dups.is_empty(),
        "Near-duplicate SimHash content must be detected"
    );
}

#[test]
fn test_rule_duplicate_title_and_meta_desc() {
    let page_1 = mock_page(
        "https://example.com/shoes-men",
        200,
        1,
        Some("Best Running Shoes 2026"),
        Some("Buy the top running shoes online with free shipping today."),
        None,
        1,
        10,
        vec![],
        vec![],
    );
    let page_2 = mock_page(
        "https://example.com/shoes-women",
        200,
        1,
        Some("Best Running Shoes 2026"), // Duplicate Title
        Some("Buy the top running shoes online with free shipping today."), // Duplicate Meta Desc
        None,
        2,
        20,
        vec![],
        vec![],
    );

    let pages = vec![page_1, page_2];
    let graph = SiteGraph::from_pages(&pages, &[]);
    let issues = evaluate_graph_rules(&pages, &graph, &[], true, &HashMap::new());

    let dup_titles: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::WarnGraphDuplicateTitles)
        .collect();
    let dup_descs: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::WarnGraphDuplicateMetaDescs)
        .collect();

    assert!(
        !dup_titles.is_empty(),
        "Duplicate titles across URLs must be flagged"
    );
    assert!(
        !dup_descs.is_empty(),
        "Duplicate meta descriptions across URLs must be flagged"
    );
}

#[test]
fn test_rule_dead_end_page_and_crawl_depth() {
    // Root links to Page Deep.
    // Page Deep has inlink, but 0 outlinks -> WARN_GRAPH_DEAD_END_PAGE
    // Page Deep has crawl_depth 5 (> 4) -> WARN_GRAPH_HIGH_CRAWL_DEPTH
    let root = mock_page(
        "https://example.com/",
        200,
        0,
        Some("Home"),
        None,
        None,
        1,
        1,
        vec![mock_link(
            "https://example.com/",
            "https://example.com/deep",
            false,
        )],
        vec![],
    );
    let deep = mock_page(
        "https://example.com/deep",
        200,
        5, // Depth 5
        Some("Deep Leaf"),
        None,
        None,
        2,
        2,
        vec![], // 0 outlinks
        vec![],
    );

    let pages = vec![root, deep];
    let graph = SiteGraph::from_pages(&pages, &[]);
    let issues = evaluate_graph_rules(&pages, &graph, &[], true, &HashMap::new());

    let dead_end: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::WarnGraphDeadEndPage)
        .collect();
    let high_depth: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::WarnGraphHighCrawlDepth)
        .collect();

    assert_eq!(dead_end.len(), 1, "Dead end page must be flagged");
    assert_eq!(dead_end[0].target_url, "https://example.com/deep");

    assert_eq!(high_depth.len(), 1, "High crawl depth (>4) must be flagged");
    assert_eq!(high_depth[0].target_url, "https://example.com/deep");
}

#[test]
fn test_rule_hreflang_non_reciprocal() {
    // English page points to Spanish alternate, but Spanish page fails to point back to English.
    let page_en = mock_page(
        "https://example.com/en",
        200,
        0,
        Some("English"),
        None,
        None,
        1,
        1,
        vec![],
        vec![HreflangTag {
            lang_code: CompactString::new("es"),
            target_url: "https://example.com/es".to_string(),
            is_reciprocal: false,
        }],
    );
    let page_es = mock_page(
        "https://example.com/es",
        200,
        1,
        Some("Spanish"),
        None,
        None,
        2,
        2,
        vec![],
        vec![], // Missing reciprocal back-link!
    );

    let pages = vec![page_en, page_es];
    let graph = SiteGraph::from_pages(&pages, &[]);
    let issues = evaluate_graph_rules(&pages, &graph, &[], true, &HashMap::new());

    let non_reciprocal: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::ErrHreflangNotReciprocal)
        .collect();

    assert_eq!(
        non_reciprocal.len(),
        1,
        "Non-reciprocal hreflang tag must be flagged"
    );
    assert_eq!(non_reciprocal[0].target_url, "https://example.com/en");
}

#[test]
fn test_zero_panics_on_empty_and_disconnected_graphs() {
    let empty_pages: Vec<PageReport> = vec![];
    let graph = SiteGraph::from_pages(&empty_pages, &[]);
    assert_eq!(graph.node_count(), 0);

    let pr = compute_pagerank(&graph, 0.85, 100, 1e-6);
    assert!(pr.is_empty());

    let issues = evaluate_graph_rules(&empty_pages, &graph, &[], true, &HashMap::new());
    assert!(issues.is_empty());
}

#[test]
fn test_partial_crawl_never_flags_uncrawled_sitemap_urls_as_orphans() {
    let fixture_xml = include_str!("fixtures/large_sitemap_partial_crawl.xml");
    let doc = parse_sitemap(fixture_xml.as_bytes()).expect("Failed to parse partial crawl fixture");
    let sitemap_urls: Vec<String> = match doc {
        SitemapDocument::UrlSet(entries) => {
            entries.into_iter().map(|e| e.loc.to_string()).collect()
        }
        _ => panic!("Expected standard UrlSet in fixture"),
    };

    assert_eq!(
        sitemap_urls.len(),
        24,
        "Fixture must contain 24 total sitemap URLs"
    );

    // Mock only 4 crawled pages (capped by max-pages = 4)
    let page_home = mock_page(
        "https://example.com/",
        200,
        0,
        Some("Home"),
        Some("Home Description"),
        None,
        0,
        0,
        vec![
            mock_link("https://example.com/", "https://example.com/about", false),
            mock_link("https://example.com/", "https://example.com/contact", false),
        ],
        vec![],
    );
    let page_about = mock_page(
        "https://example.com/about",
        200,
        1,
        Some("About Us"),
        Some("About Us Description"),
        None,
        0,
        0,
        vec![
            mock_link("https://example.com/about", "https://example.com/", false),
            mock_link(
                "https://example.com/about",
                "https://example.com/contact",
                false,
            ),
        ],
        vec![],
    );
    let page_contact = mock_page(
        "https://example.com/contact",
        200,
        1,
        Some("Contact"),
        Some("Contact Description"),
        None,
        0,
        0,
        vec![mock_link(
            "https://example.com/contact",
            "https://example.com/",
            false,
        )],
        vec![],
    );
    // This page was crawled, declared in sitemap, but receives 0 internal incoming links from any crawled page!
    let mut page_orphan = mock_page(
        "https://example.com/orphan-crawled",
        200,
        1,
        Some("Orphan Page"),
        Some("Orphan Description"),
        None,
        0,
        0,
        vec![mock_link(
            "https://example.com/orphan-crawled",
            "https://example.com/",
            false,
        )],
        vec![],
    );
    page_orphan.is_sitemap_url = true;

    let pages = vec![page_home, page_about, page_contact, page_orphan];
    let graph = SiteGraph::from_pages(&pages, &sitemap_urls);

    // Evaluate with crawl_exhaustive = false (simulating partial crawl)
    let issues = evaluate_graph_rules(&pages, &graph, &sitemap_urls, false, &HashMap::new());

    let orphan_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::AlertGraphOrphanPage)
        .collect();

    // In a partial crawl, the 20 uncrawled sitemap URLs must NOT be flagged as orphans!
    // ONLY the actually crawled orphan page (https://example.com/orphan-crawled) should be flagged.
    assert_eq!(
        orphan_issues.len(),
        1,
        "Partial crawl must ONLY flag actually crawled orphan pages, skipping uncrawled sitemap URLs"
    );
    assert_eq!(
        orphan_issues[0].target_url,
        "https://example.com/orphan-crawled"
    );

    // The health score for 4 crawled pages with 1 alert must NOT crash to 0!
    // 1 alert = 2.0 penalty. 2.0 / 4 pages = 0.5 * 10 = 5 deduction. Health score = 95.
    let score = calculate_health_score(pages.len(), &issues);
    assert_eq!(
        score, 95,
        "Health score must be 95 instead of collapsing to 0"
    );
}

#[test]
fn test_exhaustive_crawl_flags_all_unlinked_sitemap_urls_as_orphans() {
    let fixture_xml = include_str!("fixtures/large_sitemap_partial_crawl.xml");
    let doc = parse_sitemap(fixture_xml.as_bytes()).expect("Failed to parse fixture");
    let sitemap_urls: Vec<String> = match doc {
        SitemapDocument::UrlSet(entries) => {
            entries.into_iter().map(|e| e.loc.to_string()).collect()
        }
        _ => panic!("Expected standard UrlSet in fixture"),
    };

    let page_home = mock_page(
        "https://example.com/",
        200,
        0,
        Some("Home"),
        Some("Home Description"),
        None,
        100,
        200,
        vec![
            mock_link("https://example.com/", "https://example.com/about", false),
            mock_link("https://example.com/", "https://example.com/contact", false),
        ],
        vec![],
    );
    let page_about = mock_page(
        "https://example.com/about",
        200,
        1,
        Some("About Us"),
        Some("About Us Description"),
        None,
        101,
        201,
        vec![mock_link(
            "https://example.com/about",
            "https://example.com/",
            false,
        )],
        vec![],
    );
    let page_contact = mock_page(
        "https://example.com/contact",
        200,
        1,
        Some("Contact"),
        Some("Contact Description"),
        None,
        102,
        202,
        vec![mock_link(
            "https://example.com/contact",
            "https://example.com/",
            false,
        )],
        vec![],
    );
    let mut page_orphan = mock_page(
        "https://example.com/orphan-crawled",
        200,
        1,
        Some("Orphan Page"),
        Some("Orphan Description"),
        None,
        103,
        203,
        vec![mock_link(
            "https://example.com/orphan-crawled",
            "https://example.com/",
            false,
        )],
        vec![],
    );
    page_orphan.is_sitemap_url = true;

    let pages = vec![page_home, page_about, page_contact, page_orphan];
    let graph = SiteGraph::from_pages(&pages, &sitemap_urls);

    // Evaluate with crawl_exhaustive = true (simulating an exhaustive crawl that visited all linked pages)
    let issues = evaluate_graph_rules(&pages, &graph, &sitemap_urls, true, &HashMap::new());

    let orphan_issues: Vec<_> = issues
        .iter()
        .filter(|i| i.code == RuleId::AlertGraphOrphanPage)
        .collect();

    // In an exhaustive crawl, all 20 unlinked catalog URLs plus the 1 crawled orphan must be flagged = 21 orphans.
    assert_eq!(
        orphan_issues.len(),
        21,
        "Exhaustive crawl must flag all unlinked sitemap URLs as true orphans"
    );
}

#[test]
fn test_health_score_category_penalty_ceiling() {
    let mock_issue = |category, severity| IssueFinding {
        code: RuleId::AlertGraphOrphanPage,
        category,
        severity,
        title: CompactString::new("Orphan Page"),
        message: "Orphan description".to_string(),
        target_url: "https://example.com/test".to_string(),
        source_page_url: None,
    };

    // Case 1: 500 crawled pages with 20,000 Alert issues in a SINGLE category (e.g. SiteGraph)
    // Total penalty = 20,000 * 2.0 = 40,000.
    // Penalty per page = 40,000 / 500 = 80.0.
    // Raw deduction = 800.0.
    // With category ceiling of 35.0, deduction is capped at 35.0!
    // Result score = 100 - 35 = 65!
    let massive_single_category_issues: Vec<_> = (0..20_000)
        .map(|_| mock_issue(IssueCategory::SiteGraph, Severity::Alert))
        .collect();

    let score = calculate_health_score(500, &massive_single_category_issues);
    assert_eq!(
        score, 65,
        "A single catastrophic category defect must be capped at 35 points deduction, yielding 65/100"
    );

    // Case 2: Multi-category degradation across 3 distinct categories:
    // Category 1 (SiteGraph): 35 points (capped)
    // Category 2 (Security): 35 points (capped)
    // Category 3 (TitleMetadata): 10 Alert issues across 500 pages = 20 / 500 = 0.04 * 10 = 0.4 points deduction
    // Total deduction = 35 + 35 + 0.4 = 70.4 -> rounded to 70 deduction -> score = 30.
    let mut multi_category_issues = massive_single_category_issues;
    for _ in 0..10_000 {
        multi_category_issues.push(mock_issue(IssueCategory::Security, Severity::Critical));
    }
    for _ in 0..10 {
        multi_category_issues.push(mock_issue(IssueCategory::TitleMetadata, Severity::Alert));
    }

    let multi_score = calculate_health_score(500, &multi_category_issues);
    assert_eq!(
        multi_score, 30,
        "Multiple distinct failing categories should sum their capped deductions properly"
    );

    // Case 3: Zero pages returns 100
    assert_eq!(calculate_health_score(0, &[]), 100);
    assert_eq!(calculate_health_score(0, &multi_category_issues), 100);
}
