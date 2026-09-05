//! # Site Graph Topology & PageRank Engine
//!
//! Internal link graph representation (`petgraph`), topology metrics,
//! and Google-style PageRank power iteration.
//!
//! ## Submodules
//!
//! - [`graph`]: Directed graph implementation ([`SiteGraph`]), node indexing, and cycle detection.
//! - [`pagerank`]: Power-iteration internal link equity calculation ([`compute_pagerank`]).

pub mod pagerank;
pub mod topology;

pub use pagerank::compute_pagerank;
pub use topology::{LinkEdge, LinkEdgeType, PageNode, SiteGraph};
