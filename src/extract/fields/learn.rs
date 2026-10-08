//! Template learning: on pages that publish structured data, find the element holding each
//! value, build a short stable selector for it, and reuse that selector on pages of the same
//! template that publish nothing.
//!
//! Pages belong to the same template when they share a host and a URL pattern (the last path
//! segment and any segment with a digit become `*`). Rules carry a support count: confirming a
//! rule on another page with structured data raises it; a disagreement replaces the rule.

use crate::error::SeoResult;
use crate::extract::clean::node_name;
use crate::extract::convert::css_path;
use crate::extract::fields::records::{element_value, visible_text};
use crate::extract::fields::rules::apply_selector;
use crate::extract::fields::schema::{coerce, values_agree, FieldSpec, ValueKind};
use crate::storage::documents::{rules_for, upsert_rule, StoredRule};
use dom_query::{Document, NodeRef};
use hashbrown::{HashMap, HashSet};
use rusqlite::Connection;
use serde_json::Value;
use std::collections::BTreeMap;

/// Text longer than this is never a single field's element.
const MAX_FIELD_TEXT: usize = 300;

/// URL pattern used to group pages into templates: `/packs/trailhead-40` → `/packs/*`.
pub fn template_id(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return "/".to_string();
    };
    let segments: Vec<&str> = parsed
        .path_segments()
        .map(|s| s.filter(|seg| !seg.is_empty()).collect())
        .unwrap_or_default();
    if segments.is_empty() {
        return "/".to_string();
    }
    let last = segments.len() - 1;
    let pattern: Vec<String> = segments
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            if (i == last && segments.len() > 1) || seg.chars().any(|c| c.is_ascii_digit()) {
                "*".to_string()
            } else {
                seg.to_ascii_lowercase()
            }
        })
        .collect();
    format!("/{}", pattern.join("/"))
}

/// Learned rules in memory, keyed by host and template, with optional SQLite persistence.
#[derive(Debug, Clone, Default)]
pub struct RuleBook {
    rules: HashMap<(String, String), BTreeMap<String, StoredRule>>,
    loaded: HashSet<(String, String)>,
}

impl RuleBook {
    /// The rule for one field.
    pub fn get(&self, host: &str, template: &str, field: &str) -> Option<&StoredRule> {
        self.rules
            .get(&(host.to_string(), template.to_string()))
            .and_then(|r| r.get(field))
    }

    /// Adds or replaces a rule.
    pub fn put(&mut self, rule: StoredRule) {
        self.rules
            .entry((rule.host.clone(), rule.template_id.clone()))
            .or_default()
            .insert(rule.field.clone(), rule);
    }

    /// Every rule.
    pub fn all(&self) -> impl Iterator<Item = &StoredRule> {
        self.rules.values().flat_map(|r| r.values())
    }

    /// Whether rules for this host and template were already loaded from storage.
    pub fn is_loaded(&self, host: &str, template: &str) -> bool {
        self.loaded
            .contains(&(host.to_string(), template.to_string()))
    }

    /// Loads stored rules for a host and template (in-memory rules win).
    ///
    /// # Errors
    ///
    /// Returns storage errors.
    pub fn load(&mut self, conn: &Connection, host: &str, template: &str) -> SeoResult<()> {
        for rule in rules_for(conn, host, template)? {
            if self.get(host, template, &rule.field).is_none() {
                self.put(rule);
            }
        }
        self.loaded.insert((host.to_string(), template.to_string()));
        Ok(())
    }

    /// Writes every rule to storage.
    ///
    /// # Errors
    ///
    /// Returns storage errors.
    pub fn save(&self, conn: &Connection) -> SeoResult<()> {
        for rule in self.all() {
            upsert_rule(conn, rule)?;
        }
        Ok(())
    }
}

/// Applies a stored rule and coerces its value for `field`.
pub fn apply_rule(
    dom: &Document,
    rule: &StoredRule,
    field: &FieldSpec,
    base: Option<&url::Url>,
) -> Option<(Value, String)> {
    let root = dom.root();
    let (raw, _) = apply_selector(&root, &rule.selector).ok().flatten()?;
    let raw = if rule.value_type == "labelled" {
        let text = raw.as_str()?;
        Value::String(text.split_once(':')?.1.trim().to_string())
    } else {
        raw
    };
    coerce(field, &raw, base).map(|v| (v, rule.selector.clone()))
}

