//! Phase 5: browser pool, rendering, interaction and the network guard inside Chrome.
//! These tests need a local Chrome or Chromium, like the existing render tests.

use base64::Engine;
use blacksparrow::crawler::render_pool::{RenderPool, RenderPoolConfig, RenderRequest};
use blacksparrow::extract::interact::{interact, InteractRequest};
use blacksparrow::extract::scrape::{Scraper, ScraperConfig};
use blacksparrow::extract::{BrowserAction, OutputFormat, PageStatus, RenderMode, ScrapeOptions};
use std::time::{Duration, Instant};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn page(server: &MockServer, route: &str, html: &str) {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_raw(html.to_string(), "text/html"))
        .mount(server)
        .await;
}

async fn site() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    server
}

fn scraper_for(server: &MockServer) -> Scraper {
    Scraper::new(ScraperConfig {
        allowed_private_hosts: vec![server.address().to_string()],
        ..Default::default()
    })
    .unwrap()
}

const LOAD_MORE: &str = r#"<!doctype html><html><head><title>Items</title></head><body><main>
<h1>Items</h1><ul id="items"><li>Item 1</li><li>Item 2</li><li>Item 3</li></ul>
<button id="more" onclick="(function(){const ul=document.getElementById('items');
for(let i=4;i<=6;i++){const li=document.createElement('li');li.textContent='Item '+i;ul.appendChild(li);}})()">Load more</button>
<input id="q" placeholder="Search">
<p id="echo"></p>
<script>document.getElementById('q').addEventListener('keydown', e => {
  if (e.key === 'Enter') document.getElementById('echo').textContent = 'You searched for ' + e.target.value;
});</script>
</main></body></html>"#;

#[tokio::test]
async fn auto_mode_renders_an_app_shell() {
    let server = site().await;
    page(
        &server,
        "/app",
        include_str!("fixtures/agent/spa_shell.html"),
    )
    .await;
    let doc = scraper_for(&server)
        .scrape(&format!("{}/app", server.uri()), &ScrapeOptions::default())
        .await
        .unwrap();
    assert_eq!(doc.status, PageStatus::Ok, "{:?}", doc.error);
    assert_eq!(doc.source, "rendered");
    assert!(
        doc.markdown.contains("# Rendered dashboard"),
        "{}",
        doc.markdown
    );
    assert!(doc
        .markdown
        .contains("This text only exists after JavaScript runs."));
}

