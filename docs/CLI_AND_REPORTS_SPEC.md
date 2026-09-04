# SEO Lens: CLI Interface & Report Exporters Specification

Command-Line Interface (Clap v4), Terminal UI (Indicatif), CI/CD Exit Codes, and Multi-Format Exporters (JSON, Markdown, HTML, Screaming Frog CSVs)

---

## 1. CLI Command Hierarchy & Subcommands

`SEO Lens` uses `clap v4` with the derive macro to expose a modern, intuitive subcommand interface via the `seolens` binary:

```bash
seolens
├── audit <url>       # Run a full or partial website crawl and audit
├── mcp               # Start the native Model Context Protocol server (stdio for Cursor/Claude)
├── report <session>  # Re-export or inspect an existing audit from the SQLite database
└── list              # List all historical audit sessions stored locally
```

_(Note: The visual dashboard is provided as a dedicated native desktop application via Tauri v2, eliminating browser port conflicts and providing a double-clickable experience for non-technical users and CMS creators.)_

---

## 2. Command Details & Flag Specification

### 2.1 `seolens audit <url>`

The primary command for technical SEO auditing.

```bash
seolens audit https://example.com [FLAGS] [OPTIONS]
```

#### Arguments & Options:

| Flag / Option   | Short | Type     | Default            | Description                                                                           |
| --------------- | ----- | -------- | ------------------ | ------------------------------------------------------------------------------------- |
| `<url>`         |       | `String` | _(Required)_       | Root URL to crawl (e.g. `https://client.com`).                                        |
| `--max-pages`   | `-p`  | `u32`    | `500`              | Maximum pages to crawl (`0` = unlimited).                                             |
| `--max-depth`   | `-d`  | `u16`    | `5`                | Maximum crawl depth from start URL.                                                   |
| `--concurrency` | `-c`  | `usize`  | `10`               | Number of concurrent fetch tasks.                                                     |
| `--delay`       |       | `u64`    | `0`                | Delay between requests in milliseconds (0 = auto-AIMD).                               |
| `--render-js`   |       | `bool`   | `false`            | Enable Headless Chrome CDP for JavaScript rendering.                                  |
| `--chrome-ws`   |       | `String` | `auto`             | Remote Chrome WebSocket URL (e.g. `ws://127.0.0.1:9222`).                             |
| `--user-agent`  | `-u`  | `String` | `SEOLens/1.0`      | Custom User-Agent string.                                                             |
| `--format`      | `-f`  | `String` | `terminal,json,md` | Comma-separated outputs: `terminal,json,md,html,csv,all`.                             |
| `--output-dir`  | `-o`  | `Path`   | `./reports`        | Directory where export artifacts are saved.                                           |
| `--fail-on`     |       | `String` | `none`             | CI/CD threshold: `critical`, `alert`, or `warning`. Returns exit code `1` if matched. |
| `--no-robots`   |       | `bool`   | `false`            | Ignore `/robots.txt` disallow rules.                                                  |
| `--ephemeral`   |       | `bool`   | `false`            | Do not persist results to SQLite; auto-cleanup on finish.                             |

---

### 2.2 `seolens mcp`

Launches the Model Context Protocol server for AI coding assistants (Claude Desktop, Cursor, Windsurf).

```bash
seolens mcp [OPTIONS]
```

#### Options

| Option        | Default | Description                                                         |
| ------------- | ------- | ------------------------------------------------------------------- |
| `--transport` | `stdio` | Transport mechanism: `stdio` (local agents) or `sse` (remote HTTP). |
| `--port`      | `8080`  | Port to bind for HTTP/SSE transport (when `--transport sse`).       |

---

### 2.3 `seolens report <session>`

Inspects or re-exports an existing audit session from SQLite.

```bash
seolens report <session_id> --format csv,json,md -o ./exports
```

---

### 2.4 `seolens list`

Lists all audit sessions currently stored in the local SQLite database.

```bash
seolens list
```

---

## 3. Exit Codes (CI/CD Pipeline Integration)

`SEO Lens` is designed to run in automated GitHub Actions, GitLab CI, and deployment pipelines:

| Exit Code | Meaning                     | Condition                                                                                                                    |
| --------- | --------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `0`       | **Success / Clean**         | Audit completed successfully with no violations above `--fail-on` threshold.                                                 |
| `1`       | **SEO Threshold Violation** | Found one or more issues matching the `--fail-on` flag (e.g. `--fail-on critical` failed on broken links or missing titles). |
| `2`       | **Runtime / Network Error** | Target URL unreachable, DNS resolution failed, invalid flags, or disk full.                                                  |
| `130`     | **Interrupted**             | Gracefully terminated by user via `SIGINT` (`Ctrl+C`). SQLite state remains intact.                                          |

---

## 4. Terminal UI Specification (`indicatif`)

