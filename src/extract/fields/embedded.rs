//! Application state embedded in pages (`__NEXT_DATA__`, `__NUXT_DATA__`, JSON script tags,
//! `window.X = {...}` assignments), read as JSON only. No JavaScript is ever run.

use crate::extract::fields::schema::FieldSpec;
use dom_query::Document;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

/// Objects visited per page at most while searching embedded data.
const MAX_NODES: usize = 20_000;

/// Parses every embedded JSON blob on the (uncleaned) page.
pub fn collect(dom: &Document) -> Vec<Value> {
    static ASSIGN: OnceLock<Option<Regex>> = OnceLock::new();
    let assign = ASSIGN
        .get_or_init(|| {
            Regex::new(r"(?:window\.[A-Za-z_$][\w$.]*|(?:var|let|const)\s+[A-Za-z_$][\w$]*)\s*=\s*")
                .ok()
        })
        .as_ref();

    let mut blobs = Vec::new();
    for script in dom.select("script").nodes() {
        let kind = script
            .attr("type")
            .map(|t| t.to_ascii_lowercase())
            .unwrap_or_default();
        if kind.contains("ld+json") {
            continue;
        }
        let text = script.text();
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let is_json = kind.contains("json")
            || matches!(
                script.attr("id").as_deref(),
                Some("__NEXT_DATA__") | Some("__NUXT_DATA__")
            );
        if is_json {
            if let Ok(v) = serde_json::from_str::<Value>(text) {
                blobs.push(v);
            }
            continue;
        }
        let Some(assign) = assign else {
            continue;
        };
        for m in assign.find_iter(text) {
            let rest = &text[m.end()..];
            if !rest.starts_with(['{', '[']) {
                continue;
            }
            let mut stream = serde_json::Deserializer::from_str(rest).into_iter::<Value>();
            if let Some(Ok(v)) = stream.next() {
                blobs.push(v);
            }
        }
    }
    blobs
}

/// The object whose own keys name the most requested fields (at least two, or one when only
/// one field is requested).
pub fn best_object<'a>(blobs: &'a [Value], fields: &[FieldSpec]) -> Option<&'a Value> {
    let needed = fields.len().min(2);
    let mut best: Option<(&Value, usize)> = None;
    let mut stack: Vec<&Value> = blobs.iter().collect();
    let mut visited = 0usize;
    while let Some(value) = stack.pop() {
        visited += 1;
        if visited > MAX_NODES {
            break;
        }
        match value {
            Value::Object(map) => {
                let score = fields
                    .iter()
                    .filter(|f| map.keys().any(|k| f.matches_label(k)))
                    .count();
                if score >= needed && best.is_none_or(|(_, s)| score > s) {
                    best = Some((value, score));
                }
                stack.extend(map.values().filter(|v| v.is_object() || v.is_array()));
            }
            Value::Array(items) => stack.extend(items.iter().take(200)),
            _ => {}
        }
    }
    best.map(|(v, _)| v)
}
