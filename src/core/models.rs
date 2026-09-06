//! # Domain Models
//!
//! Core data models representing pages, discovered links, images, schema records,
//! SEO issues, severity tiers, and crawl summary statistics.
//!
//! ## Memory Optimization Architecture
//!
//! When conducting technical SEO audits of enterprise websites containing 50,000+ pages,
//! in-memory representation can quickly overwhelm system RAM. SEO Lens optimizes memory
//! layout across all core models:
//!
//! 1. **Compact String Inlining**: Short strings $\le 24$ bytes (such as MIME types, tag names,
//!    language codes, and issue IDs) use [`compact_str::CompactString`], which inlines the text
//!    directly on the stack rather than allocating on the heap.
//! 2. **Bitfield Directives**: Robots indexing instructions (`noindex`, `nofollow`, `nosnippet`,
//!    `noimageindex`, `noarchive`) are packed into a single 1-byte [`RobotsFlags`] bitmask.
//! 3. **Compact Primitives**: Network status codes, TTFB latencies, dimensions, and depth levels
//!    are stored in tightly-sized integers (`u16`, `u32`) instead of generic 64-bit numbers.
//! 4. **Integer URL Hashes**: Discovered links store a 64-bit target URL hash (`target_url_hash`)
//!    enabling rapid frontier deduplication in SwissTables without cloning full strings.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

/// Severity classification for technical SEO findings.
///
/// Ranked in descending order of urgency:
/// `Critical` (1) > `Alert` (2) > `Warning` (3) > `Notice` (4).
///
/// # Examples
///
/// ```rust
/// use seo_lens::core::models::Severity;
///
/// // Severities can be compared by urgency (lower value = higher urgency)
/// assert!(Severity::Critical < Severity::Alert);
/// assert!(Severity::Alert < Severity::Warning);
/// assert!(Severity::Warning < Severity::Notice);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Severe errors that completely block indexing, break protocols, or crash pages (e.g. 5xx errors, circular redirects).
    Critical = 1,
    /// High-priority defects that directly harm search rankings or prevent page discovery (e.g. accidental noindex, canonical mismatch).
    Alert = 2,
    /// Moderate optimization issues or suboptimal implementations (e.g. missing meta description, long title, missing image alt).
    Warning = 3,
    /// Informational observations or minor best-practice recommendations (e.g. protocol consistency, casing differences).
    Notice = 4,
}

/// Functional categories mapping to the 120-check technical SEO audit catalog.
///
/// Organizes audit findings into logical audit domains for reporting and filtering.
///
/// # Examples
///
/// ```rust
/// use seo_lens::core::models::IssueCategory;
///
/// let category = IssueCategory::Indexability;
/// assert_eq!(format!("{category:?}"), "Indexability");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCategory {
    /// HTTP status codes, transport protocols, connection latency, and timeouts.
    HttpTransport,
    /// Document titles, meta descriptions, viewports, charsets, and OpenGraph/Twitter tags.
    TitleMetadata,
    /// Heading tags (`<h1>`-`<h6>`), hierarchy consistency, and duplicate H1 detection.
    Headings,
    /// Robots directives (`robots.txt`, `<meta name="robots">`, X-Robots-Tag), and indexability.
    Indexability,
    /// Canonical tag implementation, self-canonicalization, and cross-domain references.
    Canonicalization,
    /// Internal/external link discovery, broken links (404s), redirect chains, and anchor text.
    Links,
    /// HTTPS enforcement, Mixed Content, HSTS, and Content-Security-Policy headers.
    Security,
    /// Mobile viewport configuration and responsiveness signals.
    MobileUx,
    /// Internationalization hreflang tags, reciprocal validity, and language codes.
    Internationalization,
    /// Schema.org JSON-LD structured data and Google Rich Results validation.
    StructuredData,
    /// LLM/AI crawler access (GPTBot, PerplexityBot, ClaudeBot, Google-Extended) and geo-targeting.
    GeoAiSearch,
    /// Site graph topology, internal PageRank distributions, and crawl depth hierarchy.
    SiteGraph,
    /// Client-side JavaScript DOM rendering diffs (CSR vs SSR content differences).
    JsDiff,
}

