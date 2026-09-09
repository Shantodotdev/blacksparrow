//! # Technical SEO Master Rule Catalog
//!
//! Authoritative dictionary defining unique error codes, severity ratings,
//! audit categories, human-readable descriptions, and remediation guidance.
//!
//! Complies with the 120-check technical SEO audit specification in `docs/rules.md`.
//!
//! ## Examples
//!
//! ```rust
//! use blacksparrow::rules::catalog::{get_rule, RuleId};
//! use blacksparrow::core::models::Severity;
//!
//! let rule = get_rule(RuleId::ErrTitleMissing);
//! assert_eq!(rule.severity, Severity::Critical);
//! assert_eq!(rule.code(), "ERR_TITLE_MISSING");
//! assert!(rule.description.contains("title"));
//! ```

pub use crate::core::models::RuleId;
use crate::core::models::{IssueCategory, Severity};

impl RuleId {
    /// Resolves a standardized `SCREAMING_SNAKE_CASE` string code into its corresponding [`RuleId`].
    ///
    /// Returns `Some(RuleId)` if the string code corresponds to a known rule, or `None` if unrecognized.
    ///
    /// # Arguments
    ///
    /// * `code` - Rule identifier string (e.g. `"ERR_TITLE_MISSING"`).
    pub fn from_code(code: &str) -> Option<Self> {
        RULE_CATALOG.iter().find(|r| r.code() == code).map(|r| r.id)
    }
}

/// Metadata definition of an individual technical SEO audit rule.
///
/// Encapsulates the rule's strongly typed identity, grouping category, severity classification,
/// descriptive title, defect explanation, and actionable remediation advice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleDefinition {
    /// Strongly-typed identifier.
    pub id: RuleId,
    /// Functional audit category for grouping and reporting.
    pub category: IssueCategory,
    /// Severity classification tier.
    pub severity: Severity,
    /// Concise human-readable headline.
    pub title: &'static str,
    /// Comprehensive explanation of the defect and search engine impact.
    pub description: &'static str,
    /// Concrete engineering instructions to resolve the defect.
    pub fix_advice: &'static str,
}

impl RuleDefinition {
    /// Standard string code (e.g. `"ERR_TITLE_MISSING"`), derived from the strongly-typed [`RuleId`].
    #[inline]
    pub const fn code(&self) -> &'static str {
        self.id.as_str()
    }

    /// Constructs an [`crate::core::models::IssueFinding`] instance from this rule definition.
    ///
    /// # Arguments
    ///
    /// * `target_url` - The URL where the defect was identified.
    /// * `custom_message` - Optional context-specific detail. If `None`, defaults to the rule's standard description.
    pub fn to_finding(
        &self,
        target_url: &str,
        custom_message: Option<&str>,
    ) -> crate::core::models::IssueFinding {
        crate::core::models::IssueFinding {
            code: self.id,
            category: self.category,
            severity: self.severity,
            title: compact_str::CompactString::new(self.title),
            message: custom_message
                .map(|m| m.to_string())
                .unwrap_or_else(|| self.description.to_string()),
            target_url: target_url.to_string(),
            source_page_url: None,
        }
    }
}

