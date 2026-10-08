//! CSS extraction rules, supplied by the caller or learned (see [`learn`](super::learn)).
//!
//! ```json
//! {"base": ".product", "fields": {"name": "h2", "url": "a@href",
//!  "price": {"selector": ".price", "type": "price"}}}
//! ```
//! A selector may end in `@attr` to read an attribute instead of the text; `@href` alone reads
//! the base element's own attribute. With `base`, every base match is one record.

use crate::error::{SeoError, SeoResult};
use crate::extract::fields::records::visible_text;
use crate::extract::fields::schema::ValueKind;
use dom_query::{Matcher, NodeRef, Selection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Caller-supplied rules.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RuleSet {
    /// Selector of each record; absent for a single object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// Field name to rule.
    pub fields: BTreeMap<String, FieldRule>,
}

/// One field's rule: a selector string or a full rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldRule {
    /// `"h2"` or `"a@href"`.
    Selector(String),
    /// Selector with a recognizer.
    Full {
        /// CSS selector, optionally with `@attr`.
        selector: String,
        /// Recognizer to apply (overrides the schema's).
        #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
        kind: Option<ValueKind>,
    },
}

impl FieldRule {
    /// The selector.
    pub fn selector(&self) -> &str {
        match self {
            FieldRule::Selector(s) => s,
            FieldRule::Full { selector, .. } => selector,
        }
    }

    /// The recognizer override.
    pub fn kind(&self) -> Option<ValueKind> {
        match self {
            FieldRule::Selector(_) => None,
            FieldRule::Full { kind, .. } => *kind,
        }
    }
}

/// Splits `selector@attr`. An `@` inside brackets (`[href^="@"]`) is part of the selector.
pub fn split_attr(selector: &str) -> (&str, Option<&str>) {
    if let Some((left, right)) = selector.rsplit_once('@') {
        let balanced = left.matches('[').count() == left.matches(']').count();
        if balanced
            && !right.is_empty()
            && right
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':'))
        {
            return (left.trim(), Some(right));
        }
    }
    (selector.trim(), None)
}

fn matcher(selector: &str) -> SeoResult<Matcher> {
    Matcher::new(selector)
        .map_err(|_| SeoError::Config(format!("Invalid CSS selector '{selector}'")))
}

/// Applies a selector relative to `scope` and returns the raw value and the element it came
/// from. `Ok(None)` means nothing matched.
///
/// # Errors
///
/// Returns [`SeoError::Config`] for an invalid selector.
pub fn apply_selector<'a>(
    scope: &NodeRef<'a>,
    selector: &str,
) -> SeoResult<Option<(Value, NodeRef<'a>)>> {
    let (css, attr) = split_attr(selector);
    let element = if css.is_empty() {
        Some(*scope)
    } else {
        let m = matcher(css)?;
        Selection::from(*scope)
            .select_matcher(&m)
            .nodes()
            .first()
            .copied()
    };
    let Some(el) = element else {
        return Ok(None);
    };
    let raw = match attr {
        Some(a) => el.attr(a).map(|v| v.trim().to_string()),
        None => Some(visible_text(&el)),
    };
    Ok(raw
        .filter(|v| !v.is_empty())
        .map(|v| (Value::String(v), el)))
}

/// Applies the base selector and returns each record element.
///
/// # Errors
///
/// Returns [`SeoError::Config`] for an invalid selector.
pub fn base_matches<'a>(root: &NodeRef<'a>, base: &str) -> SeoResult<Vec<NodeRef<'a>>> {
    let m = matcher(base)?;
    Ok(Selection::from(*root).select_matcher(&m).nodes().to_vec())
}
