//! Data the page publishes about itself: JSON-LD, Microdata, RDFa and OpenGraph, plus a
//! mapping from schema.org paths to common field names.

use crate::extract::clean::node_name;
use crate::extract::fields::schema::{compact, FieldSpec};
use crate::extract::fields::Source;
use dom_query::{Document, NodeRef};
use serde_json::{Map, Value};

/// One structured item and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Origin.
    pub source: Source,
    /// The item as a JSON object.
    pub value: Value,
}

/// Collects every structured item on the (uncleaned) page.
pub fn collect(dom: &Document) -> Vec<Item> {
    let mut items = Vec::new();
    for script in dom.select(r#"script[type="application/ld+json"]"#).nodes() {
        let text = script.text();
        if let Ok(value) = serde_json::from_str::<Value>(text.trim()) {
            flatten_json_ld(value, &mut items);
        }
    }
    for node in dom.select("[itemscope]").nodes() {
        if node.attr("itemprop").is_none() {
            items.push(Item {
                source: Source::Microdata,
                value: microdata_item(node),
            });
        }
    }
    for node in dom.select("[typeof]").nodes() {
        if node.attr("property").is_none() {
            items.push(Item {
                source: Source::Rdfa,
                value: rdfa_item(node),
            });
        }
    }
    if let Some(og) = open_graph(dom) {
        items.push(og);
    }
    items
}

fn flatten_json_ld(value: Value, out: &mut Vec<Item>) {
    match value {
        Value::Array(values) => values.into_iter().for_each(|v| flatten_json_ld(v, out)),
        Value::Object(mut map) => {
            if let Some(graph) = map.remove("@graph") {
                flatten_json_ld(graph, out);
            }
            if !map.is_empty() {
                out.push(Item {
                    source: Source::JsonLd,
                    value: Value::Object(map),
                });
            }
        }
        _ => {}
    }
}

fn last_segment(iri: &str) -> String {
    iri.rsplit(['/', '#', ':'])
        .next()
        .unwrap_or(iri)
        .to_string()
}

fn insert(map: &mut Map<String, Value>, key: String, value: Value) {
    match map.get_mut(&key) {
        Some(Value::Array(existing)) => existing.push(value),
        Some(existing) => {
            let first = existing.take();
            *existing = Value::Array(vec![first, value]);
        }
        None => {
            map.insert(key, value);
        }
    }
}

