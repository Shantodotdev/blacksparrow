//! # Frontier Priority & Smart Ordering Integration Tests
//!
//! Validates the Intelligent Priority Frontier (`PriorityFrontier`), URL importance
//! scoring heuristics, dynamic in-degree authority accumulation, and pagination suppression.

use blacksparrow::crawler::frontier::Frontier;
use blacksparrow::crawler::priority::{
    calculate_url_importance, is_pagination_url, parse_url_segments,
};

#[test]
fn test_parse_url_segments_and_parameters() {
    let (segments, query_count) =
        parse_url_segments("https://example.com/shop/laptops?sort=price&brand=asus");
    assert_eq!(segments, 2);
    assert_eq!(query_count, 2);

    let (root_segments, root_query) = parse_url_segments("https://example.com/");
    assert_eq!(root_segments, 0);
    assert_eq!(root_query, 0);

    let (deep_segments, deep_query) = parse_url_segments("https://example.com/a/b/c/d/e/f");
    assert_eq!(deep_segments, 6);
    assert_eq!(deep_query, 0);
}

#[test]
fn test_is_pagination_url() {
    assert!(is_pagination_url("https://example.com/products?page=2"));
    assert!(is_pagination_url("https://example.com/products?p=10"));
    assert!(is_pagination_url("https://example.com/products?pg=3"));
    assert!(is_pagination_url("https://example.com/products?paged=5"));
    assert!(is_pagination_url(
        "https://example.com/category/shoes/page/3"
    ));
    assert!(is_pagination_url(
        "https://example.com/category/shoes/page/4/"
    ));

    assert!(!is_pagination_url("https://example.com/"));
    assert!(!is_pagination_url("https://example.com/about-us"));
    assert!(!is_pagination_url("https://example.com/products/item-123"));
    assert!(!is_pagination_url(
        "https://example.com/pages/terms-and-conditions"
    ));
}

#[test]
fn test_url_importance_scoring_components() {
    // 1. Root seed: depth 0, 0 segments, 0 queries, not pagination
    // Seed boost (+500) + depth 0 (+1000) + indegree 1 (+50) + path 0 (+100) = 1650
    let root_score = calculate_url_importance("https://example.com/", 0, 1, true);
    assert_eq!(root_score, 1650);

    // 2. Shallow page in sitemap: depth 1, 1 segment (/about), 0 queries, 1 inlink
    // Seed boost (+500) + depth 1 (+500) + indegree 1 (+50) + path 1 (+80) = 1130
    let sitemap_about = calculate_url_importance("https://example.com/about", 1, 1, true);
    assert_eq!(sitemap_about, 1130);

    // 3. Shallow page NOT in sitemap: depth 1, 1 segment (/about), 0 queries, 1 inlink
    // No seed (0) + depth 1 (+500) + indegree 1 (+50) + path 1 (+80) = 630
    let non_sitemap_about = calculate_url_importance("https://example.com/about", 1, 1, false);
    assert_eq!(non_sitemap_about, 630);

    // 4. Deep product page: depth 3, 3 segments, 0 queries, 1 inlink
    // No seed (0) + depth 3 (+250) + indegree 1 (+50) + path 3 (+40) = 340
    let deep_product =
        calculate_url_importance("https://example.com/shop/shoes/running-trail", 3, 1, false);
    assert_eq!(deep_product, 340);

    // 5. Pagination URL: depth 1, 1 segment, 1 query param (?page=2), pagination (-300)
    // No seed (0) + depth 1 (+500) + indegree 1 (+50) + path 1 (+80) - query (-150) - pagination (-300) = 180
    let paginated_url = calculate_url_importance("https://example.com/shop?page=2", 1, 1, false);
    assert_eq!(paginated_url, 180);

    // Assert strictly ordered hierarchy
    assert!(root_score > sitemap_about);
    assert!(sitemap_about > non_sitemap_about);
    assert!(non_sitemap_about > deep_product);
    assert!(deep_product > paginated_url);
}

#[test]
fn test_frontier_priority_queue_ordering() {
    let mut frontier = Frontier::new(100, 5);

    // Register sitemap URL
    frontier.register_sitemap_urls(&["https://example.com/contact".to_string()]);

    // Push URLs in deliberately inverted / non-optimal order
    frontier
        .push("https://example.com/products?page=10", 2, None)
        .unwrap();
    frontier
        .push(
            "https://example.com/catalog/electronics/laptops/gaming/alienware-x16",
            4,
            None,
        )
        .unwrap();
    frontier.push("https://example.com/about", 1, None).unwrap();
    frontier
        .push("https://example.com/contact", 1, None)
        .unwrap();
    frontier.push("https://example.com/", 0, None).unwrap();

    // Popping should follow strict importance order
    let p1 = frontier.pop().expect("Should pop 1st");
    assert_eq!(
        p1.url, "https://example.com/",
        "Homepage seed must pop first"
    );

    let p2 = frontier.pop().expect("Should pop 2nd");
    assert_eq!(
        p2.url, "https://example.com/contact",
        "Sitemap-boosted hub must pop 2nd"
    );

    let p3 = frontier.pop().expect("Should pop 3rd");
    assert_eq!(
        p3.url, "https://example.com/about",
        "Clean shallow hub must pop 3rd"
    );

    let p4 = frontier.pop().expect("Should pop 4th");
    assert_eq!(
        p4.url, "https://example.com/catalog/electronics/laptops/gaming/alienware-x16",
        "Deep product must pop 4th"
    );

    let p5 = frontier.pop().expect("Should pop 5th");
    assert_eq!(
        p5.url, "https://example.com/products?page=10",
        "Pagination must pop last"
    );

    assert!(frontier.pop().is_none());
}

#[test]
fn test_dynamic_in_degree_authority_accumulation() {
    let mut frontier = Frontier::new(100, 5);

    // Enqueue two competing pages at depth 2
    // Item A: https://example.com/shop/item-a
    // Item B: https://example.com/shop/item-b
    frontier
        .push(
            "https://example.com/shop/item-a",
            2,
            Some("https://example.com/cat1"),
        )
        .unwrap();
    frontier
        .push(
            "https://example.com/shop/item-b",
            2,
            Some("https://example.com/cat1"),
        )
        .unwrap();

    // Initially both have indegree = 1. Now simulate 5 other pages discovering / linking to item-b!
    for i in 1..=5 {
        let src = format!("https://example.com/blog/review-{}", i);
        // Pushing an already-pending URL boosts its in-degree count and escalates its priority!
        frontier
            .push("https://example.com/shop/item-b", 2, Some(&src))
            .unwrap();
    }

    // Now item-b has in-degree = 6 (accumulating +250 extra priority points), while item-a only has in-degree = 1.
    // Item-b MUST pop before item-a!
    let first = frontier.pop().unwrap();
    assert_eq!(
        first.url, "https://example.com/shop/item-b",
        "Highly-referenced item-b must bubble above item-a"
    );

    let second = frontier.pop().unwrap();
    assert_eq!(second.url, "https://example.com/shop/item-a");

    // The queue should not contain duplicate entries for item-b
    assert!(frontier.pop().is_none());
}
