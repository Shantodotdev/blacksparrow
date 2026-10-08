//! `extract`: fill a JSON schema from a page with no LLM.
//!
//! Each field is looked up in sources from most to least reliable, and the result records
//! which source won and how confident it is:
//!
//! 1. Caller rules and `x-selector` hints ([`rules`]).
//! 2. Published structured data: JSON-LD, Microdata, RDFa ([`structured`]).
//! 3. Embedded app data such as `__NEXT_DATA__` ([`embedded`]).
//! 4. Rules learned from other pages of the same template ([`learn`]).
//! 5. Labels in the page: spec tables, definition lists, `Label: value` lines ([`records`]).
//! 6. OpenGraph and meta tags.
//! 7. Typed recognizers over the main text ([`recognize`]), and the page heading for names.
//!
//! Lists come from caller rules or repeated sibling records. Every value passes its type's
//! recognizer (a GTIN with a bad check digit is dropped, not returned), confidence rises when
//! independent sources agree, and the output is validated against the schema.

pub mod embedded;
pub mod learn;
pub mod recognize;
pub mod records;
pub mod rules;
pub mod schema;
pub mod structured;

pub use learn::{template_id, RuleBook};
pub use rules::{FieldRule, RuleSet};
pub use schema::{FieldSpec, JsonType, SchemaShape, ValueKind};

use crate::error::{SeoError, SeoResult};
use crate::extract::clean::{clean_document, CleanOptions};
use crate::extract::fields::recognize::{
    find_price, parse_date, parse_email, parse_gtin, parse_isbn, parse_phone,
};
use crate::extract::fields::records::{extract_records, find_records, labels, visible_text};
use crate::extract::fields::schema::{coerce, parse_schema, values_agree};
use crate::extract::fields::structured::{item_type, lookup, Item};
use crate::extract::scrape::Scraper;
use crate::extract::types::{OutputFormat, PageStatus, ScrapeOptions};
use crate::storage::documents::documents_for_crawl;
use dom_query::{Document, NodeRef};
use futures::stream::{self, StreamExt};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Where a value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A caller rule or `x-selector` hint.
    Rule,
    /// JSON-LD.
    JsonLd,
    /// Microdata.
    Microdata,
    /// RDFa.
    Rdfa,
    /// Embedded application JSON.
    Embedded,
    /// A rule learned from another page of the same template.
    Learned,
    /// A label in the page (table, definition list, `Label: value`).
    Label,
    /// A repeated record.
    Record,
    /// OpenGraph / meta tags.
    OpenGraph,
    /// Page heading, title or a typed recognizer over the text.
    Page,
}

impl Source {
    fn base_confidence(self) -> f64 {
        match self {
            Source::Rule | Source::JsonLd => 0.95,
            Source::Microdata | Source::Rdfa => 0.9,
            Source::Embedded => 0.85,
            Source::Learned => 0.6,
            Source::Label | Source::OpenGraph => 0.75,
            Source::Record => 0.75,
            Source::Page => 0.5,
        }
    }

    fn is_published(self) -> bool {
        matches!(
            self,
            Source::JsonLd | Source::Microdata | Source::Rdfa | Source::Embedded
        )
    }
}

/// One extracted field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldValue {
    /// The value (the first record's value for lists).
    pub value: Value,
    /// Winning source.
    pub source: Source,
    /// 0 to 1.
    pub confidence: f64,
    /// Selector of the element the value came from, when it came from the DOM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
}

/// Result for one page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractResult {
    /// Page URL.
    pub url: String,
    /// Page status; `data` is null unless `ok`.
    pub status: PageStatus,
    /// Data shaped like the schema (missing fields are `null`).
    pub data: Value,
    /// Per-field source and confidence.
    pub fields: BTreeMap<String, FieldValue>,
    /// Fields that are missing or below the confidence threshold.
    pub low_confidence: Vec<String>,
    /// Template the page was grouped into for learning.
    pub template_id: String,
    /// Whether `data` satisfies the schema.
    pub valid: bool,
    /// Schema violations.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
    /// Why the page could not be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Extracts fields from HTML, learning selectors as it goes.
