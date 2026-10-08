//! Firecrawl-compatible request normalisation and response shapes.
//!
//! Requests accept both Firecrawl's spelling (`rawHtml`, `onlyMainContent`, `timeout`,
//! `maxAge` in milliseconds, `{ "type": "click", "selector": … }` actions) and the native
//! one. Formats this server cannot produce (`json`, `summary`, `changeTracking`, …) are
//! ignored rather than rejected so existing Firecrawl clients keep working.

use crate::extract::types::{BrowserAction, OutputFormat, PageDocument};
use serde_json::{json, Map, Value};

/// Maps one Firecrawl or native format name to an output format. `None` for formats this
/// server ignores.
fn format_from_name(name: &str) -> Option<OutputFormat> {
    match name {
        "markdown" => Some(OutputFormat::Markdown),
        "html" => Some(OutputFormat::Html),
        "rawHtml" | "raw_html" => Some(OutputFormat::RawHtml),
        "links" => Some(OutputFormat::Links),
        "screenshot" | "screenshot@fullPage" => Some(OutputFormat::Screenshot),
        "text" => Some(OutputFormat::Text),
        "metadata" => Some(OutputFormat::Metadata),
        // Firecrawl's `json` format is LLM extraction; native typed blocks are `blocks`.
        "blocks" => Some(OutputFormat::Json),
        _ => None,
    }
}

/// Rewrites scrape options in place (top level of a scrape request, or a nested
/// `scrapeOptions` object) into the native spelling.
///
/// # Errors
///
/// Returns a message for actions that cannot be run.
pub(crate) fn normalize_scrape(obj: &mut Map<String, Value>) -> Result<(), String> {
    let mut screenshot_action = false;
    if let Some(actions) = obj.remove("actions") {
        let converted = convert_actions(&actions, &mut screenshot_action)?;
        obj.insert(
            "actions".into(),
            serde_json::to_value(converted).map_err(|e| e.to_string())?,
        );
    }

    if let Some(formats) = obj.remove("formats") {
        let mut out: Vec<OutputFormat> = Vec::new();
        for item in formats.as_array().into_iter().flatten() {
            let name = match item {
                Value::String(s) => Some(s.as_str()),
                Value::Object(o) => o.get("type").and_then(Value::as_str),
                _ => None,
            };
            if let Some(f) = name.and_then(format_from_name) {
                if !out.contains(&f) {
                    out.push(f);
                }
            }
        }
        if screenshot_action && !out.contains(&OutputFormat::Screenshot) {
            out.push(OutputFormat::Screenshot);
        }
        if out.iter().all(|f| *f == OutputFormat::Metadata) {
            out.insert(0, OutputFormat::Markdown);
        }
        if !out.contains(&OutputFormat::Metadata) {
            out.push(OutputFormat::Metadata);
        }
        obj.insert(
            "formats".into(),
            serde_json::to_value(out).map_err(|e| e.to_string())?,
        );
    } else if screenshot_action {
        obj.insert(
            "formats".into(),
            json!(["markdown", "metadata", "screenshot"]),
        );
    }

    if let Some(timeout) = obj.remove("timeout") {
        if timeout.is_number() {
            obj.insert("timeout_ms".into(), timeout);
        }
    }
    // Firecrawl's maxAge is in milliseconds; the native field is seconds.
    if let Some(max_age) = obj.remove("maxAge") {
        if let Some(ms) = max_age.as_u64() {
            obj.insert("max_age_secs".into(), json!(ms / 1000));
        }
    }
    Ok(())
}

/// Normalises the nested `scrapeOptions` / `scrape` object of a crawl, find, extract or
/// interact request.
pub(crate) fn normalize_nested(obj: &mut Map<String, Value>) -> Result<(), String> {
    for key in ["scrapeOptions", "scrape"] {
        if let Some(Value::Object(inner)) = obj.get_mut(key) {
            normalize_scrape(inner)?;
        }
    }
    Ok(())
}

/// Firecrawl `ignoreSitemap` / `sitemapOnly` flags to the native `sitemap` mode.
pub(crate) fn normalize_sitemap(obj: &mut Map<String, Value>) {
    if obj.remove("ignoreSitemap").and_then(|v| v.as_bool()) == Some(true) {
        obj.insert("sitemap".into(), json!("skip"));
    }
    if obj.remove("sitemapOnly").and_then(|v| v.as_bool()) == Some(true) {
        obj.insert("sitemap".into(), json!("only"));
    }
}

/// Converts Firecrawl or native actions. Firecrawl `write` types into the element the
/// previous `click` targeted (or the focused element).
pub(crate) fn convert_actions(
    actions: &Value,
    screenshot: &mut bool,
) -> Result<Vec<BrowserAction>, String> {
    let Some(list) = actions.as_array() else {
        return Err("actions must be an array".to_string());
    };
    let mut out = Vec::new();
    let mut last_target: Option<String> = None;
    for action in list {
        if let Ok(native) = serde_json::from_value::<BrowserAction>(action.clone()) {
            if let BrowserAction::Click { target } = &native {
                last_target = Some(target.clone());
            }
            out.push(native);
            continue;
        }
        let kind = action.get("type").and_then(Value::as_str).unwrap_or("");
        let selector = action
            .get("selector")
            .and_then(Value::as_str)
            .map(str::to_string);
        match kind {
            "click" => {
                let target = selector.ok_or("click needs a selector")?;
                last_target = Some(target.clone());
                out.push(BrowserAction::Click { target });
            }
            "write" => {
                let text = action
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or("write needs text")?;
                out.push(BrowserAction::Type {
                    target: selector
                        .or_else(|| last_target.clone())
                        .unwrap_or_else(|| ":focus".to_string()),
                    text: text.to_string(),
                });
            }
            "wait" => match (selector, action.get("milliseconds").and_then(Value::as_u64)) {
                (Some(selector), _) => out.push(BrowserAction::WaitFor { selector }),
                (None, Some(ms)) => out.push(BrowserAction::Wait { ms }),
                (None, None) => return Err("wait needs milliseconds or a selector".to_string()),
            },
            "screenshot" => *screenshot = true,
            "scrape" => {}
            "executeJavascript" => {
                return Err("executeJavascript actions are not supported".to_string())
            }
            other => return Err(format!("Unknown action type '{other}'")),
        }
    }
    Ok(out)
}

