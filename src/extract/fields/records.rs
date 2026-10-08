//! Repeated records (product cards, search results, table rows) and key-value pairs (spec
//! tables, definition lists, `Label: value` lines) found in the page structure.

use crate::extract::clean::node_name;
use crate::extract::convert::css_path;
use crate::extract::fields::recognize::{find_price, parse_date, parse_rating};
use crate::extract::fields::schema::{coerce, name_tokens, FieldSpec, ValueKind};
use dom_query::{Document, NodeRef, Selection};
use hashbrown::HashMap;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

/// Minimum siblings sharing a signature to count as records.
const MIN_RECORDS: usize = 3;

const INLINE_TAGS: &[&str] = &[
    "a", "abbr", "b", "bdi", "cite", "code", "data", "del", "dfn", "em", "i", "img", "ins", "kbd",
    "label", "mark", "q", "s", "small", "span", "strong", "sub", "sup", "time", "u", "var",
];

/// Text of an element with spaces between block-level children.
pub fn visible_text(node: &NodeRef) -> String {
    fn walk(node: &NodeRef, out: &mut String) {
        for child in node.children() {
            if child.is_text() {
                out.push_str(&child.text());
            } else if child.is_element() {
                let name = node_name(&child);
                let block = !INLINE_TAGS.contains(&name.as_str());
                if block || name == "br" {
                    out.push(' ');
                }
                walk(&child, out);
                if block {
                    out.push(' ');
                }
            }
        }
    }
    let mut out = String::new();
    walk(node, &mut out);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn signature(node: &NodeRef) -> String {
    let mut classes: Vec<String> = node
        .class()
        .map(|c| c.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    classes.sort();
    format!("{}.{}", node_name(node), classes.join("."))
}

/// Finds the largest group of sibling elements that share a tag-and-class signature and look
/// like records (several descendants each, some text).
pub fn find_records<'a>(dom: &'a Document) -> Vec<NodeRef<'a>> {
    let mut best: (f64, Vec<NodeRef<'a>>) = (0.0, Vec::new());
    for parent in dom.root().descendants_it().filter(|n| n.is_element()) {
        let children = parent.element_children();
        if children.len() < MIN_RECORDS {
            continue;
        }
        let mut groups: HashMap<String, Vec<NodeRef<'a>>> = HashMap::new();
        for child in children {
            groups.entry(signature(&child)).or_default().push(child);
        }
        for (_, members) in groups {
            if members.len() < MIN_RECORDS {
                continue;
            }
            let texts: Vec<usize> = members.iter().map(|m| visible_text(m).len()).collect();
            if texts.contains(&0) {
                continue;
            }
            let depth: f64 = members
                .iter()
                .map(|m| {
                    m.descendants_it()
                        .filter(|d| d.is_element())
                        .count()
                        .min(20) as f64
                })
                .sum::<f64>()
                / members.len() as f64;
            let avg_text = texts.iter().sum::<usize>() as f64 / members.len() as f64;
            let score = members.len() as f64 * (1.0 + depth) * (avg_text + 1.0).ln();
            if score > best.0 {
                best = (score, members);
            }
        }
    }
    best.1
}

fn class_tokens(node: &NodeRef) -> Vec<String> {
    let mut tokens: Vec<String> = node
        .class()
        .map(|c| c.split_whitespace().flat_map(name_tokens).collect())
        .unwrap_or_default();
    for attr in ["itemprop", "property", "data-field", "data-testid", "name"] {
        if let Some(v) = node.attr(attr) {
            tokens.extend(name_tokens(&v));
        }
    }
    tokens
}

/// Raw value of a record field, with the element it came from.
fn record_field<'a>(record: &NodeRef<'a>, field: &FieldSpec) -> Option<(Value, NodeRef<'a>, f64)> {
    let elements: Vec<NodeRef<'a>> = record.descendants_it().filter(|n| n.is_element()).collect();
    let aliases = field.aliases();
    let named = elements.iter().find(|el| {
        let tokens = class_tokens(el);
        tokens.iter().any(|t| aliases.contains(t))
    });
    if let Some(el) = named {
        if let Some(v) = element_value(el, field.kind) {
            return Some((v, *el, 0.8));
        }
    }
    let by_kind = match field.kind {
        ValueKind::Url => std::iter::once(*record)
            .chain(elements.iter().copied())
            .find(|el| node_name(el) == "a" && el.has_attr("href")),
        ValueKind::Image => elements
            .iter()
            .copied()
            .find(|el| node_name(el) == "img" && (el.has_attr("src") || el.has_attr("data-src"))),
        ValueKind::Price => leaves(&elements).find(|el| find_price(&visible_text(el)).is_some()),
        ValueKind::Rating => leaves(&elements).find(|el| {
            class_tokens(el)
                .iter()
                .any(|t| t.starts_with("rat") || t.starts_with("star") || t == "score")
                && parse_rating(&visible_text(el)).is_some()
        }),
        ValueKind::Date => elements
            .iter()
            .copied()
            .find(|el| node_name(el) == "time")
            .or_else(|| leaves(&elements).find(|el| parse_date(&visible_text(el)).is_some())),
        ValueKind::Text if aliases.iter().any(|a| a == "name" || a == "title") => elements
            .iter()
            .copied()
            .find(|el| {
                matches!(
                    node_name(el).as_str(),
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                )
            })
            .or_else(|| {
                elements
                    .iter()
                    .copied()
                    .find(|el| node_name(el) == "a" && !visible_text(el).is_empty())
            }),
        _ => None,
    }?;
    element_value(&by_kind, field.kind).map(|v| (v, by_kind, 0.65))
}