#[derive(Debug, Clone)]
pub struct Extractor {
    rules: RuleBook,
    /// Learn selectors from pages with structured data.
    pub learn: bool,
    /// Fields under this confidence are listed in `low_confidence`.
    pub min_confidence: f64,
}

impl Default for Extractor {
    fn default() -> Self {
        Self {
            rules: RuleBook::default(),
            learn: true,
            min_confidence: 0.6,
        }
    }
}

struct Candidate {
    value: Value,
    source: Source,
    confidence: f64,
    selector: Option<String>,
}

struct Page<'a> {
    base: Option<url::Url>,
    host: String,
    template: String,
    raw: &'a Document,
    clean: &'a Document,
    main: &'a Document,
    items: Vec<Item>,
    embedded: Vec<Value>,
    labels: Vec<records::LabelValue>,
}

impl Extractor {
    /// The learned rules.
    pub fn rules(&self) -> &RuleBook {
        &self.rules
    }

    /// Loads stored rules for a host and template.
    ///
    /// # Errors
    ///
    /// Returns storage errors.
    pub fn load_rules(&mut self, conn: &Connection, host: &str, template: &str) -> SeoResult<()> {
        self.rules.load(conn, host, template)
    }

    /// Saves learned rules.
    ///
    /// # Errors
    ///
    /// Returns storage errors.
    pub fn save_rules(&self, conn: &Connection) -> SeoResult<()> {
        self.rules.save(conn)
    }

    /// Extracts `schema` from a page.
    ///
    /// # Errors
    ///
    /// Returns [`SeoError::Config`] for an unsupported schema or an invalid selector.
    pub fn extract_html(
        &mut self,
        html: &str,
        url: &str,
        schema: &Value,
        caller_rules: Option<&RuleSet>,
    ) -> SeoResult<ExtractResult> {
        let shape = parse_schema(schema)?;
        let base = url::Url::parse(url).ok();
        let raw = Document::from(html);
        let clean = Document::from(html);
        clean_document(
            &clean,
            CleanOptions {
                only_main_content: false,
                exclude_selectors: &[],
            },
        )?;
        let main = Document::from(html);
        clean_document(
            &main,
            CleanOptions {
                only_main_content: true,
                exclude_selectors: &[],
            },
        )?;
        let page = Page {
            host: base
                .as_ref()
                .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
                .unwrap_or_default(),
            template: template_id(url),
            base,
            items: structured::collect(&raw),
            embedded: embedded::collect(&raw),
            labels: labels(&main),
            raw: &raw,
            clean: &clean,
            main: &main,
        };

        let mut result = ExtractResult {
            url: url.to_string(),
            status: PageStatus::Ok,
            data: Value::Null,
            fields: BTreeMap::new(),
            low_confidence: Vec::new(),
            template_id: page.template.clone(),
            valid: true,
            errors: Vec::new(),
            error: None,
        };

        match &shape {
            SchemaShape::List(fields) => {
                let (records, summary) = self.list(&page, fields, caller_rules)?;
                result.data = Value::Array(records);
                result.fields = summary;
                for field in fields {
                    if !result.fields.contains_key(&field.name) {
                        result.low_confidence.push(field.name.clone());
                    }
                }
            }
            SchemaShape::Object(fields) => {
                let mut data = Map::new();
                let scalar: Vec<FieldSpec> = fields
                    .iter()
                    .filter(|f| f.json_type != JsonType::Records)
                    .cloned()
                    .collect();
                let rules_scope = caller_rules.filter(|r| r.base.is_none());
                let embedded_obj = embedded::best_object(&page.embedded, &scalar);
                let primaries = primary_items(&page.items, &scalar);
                let mut published: Vec<(FieldSpec, Value)> = Vec::new();
                for field in fields {
                    if field.json_type == JsonType::Records {
                        let rules_for_list = caller_rules.filter(|r| r.base.is_some());
                        let (records, summary) = self.list(&page, &field.items, rules_for_list)?;
                        let confidence = summary
                            .values()
                            .map(|f| f.confidence)
                            .fold(f64::NAN, f64::min);
                        if records.is_empty() {
                            result.low_confidence.push(field.name.clone());
                        }
                        result.fields.insert(
                            field.name.clone(),
                            FieldValue {
                                value: records.first().cloned().unwrap_or(Value::Null),
                                source: summary
                                    .values()
                                    .next()
                                    .map(|f| f.source)
                                    .unwrap_or(Source::Record),
                                confidence: if confidence.is_nan() { 0.0 } else { confidence },
                                selector: None,
                            },
                        );
                        data.insert(field.name.clone(), Value::Array(records));
                        continue;
                    }
                    let candidates =
                        self.candidates(&page, field, rules_scope, &primaries, embedded_obj)?;
                    match merge(candidates) {
                        Some(winner) => {
                            if winner.source.is_published() {
                                published.push((field.clone(), winner.value.clone()));
                            }
                            if winner.confidence < self.min_confidence {
                                result.low_confidence.push(field.name.clone());
                            }
                            data.insert(field.name.clone(), winner.value.clone());
                            result.fields.insert(
                                field.name.clone(),
                                FieldValue {
                                    value: winner.value,
                                    source: winner.source,
                                    confidence: winner.confidence,
                                    selector: winner.selector,
                                },
                            );
                        }
                        None => {
                            result.low_confidence.push(field.name.clone());
                            data.insert(field.name.clone(), Value::Null);
                        }
                    }
                }
                if self.learn && !page.host.is_empty() {
                    for (field, value) in &published {
                        if let Some(rule) = learn::learn_field(
                            page.clean,
                            &self.rules,
                            &page.host,
                            &page.template,
                            field,
                            value,
                            page.base.as_ref(),
                        ) {
                            self.rules.put(rule);
                        }
                    }
                }
                for field in fields {
                    if field.required && data.get(&field.name).is_none_or(Value::is_null) {
                        result
                            .errors
                            .push(format!("missing required field '{}'", field.name));
                    }
                }
                result.data = Value::Object(data);
            }
        }
        result.valid = result.errors.is_empty();
        Ok(result)
    }

