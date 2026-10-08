//! Phase 2: map and content crawl over a 30-page mock site.

use blacksparrow::extract::crawl::{crawl_site, CrawlControl, CrawlOptions, CrawlState};
use blacksparrow::extract::jobs::CrawlJobs;
use blacksparrow::extract::map::{map_site, MapOptions};
use blacksparrow::extract::paths::PathFilter;
use blacksparrow::extract::scrape::{Scraper, ScraperConfig};
use blacksparrow::extract::sink::{DirSink, MemorySink, NdjsonSink, PageSink};
use blacksparrow::extract::{PageDocument, PageStatus, ScrapeOptions};
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BLOG_POSTS: usize = 20;
const DOC_PAGES: usize = 9;

fn page(title: &str, body: &str, links: &[(String, String)]) -> String {
    let nav: String = links
        .iter()
        .map(|(href, text)| format!("<li><a href=\"{href}\">{text}</a></li>"))
        .collect();
    format!(
        "<!doctype html><html lang=\"en\"><head><title>{title}</title></head><body>\
         <nav><ul>{nav}</ul></nav>\
         <main><h1>{title}</h1>\
         <p>{body}</p>\
         <div class=\"note\"><p>Free shipping on every order this week, no code needed.</p></div>\
         </main><footer>Copyright Example Co</footer></body></html>"
    )
}

/// Mounts a 30-page site: `/`, 20 blog posts, 9 docs pages, robots.txt and a sitemap.
/// Docs pages are only reachable through the sitemap; `/private/` is disallowed.
async fn mount_site(server: &MockServer) {
    let mut home_links: Vec<(String, String)> = (1..=BLOG_POSTS)
        .map(|i| (format!("/blog/post-{i}"), format!("Blog post {i}")))
        .collect();
    home_links.push(("/private/secret".into(), "Secret".into()));
    home_links.push(("/pricing-plans".into(), "Pricing".into()));
    home_links.push(("/logo.png".into(), "Logo".into()));
    home_links.push(("https://elsewhere.example/".into(), "Elsewhere".into()));
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            page("Home", "Welcome to the example site.", &home_links),
            "text/html",
        ))
        .mount(server)
        .await;
    for i in 1..=BLOG_POSTS {
        let body = format!("Post number {i} talks about topic {i} in detail.");
        Mock::given(method("GET"))
            .and(path(format!("/blog/post-{i}")))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                page(
                    &format!("Blog post {i}"),
                    &body,
                    &[("/".into(), "Home".into())],
                ),
                "text/html",
            ))
            .mount(server)
            .await;
    }
    for i in 1..=DOC_PAGES {
        let body = format!("Documentation page {i} explains setting {i}.");
        Mock::given(method("GET"))
            .and(path(format!("/docs/page-{i}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(page(&format!("Docs {i}"), &body, &[]), "text/html"),
            )
            .mount(server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/pricing-plans"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            page("Pricing", "Plans start at ten dollars.", &[]),
            "text/html",
        ))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/private/secret"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("secret", "text/html"))
        .expect(0)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            format!(
                "User-agent: *\nDisallow: /private/\nSitemap: {}/sitemap.xml\n",
                server.uri()
            ),
            "text/plain",
        ))
        .mount(server)
        .await;
    let urls: String = (1..=DOC_PAGES)
        .map(|i| format!("<url><loc>{}/docs/page-{i}</loc></url>", server.uri()))
        .collect();
    Mock::given(method("GET"))
        .and(path("/sitemap.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            format!(
                "<?xml version=\"1.0\"?><urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">{urls}</urlset>"
            ),
            "application/xml",
        ))
        .mount(server)
        .await;
}

fn scraper_for(server: &MockServer, db: Option<std::path::PathBuf>) -> Scraper {
    Scraper::new(ScraperConfig {
        allowed_private_hosts: vec![server.address().to_string()],
        db_path: db,
        ..Default::default()
    })
    .unwrap()
}

fn paths_of(docs: &[PageDocument]) -> Vec<String> {
    let mut paths: Vec<String> = docs
        .iter()
        .map(|d| url::Url::parse(&d.url).unwrap().path().to_string())
        .collect();
    paths.sort();
    paths
}

