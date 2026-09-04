# SEO Lens: Production Specification & Engineering Roadmap

**Document Status**: Active / Long-Term Living Specification  
**Architecture**: Unified 2-Member Cargo Workspace (`.` for Core Library & Headless CLI, `src-tauri` for Native Desktop App)  
**Development Methodology**: Strict Test-Driven Development (TDD) across Micro-Phases

---

## 1. System Overview & Production Goals

`SEO Lens` is a high-performance, local-first website crawler, technical SEO audit engine, and AI-native auditor written in Rust. It is built to run reliably on client websites, local developer machines, production Linux servers, Docker containers, and autonomous AI agent environments.

### Core Production Requirements

1. **Resilience**: The crawler must never crash or panic on malformed HTML, circular redirects, connection resets, TLS handshake errors, or malicious payloads (e.g. decompression bombs or infinite calendar loops).
2. **Politeness & Safety**: The system must actively prevent origin overloading. An adaptive congestion controller must throttle requests dynamically based on origin latency and error spikes, strictly respecting `robots.txt` directives.
3. **Accuracy**: Audit rules must avoid false positives. Edge cases (e.g., `<meta>` tags inside `<body>`, relative canonical URLs, soft 404s, WAF challenge pages) must be explicitly classified.
4. **AI-Agent Readiness (MCP)**: Native Model Context Protocol (MCP) server running in-process, exposing non-blocking asynchronous tools for AI coding assistants and autonomous agents.
5. **Zero-Bloat Multi-Target Deployment**:
   - Headless CLI / MCP single executable (`seolens audit`, `seolens mcp`) for developers, CI/CD, and AI agents.
   - Native Desktop Application powered by Tauri v2 (`.dmg`, `.msi`, `.AppImage`) for non-technical clients, WordPress/Webflow creators, and vibe coders.
   - Zero external runtime dependencies (no Node.js or Python needed for the end user).

---

## 2. Engineering Standards & TDD Protocol

Every module in `SEO Lens` will be implemented strictly using **Test-Driven Development (TDD)**:

```
┌─────────────────────────────────────────────────────────────┐
│                       TDD Cycle                             │
│                                                             │
│  [1. Write Failing Test]  ──>  [2. Minimal Implementation]  │
│          ▲                                   │              │
│          │                                   ▼              │
│  [4. Test Verification]   <──  [3. Refactor & Lint]         │
│└────────────────────────────────────────────────────────────┘
```

### Protocol Rules:

1. **Tests First**: No implementation code is written without a corresponding automated test fixture defining the expected behavior.
2. **Automated + Manual Verification**: Each micro-phase must satisfy both:
   - Automated unit and integration test passes (`cargo test`).
   - Manual verification (CLI invocation, inspecting data output, edge-case testing) reviewed collaboratively before advancing.
3. **No Unverified Assumptions**: Performance, memory, and binary sizes are treated as empirical engineering metrics to be measured, profiled, and optimized during testing—not assumed upfront.
4. **Typed Error Handling**: All library functions return structured `Result<T, SeoError>` using `thiserror`. Panics (`unwrap()`, `expect()`) are forbidden in library code paths.

---

## 3. Modular Architecture Topology (2-Member Cargo Workspace)

The project uses a **2-Member Cargo Workspace**. This architecture ensures:
- **Unified Build Cache**: One root `target/` directory and one `Cargo.lock`. Shared dependencies (`tokio`, `reqwest`, `serde`, `rusqlite`) are compiled once, preventing 5–10 GB of duplicate build artifacts.
- **Zero GUI Bloat in CLI**: Desktop/Tauri dependencies (`tauri`, `webkit2gtk`) only exist in `src-tauri/Cargo.toml`. The CLI binary (`seolens`) remains ultra-lean (~15MB) with zero OS GUI dependencies for CI/CD and Docker.
- **Frictionless Development**: Phases 0 through 9 are built directly in `src/` and `tests/`. Phase 10 seamlessly connects `src-tauri` to `seo-lens` via `path = ".."`.

### Directory Layout:

