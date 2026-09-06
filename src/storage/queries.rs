//! # Relational Query Execution
//!
//! Provides typed methods for session management, paginated page inspection,
//! issue filtering, and historical crawl retrieval from SQLite.

use crate::core::models::{
    CrawlSummary, DiscoveredLink, HreflangTag, ImageResource, IssueCategory, IssueFinding,
    PageIntent, PageReport, RobotsFlags, RuleId, SchemaRecord, Severity,
};
use crate::error::SeoResult;
use compact_str::CompactString;
use hashbrown::HashMap;
use rusqlite::{params, Connection};

/// Parameters for initializing a new crawl session in SQLite.
#[derive(Debug, Clone)]
pub struct CrawlSessionInit {
    /// Unique crawl session identifier.
    pub session_id: String,
    /// Seed root URL of the crawl.
    pub target_url: String,
    /// Configured maximum page limit (0 for unlimited).
    pub max_pages: u32,
    /// Maximum link traversal hop depth from the seed.
    pub max_depth: u16,
    /// Whether robots.txt directives were enforced.
    pub respect_robots: bool,
    /// Whether headless Chrome CDP browser rendering was active.
    pub render_js: bool,
}

/// Inserts a new crawl session record with initial status 'crawling'.
pub fn init_crawl_session(conn: &Connection, init: &CrawlSessionInit) -> SeoResult<()> {
    conn.execute(
        "INSERT INTO crawls (
            session_id, target_url, status, total_max_pages, max_depth,
            respect_robots, render_js, started_at
        ) VALUES (?1, ?2, 'crawling', ?3, ?4, ?5, ?6, datetime('now'))",
        params![
            init.session_id,
            init.target_url,
            init.max_pages,
            init.max_depth,
            init.respect_robots,
            init.render_js,
        ],
    )?;
    Ok(())
}

/// Updates crawl session status, counts, and health score.
#[allow(clippy::too_many_arguments)]
pub fn update_crawl_status(
    conn: &Connection,
    session_id: &str,
    status: &str,
    finished_at: Option<&str>,
    total_pages: u32,
    error_count: u32,
    alert_count: u32,
    warning_count: u32,
    health_score: Option<u8>,
) -> SeoResult<()> {
    conn.execute(
        "UPDATE crawls SET
            status = ?1,
            finished_at = COALESCE(?2, datetime('now')),
            total_pages = ?3,
            error_count = ?4,
            alert_count = ?5,
            warning_count = ?6,
            health_score = ?7
        WHERE session_id = ?8",
        params![
            status,
            finished_at,
            total_pages,
            error_count,
            alert_count,
            warning_count,
            health_score,
            session_id,
        ],
    )?;
    Ok(())
}

/// Lists all historical crawl sessions ordered by start time descending.
pub fn list_crawls(conn: &Connection) -> SeoResult<Vec<CrawlSummary>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, target_url, status, started_at, finished_at,
                total_pages, error_count, alert_count, warning_count, health_score
         FROM crawls ORDER BY started_at DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        let session_id: String = row.get(0)?;
        let target_url: String = row.get(1)?;
        let status: String = row.get(2)?;
        let started_at: String = row.get(3)?;
        let finished_at: Option<String> = row.get(4)?;
        let total_pages: u32 = row.get(5)?;
        let total_errors: u32 = row.get(6)?;
        let total_alerts: u32 = row.get(7)?;
        let total_warnings: u32 = row.get(8)?;
        let health_score: Option<u8> = row.get(9)?;

        Ok(CrawlSummary {
            session_id,
            target_url,
            status,
            started_at,
            finished_at,
            total_pages_crawled: total_pages,
            total_links_discovered: 0,
            total_errors,
            total_alerts,
            total_warnings,
            total_notices: 0,
            average_ttfb_ms: 0,
            p95_ttfb_ms: 0,
            health_score: health_score.unwrap_or(0),
        })
    })?;

    let mut list = Vec::new();
    for r in rows {
        list.push(r?);
    }
    Ok(list)
}

