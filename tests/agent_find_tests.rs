//! Phase 3: find passages by question, CSS selector or regex, on one page or a whole crawl.

use blacksparrow::extract::find::{
    find_in_crawl, find_in_document, find_on_page, regex_in_document, select_in_html, FindOptions,
    FindRequest,
};
use blacksparrow::extract::scrape::{Scraper, ScraperConfig};
use blacksparrow::extract::synonyms::expand_term;
use blacksparrow::extract::{html_to_document, ScrapeOptions};
use blacksparrow::storage::documents::save_document;
use blacksparrow::storage::Database;
use serde::Deserialize;
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

fn url_for(file: &str) -> String {
    #[derive(Deserialize)]
    struct Entry {
        file: String,
        url: String,
    }
    let corpus: Vec<Entry> = serde_json::from_str(&fixture("corpus.json")).unwrap();
    corpus.into_iter().find(|e| e.file == file).unwrap().url
}

#[derive(Deserialize)]
struct Question {
    file: String,
    question: String,
    expected: String,
}

#[test]
fn labelled_questions_find_the_right_passage_in_the_top_three() {
    let questions: Vec<Question> = serde_json::from_str(&fixture("questions.json")).unwrap();
    let mut misses = Vec::new();
    for q in &questions {
        let doc = html_to_document(&fixture(&q.file), &url_for(&q.file), &ScrapeOptions::default())
            .unwrap();
        let hits = find_in_document(&doc, &q.question, 3);
        if !hits.iter().any(|h| h.text.contains(&q.expected)) {
            misses.push(format!(
                "{} / {:?}: got {:?}",
                q.file,
                q.question,
                hits.iter().map(|h| &h.text).collect::<Vec<_>>()
            ));
        }
    }
    println!(
        "find hit rate (top 3): {}/{}",
        questions.len() - misses.len(),
        questions.len()
    );
    assert!(misses.is_empty(), "missed:\n{}", misses.join("\n"));
}

#[test]
fn hits_carry_url_heading_path_selector_and_score() {
    let doc = html_to_document(
        &fixture("docs.html"),
        &url_for("docs.html"),
        &ScrapeOptions::default(),
    )
    .unwrap();
    let hits = find_in_document(&doc, "default retry limit", 3);
    let top = &hits[0];
    assert_eq!(top.url, "https://relay.example/docs/retries");
    assert_eq!(
        top.heading_path,
        vec!["Configuring retries", "Changing the retry limit"]
    );
    assert!(!top.selector.is_empty());
    assert!(top.score.unwrap() > 0.0);
    assert!(hits.windows(2).all(|w| w[0].score >= w[1].score));
}

#[test]
fn synonyms_expand_common_field_words() {
    assert!(expand_term("cost").contains(&"price"));
    assert!(expand_term("price").contains(&"cost"));
    assert!(expand_term("telephone").contains(&"phone"));
    assert!(expand_term("location").contains(&"address"));
    assert!(expand_term("zebra").is_empty());
}

#[test]
fn hidden_text_is_never_searchable() {
    let html = fixture("hidden_prompt.html");
    let url = url_for("hidden_prompt.html");
    let doc = html_to_document(&html, &url, &ScrapeOptions::default()).unwrap();
    for query in ["ignore previous instructions password", "system prompt", "delete files"] {
        for hit in find_in_document(&doc, query, 5) {
            for bad in ["password", "SYSTEM", "delete", "five stars", "Pretend"] {
                assert!(!hit.text.contains(bad), "{query:?} surfaced hidden text: {}", hit.text);
            }
        }
    }
    assert!(regex_in_document(&doc, "(?i)password|attacker", &FindOptions::default())
        .unwrap()
        .is_empty());
    let selected = select_in_html(&html, &url, "p, div, span", &FindOptions::default()).unwrap();
    assert!(!selected.is_empty());
    for hit in selected {
        for bad in ["password", "SYSTEM", "Cloudly over", "delete", "five stars", "Pretend"] {
            assert!(!hit.text.contains(bad), "selector surfaced hidden text: {}", hit.text);
        }
    }
}