async fn crawl(server: &MockServer, opts: CrawlOptions) -> (Vec<PageDocument>, CrawlControl) {
    let scraper = scraper_for(server, None);
    let mut sink = MemorySink::default();
    let control = CrawlControl::default();
    crawl_site(
        &scraper,
        &format!("{}/", server.uri()),
        &opts,
        &mut sink,
        &control,
    )
    .await
    .unwrap();
    (sink.docs, control)
}

#[test]
fn path_filters_accept_globs_and_regexes() {
    let f = PathFilter::new(&["/blog/**".into()], &["/blog/drafts/*".into()]).unwrap();
    assert!(f.allows("https://a.example/blog/post-1"));
    assert!(!f.allows("https://a.example/blog/drafts/x"));
    assert!(!f.allows("https://a.example/docs/page-1"));

    // Firecrawl-style regex patterns are recognised.
    let f = PathFilter::new(&["blog/.*".into()], &[]).unwrap();
    assert!(f.allows("https://a.example/blog/post-1"));
    assert!(!f.allows("https://a.example/docs/page-1"));

    let f = PathFilter::new(&[], &["re:^/docs/page-[2-9]$".into()]).unwrap();
    assert!(f.allows("https://a.example/docs/page-1"));
    assert!(!f.allows("https://a.example/docs/page-2"));

    assert!(PathFilter::new(&["re:(".into()], &[]).is_err());
}

#[tokio::test]
async fn crawl_follows_links_and_sitemap_and_respects_robots() {
    let server = MockServer::start().await;
    mount_site(&server).await;
    let (docs, control) = crawl(&server, CrawlOptions::default()).await;

    let paths = paths_of(&docs);
    assert_eq!(docs.len(), 1 + BLOG_POSTS + DOC_PAGES + 1, "{paths:?}");
    assert!(
        paths.contains(&"/docs/page-9".to_string()),
        "sitemap URLs are crawled"
    );
    assert!(paths.contains(&"/pricing-plans".to_string()));
    assert!(!paths.iter().any(|p| p.starts_with("/private")));
    assert!(!paths.iter().any(|p| p.ends_with(".png")));
    assert!(docs.iter().all(|d| d.status == PageStatus::Ok));
    assert!(docs.iter().all(|d| !d.url.contains("elsewhere")));

    let progress = control.progress();
    assert_eq!(progress.state, CrawlState::Completed);
    assert_eq!(progress.pages_done as usize, docs.len());
    assert!(
        progress.pages_skipped >= 1,
        "robots-disallowed URL counted as skipped"
    );
}

#[tokio::test]
async fn crawl_limit_depth_and_path_filters() {
    let server = MockServer::start().await;
    mount_site(&server).await;

    let (docs, _) = crawl(
        &server,
        CrawlOptions {
            limit: 5,
            ..Default::default()
        },
    )
    .await;
    assert_eq!(docs.len(), 5);

    let (docs, _) = crawl(
        &server,
        CrawlOptions {
            include_paths: vec!["/blog/**".into()],
            ..Default::default()
        },
    )
    .await;
    let paths = paths_of(&docs);
    assert_eq!(docs.len(), BLOG_POSTS + 1, "{paths:?}");
    assert!(paths.iter().all(|p| p == "/" || p.starts_with("/blog/")));

    let (docs, _) = crawl(
        &server,
        CrawlOptions {
            exclude_paths: vec!["/blog/**".into()],
            ..Default::default()
        },
    )
    .await;
    assert!(paths_of(&docs).iter().all(|p| !p.starts_with("/blog/")));

    // Depth 0: only the seed and sitemap entries, no link following.
    let (docs, _) = crawl(
        &server,
        CrawlOptions {
            max_depth: 0,
            ..Default::default()
        },
    )
    .await;
    let paths = paths_of(&docs);
    assert!(!paths.iter().any(|p| p.starts_with("/blog/")), "{paths:?}");
    assert!(paths.contains(&"/docs/page-1".to_string()));
}

