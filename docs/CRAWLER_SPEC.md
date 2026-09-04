# SEO Lens: Crawler Engine & Politeness Specification

**Scope**: Network Concurrency, AIMD Adaptive Politeness, URL Normalization Pipeline, Frontier Scheduling, and RFC 9309 Robots/Sitemap Ingestion

---

## 1. Concurrency Architecture & Task Flow

`SEO Lens` uses an asynchronous producer-consumer pipeline built on the Tokio runtime:

```mermaid
flowchart TD
    subgraph Discovery ["URL Discovery & Frontier"]
        Queue["URL Frontier (FIFO mpsc / VecDeque)"]
        VisitedSet["Visited Set (SwissTable 64-bit Hashes)"]
        SitemapEngine["Sitemap Ingestion Engine (quick-xml)"]
        RobotsEngine["Robots.txt Engine (RFC 9309)"]
    end

    SitemapEngine -->|Seed URLs| Queue

    subgraph ConcurrencyPipeline ["Concurrency & Politeness"]
        Queue -->|Pop (URL, Depth)| Worker["Worker Task (tokio::spawn)"]
        Worker -->|Check Permission| RobotsEngine
        Worker --> RateGate["AIMD Adaptive Rate Controller (Semaphore + Token Bucket)"]
        RateGate --> HTTPClient["reqwest HTTP/2 Client Pool"]
    end

    subgraph ResponseProcessing ["Stream Ingestion & Feedback"]
        HTTPClient --> Telemetry["Latency & Error Rate Feedback Loop"]
        Telemetry -->|Tune Delay & Concurrency| RateGate
        HTTPClient --> Parser["Streaming lol_html Tokenizer"]
        Parser --> LinkFilter["Domain Boundary & Link Filter"]
        LinkFilter -->|Unseen URLs| VisitedSet
        VisitedSet -->|New URLs| Queue
    end
```

---

## 2. The AIMD Adaptive Politeness Engine

Crawling client websites without rate limiting risks causing CPU spikes, database connection exhaustion, or triggering Cloudflare/WAF IP blocks.

`SEO Lens` adapts the **Additive-Increase/Multiplicative-Decrease (AIMD)** congestion control algorithm (inspired by TCP Reno and `librecrawl-mcp`):

### 2.1 Mathematical Model & Parameters

```
                              ┌────────────────────────┐
                              │ Sample Window (N = 50) │
                              └───────────┬────────────┘
                                          │
                    Compute: Error Rate (E) & p95 TTFB
                                          │
            ┌─────────────────────────────┴─────────────────────────────┐
            ▼                                                           ▼
   Degradation Trigger                                           Healthy Recovery
   (E > 8% OR p95 > 1.5x baseline)                               (E == 0% AND p95 < 500ms)
            │                                                           │
            ▼                                                           ▼
 Multiplicative Backoff:                                       Additive Recovery:
 delay = min(delay * 2.0, 10,000ms)                            delay = max(delay - 25ms, delay_floor)
 concurrency = max(floor(c * 0.5), 1)                          concurrency = min(c + 1, c_max)
```

| Parameter                 | Symbol                        | Value                                    | Rationale                                                                              |
| ------------------------- | ----------------------------- | ---------------------------------------- | -------------------------------------------------------------------------------------- |
| **Sample Window**         | $W$                           | `50 requests`                            | Large enough to smooth statistical outliers, small enough to react in $<3$ seconds.    |
| **Error Rate Ceiling**    | $E_{\text{thresh}}$           | `0.08` (8%)                              | If $>4$ requests out of 50 fail (429, 500, 502, 503, 504), origin is in distress.      |
| **Latency Multiplier**    | $L_{\text{factor}}$           | `1.5`                                    | If p95 TTFB exceeds $1.5\times$ baseline average, backend database is queuing queries. |
| **Multiplicative Factor** | $\beta$                       | `2.0`                                    | Instantly halves origin load when distress is detected.                                |
| **Additive Step**         | $\Delta_{\text{add}}$         | `25 ms`                                  | Gently steps up crawl rate without shocking the origin.                                |
| **Max Delay Ceiling**     | $\text{delay}_{\text{max}}$   | `10,000 ms`                              | Prevents infinite stalls; caps wait at 10 seconds per request.                         |
| **Delay Floor**           | $\text{delay}_{\text{floor}}$ | $\max(\text{robots\_delay}, 0\text{ms})$ | Strictly obeys `Crawl-Delay` specified in `/robots.txt`.                               |

