//! # Structured Data & JSON-LD Parser
//!
//! Extraction and validation of schema.org JSON-LD structured data blocks
//! and Google Rich Results eligibility heuristics.
//!
//! ## Structured Data in Technical SEO
//!
//! Structured data provides explicit clues about the meaning of a page to search engines.
//! While microdata and RDFa embed metadata inside HTML tags, **JSON-LD**
//! (`<script type="application/ld+json">`) is Google's strongly recommended format because
//! it decouples semantic metadata from presentation markup.
//!
//! ### Key Rich Results Features
//!
//! - **Product**: Price, availability, aggregate rating stars.
//! - **Article & BlogPosting**: Headline, author, datePublished for Google News and Discover.
//! - **BreadcrumbList**: Hierarchical navigational trail in search snippets.
//! - **FAQPage**: Expandable accordion questions and answers directly in SERPs.
//! - **Recipe**: Cooking time, calories, ratings, and dietary information.
//!
//! ## `@graph` Flattening
//!
//! Modern WordPress SEO plugins (Yoast, RankMath) and CMS frameworks bundle all page entities
//! (Organization, WebSite, WebPage, Article, Author, Breadcrumbs) into a unified `@graph`
//! array container. The parser recursively traverses `@graph` arrays to extract every typed
//! entity as an individual [`SchemaRecord`].

use crate::core::models::SchemaRecord;
use compact_str::CompactString;
use serde_json::Value;

/// Known Google Rich Results eligible top-level schema types.
const GOOGLE_ELIGIBLE_TYPES: &[&str] = &[
    "Article",
    "NewsArticle",
    "BlogPosting",
    "Product",
    "LocalBusiness",
    "Organization",
    "BreadcrumbList",
    "FAQPage",
    "HowTo",
    "Recipe",
    "Review",
    "Event",
    "Course",
    "JobPosting",
    "VideoObject",
];

/// Parses raw JSON-LD text into one or more [`SchemaRecord`] instances.
///
/// Automatically flattens `@graph` containers, extracts `@type` classifications,
/// and checks against Google Rich Results eligibility requirements.
///
/// If the input string is malformed JSON, a record with type `"InvalidJson"` and
/// `is_valid_json: false` is returned.
///
/// # Examples
///
/// ```rust
/// use blacksparrow::parser::schema::parse_json_ld;
///
/// // Example 1: Standard Article schema
/// let json_ld = r#"
/// {
///     "@context": "https://schema.org",
///     "@type": "Article",
///     "headline": "Understanding Streaming HTML Parsers",
///     "author": { "@type": "Person", "name": "Jane Doe" }
/// }
/// "#;
///
/// let records = parse_json_ld(json_ld);
/// assert_eq!(records.len(), 1);
/// assert_eq!(records[0].schema_type.as_str(), "Article");
/// assert!(records[0].is_valid_json);
/// assert!(records[0].is_google_eligible);
///
/// // Example 2: Invalid JSON is gracefully captured
/// let bad_json = "{ invalid_json: true, }";
/// let records = parse_json_ld(bad_json);
/// assert_eq!(records.len(), 1);
/// assert_eq!(records[0].schema_type.as_str(), "InvalidJson");
/// assert!(!records[0].is_valid_json);
/// ```
pub fn parse_json_ld(raw_json: &str) -> Vec<SchemaRecord> {
    let trimmed = raw_json.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    match serde_json::from_str::<Value>(trimmed) {
        Ok(parsed) => extract_schemas_from_value(&parsed, trimmed),
        Err(_) => vec![SchemaRecord {
            schema_type: CompactString::new("InvalidJson"),
            raw_json: trimmed.to_string(),
            is_valid_json: false,
            is_google_eligible: false,
            missing_required_fields: Vec::new(),
        }],
    }
}

/// Recursively traverses a parsed JSON-LD value to extract all typed schema entities.
fn extract_schemas_from_value(val: &Value, raw_json: &str) -> Vec<SchemaRecord> {
    let mut records = Vec::new();

    match val {
        Value::Object(map) => {
            // Check for @graph container
            if let Some(Value::Array(graph)) = map.get("@graph") {
                for item in graph {
                    records.extend(extract_schemas_from_value(item, raw_json));
                }
                return records;
            }

            // Extract @type
            if let Some(type_val) = map.get("@type") {
                let type_str = match type_val {
                    Value::String(s) => s.clone(),
                    Value::Array(arr) => arr
                        .first()
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown")
                        .to_string(),
                    _ => "Unknown".to_string(),
                };

                let is_eligible = GOOGLE_ELIGIBLE_TYPES
                    .iter()
                    .any(|&t| t.eq_ignore_ascii_case(&type_str));

                records.push(SchemaRecord {
                    schema_type: CompactString::new(&type_str),
                    raw_json: raw_json.to_string(),
                    is_valid_json: true,
                    is_google_eligible: is_eligible,
                    missing_required_fields: Vec::new(),
                });
            }
        }
        Value::Array(arr) => {
            for item in arr {
                records.extend(extract_schemas_from_value(item, raw_json));
            }
        }
        _ => {}
    }

    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_single_schema() {
        let json = r#"{
            "@context": "https://schema.org",
            "@type": "Product",
            "name": "High Performance Engine"
        }"#;

        let schemas = parse_json_ld(json);
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].schema_type.as_str(), "Product");
        assert!(schemas[0].is_valid_json);
        assert!(schemas[0].is_google_eligible);
    }

    #[test]
    fn test_parse_graph_schema() {
        let json = r#"{
            "@context": "https://schema.org",
            "@graph": [
                { "@type": "Organization", "name": "SEO Lens Corp" },
                { "@type": "WebSite", "url": "https://example.com" }
            ]
        }"#;

        let schemas = parse_json_ld(json);
        assert_eq!(schemas.len(), 2);
        assert_eq!(schemas[0].schema_type.as_str(), "Organization");
        assert_eq!(schemas[1].schema_type.as_str(), "WebSite");
    }

    #[test]
    fn test_parse_invalid_json() {
        let json = r#"{ "@type": "Article", broken json here ... "#;
        let schemas = parse_json_ld(json);
        assert_eq!(schemas.len(), 1);
        assert!(!schemas[0].is_valid_json);
        assert_eq!(schemas[0].schema_type.as_str(), "InvalidJson");
    }
}
