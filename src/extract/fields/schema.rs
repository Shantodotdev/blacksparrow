//! The JSON Schema subset `extract` accepts, and value coercion into it.
//!
//! Supported: an object with typed `properties` (string, number, integer, boolean, arrays of
//! scalars, arrays of objects for lists) and `required`, or a top-level array of objects.
//! Per-field hints: `x-selector` (CSS, optionally `selector@attr`), `x-synonyms` (extra label
//! words) and `x-kind` (force a recognizer: price, date, phone, email, url, image, gtin, isbn,
//! rating, quantity, currency). `format: email | uri | date | date-time` is honoured too.

use crate::error::{SeoError, SeoResult};
use crate::extract::fields::recognize::{
    parse_currency, parse_date, parse_email, parse_gtin, parse_isbn, parse_number, parse_phone,
    parse_price, parse_rating, parse_url,
};
use crate::extract::synonyms::expand_term;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// JSON type of a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonType {
    /// `string`.
    String,
    /// `number`.
    Number,
    /// `integer`.
    Integer,
    /// `boolean`.
    Boolean,
    /// `array` of scalars.
    Array,
    /// `array` of objects (a list of records).
    Records,
}

/// Recognizer applied to a field's raw value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    /// Plain text.
    Text,
    /// Price amount (number) or price text.
    Price,
    /// ISO currency code.
    Currency,
    /// `YYYY-MM-DD`.
    Date,
    /// Phone digits.
    Phone,
    /// Email address.
    Email,
    /// Absolute link URL.
    Url,
    /// Absolute image URL.
    Image,
    /// GTIN with a valid check digit.
    Gtin,
    /// ISBN with a valid check digit.
    Isbn,
    /// Rating value.
    Rating,
    /// Any number.
    Number,
    /// Whole number.
    Integer,
    /// Number with a unit, kept as text.
    Quantity,
    /// Yes / no.
    Boolean,
}

impl ValueKind {
    /// Stable name, as stored in learned rules.
    pub fn as_str(self) -> &'static str {
        match self {
            ValueKind::Text => "text",
            ValueKind::Price => "price",
            ValueKind::Currency => "currency",
            ValueKind::Date => "date",
            ValueKind::Phone => "phone",
            ValueKind::Email => "email",
            ValueKind::Url => "url",
            ValueKind::Image => "image",
            ValueKind::Gtin => "gtin",
            ValueKind::Isbn => "isbn",
            ValueKind::Rating => "rating",
            ValueKind::Number => "number",
            ValueKind::Integer => "integer",
            ValueKind::Quantity => "quantity",
            ValueKind::Boolean => "boolean",
        }
    }

    /// Parses a stored name.
    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(Value::String(s.to_string())).ok()
    }
}

/// One requested field.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    /// Property name.
    pub name: String,
    /// JSON type.
    pub json_type: JsonType,
    /// Recognizer.
    pub kind: ValueKind,
    /// Caller-supplied selector hint.
    pub selector: Option<String>,
    /// Extra label words.
    pub synonyms: Vec<String>,
    /// Listed in `required`.
    pub required: bool,
    /// Fields of each record (for [`JsonType::Records`]).
    pub items: Vec<FieldSpec>,
}

impl FieldSpec {
    /// The field name lowercased with separators removed (`review_count` → `reviewcount`).
    pub fn compact(&self) -> String {
        compact(&self.name)
    }

    /// Words a label or key may use for this field: the name, its synonyms and hints.
    pub fn aliases(&self) -> Vec<String> {
        let mut aliases = vec![self.compact()];
        aliases.extend(expand_term(&self.compact()).iter().map(|s| compact(s)));
        let tokens = name_tokens(&self.name);
        if tokens.len() > 1 {
            if let Some(last) = tokens.last() {
                if matches!(last.as_str(), "name" | "title" | "price" | "date" | "url") {
                    aliases.push(last.clone());
                }
            }
        }
        aliases.extend(self.synonyms.iter().map(|s| compact(s)));
        aliases.dedup();
        aliases
    }

    /// Whether a key or label names this field.
    pub fn matches_label(&self, label: &str) -> bool {
        let label = compact(label);
        !label.is_empty() && self.aliases().contains(&label)
    }
}

