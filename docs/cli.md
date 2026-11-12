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

> `blacksparrow schema extract` is a different use of the same command: it prints the JSON Schema of the `extract` output document (see 5.2).

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

---

## 5. Site Extraction (`blacksparrow extract <url>`)

Reads a small-business website the way a brand analyst would and prints one JSON document on stdout (or to `--output`). Built for brand kits and AI agents: it replaces paid scrapers with a single local command. English and Bangla text are supported, including Bangla digits in phone numbers.

```bash
blacksparrow extract https://example.com --pretty
blacksparrow extract https://example.com --render never -o site.json
```

### 5.1 Flags

| Flag                             | Default | Meaning                                                                                   |
| :------------------------------- | :-----: | :---------------------------------------------------------------------------------------- |
| `-p, --max-pages <N>`            |   `6`   | Pages read in total, homepage included.                                                   |
| `--page <URL>` (repeatable)      |    -    | Read these pages after the homepage instead of choosing them by rules (kind `requested`). |
| `--max-stylesheets <N>`          |  `10`   | Most stylesheets fetched for the design analysis (one level of `@import` is followed).    |
| `--render <auto\|always\|never>` | `auto`  | When to use headless Chromium (see 5.4).                                                  |
| `--chrome-path <PATH>`           |    -    | Chromium executable (falls back to `CHROME_PATH`, then the `PATH`).                       |
| `--chrome-ws <WS_URL>`           |    -    | Render with an already running Chromium instead of launching one.                         |
| `--chrome-arg <ARG>`             |    -    | Extra switch for a launched Chromium (repeatable).                                        |
| `--chrome-no-sandbox`            |   off   | Run Chromium without its sandbox (containers without user namespaces).                    |
| `--allow-private-network`        |   off   | Allow private and loopback addresses (local test sites only, see 5.5).                    |
| `--proxy <URL>`                  |    -    | Send all traffic through this egress proxy (`http://[user:pass@]host:port`, see 5.5).     |
| `--timeout-secs <N>`             |  `90`   | Longest the whole extraction may take (minimum 5).                                        |
| `--request-timeout-secs <N>`     |  `20`   | Longest one HTTP request may take.                                                        |
| `-u, --user-agent <UA>`          |    -    | Custom User-Agent for plain fetches.                                                      |
| `--pretty`                       |   off   | Pretty-print the JSON.                                                                    |
| `-o, --output <FILE>`            |    -    | Write the JSON to a file instead of stdout.                                               |

### 5.2 Output schema (`schema_version` 1)

The document is described by a JSON Schema (draft 2020-12) that is built into the binary and committed to the repository as [`schema/extract.schema.json`](../schema/extract.schema.json). Print it with:

```bash
blacksparrow schema extract > extract.schema.json
```

A test validates real `extract` output against it, and CI fails when the committed file differs from what the binary prints. The Python and Node.js SDKs are typed from the same document. The fields:

- `schema_version`, `generator`, `start_url`, `final_url` (homepage after redirects), `outcome`, `outcome_detail`.
- `pages[]`: `url`, `final_url`, `kind` (`home`, `contact`, `about`, `catalog`, `locations`, `ordering`, `requested`), `status`, `rendered`, `render_reason` (`js_shell`, `bot_challenge`, `http_error`, `always`), `waf`, `title`, `description`, `lang`, `word_count`, `markdown`, `links[]` (`text`, `url`), and the page's `phones`, `emails`, `whatsapp`, `facebook`, `instagram`.
- `page_errors[]`: `url`, `error` for chosen pages that could not be read (network failure, non-HTML, or an HTTP error status such as `HTTP 404`).
- `design`: `title`, `description`, `site_name`, `theme_color`, `colors[]` (`hex`, `occurrences`, `usage_score`, `evidence[]` of where it was seen), `fonts[]` (`family`, `selectors` roles such as `body`, `headings`, `buttons`, `weights`), `logos[]` (`reason`, `url` or inline `svg`, best first), `stylesheets[]` (`url`, `status`, `bytes`, `error`).
- `contacts`: `phones`, `emails`, `whatsapp`, `facebook`, `instagram` merged across pages. Facebook and Instagram links are canonicalized to the page or profile (share buttons, posts and tracking pixels are dropped).
- `structured`: schema.org Organization/LocalBusiness facts (`names`, `telephones`, `emails`, `addresses`, `same_as`, `logos`, `descriptions`).
- `stats`: `elapsed_ms`, `http_fetches`, `http_bytes`, `rendered_pages`, `render_ms`, `blocked_browser_requests`, `peak_rss_kib` (Linux).

### 5.3 Outcomes

The command exits `0` with a JSON document even when the site could not be read; check `outcome`:

