//! Background crawl jobs: start a crawl, poll its progress and documents page by page, cancel
//! it. Jobs live in memory and, when the scraper has a database, in the `content_crawls`
//! table so their status and documents outlive the process.

use crate::error::{SeoError, SeoResult};
use crate::extract::crawl::{crawl_site_with_id, CrawlControl, CrawlOptions, CrawlState};
use crate::extract::paths::PathFilter;
use crate::extract::scrape::Scraper;
use crate::extract::sink::SharedSink;
use crate::extract::types::{unix_now, PageDocument};
use crate::storage::documents::{
    create_content_crawl, documents_for_crawl, get_content_crawl, update_content_crawl,
};
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// One page of a job's status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrawlStatus {
    /// Job id.
    pub id: String,
    /// Start URL.
    pub url: String,
    /// Current state.
    pub state: CrawlState,
    /// Pages scraped with status `ok`.
    pub pages_done: u32,
    /// Pages that failed or were blocked.
    pub pages_failed: u32,
    /// URLs skipped by robots.txt.
    pub pages_skipped: u32,
    /// Distinct URLs queued so far (0 when read back from storage).
    pub discovered: u32,
    /// Documents available so far.
    pub total: usize,
    /// Documents `offset..offset + limit`.
    pub documents: Vec<PageDocument>,
    /// Offset of the next page, if more documents are available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<usize>,
    /// Error that stopped the crawl.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Unix seconds.
    pub created_at: u64,
}

struct Job {
    url: String,
    control: Arc<CrawlControl>,
    docs: Arc<Mutex<Vec<PageDocument>>>,
    created_at: u64,
}

/// Registry of background crawls sharing one [`Scraper`].
pub struct CrawlJobs {
    scraper: Arc<Scraper>,
    jobs: Mutex<HashMap<String, Arc<Job>>>,
}

impl std::fmt::Debug for CrawlJobs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrawlJobs").finish_non_exhaustive()
    }
}

static JOB_COUNTER: AtomicU64 = AtomicU64::new(0);

fn new_job_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let n = JOB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mixed = crate::core::url::url_hash(&format!("{nanos}:{n}:{}", std::process::id()));
    format!("crawl_{mixed:016x}")
}

impl CrawlJobs {
    /// Creates an empty registry.
    pub fn new(scraper: Arc<Scraper>) -> Self {
        Self {
            scraper,
            jobs: Mutex::new(HashMap::new()),
        }
    }

    /// The scraper jobs run with.
    pub fn scraper(&self) -> &Arc<Scraper> {
        &self.scraper
    }

    /// Validates the request and starts the crawl on the Tokio runtime. Returns the job id.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid URL or invalid path patterns.
    pub fn start(&self, url: &str, opts: CrawlOptions) -> SeoResult<String> {
        url::Url::parse(url).map_err(|e| SeoError::Url(format!("Invalid URL '{url}': {e}")))?;
        PathFilter::new(&opts.include_paths, &opts.exclude_paths)?;

        let id = new_job_id();
        let job = Arc::new(Job {
            url: url.to_string(),
            control: Arc::new(CrawlControl::default()),
            docs: Arc::new(Mutex::new(Vec::new())),
            created_at: unix_now(),
        });
        if let Some(db) = self.scraper.database() {
            let conn = db.connect()?;
            create_content_crawl(&conn, &id, url, &serde_json::to_string(&opts)?)?;
        }
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.insert(id.clone(), job.clone());
        }

        let scraper = self.scraper.clone();
        let job_id = id.clone();
        tokio::spawn(async move {
            let mut sink = SharedSink {
                docs: job.docs.clone(),
            };
            persist(&scraper, &job_id, &job.control);
            let _ = crawl_site_with_id(
                &scraper,
                &job.url,
                &opts,
                &mut sink,
                &job.control,
                Some(&job_id),
            )
            .await;
            persist(&scraper, &job_id, &job.control);
        });
        Ok(id)
    }

    /// Status and documents `offset..offset + limit` of a job, from memory or storage.
    /// Returns `None` for an unknown id.
    pub fn status(&self, id: &str, offset: usize, limit: usize) -> Option<CrawlStatus> {
        let job = self.jobs.lock().ok().and_then(|j| j.get(id).cloned());
        if let Some(job) = job {
            let progress = job.control.progress();
            let (documents, total) = match job.docs.lock() {
                Ok(docs) => (
                    docs.iter()
                        .skip(offset)
                        .take(limit)
                        .cloned()
                        .collect::<Vec<_>>(),
                    docs.len(),
                ),
                Err(_) => (Vec::new(), 0),
            };
            return Some(CrawlStatus {
                id: id.to_string(),
                url: job.url.clone(),
                state: progress.state,
                pages_done: progress.pages_done,
                pages_failed: progress.pages_failed,
                pages_skipped: progress.pages_skipped,
                discovered: progress.discovered,
                next: next_offset(offset, documents.len(), total),
                total,
                documents,
                error: progress.error,
                created_at: job.created_at,
            });
        }

        let conn = self.scraper.database()?.connect().ok()?;
        let record = get_content_crawl(&conn, id).ok()??;
        let (mut documents, total) = documents_for_crawl(&conn, id, offset, limit).ok()?;
        if let Ok(opts) = serde_json::from_str::<CrawlOptions>(&record.options_json) {
            for doc in &mut documents {
                doc.apply_formats(&opts.scrape);
            }
        }
        Some(CrawlStatus {
            id: record.id,
            url: record.target_url,
            state: CrawlState::parse(&record.status),
            pages_done: record.pages_done,
            pages_failed: record.pages_failed,
            pages_skipped: record.pages_skipped,
            discovered: 0,
            next: next_offset(offset, documents.len(), total),
            total,
            documents,
            error: record.error,
            created_at: record.created_at,
        })
    }

    /// Jobs in this process that have not finished yet.
    pub fn active_count(&self) -> usize {
        self.jobs
            .lock()
            .map(|jobs| {
                jobs.values()
                    .filter(|job| !job.control.progress().state.is_terminal())
                    .count()
            })
            .unwrap_or(0)
    }

    /// Requests cancellation. Returns `false` for an unknown or already finished job.
    pub fn cancel(&self, id: &str) -> bool {
        let job = self.jobs.lock().ok().and_then(|j| j.get(id).cloned());
        match job {
            Some(job) if !job.control.progress().state.is_terminal() => {
                job.control.cancel();
                true
            }
            _ => false,
        }
    }
}

fn next_offset(offset: usize, returned: usize, total: usize) -> Option<usize> {
    let next = offset + returned;
    (returned > 0 && next < total).then_some(next)
}

fn persist(scraper: &Scraper, id: &str, control: &CrawlControl) {
    let Some(db) = scraper.database() else {
        return;
    };
    let Ok(conn) = db.connect() else {
        return;
    };
    let p = control.progress();
    let state = if p.state == CrawlState::Queued {
        CrawlState::Crawling
    } else {
        p.state
    };
    let _ = update_content_crawl(
        &conn,
        id,
        state.as_str(),
        p.pages_done,
        p.pages_failed,
        p.pages_skipped,
        p.error.as_deref(),
    );
}
