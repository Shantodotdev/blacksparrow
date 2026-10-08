//! Synonyms for common field words, used to expand `find` queries and to match requested
//! fields to labels on a page in `extract`.

/// Groups of interchangeable words. Every word in a group expands to the others.
const GROUPS: &[&[&str]] = &[
    &[
        "price", "cost", "costs", "pricing", "fee", "fees", "charge", "amount", "rate",
    ],
    &["phone", "telephone", "tel", "mobile", "call"],
    &["address", "location", "street", "located"],
    &["email", "e-mail", "mail"],
    &["hours", "opening", "open", "schedule"],
    &["shipping", "delivery", "dispatch"],
    &["refund", "return", "returns", "refunds"],
    &["author", "writer", "byline"],
    &["date", "published", "posted", "updated"],
    &["rating", "stars", "score", "reviews"],
    &["stock", "availability", "available", "inventory", "left"],
    &["weight", "weigh", "weighs", "mass"],
    &["size", "dimensions", "measurements"],
    &["sku", "mpn", "model", "part"],
    &["name", "title"],
    &["description", "summary", "about", "overview"],
    &["image", "photo", "picture"],
    &["brand", "manufacturer", "maker"],
    &["company", "organization", "organisation", "business"],
    &["job", "position", "role", "vacancy"],
    &["salary", "pay", "compensation", "wage"],
    &["limit", "maximum", "max", "cap", "capped"],
    &["default", "defaults"],
];

/// Other words for `term` (lowercase), excluding the term itself. Empty when unknown.
pub fn expand_term(term: &str) -> Vec<&'static str> {
    let term = term.to_ascii_lowercase();
    GROUPS
        .iter()
        .filter(|group| group.contains(&term.as_str()))
        .flat_map(|group| group.iter().copied())
        .filter(|w| *w != term)
        .collect()
}

/// Whether two words are the same or synonyms.
pub fn are_synonyms(a: &str, b: &str) -> bool {
    let a = a.to_ascii_lowercase();
    let b = b.to_ascii_lowercase();
    a == b || expand_term(&a).contains(&b.as_str())
}