```
seo-lens/
├── Cargo.toml                     # Root workspace manifest & Member 1 (Engine + CLI)
├── ui/                            # React 19 + Tailwind + Vite Desktop UI
│   ├── package.json
│   ├── vite.config.ts
│   ├── index.html
│   └── src/                       # Virtualized tables, charts, live telemetry UI
├── src-tauri/                     # Member 2: Tauri v2 Native Desktop Wrapper
│   ├── Cargo.toml                 # Depends on: seo-lens = { path = ".." }
│   ├── tauri.conf.json            # Desktop window configuration & app icons
│   └── src/
│       ├── main.rs                # Tauri desktop entry point
│       ├── commands.rs            # #[tauri::command] IPC bindings to core engine
│       └── events.rs              # Real-time event bridge (app.emit)
├── tests/                         # Integration tests & fixtures
│   ├── fixtures/                  # Synthetic HTML, sitemaps, robots.txt files
│   ├── crawl_tests.rs             # End-to-end crawling test suite
│   ├── rules_tests.rs             # SEO rules validation suite
│   └── mcp_tests.rs               # MCP JSON-RPC protocol test suite
└── src/                           # Core Library & CLI Implementation
    ├── main.rs                    # Headless CLI entry point (`audit`, `mcp`, `report`)
    ├── lib.rs                     # Library root exporting public API
    ├── core/                      # Domain models, URL normalization, configuration
    ├── crawler/                   # Async HTTP engine, AIMD politeness, frontier, browser CDP
    ├── parser/                    # lol_html streaming parser, metadata, schema, content
    ├── rules/                     # 120 technical SEO checks (single-page, graph, JS diff)
    ├── graph/                     # petgraph link topology and internal PageRank
    ├── storage/                   # SQLite WAL persistence and batch operations
    ├── mcp/                       # Model Context Protocol stdio & SSE server
    └── report/                    # Markdown (LLM), JSON, CSV, and HTML exporters
```

### Workspace Manifest Definition (`Cargo.toml`):

```toml
[workspace]
members = [
    ".",           # Member 1: Core engine library + CLI binary
    "src-tauri",   # Member 2: Tauri desktop application wrapper
]
resolver = "2"

[package]
name = "seo-lens"
version = "0.1.0"
edition = "2021"

[lib]
name = "seo_lens"
path = "src/lib.rs"

[[bin]]
name = "seolens"
path = "src/main.rs"
```

And in `src-tauri/Cargo.toml`:

```toml
[package]
name = "seo-lens-desktop"
version = "0.1.0"
edition = "2021"

[dependencies]
seo-lens = { path = ".." }
tauri = { version = "2", features = [] }
```

---

## 4. Step-by-Step Implementation Roadmap (Micro-Phases)

---

### Phase 0: Project Scaffolding, Tooling & Test Harness

**Objective**: Set up the Rust 2-member workspace, linting rules, dependency baselines, and test fixture infrastructure.

#### Tasks:

1. Initialize root `Cargo.toml` with `[workspace] members = [".", "src-tauri"]` and core dependencies: `tokio`, `serde`, `serde_json`, `thiserror`, `anyhow`, `tracing`, `clap`, `compact_str`, `bitflags`.
2. Configure Cargo release profile (`opt-level = "z"`, `lto = true`, `strip = true`).
3. Set up test fixture directory (`tests/fixtures/`) and mock HTTP server harness (`wiremock`).
4. Establish `src/lib.rs` and `src/main.rs` with baseline logging.

#### Verification Gate:

- **Automated**: `cargo test` executes cleanly. `cargo clippy` passes with zero warnings.
- **Manual**: Run `seolens --help` and verify CLI help output.

---

### Phase 1: Core Domain Models & URL Canonicalization

**Objective**: Build reliable, high-performance URL normalization and domain modeling.

#### Tasks:

1. `core/url.rs`: URL normalizer (strip tracking params, normalize schemes, resolve relative paths, lowercase hostnames, handle trailing slash consistency).
2. `core/models.rs`: Core types: `PageReport`, `DiscoveredLink`, `Issue`, `Severity` (Critical, Alert, Warning), `IssueCategory`, `CrawlSummary`.
3. `core/config.rs`: Crawl parameters (depth, concurrency, delay, headers, user-agents, proxy).

#### Verification Gate:

- **Automated (TDD)**: Test suite with 40+ URL edge cases (query strings, port numbers, IPv4/IPv6, punycode, non-ASCII paths, fragments).
- **Manual**: CLI command testing URL normalization inputs and verifying deterministic outputs.

---

### Phase 2: Streaming HTML Parsing Engine (`lol_html`)

