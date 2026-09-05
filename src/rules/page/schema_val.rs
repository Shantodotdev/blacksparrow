//! # Structured Data & Schema Validation Rules
//!
//! Evaluates JSON-LD schema syntax correctness and Google Rich Results required fields.

use crate::core::models::IssueFinding;
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};

/// Evaluates structured data records against syntax and validation rules.
pub fn check_schemas(page: &ParsedPage, url: &str, issues: &mut Vec<IssueFinding>) {
    for schema in &page.schemas {
        // 1. JSON-LD Syntax Error
        if !schema.is_valid_json {
            let rule = get_rule(RuleId::ErrSchemaSyntaxError);
            issues.push(rule.to_finding(
                url,
                Some("Invalid JSON syntax in <script type=\"application/ld+json\"> block."),
            ));
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
    }
}