/// Retrieves summary metadata for a specific crawl session.
pub fn get_crawl(conn: &Connection, session_id: &str) -> SeoResult<Option<CrawlSummary>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, target_url, status, started_at, finished_at,
                total_pages, error_count, alert_count, warning_count, health_score
         FROM crawls WHERE session_id = ?1",
    )?;

    let mut rows = stmt.query_map(params![session_id], |row| {
        let sid: String = row.get(0)?;
        let target_url: String = row.get(1)?;
        let status: String = row.get(2)?;
        let started_at: String = row.get(3)?;
        let finished_at: Option<String> = row.get(4)?;
        let total_pages: u32 = row.get(5)?;
        let total_errors: u32 = row.get(6)?;
        let total_alerts: u32 = row.get(7)?;
        let total_warnings: u32 = row.get(8)?;
        let health_score: Option<u8> = row.get(9)?;

        Ok(CrawlSummary {
            session_id: sid,
            target_url,
            status,
            started_at,
            finished_at,
            total_pages_crawled: total_pages,
            total_links_discovered: 0,
            total_errors,
            total_alerts,
            total_warnings,
            total_notices: 0,
            average_ttfb_ms: 0,
            p95_ttfb_ms: 0,
            health_score: health_score.unwrap_or(0),
        })
    })?;

    if let Some(res) = rows.next() {
        Ok(Some(res?))
    } else {
        Ok(None)
    }
}