**Objective**: Zero-copy extraction of HTML tags, metadata, and body content without loading full DOM trees into memory.

#### Tasks:

1. `parser/streaming.rs`: `lol_html` streaming rewriter setup with pre-compiled CSS selectors for `<title>`, `<meta>`, `<link>`, `<h1>`-`<h6>`, `<a>`, `<img>`, `<script>`.
2. `parser/metadata.rs`: Extraction of canonical URLs, OpenGraph, Twitter Cards, charset, viewport, robots directives.
3. `parser/content.rs`: Text token extraction isolating editorial content (ignoring `<nav>`, `<header>`, `<footer>`, `<script>`, `<style>`).
4. `parser/schema.rs`: Extraction of JSON-LD (`<script type="application/ld+json">`) and Microdata attributes.

#### Verification Gate:

- **Automated (TDD)**: Synthetic HTML test fixtures verifying correct extraction on malformed HTML, unclosed tags, and deeply nested structures.
- **Manual**: Pass a saved HTML page from a major website through the parser and verify complete extracted metadata JSON.

---

### Phase 3: Asynchronous HTTP Fetcher & AIMD Politeness Controller

**Objective**: Resilient network fetching engine with adaptive rate limiting to prevent origin overloading.

#### Tasks:

1. `crawler/client.rs`: `reqwest` HTTP/2 client wrapper with custom redirect policies, timeout handling, and transport error classification (DNS failure, SSL error, connection refused).
2. `crawler/aimd.rs`: Additive-Increase/Multiplicative-Decrease congestion controller (tunes delay based on error rates and latency percentiles).
3. `crawler/waf.rs`: Fingerprint detection for Cloudflare, Akamai, DataDome, and Imperva bot challenge screens.

#### Verification Gate:

- **Automated (TDD)**: Wiremock tests simulating 429 rate limits, 503 gateway timeouts, and response latency spikes to verify AIMD backoff and recovery.
- **Manual**: Run fetch against test endpoint with artificial rate limits and observe smooth adaptive throttling.

---

### Phase 4: Frontier Queue, Depth Traversal & Robots/Sitemap Engine

**Objective**: Robust crawl frontier managing discovery queues, depth boundaries, `robots.txt`, and XML sitemaps.

#### Tasks:

1. `crawler/frontier.rs`: Deduplication hash set (`hashbrown`), BFS/DFS queue management, max-pages and max-depth enforcement.
2. `crawler/robots.rs`: RFC 9309 compliant `robots.txt` parser with support for User-Agent matching and `Crawl-Delay`.
3. `crawler/sitemap.rs`: Streaming XML sitemap parser (`quick-xml`) supporting sitemap indexes, compressed `.xml.gz`, and alternate hreflang entries.

#### Verification Gate:

- **Automated (TDD)**: Complex `robots.txt` directive tests (wildcards, disallow vs allow precedence) and nested sitemap index parsing.
- **Manual**: Crawl a multi-level mock site; verify that max depth and URL exclusion rules are strictly respected.

---

### Phase 5: Technical SEO Rules Engine — Phase 1 (Single-Page In-Flight Rules)

**Objective**: Implement 80+ immediate document-level checks evaluated as pages are fetched.

#### Tasks:

1. `rules/catalog.rs`: Master issue dictionary with unique codes, severity tiers (Critical, Alert, Warning), and human-readable descriptions.
2. `rules/page/titles.rs` & `descriptions.rs`: Length, absence, multiple tags, whitespace irregularities.
3. `rules/page/headings.rs`: Missing H1, multiple H1, empty H1, heading hierarchy skipping.
4. `rules/page/status.rs`: HTTP 4xx, 5xx, 3xx redirects, timeouts, SSL handshake failures.
5. `rules/page/security.rs`: Missing HSTS, CSP, X-Frame-Options, mixed content resources, insecure form actions.
6. `rules/page/mobile.rs` & `images.rs`: Viewport presence, missing image `alt`, missing `width`/`height` dimensions (CLS).
7. `rules/page/schema_val.rs`: Validation of JSON-LD schemas against Google Rich Results guidelines (Article, Product, FAQ, LocalBusiness, Breadcrumb).
8. `rules/page/geo.rs`: Checking `/llms.txt` presence and evaluating `robots.txt` for AI Training Bots vs. AI Retrieval/Search Bots.

#### Verification Gate:

- **Automated (TDD)**: Unit tests for each rule module with positive (violating) and negative (passing) HTML fixtures.
- **Manual**: Run audit against an intentionally broken test site and verify that all intentional issues are detected.

---

### Phase 6: Site Graph Topology & Phase 2 Rules (Multi-Page Graph Checks)

**Objective**: Build a directed link graph to execute post-crawl, site-wide architectural analysis.

#### Tasks:

1. `graph/graph.rs`: Directed internal link graph (`petgraph`) mapping source pages to target pages with link attributes (nofollow, anchor text).
2. `graph/pagerank.rs`: Power-iteration internal link equity (PageRank) calculation.
3. `rules/graph/orphans.rs`: Detection of pages found in XML sitemaps with zero incoming internal links.
4. `rules/graph/duplicates.rs`: SHA256 exact duplicate content and SimHash/MinHash near-duplicate detection.
5. `rules/graph/canonicals.rs` & `redirects.rs`: Canonical chains (>1 hop), canonical loops, redirect chains, and circular redirect loops.
6. `rules/graph/hreflang.rs`: Reciprocal bidirectional return tag validation across languages.

#### Verification Gate:

- **Automated (TDD)**: Graph fixture tests verifying correct identification of orphan nodes, cycle detection in redirects, and broken hreflang pairs.
- **Manual**: Verify graph metrics on a simulated site with known orphan pages and redirect loops.

---

### Phase 7: Headless Browser CDP Engine & JavaScript SEO Diffing (`--render-js`)

**Objective**: Optional Chrome DevTools Protocol integration to audit client-rendered SPAs and compare raw HTML vs. rendered DOM.

#### Tasks:

1. `crawler/browser.rs`: `chromiumoxide` CDP manager behind `[features] js-render`. Auto-detects local Chrome/Brave/Edge or connects via `--chrome-ws`.
2. `crawler/diff.rs`: JavaScript SEO Diffing engine comparing initial server HTML with client-rendered DOM.
3. `rules/page/js_diff.rs`: Rules flagging client-side canonical modification, dynamic noindex injection, title/H1 overwriting, and hydration crashes.
4. SPA Heuristic warning in raw HTTP mode when unrendered SPAs are detected.

#### Verification Gate:

- **Automated (TDD)**: Test fixture comparing raw vs rendered HTML with simulated JS modifications.
- **Manual**: Audit a client-rendered React/Vue page with and without `--render-js` to observe the rendered DOM differences.

---

### Phase 8: SQLite Persistence & Storage Layer

**Objective**: High-throughput persistent storage for audit histories, client projects, and resume support.

#### Tasks:

1. `storage/schema.sql`: Relational tables for projects, audits, pages, issues, links, and resources with indexed URL hashes.
2. `storage/sqlite.rs`: SQLite connection pool in WAL mode (`rusqlite`) with batched transactions (250 pages/batch).
3. Query API for filtering issues by category, severity, or URL path.
4. Ephemeral mode support (auto-cleanup for temporary runs).

#### Verification Gate:

- **Automated (TDD)**: In-memory and on-disk SQLite migration and batch insert tests under load.
- **Manual**: Query the resulting SQLite database via CLI to verify relational integrity and query performance.

---

### Phase 9: Model Context Protocol (MCP) Server Implementation

**Objective**: Native in-process MCP server allowing AI agents to perform audits without external wrappers or timeouts.

#### Tasks:

1. `mcp/server.rs`: Stdio and HTTP/SSE JSON-RPC 2.0 transport using `rmcp`.
2. `mcp/tools.rs`: Implementation of non-blocking tools:
   - `seo_start_audit`: Launches background audit, returns `session_id` immediately.
   - `seo_audit_status`: Live progress polling.
   - `seo_get_markdown_report`: Concise Markdown report for LLM ingestion.
   - `seo_quick_page_check`: Synchronous single-URL check.
   - `seo_query_issues`: Query issues with filters.
   - `seo_check_ai_readiness`: Dedicated GEO `/llms.txt` and AI bot check.
3. `mcp/resources.rs`: MCP URI resources for audit summaries and issue logs.

#### Verification Gate:

- **Automated (TDD)**: End-to-end JSON-RPC test simulating an MCP client handshaking, tool listing, and non-blocking audit execution.
- **Manual**: Connect Claude Desktop or Cursor to `seolens mcp` via stdio and execute an audit via natural language.