/// How an element's content relates to the value it was matched with.
enum Shape {
    Text,
    Labelled,
    Attr(&'static str),
}

/// Finds the deepest element whose content yields `value` for `field`.
fn locate<'a>(
    dom: &'a Document,
    field: &FieldSpec,
    value: &Value,
    base: Option<&url::Url>,
) -> Option<(NodeRef<'a>, Shape)> {
    let mut matches: Vec<(NodeRef<'a>, Shape)> = Vec::new();
    for el in dom.root().descendants_it().filter(|n| n.is_element()) {
        let name = node_name(&el);
        if matches!(
            name.as_str(),
            "html" | "head" | "body" | "title" | "script" | "style"
        ) {
            continue;
        }
        if matches!(field.kind, ValueKind::Url | ValueKind::Image) {
            let attr = if field.kind == ValueKind::Url {
                "href"
            } else {
                "src"
            };
            if let Some(Value::String(v)) = element_value(&el, field.kind) {
                let own = el.attr(attr).is_some();
                if own
                    && coerce(field, &Value::String(v), base)
                        .is_some_and(|c| values_agree(&c, value))
                {
                    matches.push((el, Shape::Attr(attr)));
                }
            }
            continue;
        }
        let text = visible_text(&el);
        if text.is_empty() || text.len() > MAX_FIELD_TEXT {
            continue;
        }
        let agrees = |raw: &str| {
            coerce(field, &Value::String(raw.to_string()), base)
                .is_some_and(|c| values_agree(&c, value))
        };
        if agrees(&text) {
            matches.push((el, Shape::Text));
        } else if let Some((_, after)) = text.split_once(':') {
            if agrees(after.trim()) {
                matches.push((el, Shape::Labelled));
            }
        }
    }
    // Deepest match: one with no other match inside it.
    let ids: Vec<NodeRef<'a>> = matches.iter().map(|(n, _)| *n).collect();
    let index = matches.iter().position(|(node, _)| {
        !ids.iter()
            .any(|other| other.id != node.id && other.ancestors_it(None).any(|a| a.id == node.id))
    })?;
    Some(matches.swap_remove(index))
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '-')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Shortest selector that matches exactly `node`, preferring itemprop, id, then classes.
pub fn stable_selector(dom: &Document, node: &NodeRef) -> String {
    let tag = node_name(node);
    let classes: Vec<String> = node
        .class()
        .map(|c| {
            c.split_whitespace()
                .filter(|c| is_identifier(c))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let mut candidates: Vec<String> = Vec::new();
    if let Some(prop) = node.attr("itemprop") {
        if is_identifier(&prop) {
            candidates.push(format!("[itemprop=\"{prop}\"]"));
        }
    }
    if let Some(id) = node.attr("id") {
        if is_identifier(&id) && !id.chars().any(|c| c.is_ascii_digit()) {
            candidates.push(format!("#{id}"));
        }
    }
    for class in &classes {
        candidates.push(format!("{tag}.{class}"));
        candidates.push(format!(".{class}"));
    }
    if classes.len() > 1 {
        candidates.push(format!("{tag}.{}", classes.join(".")));
    }
    if let Some(parent) = node.parent() {
        let parent_classes: Vec<String> = parent
            .class()
            .map(|c| {
                c.split_whitespace()
                    .filter(|c| is_identifier(c))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        for pc in &parent_classes {
            candidates.push(format!(".{pc} > {tag}"));
            for class in &classes {
                candidates.push(format!(".{pc} {tag}.{class}"));
            }
        }
    }
    candidates.push(tag.clone());
    for candidate in candidates {
        let selection = dom.select(&candidate);
        let nodes = selection.nodes();
        if nodes.len() == 1 && nodes[0].id == node.id {
            return candidate;
        }
    }
    css_path(node)
}

/// Learns or confirms the rule for one field from a page where `value` came from structured
/// data. Returns the updated rule, or `None` when the value is not visible on the page.
pub fn learn_field(
    dom: &Document,
    book: &RuleBook,
    host: &str,
    template: &str,
    field: &FieldSpec,
    value: &Value,
    base: Option<&url::Url>,
) -> Option<StoredRule> {
    if let Some(existing) = book.get(host, template, &field.name) {
        if let Some((found, _)) = apply_rule(dom, existing, field, base) {
            if values_agree(&found, value) {
                let mut confirmed = existing.clone();
                confirmed.support = confirmed.support.saturating_add(1);
                return Some(confirmed);
            }
        }
    }
    let (node, shape) = locate(dom, field, value, base)?;
    let mut selector = stable_selector(dom, &node);
    let value_type = match shape {
        Shape::Text => field.kind.as_str().to_string(),
        Shape::Labelled => "labelled".to_string(),
        Shape::Attr(attr) => {
            selector = format!("{selector}@{attr}");
            field.kind.as_str().to_string()
        }
    };
    Some(StoredRule {
        host: host.to_string(),
        template_id: template.to_string(),
        field: field.name.clone(),
        selector,
        value_type,
        source: "learned".to_string(),
        support: 1,
    })
}
