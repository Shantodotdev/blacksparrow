//! Phase 4: structured extraction without an LLM.

use blacksparrow::extract::crawl::{crawl_site_with_id, CrawlControl, CrawlOptions};
use blacksparrow::extract::fields::recognize::{
    parse_date, parse_email, parse_gtin, parse_isbn, parse_phone, parse_price, parse_quantity,
    parse_rating,
};
use blacksparrow::extract::fields::{extract, ExtractRequest, Extractor, RuleSet, Source};
use blacksparrow::extract::map::SitemapMode;
use blacksparrow::extract::scrape::{Scraper, ScraperConfig};
use blacksparrow::extract::sink::MemorySink;
use blacksparrow::storage::documents::rules_for;
use blacksparrow::storage::Database;
use serde_json::{json, Value};
use std::path::PathBuf;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/agent")
            .join(name),
    )
    .unwrap()
}

fn product_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "price": {"type": "number"},
            "currency": {"type": "string"},
            "sku": {"type": "string"},
            "gtin": {"type": "string"},
            "rating": {"type": "number"},
            "brand": {"type": "string"}
        },
        "required": ["name", "price"]
    })
}

fn listing_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "price": {"type": "number"},
                "url": {"type": "string", "format": "uri"}
            }
        }
    })
}

#[test]
fn recognizers_normalise_values_and_validate_checksums() {
    let p = parse_price("$1,299.99").unwrap();
    assert_eq!((p.amount, p.currency.as_deref()), (1299.99, Some("USD")));
    let p = parse_price("1.299,99 €").unwrap();
    assert_eq!((p.amount, p.currency.as_deref()), (1299.99, Some("EUR")));
    assert_eq!(parse_price("£12").unwrap().currency.as_deref(), Some("GBP"));
    assert_eq!(parse_price("USD 40").unwrap().amount, 40.0);
    assert!(parse_price("call us").is_none());

    assert_eq!(parse_date("March 14, 2026").as_deref(), Some("2026-03-14"));
    assert_eq!(parse_date("14 Mar 2026").as_deref(), Some("2026-03-14"));
    assert_eq!(
        parse_date("2026-03-14T10:00:00Z").as_deref(),
        Some("2026-03-14")
    );
    assert_eq!(parse_date("14/03/2026").as_deref(), Some("2026-03-14"));
    assert_eq!(parse_date("2026/3/4").as_deref(), Some("2026-03-04"));
    assert!(parse_date("2026-02-30").is_none());
    assert!(parse_date("someday").is_none());

    assert_eq!(
        parse_phone("+1 (415) 555-0132").as_deref(),
        Some("+14155550132")
    );
    assert_eq!(parse_phone("020 7946 0958").as_deref(), Some("02079460958"));
    assert!(parse_phone("12").is_none());

    assert_eq!(
        parse_email("Write to Sales@Example.com today").as_deref(),
        Some("sales@example.com")
    );
    assert!(parse_email("not an email").is_none());

    assert_eq!(
        parse_gtin("4006381333931").as_deref(),
        Some("4006381333931")
    );
    assert!(parse_gtin("4006381333932").is_none(), "bad checksum");
    assert_eq!(
        parse_gtin("0 12345 67890 5").as_deref(),
        Some("012345678905")
    );
    assert_eq!(
        parse_isbn("978-0-306-40615-7").as_deref(),
        Some("9780306406157")
    );
    assert_eq!(parse_isbn("0-306-40615-2").as_deref(), Some("0306406152"));
    assert!(parse_isbn("0-306-40615-3").is_none());

    assert_eq!(parse_rating("4.6 out of 5"), Some(4.6));
    assert_eq!(parse_rating("Rated 3/5"), Some(3.0));
    assert_eq!(parse_rating("4.3"), Some(4.3));
    assert_eq!(parse_quantity("1.2 kg"), Some((1.2, "kg".to_string())));
    assert_eq!(parse_quantity("40 L"), Some((40.0, "L".to_string())));
}