#[tokio::test]
async fn snapshot_references_drive_clicks_typing_and_key_presses() {
    let server = site().await;
    page(&server, "/items", LOAD_MORE).await;
    let scraper = scraper_for(&server);
    let url = format!("{}/items", server.uri());

    let first = interact(
        &scraper,
        &InteractRequest {
            url: url.clone(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let button = first
        .snapshot
        .iter()
        .find(|e| e.role == "button" && e.name == "Load more")
        .expect("snapshot lists the button");
    assert!(first.snapshot.iter().any(|e| e.role == "textbox"));
    assert!(!first.document.markdown.contains("Item 6"));

    let clicked = interact(
        &scraper,
        &InteractRequest {
            url: url.clone(),
            actions: vec![
                BrowserAction::Click {
                    target: button.reference.clone(),
                },
                BrowserAction::Type {
                    target: "#q".into(),
                    text: "tents".into(),
                },
                BrowserAction::Press {
                    key: "Enter".into(),
                },
            ],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        clicked.action_errors.is_empty(),
        "{:?}",
        clicked.action_errors
    );
    assert!(
        clicked.document.markdown.contains("Item 6"),
        "{}",
        clicked.document.markdown
    );
    assert!(clicked.document.markdown.contains("You searched for tents"));

    let broken = interact(
        &scraper,
        &InteractRequest {
            url,
            actions: vec![BrowserAction::Click {
                target: "#does-not-exist".into(),
            }],
            timeout_ms: Some(3_000),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(broken.action_errors.len(), 1);
}

#[tokio::test]
async fn chrome_requests_to_private_addresses_are_blocked() {
    let server = site().await;
    // A second local server that is NOT on the allowlist stands in for an internal service.
    let internal = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("access-control-allow-origin", "*")
                .set_body_raw("<p>INTERNAL SECRET</p>", "text/html"),
        )
        .mount(&internal)
        .await;
    Mock::given(method("GET"))
        .and(path("/jump"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/admin", internal.uri())),
        )
        .mount(&server)
        .await;
    page(
        &server,
        "/leaky",
        &format!(
            r#"<html><body><main><h1>Leaky</h1><p id="out">waiting</p><script>
            fetch('{}/admin').then(r => r.text()).then(t => document.getElementById('out').textContent = t)
              .catch(() => document.getElementById('out').textContent = 'fetch blocked');
            </script></main></body></html>"#,
            internal.uri()
        ),
    )
    .await;

    let pool = RenderPool::launch(RenderPoolConfig {
        allowed_private_hosts: vec![server.address().to_string()],
        ..Default::default()
    })
    .await
    .unwrap();

    let leaky = pool
        .render(
            &format!("{}/leaky", server.uri()),
            &RenderRequest {
                wait_for_selector: Some("#out:not(:empty)".into()),
                wait_ms: Some(300),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(!leaky.html.contains("INTERNAL SECRET"));
    assert!(leaky.html.contains("fetch blocked"), "{}", leaky.html);
    assert!(leaky.blocked_requests.iter().any(|u| u.contains("/admin")));

    let jumped = pool
        .render(&format!("{}/jump", server.uri()), &RenderRequest::default())
        .await;
    match jumped {
        Ok(output) => {
            assert!(!output.html.contains("INTERNAL SECRET"));
            assert!(output.blocked_requests.iter().any(|u| u.contains("/admin")));
        }
        Err(e) => assert!(e.to_string().contains("load") || e.to_string().contains("Chrome")),
    }

    // The starting URL itself is checked before Chrome is involved.
    assert!(pool
        .render(
            "http://169.254.169.254/latest/meta-data",
            &RenderRequest::default()
        )
        .await
        .is_err());
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn css_hidden_text_is_removed_from_rendered_pages() {
    let server = site().await;
    page(
        &server,
        "/styled",
        r#"<html><head><style>
            .gone { display: none } .ghost { opacity: 0 } .away { position: absolute; left: -9999px }
            .tiny { font-size: 0 }
        </style></head><body><main><h1>Styled</h1>
        <p>Visible paragraph.</p>
        <p class="gone">Hidden by a stylesheet.</p>
        <p class="ghost">Transparent instruction.</p>
        <p class="away">Off-screen instruction.</p>
        <p class="tiny">Zero-size instruction.</p>
        <div id="late"></div>
        <script>const p = document.createElement('p'); p.textContent = 'Injected but hidden';
          p.style.visibility = 'hidden'; document.getElementById('late').appendChild(p);</script>
        </main></body></html>"#,
    )
    .await;
    let doc = scraper_for(&server)
        .scrape(
            &format!("{}/styled", server.uri()),
            &ScrapeOptions {
                render: RenderMode::Always,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(doc.source, "rendered");
    assert!(doc.markdown.contains("Visible paragraph."));
    for hidden in [
        "Hidden by a stylesheet",
        "Transparent instruction",
        "Off-screen instruction",
        "Zero-size instruction",
        "Injected but hidden",
    ] {
        assert!(
            !doc.markdown.contains(hidden),
            "{hidden:?} leaked:\n{}",
            doc.markdown
        );
    }
}

#[tokio::test]
async fn pages_render_in_parallel_tabs() {
    let server = site().await;
    for i in 0..3 {
        page(
            &server,
            &format!("/p{i}"),
            &format!("<html><body><main><h1>Page {i}</h1></main></body></html>"),
        )
        .await;
    }
    let pool = RenderPool::launch(RenderPoolConfig {
        max_tabs: 3,
        allowed_private_hosts: vec![server.address().to_string()],
        ..Default::default()
    })
    .await
    .unwrap();
    // Warm up so browser start-up is not timed.
    pool.render(&format!("{}/p0", server.uri()), &RenderRequest::default())
        .await
        .unwrap();

    let wait = Duration::from_millis(1_500);
    let request = RenderRequest {
        wait_ms: Some(wait.as_millis() as u64),
        ..Default::default()
    };
    let urls: Vec<String> = (0..3).map(|i| format!("{}/p{i}", server.uri())).collect();
    let outputs = futures::future::join_all(urls.iter().map(|u| async {
        let out = pool.render(u, &request).await;
        (out, Instant::now())
    }))
    .await;
    let mut finished = Vec::new();
    for (i, (out, at)) in outputs.into_iter().enumerate() {
        assert!(out.unwrap().html.contains(&format!("Page {i}")));
        finished.push(at);
    }
    // One tab at a time would finish the renders at least `wait` apart (2 × wait from first
    // to last). Comparing finish times instead of total time keeps the check meaningful on
    // slow, busy CI machines.
    let first = finished.iter().min().copied().unwrap();
    let last = finished.iter().max().copied().unwrap();
    let spread = last - first;
    assert!(
        spread < wait * 2,
        "renders finished {spread:?} apart; tabs are not running in parallel"
    );
    pool.shutdown().await.unwrap();
}

#[tokio::test]
async fn screenshots_are_full_page_pngs() {
    let server = site().await;
    page(
        &server,
        "/shot",
        "<html><body><main><h1>Shot</h1><p>Picture me.</p></main></body></html>",
    )
    .await;
    let doc = scraper_for(&server)
        .scrape(
            &format!("{}/shot", server.uri()),
            &ScrapeOptions {
                formats: vec![OutputFormat::Markdown, OutputFormat::Screenshot],
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let png = base64::engine::general_purpose::STANDARD
        .decode(doc.screenshot.expect("screenshot requested"))
        .unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(doc.markdown.contains("Picture me."));
}