/// Strongly typed identifier for technical SEO audit rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RuleId {
    // --- Category 1: HTTP Status & Transport ---
    ErrHttp4xxClientError,
    ErrHttp5xxServerError,
    InfoHttp301PermanentRedirect,
    InfoHttp302TemporaryRedirect,
    InfoHttp307_308Redirect,
    AlertWafBotChallenge,
    WarnSlowTtfb,
    AlertFacetedSpiderTrap,
    ErrHttpSoft404,

    // --- Category 2: Titles & Basic Metadata ---
    ErrTitleMissing,
    WarnTitleTooShort,
    WarnTitleTooLong,
    ErrTitleMultiple,
    WarnTitleWhitespacePadded,
    WarnTitleSameAsH1,
    WarnMetaDescMissing,
    WarnMetaDescTooShort,
    WarnMetaDescTooLong,
    ErrMetaDescMultiple,
    WarnMetaKeywordsPresent,

    // --- Category 3: Headings & Document Structure ---
    ErrH1Missing,
    WarnH1Multiple,
    WarnH1Empty,
    WarnH1TooLong,
    WarnHeadingHierarchySkipped,
    WarnDuplicateHeadingText,
    WarnExcessiveDomDepth,

    // --- Category 4: Indexability & Directives ---
    AlertIndexingBlockedNoindex,
    WarnLinkEquityBlockedNofollow,
    WarnNoarchivePresent,
    WarnNosnippetPresent,
    WarnPaginationMissingCanonical,
    AlertPaginationNoindex,
    AlertUnrenderedSpaHeuristic,

    // --- Category 5: Canonicalization ---
    WarnCanonicalMissing,
    ErrCanonicalRelative,
    ErrCanonicalMultiple,
    AlertCanonicalMismatch,
    AlertCanonicalCrossDomain,
    WarnCanonicalToUnverifiedHttp,

    // --- Category 6: Modern Security & Transport ---
    ErrSecurityInsecureHttp,
    ErrSecurityMixedContent,
    WarnSecurityMissingHsts,
    WarnSecurityMissingCsp,
    WarnSecurityMissingXFrameOptions,
    WarnSecurityMissingXContentType,
    WarnSecurityMissingReferrerPolicy,
    WarnSecurityTargetBlankNoOpener,
    WarnSecurityInsecureForm,

    // --- Category 7: Images & Core Web Vitals (CLS) ---
    WarnImageMissingAlt,
    WarnImageMissingDimensions,
    WarnImageDataUri,
    WarnImgAltTooLong,

    // --- Category 8: Mobile UX & Viewports ---
    ErrMobileNoViewport,
    WarnMobileViewportNonScalable,
    WarnPerfLargeHtmlPayload,
    ErrPerfExcessiveHtmlPayload,

    // --- Category 9: Links & Anchor Quality ---
    WarnLinksTooManyOnPage,
    WarnLinkSuspiciousAnchor,
    WarnLinkEmptyAnchor,

    // --- Category 9: Structured Data & Schema.org ---
    ErrSchemaSyntaxError,
    WarnSchemaMissingRequiredFields,
    WarnSchemaMultipleProductEntities,
    WarnSchemaInvalidDateFormat,

    // --- Category 10: Content Quality & AI Search ---
    WarnContentThin,
    WarnLoremIpsumDetected,
    AlertAiSearchBotsBlocked,
    WarnLlmsTxtMissing,

    // --- Category 11: Internationalization & Hreflang ---
    ErrHreflangNotReciprocal,
    ErrHreflangToNonCanonical,
    ErrHreflangToBrokenOrRedirect,
    ErrHreflangInvalidLangCode,
    WarnHreflangCrossDomain,
    WarnHreflangMissingXDefault,
    ErrHreflangMissingSelfReference,
    WarnHtmlLangMissing,

    // --- Category 12: Site-Wide Graph & Architecture (Post-Crawl) ---
    AlertGraphOrphanPage,
    ErrGraphRedirectLoop,
    WarnGraphRedirectChain,
    ErrGraphCanonicalLoop,
    WarnGraphExactDuplicateContent,
    WarnGraphNearDuplicateContent,
    WarnGraphDuplicateTitles,
    WarnGraphDuplicateMetaDescs,
    WarnGraphDeadEndPage,
    WarnGraphHighCrawlDepth,
    WarnLowInternalPagerankHub,
}