---

### Phase 10: Native Desktop Application (Tauri v2 + React 19 + Tailwind CSS)

**Objective**: Build the cross-platform native desktop application providing a double-clickable GUI for non-technical users, WordPress/Webflow designers, vibe coders, and technical developers.

#### Tasks:
1. `ui/`: Initialize Vite + React 19 + TypeScript + Tailwind CSS application.
2. Build UI views tailored for all target personas:
   - **Audits Overview & Launch Center**: URL input, preset selectors, AI audit toggle.
   - **Live Telemetry Ring**: Animated % radial gauge, AIMD delay, live TTFB, real-time issue counters.
   - **Executive Scorecard**: 0–100 health gauge, status breakdown donut, 6-pillar radar, and CMS banner.
   - **All Pages Explorer**: High-performance 50,000+ row virtualized grid via `@tanstack/react-virtual`.
   - **Issues Explorer**: Remediation tabs (Code vs. WordPress vs. Webflow/Framer) + **"Copy AI Fix Prompt"** button for vibe coders.
   - **Single Page Detail Drawer**: Headings hierarchy tree, inlinks/outlinks graph, JSON-LD schema inspector, and side-by-side JS SEO Diff.
3. `src-tauri/`: Initialize Tauri v2 desktop wrapper with `tauri-plugin-dialog` (native file export) and `tauri-plugin-notification` (OS alerts).
4. `src-tauri/src/commands.rs`: Implement IPC handlers (`start_crawl`, `stop_crawl`, `list_crawls`, `get_pages`, `get_issues`, `export_report`) directly invoking `src/lib.rs`.
5. `src-tauri/src/events.rs`: Bridge real-time Tokio crawler telemetry to frontend via `app.emit("crawl-progress")`.

#### Verification Gate:
- **Automated (TDD)**: Tauri IPC command deserialization tests, live event emission unit tests, and virtualized table performance benchmarks.
- **Manual**: Run `cargo tauri dev`, launch an audit on a test domain, verify live progress events, test "Copy AI Fix Prompt", and export CSV via the native OS file picker.

---

### Phase 11: CLI Reporting & Exporters

**Objective**: Polished command-line user interfaces and multi-format export files.

#### Tasks:
1. `report/terminal.rs`: Live ANSI terminal dashboard (`indicatif`) with progress indicators and issue severity tables.
2. `report/markdown.rs`: Executive summary formatted for human reading and LLM context windows.
3. `report/json.rs`: Full structured data export.
4. `report/csv.rs`: Screaming Frog compatible CSV exports (`internal_all.csv`, `issues_all.csv`, `response_codes.csv`, `external_all.csv`).
5. `report/html.rs`: Standalone self-contained single-page offline HTML report.

#### Verification Gate:
- **Automated (TDD)**: CSV/JSON schema validation and markdown formatting tests.
- **Manual**: Run full CLI audit on a test domain, inspect terminal output, and verify generated CSVs open cleanly in Excel/Google Sheets.

---

### Phase 12: Production Hardening, Cross-Platform Packaging & Docker

**Objective**: Final production readiness, cross-compilation, desktop installers, and containerization.

#### Tasks:
1. Compiler optimization profile validation.
2. Static compilation verification (`x86_64-unknown-linux-musl`, macOS Apple Silicon/Intel, Windows `.exe`).
3. Tauri desktop packaging (`cargo tauri build`):
   - macOS `.dmg` and `.app` bundle.
   - Windows `.msi` and `.exe` installer.
   - Linux `.AppImage` and `.deb`.
4. Multi-stage Dockerfile:
   - `seolens:slim`: Scratch/Alpine base containing only the static headless CLI binary.
   - `seolens:full`: Debian Slim with pre-installed Chromium for out-of-the-box JS rendering.
5. Final end-to-end integration test suite across CLI (`audit`), MCP (`stdio`), and Desktop (`Tauri`) modes.

#### Verification Gate:
- **Automated**: Full integration test suite passing on all target platforms in CI.
- **Manual**: Run headless CLI in Docker, test MCP server with Cursor/Claude Desktop, and launch the compiled native desktop installer (`.dmg`/`.msi`).

---

## 5. Summary

This document serves as our binding engineering contract. Each micro-phase must be built strictly test-first, verified through both automated tests and manual inspection, and signed off before proceeding to the next.
