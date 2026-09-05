//! # Internal Link Equity (PageRank) Engine
//!
//! Computes PageRank scores over the internal site topology graph using the
//! power-iteration method.
//!
//! Supports customizable damping factors ($d = 0.85$), convergence thresholds,
//! handling of dangling nodes (zero outlinks), and exclusion of `nofollow` edges.

use crate::graph::topology::{LinkEdgeType, SiteGraph};
use hashbrown::HashMap;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

/// Computes internal PageRank link equity distribution over a [`SiteGraph`].
///
/// Implements Google-style power iteration:
///
/// $$PR_{t+1}(v) = \frac{1 - d}{N} + d \cdot \frac{S_{\text{dangling}}}{N} + d \cdot \sum_{u \in \text{In}(v)} \frac{PR_t(u)}{|\text{Out}(u)|}$$
///
/// # Arguments
///
/// * `site_graph` - The internal topology graph to analyze.
/// * `damping_factor` - Probability of continuing random crawl traversal (typically 0.85).
/// * `max_iterations` - Maximum power-iteration loops to prevent infinite cycles.
/// * `tolerance` - $L_1$ norm convergence threshold ($\epsilon$, typically $10^{-6}$).
///
/// # Returns
///
/// A map associating each node's 64-bit URL hash with its normalized PageRank equity score.
pub fn compute_pagerank(
    site_graph: &SiteGraph,
    damping_factor: f64,
    max_iterations: usize,
    tolerance: f64,
) -> HashMap<u64, f64> {
    let graph = site_graph.raw_graph();
    let num_nodes = graph.node_count();

    if num_nodes == 0 {
        return HashMap::new();
    }

    if num_nodes == 1 {
        let mut single_res = HashMap::with_capacity(1);
        if let Some(node) = graph.node_weight(petgraph::graph::NodeIndex::new(0)) {
            single_res.insert(node.url_hash, 1.0);
        }
        return single_res;
    }

    let n = num_nodes as f64;
    let initial_score = 1.0 / n;
    let mut scores = vec![initial_score; num_nodes];
    let mut next_scores = vec![0.0; num_nodes];

    // Precalculate outgoing valid link counts for each node (excluding nofollow and non-hyperlinks)
    let mut out_degrees = vec![0usize; num_nodes];
    for (i, out_deg) in out_degrees.iter_mut().enumerate() {
        let idx = petgraph::graph::NodeIndex::new(i);
        *out_deg = graph
            .edges_directed(idx, Direction::Outgoing)
            .filter(|e| {
                e.weight().edge_type == LinkEdgeType::InternalHyperlink && !e.weight().is_nofollow
            })
            .count();
    }

    let teleport_base = (1.0 - damping_factor) / n;

    for _ in 0..max_iterations {
        // Calculate equity contribution from dangling nodes (nodes with 0 outgoing followed internal links)
        let mut dangling_equity = 0.0;
        for (i, &score) in scores.iter().enumerate() {
            if out_degrees[i] == 0 {
                dangling_equity += score;
            }
        }
        let dangling_redistribution = damping_factor * (dangling_equity / n);

        for (v_idx_raw, next_score) in next_scores.iter_mut().enumerate() {
            let v_idx = petgraph::graph::NodeIndex::new(v_idx_raw);

            // Sum incoming link equity from non-nofollow internal links
            let mut incoming_equity = 0.0;
            for edge in graph.edges_directed(v_idx, Direction::Incoming) {
                if edge.weight().edge_type == LinkEdgeType::InternalHyperlink
                    && !edge.weight().is_nofollow
                {
                    let u_idx_raw = edge.source().index();
                    let u_out = out_degrees[u_idx_raw];
                    if u_out > 0 {
                        incoming_equity += scores[u_idx_raw] / (u_out as f64);
                    }
                }
            }

            *next_score =
                teleport_base + dangling_redistribution + (damping_factor * incoming_equity);
        }

        // Evaluate L1 convergence difference
        let mut delta = 0.0;
        for i in 0..num_nodes {
            delta += (next_scores[i] - scores[i]).abs();
        }

        std::mem::swap(&mut scores, &mut next_scores);

        if delta < tolerance {
            break;
        }
    }

    // Ensure sum equals 1.0 through normalization
    let sum: f64 = scores.iter().sum();
    let norm_factor = if sum > 0.0 { 1.0 / sum } else { 1.0 };

    let mut result = HashMap::with_capacity(num_nodes);
    for (i, score) in scores.into_iter().enumerate() {
        if let Some(node) = graph.node_weight(petgraph::graph::NodeIndex::new(i)) {
            result.insert(node.url_hash, score * norm_factor);
        }
    }

    result
}