impl RuleId {
    /// Returns the standardized SCREAMING_SNAKE_CASE string identifier for this rule.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ErrHttp4xxClientError => "ERR_HTTP_4XX_CLIENT_ERROR",
            Self::ErrHttp5xxServerError => "ERR_HTTP_5XX_SERVER_ERROR",
            Self::InfoHttp301PermanentRedirect => "INFO_HTTP_301_PERMANENT_REDIRECT",
            Self::InfoHttp302TemporaryRedirect => "INFO_HTTP_302_TEMPORARY_REDIRECT",
            Self::InfoHttp307_308Redirect => "INFO_HTTP_307_308_REDIRECT",
            Self::AlertWafBotChallenge => "ALERT_WAF_BOT_CHALLENGE",
            Self::WarnSlowTtfb => "WARN_SLOW_TTFB",
            Self::AlertFacetedSpiderTrap => "ALERT_FACETED_SPIDER_TRAP",
            Self::ErrHttpSoft404 => "ERR_HTTP_SOFT_404",

            Self::ErrTitleMissing => "ERR_TITLE_MISSING",
            Self::WarnTitleTooShort => "WARN_TITLE_TOO_SHORT",
            Self::WarnTitleTooLong => "WARN_TITLE_TOO_LONG",
            Self::ErrTitleMultiple => "ERR_TITLE_MULTIPLE",
            Self::WarnTitleWhitespacePadded => "WARN_TITLE_WHITESPACE_PADDED",
            Self::WarnTitleSameAsH1 => "WARN_TITLE_SAME_AS_H1",
            Self::WarnMetaDescMissing => "WARN_META_DESC_MISSING",
            Self::WarnMetaDescTooShort => "WARN_META_DESC_TOO_SHORT",
            Self::WarnMetaDescTooLong => "WARN_META_DESC_TOO_LONG",
            Self::ErrMetaDescMultiple => "ERR_META_DESC_MULTIPLE",
            Self::WarnMetaKeywordsPresent => "WARN_META_KEYWORDS_PRESENT",

            Self::ErrH1Missing => "ERR_H1_MISSING",
            Self::WarnH1Multiple => "WARN_H1_MULTIPLE",
            Self::WarnH1Empty => "WARN_H1_EMPTY",
            Self::WarnH1TooLong => "WARN_H1_TOO_LONG",
            Self::WarnHeadingHierarchySkipped => "WARN_HEADING_HIERARCHY_SKIPPED",
            Self::WarnDuplicateHeadingText => "WARN_DUPLICATE_HEADING_TEXT",
            Self::WarnExcessiveDomDepth => "WARN_EXCESSIVE_DOM_DEPTH",

            Self::AlertIndexingBlockedNoindex => "ALERT_INDEXING_BLOCKED_NOINDEX",
            Self::WarnLinkEquityBlockedNofollow => "WARN_LINK_EQUITY_BLOCKED_NOFOLLOW",
            Self::WarnNoarchivePresent => "WARN_NOARCHIVE_PRESENT",
            Self::WarnNosnippetPresent => "WARN_NOSNIPPET_PRESENT",
            Self::WarnPaginationMissingCanonical => "WARN_PAGINATION_MISSING_CANONICAL",
            Self::AlertPaginationNoindex => "ALERT_PAGINATION_NOINDEX",
            Self::AlertUnrenderedSpaHeuristic => "ALERT_UNRENDERED_SPA_HEURISTIC",

            Self::WarnCanonicalMissing => "WARN_CANONICAL_MISSING",
            Self::ErrCanonicalRelative => "ERR_CANONICAL_RELATIVE",
            Self::ErrCanonicalMultiple => "ERR_CANONICAL_MULTIPLE",
            Self::AlertCanonicalMismatch => "ALERT_CANONICAL_MISMATCH",
            Self::AlertCanonicalCrossDomain => "ALERT_CANONICAL_CROSS_DOMAIN",
            Self::WarnCanonicalToUnverifiedHttp => "WARN_CANONICAL_TO_UNVERIFIED_HTTP",

