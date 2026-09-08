# Technical SEO Rules Catalog (120 Checks)

This catalog is the authoritative specification for all 120 automated SEO checks in the `SEO Lens` engine. It details the detection heuristics, severity classification, search impact, and remediation guidance for each rule.

---

## Severity Levels

- **Critical (Priority 1)**: Serious architectural defects that actively break indexing, block crawlers, produce error status codes, or cause search invisibility.
- **Alert (Priority 2)**: High-risk issues that degrade rankings, waste crawl budget, cause content desynchronization, or trigger duplicate penalties.
- **Warning (Priority 3)**: Suboptimal technical optimizations, metadata length violations, missing UX signals, or performance bottlenecks.
- **Notice / Info (Priority 4)**: Informational signals, redirects, or healthy confirmation states.

---

## Summary Matrix by Category

> **Quick Jump**:
>
> 1. [HTTP & Network Transport](#category-1-http-status--network-transport-checks-110)
> 2. [Titles & Basic Metadata](#category-2-titles--basic-metadata-checks-1120)
> 3. [Headings & Document Structure](#category-3-headings--document-structure-checks-2130)
> 4. [Indexability & Directives](#category-4-indexability--robots-directives-checks-3140)
> 5. [Canonicalization](#category-5-canonicalization-checks-4150)
> 6. [Links & Anchor Quality](#category-6-links--anchor-quality-checks-5160)
> 7. [Security & Modern Transport](#category-7-security--modern-transport-checks-6170)
> 8. [Mobile, Performance & UX](#category-8-mobile-performance--ux-signals-checks-7180)
> 9. [Internationalization (Hreflang)](#category-9-internationalization--hreflang-checks-8188)
> 10. [Structured Data & Rich Results](#category-10-structured-data--google-rich-results-checks-8996)
> 11. [GEO & AI Search Readiness](#category-11-geo-generative-engine-optimization--ai-search-checks-97104)
> 12. [Site Graph & Architecture](#category-12-site-wide-graph--architecture-post-crawl-checks-105114)
> 13. [JavaScript SEO Diffing](#category-13-javascript-seo-diffing-checks-when---render-js-is-enabled-checks-115120)

| Category | Checks | Phase | Focus |
| --- | --- | --- | --- |
| [**1. HTTP & Network Transport**](#category-1-http-status--network-transport-checks-110) | 10 | Phase 1 (In-Flight) | HTTP codes, timeouts, DNS/TLS errors, WAF challenges |
| [**2. Titles & Basic Metadata**](#category-2-titles--basic-metadata-checks-1120) | 10 | Phase 1 (In-Flight) | Title/description absence, lengths, multiplicity, keywords |
| [**3. Headings & Document Structure**](#category-3-headings--document-structure-checks-2130) | 10 | Phase 1 (In-Flight) | H1 presence, hierarchy order, empty tags, DOM depth |
| [**4. Indexability & Directives**](#category-4-indexability--robots-directives-checks-3140) | 10 | Phase 1 (In-Flight) | noindex, nofollow, robots.txt, pagination, SPA heuristics |
| [**5. Canonicalization**](#category-5-canonicalization-checks-4150) | 10 | Phase 1 (In-Flight) | Missing, relative, conflicting, cross-domain, chain canonicals |
| [**6. Links & Anchor Quality**](#category-6-links--anchor-quality-checks-5160) | 10 | Phase 1 & 2 | Broken links, nofollow mix, empty anchors, bookmarks |
| [**7. Security & Modern Transport**](#category-7-security--modern-transport-checks-6170) | 10 | Phase 1 (In-Flight) | HTTPS, mixed content, insecure forms, HSTS, CSP, headers |
| [**8. Mobile, Performance & UX**](#category-8-mobile-performance--ux-signals-checks-7180) | 10 | Phase 1 (In-Flight) | Viewports, TTFB, page size, image alt/dimensions (CLS) |
| [**9. Internationalization (Hreflang)**](#category-9-internationalization--hreflang-checks-8188) | 8 | Phase 1 & 2 | Reciprocity graph, self-reference, lang codes, x-default |
| [**10. Structured Data & Rich Results**](#category-10-structured-data--google-rich-results-checks-8996) | 8 | Phase 1 (In-Flight) | JSON-LD syntax, Google Rich Result required fields, OpenGraph |
| [**11. GEO & AI Search Readiness**](#category-11-geo-generative-engine-optimization--ai-search-checks-97104) | 8 | Phase 1 (In-Flight) | /llms.txt, AI training vs retrieval bots, soft 404, AI tells |
| [**12. Site Graph & Architecture**](#category-12-site-wide-graph--architecture-post-crawl-checks-105114) | 10 | Phase 2 (Post-Crawl) | Orphan pages, redirect/canonical loops, duplicates, PageRank |
| [**13. JavaScript SEO Diffing**](#category-13-javascript-seo-diffing-checks-when---render-js-is-enabled-checks-115120) | 6 | Phase 1 (with `--render-js`) | Raw HTML vs Rendered DOM discrepancies |
| **Total** | **120** | | Comprehensive Technical Coverage |

---

## Detailed Rules Specification

---

### Category 1: HTTP Status & Network Transport (Checks 1–10)

#### 1. `ERR_HTTP_4XX_CLIENT_ERROR`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: HTTP status code between 400 and 499 (e.g. 404 Not Found, 403 Forbidden, 410 Gone).
- **Impact**: Page is inaccessible to users and crawlers, leaking link equity.
- **Fix**: Restore the missing page or return a 301 redirect to the closest relevant alternative URL.

#### 2. `ERR_HTTP_5XX_SERVER_ERROR`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: HTTP status code between 500 and 599 (e.g. 500 Internal Error, 502 Bad Gateway, 503 Service Unavailable).
- **Impact**: Server crashed or gateway failed; search engines drop pages from index if persistent.
- **Fix**: Investigate server application logs, database connection pools, or upstream proxy timeouts.

#### 3. `INFO_HTTP_301_PERMANENT_REDIRECT`

- **Severity**: Notice | **Phase**: 1 (In-Flight)
- **Detection**: HTTP status code 301 Moved Permanently.
- **Impact**: Expected redirect, but internal links pointing to it should be updated to bypass the extra hop.
- **Fix**: Update internal links on source pages to point directly to the destination URL.

#### 4. `INFO_HTTP_302_TEMPORARY_REDIRECT`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: HTTP status code 302 Found / Temporary Redirect.
- **Impact**: Search engines may not transfer link equity (PageRank) across temporary redirects.
- **Fix**: Change to 301 Moved Permanently if the move is intended to be permanent.

#### 5. `INFO_HTTP_307_308_REDIRECT`

- **Severity**: Notice | **Phase**: 1 (In-Flight)
- **Detection**: HTTP status code 307 (Temporary) or 308 (Permanent).
- **Impact**: Method-preserving redirect; update links if permanent.
- **Fix**: Update internal links to direct destination if 308.

#### 6. `ERR_NETWORK_TIMEOUT`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: Connection or read timeout exceeded configured threshold (default: 10s).
- **Impact**: Search crawlers abandon request, wasting crawl budget.
- **Fix**: Optimize backend response generation or increase server resources.

#### 7. `ERR_NETWORK_DNS_FAILURE`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: Domain name failed to resolve (NXDOMAIN or DNS lookup error).
- **Impact**: Host completely unreachable; site is effectively down.
- **Fix**: Check domain registrar status, DNS records (A/AAAA/CNAME), and nameserver health.

#### 8. `ERR_NETWORK_CONNECTION_REFUSED`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: Host reached but TCP connection was refused on specified port (80/443).
- **Impact**: Web server daemon (Nginx/Apache/Caddy) is stopped or firewall is blocking traffic.
- **Fix**: Restart web server and verify firewall/security group port permissions.

#### 9. `ERR_NETWORK_SSL_TLS_ERROR`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: TLS handshake failure, certificate expired, untrusted CA, or cipher mismatch.
- **Impact**: Browsers display security warnings; search engines degrade ranking and traffic plummets.
- **Fix**: Renew SSL/TLS certificate via Let's Encrypt or your certificate authority.

#### 10. `ALERT_WAF_BOT_CHALLENGE`

- **Severity**: Alert | **Phase**: 1 (In-Flight)
- **Detection**: HTTP 200/403 response whose HTML matches known Cloudflare, Akamai, DataDome, or Imperva challenge signatures.
- **Impact**: Crawler is blocked by anti-bot firewall; audit results for this URL do not reflect real user page content.
- **Fix**: Whitelist the crawler's IP, adjust WAF rate limit rules, or supply a valid session cookie.

---

### Category 2: Titles & Basic Metadata (Checks 11–20)

#### 11. `ERR_TITLE_MISSING`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: Document has no `<title>` element inside `<head>`.
- **Impact**: Search engines synthesize arbitrary titles in SERPs, causing CTR collapse.
- **Fix**: Add a unique, descriptive `<title>` tag inside `<head>`.

#### 12. `ERR_TITLE_MULTIPLE`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: Document contains more than one `<title>` element.
- **Impact**: Search engines unpredictably pick one or ignore both.
- **Fix**: Remove duplicate title tags so only one canonical `<title>` tag exists.

#### 13. `WARN_TITLE_TOO_SHORT`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Title tag character count $< 30$ characters.
- **Impact**: Under-optimized; misses opportunity to target primary and secondary keywords.
- **Fix**: Expand title to 45–60 characters with descriptive keyword context and brand name.

#### 14. `WARN_TITLE_TOO_LONG`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Title tag character count $> 60$ characters.
- **Impact**: Text is truncated with an ellipsis (`...`) in Google SERPs.
- **Fix**: Condense title to under 60 characters, front-loading critical keywords.

#### 15. `WARN_TITLE_SAME_AS_H1`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: `<title>` text exactly matches the `<h1>` text verbatim.
- **Impact**: Missed opportunity to target complementary long-tail keywords.
- **Fix**: Differentiate title (for search snippets) and H1 (for on-page visitor context).

#### 16. `ERR_META_DESC_MISSING`

- **Severity**: Alert | **Phase**: 1 (In-Flight)
- **Detection**: Document lacks a `<meta name="description">` tag.
- **Impact**: Search engines extract random body text for snippets, reducing CTR.
- **Fix**: Write a compelling 120–160 character meta description summarizing page value.

#### 17. `ERR_META_DESC_MULTIPLE`

- **Severity**: Alert | **Phase**: 1 (In-Flight)
- **Detection**: More than one `<meta name="description">` tag in `<head>`.
- **Impact**: Ambiguous metadata confuses search crawlers.
- **Fix**: Consolidate into a single `<meta name="description">` tag.

#### 18. `WARN_META_DESC_TOO_SHORT`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Meta description character count $< 120$ characters.
- **Impact**: Snippet does not fully utilize available SERP pixel space.
- **Fix**: Expand description to 120–160 characters with clear value proposition.

#### 19. `WARN_META_DESC_TOO_LONG`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Meta description character count $> 160$ characters.
- **Impact**: Description is cut off mid-sentence on search results pages.
- **Fix**: Keep meta description concise (between 120 and 160 characters).

#### 20. `WARN_META_KEYWORDS_PRESENT`

- **Severity**: Notice | **Phase**: 1 (In-Flight)
- **Detection**: `<meta name="keywords">` tag present in document.
- **Impact**: Ignored by major search engines (Google, Bing) since 2009; can reveal keyword strategy to competitors.
- **Fix**: Safely delete the `<meta name="keywords">` tag.

---

### Category 3: Headings & Document Structure (Checks 21–30)

#### 21. `ERR_H1_MISSING`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: No `<h1>` heading tag present on an HTML page.
- **Impact**: Weakens topic relevance and accessibility structure.
- **Fix**: Add a single primary `<h1>` tag summarizing the main topic of the page.

#### 22. `WARN_H1_MULTIPLE`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: More than one `<h1>` tag found on the page.
- **Impact**: Can dilute the primary topic signal for search engines.
- **Fix**: Demote secondary `<h1>` tags to `<h2>` or `<h3>`.

#### 23. `ERR_H1_EMPTY`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: `<h1>` tag exists but contains only whitespace or an image without alt text.
- **Impact**: Crawlers register an empty heading; screen readers cannot announce it.
- **Fix**: Ensure text content is placed directly inside the `<h1>`.

#### 24. `WARN_H1_TOO_LONG`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: `<h1>` text exceeds 70 characters.
- **Impact**: Can read like a paragraph rather than a concise headline.
- **Fix**: Shorten the H1 to a clean, focused statement.

#### 25. `WARN_HEADING_ORDER_INVALID`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Heading levels skip an intermediate rank (e.g. `<h1>` followed by `<h3>` without an intervening `<h2>`).
- **Impact**: Violates WCAG accessibility structure and breaks semantic hierarchy.
- **Fix**: Adjust heading levels sequentially (`H1 -> H2 -> H3`).

#### 26. `WARN_HEADING_EMPTY`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: `<h2>`, `<h3>`, `<h4>`, `<h5>`, or `<h6>` tag with no text content.
- **Impact**: Clutters DOM structure with meaningless nodes.
- **Fix**: Remove empty heading tags.

#### 27. `WARN_DUPLICATE_HEADING_TEXT`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Multiple headings of the same level share exact identical text on the same page.
- **Impact**: Confuses screen reader navigation and search outline parsing.
- **Fix**: Ensure subheadings are distinct and uniquely describe their respective sections.

#### 28. `WARN_EXCESSIVE_DOM_DEPTH`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Total DOM node count $> 1,500$ or tree nesting depth $> 32$ levels.
- **Impact**: Slows browser rendering engine, degrades INP (Interaction to Next Paint) and Core Web Vitals.
- **Fix**: Flatten deeply nested wrapper `<div>` tags and virtualize large lists.

#### 29. `ERR_META_IN_BODY`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: `<meta>`, `<link rel="canonical">`, or `<title>` tags found inside `<body>` instead of `<head>`.
- **Impact**: Browsers and search bots often discard metadata outside `<head>`.
- **Fix**: Move all metadata and canonical links into `<head>`.

#### 30. `WARN_DUPLICATE_ID_ATTRIBUTES`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Two or more HTML elements share the exact same `id` attribute.
- **Impact**: Invalid HTML; breaks anchor linking (`#id`) and JavaScript document querying.
- **Fix**: Ensure all element `id` values are unique across the page.

---

### Category 4: Indexability & Robots Directives (Checks 31–40)

#### 31. `ERR_INDEXABILITY_NOINDEX`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: `<meta name="robots" content="noindex">` or HTTP header `X-Robots-Tag: noindex`.
- **Impact**: Page is completely excluded from search engine index.
- **Fix**: Remove `noindex` directive if the page is intended to rank organically.

#### 32. `WARN_INDEXABILITY_NOFOLLOW`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: `<meta name="robots" content="nofollow">` present.
- **Impact**: Search engines will not follow or pass link equity through any link on this page.
- **Fix**: Remove page-level `nofollow` unless intentionally isolating untrusted user-generated content.

#### 33. `WARN_INDEXABILITY_NOIMAGEINDEX`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: `noimageindex` directive present in robots meta.
- **Impact**: Prevents images on this page from appearing in Google Images search.
- **Fix**: Remove directive unless images are copyrighted or proprietary.

#### 34. `WARN_INDEXABILITY_NOSNIPPET`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: `nosnippet` directive present in robots meta.
- **Impact**: Disables search snippets and video previews in SERPs.
- **Fix**: Remove directive unless legally required.

#### 35. `ERR_ROBOTS_TXT_DISALLOWED`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: URL matches a `Disallow` rule in `/robots.txt` for `Googlebot` or wildcard `*`.
- **Impact**: Search engines are forbidden from fetching the page.
- **Fix**: Update `/robots.txt` to allow access if page should be crawled.

#### 36. `ERR_ROBOTS_TXT_MISSING`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: `/robots.txt` returns HTTP 4xx or 5xx.
- **Impact**: Crawlers may assume entire site is allowed (on 404) or halt crawling completely (on 500).
- **Fix**: Deploy a valid, readable `/robots.txt` file at the root.

#### 37. `WARN_X_ROBOTS_TAG_CONFLICT`

- **Severity**: Alert | **Phase**: 1 (In-Flight)
- **Detection**: HTTP header `X-Robots-Tag` specifies `noindex` while HTML `<meta name="robots">` specifies `index` (or vice-versa).
- **Impact**: Conflicting instructions cause unpredictable indexing behavior.
- **Fix**: Synchronize HTTP headers and HTML meta tags.

#### 38. `ALERT_PAGINATION_NOINDEX`

- **Severity**: Alert | **Phase**: 1 (In-Flight)
- **Detection**: Paginated URLs (e.g. `?p=2`, `/page/2/`) marked with `noindex`.
- **Impact**: Search engines drop deep paginated pages, breaking crawler pathways to catalog items.
- **Fix**: Allow indexing on paginated pages, canonicalizing each paginated page to itself.

#### 39. `WARN_PAGINATION_BROKEN_LINKS`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: `<link rel="next">` or `<link rel="prev">` links point to 4xx or non-existent URLs.
- **Impact**: Breaks pagination chain discovery.
- **Fix**: Verify and repair next/prev pagination URLs.

#### 40. `ALERT_UNRENDERED_SPA_HEURISTIC`

- **Severity**: Alert | **Phase**: 1 (In-Flight)
- **Detection**: Raw HTML has `<div id="root"></div>` or `<noscript>` with $< 50$ body words and 0 internal links.
- **Impact**: Client-rendered React/Vue app without SSR; raw HTTP crawlers see zero content.
- **Fix**: Implement Server-Side Rendering (SSR) or run audit with `--render-js`.

---

### Category 5: Canonicalization (Checks 41–50)

#### 41. `WARN_CANONICAL_MISSING`

- **Severity**: Alert | **Phase**: 1 (In-Flight)
- **Detection**: Page has no `<link rel="canonical">` tag.
- **Impact**: Leaves duplicate URL selection entirely to Google's heuristic algorithm.
- **Fix**: Add an explicit self-referential canonical tag.

#### 42. `ERR_CANONICAL_MULTIPLE`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: Multiple distinct canonical tags found on the page.
- **Impact**: Google ignores all canonical tags when duplicates exist.
- **Fix**: Ensure exactly one canonical tag is emitted in `<head>`.

#### 43. `WARN_CANONICAL_MISMATCH`

- **Severity**: Alert | **Phase**: 1 (In-Flight)
- **Detection**: Canonical URL in HTML differs from HTTP `Link: rel="canonical"` header.
- **Impact**: Search engines discard conflicting canonical directives.
- **Fix**: Align HTML and HTTP header canonical declarations.

#### 44. `WARN_CANONICAL_RELATIVE`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Canonical tag contains a relative path (`href="/page"`) instead of absolute URL (`https://...`).
- **Impact**: Can lead to incorrect URL resolution across subdomains and protocols.
- **Fix**: Always specify the fully qualified canonical URL including `https://` scheme.

#### 45. `INFO_CANONICAL_SELF_REFERENTIAL`

- **Severity**: Notice | **Phase**: 1 (In-Flight)
- **Detection**: Canonical tag matches the current URL.
- **Impact**: Healthy confirmation that page declares itself authoritative.
- **Fix**: None required (best practice).

#### 46. `ALERT_CANONICAL_CROSS_DOMAIN`

- **Severity**: Alert | **Phase**: 1 (In-Flight)
- **Detection**: Canonical URL points to a different root domain.
- **Impact**: Transfers all indexing authority and link equity to an external website.
- **Fix**: Verify cross-domain syndication intent; change to self-referential if unintended.

#### 47. `ALERT_CANONICAL_TO_REDIRECT`

- **Severity**: Alert | **Phase**: 1 (In-Flight)
- **Detection**: Canonical tag points to a URL that returns a 3xx redirect.
- **Impact**: Forces search engine to follow multiple hops to determine true canonical.
- **Fix**: Point canonical tag directly to the final destination URL.

#### 48. `ERR_CANONICAL_TO_4XX_5XX`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: Canonical tag points to a URL returning a 4xx or 5xx status.
- **Impact**: Tells search engine that the canonical source is dead, risking page de-indexing.
- **Fix**: Update canonical to point to an active, valid 200 OK page.

#### 49. `ERR_CANONICAL_TO_NOINDEX`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: Canonical points to a page that carries a `noindex` directive.
- **Impact**: Contradictory signals; page risks being dropped from the index.
- **Fix**: Either remove `noindex` on target, or change canonical to self-referential.

#### 50. `ALERT_CANONICAL_CHAIN`

- **Severity**: Alert | **Phase**: 2 (Multi-Page Graph)
- **Detection**: Page A canonicalizes to Page B, which in turn canonicalizes to Page C.
- **Impact**: Search engine may reject the entire canonical chain and pick arbitrary URLs.
- **Fix**: Update Page A's canonical tag to point directly to Page C.

---

### Category 6: Links & Anchor Quality (Checks 51–60)

#### 51. `ERR_INTERNAL_LINK_BROKEN`

- **Severity**: Critical | **Phase**: 1 & 2
- **Detection**: Internal link (`<a href>`) points to a URL returning 4xx, 5xx, or DNS error.
- **Impact**: Degrades user experience, leaks PageRank into dead ends.
- **Fix**: Update or remove the broken link; replace with valid destination.

#### 52. `WARN_INTERNAL_LINK_REDIRECT`

- **Severity**: Warning | **Phase**: 1 & 2
- **Detection**: Internal link points to a 3xx redirecting URL.
- **Impact**: Introduces unnecessary latency hop and consumes crawl budget.
- **Fix**: Update link href directly to the final redirect destination.

#### 53. `WARN_INTERNAL_LINK_NOFOLLOW`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Internal link contains `rel="nofollow"`.
- **Impact**: Wastes PageRank flow within your own domain architecture.
- **Fix**: Remove `nofollow` from internal navigation links.

#### 54. `WARN_EXTERNAL_LINK_BROKEN`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Outbound external link returns 4xx, 5xx, or connection timeout.
- **Impact**: Negative user experience and quality signal.
- **Fix**: Update or unlink the broken external reference.

#### 55. `WARN_EXTERNAL_LINK_REDIRECT`

- **Severity**: Notice | **Phase**: 1 (In-Flight)
- **Detection**: Outbound external link returns a 3xx redirect.
- **Impact**: Slight latency delay for visitors.
- **Fix**: Update external href to final destination URL.

#### 56. `WARN_LINK_NON_DESCRIPTIVE_ANCHOR`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Anchor text matches generic terms: "click here", "read more", "more", "link", "here".
- **Impact**: Fails to provide search engines with contextual anchor keyword signals.
- **Fix**: Rewrite anchor text to describe the target destination specifically.

#### 57. `WARN_LINK_EMPTY_ANCHOR`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: `<a>` tag has no text, no `aria-label`, and child `<img>` lacks `alt` text.
- **Impact**: Zero semantic meaning passed to search engines or screen readers.
- **Fix**: Add descriptive link text or `aria-label`.

#### 58. `WARN_EXCESSIVE_OUTBOUND_LINKS`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Total link count on page exceeds 250 links.
- **Impact**: Dilutes internal link equity and can trigger spam heuristics.
- **Fix**: Trim unnecessary footer or sidebar links.

#### 59. `ERR_BROKEN_BOOKMARK_ANCHOR`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: In-page link (`<a href="#section">`) has no matching element with `id="section"` on the page.
- **Impact**: Broken in-page navigation causes poor UX.
- **Fix**: Match target element `id` attribute to href hash.

#### 60. `WARN_LINK_TO_LOCALHOST`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: Link points to `localhost`, `127.0.0.1`, or development staging hostnames.
- **Impact**: Broken links for public visitors; exposes internal infrastructure.
- **Fix**: Replace staging/localhost URLs with production paths.

---

### Category 7: Security & Modern Transport (Checks 61–70)

#### 61. `ERR_SECURITY_HTTP_INSECURE`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: Page URL served over unencrypted plain `http://`.
- **Impact**: Security vulnerability, "Not Secure" browser warning, ranking penalty.
- **Fix**: Enforce HTTPS redirection site-wide.

#### 62. `ERR_SECURITY_MIXED_CONTENT`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: HTTPS page requests subresources (images, scripts, styles) over insecure `http://`.
- **Impact**: Browsers block mixed content scripts; causes broken layouts and security warnings.
- **Fix**: Update all asset references to use `https://` or relative URLs.

#### 63. `ERR_SECURITY_INSECURE_FORM`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: `<form>` element submits data to an insecure `http://` action URL.
- **Impact**: User credentials/data transmitted in cleartext; browsers flag form as dangerous.
- **Fix**: Change form action to an `https://` endpoint.

#### 64. `WARN_SECURITY_MISSING_HSTS`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Missing `Strict-Transport-Security` HTTP header on HTTPS responses.
- **Impact**: Vulnerable to SSL-stripping man-in-the-middle attacks.
- **Fix**: Add `Strict-Transport-Security: max-age=31536000; includeSubDomains`.

#### 65. `WARN_SECURITY_MISSING_CSP`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Missing `Content-Security-Policy` header.
- **Impact**: Vulnerable to Cross-Site Scripting (XSS) and data injection.
- **Fix**: Configure and deploy a restrictive CSP header.

#### 66. `WARN_SECURITY_MISSING_X_CONTENT_TYPE`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Missing `X-Content-Type-Options: nosniff` header.
- **Impact**: Allows MIME-type sniffing, risking execution of malicious uploads.
- **Fix**: Add header `X-Content-Type-Options: nosniff`.

#### 67. `WARN_SECURITY_MISSING_X_FRAME_OPTIONS`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Missing `X-Frame-Options` or CSP `frame-ancestors` directive.
- **Impact**: Vulnerable to clickjacking attacks via malicious iframes.
- **Fix**: Add `X-Frame-Options: SAMEORIGIN`.

#### 68. `WARN_SECURITY_MISSING_REFERRER_POLICY`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Missing `Referrer-Policy` header.
- **Impact**: May inadvertently leak sensitive URL parameters to third-party referrers.
- **Fix**: Add `Referrer-Policy: strict-origin-when-cross-origin`.

#### 69. `WARN_SECURITY_TARGET_BLANK_NO_OPENER`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Link has `target="_blank"` but lacks `rel="noopener"` or `rel="noreferrer"`.
- **Impact**: Vulnerable to `window.opener` reverse tabnabbing exploit on older browsers.
- **Fix**: Add `rel="noopener noreferrer"` to external blank-target links.

#### 70. `WARN_SECURITY_SERVER_VERSION_LEAK`

- **Severity**: Notice | **Phase**: 1 (In-Flight)
- **Detection**: `Server` or `X-Powered-By` header exposes exact software versions (e.g. `PHP/7.4.3`, `Apache/2.4.41`).
- **Impact**: Helps automated exploit scanners identify unpatched vulnerabilities.
- **Fix**: Suppress or sanitize server banner headers.

---

### Category 8: Mobile, Performance & UX Signals (Checks 71–80)

#### 71. `ERR_MOBILE_VIEWPORT_MISSING`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: Missing `<meta name="viewport">` tag in `<head>`.
- **Impact**: Page fails Google Mobile-Friendly test; desktop layout shrunk on mobile devices.
- **Fix**: Add `<meta name="viewport" content="width=device-width, initial-scale=1.0">`.

#### 72. `WARN_MOBILE_VIEWPORT_USER_SCALABLE_NO`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Viewport tag contains `user-scalable=no` or `maximum-scale=1.0`.
- **Impact**: Violates accessibility guidelines by blocking pinch-to-zoom for visually impaired users.
- **Fix**: Allow user zooming by removing `user-scalable=no`.

#### 73. `WARN_PERF_SLOW_TTFB`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Time to First Byte (TTFB) $> 1,000\text{ms}$.
- **Impact**: Sluggish server response degrades Core Web Vitals (LCP).
- **Fix**: Implement server-side caching, edge CDN caching, or optimize database queries.

#### 74. `ERR_PERF_VERY_SLOW_TTFB`

- **Severity**: Alert | **Phase**: 1 (In-Flight)
- **Detection**: Time to First Byte (TTFB) $> 3,000\text{ms}$.
- **Impact**: Extreme crawl delay; search bots throttle crawling speed.
- **Fix**: Investigate backend bottleneck, unoptimized database queries, or server load.

#### 75. `WARN_PERF_LARGE_HTML_SIZE`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Uncompressed HTML document size $> 1.5\text{MB}$.
- **Impact**: High memory consumption and slower DOM parsing.
- **Fix**: Move inline base64 images and large embedded JSON state payloads to external files.

#### 76. `ERR_PERF_EXCESSIVE_HTML_SIZE`

- **Severity**: Alert | **Phase**: 1 (In-Flight)
- **Detection**: HTML document size $> 3.0\text{MB}$.
- **Impact**: Googlebot may truncate indexing on documents exceeding size budgets.
- **Fix**: Trim DOM bloat and externalize heavy scripts.

#### 77. `WARN_IMG_MISSING_ALT`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: `<img>` element missing `alt` attribute.
- **Impact**: Fails accessibility compliance and loses image search ranking signals.
- **Fix**: Provide descriptive `alt` text explaining image context.

#### 78. `WARN_IMG_ALT_TOO_LONG`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Image `alt` text exceeds 125 characters.
- **Impact**: Screen readers announce excessive text; looks like keyword stuffing.
- **Fix**: Keep alt text succinct and descriptive.

#### 79. `WARN_IMG_MISSING_DIMENSIONS`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: `<img>` tag lacks explicit `width` and `height` attributes.
- **Impact**: Causes Cumulative Layout Shift (CLS), damaging Core Web Vitals score.
- **Fix**: Add explicit `width` and `height` attributes or CSS aspect-ratio.

#### 80. `WARN_IMG_OVERSIZED`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Image file size $> 1.0\text{MB}$.
- **Impact**: Slows page load time, especially on mobile connections.
- **Fix**: Compress images and serve in next-gen formats (WebP or AVIF).

---

### Category 9: Internationalization & Hreflang (Checks 81–88)

#### 81. `ERR_HREFLANG_NOT_RECIPROCAL`

- **Severity**: Critical | **Phase**: 2 (Multi-Page Graph)
- **Detection**: Page A specifies Page B as alternate language, but Page B does NOT link back to Page A.
- **Impact**: Google ignores non-reciprocal hreflang tags to prevent rogue third-party hijacking.
- **Fix**: Ensure return hreflang links exist bidirectionally between all localized versions.

#### 82. `ERR_HREFLANG_TO_NON_CANONICAL`

- **Severity**: Critical | **Phase**: 1 & 2
- **Detection**: Hreflang alternate points to a URL that has a different canonical tag.
- **Impact**: Conflicting instructions cause search engines to ignore the localized alternate.
- **Fix**: Hreflang tags must always point exclusively to canonical URLs.

#### 83. `ERR_HREFLANG_TO_BROKEN_OR_REDIRECT`

- **Severity**: Critical | **Phase**: 1 & 2
- **Detection**: Hreflang URL returns a 3xx redirect, 4xx, or 5xx status.
- **Impact**: Alternate language target cannot be fetched.
- **Fix**: Point hreflang annotations directly to active 200 OK canonical URLs.

#### 84. `WARN_HREFLANG_MISSING_X_DEFAULT`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Hreflang group lacks `hreflang="x-default"` tag.
- **Impact**: No designated fallback for users whose language is not explicitly targeted.
- **Fix**: Add `hreflang="x-default"` pointing to the default global or language-selector page.

#### 85. `ERR_HREFLANG_MISSING_SELF_REFERENCE`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: Page declares hreflang alternates for other languages but omits its own URL.
- **Impact**: Incomplete hreflang cluster; invalidates configuration in Google Search.
- **Fix**: Include a self-referencing hreflang tag matching the page's current URL.

#### 86. `WARN_HREFLANG_INVALID_LANG_CODE`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Hreflang language or region code fails ISO 639-1 / ISO 3166-1 validation (e.g. `en-UK` instead of `en-GB`).
- **Impact**: Search engines fail to parse the targeted region or language.
- **Fix**: Correct language/region codes to match standard ISO formats.

#### 87. `WARN_HREFLANG_RELATIVE_URL`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Hreflang tag uses a relative path instead of an absolute URL.
- **Impact**: Unreliable resolution by search bots.
- **Fix**: Use absolute URLs including protocol for all hreflang attributes.

#### 88. `WARN_HTML_LANG_MISSING`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: `<html>` tag missing `lang` attribute.
- **Impact**: Screen readers cannot determine pronunciation rules; search engines rely on heuristics.
- **Fix**: Add `lang="en"` (or applicable language code) to the `<html>` root tag.

---

### Category 10: Structured Data & Google Rich Results (Checks 89–96)

#### 89. `ERR_SCHEMA_JSON_LD_SYNTAX_ERROR`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: `<script type="application/ld+json">` contains invalid JSON (parsing syntax error).
- **Impact**: Entire structured data block discarded by search engines.
- **Fix**: Validate JSON syntax and escape quotes/special characters properly.

#### 90. `WARN_SCHEMA_MISSING`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Page has zero JSON-LD or Microdata structured markup.
- **Impact**: Misses out on Google Rich Results enhancements (stars, prices, FAQs, breadcrumbs).
- **Fix**: Implement appropriate Schema.org schema (e.g. `Article`, `Product`, `LocalBusiness`).

#### 91. `ERR_SCHEMA_RICH_RESULT_MISSING_REQUIRED`

- **Severity**: Critical | **Phase**: 1 (In-Flight)
- **Detection**: Schema type (Article, Product, Recipe, FAQPage, Event, JobPosting) lacks mandatory Google Rich Result properties.
- **Impact**: Ineligible for Google Rich Results features in SERPs.
- **Fix**: Add required fields (e.g. `image` and `headline` for Article; `name` and `offers` for Product).

#### 92. `WARN_SCHEMA_RICH_RESULT_MISSING_RECOMMENDED`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Schema has all required properties but lacks recommended enhancements (e.g. `ratingValue`, `reviewCount`).
- **Impact**: Lower priority for rich presentation in search results.
- **Fix**: Populate recommended schema properties where applicable.

#### 93. `WARN_SCHEMA_MULTIPLE_INCOMPATIBLE_TYPES`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Page contains multiple unconnected top-level schema blocks with conflicting primary entity types.
- **Impact**: Ambiguity regarding primary subject of page.
- **Fix**: Link schemas hierarchically using `@graph` or `mainEntity`.

#### 94. `WARN_SCHEMA_MICRODATA_SYNTAX_ERROR`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: `itemprop` attributes without parent `itemscope`, or malformed `itemtype` URLs.
- **Impact**: Microdata parser fails to connect properties to entities.
- **Fix**: Clean up HTML microdata or migrate to cleaner JSON-LD.

#### 95. `WARN_OPENGRAPH_MISSING`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Missing basic OpenGraph tags (`og:title`, `og:description`, `og:image`, `og:url`).
- **Impact**: Unattractive, broken previews when links are shared on Slack, WhatsApp, Facebook, LinkedIn.
- **Fix**: Add standard OpenGraph meta tags in `<head>`.

#### 96. `WARN_TWITTER_CARD_MISSING`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Missing `twitter:card` or `twitter:title` tags.
- **Impact**: Degraded snippet appearance on Twitter/X.
- **Fix**: Add `<meta name="twitter:card" content="summary_large_image">`.

---

### Category 11: GEO (Generative Engine Optimization) & AI Search (Checks 97–104)

#### 97. `WARN_GEO_LLMS_TXT_MISSING`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: `/llms.txt` returns 404 or does not exist.
- **Impact**: AI assistants and LLMs lack structured site navigation file for context retrieval.
- **Fix**: Publish `/llms.txt` and `/llms-full.txt` summarizing site structure and key documentation.

#### 98. `ALERT_GEO_AI_RETRIEVAL_BOT_BLOCKED`

- **Severity**: Alert | **Phase**: 1 (In-Flight)
- **Detection**: `/robots.txt` blocks AI search/retrieval user-agents (`OAI-SearchBot`, `ChatGPT-User`, `PerplexityBot`, `Claude-User`).
- **Impact**: Site cannot be cited in ChatGPT Search, Perplexity, or AI Overviews answers.
- **Fix**: Remove Disallow rules for retrieval/search citation bots in `/robots.txt`.

#### 99. `INFO_GEO_AI_TRAINING_BOT_BLOCKED`

- **Severity**: Notice | **Phase**: 1 (In-Flight)
- **Detection**: `/robots.txt` blocks training crawlers (`GPTBot`, `ClaudeBot`, `Google-Extended`).
- **Impact**: Site content excluded from model training datasets (defensible policy choice).
- **Fix**: Informative only; no action needed unless client wishes to permit training ingestion.

#### 100. `WARN_CONTENT_THIN_WORD_COUNT`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Clean editorial body text has $< 300$ words.
- **Impact**: May trigger Google "Thin Content" algorithmic quality suppression.
- **Fix**: Enrich page with valuable in-depth content or consolidate with related pages.

#### 101. `WARN_CONTENT_SOFT_404`

- **Severity**: Alert | **Phase**: 1 (In-Flight)
- **Detection**: HTTP 200 OK status with thin body containing phrases like "page does not exist", "not found", "no results".
- **Impact**: Search engines classify as soft-404, wasting crawl budget and muddying analytics.
- **Fix**: Return true HTTP 404 or 410 status code for non-existent pages.

#### 102. `WARN_CONTENT_AI_TELL_OVERUSE`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: High density of repetitive AI boilerplate words ("delve", "unlock", "seamlessly", "tapestry") $\ge 3$, or em-dash density $> 1.0$ per 100 words.
- **Impact**: Low perceived editorial quality; vulnerability to helpful content algorithm penalties.
- **Fix**: Edit content for natural human voice and remove generic filler buzzwords.

#### 103. `WARN_CONTENT_LOREM_IPSUM`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: Latin placeholder text ("Lorem ipsum dolor sit amet") found in page content.
- **Impact**: Demonstrates unfinished template content on production pages.
- **Fix**: Replace placeholder text with finalized copy.

#### 104. `WARN_URL_HYGIENE_ISSUES`

- **Severity**: Warning | **Phase**: 1 (In-Flight)
- **Detection**: URL contains raw spaces (`%20`), double slashes (`//`), uppercase letters, or underscores (`_`).
- **Impact**: Risk of duplicate URLs, canonical confusion, and tracking errors.
- **Fix**: Use lowercase, hyphen-separated (`-`) clean URL structures.

---

### Category 12: Site-Wide Graph & Architecture (Post-Crawl) (Checks 105–114)

#### 105. `ALERT_GRAPH_ORPHAN_PAGE`

- **Severity**: Alert | **Phase**: 2 (Multi-Page Graph)
- **Detection**: URL present in XML sitemap but receives 0 internal incoming links from crawled pages.
- **Impact**: Buried page receives zero internal link equity and risks indexing drop.
- **Fix**: Add internal contextual links from relevant parent or category pages.

#### 106. `ERR_GRAPH_REDIRECT_LOOP`

- **Severity**: Critical | **Phase**: 2 (Multi-Page Graph)
- **Detection**: Circular redirect detected (e.g. `A -> B -> A`).
- **Impact**: Browsers display "Too many redirects"; search crawlers fail completely.
- **Fix**: Break the loop by redirecting directly to the intended final URL.

#### 107. `WARN_GRAPH_REDIRECT_CHAIN`

- **Severity**: Warning | **Phase**: 2 (Multi-Page Graph)
- **Detection**: Redirect sequence with $> 1$ hop (e.g. `A -> B -> C`).
- **Impact**: Adds latency per hop; search engines may abandon following chains $> 4$ hops.
- **Fix**: Update the initial redirect rule to point directly from A to C.

#### 108. `ERR_GRAPH_CANONICAL_LOOP`

- **Severity**: Critical | **Phase**: 2 (Multi-Page Graph)
- **Detection**: Circular canonical tags (Page A canonicalizes to B, Page B canonicalizes to A).
- **Impact**: Invalidates canonicalization on both pages; search engine picks arbitrary winner.
- **Fix**: Designate a single authoritative URL and point both canonicals to it.

#### 109. `WARN_GRAPH_EXACT_DUPLICATE_CONTENT`

- **Severity**: Alert | **Phase**: 2 (Multi-Page Graph)
- **Detection**: Multiple distinct URLs share identical SHA256 body content hashes.
- **Impact**: Keyword cannibalization, wasted crawl budget, diluted ranking signals.
- **Fix**: Consolidate with 301 redirects or designate a primary URL with canonical tags.

#### 110. `WARN_GRAPH_NEAR_DUPLICATE_CONTENT`

- **Severity**: Warning | **Phase**: 2 (Multi-Page Graph)
- **Detection**: Multiple URLs share $> 85\%$ SimHash content similarity.
- **Impact**: Templated pages with minimal unique value compete against each other.
- **Fix**: Inject unique content or consolidate into a single comprehensive guide.

#### 111. `WARN_GRAPH_DUPLICATE_TITLES`

- **Severity**: Warning | **Phase**: 2 (Multi-Page Graph)
- **Detection**: Distinct URLs share the exact same `<title>` text.
- **Impact**: Search engines cannot distinguish topic differences between pages in search results.
- **Fix**: Ensure every indexable page has a distinct, customized title tag.

#### 112. `WARN_GRAPH_DUPLICATE_META_DESCS`

- **Severity**: Warning | **Phase**: 2 (Multi-Page Graph)
- **Detection**: Distinct URLs share identical meta description strings.
- **Impact**: Repetitive search snippets reduce overall search click-through rates.
- **Fix**: Author unique descriptions for distinct landing pages.

#### 113. `WARN_GRAPH_DEAD_END_PAGE`

- **Severity**: Warning | **Phase**: 2 (Multi-Page Graph)
- **Detection**: Page has internal inlinks but 0 outgoing links to any other page on the site.
- **Impact**: Link equity dead end; users cannot continue navigating naturally.
- **Fix**: Add related links, navigation menus, or breadcrumbs to keep equity flowing.

#### 114. `WARN_GRAPH_HIGH_CRAWL_DEPTH`

- **Severity**: Warning | **Phase**: 2 (Multi-Page Graph)
- **Detection**: Page requires $> 4$ clicks to reach from the homepage.
- **Impact**: Buried content crawled infrequently by search engines.
- **Fix**: Flatten site architecture; link important pages from top-level menus or hubs.

---

### Category 13: JavaScript SEO Diffing Checks (When `--render-js` is enabled) (Checks 115–120)

#### 115. `ERR_JS_DIFF_CANONICAL_ALTERED`

- **Severity**: Critical | **Phase**: 1 (JS Diff)
- **Detection**: Canonical URL tag in client-rendered DOM differs from the raw server HTML canonical.
- **Impact**: Google may distrust and reject client-injected canonicals, causing index confusion.
- **Fix**: Ensure canonical tags are rendered identically on the server before client hydration.

#### 116. `ERR_JS_DIFF_NOINDEX_INJECTED`

- **Severity**: Critical | **Phase**: 1 (JS Diff)
- **Detection**: Raw HTML has no `noindex`, but client-side JavaScript injects `<meta name="robots" content="noindex">`.
- **Impact**: Page is de-indexed during Google's render queue pass (Wave 2).
- **Fix**: Remove client-side script dynamically adding `noindex` directives.

#### 117. `WARN_JS_DIFF_TITLE_META_DESYNC`

- **Severity**: Alert | **Phase**: 1 (JS Diff)
- **Detection**: Single Page App navigation changes the URL but client router fails to update `<title>` or meta description in the DOM.
- **Impact**: Search crawlers rendering the page see generic homepage metadata.
- **Fix**: Use Head management libraries (e.g. React Helmet, Nuxt Head) correctly on route transitions.

#### 118. `ALERT_JS_DIFF_VANISHING_CONTENT`

- **Severity**: Alert | **Phase**: 1 (JS Diff)
- **Detection**: Substantial editorial text or internal links present in raw HTML disappear after JavaScript executes.
- **Impact**: Content visible to raw HTTP bots is destroyed or replaced during client hydration.
- **Fix**: Resolve React/Vue hydration mismatch errors.

#### 119. `ALERT_JS_DIFF_LATE_RENDERED_LINKS`

- **Severity**: Warning | **Phase**: 1 (JS Diff)
- **Detection**: Internal navigation links only appear after JavaScript execution; 0 links present in raw HTML.
- **Impact**: Delays crawler discovery until Google's headless rendering queue executes (days/weeks delay).
- **Fix**: Render navigation links in the initial server HTML response.

#### 120. `ERR_JS_DIFF_HYDRATION_CRASH`

- **Severity**: Critical | **Phase**: 1 (JS Diff)
- **Detection**: Client-side JavaScript throws an uncaught runtime error, leaving a blank white screen or error boundary placeholder.
- **Impact**: Search bot rendering produces an empty page; rankings drop.
- **Fix**: Debug client JavaScript console errors and fix hydration exceptions.

---

## Conclusion

This catalog serves as the static blueprint for `src/rules/`. Every rule will have a corresponding enum variant in Rust, automated unit test fixtures, and consistent formatting in JSON, Markdown, and CSV exports.