    fn list(
        &self,
        page: &Page<'_>,
        fields: &[FieldSpec],
        caller_rules: Option<&RuleSet>,
    ) -> SeoResult<(Vec<Value>, BTreeMap<String, FieldValue>)> {
        let base = page.base.as_ref();
        let (records, source): (Vec<records::Record>, Source) =
            match caller_rules.and_then(|r| r.base.as_deref().map(|b| (r, b))) {
                Some((rules, base_sel)) => {
                    let root = page.clean.root();
                    let mut out = Vec::new();
                    for node in rules::base_matches(&root, base_sel)? {
                        out.push(rule_record(&node, rules, fields, base)?);
                    }
                    (out, Source::Rule)
                }
                None => {
                    let nodes: Vec<NodeRef> = find_records(page.main);
                    (extract_records(&nodes, fields, base), Source::Record)
                }
            };
        let mut summary: BTreeMap<String, FieldValue> = BTreeMap::new();
        let mut values = Vec::new();
        for record in &records {
            let mut obj = Map::new();
            for field in fields {
                obj.insert(field.name.clone(), Value::Null);
            }
            for (name, value, confidence, selector) in record {
                obj.insert(name.clone(), value.clone());
                summary.entry(name.clone()).or_insert_with(|| FieldValue {
                    value: value.clone(),
                    source,
                    confidence: *confidence,
                    selector: Some(selector.clone()),
                });
            }
            if record.is_empty() {
                continue;
            }
            values.push(Value::Object(obj));
        }
        Ok((values, summary))
    }

