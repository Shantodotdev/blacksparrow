//! # Structured Data & Schema Validation Rules
//!
//! Evaluates JSON-LD schema syntax correctness, Google Rich Results required fields,
//! multiple Product entities, ISO 8601 date formatting, and contextual schema compliance
//! based on detected page intent.

use crate::core::models::{IssueFinding, PageArchetype};
use crate::error::SeoResult;
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Checks if a date string conforms to standard ISO 8601 (YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS...).
pub fn is_valid_iso_date(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.len() < 10 {
        return false;
    }
    let bytes = trimmed.as_bytes();
    if !bytes[0..4].iter().all(|b| b.is_ascii_digit())
        || bytes[4] != b'-'
        || !bytes[5..7].iter().all(|b| b.is_ascii_digit())
        || bytes[7] != b'-'
        || !bytes[8..10].iter().all(|b| b.is_ascii_digit())
    {
        return false;
    }

    let year: u32 = trimmed[0..4].parse().unwrap_or(0);
    let month: u32 = trimmed[5..7].parse().unwrap_or(0);
    let day: u32 = trimmed[8..10].parse().unwrap_or(0);

    if year < 1000 || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return false;
    }

    if trimmed.len() == 10 {
        return true;
    }

    if bytes[10] != b'T' && bytes[10] != b't' && bytes[10] != b' ' {
        return false;
    }

    if trimmed.len() < 19 {
        return false;
    }
    if !bytes[11..13].iter().all(|b| b.is_ascii_digit())
        || bytes[13] != b':'
        || !bytes[14..16].iter().all(|b| b.is_ascii_digit())
        || bytes[16] != b':'
        || !bytes[17..19].iter().all(|b| b.is_ascii_digit())
    {
        return false;
    }

    let hour: u32 = trimmed[11..13].parse().unwrap_or(99);
    let min: u32 = trimmed[14..16].parse().unwrap_or(99);
    let sec: u32 = trimmed[17..19].parse().unwrap_or(99);

    if hour > 23 || min > 59 || sec > 59 {
        return false;
    }

    true
}

const DATE_FIELDS: &[&str] = &[
    "datePublished",
    "dateModified",
    "uploadDate",
    "priceValidUntil",
    "validFrom",
    "validThrough",
    "startDate",
    "endDate",
    "expires",
];

fn inspect_json_date_fields(value: &Value, url: &str, issues: &mut Vec<IssueFinding>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if DATE_FIELDS.iter().any(|&f| f.eq_ignore_ascii_case(k)) {
                    if let Some(date_str) = v.as_str() {
                        if !is_valid_iso_date(date_str) {
                            let rule = get_rule(RuleId::WarnSchemaInvalidDateFormat);
                            let msg = format!(
                                "Structured data date property '{}' has invalid ISO 8601 format: \"{}\".",
                                k, date_str
                            );
                            issues.push(rule.to_finding(url, Some(&msg)));
                        }
                    }
                }
                inspect_json_date_fields(v, url, issues);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                inspect_json_date_fields(item, url, issues);
            }
        }
        _ => {}
    }
}