/// Retrieves paginated page reports for a session with reconstructed child collections.
pub fn get_crawl_pages(
    conn: &Connection,
    session_id: &str,
    limit: usize,
    offset: usize,
) -> SeoResult<Vec<PageReport>> {
    let mut stmt = conn.prepare(
        "SELECT id, crawl_id, url, url_hash, final_url, status_code, content_type,
                size_bytes, ttfb_ms, crawl_depth, title, title_length, meta_description,
                meta_desc_length, canonical_url, html_lang, charset, viewport,
                robots_flags, is_sitemap_url, is_internal, h1_primary, h1_count,
                h2_headings, h3_headings, word_count, content_hash, simhash,
                is_soft_404, has_lorem_ipsum, is_https, has_hsts, has_csp,
                has_x_frame, has_x_content_type, mixed_content_count, page_intent
         FROM pages
         WHERE crawl_id = ?1
         ORDER BY id ASC
         LIMIT ?2 OFFSET ?3",
    )?;

    let mut pages = Vec::new();
    let rows = stmt.query_map(params![session_id, limit as i64, offset as i64], |row| {
        let id: i64 = row.get(0)?;
        let crawl_id: String = row.get(1)?;
        let url: String = row.get(2)?;
        let _url_hash: i64 = row.get(3)?;
        let final_url: Option<String> = row.get(4)?;
        let status_code: u16 = row.get(5)?;
        let content_type: String = row.get(6)?;
        let size_bytes: u32 = row.get(7)?;
        let ttfb_ms: u32 = row.get(8)?;
        let crawl_depth: u16 = row.get(9)?;
        let title: Option<String> = row.get(10)?;
        let title_length: u16 = row.get(11)?;
        let meta_description: Option<String> = row.get(12)?;
        let meta_desc_length: u16 = row.get(13)?;
        let canonical_url: Option<String> = row.get(14)?;
        let html_lang: Option<String> = row.get(15)?;
        let charset: Option<String> = row.get(16)?;
        let viewport: Option<String> = row.get(17)?;
        let robots_flags_raw: u8 = row.get(18)?;
        let is_sitemap_url: bool = row.get(19)?;
        let is_internal: bool = row.get(20)?;
        let h1_primary: Option<String> = row.get(21)?;
        let h1_count: u16 = row.get(22)?;
        let h2_raw: String = row.get(23)?;
        let h3_raw: String = row.get(24)?;
        let word_count: u32 = row.get(25)?;
        let content_hash: i64 = row.get(26)?;
        let simhash: i64 = row.get(27)?;
        let is_soft_404: bool = row.get(28)?;
        let has_lorem_ipsum: bool = row.get(29)?;
        let is_https: bool = row.get(30)?;
        let has_hsts: bool = row.get(31)?;
        let has_csp: bool = row.get(32)?;
        let has_x_frame: bool = row.get(33)?;
        let has_x_content_type: bool = row.get(34)?;
        let mixed_content_count: u16 = row.get(35)?;
        let page_intent_raw: String = row.get(36)?;

        let h2_headings: Vec<String> = serde_json::from_str(&h2_raw).unwrap_or_default();
        let h3_headings: Vec<String> = serde_json::from_str(&h3_raw).unwrap_or_default();
        let page_intent: PageIntent = serde_json::from_str(&page_intent_raw).unwrap_or_default();

        Ok(PageReport {
            id: Some(id),
            crawl_id: CompactString::new(&crawl_id),
            url: url.clone(),
            url_hash: crate::core::url::url_hash(&url),
            final_url,
            status_code,
            content_type: CompactString::new(&content_type),
            size_bytes,
            ttfb_ms,
            crawl_depth,
            title,
            title_length,
            meta_description,
            meta_desc_length,
            canonical_url,
            html_lang: html_lang.map(|s| CompactString::new(&s)),
            charset: charset.map(|s| CompactString::new(&s)),
            viewport: viewport.map(|s| CompactString::new(&s)),
            robots_flags: RobotsFlags::from_bits_truncate(robots_flags_raw),
            is_sitemap_url,
            is_internal,
            h1_primary,
            h1_count,
            h2_headings,
            h3_headings,
            word_count,
            content_hash: content_hash as u64,
            simhash: simhash as u64,
            is_soft_404,
            has_lorem_ipsum,
            is_https,
            has_hsts,
            has_csp,
            has_x_frame,
            has_x_content_type,
            mixed_content_count,
            page_intent,
            links: Vec::new(),
            images: Vec::new(),
            schemas: Vec::new(),
            hreflangs: Vec::new(),
            issues: Vec::new(),
        })
    })?;

    for r in rows {
        pages.push(r?);
    }

    if pages.is_empty() {
        return Ok(pages);
    }

    // Load child collections in bulk
    let mut links_map: HashMap<String, Vec<DiscoveredLink>> = HashMap::new();
    let mut images_map: HashMap<String, Vec<ImageResource>> = HashMap::new();
    let mut schemas_map: HashMap<String, Vec<SchemaRecord>> = HashMap::new();
    let mut hreflangs_map: HashMap<String, Vec<HreflangTag>> = HashMap::new();
    let mut issues_map: HashMap<String, Vec<IssueFinding>> = HashMap::new();

    // 1. Links
    {
        let mut link_stmt = conn.prepare(
            "SELECT source_url, target_url, target_url_hash, anchor_text, is_internal,
                    is_nofollow, is_image_link, is_target_blank, has_opener_or_referrer, status_code
             FROM links WHERE crawl_id = ?1",
        )?;
        let link_rows = link_stmt.query_map(params![session_id], |row| {
            let source_url: String = row.get(0)?;
            let target_url: String = row.get(1)?;
            let _target_url_hash: i64 = row.get(2)?;
            let anchor_text: String = row.get(3)?;
            let is_internal: bool = row.get(4)?;
            let is_nofollow: bool = row.get(5)?;
            let is_image_link: bool = row.get(6)?;
            let is_target_blank: bool = row.get(7)?;
            let has_opener_or_referrer: bool = row.get(8)?;
            let status_code: Option<u16> = row.get(9)?;

            Ok((
                source_url.clone(),
                DiscoveredLink {
                    source_url,
                    target_url: target_url.clone(),
                    target_url_hash: crate::core::url::url_hash(&target_url),
                    anchor_text,
                    is_internal,
                    is_nofollow,
                    is_image_link,
                    is_target_blank,
                    has_opener_or_referrer,
                    status_code,
                },
            ))
        })?;
        for r in link_rows {
            let (src, link) = r?;
            links_map.entry(src).or_default().push(link);
        }
    }

    // 2. Images
    {
        let mut img_stmt = conn.prepare(
            "SELECT page_url, src_url, alt_text, width, height, size_bytes, has_dimensions, is_broken
             FROM images WHERE crawl_id = ?1",
        )?;
        let img_rows = img_stmt.query_map(params![session_id], |row| {
            let page_url: String = row.get(0)?;
            let src_url: String = row.get(1)?;
            let alt_text: Option<String> = row.get(2)?;
            let width: Option<u32> = row.get(3)?;
            let height: Option<u32> = row.get(4)?;
            let size_bytes: Option<u32> = row.get(5)?;
            let has_dimensions: bool = row.get(6)?;
            let is_broken: bool = row.get(7)?;

            Ok((
                page_url,
                ImageResource {
                    src_url,
                    alt_text,
                    width,
                    height,
                    size_bytes,
                    has_dimensions,
                    is_broken,
                },
            ))
        })?;
        for r in img_rows {
            let (page_url, img) = r?;
            images_map.entry(page_url).or_default().push(img);
        }
    }

    // 3. Schemas
    {
        let mut schema_stmt = conn.prepare(
            "SELECT page_url, schema_type, raw_json, is_valid_json, is_google_eligible
             FROM schemas WHERE crawl_id = ?1",
        )?;
        let schema_rows = schema_stmt.query_map(params![session_id], |row| {
            let page_url: String = row.get(0)?;
            let schema_type: String = row.get(1)?;
            let raw_json: String = row.get(2)?;
            let is_valid_json: bool = row.get(3)?;
            let is_google_eligible: bool = row.get(4)?;

            Ok((
                page_url,
                SchemaRecord {
                    schema_type: CompactString::new(&schema_type),
                    raw_json,
                    is_valid_json,
                    is_google_eligible,
                    missing_required_fields: Vec::new(),
                },
            ))
        })?;
        for r in schema_rows {
            let (page_url, schema) = r?;
            schemas_map.entry(page_url).or_default().push(schema);
        }
    }

    // 4. Hreflangs
    {
        let mut href_stmt = conn.prepare(
            "SELECT page_url, lang_code, target_url, is_reciprocal
             FROM hreflangs WHERE crawl_id = ?1",
        )?;
        let href_rows = href_stmt.query_map(params![session_id], |row| {
            let page_url: String = row.get(0)?;
            let lang_code: String = row.get(1)?;
            let target_url: String = row.get(2)?;
            let is_reciprocal: bool = row.get(3)?;

            Ok((
                page_url,
                HreflangTag {
                    lang_code: CompactString::new(&lang_code),
                    target_url,
                    is_reciprocal,
                },
            ))
        })?;
        for r in href_rows {
            let (page_url, tag) = r?;
            hreflangs_map.entry(page_url).or_default().push(tag);
        }
    }

    // 5. Issues
    {
        let mut issue_stmt = conn.prepare(
            "SELECT target_url, code, category, severity, title, message, source_page_url
             FROM issues WHERE crawl_id = ?1",
        )?;
        let issue_rows = issue_stmt.query_map(params![session_id], |row| {
            let target_url: String = row.get(0)?;
            let code_str: String = row.get(1)?;
            let cat_str: String = row.get(2)?;
            let sev_u8: u8 = row.get(3)?;
            let title: String = row.get(4)?;
            let message: String = row.get(5)?;
            let source_page_url: Option<String> = row.get(6)?;

            let code = RuleId::from_code(&code_str).unwrap_or(RuleId::ErrHttp5xxServerError);
            let category =
                IssueCategory::from_str_name(&cat_str).unwrap_or(IssueCategory::HttpTransport);
            let severity = Severity::from_u8(sev_u8).unwrap_or(Severity::Warning);

            Ok((
                target_url.clone(),
                IssueFinding {
                    code,
                    category,
                    severity,
                    title: CompactString::new(&title),
                    message,
                    target_url,
                    source_page_url,
                },
            ))
        })?;
        for r in issue_rows {
            let (target_url, issue) = r?;
            issues_map.entry(target_url).or_default().push(issue);
        }
    }

    // Attach child collections to each PageReport
    for page in &mut pages {
        if let Some(links) = links_map.remove(&page.url) {
            page.links = links;
        }
        if let Some(images) = images_map.remove(&page.url) {
            page.images = images;
        }
        if let Some(schemas) = schemas_map.remove(&page.url) {
            page.schemas = schemas;
        }
        if let Some(hreflangs) = hreflangs_map.remove(&page.url) {
            page.hreflangs = hreflangs;
        }
        if let Some(issues) = issues_map.remove(&page.url) {
            page.issues = issues;
        }
    }

    Ok(pages)
}