    fn candidates(
        &self,
        page: &Page<'_>,
        field: &FieldSpec,
        caller_rules: Option<&RuleSet>,
        primaries: &[&Item],
        embedded_obj: Option<&Value>,
    ) -> SeoResult<Vec<Candidate>> {
        let base = page.base.as_ref();
        let mut out = Vec::new();
        let mut push =
            |raw: Option<Value>, source: Source, confidence: f64, selector: Option<String>| {
                if let Some(value) = raw.and_then(|r| coerce(field, &r, base)) {
                    out.push(Candidate {
                        value,
                        source,
                        confidence,
                        selector,
                    });
                }
            };

        // 1. Caller rules and hints.
        let rule_selector = caller_rules
            .and_then(|r| r.fields.get(&field.name))
            .map(|r| r.selector().to_string())
            .or_else(|| field.selector.clone());
        if let Some(selector) = rule_selector {
            let root = page.clean.root();
            if let Some((raw, el)) = rules::apply_selector(&root, &selector)? {
                push(
                    Some(raw),
                    Source::Rule,
                    Source::Rule.base_confidence(),
                    Some(crate::extract::convert::css_path(&el)),
                );
            }
        }
        // 2. Structured data, best item per source.
        for item in primaries {
            push(
                lookup(&item.value, field, 3).cloned(),
                item.source,
                item.source.base_confidence(),
                None,
            );
        }
        // 3. Embedded app data.
        if let Some(obj) = embedded_obj {
            push(
                structured::lookup(obj, field, 2).cloned(),
                Source::Embedded,
                Source::Embedded.base_confidence(),
                None,
            );
        }
        // 4. Learned rules.
        if let Some(rule) = self.rules.get(&page.host, &page.template, &field.name) {
            if let Some((value, selector)) = learn::apply_rule(page.clean, rule, field, base) {
                let confidence = (Source::Learned.base_confidence()
                    + 0.1 * f64::from(rule.support.min(3)))
                .min(0.9);
                out.push(Candidate {
                    value,
                    source: Source::Learned,
                    confidence,
                    selector: Some(selector),
                });
            }
        }
        let mut push = |raw: Option<Value>, source: Source, selector: Option<String>| {
            if let Some(value) = raw.and_then(|r| coerce(field, &r, base)) {
                out.push(Candidate {
                    value,
                    source,
                    confidence: source.base_confidence(),
                    selector,
                });
            }
        };
        // 5. Labels.
        if let Some(lv) = page.labels.iter().find(|lv| field.matches_label(&lv.label)) {
            push(
                Some(Value::String(lv.value.clone())),
                Source::Label,
                Some(lv.selector.clone()),
            );
        }
        // 6. OpenGraph and meta.
        if let Some(og) = page.items.iter().find(|i| i.source == Source::OpenGraph) {
            push(
                lookup(&og.value, field, 0).cloned(),
                Source::OpenGraph,
                None,
            );
        }
        // 7. Page heuristics and recognizers over the main text.
        push(page_value(page, field), Source::Page, None);
        Ok(out)
    }
}

/// Applies caller field rules inside one record element.
fn rule_record(
    node: &NodeRef,
    rules: &RuleSet,
    fields: &[FieldSpec],
    base: Option<&url::Url>,
) -> SeoResult<records::Record> {
    let mut record = Vec::new();
    for field in fields {
        let Some(rule) = rules.fields.get(&field.name) else {
            continue;
        };
        let Some((raw, el)) = rules::apply_selector(node, rule.selector())? else {
            continue;
        };
        let spec = match rule.kind() {
            Some(kind) => FieldSpec {
                kind,
                ..field.clone()
            },
            None => field.clone(),
        };
        if let Some(value) = coerce(&spec, &raw, base) {
            record.push((
                field.name.clone(),
                value,
                Source::Rule.base_confidence(),
                crate::extract::convert::css_path(&el),
            ));
        }
    }
    Ok(record)
}