| Outcome           | Meaning                                                |
| :---------------- | :----------------------------------------------------- |
| `ok`              | The homepage was read.                                 |
| `unsupported_url` | The URL is malformed or not http(s).                   |
| `unresolved`      | The host name does not resolve.                        |
| `blocked_address` | The host is a private or reserved address (see 5.5).   |
| `unreachable`     | Connection failure, timeout or too many redirects.     |
| `bot_protected`   | The homepage is a bot challenge, even after rendering. |
| `http_error`      | The homepage answered with an HTTP error status.       |
| `not_html`        | The homepage is not an HTML page.                      |

### 5.4 Rendering

Pages are fetched over plain HTTP first. Chromium is started lazily and only when needed: in `auto` mode for pages that look like empty JavaScript shells (empty app root, "enable JavaScript" notice, almost no text), bot-challenge screens, and 401/403/429/503 answers. `always` renders every HTML page; `never` disables Chromium. Rendering returns the DOM after scripts settled plus CSS rules that exist only in the CSSOM (CSS-in-JS), so colours and fonts of client-rendered sites are found. If Chromium is missing or fails, the plain page is used and the run continues. Images, fonts and media are not downloaded in the browser (counted in `blocked_browser_requests`), and each run uses a temporary browser profile that is deleted afterwards.

A page that merely embeds a protection provider's script but has real content (150 words or more) is not treated as a challenge. Pages nested deeper than 1000 elements are cut at that depth before parsing, because HTML parsing time grows with the square of the nesting depth.

### 5.5 Security guard

The site is untrusted input. Every request, including each redirect hop and every request made by Chromium, is checked: only `http` and `https` are followed, URLs with credentials are refused, and hosts resolving to private, loopback, link-local (cloud metadata `169.254.169.254`), unique-local, multicast, documentation or otherwise reserved addresses are refused unless `--allow-private-network` is set. Link-local addresses stay refused even then. Name resolution is checked at connect time, so DNS answers cannot swap in a private address afterwards. Response bodies are capped (2 MiB for HTML and CSS, 8 million characters for a rendered DOM) and the whole run is bound by `--timeout-secs`. Only pages linked from the homepage on the same site can be chosen by the rules. `--allow-private-network` exists for local test servers only and must not be used with untrusted URLs.

A launched Chromium resolves no names itself: all its traffic (pages, scripts, WebSockets, iframes, preconnects) goes through a small proxy inside Black Sparrow on the loopback interface, which resolves each host, checks every address and connects only to addresses that passed. WebRTC may not send UDP outside the proxy. This closes DNS rebinding between a check and the browser's own connection.

Proxy environment variables (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`) are ignored, since a proxy resolves names itself and would bypass the check. Pass `--proxy` to use an egress proxy on purpose; the check then becomes advisory (the proxy resolves the name again), so that proxy must enforce its own egress rules.

A browser reached with `--chrome-ws` has its own network: its requests are checked by URL only, so its operator must restrict that browser's egress, and it is left running at the end. `--chrome-ws`, `--chrome-arg`, `--chrome-path`, `--proxy`, `--allow-private-network` and `-o` are operator settings: never build them from untrusted input. All output fields carry website content and are untrusted for the consumer; inline SVG logos are reduced to an allow-list of drawing elements and attributes, but should still be served with a restrictive `Content-Security-Policy` or rendered as images.

---

## 6. Releases and installation

Releases are published from one version tag to GitHub Releases: the binaries, the installers and the Python wheels. The binary is self-contained (no OpenSSL or other system library). **Chromium is not bundled**: it is needed only to render JavaScript-only sites in `extract`, and is found through `--chrome-path`, then `CHROME_PATH`, then the `PATH`.

### 6.1 GitHub Releases

Archives are named `blacksparrow-<target>.tar.xz` (`.zip` on Windows) with a `.sha256` file next to each. Targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`.

```bash
target=x86_64-unknown-linux-gnu
base=https://github.com/Shantodotdev/blacksparrow/releases/latest/download
curl -fsSLO "$base/blacksparrow-$target.tar.xz"
curl -fsSLO "$base/blacksparrow-$target.tar.xz.sha256"
sha256sum -c "blacksparrow-$target.tar.xz.sha256"
gh attestation verify "blacksparrow-$target.tar.xz" --repo Shantodotdev/blacksparrow
```

The attestation proves that the archive was built by this repository's release workflow.

### 6.2 Shell installer

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Shantodotdev/blacksparrow/releases/latest/download/blacksparrow-installer.sh | sh
```

`install.sh` in the repository root is an alternative that also creates the `sparrow` alias; it accepts `--version <tag>`, `--dir <path>` and `--no-verify`, and checks the SHA-256 of the download. Windows has `blacksparrow-installer.ps1` and an `.msi` on the release page.

### 6.3 Python and Node.js

The Python wheels, with the binary inside, are attached to each GitHub release; install the one for your platform by its URL (`pip install "blacksparrow @ <wheel URL>"`). The Python API is in `sdks/python`. The Node package in `sdks/js` is not on the npm registry; install it by path and point it at the release binary with `BLACKSPARROW_BINARY`.

Both SDKs run the binary found at `BLACKSPARROW_BINARY` when that variable is set, which is the way to use a custom build or a binary outside the package.