/// Queries issues matching session ID with optional severity and category filters.
pub fn get_crawl_issues(
    conn: &Connection,
    session_id: &str,
    severity_filter: Option<Severity>,
    category_filter: Option<IssueCategory>,
) -> SeoResult<Vec<IssueFinding>> {
    let mut query = "SELECT target_url, code, category, severity, title, message, source_page_url
                     FROM issues WHERE crawl_id = ?1"
        .to_string();

    if severity_filter.is_some() {
        query.push_str(" AND severity = ?2");
    }
    if category_filter.is_some() {
        if severity_filter.is_some() {
            query.push_str(" AND category = ?3");
        } else {
            query.push_str(" AND category = ?2");
        }
    }
    query.push_str(" ORDER BY severity ASC, id ASC");

    let mut stmt = conn.prepare(&query)?;

    let mapper = |row: &rusqlite::Row| -> rusqlite::Result<IssueFinding> {
        let target_url: String = row.get(0)?;
        let code_str: String = row.get(1)?;
        let cat_str: String = row.get(2)?;
        let sev_u8: u8 = row.get(3)?;
        let title: String = row.get(4)?;
        let message: String = row.get(5)?;
        let source_page_url: Option<String> = row.get(6)?;

        let code = RuleId::from_code(&code_str).unwrap_or(RuleId::ErrHttp5xxServerError);
        let category =
            IssueCategory::from_str_name(&cat_str).unwrap_or(IssueCategory::HttpTransport);
        let severity = Severity::from_u8(sev_u8).unwrap_or(Severity::Warning);

        Ok(IssueFinding {
            code,
            category,
            severity,
            title: CompactString::new(&title),
            message,
            target_url,
            source_page_url,
        })
    };

    let issues: Vec<IssueFinding> = match (severity_filter, category_filter) {
        (Some(s), Some(c)) => {
            let rows = stmt.query_map(params![session_id, s.as_u8(), c.as_str()], mapper)?;
            let mut list = Vec::new();
            for r in rows {
                list.push(r?);
            }
            list
        }
        (Some(s), None) => {
            let rows = stmt.query_map(params![session_id, s.as_u8()], mapper)?;
            let mut list = Vec::new();
            for r in rows {
                list.push(r?);
            }
            list
        }
        (None, Some(c)) => {
            let rows = stmt.query_map(params![session_id, c.as_str()], mapper)?;
            let mut list = Vec::new();
            for r in rows {
                list.push(r?);
            }
            list
        }
        (None, None) => {
            let rows = stmt.query_map(params![session_id], mapper)?;
            let mut list = Vec::new();
            for r in rows {
                list.push(r?);
            }
            list
        }
    };

    Ok(issues)
}

