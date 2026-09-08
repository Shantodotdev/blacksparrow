# SEO Lens: CLI & Exporters Reference

Welcome to the command-line interface (CLI) and reporting reference for **SEO Lens**!

Whether you're running audits locally from your terminal, integrating SEO checks into your CI/CD pipelines, or exporting data for spreadsheets and dashboards, this guide covers every subcommand, option, exit code, and export format.

---

## 1. Quick Tour & Subcommand Overview

SEO Lens provides **10 purpose-built subcommands** via the `seolens` binary:

```bash
seolens
├── audit <url>       # Run a full website crawl & technical audit
├── inspect <url>     # Instant X-ray for a single webpage (headers, DOM, schema)
├── mcp               # Start native Model Context Protocol server (stdio)
├── report <session>  # Re-export or inspect a past audit without recrawling
├── list              # List all historical audit sessions stored in SQLite
├── issues <session>  # Filter and drill down into findings for an audit
├── check-ai <url>    # Audit AI search readiness (/llms.txt, AI bot policies)
├── schema <url>      # Validate JSON-LD structured data against Google Rich Results
├── delete <session>  # Delete a specific crawl session and its records
└── clean             # Reclaim disk space by cleaning old crawl sessions
```

> [!TIP]
> Run `seolens <subcommand> --help` anytime to view the built-in documentation and default values directly in your terminal.

---

## 2. Command Details & Practical Examples

### 2.1 `seolens audit <url>` (Full Website Crawl)

The primary command for technical SEO auditing. It discovers URLs, parses pages with a zero-copy streaming parser, applies AIMD adaptive rate limiting, runs 120 SEO rules, and exports reports.

```bash
# Basic crawl (defaults: 500 pages, max depth 5, concurrency 10)
seolens audit https://example.com

# High-depth crawl with HTML, CSV, and Markdown exports
seolens audit https://example.com -p 2000 -d 8 -f html,csv,md -o ./my-reports

# High-speed local audit with AIMD throttling disabled
seolens audit http://localhost:3000 --no-aimd -p 100

# CI/CD check: fail pipeline if any Critical issues are detected
seolens audit https://staging.example.com --fail-on critical
```

#### Flags & Options

| Flag / Option             | Short | Type     | Default              | What It Does                                                      |
| :------------------------ | :---- | :------- | :------------------- | :---------------------------------------------------------------- |
| `<url>`                   |       | `String` | _(Required)_         | Root URL to crawl (e.g. `https://example.com`).                   |
| `--max-pages`             | `-p`  | `u32`    | `500`                | Maximum pages to crawl (`0` = unlimited).                         |
| `--max-depth`             | `-d`  | `u16`    | `5`                  | Maximum click depth from start URL.                               |
| `--concurrency`           | `-c`  | `usize`  | `10`                 | Number of concurrent network requests.                            |
| `--delay`                 |       | `u64`    | `0`                  | Delay between requests in ms (`0` = auto-AIMD).                   |
| `--no-aimd`               |       | `bool`   | `false`              | Disable adaptive AIMD throttling (ideal for local staging tests). |
| `--render-js`             |       | `bool`   | `false`              | Enable Headless Chrome CDP for JavaScript SPAs.                   |
| `--chrome-ws`             |       | `String` | `"auto"`             | Custom Chrome WebSocket URL (e.g. `ws://127.0.0.1:9222`).         |
| `--user-agent`            | `-u`  | `String` | `"SEOLens/1.0"`      | Custom User-Agent header string.                                  |
| `--format`                | `-f`  | `String` | `"terminal,json,md"` | Outputs: `terminal`, `json`, `md`, `html`, `csv`, or `all`.       |
| `--output-dir`            | `-o`  | `Path`   | `"./reports"`        | Directory where export files will be saved.                       |
| `--fail-on`               |       | `String` | `"none"`             | CI/CD gate: `critical`, `alert`, or `warning`.                    |
| `--no-robots`             |       | `bool`   | `false`              | Ignore `/robots.txt` disallow rules.                              |
| `--ephemeral`             |       | `bool`   | `false`              | Ephemeral run: auto-cleans SQLite state on finish.                |
| `--max-query-params`      |       | `usize`  | `2`                  | Max query parameters allowed before pruning spider traps.         |
| `--ignore-sorting-facets` |       | `bool`   | `true`               | Prunes faceted sorting parameters (`sort`, `order`, etc.).        |
| `--db-path`               |       | `Path`   | _(System default)_   | Custom path to SQLite persistence database.                       |
| `--local`                 | `-L`  | `bool`   | `false`              | Persist database locally to `./.seolens/seolens.db`.              |

---

### 2.2 `seolens inspect <url>` (Single-Page X-Ray)

Need to quickly inspect a single page without running a site crawl? `inspect` fetches the URL, runs document-level SEO checks, and outputs a complete technical X-ray in under 500 milliseconds.

```bash
# Inspect a live page
seolens inspect https://example.com/about

# Output as structured JSON for piping into jq
seolens inspect https://example.com/about -f json | jq '.headings'
```

---

### 2.3 `seolens mcp` (Model Context Protocol Server)

Launches the native Model Context Protocol (MCP) server over `stdio`. This allows AI coding agents like **Claude Desktop**, **Cursor**, and **Windsurf** to communicate directly with SEO Lens.

```bash
# Start MCP server over stdio
seolens mcp
```