            Self::ErrSecurityInsecureHttp => "ERR_SECURITY_INSECURE_HTTP",
            Self::ErrSecurityMixedContent => "ERR_SECURITY_MIXED_CONTENT",
            Self::WarnSecurityMissingHsts => "WARN_SECURITY_MISSING_HSTS",
            Self::WarnSecurityMissingCsp => "WARN_SECURITY_MISSING_CSP",
            Self::WarnSecurityMissingXFrameOptions => "WARN_SECURITY_MISSING_X_FRAME_OPTIONS",
            Self::WarnSecurityMissingXContentType => "WARN_SECURITY_MISSING_X_CONTENT_TYPE",
            Self::WarnSecurityMissingReferrerPolicy => "WARN_SECURITY_MISSING_REFERRER_POLICY",
            Self::WarnSecurityTargetBlankNoOpener => "WARN_SECURITY_TARGET_BLANK_NO_OPENER",
            Self::WarnSecurityInsecureForm => "WARN_SECURITY_INSECURE_FORM",

            Self::WarnImageMissingAlt => "WARN_IMAGE_MISSING_ALT",
            Self::WarnImageMissingDimensions => "WARN_IMAGE_MISSING_DIMENSIONS",
            Self::WarnImageDataUri => "WARN_IMAGE_DATA_URI",
            Self::WarnImgAltTooLong => "WARN_IMG_ALT_TOO_LONG",

            Self::ErrMobileNoViewport => "ERR_MOBILE_NO_VIEWPORT",
            Self::WarnMobileViewportNonScalable => "WARN_MOBILE_VIEWPORT_NON_SCALABLE",
            Self::WarnPerfLargeHtmlPayload => "WARN_PERF_LARGE_HTML_PAYLOAD",
            Self::ErrPerfExcessiveHtmlPayload => "ERR_PERF_EXCESSIVE_HTML_PAYLOAD",

            Self::WarnLinksTooManyOnPage => "WARN_LINKS_TOO_MANY_ON_PAGE",
            Self::WarnLinkSuspiciousAnchor => "WARN_LINK_SUSPICIOUS_ANCHOR",
            Self::WarnLinkEmptyAnchor => "WARN_LINK_EMPTY_ANCHOR",

            Self::ErrSchemaSyntaxError => "ERR_SCHEMA_SYNTAX_ERROR",
            Self::WarnSchemaMissingRequiredFields => "WARN_SCHEMA_MISSING_REQUIRED_FIELDS",
            Self::WarnSchemaMultipleProductEntities => "WARN_SCHEMA_MULTIPLE_PRODUCT_ENTITIES",
            Self::WarnSchemaInvalidDateFormat => "WARN_SCHEMA_INVALID_DATE_FORMAT",

            Self::WarnContentThin => "WARN_CONTENT_THIN",
            Self::WarnLoremIpsumDetected => "WARN_LOREM_IPSUM_DETECTED",
            Self::AlertAiSearchBotsBlocked => "ALERT_AI_SEARCH_BOTS_BLOCKED",
            Self::WarnLlmsTxtMissing => "WARN_LLMS_TXT_MISSING",

            Self::ErrHreflangNotReciprocal => "ERR_HREFLANG_NOT_RECIPROCAL",
            Self::ErrHreflangToNonCanonical => "ERR_HREFLANG_TO_NON_CANONICAL",
            Self::ErrHreflangToBrokenOrRedirect => "ERR_HREFLANG_TO_BROKEN_OR_REDIRECT",
            Self::ErrHreflangInvalidLangCode => "ERR_HREFLANG_INVALID_LANG_CODE",
            Self::WarnHreflangCrossDomain => "WARN_HREFLANG_CROSS_DOMAIN",
            Self::WarnHreflangMissingXDefault => "WARN_HREFLANG_MISSING_X_DEFAULT",
            Self::ErrHreflangMissingSelfReference => "ERR_HREFLANG_MISSING_SELF_REFERENCE",
            Self::WarnHtmlLangMissing => "WARN_HTML_LANG_MISSING",