/// Static catalog of all single-page in-flight technical SEO checks.
pub static RULE_CATALOG: &[RuleDefinition] = &[
    // --- Category 1: HTTP Status & Transport ---
    RuleDefinition {
        id: RuleId::ErrHttp4xxClientError,
        category: IssueCategory::HttpTransport,
        severity: Severity::Critical,
        title: "HTTP 4xx Client Error",
        description: "The page returned a 4xx client error (e.g. 404 Not Found, 403 Forbidden, 410 Gone), preventing access.",
        fix_advice: "Restore the missing URL or implement a permanent 301 redirect to the closest equivalent content.",
    },
    RuleDefinition {
        id: RuleId::ErrHttp5xxServerError,
        category: IssueCategory::HttpTransport,
        severity: Severity::Critical,
        title: "HTTP 5xx Server Error",
        description: "The web server returned a 5xx internal server failure (e.g. 500, 502, 503, 504), crashing the request.",
        fix_advice: "Inspect server application logs, database connection pools, and upstream reverse proxy configurations.",
    },
    RuleDefinition {
        id: RuleId::InfoHttp301PermanentRedirect,
        category: IssueCategory::HttpTransport,
        severity: Severity::Notice,
        title: "HTTP 301 Moved Permanently",
        description: "The requested URL permanently redirected to another destination.",
        fix_advice: "Update internal hyperlinks directly to the final destination URL to avoid unnecessary redirect hops.",
    },
    RuleDefinition {
        id: RuleId::InfoHttp302TemporaryRedirect,
        category: IssueCategory::HttpTransport,
        severity: Severity::Warning,
        title: "HTTP 302 Found (Temporary Redirect)",
        description: "The page returned a temporary redirect. Search engines may not transfer link equity (PageRank) across temporary hops.",
        fix_advice: "Change temporary 302 redirects to permanent 301 redirects if the move is permanent.",
    },
    RuleDefinition {
        id: RuleId::InfoHttp307_308Redirect,
        category: IssueCategory::HttpTransport,
        severity: Severity::Notice,
        title: "HTTP 307/308 Redirect",
        description: "Method-preserving temporary (307) or permanent (308) HTTP redirect detected.",
        fix_advice: "Update referencing internal links directly to the destination URL if permanent.",
    },
    RuleDefinition {
        id: RuleId::AlertWafBotChallenge,
        category: IssueCategory::HttpTransport,
        severity: Severity::Alert,
        title: "WAF Anti-Bot Challenge Screen Detected",
        description: "A Cloudflare, Akamai, DataDome, or Imperva challenge screen intercepted the crawler.",
        fix_advice: "Allowlist the crawler IP address or pass session cookies to audit protected pages.",
    },
    RuleDefinition {
        id: RuleId::WarnSlowTtfb,
        category: IssueCategory::HttpTransport,
        severity: Severity::Warning,
        title: "Slow Server Response Time (TTFB)",
        description: "Time to First Byte (TTFB) exceeded 1,800 ms, indicating severe origin database or compute latency.",
        fix_advice: "Enable edge caching (CDN), optimize backend database queries, or increase server resources.",
    },
    RuleDefinition {
        id: RuleId::AlertFacetedSpiderTrap,
        category: IssueCategory::HttpTransport,
        severity: Severity::Alert,
        title: "Faceted Navigation Spider Trap",
        description: "The URL contains excessive faceted filter or sorting parameters without proper canonicalization, risking search engine crawl budget waste.",
        fix_advice: "Implement self-referencing canonical tags to base URLs or disallow parameterized filter combinations in robots.txt.",
    },
    RuleDefinition {
        id: RuleId::ErrHttpSoft404,
        category: IssueCategory::HttpTransport,
        severity: Severity::Critical,
        title: "Soft 404 Error Detected",
        description: "Page returned HTTP 200 OK but content contains '404 Not Found' or error messaging, confusing search engines.",
        fix_advice: "Return a true HTTP 404 Not Found or 410 Gone status code, or 301 redirect to relevant content.",
    },

    // --- Category 2: Titles & Metadata ---
    RuleDefinition {
        id: RuleId::ErrTitleMissing,
        category: IssueCategory::TitleMetadata,
        severity: Severity::Critical,
        title: "Missing Document Title",
        description: "The HTML document lacks a `<title>` tag, preventing search engines from generating snippet headlines.",
        fix_advice: "Add a concise, descriptive `<title>` element inside the `<head>` block.",
    },
    RuleDefinition {
        id: RuleId::WarnTitleTooShort,
        category: IssueCategory::TitleMetadata,
        severity: Severity::Warning,
        title: "Document Title Too Short",
        description: "The `<title>` is under 30 characters, which may lack descriptive keyword context.",
        fix_advice: "Expand the document title to between 30 and 60 characters with relevant brand and subject terms.",
    },
    RuleDefinition {
        id: RuleId::WarnTitleTooLong,
        category: IssueCategory::TitleMetadata,
        severity: Severity::Warning,
        title: "Document Title Too Long",
        description: "The `<title>` exceeds 60 characters (approximately 600px pixel width) and will be truncated in SERPs.",
        fix_advice: "Shorten the title to 50–60 characters to fit search snippet display boundaries.",
    },
    RuleDefinition {
        id: RuleId::ErrTitleMultiple,
        category: IssueCategory::TitleMetadata,
        severity: Severity::Alert,
        title: "Multiple Title Tags",
        description: "The page defines more than one `<title>` element in the DOM, creating indexation ambiguity.",
        fix_advice: "Consolidate into a single authoritative `<title>` tag in the `<head>`.",
    },
    RuleDefinition {
        id: RuleId::WarnTitleWhitespacePadded,
        category: IssueCategory::TitleMetadata,
        severity: Severity::Notice,
        title: "Title Has Irregular Whitespace",
        description: "The title string contains superfluous leading, trailing, or consecutive internal spaces.",
        fix_advice: "Trim leading and trailing whitespace from the `<title>` tag template.",
    },
    RuleDefinition {
        id: RuleId::WarnTitleSameAsH1,
        category: IssueCategory::TitleMetadata,
        severity: Severity::Warning,
        title: "Title Tag Identical to H1",
        description: "The document <title> text exactly matches the primary <h1> heading, missing an opportunity to target complementary keywords.",
        fix_advice: "Differentiate the <title> tag for search result snippets and the <h1> tag for on-page visitors.",
    },
    RuleDefinition {
        id: RuleId::WarnMetaDescMissing,
        category: IssueCategory::TitleMetadata,
        severity: Severity::Warning,
        title: "Missing Meta Description",
        description: "The document lacks a `<meta name=\"description\">` tag, forcing search engines to generate arbitrary snippet text.",
        fix_advice: "Add an actionable meta description between 70 and 160 characters summarizing the page.",
    },
    RuleDefinition {
        id: RuleId::WarnMetaDescTooShort,
        category: IssueCategory::TitleMetadata,
        severity: Severity::Warning,
        title: "Meta Description Too Short",
        description: "The meta description is shorter than 70 characters, missing an opportunity to attract user clicks.",
        fix_advice: "Elaborate the description to between 70 and 160 characters with clear call-to-action text.",
    },
    RuleDefinition {
        id: RuleId::WarnMetaDescTooLong,
        category: IssueCategory::TitleMetadata,
        severity: Severity::Warning,
        title: "Meta Description Too Long",
        description: "The meta description exceeds 160 characters and will be truncated by search engines.",
        fix_advice: "Condense the summary to 140–160 characters to prevent snippet truncation.",
    },
    RuleDefinition {
        id: RuleId::ErrMetaDescMultiple,
        category: IssueCategory::TitleMetadata,
        severity: Severity::Alert,
        title: "Multiple Meta Description Tags",
        description: "Multiple `<meta name=\"description\">` tags exist in the document.",
        fix_advice: "Remove duplicate meta description elements, retaining only the primary summary.",
    },
    RuleDefinition {
        id: RuleId::WarnMetaKeywordsPresent,
        category: IssueCategory::TitleMetadata,
        severity: Severity::Notice,
        title: "Obsolete Meta Keywords Tag Present",
        description: "The <meta name=\"keywords\"> tag is ignored by all major search engines and may expose internal keyword targeting to competitors.",
        fix_advice: "Safely remove the <meta name=\"keywords\"> tag from the document head.",
    },

    // --- Category 3: Headings & Hierarchy ---
    RuleDefinition {
        id: RuleId::ErrH1Missing,
        category: IssueCategory::Headings,
        severity: Severity::Critical,
        title: "Missing H1 Headline",
        description: "The page lacks a top-level `<h1>` heading, depriving crawlers of the primary topical anchor.",
        fix_advice: "Introduce a clear, prominent `<h1>` heading identifying the page topic.",
    },
    RuleDefinition {
        id: RuleId::WarnH1Multiple,
        category: IssueCategory::Headings,
        severity: Severity::Warning,
        title: "Multiple H1 Headlines",
        description: "The document defines multiple `<h1>` elements, diluting structural hierarchy.",
        fix_advice: "Ensure only one primary `<h1>` represents the document title; convert secondary headings to `<h2>`.",
    },
    RuleDefinition {
        id: RuleId::WarnH1Empty,
        category: IssueCategory::Headings,
        severity: Severity::Warning,
        title: "Empty H1 Tag",
        description: "An `<h1>` tag exists but contains no textual content or anchor text.",
        fix_advice: "Populate the `<h1>` with descriptive text or remove the empty element.",
    },
    RuleDefinition {
        id: RuleId::WarnH1TooLong,
        category: IssueCategory::Headings,
        severity: Severity::Warning,
        title: "H1 Headline Too Long",
        description: "The `<h1>` heading exceeds 70 characters, reading more like a paragraph than a headline.",
        fix_advice: "Condense the headline to under 70 characters.",
    },
    RuleDefinition {
        id: RuleId::WarnHeadingHierarchySkipped,
        category: IssueCategory::Headings,
        severity: Severity::Warning,
        title: "Skipped Heading Hierarchy Level",
        description: "Heading tags skip structural tiers (e.g. jumping directly from H1 to H3 or H4 without intermediate levels).",
        fix_advice: "Nest headings sequentially (H1 -> H2 -> H3) to maintain accessible document outlines.",
    },
    RuleDefinition {
        id: RuleId::WarnDuplicateHeadingText,
        category: IssueCategory::Headings,
        severity: Severity::Warning,
        title: "Duplicate Heading Text",
        description: "Multiple headings share identical text content, creating repetitive structural hierarchy.",
        fix_advice: "Author distinctive, descriptive heading text reflecting section-specific topics.",
    },
    RuleDefinition {
        id: RuleId::WarnExcessiveDomDepth,
        category: IssueCategory::Headings,
        severity: Severity::Warning,
        title: "Excessive DOM Element Depth",
        description: "The HTML document contains more than 1,500 DOM elements, slowing parsing and increasing memory pressure.",
        fix_advice: "Simplify DOM structure, remove unnecessary wrapper elements, and paginate or virtualize large lists.",
    },

    // --- Category 4: Directives & Robots ---
    RuleDefinition {
        id: RuleId::AlertIndexingBlockedNoindex,
        category: IssueCategory::Indexability,
        severity: Severity::Alert,
        title: "Page Blocked by noindex Directive",
        description: "The document specifies `noindex` via `<meta name=\"robots\">` or HTTP `X-Robots-Tag`, preventing search indexing.",
        fix_advice: "Remove `noindex` if this page is intended to rank in public search engine results.",
    },
    RuleDefinition {
        id: RuleId::WarnLinkEquityBlockedNofollow,
        category: IssueCategory::Indexability,
        severity: Severity::Warning,
        title: "Page Specifies nofollow Directive",
        description: "The page specifies `nofollow`, instructing search engines not to crawl or pass equity to outbound hyperlinks.",
        fix_advice: "Remove `nofollow` unless you explicitly intend to isolate all links on this page.",
    },
    RuleDefinition {
        id: RuleId::WarnNoarchivePresent,
        category: IssueCategory::Indexability,
        severity: Severity::Notice,
        title: "Page Specifies noarchive",
        description: "The page instructs search engines not to cache or display cached versions in search results.",
        fix_advice: "Review if disabling search caching is intentional for this resource.",
    },
    RuleDefinition {
        id: RuleId::WarnNosnippetPresent,
        category: IssueCategory::Indexability,
        severity: Severity::Notice,
        title: "Page Specifies nosnippet",
        description: "The page forbids search engines from showing text snippets or video previews in search results.",
        fix_advice: "Review if disabling text snippets is intentional.",
    },
    RuleDefinition {
        id: RuleId::WarnPaginationMissingCanonical,
        category: IssueCategory::Indexability,
        severity: Severity::Warning,
        title: "Paginated Page Missing Canonical",
        description: "A paginated URL (?page=N or /page/N) lacks a canonical link tag, risking duplicate indexing or ranking cannibalization.",
        fix_advice: "Add a self-referencing canonical tag to each paginated page or link to a view-all version.",
    },
    RuleDefinition {
        id: RuleId::AlertPaginationNoindex,
        category: IssueCategory::Indexability,
        severity: Severity::Alert,
        title: "Paginated Page Blocked by noindex",
        description: "A paginated component page specifies noindex, which can prevent discovery and crawling of deep internal links over time.",
        fix_advice: "Allow pagination pages to be indexed with self-referencing canonicals rather than noindex.",
    },
    RuleDefinition {
        id: RuleId::AlertUnrenderedSpaHeuristic,
        category: IssueCategory::Indexability,
        severity: Severity::Alert,
        title: "Unrendered SPA Container Detected",
        description: "Page appears to be a client-side Single Page Application (SPA) container with empty root elements and minimal server-rendered HTML.",
        fix_advice: "Implement server-side rendering (SSR), static site generation (SSG), or pre-rendering for search crawlers.",
    },

    // --- Category 5: Canonicalization ---
    RuleDefinition {
        id: RuleId::WarnCanonicalMissing,
        category: IssueCategory::Canonicalization,
        severity: Severity::Warning,
        title: "Missing Canonical Tag",
        description: "The document does not declare `<link rel=\"canonical\">`, increasing vulnerability to duplicate content cannibalization.",
        fix_advice: "Add a self-referencing absolute canonical URL to `<head>`.",
    },
    RuleDefinition {
        id: RuleId::ErrCanonicalRelative,
        category: IssueCategory::Canonicalization,
        severity: Severity::Alert,
        title: "Relative Canonical URL",
        description: "The `<link rel=\"canonical\">` tag contains a relative path instead of a fully-qualified absolute URL.",
        fix_advice: "Update canonical target to include scheme and domain (e.g. `https://example.com/page`).",
    },
    RuleDefinition {
        id: RuleId::ErrCanonicalMultiple,
        category: IssueCategory::Canonicalization,
        severity: Severity::Alert,
        title: "Multiple Canonical Tags",
        description: "Multiple `<link rel=\"canonical\">` declarations exist on the page.",
        fix_advice: "Retain exactly one authoritative canonical URL per page.",
    },
    RuleDefinition {
        id: RuleId::AlertCanonicalMismatch,
        category: IssueCategory::Canonicalization,
        severity: Severity::Alert,
        title: "Canonical URL Mismatch",
        description: "The declared canonical URL points to a different resource than the current request URL.",
        fix_advice: "Verify whether this page is meant to be indexed or consolidated under the canonical target.",
    },
    RuleDefinition {
        id: RuleId::AlertCanonicalCrossDomain,
        category: IssueCategory::Canonicalization,
        severity: Severity::Alert,
        title: "Cross-Domain Canonical URL",
        description: "The canonical tag points to an external third-party domain rather than the current website host.",
        fix_advice: "Verify that canonicalizing to an external domain is intended for syndication, otherwise use an internal URL.",
    },
    RuleDefinition {
        id: RuleId::WarnCanonicalToUnverifiedHttp,
        category: IssueCategory::Canonicalization,
        severity: Severity::Warning,
        title: "HTTPS Page Canonicalizes to HTTP",
        description: "An HTTPS page specifies an insecure HTTP URL in its canonical tag, downgrading security signaling.",
        fix_advice: "Update canonical target URLs to use the secure https:// scheme.",
    },

    // --- Category 6: Modern Security & Transport ---
    RuleDefinition {
        id: RuleId::ErrSecurityInsecureHttp,
        category: IssueCategory::Security,
        severity: Severity::Critical,
        title: "Insecure HTTP Protocol",
        description: "The URL is served over unencrypted HTTP rather than HTTPS.",
        fix_advice: "Configure an SSL/TLS certificate and enforce automatic 301 redirects from HTTP to HTTPS.",
    },
    RuleDefinition {
        id: RuleId::ErrSecurityMixedContent,
        category: IssueCategory::Security,
        severity: Severity::Critical,
        title: "Mixed Content Security Violation",
        description: "The HTTPS document references insecure `http://` subresources (images, iframes, scripts).",
        fix_advice: "Update all asset references to use HTTPS or protocol-relative schemes.",
    },
    RuleDefinition {
        id: RuleId::WarnSecurityMissingHsts,
        category: IssueCategory::Security,
        severity: Severity::Warning,
        title: "Missing HSTS Security Header",
        description: "The response lacks the `Strict-Transport-Security` header, leaving users vulnerable to SSL stripping attacks.",
        fix_advice: "Enable `Strict-Transport-Security: max-age=31536000; includeSubDomains` in server headers.",
    },
    RuleDefinition {
        id: RuleId::WarnSecurityMissingCsp,
        category: IssueCategory::Security,
        severity: Severity::Warning,
        title: "Missing Content-Security-Policy (CSP)",
        description: "The server does not transmit a `Content-Security-Policy` header to prevent Cross-Site Scripting (XSS).",
        fix_advice: "Define an appropriate Content-Security-Policy header restricting unauthorized script origins.",
    },
    RuleDefinition {
        id: RuleId::WarnSecurityMissingXFrameOptions,
        category: IssueCategory::Security,
        severity: Severity::Notice,
        title: "Missing X-Frame-Options Header",
        description: "The document lacks `X-Frame-Options` or CSP `frame-ancestors`, leaving it susceptible to clickjacking.",
        fix_advice: "Add `X-Frame-Options: SAMEORIGIN` or `DENY`.",
    },
    RuleDefinition {
        id: RuleId::WarnSecurityMissingXContentType,
        category: IssueCategory::Security,
        severity: Severity::Notice,
        title: "Missing X-Content-Type-Options Header",
        description: "The response lacks `X-Content-Type-Options: nosniff` to prevent MIME-confusion exploits.",
        fix_advice: "Include `X-Content-Type-Options: nosniff` on all HTTP responses.",
    },
    RuleDefinition {
        id: RuleId::WarnSecurityMissingReferrerPolicy,
        category: IssueCategory::Security,
        severity: Severity::Notice,
        title: "Missing Referrer-Policy Header",
        description: "The HTTP response does not specify a Referrer-Policy header, risking referrer information leakage.",
        fix_advice: "Configure `Referrer-Policy: strict-origin-when-cross-origin` or similar secure policy in server headers.",
    },
    RuleDefinition {
        id: RuleId::WarnSecurityTargetBlankNoOpener,
        category: IssueCategory::Security,
        severity: Severity::Warning,
        title: "Target Blank Link Missing noopener",
        description: "An external link with `target=\"_blank\"` lacks `rel=\"noopener\"` or `rel=\"noreferrer\"`, exposing reverse tabnabbing vulnerabilities.",
        fix_advice: "Add `rel=\"noopener noreferrer\"` to all hyperlinks opening in new windows.",
    },
    RuleDefinition {
        id: RuleId::WarnSecurityInsecureForm,
        category: IssueCategory::Security,
        severity: Severity::Alert,
        title: "Insecure Form Action URL",
        description: "An HTML `<form>` action points to an unencrypted HTTP URL, risking cleartext transmission of user form data.",
        fix_advice: "Ensure all form actions submit to secure HTTPS endpoints.",
    },

    // --- Category 7: Images & Core Web Vitals (CLS) ---
    RuleDefinition {
        id: RuleId::WarnImageMissingAlt,
        category: IssueCategory::TitleMetadata,
        severity: Severity::Warning,
        title: "Missing Image Alt Attribute",
        description: "An embedded `<img>` tag lacks an `alt` attribute, harming accessibility and image search indexing.",
        fix_advice: "Provide concise, descriptive alternative text describing the image subject.",
    },
    RuleDefinition {
        id: RuleId::WarnImageMissingDimensions,
        category: IssueCategory::MobileUx,
        severity: Severity::Warning,
        title: "Missing Image Dimensions (CLS Risk)",
        description: "An `<img>` tag lacks explicit `width` or `height` attributes, triggering Cumulative Layout Shift.",
        fix_advice: "Declare explicit width and height attributes or CSS aspect-ratio properties on all images.",
    },
    RuleDefinition {
        id: RuleId::WarnImageDataUri,
        category: IssueCategory::MobileUx,
        severity: Severity::Notice,
        title: "Large Inline Data URI Image",
        description: "An image is embedded directly in the HTML as a base64 data URI, bloating initial document transfer.",
        fix_advice: "Host images externally as optimized WebP or AVIF assets.",
    },
    RuleDefinition {
        id: RuleId::WarnImgAltTooLong,
        category: IssueCategory::TitleMetadata,
        severity: Severity::Notice,
        title: "Image Alt Text Too Long",
        description: "An image alt attribute exceeds 125 characters, reading more like a paragraph than a concise description.",
        fix_advice: "Shorten image alt descriptions to 125 characters or fewer for screen readers and search engines.",
    },

    // --- Category 8: Mobile UX & Viewports ---
    RuleDefinition {
        id: RuleId::ErrMobileNoViewport,
        category: IssueCategory::MobileUx,
        severity: Severity::Critical,
        title: "Missing Mobile Viewport Configuration",
        description: "The document lacks `<meta name=\"viewport\">`, breaking mobile rendering and Google Mobile-First Indexing.",
        fix_advice: "Add `<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">` to `<head>`.",
    },
    RuleDefinition {
        id: RuleId::WarnMobileViewportNonScalable,
        category: IssueCategory::MobileUx,
        severity: Severity::Warning,
        title: "Viewport Disables User Zooming",
        description: "The viewport tag disables user zoom (`user-scalable=no` or `maximum-scale=1.0`), violating accessibility guidelines.",
        fix_advice: "Allow users to scale and zoom content freely on mobile devices.",
    },
    RuleDefinition {
        id: RuleId::WarnPerfLargeHtmlPayload,
        category: IssueCategory::MobileUx,
        severity: Severity::Warning,
        title: "Large HTML Payload Size",
        description: "The uncompressed HTML response exceeds 1.5 MB, increasing download latency and mobile memory consumption.",
        fix_advice: "Minify HTML, reduce inline CSS/JS, and defer heavy scripts or large embedded payloads.",
    },
    RuleDefinition {
        id: RuleId::ErrPerfExcessiveHtmlPayload,
        category: IssueCategory::MobileUx,
        severity: Severity::Critical,
        title: "Excessive HTML Payload Size",
        description: "The uncompressed HTML response exceeds 3.0 MB, severely hurting mobile performance and crawl efficiency.",
        fix_advice: "Split large pages, paginate data tables, and offload inline media or JSON blobs to separate endpoints.",
    },

    // --- Category 9: Links & Anchor Quality ---
    RuleDefinition {
        id: RuleId::WarnLinksTooManyOnPage,
        category: IssueCategory::Links,
        severity: Severity::Warning,
        title: "Excessive Links On Page",
        description: "Page contains more than 250 hyperlinks, diluting internal link equity distribution across pages.",
        fix_advice: "Reduce excessive link counts and prune redundant header/footer or mega-menu links.",
    },
    RuleDefinition {
        id: RuleId::WarnLinkSuspiciousAnchor,
        category: IssueCategory::Links,
        severity: Severity::Warning,
        title: "Non-Descriptive Generic Anchor Text",
        description: "Hyperlink uses generic anchor text (e.g. 'click here', 'read more') lacking descriptive topical context.",
        fix_advice: "Replace generic anchor text with descriptive keywords communicating the target page topic.",
    },
    RuleDefinition {
        id: RuleId::WarnLinkEmptyAnchor,
        category: IssueCategory::Links,
        severity: Severity::Warning,
        title: "Empty Hyperlink Anchor Text",
        description: "A text hyperlink contains zero text or only whitespace characters, hindering screen readers and search discovery.",
        fix_advice: "Provide descriptive anchor text or an aria-label attribute for every text link.",
    },

    // --- Category 9: Structured Data & Schema.org ---
    RuleDefinition {
        id: RuleId::ErrSchemaSyntaxError,
        category: IssueCategory::StructuredData,
        severity: Severity::Alert,
        title: "Structured Data Syntax Error",
        description: "A `<script type=\"application/ld+json\">` block contains invalid JSON syntax and cannot be parsed.",
        fix_advice: "Validate and format JSON-LD payloads using standard JSON parsers.",
    },
    RuleDefinition {
        id: RuleId::WarnSchemaMissingRequiredFields,
        category: IssueCategory::StructuredData,
        severity: Severity::Warning,
        title: "Schema Missing Required Fields",
        description: "A structured data block lacks required fields needed to qualify for Google Rich Results.",
        fix_advice: "Provide all mandatory fields per Google Search Central structured data specifications.",
    },
    RuleDefinition {
        id: RuleId::WarnSchemaMultipleProductEntities,
        category: IssueCategory::StructuredData,
        severity: Severity::Warning,
        title: "Multiple Product Schema Entities",
        description: "Document defines multiple top-level Product schemas, creating ambiguity for Google Rich Results.",
        fix_advice: "Consolidate into a single primary Product entity or nest secondary items under isRelatedTo or hasVariant.",
    },
    RuleDefinition {
        id: RuleId::WarnSchemaInvalidDateFormat,
        category: IssueCategory::StructuredData,
        severity: Severity::Warning,
        title: "Invalid Schema Date Format",
        description: "Structured data date field fails ISO 8601 date format validation.",
        fix_advice: "Format dates using standard ISO 8601 format (YYYY-MM-DD or YYYY-MM-DDTHH:MM:SSZ).",
    },
    RuleDefinition {
        id: RuleId::ErrProductMissingSchema,
        category: IssueCategory::StructuredData,
        severity: Severity::Alert,
        title: "Product Page Missing Product Schema",
        description: "The page exhibits clear e-commerce product intent (price, transactional buttons, cart elements) but lacks Schema.org Product structured data.",
        fix_advice: "Add JSON-LD Product structured data with name, offers, price, and availability to qualify for Google Product rich snippets.",
    },
    RuleDefinition {
        id: RuleId::WarnProductMissingPriceOffer,
        category: IssueCategory::StructuredData,
        severity: Severity::Warning,
        title: "Product Schema Missing Offer or Price",
        description: "Product structured data is present but lacks an 'offers' entity with price and priceCurrency required by Google Search.",
        fix_advice: "Include an 'offers' specification containing valid 'price' and 'priceCurrency' properties in the Product JSON-LD block.",
    },
    RuleDefinition {
        id: RuleId::WarnProductMissingAvailability,
        category: IssueCategory::StructuredData,
        severity: Severity::Warning,
        title: "Product Schema Missing Availability Status",
        description: "Product structured data lacks an 'availability' status (e.g. InStock, OutOfStock) in its offers.",
        fix_advice: "Specify the schema.org availability URI (e.g. 'https://schema.org/InStock') inside the product offer object.",
    },
    RuleDefinition {
        id: RuleId::ErrArticleMissingSchema,
        category: IssueCategory::StructuredData,
        severity: Severity::Alert,
        title: "Article Page Missing Article Schema",
        description: "The page exhibits editorial article or blog intent (longform narrative, author byline, publication date) but lacks Schema.org Article, NewsArticle, or BlogPosting structured data.",
        fix_advice: "Add JSON-LD Article structured data with headline, author, and datePublished to qualify for Google News and Top Stories.",
    },
    RuleDefinition {
        id: RuleId::WarnArticleMissingAuthor,
        category: IssueCategory::StructuredData,
        severity: Severity::Warning,
        title: "Article Schema Missing Author",
        description: "Article structured data is present but lacks an 'author' Person or Organization property required for Google News and E-E-A-T trust signals.",
        fix_advice: "Add an 'author' entity of type Person or Organization with a 'name' property to the Article schema.",
    },
    RuleDefinition {
        id: RuleId::WarnArticleMissingDatePublished,
        category: IssueCategory::StructuredData,
        severity: Severity::Warning,
        title: "Article Schema Missing Date Published",
        description: "Article structured data lacks a 'datePublished' ISO 8601 timestamp required for search snippet publication dating.",
        fix_advice: "Include 'datePublished' in ISO 8601 format (e.g. '2025-10-14T08:00:00Z') in the Article structured data.",
    },
    RuleDefinition {
        id: RuleId::WarnOrgMissingLocalSchema,
        category: IssueCategory::StructuredData,
        severity: Severity::Warning,
        title: "Contact Page Missing Organization/LocalBusiness Schema",
        description: "The page is identified as a Contact or reach-us page, but lacks Schema.org Organization or LocalBusiness structured data.",
        fix_advice: "Add JSON-LD Organization or LocalBusiness structured data containing telephone, address, and contactPoint details.",
    },

    // --- Category 10: AI Search & Content Quality ---
    RuleDefinition {
        id: RuleId::WarnContentThin,
        category: IssueCategory::GeoAiSearch,
        severity: Severity::Warning,
        title: "Thin Editorial Content",
        description: "The page has fewer than 200 words of editorial body content, risking classification as low quality.",
        fix_advice: "Expand the page with comprehensive, helpful content addressing user intent.",
    },
    RuleDefinition {
        id: RuleId::WarnLoremIpsumDetected,
        category: IssueCategory::GeoAiSearch,
        severity: Severity::Warning,
        title: "Placeholder Lorem Ipsum Text Detected",
        description: "The document contains placeholder dummy text (\"Lorem ipsum\"), indicating unfinished production staging.",
        fix_advice: "Replace dummy placeholder text with finalized editorial copy before indexing.",
    },
    RuleDefinition {
        id: RuleId::AlertAiSearchBotsBlocked,
        category: IssueCategory::GeoAiSearch,
        severity: Severity::Alert,
        title: "AI Search Citation Bots Blocked",
        description: "Robots.txt disallows AI search and citation crawlers (e.g. GPTBot, PerplexityBot, ClaudeBot), preventing discovery in AI search overviews.",
        fix_advice: "Remove Disallow directives for search citation user-agents in robots.txt if AI search visibility is desired.",
    },
    RuleDefinition {
        id: RuleId::WarnLlmsTxtMissing,
        category: IssueCategory::GeoAiSearch,
        severity: Severity::Warning,
        title: "Missing /llms.txt File",
        description: "The website does not publish a /llms.txt file to guide AI crawlers and LLMs.",
        fix_advice: "Publish a /llms.txt Markdown file at the domain root providing a structured overview of site content for LLMs.",
    },

    // --- Category 11: Internationalization & Hreflang ---
    RuleDefinition {
        id: RuleId::ErrHreflangNotReciprocal,
        category: IssueCategory::Internationalization,
        severity: Severity::Critical,
        title: "Non-Reciprocal Hreflang Alternate",
        description: "Page declares an alternate language URL, but the target page does not reciprocally link back.",
        fix_advice: "Ensure return hreflang links exist bidirectionally between all localized versions.",
    },
    RuleDefinition {
        id: RuleId::ErrHreflangToNonCanonical,
        category: IssueCategory::Internationalization,
        severity: Severity::Alert,
        title: "Hreflang Points to Non-Canonical URL",
        description: "Hreflang alternate points to a target URL that declares a different canonical destination.",
        fix_advice: "Update hreflang annotations to point exclusively to authoritative canonical URLs.",
    },
    RuleDefinition {
        id: RuleId::ErrHreflangToBrokenOrRedirect,
        category: IssueCategory::Internationalization,
        severity: Severity::Critical,
        title: "Hreflang Points to Broken or Redirecting URL",
        description: "Hreflang alternate points to a target that returns a 3xx redirect, 4xx client error, or 5xx server error.",
        fix_advice: "Point hreflang annotations directly to active 200 OK canonical destinations.",
    },
    RuleDefinition {
        id: RuleId::ErrHreflangInvalidLangCode,
        category: IssueCategory::Internationalization,
        severity: Severity::Critical,
        title: "Invalid Hreflang Language/Region Code",
        description: "Hreflang language or region code fails ISO 639-1 / RFC 5646 validation.",
        fix_advice: "Correct language and region codes to match standard ISO formats (e.g. 'en-US', 'es', or 'x-default').",
    },
    RuleDefinition {
        id: RuleId::WarnHreflangCrossDomain,
        category: IssueCategory::Internationalization,
        severity: Severity::Warning,
        title: "Cross-Domain Hreflang Alternate",
        description: "Hreflang tag references an external domain rather than the internal crawl host.",
        fix_advice: "Verify that cross-domain hreflang clusters are reciprocal and intentionally configured.",
    },
    RuleDefinition {
        id: RuleId::WarnHreflangMissingXDefault,
        category: IssueCategory::Internationalization,
        severity: Severity::Warning,
        title: "Missing Hreflang x-default Tag",
        description: "Hreflang cluster does not specify an 'x-default' fallback page for unmatched languages.",
        fix_advice: "Add hreflang='x-default' pointing to the global landing or language selector page.",
    },
    RuleDefinition {
        id: RuleId::ErrHreflangMissingSelfReference,
        category: IssueCategory::Internationalization,
        severity: Severity::Critical,
        title: "Missing Self-Referencing Hreflang",
        description: "Page declares alternate hreflang tags for other languages but omits a self-referencing tag for its own URL.",
        fix_advice: "Include a self-referencing hreflang tag matching the current page's URL.",
    },
    RuleDefinition {
        id: RuleId::WarnHtmlLangMissing,
        category: IssueCategory::Internationalization,
        severity: Severity::Warning,
        title: "Missing HTML Lang Attribute",
        description: "The root <html> tag lacks a lang attribute or contains empty whitespace.",
        fix_advice: "Declare the primary document language on the root <html> tag (e.g. <html lang='en'>).",
    },

    // --- Category 12: Site-Wide Graph & Architecture (Post-Crawl) ---
    RuleDefinition {
        id: RuleId::AlertGraphOrphanPage,
        category: IssueCategory::SiteGraph,
        severity: Severity::Alert,
        title: "Orphan Page Detected",
        description: "URL discovered in XML sitemap receives zero internal incoming links from any crawled page.",
        fix_advice: "Add internal contextual links from relevant parent, category, or navigation pages.",
    },
    RuleDefinition {
        id: RuleId::ErrGraphRedirectLoop,
        category: IssueCategory::SiteGraph,
        severity: Severity::Critical,
        title: "Circular Redirect Loop",
        description: "Circular redirect sequence detected between pages, causing crawlers and browsers to fail completely.",
        fix_advice: "Break the redirect cycle by redirecting directly to the intended final destination URL.",
    },
    RuleDefinition {
        id: RuleId::WarnGraphRedirectChain,
        category: IssueCategory::SiteGraph,
        severity: Severity::Warning,
        title: "Multi-Hop Redirect Chain",
        description: "Redirect sequence exceeds 1 hop, introducing unnecessary latency and wasting crawl budget.",
        fix_advice: "Update initial redirect rule to point directly to final destination URL.",
    },
    RuleDefinition {
        id: RuleId::ErrGraphCanonicalLoop,
        category: IssueCategory::SiteGraph,
        severity: Severity::Critical,
        title: "Circular Canonical Loop",
        description: "Circular canonical references detected between pages, invalidating canonicalization.",
        fix_advice: "Designate a single authoritative URL and ensure all canonical tags point directly to it.",
    },
    RuleDefinition {
        id: RuleId::WarnGraphExactDuplicateContent,
        category: IssueCategory::SiteGraph,
        severity: Severity::Alert,
        title: "Exact Duplicate Content",
        description: "Multiple distinct URLs share identical text content, leading to keyword cannibalization and wasted crawl budget.",
        fix_advice: "Consolidate duplicate pages using 301 redirects or designate the authoritative page with a canonical tag.",
    },
    RuleDefinition {
        id: RuleId::WarnGraphNearDuplicateContent,
        category: IssueCategory::SiteGraph,
        severity: Severity::Warning,
        title: "Near-Duplicate Content",
        description: "Multiple distinct URLs share 85% or greater SimHash content similarity, competing against each other.",
        fix_advice: "Add substantially unique, comprehensive content or consolidate into a single authoritative resource.",
    },
    RuleDefinition {
        id: RuleId::WarnGraphDuplicateTitles,
        category: IssueCategory::SiteGraph,
        severity: Severity::Warning,
        title: "Duplicate Title Tags Across URLs",
        description: "Distinct URLs share the exact same document title, preventing search engines from distinguishing page topics.",
        fix_advice: "Author unique, descriptive title tags for every indexable page.",
    },
    RuleDefinition {
        id: RuleId::WarnGraphDuplicateMetaDescs,
        category: IssueCategory::SiteGraph,
        severity: Severity::Warning,
        title: "Duplicate Meta Descriptions Across URLs",
        description: "Distinct URLs share identical meta descriptions, reducing snippet variety and click-through rates.",
        fix_advice: "Author tailored meta descriptions highlighting each page's distinct value proposition.",
    },
    RuleDefinition {
        id: RuleId::WarnGraphDeadEndPage,
        category: IssueCategory::SiteGraph,
        severity: Severity::Warning,
        title: "Dead-End Page With No Outlinks",
        description: "Page receives internal links but contains zero outgoing links to any other page on the site.",
        fix_advice: "Add contextual internal links, related article suggestions, or navigation menus to preserve link equity.",
    },
    RuleDefinition {
        id: RuleId::WarnGraphHighCrawlDepth,
        category: IssueCategory::SiteGraph,
        severity: Severity::Warning,
        title: "High Crawl Depth",
        description: "Page requires more than 4 link hops to reach from the root seed URL, causing search crawlers to crawl it infrequently.",
        fix_advice: "Flatten site architecture by linking important pages from higher-level hub pages or category navigation.",
    },
    RuleDefinition {
        id: RuleId::WarnLowInternalPagerankHub,
        category: IssueCategory::SiteGraph,
        severity: Severity::Warning,
        title: "Low PageRank Navigation Hub",
        description: "Page contains high internal out-degree (50+ outgoing links) but receives very low PageRank equity, indicating an isolated navigation hub.",
        fix_advice: "Strengthen incoming internal link pathways to this hub from the homepage, main navigation, or high-equity category pages.",
    },
];