#[test]
fn json_ld_product_gives_exact_fields() {
    let mut ex = Extractor::default();
    let result = ex
        .extract_html(
            &fixture("product.html"),
            "https://shop.example/packs/trailhead-40",
            &product_schema(),
            None,
        )
        .unwrap();
    let expected: Value = serde_json::from_str(&fixture("product.fields.json")).unwrap();
    assert_eq!(result.data, expected);
    assert!(result.valid, "{:?}", result.errors);
    for (name, field) in &result.fields {
        assert_eq!(field.source, Source::JsonLd, "{name}");
        assert!(field.confidence >= 0.9, "{name}: {}", field.confidence);
    }
    assert!(result.low_confidence.is_empty());
    assert_eq!(result.template_id, "/packs/*");
}

#[test]
fn sibling_page_without_json_ld_is_filled_by_learned_rules() {
    let schema = product_schema();
    let mut ex = Extractor::default();
    ex.extract_html(
        &fixture("product.html"),
        "https://shop.example/packs/trailhead-40",
        &schema,
        None,
    )
    .unwrap();
    let result = ex
        .extract_html(
            &fixture("product_sibling.html"),
            "https://shop.example/packs/ridge-30",
            &schema,
            None,
        )
        .unwrap();

    let expected: Value = serde_json::from_str(&fixture("product_sibling.fields.json")).unwrap();
    for (key, value) in expected.as_object().unwrap() {
        assert_eq!(
            &result.data[key],
            value,
            "{key}: {:?}",
            result.fields.get(key)
        );
    }
    assert_eq!(result.fields["name"].source, Source::Learned);
    assert_eq!(result.fields["price"].source, Source::Learned);
    assert!(result.fields["price"].selector.is_some());
    // GTIN and brand are not on the sibling page at all: missing, not invented.
    assert_eq!(result.data["gtin"], Value::Null);
    assert!(result.low_confidence.contains(&"gtin".to_string()));
    assert!(result.valid, "{:?}", result.errors);

    // Without learning, the same page still gets labelled values but no learned selectors.
    let fresh = Extractor::default()
        .extract_html(
            &fixture("product_sibling.html"),
            "https://shop.example/packs/ridge-30",
            &schema,
            None,
        )
        .unwrap();
    assert!(fresh.fields.values().all(|f| f.source != Source::Learned));
    assert_eq!(fresh.data["sku"], "RG-30-BLU", "from the 'SKU: …' label");
}

#[test]
fn learned_rules_persist_in_storage() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("rules.db")).unwrap();
    let conn = db.connect().unwrap();
    let schema = product_schema();

    let mut ex = Extractor::default();
    ex.extract_html(
        &fixture("product.html"),
        "https://shop.example/packs/trailhead-40",
        &schema,
        None,
    )
    .unwrap();
    ex.save_rules(&conn).unwrap();
    let stored = rules_for(&conn, "shop.example", "/packs/*").unwrap();
    assert!(stored
        .iter()
        .any(|r| r.field == "price" && r.source == "learned"));

    let mut reloaded = Extractor::default();
    reloaded
        .load_rules(&conn, "shop.example", "/packs/*")
        .unwrap();
    let result = reloaded
        .extract_html(
            &fixture("product_sibling.html"),
            "https://shop.example/packs/ridge-30",
            &schema,
            None,
        )
        .unwrap();
    assert_eq!(result.data["price"], 99.5);
    assert_eq!(result.fields["price"].source, Source::Learned);
}

#[test]
fn listing_page_yields_one_record_per_card() {
    let result = Extractor::default()
        .extract_html(
            &fixture("listing.html"),
            "https://shop.example/packs",
            &listing_schema(),
            None,
        )
        .unwrap();
    let expected: Value = serde_json::from_str(&fixture("listing.records.json")).unwrap();
    assert_eq!(result.data, expected);
    assert!(result.valid);

    // The same list as a property of an object schema.
    let wrapped = Extractor::default()
        .extract_html(
            &fixture("listing.html"),
            "https://shop.example/packs",
            &json!({
                "type": "object",
                "properties": {"products": listing_schema()}
            }),
            None,
        )
        .unwrap();
    assert_eq!(wrapped.data["products"], expected);
}

