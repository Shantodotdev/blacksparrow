//! # Site Graph Topology
//!
//! Directed graph model (`petgraph`) representing the internal link topology of a crawled website.
//!
//! Tracks internal hyperlinks, redirects, and canonical links as directed edges,
//! providing graph-level metrics such as in-degree, out-degree, cycle detection,
//! and chain tracing.

use crate::core::models::PageReport;
use crate::core::url::url_hash;
use compact_str::CompactString;
use hashbrown::{HashMap, HashSet};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;

/// Edge classification for internal graph transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinkEdgeType {
    /// Standard navigational hyperlink (`<a href="...">`).
    InternalHyperlink,
    /// Canonical link declaration (`<link rel="canonical" href="...">`).
    Canonical,
    /// HTTP redirect transition (`301`, `302`, `307`, `308`).
    Redirect,
}

/// Metadata payload stored on each graph node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageNode {
    /// Fully normalized absolute URL.
    pub url: String,
    /// 64-bit deterministic hash of the normalized URL.
    pub url_hash: u64,
    /// HTTP status code (e.g. 200, 301, 404).
    pub status_code: u16,
    /// Crawl depth from the root seed URL.
    pub crawl_depth: u16,
    /// Whether this URL was discovered in the site's XML sitemap.
    pub is_sitemap_url: bool,
}

/// Metadata payload stored on each directed graph edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkEdge {
    /// The structural relationship type between source and target.
    pub edge_type: LinkEdgeType,
    /// Whether the link includes a `rel="nofollow"` directive.
    pub is_nofollow: bool,
    /// Anchor text or child alt text associated with the link.
    pub anchor_text: CompactString,
}

/// Directed internal link topology graph.
///
/// Built on top of [`petgraph::graph::DiGraph`], indexed by 64-bit URL hashes
/// for efficient $O(1)$ node lookup and fast neighbor traversal.
#[derive(Debug, Clone, Default)]
pub struct SiteGraph {
    /// Underlying directed graph data structure.
    graph: DiGraph<PageNode, LinkEdge>,
    /// Index mapping 64-bit URL hashes to petgraph `NodeIndex`.
    url_to_node: HashMap<u64, NodeIndex>,
}