            Self::AlertGraphOrphanPage => "ALERT_GRAPH_ORPHAN_PAGE",
            Self::ErrGraphRedirectLoop => "ERR_GRAPH_REDIRECT_LOOP",
            Self::WarnGraphRedirectChain => "WARN_GRAPH_REDIRECT_CHAIN",
            Self::ErrGraphCanonicalLoop => "ERR_GRAPH_CANONICAL_LOOP",
            Self::WarnGraphExactDuplicateContent => "WARN_GRAPH_EXACT_DUPLICATE_CONTENT",
            Self::WarnGraphNearDuplicateContent => "WARN_GRAPH_NEAR_DUPLICATE_CONTENT",
            Self::WarnGraphDuplicateTitles => "WARN_GRAPH_DUPLICATE_TITLES",
            Self::WarnGraphDuplicateMetaDescs => "WARN_GRAPH_DUPLICATE_META_DESCS",
            Self::WarnGraphDeadEndPage => "WARN_GRAPH_DEAD_END_PAGE",
            Self::WarnGraphHighCrawlDepth => "WARN_GRAPH_HIGH_CRAWL_DEPTH",
            Self::WarnLowInternalPagerankHub => "WARN_LOW_INTERNAL_PAGERANK_HUB",
        }
    }
}

impl std::fmt::Display for RuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

bitflags::bitflags! {
    /// Memory-efficient 1-byte bitfield for robots and indexing directives.
    ///
    /// Web crawlers evaluate robots directives across both `<meta name="robots">` tags
    /// and HTTP `X-Robots-Tag` headers. Packing these boolean flags into a single `u8`
    /// uses 87.5% less memory than storing 6 separate boolean fields.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use seo_lens::core::models::RobotsFlags;
    ///
    /// let mut flags = RobotsFlags::NONE;
    /// flags.insert(RobotsFlags::NOINDEX);
    /// flags.insert(RobotsFlags::NOFOLLOW);
    ///
    /// assert!(flags.contains(RobotsFlags::NOINDEX));
    /// assert!(flags.contains(RobotsFlags::NOFOLLOW));
    /// assert!(!flags.contains(RobotsFlags::NOARCHIVE));
    ///
    /// // Bitwise combinations
    /// let combined = RobotsFlags::NOINDEX | RobotsFlags::NOSNIPPET;
    /// assert!(combined.contains(RobotsFlags::NOINDEX));
    /// assert!(combined.contains(RobotsFlags::NOSNIPPET));
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    pub struct RobotsFlags: u8 {
        /// No restrictive directives present; page is fully indexable and followable.
        const NONE         = 0b0000_0000;
        /// Instructs search engines not to index or display this page in SERPs.
        const NOINDEX      = 0b0000_0001;
        /// Instructs search engines not to follow outbound links on this page.
        const NOFOLLOW     = 0b0000_0010;
        /// Prevents search engines from displaying text snippets or video previews in search results.
        const NOSNIPPET    = 0b0000_0100;
        /// Prevents search engines from indexing images hosted on this page.
        const NOIMAGEINDEX = 0b0000_1000;
        /// Prevents search engines from offering cached links for this page.
        const NOARCHIVE    = 0b0001_0000;
    }
}