#[test]
fn caller_rules_select_records_and_attributes() {
    let rules: RuleSet = serde_json::from_value(json!({
        "base": ".product-card",
        "fields": {
            "name": ".card-title",
            "url": "a@href",
            "price": {"selector": ".card-price", "type": "price"}
        }
    }))
    .unwrap();
    let result = Extractor::default()
        .extract_html(
            &fixture("listing.html"),
            "https://shop.example/packs",
            &listing_schema(),
            Some(&rules),
        )
        .unwrap();
    let expected: Value = serde_json::from_str(&fixture("listing.records.json")).unwrap();
    assert_eq!(result.data, expected);
    assert_eq!(result.fields["name"].source, Source::Rule);
}

#[test]
fn spec_tables_definition_lists_and_label_lines_become_fields() {
    let html = r#"<html><body><main><h1>Phone X</h1>
        <table class="specs"><tr><th>Battery</th><td>4000 mAh</td></tr>
        <tr><th>Screen size</th><td>6.1 in</td></tr></table>
        <dl><dt>Weight</dt><dd>174 g</dd></dl>
        <p>Model: PX-2026</p><p>Telephone: +44 20 7946 0958</p>
        <p>Released on 14 March 2026</p></main></body></html>"#;
    let schema = json!({"type": "object", "properties": {
        "battery": {"type": "string"},
        "screen_size": {"type": "string"},
        "weight": {"type": "string"},
        "model": {"type": "string"},
        "phone": {"type": "string"},
        "release_date": {"type": "string", "format": "date"},
        "title": {"type": "string"}
    }});
    let result = Extractor::default()
        .extract_html(html, "https://phones.example/x", &schema, None)
        .unwrap();
    assert_eq!(result.data["battery"], "4000 mAh");
    assert_eq!(result.data["screen_size"], "6.1 in");
    assert_eq!(result.data["weight"], "174 g");
    assert_eq!(result.data["model"], "PX-2026");
    assert_eq!(result.data["phone"], "+442079460958");
    assert_eq!(result.data["release_date"], "2026-03-14");
    assert_eq!(result.data["title"], "Phone X");
    assert_eq!(result.fields["battery"].source, Source::Label);
}

#[test]
fn invalid_gtin_is_rejected_not_returned() {
    let html = r#"<html><head><script type="application/ld+json">
        {"@type":"Product","name":"Widget","gtin13":"4006381333932","offers":{"price":"5.00"}}
        </script></head><body><h1>Widget</h1></body></html>"#;
    let result = Extractor::default()
        .extract_html(html, "https://w.example/p/1", &product_schema(), None)
        .unwrap();
    assert_eq!(result.data["gtin"], Value::Null);
    assert_eq!(result.data["price"], 5.0);
    assert!(result.low_confidence.contains(&"gtin".to_string()));
}

#[test]
fn embedded_app_data_is_parsed_without_running_javascript() {
    let next = r#"<html><body><div id="__next"></div>
        <script id="__NEXT_DATA__" type="application/json">
        {"props":{"pageProps":{"product":{"title":"Lamp","price":{"amount":"24.50","currency":"EUR"},"sku":"LMP-1"}}}}
        </script></body></html>"#;
    let schema = json!({"type":"object","properties":{
        "name":{"type":"string"},"price":{"type":"number"},"sku":{"type":"string"}}});
    let result = Extractor::default()
        .extract_html(next, "https://lamps.example/p/lamp", &schema, None)
        .unwrap();
    assert_eq!(
        result.data,
        json!({"name":"Lamp","price":24.5,"sku":"LMP-1"})
    );
    assert_eq!(result.fields["sku"].source, Source::Embedded);

    let window = r#"<html><body><script>
        window.__INITIAL_STATE__ = {"item": {"name": "Desk", "price": 199, "sku": "DSK-9"}};
        </script></body></html>"#;
    let result = Extractor::default()
        .extract_html(window, "https://desks.example/d/9", &schema, None)
        .unwrap();
    assert_eq!(
        result.data,
        json!({"name":"Desk","price":199.0,"sku":"DSK-9"})
    );
}