impl SiteGraph {
    /// Creates a new empty `SiteGraph`.
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            url_to_node: HashMap::new(),
        }
    }

    /// Returns the total count of nodes (pages) in the graph.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Returns the total count of edges (links) in the graph.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Returns the `NodeIndex` for a URL, if present in the graph.
    pub fn get_node_index(&self, url: &str) -> Option<NodeIndex> {
        let hash = url_hash(url);
        self.url_to_node.get(&hash).copied()
    }

    /// Returns a reference to the `PageNode` for a URL, if present.
    pub fn get_node(&self, url: &str) -> Option<&PageNode> {
        let idx = self.get_node_index(url)?;
        self.graph.node_weight(idx)
    }

    /// Adds or updates a node in the graph.
    ///
    /// If the node already exists, its status code, crawl depth, and sitemap flag
    /// are updated if the new information is more specific.
    pub fn add_node(
        &mut self,
        url: &str,
        status_code: u16,
        crawl_depth: u16,
        is_sitemap_url: bool,
    ) -> NodeIndex {
        let hash = url_hash(url);
        if let Some(&idx) = self.url_to_node.get(&hash) {
            if let Some(node) = self.graph.node_weight_mut(idx) {
                if status_code != 0 {
                    node.status_code = status_code;
                }
                if is_sitemap_url {
                    node.is_sitemap_url = true;
                }
                if crawl_depth < node.crawl_depth {
                    node.crawl_depth = crawl_depth;
                }
            }
            idx
        } else {
            let node = PageNode {
                url: url.to_string(),
                url_hash: hash,
                status_code,
                crawl_depth,
                is_sitemap_url,
            };
            let idx = self.graph.add_node(node);
            self.url_to_node.insert(hash, idx);
            idx
        }
    }

    /// Adds a directed edge between two pages in the graph.
    ///
    /// Automatically ensures source and target nodes exist in the graph.
    pub fn add_edge(
        &mut self,
        source_url: &str,
        target_url: &str,
        edge_type: LinkEdgeType,
        is_nofollow: bool,
        anchor_text: &str,
    ) {
        let source_idx = self.add_node(source_url, 0, 0, false);
        let target_idx = self.add_node(target_url, 0, 0, false);

        let edge = LinkEdge {
            edge_type,
            is_nofollow,
            anchor_text: CompactString::new(anchor_text),
        };

        self.graph.add_edge(source_idx, target_idx, edge);
    }

    /// Computes the incoming internal hyperlink count for a page.
    ///
    /// Considers only [`LinkEdgeType::InternalHyperlink`] edges.
    pub fn in_degree(&self, url: &str) -> usize {
        match self.get_node_index(url) {
            Some(idx) => self
                .graph
                .edges_directed(idx, Direction::Incoming)
                .filter(|e| e.weight().edge_type == LinkEdgeType::InternalHyperlink)
                .count(),
            None => 0,
        }
    }

    /// Computes the outgoing internal hyperlink count for a page.
    ///
    /// Considers only [`LinkEdgeType::InternalHyperlink`] edges.
    pub fn out_degree(&self, url: &str) -> usize {
        match self.get_node_index(url) {
            Some(idx) => self
                .graph
                .edges_directed(idx, Direction::Outgoing)
                .filter(|e| e.weight().edge_type == LinkEdgeType::InternalHyperlink)
                .count(),
            None => 0,
        }
    }

    /// Builds a `SiteGraph` from crawled page reports and discovered sitemap URLs.
    pub fn from_pages(pages: &[PageReport], sitemap_urls: &[String]) -> Self {
        let mut graph = Self::new();

        // 1. Seed nodes from sitemap URLs
        for sitemap_url in sitemap_urls {
            graph.add_node(sitemap_url, 0, 0, true);
        }

        // 2. Add all crawled pages
        for page in pages {
            graph.add_node(
                &page.url,
                page.status_code,
                page.crawl_depth,
                page.is_sitemap_url,
            );
        }

        // 3. Populate directed edges
        for page in pages {
            // HTTP Redirect edge
            if let Some(ref dest) = page.final_url {
                if dest != &page.url {
                    graph.add_edge(&page.url, dest, LinkEdgeType::Redirect, false, "");
                }
            }

            // Canonical link edge
            if let Some(ref canon) = page.canonical_url {
                if canon != &page.url {
                    graph.add_edge(&page.url, canon, LinkEdgeType::Canonical, false, "");
                }
            }

            // Navigational internal hyperlinks
            for link in &page.links {
                if link.is_internal {
                    graph.add_edge(
                        &page.url,
                        &link.target_url,
                        LinkEdgeType::InternalHyperlink,
                        link.is_nofollow,
                        &link.anchor_text,
                    );
                }
            }
        }

        graph
    }

    /// Detects circular redirect loops in the graph.
    ///
    /// Returns a list of cycles, where each cycle is represented as an ordered sequence of URLs.
    pub fn find_redirect_loops(&self) -> Vec<Vec<String>> {
        let mut loops = Vec::new();
        let mut visited_globally = HashSet::new();

        for node_idx in self.graph.node_indices() {
            if visited_globally.contains(&node_idx) {
                continue;
            }

            let mut path = Vec::new();
            let mut path_indices = HashSet::new();
            let mut curr = node_idx;

            loop {
                visited_globally.insert(curr);
                path.push(curr);
                path_indices.insert(curr);

                // Find next redirect target
                let next_redirect = self
                    .graph
                    .edges_directed(curr, Direction::Outgoing)
                    .find(|e| e.weight().edge_type == LinkEdgeType::Redirect)
                    .map(|e| e.target());

                match next_redirect {
                    Some(target) => {
                        if path_indices.contains(&target) {
                            // Cycle detected! Extract cycle portion
                            if let Some(start_pos) = path.iter().position(|&idx| idx == target) {
                                let mut cycle_urls = Vec::new();
                                for &idx in &path[start_pos..] {
                                    if let Some(node) = self.graph.node_weight(idx) {
                                        cycle_urls.push(node.url.clone());
                                    }
                                }
                                if let Some(first_url) = cycle_urls.first().cloned() {
                                    cycle_urls.push(first_url);
                                }
                                loops.push(cycle_urls);
                            }
                            break;
                        } else if visited_globally.contains(&target) {
                            // Hit a previously explored path that had no cycle
                            break;
                        } else {
                            curr = target;
                        }
                    }
                    None => break,
                }
            }
        }

        loops
    }

    /// Detects multi-hop redirect chains (chains with $>1$ redirect hops).
    ///
    /// Returns chains represented as sequences of URLs `[A, B, C, ...]`.
    pub fn find_redirect_chains(&self) -> Vec<Vec<String>> {
        let mut chains = Vec::new();

        for node_idx in self.graph.node_indices() {
            // Check if node is the start of a redirect (and not a target of an incoming redirect)
            let has_incoming_redirect = self
                .graph
                .edges_directed(node_idx, Direction::Incoming)
                .any(|e| e.weight().edge_type == LinkEdgeType::Redirect);

            if has_incoming_redirect {
                continue;
            }

            let has_outgoing_redirect = self
                .graph
                .edges_directed(node_idx, Direction::Outgoing)
                .any(|e| e.weight().edge_type == LinkEdgeType::Redirect);

            if !has_outgoing_redirect {
                continue;
            }

            let mut path = Vec::new();
            let mut curr = node_idx;
            let mut visited = HashSet::new();

            loop {
                if !visited.insert(curr) {
                    // Loop detected, handled by find_redirect_loops
                    break;
                }

                if let Some(node) = self.graph.node_weight(curr) {
                    path.push(node.url.clone());
                }

                let next_redirect = self
                    .graph
                    .edges_directed(curr, Direction::Outgoing)
                    .find(|e| e.weight().edge_type == LinkEdgeType::Redirect)
                    .map(|e| e.target());

                match next_redirect {
                    Some(target) => curr = target,
                    None => break,
                }
            }

            // A chain has occurred if there are at least 3 nodes (2 hops: A -> B -> C)
            if path.len() > 2 {
                chains.push(path);
            }
        }

        chains
    }

    /// Detects circular canonical references (e.g. Page A -> B and Page B -> A).
    pub fn find_canonical_loops(&self) -> Vec<(String, String)> {
        let mut loops = Vec::new();
        let mut seen_pairs = HashSet::new();

        for node_idx in self.graph.node_indices() {
            let canon_targets: Vec<_> = self
                .graph
                .edges_directed(node_idx, Direction::Outgoing)
                .filter(|e| e.weight().edge_type == LinkEdgeType::Canonical)
                .map(|e| e.target())
                .collect();

            for target_idx in canon_targets {
                if target_idx == node_idx {
                    continue; // Self-canonical is valid and not a loop
                }

                // Check if target points back with a canonical edge
                let return_canonical = self
                    .graph
                    .edges_directed(target_idx, Direction::Outgoing)
                    .any(|e| {
                        e.weight().edge_type == LinkEdgeType::Canonical && e.target() == node_idx
                    });

                if return_canonical {
                    let u1 = self.graph.node_weight(node_idx).map(|n| n.url.as_str());
                    let u2 = self.graph.node_weight(target_idx).map(|n| n.url.as_str());

                    if let (Some(url_a), Some(url_b)) = (u1, u2) {
                        let pair_key = if url_a < url_b {
                            (url_a.to_string(), url_b.to_string())
                        } else {
                            (url_b.to_string(), url_a.to_string())
                        };

                        if seen_pairs.insert(pair_key) {
                            loops.push((url_a.to_string(), url_b.to_string()));
                        }
                    }
                }
            }
        }

        loops
    }

    /// Returns internal references to the raw petgraph instance and indices for algorithms.
    pub(crate) fn raw_graph(&self) -> &DiGraph<PageNode, LinkEdge> {
        &self.graph
    }
}