/// Represents the complete audit report for a single crawled URL.
///
/// Contains all HTTP transport telemetry, metadata, heading hierarchy,
/// editorial content metrics, security headers, and associated child collections.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageReport {
    /// Unique incremental identifier (primary key in SQLite).
    pub id: Option<i64>,
    /// Associated crawl session identifier.
    pub crawl_id: CompactString,

    // --- Network & Transport ---
    /// Normalized request URL.
    pub url: String,
    /// 64-bit deterministic hash of the normalized request URL.
    pub url_hash: u64,
    /// Final destination URL after following HTTP redirects, if different.
    pub final_url: Option<String>,
    /// HTTP response status code (e.g. 200, 301, 404, 500).
    pub status_code: u16,
    /// Content-Type header value (e.g. "text/html; charset=utf-8").
    pub content_type: CompactString,
    /// Total response body size in bytes.
    pub size_bytes: u32,
    /// Time to first byte (TTFB) in milliseconds.
    pub ttfb_ms: u32,
    /// Crawl depth level from the root seed URL (0 = seed).
    pub crawl_depth: u16,

    // --- Metadata ---
    /// Document title extracted from `<title>`.
    pub title: Option<String>,
    /// Character length of the document title.
    pub title_length: u16,
    /// Meta description content extracted from `<meta name="description">`.
    pub meta_description: Option<String>,
    /// Character length of the meta description.
    pub meta_desc_length: u16,
    /// Canonical URL declared via `<link rel="canonical">`.
    pub canonical_url: Option<String>,
    /// Document language declared in `<html lang="...">`.
    pub html_lang: Option<CompactString>,
    /// Character encoding declared via `<meta charset="...">`.
    pub charset: Option<CompactString>,
    /// Viewport configuration declared via `<meta name="viewport">`.
    pub viewport: Option<CompactString>,

    // --- Directives ---
    /// Combined robots directives parsed into a 1-byte bitfield.
    pub robots_flags: RobotsFlags,
    /// Whether this URL was discovered in the site's XML sitemap.
    pub is_sitemap_url: bool,
    /// Whether this URL belongs to the internal crawl target domain.
    pub is_internal: bool,

    // --- Headings ---
    /// First `<h1>` heading text found in the document.
    pub h1_primary: Option<String>,
    /// Total count of `<h1>` tags on the page.
    pub h1_count: u16,
    /// Ordered list of all `<h2>` heading texts.
    pub h2_headings: Vec<String>,
    /// Ordered list of all `<h3>` heading texts.
    pub h3_headings: Vec<String>,

    // --- Content & Quality ---
    /// Word count of editorial body text (excluding navigation, header, footer).
    pub word_count: u32,
    /// 64-bit deterministic hash of editorial text for exact duplicate detection.
    pub content_hash: u64,
    /// 64-bit locality-sensitive SimHash fingerprint for near-duplicate detection.
    pub simhash: u64,
    /// Whether the page returns a 200 OK status while presenting 404 error content.
    pub is_soft_404: bool,
    /// Whether placeholder "Lorem ipsum" dummy text was detected.
    pub has_lorem_ipsum: bool,

    // --- Security ---
    /// Whether the URL is delivered over HTTPS.
    pub is_https: bool,
    /// Whether the Strict-Transport-Security (HSTS) header is present.
    pub has_hsts: bool,
    /// Whether the Content-Security-Policy (CSP) header is present.
    pub has_csp: bool,
    /// Whether the X-Frame-Options clickjacking protection header is present.
    pub has_x_frame: bool,
    /// Whether the X-Content-Type-Options: nosniff header is present.
    pub has_x_content_type: bool,
    /// Count of insecure HTTP resources requested by an HTTPS page.
    pub mixed_content_count: u16,

    // --- Child Collections (stored relationally) ---
    /// Hyperlinks discovered on this page.
    pub links: Vec<DiscoveredLink>,
    /// Image assets embedded in this page.
    pub images: Vec<ImageResource>,
    /// Structured data records parsed from this page.
    pub schemas: Vec<SchemaRecord>,
    /// Alternate language hreflang links declared on this page.
    pub hreflangs: Vec<HreflangTag>,
    /// Audit issues identified on this page by the rules engine.
    pub issues: Vec<IssueFinding>,
}

/// A hyperlink discovered in an HTML document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredLink {
    /// URL of the page where the link was found.
    pub source_url: String,
    /// Fully resolved absolute destination URL.
    pub target_url: String,
    /// 64-bit deterministic hash of the target URL for fast frontier lookup.
    pub target_url_hash: u64,
    /// Anchor text or child image alt text associated with the hyperlink.
    pub anchor_text: String,
    /// Whether the link points to the same hostname as the source.
    pub is_internal: bool,
    /// Whether the link includes a `rel="nofollow"` directive.
    pub is_nofollow: bool,
    /// Whether the link wraps an image instead of textual anchor text.
    pub is_image_link: bool,
    /// Whether the link specifies `target="_blank"`.
    pub is_target_blank: bool,
    /// Whether the link specifies `rel="noopener"` or `rel="noreferrer"`.
    pub has_opener_or_referrer: bool,
    /// HTTP status code of the target URL, once fetched.
    pub status_code: Option<u16>,
}