#[test]
fn microdata_and_rdfa_are_read() {
    let microdata = r#"<html><body><div itemscope itemtype="https://schema.org/Product">
        <h1 itemprop="name">Kettle</h1>
        <div itemprop="offers" itemscope itemtype="https://schema.org/Offer">
          <span itemprop="price" content="35.00">$35</span><meta itemprop="priceCurrency" content="USD">
        </div></div></body></html>"#;
    let schema = json!({"type":"object","properties":{
        "name":{"type":"string"},"price":{"type":"number"},"currency":{"type":"string"}}});
    let result = Extractor::default()
        .extract_html(microdata, "https://k.example/k", &schema, None)
        .unwrap();
    assert_eq!(
        result.data,
        json!({"name":"Kettle","price":35.0,"currency":"USD"})
    );
    assert_eq!(result.fields["name"].source, Source::Microdata);

    let rdfa = r#"<html><body><div vocab="https://schema.org/" typeof="Product">
        <h1 property="name">Toaster</h1>
        <div property="offers" typeof="Offer"><span property="price" content="49.99">$49.99</span>
        <span property="priceCurrency" content="GBP"></span></div></div></body></html>"#;
    let result = Extractor::default()
        .extract_html(rdfa, "https://t.example/t", &schema, None)
        .unwrap();
    assert_eq!(
        result.data,
        json!({"name":"Toaster","price":49.99,"currency":"GBP"})
    );
    assert_eq!(result.fields["name"].source, Source::Rdfa);
}

#[test]
fn schema_validation_reports_missing_required_fields() {
    let html = "<html><body><h1>Just a heading</h1><p>No price here.</p></body></html>";
    let result = Extractor::default()
        .extract_html(html, "https://x.example/a", &product_schema(), None)
        .unwrap();
    assert!(!result.valid);
    assert!(
        result.errors.iter().any(|e| e.contains("price")),
        "{:?}",
        result.errors
    );
    assert_eq!(result.data["name"], "Just a heading");

    assert!(Extractor::default()
        .extract_html(
            html,
            "https://x.example/a",
            &json!({"type": "string"}),
            None
        )
        .is_err());
}

async fn mount_shop(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(server)
        .await;
    for (route, file) in [
        ("/packs", "listing.html"),
        ("/packs/trailhead-40", "product.html"),
        ("/packs/ridge-30", "product_sibling.html"),
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_raw(fixture(file), "text/html"))
            .mount(server)
            .await;
    }
}

#[tokio::test]
async fn extract_over_a_crawl_applies_learned_rules_to_every_page_of_a_template() {
    let server = MockServer::start().await;
    mount_shop(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let scraper = Scraper::new(ScraperConfig {
        allowed_private_hosts: vec![server.address().to_string()],
        db_path: Some(dir.path().join("shop.db")),
        ..Default::default()
    })
    .unwrap();

    // Crawl the listing; the sibling is discovered before the JSON-LD page in link order.
    crawl_site_with_id(
        &scraper,
        &format!("{}/packs", server.uri()),
        &CrawlOptions {
            sitemap: SitemapMode::Skip,
            include_paths: vec!["/packs/*".into()],
            ..Default::default()
        },
        &mut MemorySink::default(),
        &CrawlControl::default(),
        Some("shop_crawl"),
    )
    .await
    .unwrap();

    let results = extract(
        &scraper,
        &ExtractRequest {
            crawl_id: Some("shop_crawl".into()),
            schema: product_schema(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let ridge = results
        .iter()
        .find(|r| r.url.ends_with("/packs/ridge-30"))
        .expect("ridge page extracted");
    assert_eq!(ridge.data["price"], 99.5);
    assert_eq!(ridge.data["name"], "Ridge 30L Backpack");
    assert_eq!(ridge.fields["price"].source, Source::Learned);

    let single = extract(
        &scraper,
        &ExtractRequest {
            url: Some(format!("{}/packs/trailhead-40", server.uri())),
            schema: product_schema(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(single.len(), 1);
    assert_eq!(single[0].data["sku"], "TH-40-GRN");
}