/// Deletes a specific crawl session from SQLite (cascading to pages, issues, links, etc.).
pub fn delete_crawl(conn: &Connection, session_id: &str) -> SeoResult<bool> {
    let rows = conn.execute(
        "DELETE FROM crawls WHERE session_id = ?1",
        params![session_id],
    )?;
    Ok(rows > 0)
}

/// Cleans/purges crawl sessions started older than `days` days ago.
pub fn clean_crawls_older_than(conn: &Connection, days: u32) -> SeoResult<usize> {
    let rows = conn.execute(
        "DELETE FROM crawls WHERE started_at < datetime('now', '-' || ?1 || ' days')",
        params![days],
    )?;
    Ok(rows)
}

/// Purges all historical crawl sessions from SQLite.
pub fn clean_all_crawls(conn: &Connection) -> SeoResult<usize> {
    let rows = conn.execute("DELETE FROM crawls", [])?;
    Ok(rows)
}

/// Criteria for filtering issues during queries and counting.
#[derive(Debug, Clone, Default)]
pub struct IssueFilterCriteria<'a> {
    pub severity: Option<Severity>,
    pub category: Option<IssueCategory>,
    pub code: Option<&'a str>,
    pub url_substring: Option<&'a str>,
    pub limit: usize,
    pub offset: usize,
}