---

## 3. URL Normalization Pipeline

To eliminate crawl duplication (e.g. `https://example.com`, `http://example.com/`, `https://example.com/?utm_source=fb` must all resolve to the same canonical identifier), all discovered links pass through an 8-stage normalization pipeline in `src/core/url.rs`:

```
Raw Href ──> [1. Scheme] ──> [2. Hostname] ──> [3. Port] ──> [4. Path]
         ──> [5. Trailing Slash] ──> [6. Strip Fragments]
         ──> [7. Strip Tracking Query] ──> [8. Sort Query] ──> Normalized URL
```

### Stage-by-Stage Transformation Specification:

1. **Scheme Normalization**:
   - Convert scheme to lowercase (`HTTP` $\rightarrow$ `http`, `HTTPS` $\rightarrow$ `https`).
   - Resolve protocol-relative URLs (`//cdn.example.com/a` $\rightarrow$ `https://cdn.example.com/a`).
2. **Hostname Normalization**:
   - Convert host to lowercase (`Example.COM` $\rightarrow$ `example.com`).
   - Remove trailing root dot if present (`example.com.` $\rightarrow$ `example.com`).
3. **Default Port Stripping**:
   - Strip standard ports (`http://example.com:80/` $\rightarrow$ `http://example.com/`).
   - Strip `:443` for HTTPS (`https://example.com:443/` $\rightarrow$ `https://example.com/`).
4. **Path Segment Resolution**:
   - Resolve relative path dots (`/a/b/../c` $\rightarrow$ `/a/c`).
   - Deduplicate consecutive internal slashes (`/blog//post` $\rightarrow$ `/blog/post`).
5. **Root Path Enforcement**:
   - If path is completely empty, supply `/` (`https://example.com` $\rightarrow$ `https://example.com/`).
6. **Fragment Removal**:
   - Strip hash fragments (`/page#reviews` $\rightarrow$ `/page`).
7. **Tracking Parameter Stripping**:
   - Strip marketing, analytics, and session query parameters:
     - `utm_source`, `utm_medium`, `utm_campaign`, `utm_term`, `utm_content`
     - `fbclid`, `gclid`, `msclkid`, `mc_eid`, `_ga`, `_gl`, `ref`
8. **Deterministic Query Parameter Sorting**:
   - Lexicographically sort remaining query keys (`?b=2&a=1` $\rightarrow$ `?a=1&b=2`).

### Hash-Based Deduplication

Once normalized, the URL string is converted into a **64-bit AHash (`u64`)**.

- The `VisitedSet` in `src/crawler/frontier.rs` stores only `u64` hashes in a SwissTable (`hashbrown::HashSet<u64>`).
- Storing 50,000 URLs consumes only **400 KB of RAM** compared to >10 MB for raw strings.

---

## 4. Frontier Queue & Depth Management

The frontier schedules pending URLs and enforces crawl boundaries:

```rust
pub struct FrontierEntry {
    pub url: String,
    pub depth: u16,
    pub source_url: Option<String>,
}
```

### Scheduling Policies:

1. **Breadth-First Search (BFS) (Default)**:
   - Uses `VecDeque<FrontierEntry>` (FIFO queue).
   - Ensures shallow, high-PageRank category pages are audited before deep product catalog leaves.
2. **Depth Limiting**:
   - When a link is discovered on page at `depth = d`, its candidate entry is assigned `depth = d + 1`.
   - If `d + 1 > max_depth`, the URL is added to the link graph as an edge but is **never enqueued for crawling**.
3. **Domain Boundary Control**:
   - **Internal Domain**: Host matches starting domain or its subdomains (if `--include-subdomains` is on). Crawled recursively.
   - **External Domain**: Outbound link. Validated with a lightweight HTTP `HEAD` or single `GET` to verify HTTP status, but child links are not extracted.