/// An image asset referenced on a page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageResource {
    /// Resolved absolute image source URL.
    pub src_url: String,
    /// Alternative text extracted from the `alt` attribute.
    pub alt_text: Option<String>,
    /// Explicit width in pixels, if declared in attributes.
    pub width: Option<u32>,
    /// Explicit height in pixels, if declared in attributes.
    pub height: Option<u32>,
    /// Response payload size in bytes, once fetched.
    pub size_bytes: Option<u32>,
    /// Whether both width and height attributes are explicitly defined.
    pub has_dimensions: bool,
    /// Whether the image URL returned a 4xx/5xx HTTP error.
    pub is_broken: bool,
}

/// JSON-LD or Microdata structured data block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRecord {
    /// Extracted schema `@type` (e.g. "Article", "Product", "Organization").
    pub schema_type: CompactString,
    /// Raw JSON string of the structured data block.
    pub raw_json: String,
    /// Whether the block is syntactically valid JSON.
    pub is_valid_json: bool,
    /// Whether the schema type is eligible for Google Rich Results.
    pub is_google_eligible: bool,
    /// Mandatory schema fields missing for rich snippet qualification.
    pub missing_required_fields: Vec<CompactString>,
}

/// Hreflang alternate language tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HreflangTag {
    /// Language or region code (e.g. "en", "es-ES", "x-default").
    pub lang_code: CompactString,
    /// Target alternate URL for this language.
    pub target_url: String,
    /// Whether the target page reciprocally links back with matching hreflang.
    pub is_reciprocal: bool,
}

/// A specific technical SEO defect identified by the rules engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueFinding {
    /// Strongly-typed issue identifier.
    pub code: RuleId,
    /// Functional audit category for grouping.
    pub category: IssueCategory,
    /// Severity classification tier.
    pub severity: Severity,
    /// Human-readable headline summarizing the issue.
    pub title: CompactString,
    /// Context-specific detail explaining where and why the rule triggered.
    pub message: String,
    /// URL where the issue was identified.
    pub target_url: String,
    /// Source page that linked to this target (for broken link tracking).
    pub source_page_url: Option<String>,
}

