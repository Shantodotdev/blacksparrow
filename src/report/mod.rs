//! # Reporting & Exporters Module
//!
//! Exporters and presentation layers for technical SEO audit results:
//! - [`terminal`]: Live ANSI terminal progress indicators and post-crawl executive audit matrix.
//! - [`markdown`]: Formatted GitHub-Flavored Markdown for human reading and LLM context windows.
//! - [`json`]: Complete structured JSON export for automated pipelines and data warehouses.
//! - [`score`]: Normalized 0–100 technical SEO Health Score calculation.

pub mod inspector;
pub mod json;
pub mod markdown;
pub mod score;
pub mod terminal;

pub use inspector::{format_page_inspection, print_page_inspection};
pub use json::export_json_report;
pub use markdown::export_markdown_report;
pub use score::calculate_health_score;
pub use terminal::{
    create_crawl_progress_bar, finish_crawl_progress, print_audit_banner,
    print_executive_scorecard, print_historical_sessions, update_crawl_progress,
};
