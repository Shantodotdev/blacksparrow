//! # Frontier URL Importance Scoring & Heuristics
//!
//! Evaluates candidate URLs discovered during crawling and computes an importance priority
//! score $P(u)$ to schedule authoritative hubs, category branches, and shallow pages before
//! deep pagination sequences or query-heavy faceted parameters.
//!
//! ## Importance Scoring Formula
//!
//! $$P(u) = S_{\text{seed}} + S_{\text{depth}} + S_{\text{indegree}} + S_{\text{path}} - P_{\text{query}} - P_{\text{pagination}}$$
//!
//! - **$S_{\text{seed}}$ (Seed / Sitemap Boost)**: $+500$ points if the URL is seed (depth 0) or declared in XML sitemaps.
//! - **$S_{\text{depth}}$ (Depth Decay)**: $\frac{1000}{1 + \text{depth}}$ ($+1000$ for root, $+500$ for depth 1, $+333$ for depth 2, $+250$ for depth 3).
//! - **$S_{\text{indegree}}$ (Authority Accumulation)**: $+50 \times \min(\text{in\_links}, 20)$ (up to $+1000$ points for highly referenced pages).
//! - **$S_{\text{path}}$ (Path Brevity)**: $100 - (20 \times \text{slash\_count})$ (prioritizes shallow section hubs over deep subdirectories).
//! - **$P_{\text{query}}$ (Query Parameter Penalty)**: $-150 \times \text{param\_count}$ (penalizes parameter-heavy dynamic URLs).
//! - **$P_{\text{pagination}}$ (Pagination Deprioritization)**: $-300$ if URL matches pagination query parameters or path structure.

/// Parses a URL into path segment count and query parameter count without heap allocations.
pub fn parse_url_segments(url: &str) -> (usize, usize) {
    // Strip scheme
    let after_scheme = if let Some(idx) = url.find("://") {
        &url[idx + 3..]
    } else {
        url
    };

    // Find path start
    let path_and_beyond = if let Some(idx) = after_scheme.find('/') {
        &after_scheme[idx..]
    } else {
        ""
    };

    let (path_part, query_part) = match path_and_beyond.find('?') {
        Some(q_idx) => {
            let path = &path_and_beyond[..q_idx];
            let rest = &path_and_beyond[q_idx + 1..];
            let query = match rest.find('#') {
                Some(h_idx) => &rest[..h_idx],
                None => rest,
            };
            (path, Some(query))
        }
        None => match path_and_beyond.find('#') {
            Some(h_idx) => (&path_and_beyond[..h_idx], None),
            None => (path_and_beyond, None),
        },
    };

    // Count non-empty path segments
    let segments = path_part.split('/').filter(|s| !s.is_empty()).count();

    // Count non-empty query parameters
    let query_count = match query_part {
        Some(q) => q.split('&').filter(|s| !s.is_empty()).count(),
        None => 0,
    };

    (segments, query_count)
}

/// Detects whether a candidate URL represents a paginated catalog page.
///
/// Recognizes pagination patterns:
/// - Query parameters: `page=\d+`, `p=\d+`, `pg=\d+`, `paged=\d+`, `pagination=\d+`
/// - Path segments: `/page/\d+/`, `/p/\d+/`, etc.
pub fn is_pagination_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();

    // Check query parameters
    if let Some(q_idx) = lower.find('?') {
        let query_str = match lower[q_idx + 1..].find('#') {
            Some(h_idx) => &lower[q_idx + 1..q_idx + 1 + h_idx],
            None => &lower[q_idx + 1..],
        };

        for param in query_str.split('&') {
            if let Some((k, v)) = param.split_once('=') {
                let key = k.trim();
                let val = v.trim();
                if matches!(key, "page" | "p" | "pg" | "paged" | "pagination")
                    && !val.is_empty()
                    && val.chars().all(|c| c.is_ascii_digit())
                {
                    return true;
                }
            }
        }
    }

    // Check path segments
    let path = if let Some(q_idx) = lower.find('?') {
        &lower[..q_idx]
    } else if let Some(h_idx) = lower.find('#') {
        &lower[..h_idx]
    } else {
        &lower
    };

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    for i in 0..segments.len() {
        let seg = segments[i];
        if (seg == "page" || seg == "p") && i + 1 < segments.len() {
            let next_seg = segments[i + 1];
            if !next_seg.is_empty() && next_seg.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }

    false
}

/// Calculates the Importance Priority Score $P(u)$ for a candidate URL.
///
/// Returns a signed integer score where higher values represent greater crawling urgency.
pub fn calculate_url_importance(
    url: &str,
    depth: u16,
    in_degree: u32,
    is_sitemap_or_seed: bool,
) -> i64 {
    // 1. Seed / Sitemap Boost: +500
    let s_seed = if is_sitemap_or_seed || depth == 0 {
        500i64
    } else {
        0i64
    };

    // 2. Depth Decay: 1000 / (1 + depth)
    let s_depth = 1000i64 / (1 + depth as i64);

    // 3. Authority Accumulation: +50 * min(in_links, 20)
    let s_indegree = 50i64 * (in_degree as i64).min(20);

    // 4. Path Brevity: 100 - (20 * segment_count)
    let (segments, query_count) = parse_url_segments(url);
    let s_path = 100i64 - (20i64 * segments as i64);

    // 5. Query Parameter Penalty: -150 * param_count
    let p_query = 150i64 * query_count as i64;

    // 6. Pagination Penalty: -300 if paginated
    let p_pagination = if is_pagination_url(url) { 300i64 } else { 0i64 };

    // 7. Sorting / Display Facet Penalty: -500 if contains sorting or display parameters
    let p_sorting = if crate::core::url::has_sorting_facets(url) {
        500i64
    } else {
        0i64
    };

    s_seed + s_depth + s_indegree + s_path - p_query - p_pagination - p_sorting
}