/// The structured item per source (JSON-LD, Microdata, RDFa) that answers the most fields.
fn primary_items<'a>(items: &'a [Item], fields: &[FieldSpec]) -> Vec<&'a Item> {
    let mut out = Vec::new();
    for source in [Source::JsonLd, Source::Microdata, Source::Rdfa] {
        let best = items
            .iter()
            .filter(|i| i.source == source)
            .map(|i| {
                let score = fields
                    .iter()
                    .filter(|f| lookup(&i.value, f, 3).is_some())
                    .count();
                // Page-level wrappers (WebPage, BreadcrumbList) lose ties to real entities.
                let wrapper = matches!(
                    item_type(&i.value).as_str(),
                    "webpage" | "website" | "breadcrumblist" | "organization"
                );
                (score * 2 + usize::from(!wrapper), i)
            })
            .filter(|(score, _)| *score > 1)
            .max_by_key(|(score, _)| *score);
        if let Some((_, item)) = best {
            out.push(item);
        }
    }
    out
}

/// Last-resort values read from the page itself.
fn page_value(page: &Page<'_>, field: &FieldSpec) -> Option<Value> {
    let text = || {
        page.main
            .select("body")
            .nodes()
            .first()
            .map(visible_text)
            .unwrap_or_default()
    };
    let aliases = field.aliases();
    let is = |word: &str| aliases.iter().any(|a| a == word);
    let found = match field.kind {
        ValueKind::Price => find_price(&text()).map(|p| p.amount.to_string()),
        ValueKind::Date => parse_date(&text()),
        ValueKind::Phone => parse_phone(&text()),
        ValueKind::Email => parse_email(&text()),
        ValueKind::Gtin => text()
            .split(|c: char| !c.is_ascii_digit())
            .find_map(parse_gtin),
        ValueKind::Isbn => text()
            .split(|c: char| !(c.is_ascii_digit() || c == '-' || c == 'X'))
            .find_map(parse_isbn),
        ValueKind::Url if is("url") => page.base.as_ref().map(|u| u.to_string()),
        ValueKind::Text if is("name") || is("title") => page
            .main
            .select("h1")
            .nodes()
            .first()
            .map(visible_text)
            .filter(|t| !t.is_empty())
            .or_else(|| {
                page.raw
                    .select("title")
                    .nodes()
                    .first()
                    .map(visible_text)
                    .filter(|t| !t.is_empty())
            }),
        ValueKind::Text if is("description") => page
            .raw
            .select(r#"meta[name="description"]"#)
            .nodes()
            .first()
            .and_then(|m| m.attr("content").map(|c| c.to_string())),
        _ => None,
    }?;
    Some(Value::String(found))
}

/// Picks the first candidate (sources are pushed in priority order) and raises its
/// confidence by 0.05 for every other source that agrees.
fn merge(candidates: Vec<Candidate>) -> Option<Candidate> {
    let mut iter = candidates.into_iter();
    let mut winner = iter.next()?;
    let mut seen = vec![winner.source];
    for other in iter {
        if !seen.contains(&other.source) && values_agree(&other.value, &winner.value) {
            winner.confidence = (winner.confidence + 0.05).min(0.99);
            seen.push(other.source);
        }
        if winner.selector.is_none() && values_agree(&other.value, &winner.value) {
            winner.selector = other.selector;
        }
    }
    Some(winner)
}

/// An extract request for the CLI, MCP and HTTP API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtractRequest {
    /// One page.
    pub url: Option<String>,
    /// Several pages.
    pub urls: Vec<String>,
    /// Every page stored for this crawl.
    #[serde(alias = "crawlId")]
    pub crawl_id: Option<String>,
    /// JSON schema.
    pub schema: Value,
    /// Caller rules.
    pub rules: Option<RuleSet>,
    /// Learn selectors from pages with structured data.
    pub learn: bool,
    /// Fields under this confidence are reported as low confidence.
    #[serde(alias = "minConfidence")]
    pub min_confidence: f64,
    /// Pages processed at most.
    pub limit: usize,
    /// How pages are fetched.
    #[serde(alias = "scrapeOptions")]
    pub scrape: ScrapeOptions,
}

impl Default for ExtractRequest {
    fn default() -> Self {
        Self {
            url: None,
            urls: Vec::new(),
            crawl_id: None,
            schema: Value::Null,
            rules: None,
            learn: true,
            min_confidence: 0.6,
            limit: 100,
            scrape: ScrapeOptions::default(),
        }
    }
}