fn leaves<'a, 'b>(elements: &'b [NodeRef<'a>]) -> impl Iterator<Item = NodeRef<'a>> + 'b {
    elements
        .iter()
        .copied()
        .filter(|el| el.element_children().is_empty())
}

/// The value an element carries for a field kind: link targets for URLs, image sources for
/// images, `datetime` / `content` when present, text otherwise.
pub fn element_value(el: &NodeRef, kind: ValueKind) -> Option<Value> {
    let name = node_name(el);
    let attr = |a: &str| {
        el.attr(a)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let value = match kind {
        ValueKind::Url => {
            if name == "a" {
                attr("href")
            } else {
                Selection::from(*el)
                    .select("a[href]")
                    .nodes()
                    .first()
                    .and_then(|a| a.attr("href").map(|v| v.to_string()))
            }
        }
        ValueKind::Image => {
            if name == "img" {
                attr("src").or_else(|| attr("data-src"))
            } else {
                Selection::from(*el)
                    .select("img")
                    .nodes()
                    .first()
                    .and_then(|i| i.attr("src").or_else(|| i.attr("data-src")))
                    .map(|v| v.to_string())
            }
        }
        ValueKind::Date => attr("datetime")
            .or_else(|| attr("content"))
            .or_else(|| Some(visible_text(el))),
        _ => attr("content").or_else(|| Some(visible_text(el))),
    }?;
    (!value.is_empty()).then_some(Value::String(value))
}

/// One record: field name to (coerced value, confidence, selector).
pub type Record = Vec<(String, Value, f64, String)>;

/// Extracts the requested fields from each record element.
pub fn extract_records(
    records: &[NodeRef],
    fields: &[FieldSpec],
    base: Option<&url::Url>,
) -> Vec<Record> {
    records
        .iter()
        .map(|record| {
            fields
                .iter()
                .filter_map(|field| {
                    let (raw, el, confidence) = match &field.selector {
                        Some(sel) => crate::extract::fields::rules::apply_selector(record, sel)
                            .ok()
                            .flatten()
                            .map(|(v, el)| (v, el, 0.9))?,
                        None => record_field(record, field)?,
                    };
                    let value = coerce(field, &raw, base)?;
                    Some((field.name.clone(), value, confidence, css_path(&el)))
                })
                .collect()
        })
        .collect()
}

/// A label and its value found in the page structure.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelValue {
    /// Label text (`Weight`).
    pub label: String,
    /// Value text (`1.2 kg`).
    pub value: String,
    /// Selector of the value element.
    pub selector: String,
}

/// Collects key-value pairs from two-cell table rows, definition lists and `Label: value`
/// lines.
pub fn labels(dom: &Document) -> Vec<LabelValue> {
    static LINE: OnceLock<Option<Regex>> = OnceLock::new();
    let line = LINE
        .get_or_init(|| Regex::new(r"^([^:]{1,40}?)\s*:\s*(\S.{0,200})$").ok())
        .as_ref();

    let mut out = Vec::new();
    for row in dom.select("tr").nodes() {
        let cells: Vec<NodeRef> = row
            .element_children()
            .into_iter()
            .filter(|c| matches!(node_name(c).as_str(), "th" | "td"))
            .collect();
        if let [label, value] = cells.as_slice() {
            let (l, v) = (visible_text(label), visible_text(value));
            if !l.is_empty() && !v.is_empty() {
                out.push(LabelValue {
                    label: l,
                    value: v,
                    selector: css_path(value),
                });
            }
        }
    }
    for dl in dom.select("dl").nodes() {
        let mut label: Option<String> = None;
        for child in dl.element_children() {
            match node_name(&child).as_str() {
                "dt" => label = Some(visible_text(&child)),
                "dd" => {
                    if let Some(l) = label.take() {
                        let v = visible_text(&child);
                        if !l.is_empty() && !v.is_empty() {
                            out.push(LabelValue {
                                label: l,
                                value: v,
                                selector: css_path(&child),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let Some(line) = line else {
        return out;
    };
    for el in dom.select("p, li, div, span, dd, td").nodes() {
        if el
            .element_children()
            .iter()
            .any(|c| !INLINE_TAGS.contains(&node_name(c).as_str()))
        {
            continue;
        }
        let text = visible_text(el);
        if let Some(c) = line.captures(&text) {
            let (Some(l), Some(v)) = (c.get(1), c.get(2)) else {
                continue;
            };
            let label = l.as_str().trim();
            if label.split_whitespace().count() > 5
                || !label.chars().any(char::is_alphabetic)
                || v.as_str().starts_with("//")
            {
                continue;
            }
            out.push(LabelValue {
                label: label.to_string(),
                value: v.as_str().trim().to_string(),
                selector: css_path(el),
            });
        }
    }
    out
}