/// Evaluates structured data records against syntax and validation rules,
/// and audits contextual structured data based on page intent.
pub fn check_schemas(page: &ParsedPage, url: &str, issues: &mut Vec<IssueFinding>) {
    let mut product_count = 0usize;

    for schema in &page.schemas {
        // Count top-level Product schemas
        if schema.schema_type.eq_ignore_ascii_case("product") {
            product_count += 1;
        }

        // 1. JSON-LD Syntax Error
        if !schema.is_valid_json {
            let rule = get_rule(RuleId::ErrSchemaSyntaxError);
            issues.push(rule.to_finding(
                url,
                Some("Invalid JSON syntax in <script type=\"application/ld+json\"> block."),
            ));
            continue;
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

        // 3. Check Date Format in JSON-LD
        if let Ok(json_val) = serde_json::from_str::<Value>(&schema.raw_json) {
            inspect_json_date_fields(&json_val, url, issues);
        }
    }

    // 4. Multiple Product Entities
    if product_count > 1 {
        let rule = get_rule(RuleId::WarnSchemaMultipleProductEntities);
        let msg = format!(
            "Page defines {} top-level Product schema entities, creating ambiguity for Google Rich Results.",
            product_count
        );
        issues.push(rule.to_finding(url, Some(&msg)));
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

/// Result of validating a raw schema block against Google Rich Results guidelines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaValidationOutcome {
    /// Whether the input string parsed successfully as valid JSON.
    pub is_valid_json: bool,
    /// Schema @type extracted from top-level or @graph container.
    pub detected_type: Option<String>,
    /// Whether the schema fulfills all required properties for Google Rich Results.
    pub is_rich_result_eligible: bool,
    /// Missing required fields that completely block Rich Results eligibility.
    pub missing_required_fields: Vec<String>,
    /// Missing recommended fields that enhance SERP appearance.
    pub missing_recommended_fields: Vec<String>,
    /// Descriptive error or guidance message.
    pub error_message: Option<String>,
}

/// Validates a raw JSON-LD snippet or HTML block against Google Rich Results eligibility rules.
pub fn validate_raw_schema(
    raw: &str,
    expected_type: Option<&str>,
) -> SeoResult<SchemaValidationOutcome> {
    let trimmed = raw.trim();
    let json_text = if let Some(start) = trimmed.find("<script") {
        if let Some(content_start) = trimmed[start..].find('>') {
            let rest = &trimmed[start + content_start + 1..];
            if let Some(end) = rest.find("</script>") {
                rest[..end].trim()
            } else {
                trimmed
            }
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    let val: Value = match serde_json::from_str(json_text) {
        Ok(v) => v,
        Err(e) => {
            return Ok(SchemaValidationOutcome {
                is_valid_json: false,
                detected_type: None,
                is_rich_result_eligible: false,
                missing_required_fields: Vec::new(),
                missing_recommended_fields: Vec::new(),
                error_message: Some(format!("Invalid JSON syntax: {e}")),
            });
        }
    };

    let detected_type = if let Some(t) = val.get("@type").and_then(|v| v.as_str()) {
        Some(t.to_string())
    } else if let Some(Value::Array(graph)) = val.get("@graph") {
        graph
            .first()
            .and_then(|item| item.get("@type"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    } else {
        None
    };

    let target = expected_type
        .map(|s| s.to_string())
        .or_else(|| detected_type.clone());
    let mut missing_required = Vec::new();
    let mut missing_recommended = Vec::new();

    if let Some(ref t) = target {
        if t.eq_ignore_ascii_case("product") {
            if !json_has_field(&val, "name") {
                missing_required.push("name".to_string());
            }
            if !json_has_field(&val, "image") {
                missing_recommended.push("image".to_string());
            }
            if !json_has_field(&val, "offers") {
                missing_required.push("offers".to_string());
            }
            if !json_has_field(&val, "aggregateRating") && !json_has_field(&val, "review") {
                missing_recommended.push("aggregateRating".to_string());
            }
        } else if t.eq_ignore_ascii_case("article")
            || t.eq_ignore_ascii_case("newsarticle")
            || t.eq_ignore_ascii_case("blogposting")
        {
            if !json_has_field(&val, "headline") {
                missing_required.push("headline".to_string());
            }
            if !json_has_field(&val, "author") {
                missing_recommended.push("author".to_string());
            }
            if !json_has_field(&val, "datePublished") {
                missing_recommended.push("datePublished".to_string());
            }
            if !json_has_field(&val, "image") {
                missing_recommended.push("image".to_string());
            }
        } else if t.eq_ignore_ascii_case("faqpage") {
            if !json_has_field(&val, "mainEntity") {
                missing_required.push("mainEntity".to_string());
            }
        } else if t.eq_ignore_ascii_case("breadcrumblist")
            && !json_has_field(&val, "itemListElement")
        {
            missing_required.push("itemListElement".to_string());
        }
    }

    let is_eligible = detected_type.is_some() && missing_required.is_empty();
    let err_msg = if !missing_required.is_empty() {
        Some(format!(
            "Missing required Google Rich Result property '{}'",
            missing_required.join("', '")
        ))
    } else {
        None
    };

    Ok(SchemaValidationOutcome {
        is_valid_json: true,
        detected_type,
        is_rich_result_eligible: is_eligible,
        missing_required_fields: missing_required,
        missing_recommended_fields: missing_recommended,
        error_message: err_msg,
    })
}