#[tokio::test]
async fn crawl_removes_text_repeated_across_pages() {
    let server = MockServer::start().await;
    mount_site(&server).await;

    let (docs, _) = crawl(&server, CrawlOptions::default()).await;
    for doc in &docs {
        assert!(!doc.markdown.contains("Free shipping"), "{}", doc.markdown);
    }
    let post = docs
        .iter()
        .find(|d| d.url.ends_with("/blog/post-3"))
        .unwrap();
    assert!(post.markdown.contains("Post number 3 talks about topic 3"));
    assert!(post.markdown.starts_with("# Blog post 3"));

    let (docs, _) = crawl(
        &server,
        CrawlOptions {
            dedupe_boilerplate: false,
            ..Default::default()
        },
    )
    .await;
    assert!(docs.iter().all(|d| d.markdown.contains("Free shipping")));
}

#[tokio::test]
async fn cancelled_crawl_stops_early() {
    let server = MockServer::start().await;
    mount_site(&server).await;
    let scraper = scraper_for(&server, None);
    let mut sink = MemorySink::default();
    let control = CrawlControl::default();
    control.cancel();
    crawl_site(
        &scraper,
        &format!("{}/", server.uri()),
        &CrawlOptions::default(),
        &mut sink,
        &control,
    )
    .await
    .unwrap();
    assert!(sink.docs.is_empty());
    assert_eq!(control.progress().state, CrawlState::Cancelled);
}

#[tokio::test]
async fn max_age_serves_from_cache_and_changes_are_flagged() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/news"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(page("News", "First edition.", &[]), "text/html"),
        )
        .up_to_n_times(2)
        .expect(2)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let scraper = scraper_for(&server, Some(dir.path().join("agent.db")));
    let url = format!("{}/news", server.uri());

    let first = scraper
        .scrape(&url, &ScrapeOptions::default())
        .await
        .unwrap();
    assert_eq!(first.changed, None, "first sighting has nothing to compare");
    assert!(!first.from_cache);

    let cached_opts = ScrapeOptions {
        max_age_secs: Some(3600),
        ..Default::default()
    };
    let cached = scraper.scrape(&url, &cached_opts).await.unwrap();
    assert!(cached.from_cache);
    assert_eq!(cached.markdown, first.markdown);

    let again = scraper
        .scrape(&url, &ScrapeOptions::default())
        .await
        .unwrap();
    assert!(!again.from_cache);
    assert_eq!(again.changed, Some(false), "same content is not a change");

    Mock::given(method("GET"))
        .and(path("/news"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(page("News", "Second edition.", &[]), "text/html"),
        )
        .mount(&server)
        .await;
    let updated = scraper
        .scrape(&url, &ScrapeOptions::default())
        .await
        .unwrap();
    assert_eq!(updated.changed, Some(true));
    assert!(updated.markdown.contains("Second edition"));
}