/// Advanced query for crawl issues with optional filters for severity, category, rule code, URL substring, and pagination.
pub fn query_issues_filtered(
    conn: &Connection,
    session_id: &str,
    criteria: &IssueFilterCriteria,
) -> SeoResult<Vec<IssueFinding>> {
    let mut sql = "SELECT target_url, code, category, severity, title, message, source_page_url
                   FROM issues WHERE crawl_id = ?1"
        .to_string();
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(session_id.to_string())];

    if let Some(sev) = criteria.severity {
        params_vec.push(Box::new(sev.as_u8()));
        sql.push_str(&format!(" AND severity = ?{}", params_vec.len()));
    }
    if let Some(cat) = criteria.category {
        params_vec.push(Box::new(cat.as_str().to_string()));
        sql.push_str(&format!(" AND category = ?{}", params_vec.len()));
    }
    if let Some(code) = criteria.code {
        params_vec.push(Box::new(code.to_string()));
        sql.push_str(&format!(" AND code = ?{}", params_vec.len()));
    }
    if let Some(sub) = criteria.url_substring {
        params_vec.push(Box::new(format!("%{sub}%")));
        sql.push_str(&format!(" AND target_url LIKE ?{}", params_vec.len()));
    }

    sql.push_str(" ORDER BY severity ASC, id ASC");
    let limit = if criteria.limit == 0 {
        50
    } else {
        criteria.limit
    };
    params_vec.push(Box::new(limit as i64));
    sql.push_str(&format!(" LIMIT ?{}", params_vec.len()));
    params_vec.push(Box::new(criteria.offset as i64));
    sql.push_str(&format!(" OFFSET ?{}", params_vec.len()));

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let target_url: String = row.get(0)?;
        let code_str: String = row.get(1)?;
        let cat_str: String = row.get(2)?;
        let sev_u8: u8 = row.get(3)?;
        let title: String = row.get(4)?;
        let message: String = row.get(5)?;
        let source_page_url: Option<String> = row.get(6)?;

        let code = RuleId::from_code(&code_str).unwrap_or(RuleId::ErrHttp5xxServerError);
        let category =
            IssueCategory::from_str_name(&cat_str).unwrap_or(IssueCategory::HttpTransport);
        let severity = Severity::from_u8(sev_u8).unwrap_or(Severity::Warning);

        Ok(IssueFinding {
            code,
            category,
            severity,
            title: CompactString::new(&title),
            message,
            target_url,
            source_page_url,
        })
    })?;

    let mut list = Vec::new();
    for r in rows {
        list.push(r?);
    }
    Ok(list)
}

/// Counts total issues matching filters for pagination.
pub fn count_issues_filtered(
    conn: &Connection,
    session_id: &str,
    criteria: &IssueFilterCriteria,
) -> SeoResult<usize> {
    let mut sql = "SELECT COUNT(*) FROM issues WHERE crawl_id = ?1".to_string();
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(session_id.to_string())];

    if let Some(sev) = criteria.severity {
        params_vec.push(Box::new(sev.as_u8()));
        sql.push_str(&format!(" AND severity = ?{}", params_vec.len()));
    }
    if let Some(cat) = criteria.category {
        params_vec.push(Box::new(cat.as_str().to_string()));
        sql.push_str(&format!(" AND category = ?{}", params_vec.len()));
    }
    if let Some(code) = criteria.code {
        params_vec.push(Box::new(code.to_string()));
        sql.push_str(&format!(" AND code = ?{}", params_vec.len()));
    }
    if let Some(sub) = criteria.url_substring {
        params_vec.push(Box::new(format!("%{sub}%")));
        sql.push_str(&format!(" AND target_url LIKE ?{}", params_vec.len()));
    }

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
    let count: usize = stmt.query_row(param_refs.as_slice(), |r| r.get(0))?;
    Ok(count)
}
