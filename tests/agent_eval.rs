//! Evaluation harness for agent-mode extraction over the offline corpus in
//! `tests/fixtures/agent/`.
//!
//! Prints main-content word-level F1 per page type and fails when any score drops more than
//! `TOLERANCE` below the signed-off baseline in `baseline.json`. Run with
//! `cargo test --test agent_eval -- --nocapture` to see the report; set
//! `BLACKSPARROW_EVAL_WRITE_BASELINE=1` to record a new baseline.

use blacksparrow::extract::{html_to_document, ScrapeOptions};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

const TOLERANCE: f64 = 0.02;

#[derive(Deserialize)]
struct Entry {
    file: String,
    url: String,
    page_type: String,
}

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agent")
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        // Link reference numbers and URLs in reference lists are formatting, not content.
        .filter(|w| !w.chars().all(|c| c.is_ascii_digit()) || w.len() > 1)
        .map(str::to_lowercase)
        .collect()
}

/// Word-level F1 between two texts (multiset overlap).
fn word_f1(predicted: &str, expected: &str) -> f64 {
    let p = words(predicted);
    let e = words(expected);
    if p.is_empty() || e.is_empty() {
        return if p.is_empty() && e.is_empty() {
            1.0
        } else {
            0.0
        };
    }
    let mut counts: BTreeMap<&str, i64> = BTreeMap::new();
    for w in &e {
        *counts.entry(w.as_str()).or_insert(0) += 1;
    }
    let mut overlap = 0i64;
    for w in &p {
        if let Some(c) = counts.get_mut(w.as_str()) {
            if *c > 0 {
                *c -= 1;
                overlap += 1;
            }
        }
    }
    let precision = overlap as f64 / p.len() as f64;
    let recall = overlap as f64 / e.len() as f64;
    if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    }
}

/// Strips the `[n]: url` reference list so only body text is compared.
fn body_only(markdown: &str) -> String {
    markdown
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with('[') && t.contains("]: http"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn main_content_f1_per_page_type() {
    let dir = corpus_dir();
    let corpus: Vec<Entry> =
        serde_json::from_str(&std::fs::read_to_string(dir.join("corpus.json")).unwrap()).unwrap();

    let mut per_type: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut per_page: BTreeMap<String, f64> = BTreeMap::new();
    let mut total_ms = 0u128;
    println!(
        "\n{:<24} {:<12} {:>6} {:>8} {:>7}",
        "page", "type", "F1", "tokens", "ms"
    );
    for entry in &corpus {
        let html = std::fs::read_to_string(dir.join(&entry.file)).unwrap();
        let expected_path = dir.join(entry.file.replace(".html", ".expected.md"));
        let expected = std::fs::read_to_string(&expected_path).unwrap();
        let started = std::time::Instant::now();
        let doc = html_to_document(&html, &entry.url, &ScrapeOptions::default()).unwrap();
        let ms = started.elapsed().as_millis();
        total_ms += ms;
        let f1 = word_f1(&body_only(&doc.markdown), &body_only(&expected));
        println!(
            "{:<24} {:<12} {:>6.3} {:>8} {:>7} ({})",
            entry.file, entry.page_type, f1, doc.tokens, ms, doc.extractor
        );
        if std::env::var("BLACKSPARROW_EVAL_SHOW").is_ok() {
            println!("----\n{}\n----", doc.markdown);
        }
        per_type
            .entry(entry.page_type.clone())
            .or_default()
            .push(f1);
        per_page.insert(entry.file.clone(), f1);
    }
    println!("\nmean F1 per page type:");
    for (page_type, scores) in &per_type {
        let mean = scores.iter().sum::<f64>() / scores.len() as f64;
        println!("  {page_type:<12} {mean:.3}");
    }
    println!(
        "  total extraction time {total_ms} ms for {} pages (debug build)",
        corpus.len()
    );

    let baseline_path = dir.join("baseline.json");
    if std::env::var("BLACKSPARROW_EVAL_WRITE_BASELINE").is_ok() {
        let rounded: BTreeMap<&String, f64> = per_page
            .iter()
            .map(|(k, v)| (k, (v * 1000.0).floor() / 1000.0))
            .collect();
        std::fs::write(
            &baseline_path,
            serde_json::to_string_pretty(&rounded).unwrap() + "\n",
        )
        .unwrap();
        return;
    }
    let baseline: BTreeMap<String, f64> =
        serde_json::from_str(&std::fs::read_to_string(&baseline_path).unwrap()).unwrap();
    for (page, floor) in &baseline {
        let score = per_page.get(page).copied().unwrap_or(0.0);
        assert!(
            score + TOLERANCE >= *floor,
            "{page}: F1 {score:.3} fell below baseline {floor:.3}"
        );
    }
}

/// Share of labelled fields extracted with exactly the labelled value.
fn field_accuracy(data: &serde_json::Value, expected: &serde_json::Value) -> f64 {
    match (data, expected) {
        (serde_json::Value::Array(got), serde_json::Value::Array(want)) => {
            let total: usize = want
                .iter()
                .map(|w| w.as_object().map_or(0, |o| o.len()))
                .sum();
            let correct: usize = want
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    let g = got.get(i).cloned().unwrap_or_default();
                    w.as_object().map_or(0, |o| {
                        o.iter()
                            .filter(|(k, v)| g.get(k.as_str()) == Some(v))
                            .count()
                    })
                })
                .sum();
            correct as f64 / total.max(1) as f64
        }
        (got, serde_json::Value::Object(want)) => {
            let correct = want
                .iter()
                .filter(|(k, v)| got.get(k.as_str()) == Some(v))
                .count();
            correct as f64 / want.len().max(1) as f64
        }
        _ => 0.0,
    }
}