#[tokio::test]
async fn map_lists_sitemap_and_page_links_ranked_by_search() {
    let server = MockServer::start().await;
    mount_site(&server).await;
    let scraper = scraper_for(&server, None);
    let seed = format!("{}/", server.uri());

    let result = map_site(&scraper, &seed, &MapOptions::default())
        .await
        .unwrap();
    let urls: Vec<&str> = result.links.iter().map(|l| l.url.as_str()).collect();
    assert!(urls.contains(&format!("{}/docs/page-4", server.uri()).as_str()));
    assert!(urls.contains(&format!("{}/blog/post-12", server.uri()).as_str()));
    assert!(!urls.iter().any(|u| u.contains("/private/")));
    assert!(!urls.iter().any(|u| u.contains("elsewhere")));
    assert!(!urls.iter().any(|u| u.ends_with(".png")));
    let post = result
        .links
        .iter()
        .find(|l| l.url.ends_with("/blog/post-12"))
        .unwrap();
    assert_eq!(post.title.as_deref(), Some("Blog post 12"));

    let ranked = map_site(
        &scraper,
        &seed,
        &MapOptions {
            search: Some("pricing".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        ranked.links[0].url.ends_with("/pricing-plans"),
        "{:?}",
        ranked.links[0]
    );

    let limited = map_site(
        &scraper,
        &seed,
        &MapOptions {
            include_paths: vec!["/docs/*".into()],
            limit: 3,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(limited.links.len(), 3);
    assert!(limited.links.iter().all(|l| l.url.contains("/docs/")));
}

#[tokio::test]
async fn dir_and_ndjson_sinks_write_documents() {
    let server = MockServer::start().await;
    mount_site(&server).await;
    let scraper = scraper_for(&server, None);
    let out = tempfile::tempdir().unwrap();
    let opts = CrawlOptions {
        include_paths: vec!["/docs/*".into()],
        ..Default::default()
    };

    let mut dir_sink = DirSink::new(out.path()).unwrap();
    crawl_site(
        &scraper,
        &format!("{}/", server.uri()),
        &opts,
        &mut dir_sink,
        &CrawlControl::default(),
    )
    .await
    .unwrap();
    let host_dir = out
        .path()
        .join(server.address().to_string().replace(':', "_"));
    let page = std::fs::read_to_string(host_dir.join("docs/page-2.md")).unwrap();
    assert!(page.starts_with("---\nurl: "), "{page}");
    assert!(page.contains("title: \"Docs 2\""));
    assert!(page.contains("# Docs 2"));
    assert!(
        host_dir.join("index.md").exists(),
        "the seed page maps to index.md"
    );

    let mut buf = Vec::new();
    {
        let mut ndjson = NdjsonSink::new(&mut buf);
        crawl_site(
            &scraper,
            &format!("{}/", server.uri()),
            &opts,
            &mut ndjson,
            &CrawlControl::default(),
        )
        .await
        .unwrap();
        ndjson.finish().unwrap();
    }
    let lines: Vec<PageDocument> = String::from_utf8(buf)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), DOC_PAGES + 1);
}

#[tokio::test]
async fn background_jobs_report_progress_paginate_and_persist() {
    let server = MockServer::start().await;
    mount_site(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let scraper = Arc::new(scraper_for(&server, Some(dir.path().join("agent.db"))));
    let jobs = CrawlJobs::new(scraper.clone());

    let id = jobs
        .start(&format!("{}/", server.uri()), CrawlOptions::default())
        .unwrap();
    let mut status = jobs.status(&id, 0, 10).unwrap();
    for _ in 0..200 {
        if status.state.is_terminal() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        status = jobs.status(&id, 0, 10).unwrap();
    }
    assert_eq!(status.state, CrawlState::Completed);
    assert_eq!(status.total, 31);
    assert_eq!(status.documents.len(), 10);
    assert_eq!(status.next, Some(10));
    let last_page = jobs.status(&id, 30, 10).unwrap();
    assert_eq!(last_page.documents.len(), 1);
    assert_eq!(last_page.next, None);

    // A fresh registry over the same database still answers from storage.
    let reopened = CrawlJobs::new(scraper);
    let stored = reopened.status(&id, 0, 50).unwrap();
    assert_eq!(stored.state, CrawlState::Completed);
    assert_eq!(stored.total, 31);
    assert_eq!(stored.documents.len(), 31);

    assert!(jobs.status("crawl_missing", 0, 10).is_none());
}

#[tokio::test]
async fn background_jobs_can_be_cancelled() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let links: Vec<(String, String)> = (1..=50)
        .map(|i| (format!("/slow/{i}"), format!("Slow {i}")))
        .collect();
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(page("Slow home", "Hi.", &links), "text/html"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(300))
                .set_body_raw(page("Slow", "Slow page.", &[]), "text/html"),
        )
        .mount(&server)
        .await;
    let jobs = CrawlJobs::new(Arc::new(scraper_for(&server, None)));
    let id = jobs
        .start(
            &format!("{}/", server.uri()),
            CrawlOptions {
                sitemap: blacksparrow::extract::map::SitemapMode::Skip,
                concurrency: 2,
                ..Default::default()
            },
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(jobs.cancel(&id));
    let mut status = jobs.status(&id, 0, 100).unwrap();
    for _ in 0..100 {
        if status.state.is_terminal() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        status = jobs.status(&id, 0, 100).unwrap();
    }
    assert_eq!(status.state, CrawlState::Cancelled);
    assert!(status.total < 20, "stopped early, got {}", status.total);
    assert!(!jobs.cancel("crawl_missing"));
}