/// Fetches pages and extracts the schema from each. With several pages, a first pass learns
/// selectors from pages with structured data so a second pass can fill pages of the same
/// template that have none. Learned rules are stored when the scraper has a database.
///
/// # Errors
///
/// Returns [`SeoError::Config`] for a request without pages, an invalid schema or rules, and
/// storage errors.
pub async fn extract(scraper: &Scraper, req: &ExtractRequest) -> SeoResult<Vec<ExtractResult>> {
    parse_schema(&req.schema)?;
    let mut urls: Vec<String> = req
        .url
        .iter()
        .cloned()
        .chain(req.urls.iter().cloned())
        .collect();
    if let Some(crawl_id) = &req.crawl_id {
        let db = scraper.database().ok_or_else(|| {
            SeoError::Config("Extracting from a crawl needs a database".to_string())
        })?;
        let conn = db.connect()?;
        let mut offset = 0;
        loop {
            let (docs, total) = documents_for_crawl(&conn, crawl_id, offset, 200)?;
            if docs.is_empty() {
                break;
            }
            offset += docs.len();
            urls.extend(
                docs.into_iter()
                    .filter(|d| {
                        d.status == PageStatus::Ok
                            && matches!(d.source.as_str(), "html" | "rendered")
                    })
                    .map(|d| d.url),
            );
            if offset >= total {
                break;
            }
        }
    }
    urls.dedup();
    urls.truncate(req.limit.max(1));
    if urls.is_empty() {
        return Err(SeoError::Config(
            "extract needs a url, urls or a crawl_id".to_string(),
        ));
    }

    let mut scrape = req.scrape.clone();
    scrape.formats = vec![OutputFormat::RawHtml];
    scrape.max_age_secs = None;
    let pages: Vec<(String, crate::extract::PageDocument)> = stream::iter(urls.clone())
        .map(|u| {
            let scrape = &scrape;
            async move {
                let doc = scraper.scrape_full(&u, scrape).await;
                (u, doc)
            }
        })
        .buffered(4)
        .map(|(u, doc)| doc.map(|d| (u, d)))
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<SeoResult<Vec<_>>>()?;

    let mut extractor = Extractor {
        learn: req.learn,
        min_confidence: req.min_confidence,
        ..Default::default()
    };
    let conn = match scraper.database() {
        Some(db) => Some(db.connect()?),
        None => None,
    };
    if let Some(conn) = &conn {
        for (url, _) in &pages {
            let host = url::Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
                .unwrap_or_default();
            let template = template_id(url);
            if !extractor.rules().is_loaded(&host, &template) {
                extractor.load_rules(conn, &host, &template)?;
            }
        }
    }

    let html_of = |doc: &crate::extract::PageDocument| -> Option<String> {
        (doc.status == PageStatus::Ok && matches!(doc.source.as_str(), "html" | "rendered"))
            .then(|| doc.raw_html.clone())
            .flatten()
    };
    if pages.len() > 1 && req.learn {
        for (url, doc) in &pages {
            if let Some(html) = html_of(doc) {
                extractor.extract_html(&html, url, &req.schema, req.rules.as_ref())?;
            }
        }
    }
    let mut results = Vec::new();
    for (url, doc) in &pages {
        match html_of(doc) {
            Some(html) => {
                results.push(extractor.extract_html(&html, url, &req.schema, req.rules.as_ref())?)
            }
            None => results.push(ExtractResult {
                url: url.clone(),
                status: if doc.status == PageStatus::Ok {
                    PageStatus::NotHtml
                } else {
                    doc.status
                },
                data: Value::Null,
                fields: BTreeMap::new(),
                low_confidence: Vec::new(),
                template_id: template_id(url),
                valid: false,
                errors: Vec::new(),
                error: doc
                    .error
                    .clone()
                    .or_else(|| Some(format!("Not an HTML page ({})", doc.content_type))),
            }),
        }
    }
    if let Some(conn) = &conn {
        extractor.save_rules(conn)?;
    }
    Ok(results)
}