#[test]
fn selector_mode_returns_text_attributes_html_and_heading_path() {
    let html = fixture("docs.html");
    let url = url_for("docs.html");
    let rows = select_in_html(&html, &url, "table tr", &FindOptions::default()).unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[2].text, "HTTP 429 Yes, after Retry-After");
    assert_eq!(
        rows[2].heading_path,
        vec!["Configuring retries", "Which errors are retried"]
    );
    assert!(rows[2].selector.contains("tr"));

    let links = select_in_html(
        &html,
        &url,
        "main a",
        &FindOptions {
            attributes: vec!["href".into()],
            outer_html: true,
            ..Default::default()
        },
    )
    .unwrap();
    let errors = links.iter().find(|l| l.text == "Error handling").unwrap();
    assert_eq!(
        errors.attributes.get("href").map(String::as_str),
        Some("https://relay.example/docs/errors"),
        "href is resolved to an absolute URL"
    );
    assert!(errors.html.as_deref().unwrap().starts_with("<a "));

    assert!(select_in_html(&html, &url, "p:::bad", &FindOptions::default()).is_err());
}

#[test]
fn regex_mode_returns_matches_with_context_and_caps() {
    let doc = html_to_document(
        &fixture("listing.html"),
        &url_for("listing.html"),
        &ScrapeOptions::default(),
    )
    .unwrap();
    let hits = regex_in_document(&doc, r"\$\d+\.\d{2}", &FindOptions::default()).unwrap();
    let prices: Vec<&str> = hits.iter().map(|h| h.text.as_str()).collect();
    assert_eq!(prices, vec!["$129.00", "$99.50", "$189.00", "$59.00"]);
    assert!(hits[0].snippet.as_deref().unwrap().contains("Trailhead"));

    let capped = regex_in_document(
        &doc,
        r"\$\d+\.\d{2}",
        &FindOptions {
            max_matches: 2,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(capped.len(), 2);
    assert!(regex_in_document(&doc, "(unclosed", &FindOptions::default()).is_err());
}

#[test]
fn crawl_wide_search_uses_full_text_index_with_filters() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join("find.db")).unwrap();
    let conn = db.connect().unwrap();
    for file in ["article.html", "docs.html", "hidden_prompt.html", "product.html"] {
        let doc =
            html_to_document(&fixture(file), &url_for(file), &ScrapeOptions::default()).unwrap();
        save_document(&conn, Some("crawl_a"), &doc).unwrap();
    }

    let hits = find_in_crawl(
        &conn,
        &FindRequest {
            crawl_id: Some("crawl_a".into()),
            query: Some("how much does the team plan cost".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(hits[0].url, "https://cloudly.example/pricing", "{hits:?}");
    assert!(hits[0].text.contains("$20 per user"));
    assert!(hits[0].snippet.as_deref().unwrap().contains("**"));

    let filtered = find_in_crawl(
        &conn,
        &FindRequest {
            crawl_id: Some("crawl_a".into()),
            query: Some("retries".into()),
            url_prefix: Some("https://coastal.example/".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(filtered.is_empty());

    let regex = find_in_crawl(
        &conn,
        &FindRequest {
            crawl_id: Some("crawl_a".into()),
            regex: Some(r"\d+ °C".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(regex.len(), 2);
    assert!(regex
        .iter()
        .all(|h| h.url == "https://coastal.example/articles/tide-pools"));
}

#[tokio::test]
async fn find_on_page_fetches_then_searches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/docs/retries"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(fixture("docs.html"), "text/html"))
        .mount(&server)
        .await;
    let scraper = Scraper::new(ScraperConfig {
        allowed_private_hosts: vec![server.address().to_string()],
        ..Default::default()
    })
    .unwrap();
    let url = format!("{}/docs/retries", server.uri());

    let by_query = find_on_page(
        &scraper,
        &FindRequest {
            url: Some(url.clone()),
            query: Some("which errors are retried".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(by_query.hits[0].text.contains("Connection reset"));
    assert_eq!(by_query.mode, "query");

    let by_selector = find_on_page(
        &scraper,
        &FindRequest {
            url: Some(url.clone()),
            selector: Some("pre code".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(by_selector.hits.len(), 1);
    assert!(by_selector.hits[0].text.contains("max_retries(5)"));

    let none = find_on_page(
        &scraper,
        &FindRequest {
            url: Some(url),
            ..Default::default()
        },
    )
    .await;
    assert!(none.is_err(), "one of query, selector or regex is required");
}
