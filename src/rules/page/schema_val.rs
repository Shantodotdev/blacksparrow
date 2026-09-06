//! # Structured Data & Schema Validation Rules
//!
//! Evaluates JSON-LD schema syntax correctness, Google Rich Results required fields,
//! multiple Product entities, and ISO 8601 date formatting.

use crate::core::models::IssueFinding;
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};
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

/// Evaluates structured data records against syntax and validation rules.
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

        // 2. Missing Required Fields for Rich Results
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
}