---

## 5. RFC 9309 Robots.txt & Sitemap Compliance

### 5.1 Robots.txt Rules (`src/crawler/robots.rs`)

Complies strictly with **RFC 9309 (Robots Exclusion Protocol)**:

1. **User-Agent Matching**:
   - Specific user-agent matches take precedence over wildcard `*`.
   - Priority order: `SEOLens` $\rightarrow$ `Googlebot` $\rightarrow$ `*`.
2. **Longest Match Precedence**:
   - When multiple rules match a path, the rule with the longest matching character length wins:
     ```
     Allow: /products/
     Disallow: /products/archived/
     # URL: /products/archived/item-1 -> DISALLOWED (20 chars vs 10 chars)
     ```
3. **Allow Overrides Disallow on Equal Length**:
   - If `Allow: /blog` and `Disallow: /blog` have identical length, `Allow` wins per RFC 9309.
4. **Wildcards**: Supports `*` (zero or more characters) and `$` (end of URL).

### 5.2 Streaming Sitemap Ingestion (`src/crawler/sitemap.rs`)

Uses `quick-xml` for zero-allocation streaming XML parsing:

1. **Auto-Discovery**:
   - Checks `Sitemap:` directives declared in `/robots.txt`.
   - Checks common standard paths: `/sitemap.xml`, `/sitemap_index.xml`, `/wp-sitemap.xml`.
2. **Sitemap Index Recursion**:
   - When a `<sitemapindex>` document is parsed, its child `<sitemap><loc>` URLs are recursively fetched and parsed up to 3 levels deep.
3. **Compressed Sitemaps (`.xml.gz`)**:
   - Automatically detects `.gz` extensions and decompresses streams on the fly using `flate2`.
4. **Orphan Identification Hook**:
   - All URLs extracted from sitemaps are tagged with `is_sitemap_url = true`.
   - If post-crawl analysis finds that a sitemap URL has 0 incoming internal links from HTML pages, `ALERT_GRAPH_ORPHAN_PAGE` is raised.

---

## 6. WAF & Anti-Bot Fingerprint Probes (`src/crawler/waf.rs`)

When crawling client sites protected by Cloudflare, Akamai, or DataDome, firewalls often return HTTP 200 OK or 403 with a JavaScript challenge screen. If parsed blindly, the crawler reports "0 words, missing H1, missing title" when the site is actually healthy.

`SEO Lens` probes raw responses for known challenge signatures:

```rust
pub struct WafProbe {
    pub provider: &'static str,
    pub signatures: &'static [&'static str],
}

pub const WAF_SIGNATURES: &[WafProbe] = &[
    WafProbe {
        provider: "Cloudflare",
        signatures: &[
            "cf-browser-verification",
            "checking your browser before accessing",
            "cloudflare ray id",
            "/cdn-cgi/challenge-platform/",
        ],
    },
    WafProbe {
        provider: "Akamai",
        signatures: &[
            "akamai bot manager",
            "_abck=",
            "reference&#32;number:",
            "bm-sz=",
        ],
    },
    WafProbe {
        provider: "DataDome",
        signatures: &[
            "geo.captcha-delivery.com",
            "datadome",
            "dd_cookie_test",
        ],
    },
    WafProbe {
        provider: "Imperva",
        signatures: &[
            "incapsula incident id",
            "_incap_ses",
            "visid_incap",
        ],
    },
];
```

When a signature matches, `SEO Lens`:

1. Flags `ALERT_WAF_BOT_CHALLENGE` on the URL.
2. Does **not** trigger false-positive alerts for missing titles, H1s, or content.
3. Automatically instructs the user/agent in the report: _"Blocked by Cloudflare/Akamai WAF challenge. Whitelist the crawler IP or pass a valid session cookie."_

---

## 7. Summary

This specification guarantees:

- Origin servers are actively protected via empirical AIMD rate-tuning.
- URLs are deduplicated with zero ambiguity through an 8-stage pipeline.
- RFC 9309 robots compliance and XML sitemap recursion operate reliably.
- Anti-bot firewall screens are accurately detected without polluting client audit reports.