/// `og:site_name` → `ogSiteName`, `article:published_time` → `articlePublishedTime`.
fn camel_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut upper = false;
    for c in key.chars() {
        if c == ':' || c == '_' || c == '-' {
            upper = true;
        } else if upper && !out.is_empty() {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
            upper = false;
        }
    }
    out
}

/// Firecrawl's `metadata` object for a document.
pub(crate) fn metadata_json(doc: &PageDocument) -> Value {
    let meta = &doc.metadata;
    let mut out = Map::new();
    for (key, value) in &meta.open_graph {
        out.insert(camel_key(key), json!(value));
    }
    let mut set = |key: &str, value: &Option<String>| {
        if let Some(v) = value {
            out.insert(key.to_string(), json!(v));
        }
    };
    set("title", &meta.title);
    set("description", &meta.description);
    set("language", &meta.language);
    set("canonical", &meta.canonical_url);
    set("author", &meta.author);
    set("publishedTime", &meta.published);
    set("siteName", &meta.site_name);
    set("contentSignal", &meta.content_signal);
    set("pageType", &meta.page_type);
    set("error", &doc.error);
    set("blockedBy", &doc.blocked_by);
    if !meta.robots.is_empty() {
        out.insert("robots".into(), json!(meta.robots.join(", ")));
    }
    out.insert("sourceURL".into(), json!(doc.url));
    let url = if doc.final_url.is_empty() {
        &doc.url
    } else {
        &doc.final_url
    };
    out.insert("url".into(), json!(url));
    out.insert("statusCode".into(), json!(doc.status_code));
    if !doc.content_type.is_empty() {
        out.insert("contentType".into(), json!(doc.content_type));
    }
    out.insert("pageStatus".into(), json!(doc.status.as_str()));
    if !doc.source.is_empty() {
        out.insert("source".into(), json!(doc.source));
    }
    out.insert("tokens".into(), json!(doc.tokens));
    out.insert(
        "cacheState".into(),
        json!(if doc.from_cache { "hit" } else { "miss" }),
    );
    if let Some(changed) = doc.changed {
        out.insert("changed".into(), json!(changed));
    }
    Value::Object(out)
}

/// A document in Firecrawl's `data` shape, with only the requested formats.
pub(crate) fn document_json(doc: &PageDocument, formats: &[OutputFormat]) -> Value {
    let wants = |f: OutputFormat| formats.contains(&f);
    let mut out = Map::new();
    if wants(OutputFormat::Markdown) {
        out.insert("markdown".into(), json!(doc.markdown));
    }
    if wants(OutputFormat::Html) {
        out.insert("html".into(), json!(doc.html));
    }
    if wants(OutputFormat::RawHtml) {
        out.insert("rawHtml".into(), json!(doc.raw_html));
    }
    if wants(OutputFormat::Links) {
        let links = if doc.outlinks.is_empty() {
            &doc.links
        } else {
            &doc.outlinks
        };
        let urls: Vec<&str> = links.iter().map(|l| l.url.as_str()).collect();
        out.insert("links".into(), json!(urls));
    }
    if wants(OutputFormat::Screenshot) {
        if let Some(png) = &doc.screenshot {
            out.insert(
                "screenshot".into(),
                json!(format!("data:image/png;base64,{png}")),
            );
        }
    }
    if wants(OutputFormat::Text) {
        out.insert("text".into(), json!(doc.text));
    }
    if wants(OutputFormat::Json) {
        out.insert("blocks".into(), json!(doc.blocks));
    }
    out.insert("metadata".into(), metadata_json(doc));
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_camel_cased() {
        assert_eq!(camel_key("og:site_name"), "ogSiteName");
        assert_eq!(camel_key("article:published_time"), "articlePublishedTime");
    }

    #[test]
    fn firecrawl_actions_are_converted() {
        let mut shot = false;
        let actions = json!([
            {"type": "click", "selector": "#q"},
            {"type": "write", "text": "tents"},
            {"type": "press", "key": "Enter"},
            {"type": "wait", "milliseconds": 500},
            {"type": "screenshot"}
        ]);
        let out = convert_actions(&actions, &mut shot).unwrap_or_default();
        assert_eq!(out.len(), 4);
        assert_eq!(
            out[1],
            BrowserAction::Type {
                target: "#q".into(),
                text: "tents".into()
            }
        );
        assert!(shot);
        assert!(convert_actions(&json!([{"type": "executeJavascript"}]), &mut shot).is_err());
    }
}
