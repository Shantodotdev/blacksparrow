# 🚀 SEO Lens v0.1.0-rc.1 (Release Candidate)

**SEO Lens** is a high-performance, local-first website crawler, 120-rule technical SEO audit engine, and AI-native auditor built from scratch in Rust.

This first Release Candidate introduces the complete core crawler, graph analysis engine, comprehensive rule catalog, and native Model Context Protocol (MCP) integration for AI agents.

---

## ⚡ Quick Install

### macOS & Linux (Native Installer)
```bash
curl -fsSL https://raw.githubusercontent.com/Shantodotdev/seo-lens/main/install.sh | bash
```

### Windows (PowerShell)
```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/Shantodotdev/seo-lens/releases/download/v0.1.0-rc.1/seo-lens-installer.ps1 | iex"
```

*Prebuilt `.tar.xz`, `.zip`, and `.msi` installers are also directly downloadable in the Assets section below.*

---

## 🌟 What's Inside v0.1.0-rc.1

### 🕷️ 1. High-Performance Local-First Crawler

- **AIMD Congestion Controller**: Adaptive additive-increase/multiplicative-decrease politeness engine that dynamically tracks server TTFB latency and backs off on HTTP 429/5xx errors without overloading target servers.
- **Frontier & SwissTable Deduplication**: Hash-based deduplication (`hashbrown::HashSet<u64>`) capable of managing 50,000+ frontier URLs in under 1 MB of memory.
- **Faceted Spider Trap Guard**: Automatic pruning of infinite calendar loops, tracking parameters (`utm_*`, `gclid`, `fbclid`), and sorting facet variations.
- **Robots & Sitemap Discovery**: Fast RFC 9309 compliant `/robots.txt` parser with automated recursive XML sitemap index feed discovery.

### 🔍 2. 120-Rule Technical SEO Audit Catalog

Diagnostic heuristics spanning 11 core SEO categories:

- **Indexability & Status Codes**: 4xx dead ends, 5xx server errors, redirect chains, redirect loops, and canonicalization defects.
- **Metadata & Viewport**: Title length/boundaries, meta description optimization, heading hierarchy (`h1`-`h6`), and mobile viewports.
- **Structured Data (Schema.org)**: Streaming JSON-LD extractor validating against Google Rich Results guidelines (Product, Article, FAQ, Breadcrumbs).
- **Generative Engine Optimization (GEO)**: Automated discovery and compliance audits for `/llms.txt` and AI search citation bots (`GPTBot`, `ClaudeBot`, `PerplexityBot`).

### 🕸️ 3. Site Graph Topology & Internal PageRank

- **Petgraph Directed Topology**: In-memory link graph tracking inbound links, outbound links, and site depth hierarchy.
- **Internal PageRank Algorithm**: Iterative power-iteration PageRank calculation identifying orphaned pages and link-equity distribution hubs.

### 🤖 4. Native Model Context Protocol (MCP) Server

- Turn SEO Lens into an autonomous pair-programming auditor for **Claude Desktop, Cursor, Windsurf, and Antigravity**.
- Implements 8 native JSON-RPC 2.0 tools over `stdio`:
  - `seo_start_audit`: Non-blocking background crawl kickoff (< 1.0s).
  - `seo_audit_status`: Real-time telemetry, TTFB latency, and issue counts.
  - `seo_get_markdown_report`: Token-efficient Markdown reports with zero ANSI escape sequences.
  - `seo_quick_page_check`: Synchronous single-page audits in < 500ms.
  - `seo_query_issues`: Filtered defect queries by severity, category, or URL pattern.
  - `seo_check_ai_readiness`: Standalone Generative Engine Optimization audit.
  - `seo_validate_schema`: Google Rich Results structured data validation.
  - `seo_cleanup_session`: Session disk reclamation.

### 📊 5. Rich Multi-Format Exporters

- **Single-File Interactive HTML**: Self-contained visual audit dashboard with zero external CDN dependencies, embedded SVG dials, live search, and severity filters.
- **Terminal UI**: ANSI dashboard with color-coded telemetry badges, health scores, and defect trees.
- **Screaming Frog Compatible CSVs**: Direct drop-in `internal_all.csv`, `issues_all.csv`, and `external_all.csv` exports with formula injection (CWE-1236) protection.
- **Machine-Readable JSON**: Complete schema-compliant audit dumps for CI/CD assertions.

---

## 🛠️ CLI Quickstart

```bash
# 1. Quick single-page inspection
seolens inspect https://example.com

# 2. Full site crawl and multi-format export
seolens audit https://example.com --max-pages 500 --format html,markdown,csv --output ./reports

# 3. Launch the native AI agent MCP server
seolens mcp
```

---

## 📦 Verified Assets & SHA-256 Checksums

All platform binaries and packages are cryptographically verified via `sha256.sum`.