/// Retrieves the metadata definition for a technical SEO rule by its strongly typed [`RuleId`].
///
/// Returns the authoritative [`RuleDefinition`] containing the audit category, severity tier,
/// human-readable headline, technical explanation, and remediation advice.
///
/// # Arguments
///
/// * `id` - The strongly typed rule identifier to query.
///
/// # Examples
///
/// ```rust
/// use blacksparrow::rules::catalog::{get_rule, RuleId};
/// use blacksparrow::core::models::Severity;
///
/// let rule = get_rule(RuleId::ErrTitleMissing);
/// assert_eq!(rule.severity, Severity::Critical);
/// assert_eq!(rule.code(), "ERR_TITLE_MISSING");
/// ```
pub fn get_rule(id: RuleId) -> &'static RuleDefinition {
    match RULE_CATALOG.iter().find(|r| r.id == id) {
        Some(rule) => rule,
        None => &RULE_CATALOG[0],
    }
}

/// Retrieves a rule definition from the master catalog by its standardized string code.
///
/// Looks up rules matching the `SCREAMING_SNAKE_CASE` convention (e.g. `"ERR_TITLE_MISSING"`).
/// Returns `None` if the code does not correspond to any registered audit check.
///
/// # Arguments
///
/// * `code` - The standardized rule code string to resolve.
///
/// # Examples
///
/// ```rust
/// use blacksparrow::rules::catalog::get_rule_by_code;
///
/// assert!(get_rule_by_code("ERR_TITLE_MISSING").is_some());
/// assert!(get_rule_by_code("UNKNOWN_CODE").is_none());
/// ```
pub fn get_rule_by_code(code: &str) -> Option<&'static RuleDefinition> {
    RULE_CATALOG.iter().find(|r| r.code() == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_catalog_codes_are_unique() {
        let mut codes = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        for rule in RULE_CATALOG {
            assert!(
                codes.insert(rule.code()),
                "Duplicate rule code detected in master catalog: {}",
                rule.code()
            );
            assert!(
                ids.insert(rule.id),
                "Duplicate rule id detected in master catalog: {:?}",
                rule.id
            );
            assert_eq!(
                rule.id.as_str(),
                rule.code(),
                "Rule id as_str mismatch for {:?}",
                rule.id
            );
        }
    }

    #[test]
    fn test_every_rule_id_resolves() {
        for rule in RULE_CATALOG {
            let found = get_rule(rule.id);
            assert_eq!(found.id, rule.id);
            assert_eq!(RuleId::from_code(rule.code()), Some(rule.id));
        }
    }
}