/// Value of a Microdata / RDFa property element.
fn property_value(node: &NodeRef) -> Value {
    if let Some(content) = node.attr("content") {
        return Value::String(content.to_string());
    }
    let attr = match node_name(node).as_str() {
        "a" | "link" | "area" => "href",
        "img" | "audio" | "video" | "source" | "iframe" | "embed" => "src",
        "time" => "datetime",
        "data" | "meter" => "value",
        "object" => "data",
        _ => "",
    };
    if !attr.is_empty() {
        if let Some(v) = node.attr(attr) {
            return Value::String(v.to_string());
        }
    }
    Value::String(node.text().split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Nearest ancestor (not the node itself) carrying `attr`.
fn scope_of<'a>(node: &NodeRef<'a>, attr: &str) -> Option<NodeRef<'a>> {
    let mut current = node.parent();
    while let Some(n) = current {
        if n.is_element() && n.has_attr(attr) {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

fn microdata_item(scope: &NodeRef) -> Value {
    let mut map = Map::new();
    if let Some(t) = scope.attr("itemtype") {
        map.insert("@type".into(), Value::String(last_segment(t.trim())));
    }
    for node in scope.descendants_it().filter(|n| n.is_element()) {
        let Some(props) = node.attr("itemprop") else {
            continue;
        };
        if scope_of(&node, "itemscope").map(|s| s.id) != Some(scope.id) {
            continue;
        }
        let value = if node.has_attr("itemscope") {
            microdata_item(&node)
        } else {
            property_value(&node)
        };
        for prop in props.split_whitespace() {
            insert(&mut map, last_segment(prop), value.clone());
        }
    }
    Value::Object(map)
}

fn rdfa_item(scope: &NodeRef) -> Value {
    let mut map = Map::new();
    if let Some(t) = scope.attr("typeof") {
        map.insert("@type".into(), Value::String(last_segment(t.trim())));
    }
    for node in scope.descendants_it().filter(|n| n.is_element()) {
        let Some(props) = node.attr("property") else {
            continue;
        };
        if scope_of(&node, "typeof").map(|s| s.id) != Some(scope.id) {
            continue;
        }
        let value = if node.has_attr("typeof") {
            rdfa_item(&node)
        } else {
            property_value(&node)
        };
        for prop in props.split_whitespace() {
            insert(&mut map, last_segment(prop), value.clone());
        }
    }
    Value::Object(map)
}

fn open_graph(dom: &Document) -> Option<Item> {
    const MAP: &[(&str, &str)] = &[
        ("og:title", "name"),
        ("og:description", "description"),
        ("og:image", "image"),
        ("og:url", "url"),
        ("og:site_name", "site_name"),
        ("og:type", "@type"),
        ("product:price:amount", "price"),
        ("og:price:amount", "price"),
        ("product:price:currency", "priceCurrency"),
        ("og:price:currency", "priceCurrency"),
        ("product:brand", "brand"),
        ("article:published_time", "datePublished"),
        ("article:modified_time", "dateModified"),
        ("article:author", "author"),
    ];
    let mut map = Map::new();
    for meta in dom.select("meta[property], meta[name]").nodes() {
        let key = meta
            .attr("property")
            .or_else(|| meta.attr("name"))
            .map(|k| k.to_ascii_lowercase());
        let (Some(key), Some(content)) = (key, meta.attr("content")) else {
            continue;
        };
        if let Some((_, field)) = MAP.iter().find(|(k, _)| *k == key) {
            map.entry(field.to_string())
                .or_insert_with(|| Value::String(content.to_string()));
        }
    }
    (!map.is_empty()).then_some(Item {
        source: Source::OpenGraph,
        value: Value::Object(map),
    })
}

/// schema.org paths for common field names, tried before a generic key search.
const PATHS: &[(&str, &[&str])] = &[
    ("name", &["name", "headline", "title"]),
    ("title", &["headline", "name", "title"]),
    (
        "price",
        &[
            "offers.price",
            "offers.lowPrice",
            "price",
            "offers.priceSpecification.price",
        ],
    ),
    (
        "currency",
        &[
            "offers.priceCurrency",
            "priceCurrency",
            "offers.priceSpecification.priceCurrency",
        ],
    ),
    (
        "rating",
        &[
            "aggregateRating.ratingValue",
            "ratingValue",
            "reviewRating.ratingValue",
        ],
    ),
    (
        "reviewcount",
        &[
            "aggregateRating.reviewCount",
            "aggregateRating.ratingCount",
            "reviewCount",
        ],
    ),
    (
        "reviews",
        &["aggregateRating.reviewCount", "aggregateRating.ratingCount"],
    ),
    ("brand", &["brand.name", "brand", "manufacturer.name"]),
    (
        "gtin",
        &[
            "gtin13",
            "gtin",
            "gtin12",
            "gtin14",
            "gtin8",
            "offers.gtin13",
        ],
    ),
    ("availability", &["offers.availability", "availability"]),
    ("instock", &["offers.availability"]),
    ("author", &["author.name", "author", "creator.name"]),
    (
        "date",
        &["datePublished", "dateCreated", "uploadDate", "startDate"],
    ),
    ("published", &["datePublished", "dateCreated"]),
    ("updated", &["dateModified"]),
    (
        "image",
        &["image.url", "image.contentUrl", "image", "thumbnailUrl"],
    ),
    ("description", &["description"]),
    ("url", &["url", "@id"]),
    ("sku", &["sku", "offers.sku", "productID"]),
    ("mpn", &["mpn"]),
    ("isbn", &["isbn"]),
    ("phone", &["telephone", "contactPoint.telephone"]),
    ("email", &["email", "contactPoint.email"]),
    ("address", &["address.streetAddress", "address"]),
    (
        "company",
        &[
            "hiringOrganization.name",
            "organization.name",
            "publisher.name",
        ],
    ),
    (
        "salary",
        &[
            "baseSalary.value.value",
            "baseSalary.value.minValue",
            "baseSalary",
        ],
    ),
    (
        "location",
        &[
            "jobLocation.address.addressLocality",
            "location.name",
            "address.addressLocality",
        ],
    ),
];

/// Follows a dotted path (`offers.price`); arrays without an index use their first element.
pub fn resolve_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        if let Value::Array(items) = current {
            current = match segment.parse::<usize>() {
                Ok(i) => {
                    current = items.get(i)?;
                    continue;
                }
                Err(_) => items.first()?,
            };
        }
        current = current.as_object()?.get(segment)?;
    }
    match current {
        Value::Null => None,
        Value::String(s) if s.trim().is_empty() => None,
        other => Some(other),
    }
}

/// Finds a field's raw value in a structured object: known schema.org paths first, then a
/// key search (name, synonyms, hints) up to `max_depth` levels deep, shallowest first.
pub fn lookup<'a>(item: &'a Value, field: &FieldSpec, max_depth: usize) -> Option<&'a Value> {
    for alias in field.aliases() {
        if let Some((_, paths)) = PATHS.iter().find(|(name, _)| *name == alias) {
            if let Some(v) = paths.iter().find_map(|p| resolve_path(item, p)) {
                return Some(v);
            }
        }
    }
    let mut level: Vec<&Value> = vec![item];
    for _ in 0..=max_depth {
        let mut next = Vec::new();
        for value in level {
            let Some(map) = value.as_object() else {
                if let Value::Array(items) = value {
                    next.extend(items.iter().take(3));
                }
                continue;
            };
            for (key, v) in map {
                if !key.starts_with('@') && field.matches_label(key) && !v.is_null() {
                    return Some(v);
                }
            }
            next.extend(map.values().filter(|v| v.is_object() || v.is_array()));
        }
        if next.is_empty() {
            break;
        }
        level = next;
    }
    None
}

/// The `@type` of an item, lowercased.
pub fn item_type(item: &Value) -> String {
    match item.get("@type") {
        Some(Value::String(t)) => compact(t),
        Some(Value::Array(types)) => types
            .first()
            .and_then(Value::as_str)
            .map(compact)
            .unwrap_or_default(),
        _ => String::new(),
    }
}