#[test]
fn field_accuracy_per_page_type() {
    use blacksparrow::extract::fields::Extractor;
    let dir = corpus_dir();
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
    let product_schema = serde_json::json!({"type": "object", "properties": {
        "name": {"type": "string"}, "price": {"type": "number"}, "currency": {"type": "string"},
        "sku": {"type": "string"}, "gtin": {"type": "string"}, "rating": {"type": "number"},
        "brand": {"type": "string"}}});
    let listing_schema = serde_json::json!({"type": "array", "items": {"type": "object",
        "properties": {"name": {"type": "string"}, "price": {"type": "number"},
        "url": {"type": "string", "format": "uri"}}}});

    // The sibling page is extracted after the JSON-LD page, so it can use learned rules.
    let mut extractor = Extractor::default();
    let cases = [
        (
            "product.html",
            "https://shop.example/packs/trailhead-40",
            &product_schema,
            "product.fields.json",
        ),
        (
            "product_sibling.html",
            "https://shop.example/packs/ridge-30",
            &product_schema,
            "product_sibling.fields.json",
        ),
        (
            "listing.html",
            "https://shop.example/packs",
            &listing_schema,
            "listing.records.json",
        ),
    ];
    let mut scores = BTreeMap::new();
    println!("\n{:<24} {:>8}", "page", "fields");
    for (file, url, schema, labels) in cases {
        let result = extractor
            .extract_html(&read(file), url, schema, None)
            .unwrap();
        let expected: serde_json::Value = serde_json::from_str(&read(labels)).unwrap();
        let accuracy = field_accuracy(&result.data, &expected);
        println!("{file:<24} {accuracy:>8.3}");
        scores.insert(file.to_string(), accuracy);
    }

    let baseline_path = dir.join("fields_baseline.json");
    if std::env::var("BLACKSPARROW_EVAL_WRITE_BASELINE").is_ok() {
        std::fs::write(
            &baseline_path,
            serde_json::to_string_pretty(&scores).unwrap() + "\n",
        )
        .unwrap();
        return;
    }
    let baseline: BTreeMap<String, f64> =
        serde_json::from_str(&std::fs::read_to_string(&baseline_path).unwrap()).unwrap();
    for (page, floor) in &baseline {
        let score = scores.get(page).copied().unwrap_or(0.0);
        assert!(
            score + TOLERANCE >= *floor,
            "{page}: field accuracy {score:.3} fell below baseline {floor:.3}"
        );
    }
}