_(See [`mcp.md`](./mcp.md) for tool definitions and agent configuration guides)._

---

### 2.4 `seolens report <session_id>` (Re-Export Existing Audits)

Every crawl is saved in your local SQLite database. If you ran an audit yesterday and now want to generate an interactive HTML report or CSV files, `report` does this instantly without touching the network:

```bash
# Generate HTML and CSV reports for session crawl_1788718395
seolens report crawl_1788718395 -f html,csv -o ./exports
```

---

### 2.5 `seolens list` (Session History)

Lists all audit sessions stored in your local database with target URLs, page counts, durations, and health scores.

```bash
# List recent sessions
seolens list

# Show up to 50 sessions in JSON format
seolens list -n 50 -f json
```

---

### 2.6 `seolens issues <session_id>` (Issue Drill-Down)

Allows you to filter and inspect issues discovered during a crawl directly in your terminal.

```bash
# View only Critical issues for an audit
seolens issues crawl_1788718395 -s critical

# View Security category issues
seolens issues crawl_1788718395 -c security
```

---

### 2.7 `seolens check-ai <url>` (AI & GEO Readiness)

Audits whether a website is ready for Generative Engine Optimization (GEO) and AI search engines:

- Checks presence and structure of `/llms.txt` and `/llms-full.txt`.
- Inspects `/robots.txt` to see if AI Retrieval/Search Bots (e.g. `PerplexityBot`, `OAI-SearchBot`) or AI Training Crawlers (e.g. `GPTBot`, `ClaudeBot`) are blocked.

```bash
seolens check-ai https://example.com
```

---

### 2.8 `seolens schema <url>` (Structured Data Validator)

Extracts all JSON-LD scripts from a URL and validates them against Google Rich Results specifications (Product, Article, FAQ, LocalBusiness, Breadcrumbs, etc.).

```bash
seolens schema https://example.com/products/headphones
```

---

### 2.9 `seolens delete <session_id>` & `seolens clean` (Storage Management)

Manage your local SQLite storage footprint:

```bash
# Delete a specific crawl session
seolens delete crawl_1788718395 -y

# Preview what sessions would be cleaned (dry run)
seolens clean --keep 5 --dry-run

# Reclaim space: keep only the 5 most recent crawls and delete the rest
seolens clean --keep 5 -y
```

---

## 3. Exit Codes (CI/CD Pipelines)

SEO Lens uses standard UNIX exit codes so you can plug audits directly into GitHub Actions, GitLab CI, or pre-deployment hooks:

| Exit Code | Meaning                     | Condition                                                                               |
| :-------: | :-------------------------- | :-------------------------------------------------------------------------------------- |
|  **`0`**  | **Clean / Success**         | Audit finished successfully and no issues violated the `--fail-on` threshold.           |
|  **`1`**  | **Threshold Violation**     | Found one or more issues matching or exceeding `--fail-on` (e.g. `--fail-on critical`). |
|  **`2`**  | **Runtime / Network Error** | Target URL unreachable, DNS failure, invalid arguments, or disk error.                  |
| **`130`** | **Interrupted (`Ctrl+C`)**  | User cancelled the crawl gracefully. Partial results remain saved in SQLite.            |

---

## 4. Multi-Format Exporters

SEO Lens supports 5 complementary export formats:

### 4.1 Standalone Interactive HTML (`report.html`)

- **Self-Contained**: 100% offline. CSS, SVG icons, and JavaScript are bundled directly into the single file. No external CDNs or Google Fonts.
- **Authentic Workstation Aesthetic**: Features an ASCII branding banner, high-contrast monospace typography, and retro CRT workstation styling.
- **Interactive Filtering**:
  - Real-time search bar filtering across URLs, titles, error codes, and issue descriptions.
  - Severity filter tabs (All, Critical, Alerts, Warnings, Notices).
  - Expandable issue drawers with direct remediation advice.
  - Dynamic ASCII-style progress bars and telemetry indicators.

### 4.2 Screaming Frog Compatible CSV Suite (`reports/csv/`)

Designed for agency teams and SEO consultants who work with spreadsheets:

1. `internal_all.csv`: Full crawl inventory matching Screaming Frog columns (URL, Status, Title, Description, H1, Canonical, Inlinks, Outlinks, TTFB, Word Count).
2. `issues_all.csv`: Complete list of triggered audit findings with severity, categories, affected URLs, and remediation steps.
3. `response_codes.csv`: HTTP routing breakdown and redirect targets.
4. `external_all.csv`: External links found, anchor texts, and `rel="nofollow"` attributes.

### 4.3 Executive Markdown (`report.md`)

Formatted using clean GitHub-Flavored Markdown. Perfect for:

- Pasting directly into client audit summaries or PR descriptions.
- Providing context directly to LLM prompt windows without token bloat.

### 4.4 Machine-Readable JSON (`report.json`)

Lossless structured dump of the entire audit: crawl summary, timing percentiles, every `PageReport`, triggered issues, and site link graph edges.

### 4.5 Live Terminal UI

During execution, `seolens audit` renders a live colored ANSI dashboard showing:

- Real-time crawl rate (pages/sec) and progress bar.
- Dynamic AIMD delay and p95 server TTFB.
- Live issue counters (🚨 Critical, ⚠️ Alerts, ⚡ Warnings).
- Post-crawl executive scorecard with health score (0–100) and status breakdown.