During execution, `seolens audit` renders a live, colored ANSI terminal dashboard:

```bash
🔍 SEO Lens v0.1.0 — Crawling: https://example.com
────────────────────────────────────────────────────────────────────────
[00:00:18] [████████████████████░░░░░] 342/500 pages (19.0 p/s)
Active Delay: 75ms (AIMD) | p95 TTFB: 240ms | Memory: 32MB

Found Issues: 🚨 2 Critical  |  ⚠️ 8 Alerts  |  ⚡ 24 Warnings
────────────────────────────────────────────────────────────────────────
Current: https://example.com/products/wireless-headphones
```

### Post-Crawl Terminal Scorecard

Upon completion, the terminal displays an executive scorecard:

```bash
========================================================================
                        SEO LENS AUDIT SCORECARD
========================================================================
Target:          https://example.com
Health Score:    84 / 100
Duration:        24.6s (482 pages crawled, 1,420 internal links)
Avg TTFB:        185ms (p95: 310ms)

HTTP Status Breakdown:
  ✔ 200 OK:           468 (97.1%)
  ℹ 301 Redirect:      10  (2.1%)
  ✖ 404 Not Found:      4  (0.8%)

Top Priority Issues:
  🚨 [ERR_CANONICAL_TO_4XX_5XX] (3 pages)
     Canonical points to dead 404 URL
  🚨 [ERR_H1_MISSING] (1 page)
     https://example.com/checkout
  ⚠️ [ALERT_GEO_AI_RETRIEVAL_BOT_BLOCKED] (Site-wide)
     robots.txt blocks PerplexityBot
  ⚡ [WARN_IMG_MISSING_ALT] (14 images)
     Missing descriptive alt attributes

Exported Artifacts:
  📄 Markdown:  ./reports/example_com_audit.md
  📊 JSON:      ./reports/example_com_audit.json
  🌐 HTML:      ./reports/example_com_audit.html
  📑 CSVs:      ./reports/csv/ (internal_all.csv, issues_all.csv)
========================================================================
```

---

## 5. Report Exporters Specification

---

### 5.1 Standalone HTML Report (`report.html`)

- **Self-Contained**: CSS and JS are compiled directly into the HTML file using `rust-embed`.
- **Zero External Dependencies**: Renders completely offline without calling Google Fonts, external CDNs, or third-party trackers.
- **Interactive Features**:
  - Live search bar filtering by URL, title, or status code.
  - Severity filter chips (Critical, Alert, Warning).
  - Issue accordion with copyable code remediation instructions.
  - Interactive link equity chart and crawl depth histogram.

---

### 5.2 Screaming Frog Compatible CSV Suite

To enable immediate compatibility with existing client spreadsheet workflows, `seolens` exports four industry-standard CSVs under `--format csv`:

#### 1. `internal_all.csv`

Columns matching Screaming Frog standard export:
`Address`, `Status Code`, `Status`, `Content Type`, `Size (Bytes)`, `Word Count`, `Title 1`, `Title 1 Length`, `Meta Description 1`, `Meta Description 1 Length`, `H1-1`, `H1-1 Length`, `Canonical Link Element 1`, `Indexability`, `Indexability Status`, `Inlinks`, `Outlinks`, `Crawl Depth`, `Response Time (ms)`.

#### 2. `issues_all.csv`

Summary of all triggered rules:
`Issue Code`, `Issue Name`, `Severity`, `Category`, `URL`, `Source URL`, `Details`, `Recommendation`.

#### 3. `response_codes.csv`

URL routing map:
`URL`, `Status Code`, `Status`, `Redirect URL`, `Redirect Type`, `Inlinks Count`.

#### 4. `external_all.csv`

Outbound link audit:
`Source URL`, `Destination URL`, `Anchor Text`, `Status Code`, `Is Nofollow`.

---

### 5.3 Machine-Readable JSON (`audit.json`)

The complete, lossless structured schema containing:

- `summary`: Crawl statistics, health score, duration, timing percentiles.
- `crawled_pages`: Array of full `PageReport` objects.
- `issues`: Grouped issue arrays with occurrence counts and affected URLs.
- `site_graph`: Nodes (URLs) and edges (links with anchor text and attributes).

---

### 5.4 Executive Markdown (`report.md`)

Designed for human executive review and client emails:

- Clean GitHub Flavored Markdown with badge formatting.
- Organized into: Executive Summary $\rightarrow$ Critical Blockers $\rightarrow$ Optimization Opportunities $\rightarrow$ Technical Action Items.

---

## 6. Summary

This specification guarantees:

- A polished, developer-friendly CLI with standard UNIX exit codes for CI/CD integration.
- 100% interoperability with agency client workflows via Screaming Frog compatible CSVs.
- Self-contained, beautiful offline HTML dashboards that clients can open directly in any browser.
