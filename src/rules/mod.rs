//! # Technical SEO Rules Engine
//!
//! Master evaluation engine for document-level (in-flight) and graph-level (post-crawl) checks.
//!
//! ## Modules
//!
//! - [`catalog`]: Authoritative dictionary defining 120 SEO rule specifications, severity tiers, and fix advice.
//! - [`page`]: In-flight document-level rules (titles, headings, status, security, schemas, mobile, CLS).
//!
//! ## Examples
//!
//! ```rust
//! use blacksparrow::rules::catalog::{get_rule, RuleId};
//! use blacksparrow::core::models::Severity;
//!
//! let rule = get_rule(RuleId::ErrH1Missing);
//! assert_eq!(rule.severity, Severity::Critical);
//! ```

pub mod catalog;
pub mod graph;
pub mod page;

pub use graph::evaluate_graph_rules as evaluate_graph;
pub use page::evaluate_page_rules as evaluate_page;