/// Lowercase alphanumerics only.
pub fn compact(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Splits `reviewCount`, `review_count` or `review-count` into lowercase words.
pub fn name_tokens(name: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if !c.is_alphanumeric() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        current.extend(c.to_lowercase());
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Top-level shape of a schema.
#[derive(Debug, Clone, PartialEq)]
pub enum SchemaShape {
    /// One object.
    Object(Vec<FieldSpec>),
    /// A list of records.
    List(Vec<FieldSpec>),
}

/// Parses a schema.
///
/// # Errors
///
/// Returns [`SeoError::Config`] when the schema is not an object or an array of objects.
pub fn parse_schema(schema: &Value) -> SeoResult<SchemaShape> {
    match schema.get("type").and_then(Value::as_str) {
        Some("array") => {
            let items = schema
                .get("items")
                .ok_or_else(|| bad("array schema without items"))?;
            Ok(SchemaShape::List(object_fields(items)?))
        }
        Some("object") | None if schema.get("properties").is_some() => {
            Ok(SchemaShape::Object(object_fields(schema)?))
        }
        _ => Err(bad(
            "schema must be an object with properties or an array of objects",
        )),
    }
}

fn bad(msg: &str) -> SeoError {
    SeoError::Config(format!("Invalid extract schema: {msg}"))
}

fn object_fields(schema: &Value) -> SeoResult<Vec<FieldSpec>> {
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| bad("object schema without properties"))?;
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    properties
        .iter()
        .map(|(name, prop)| field_spec(name, prop, required.contains(&name.as_str())))
        .collect()
}

fn field_spec(name: &str, prop: &Value, required: bool) -> SeoResult<FieldSpec> {
    let type_name = match prop.get("type") {
        Some(Value::String(t)) => t.as_str(),
        // ["string", "null"] style unions: take the first non-null type.
        Some(Value::Array(types)) => types
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null")
            .unwrap_or("string"),
        _ => "string",
    };
    let mut items = Vec::new();
    let json_type = match type_name {
        "number" => JsonType::Number,
        "integer" => JsonType::Integer,
        "boolean" => JsonType::Boolean,
        "array" => {
            let item = prop.get("items").cloned().unwrap_or(Value::Null);
            if item.get("properties").is_some() {
                items = object_fields(&item)?;
                JsonType::Records
            } else {
                JsonType::Array
            }
        }
        "object" => {
            return Err(bad(&format!(
                "nested object '{name}' is not supported; flatten it"
            )))
        }
        _ => JsonType::String,
    };
    let format = prop.get("format").and_then(Value::as_str);
    let explicit = prop
        .get("x-kind")
        .and_then(Value::as_str)
        .and_then(ValueKind::parse);
    let kind = explicit.unwrap_or_else(|| infer_kind(name, json_type, format));
    Ok(FieldSpec {
        name: name.to_string(),
        json_type,
        kind,
        selector: prop
            .get("x-selector")
            .and_then(Value::as_str)
            .map(str::to_string),
        synonyms: prop
            .get("x-synonyms")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        required,
        items,
    })
}

fn infer_kind(name: &str, json_type: JsonType, format: Option<&str>) -> ValueKind {
    match format {
        Some("email") => return ValueKind::Email,
        Some("uri") | Some("url") | Some("iri") => return ValueKind::Url,
        Some("date") | Some("date-time") => return ValueKind::Date,
        _ => {}
    }
    let tokens = name_tokens(name);
    let has = |words: &[&str]| tokens.iter().any(|t| words.contains(&t.as_str()));
    let numeric = matches!(json_type, JsonType::Number | JsonType::Integer);
    if has(&["currency"]) {
        ValueKind::Currency
    } else if has(&["price", "cost", "amount", "fee", "salary"]) {
        ValueKind::Price
    } else if has(&[
        "date",
        "published",
        "updated",
        "modified",
        "posted",
        "created",
    ]) {
        ValueKind::Date
    } else if has(&["phone", "telephone", "tel", "mobile"]) {
        ValueKind::Phone
    } else if has(&["email"]) {
        ValueKind::Email
    } else if has(&["image", "photo", "thumbnail", "picture", "logo"]) {
        ValueKind::Image
    } else if has(&["url", "link", "href", "website"]) {
        ValueKind::Url
    } else if has(&["gtin", "ean", "upc", "barcode"]) {
        ValueKind::Gtin
    } else if has(&["isbn"]) {
        ValueKind::Isbn
    } else if has(&["rating", "stars"]) && json_type != JsonType::Integer {
        ValueKind::Rating
    } else {
        match json_type {
            JsonType::Number => ValueKind::Number,
            JsonType::Integer => ValueKind::Integer,
            JsonType::Boolean => ValueKind::Boolean,
            _ if numeric => ValueKind::Number,
            _ => ValueKind::Text,
        }
    }
}

/// Coerces a raw value into the field's type, applying its recognizer. `None` means the value
/// does not fit (for example a GTIN with a bad check digit) and must not be used.
pub fn coerce(field: &FieldSpec, raw: &Value, base: Option<&url::Url>) -> Option<Value> {
    match field.json_type {
        JsonType::Array => match raw {
            Value::Array(values) => {
                let scalar = FieldSpec {
                    json_type: JsonType::String,
                    ..field.clone()
                };
                let out: Vec<Value> = values
                    .iter()
                    .filter_map(|v| coerce(&scalar, v, base))
                    .collect();
                (!out.is_empty()).then_some(Value::Array(out))
            }
            other => {
                let scalar = FieldSpec {
                    json_type: JsonType::String,
                    ..field.clone()
                };
                coerce(&scalar, other, base).map(|v| Value::Array(vec![v]))
            }
        },
        JsonType::Records => None,
        _ => coerce_scalar(field, raw, base),
    }
}

/// Text form of a raw value: strings as-is, numbers formatted, objects by their `name`,
/// `value`, `amount` or `@value` key, arrays by their first element.
pub fn raw_text(raw: &Value) -> Option<String> {
    match raw {
        Value::String(s) => {
            let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
            (!t.is_empty()).then_some(t)
        }
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Array(a) => a.iter().find_map(raw_text),
        Value::Object(o) => pick(
            o,
            &["name", "@value", "value", "text", "amount", "url", "@id"],
        )
        .and_then(raw_text),
        Value::Null => None,
    }
}

fn pick<'a>(o: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| o.get(*k))
}

