//! # Structured Data & Schema Validation Rules
//!
//! Evaluates JSON-LD schema syntax correctness, Google Rich Results required fields,
//! and contextual schema compliance based on detected page intent.

use crate::core::models::{IssueFinding, PageArchetype};
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};
use serde_json::Value;

/// Evaluates structured data records against syntax and validation rules,
/// and audits contextual structured data based on page intent.
pub fn check_schemas(page: &ParsedPage, url: &str, issues: &mut Vec<IssueFinding>) {
    // 1. Existing JSON-LD syntax and missing required fields checks
    for schema in &page.schemas {
        if !schema.is_valid_json {
            let rule = get_rule(RuleId::ErrSchemaSyntaxError);
            issues.push(rule.to_finding(
                url,
                Some("Invalid JSON syntax in <script type=\"application/ld+json\"> block."),
            ));
        }

        if !schema.missing_required_fields.is_empty() {
            let rule = get_rule(RuleId::WarnSchemaMissingRequiredFields);
            issues.push(rule.to_finding(
                url,
                Some(&format!(
                    "Schema @type '{}' is missing required fields for Google Rich Results: {}",
                    schema.schema_type,
                    schema.missing_required_fields.join(", ")
                )),
            ));
        }
    }

    // 2. Contextual Intent-Driven Schema Auditing
    match page.page_intent.archetype {
        PageArchetype::Product => {
            let product_schemas: Vec<&crate::core::models::SchemaRecord> = page
                .schemas
                .iter()
                .filter(|s| s.is_valid_json && s.schema_type.eq_ignore_ascii_case("product"))
                .collect();

            if product_schemas.is_empty() {
                let rule = get_rule(RuleId::ErrProductMissingSchema);
                issues.push(rule.to_finding(
                    url,
                    Some("Page classified as E-Commerce Product (PDP), but missing Schema.org Product structured data."),
                ));
            } else {
                for s in product_schemas {
                    if let Ok(json_val) = serde_json::from_str::<Value>(&s.raw_json) {
                        let has_offers = json_has_field(&json_val, "offers");
                        let has_price = json_has_field(&json_val, "price")
                            || json_has_nested_field(&json_val, "offers", "price")
                            || json_has_nested_field(&json_val, "offers", "lowPrice");

                        if !has_offers && !has_price {
                            let rule = get_rule(RuleId::WarnProductMissingPriceOffer);
                            issues.push(rule.to_finding(
                                url,
                                Some("Product schema lacks an 'offers' entity or 'price' specification."),
                            ));
                        }

                        let has_availability = json_has_field(&json_val, "availability")
                            || json_has_nested_field(&json_val, "offers", "availability");

                        if !has_availability {
                            let rule = get_rule(RuleId::WarnProductMissingAvailability);
                            issues.push(rule.to_finding(
                                url,
                                Some("Product schema lacks an 'availability' specification (e.g. InStock, OutOfStock)."),
                            ));
                        }
                    }
                }
            }
        }
        PageArchetype::Article => {
            let article_schemas: Vec<&crate::core::models::SchemaRecord> = page
                .schemas
                .iter()
                .filter(|s| {
                    s.is_valid_json
                        && (s.schema_type.eq_ignore_ascii_case("article")
                            || s.schema_type.eq_ignore_ascii_case("newsarticle")
                            || s.schema_type.eq_ignore_ascii_case("blogposting")
                            || s.schema_type.eq_ignore_ascii_case("techarticle")
                            || s.schema_type.eq_ignore_ascii_case("report"))
                })
                .collect();

            if article_schemas.is_empty() {
                let rule = get_rule(RuleId::ErrArticleMissingSchema);
                issues.push(rule.to_finding(
                    url,
                    Some("Page classified as Editorial Article, but missing Schema.org Article or NewsArticle structured data."),
                ));
            } else {
                for s in article_schemas {
                    if let Ok(json_val) = serde_json::from_str::<Value>(&s.raw_json) {
                        let has_author = json_has_field(&json_val, "author")
                            || json_has_field(&json_val, "creator");
                        if !has_author {
                            let rule = get_rule(RuleId::WarnArticleMissingAuthor);
                            issues.push(rule.to_finding(
                                url,
                                Some("Article schema is missing the 'author' Person or Organization property."),
                            ));
                        }

                        let has_date = json_has_field(&json_val, "datePublished")
                            || json_has_field(&json_val, "dateCreated");
                        if !has_date {
                            let rule = get_rule(RuleId::WarnArticleMissingDatePublished);
                            issues.push(rule.to_finding(
                                url,
                                Some(
                                    "Article schema is missing the 'datePublished' ISO timestamp.",
                                ),
                            ));
                        }
                    }
                }
            }
        }
        PageArchetype::Contact => {
            let has_org_or_local = page.schemas.iter().any(|s| {
                s.is_valid_json
                    && (s.schema_type.eq_ignore_ascii_case("organization")
                        || s.schema_type.eq_ignore_ascii_case("localbusiness")
                        || s.schema_type.eq_ignore_ascii_case("contactpage")
                        || s.schema_type.eq_ignore_ascii_case("place"))
            });

            if !has_org_or_local {
                let rule = get_rule(RuleId::WarnOrgMissingLocalSchema);
                issues.push(rule.to_finding(
                    url,
                    Some("Contact page lacks Organization, LocalBusiness, or ContactPoint structured data."),
                ));
            }
        }
        _ => {}
    }
}

/// Recursively checks whether a JSON value or any contained `@graph` element has a named field.
fn json_has_field(val: &Value, field: &str) -> bool {
    match val {
        Value::Object(map) => {
            if map.contains_key(field) {
                return true;
            }
            if let Some(Value::Array(graph)) = map.get("@graph") {
                for item in graph {
                    if json_has_field(item, field) {
                        return true;
                    }
                }
            }
            false
        }
        Value::Array(arr) => arr.iter().any(|item| json_has_field(item, field)),
        _ => false,
    }
}

/// Recursively checks whether a JSON value has a child field inside a specified parent object.
fn json_has_nested_field(val: &Value, parent: &str, child: &str) -> bool {
    match val {
        Value::Object(map) => {
            if let Some(parent_val) = map.get(parent) {
                if json_has_field(parent_val, child) {
                    return true;
                }
            }
            if let Some(Value::Array(graph)) = map.get("@graph") {
                for item in graph {
                    if json_has_nested_field(item, parent, child) {
                        return true;
                    }
                }
            }
            false
        }
        Value::Array(arr) => arr
            .iter()
            .any(|item| json_has_nested_field(item, parent, child)),
        _ => false,
    }
}