/// Summary metrics for an entire crawl session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CrawlSummary {
    /// Unique crawl session identifier.
    pub session_id: String,
    /// Seed root URL of the crawl.
    pub target_url: String,
    /// ISO 8601 timestamp when the crawl started.
    pub started_at: String,
    /// ISO 8601 timestamp when the crawl completed, if finished.
    pub finished_at: Option<String>,
    /// Total number of unique URLs successfully crawled.
    pub total_pages_crawled: u32,
    /// Total number of unique hyperlinks discovered.
    pub total_links_discovered: u32,
    /// Total number of Critical severity defects found.
    pub total_errors: u32,
    /// Total number of Alert severity defects found.
    pub total_alerts: u32,
    /// Total number of Warning severity defects found.
    pub total_warnings: u32,
    /// Total number of Notice severity observations found.
    pub total_notices: u32,
    /// Mean time to first byte across all successful page fetches.
    pub average_ttfb_ms: u32,
    /// 95th percentile TTFB latency across all requests.
    pub p95_ttfb_ms: u32,
    /// Overall website technical health score from 0 to 100.
    pub health_score: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::url::url_hash;

    #[test]
    fn test_domain_models_integrity() {
        let mut flags = RobotsFlags::NONE;
        flags.insert(RobotsFlags::NOINDEX);
        flags.insert(RobotsFlags::NOFOLLOW);
        assert!(flags.contains(RobotsFlags::NOINDEX));
        assert!(flags.contains(RobotsFlags::NOFOLLOW));
        assert!(!flags.contains(RobotsFlags::NOARCHIVE));

        let issue = IssueFinding {
            code: RuleId::ErrTitleMissing,
            category: IssueCategory::TitleMetadata,
            severity: Severity::Critical,
            title: CompactString::new("Missing Document Title"),
            message: "Page lacks a <title> tag.".to_string(),
            target_url: "https://example.com/missing-title".to_string(),
            source_page_url: None,
        };
        assert_eq!(issue.code, RuleId::ErrTitleMissing);
        assert_eq!(issue.code.as_str(), "ERR_TITLE_MISSING");
        assert_eq!(issue.severity, Severity::Critical);
        assert_eq!(issue.category, IssueCategory::TitleMetadata);

        let link = DiscoveredLink {
            source_url: "https://example.com/".to_string(),
            target_url: "https://example.com/about".to_string(),
            target_url_hash: url_hash("https://example.com/about"),
            anchor_text: "About Us".to_string(),
            is_internal: true,
            is_nofollow: false,
            is_image_link: false,
            is_target_blank: false,
            has_opener_or_referrer: true,
            status_code: Some(200),
        };
        assert!(link.is_internal);

        let image = ImageResource {
            src_url: "https://example.com/logo.png".to_string(),
            alt_text: Some("Company Logo".to_string()),
            width: Some(200),
            height: Some(50),
            size_bytes: Some(15000),
            has_dimensions: true,
            is_broken: false,
        };
        assert!(image.has_dimensions);

        let schema = SchemaRecord {
            schema_type: CompactString::new("Organization"),
            raw_json: r#"{"@type":"Organization"}"#.to_string(),
            is_valid_json: true,
            is_google_eligible: true,
            missing_required_fields: vec![],
        };
        assert!(schema.is_valid_json);

        let hreflang = HreflangTag {
            lang_code: CompactString::new("en-US"),
            target_url: "https://example.com/en-us/".to_string(),
            is_reciprocal: true,
        };
        assert_eq!(hreflang.lang_code.as_str(), "en-US");

        let page_report = PageReport {
            id: None,
            crawl_id: CompactString::new("crawl-test-1"),
            url: "https://example.com/".to_string(),
            url_hash: url_hash("https://example.com/"),
            final_url: None,
            status_code: 200,
            content_type: CompactString::new("text/html; charset=utf-8"),
            size_bytes: 4096,
            ttfb_ms: 120,
            crawl_depth: 0,
            title: Some("Homepage".to_string()),
            title_length: 8,
            meta_description: Some("Homepage description".to_string()),
            meta_desc_length: 20,
            canonical_url: Some("https://example.com/".to_string()),
            html_lang: Some(CompactString::new("en")),
            charset: Some(CompactString::new("utf-8")),
            viewport: Some(CompactString::new("width=device-width, initial-scale=1")),
            robots_flags: flags,
            is_sitemap_url: true,
            is_internal: true,
            h1_primary: Some("Welcome".to_string()),
            h1_count: 1,
            h2_headings: vec!["Features".to_string()],
            h3_headings: vec![],
            word_count: 350,
            content_hash: 12345,
            simhash: 67890,
            is_soft_404: false,
            has_lorem_ipsum: false,
            is_https: true,
            has_hsts: true,
            has_csp: false,
            has_x_frame: true,
            has_x_content_type: true,
            mixed_content_count: 0,
            links: vec![link],
            images: vec![image],
            schemas: vec![schema],
            hreflangs: vec![hreflang],
            issues: vec![issue],
        };

        assert_eq!(page_report.status_code, 200);
        assert_eq!(page_report.links.len(), 1);

        // Serialization test
        let json = serde_json::to_string(&page_report).expect("Failed to serialize PageReport");
        assert!(json.contains("crawl-test-1"));
        assert!(json.contains("Homepage"));

        let summary = CrawlSummary {
            session_id: "test-session".to_string(),
            target_url: "https://example.com/".to_string(),
            started_at: "2026-09-04T12:00:00Z".to_string(),
            finished_at: None,
            total_pages_crawled: 1,
            total_links_discovered: 1,
            total_errors: 0,
            total_alerts: 0,
            total_warnings: 0,
            total_notices: 0,
            average_ttfb_ms: 120,
            p95_ttfb_ms: 120,
            health_score: 100,
        };
        assert_eq!(summary.health_score, 100);
    }
}