fn number_value(v: f64, json_type: JsonType) -> Option<Value> {
    if json_type == JsonType::Integer {
        (v.fract() == 0.0).then(|| Value::from(v as i64))
    } else {
        serde_json::Number::from_f64(v).map(Value::Number)
    }
}

fn coerce_scalar(field: &FieldSpec, raw: &Value, base: Option<&url::Url>) -> Option<Value> {
    let numeric = matches!(field.json_type, JsonType::Number | JsonType::Integer);
    match field.kind {
        ValueKind::Price => {
            let amount = match raw {
                Value::Number(n) => n.as_f64(),
                Value::Object(o) => {
                    return pick(
                        o,
                        &["amount", "value", "price", "current", "raw", "lowPrice"],
                    )
                    .and_then(|v| coerce_scalar(field, v, base));
                }
                other => raw_text(other)
                    .and_then(|t| parse_price(&t))
                    .map(|p| p.amount),
            }?;
            if numeric {
                number_value(amount, field.json_type)
            } else {
                raw_text(raw).map(Value::String)
            }
        }
        ValueKind::Currency => {
            let text = match raw {
                Value::Object(o) => {
                    pick(o, &["currency", "currencyCode", "priceCurrency"]).and_then(raw_text)?
                }
                other => raw_text(other)?,
            };
            parse_currency(&text).map(Value::String)
        }
        ValueKind::Date => parse_date(&raw_text(raw)?).map(Value::String),
        ValueKind::Phone => parse_phone(&raw_text(raw)?).map(Value::String),
        ValueKind::Email => {
            let text = raw_text(raw)?;
            parse_email(text.trim_start_matches("mailto:")).map(Value::String)
        }
        ValueKind::Url | ValueKind::Image => parse_url(&raw_text(raw)?, base).map(Value::String),
        ValueKind::Gtin => parse_gtin(&raw_text(raw)?).map(Value::String),
        ValueKind::Isbn => parse_isbn(&raw_text(raw)?).map(Value::String),
        ValueKind::Rating => {
            let value = match raw {
                Value::Number(n) => n.as_f64(),
                Value::Object(o) => {
                    return pick(o, &["ratingValue", "value", "average", "rating"])
                        .and_then(|v| coerce_scalar(field, v, base));
                }
                other => raw_text(other).and_then(|t| parse_rating(&t)),
            }?;
            match field.json_type {
                JsonType::String => Some(Value::String(raw_text(raw)?)),
                JsonType::Integer => number_value(value, JsonType::Integer),
                _ => number_value(value, JsonType::Number),
            }
        }
        ValueKind::Number | ValueKind::Integer => {
            let value = match raw {
                Value::Number(n) => n.as_f64(),
                other => raw_text(other).and_then(|t| parse_number(&t)),
            }?;
            number_value(value, field.json_type)
        }
        ValueKind::Boolean => match raw {
            Value::Bool(b) => Some(Value::Bool(*b)),
            other => {
                let t = compact(&raw_text(other)?);
                if ["true", "yes", "instock", "available", "1"]
                    .iter()
                    .any(|y| t.ends_with(y))
                {
                    Some(Value::Bool(true))
                } else if ["false", "no", "outofstock", "unavailable", "soldout", "0"]
                    .iter()
                    .any(|n| t.ends_with(n))
                {
                    Some(Value::Bool(false))
                } else {
                    None
                }
            }
        },
        ValueKind::Quantity | ValueKind::Text => {
            if numeric {
                let value = match raw {
                    Value::Number(n) => n.as_f64(),
                    other => raw_text(other).and_then(|t| parse_number(&t)),
                }?;
                number_value(value, field.json_type)
            } else {
                raw_text(raw).map(Value::String)
            }
        }
    }
}

/// Whether two coerced values agree (numbers within a relative 1e-6, text case-insensitively).
pub fn values_agree(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= 1e-6 * x.abs().max(y.abs()).max(1.0),
            _ => false,
        },
        (Value::String(x), Value::String(y)) => {
            x.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
                == y.split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase()
        }
        _ => a == b,
    }
}
