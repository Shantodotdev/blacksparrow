# SEO Lens: Native Desktop Application Specification (Tauri v2)
**Document Status**: Permanent Technical Specification  
**Architecture**: Native Cross-Platform Desktop App (Tauri v2 + React 19 + Tailwind CSS + Vite)  
**Deployment**: Native Installers (.dmg for macOS, .msi/.exe for Windows, .AppImage/.deb for Linux), 100% Offline, Zero Port Conflicts, Zero External Runtime

---

## 1. System Vision & Persona Experience

While technical developers and CI/CD pipelines interact with the `seolens` CLI via headless commands, and autonomous AI agents interact via the Model Context Protocol (MCP), **non-technical clients, agency marketers, WordPress/Webflow designers, and vibe coders require a native desktop experience**.

Rather than running a local web server (`localhost:8080`) that creates port conflicts, firewall alerts, and terminal confusion, `SEO Lens` packages a **native desktop application powered by Tauri v2**.

### Target Personas & Tailored Features

```
                     ┌────────────────────────────────────────────────────────┐
                     │              SEO Lens Core Engine (Rust)               │
                     │  Crawler • 120 Rules • SQLite • Parser • Graph • Diff  │
                     └──────────────────────────┬─────────────────────────────┘
                                                │
                 ┌──────────────────────────────┴─────────────────────────────┐
                 │                                                            │
    ┌────────────▼────────────┐                                  ┌────────────▼────────────┐
    │     CLI & MCP Mode      │                                  │   Tauri v2 Desktop App  │
    │ (Headless Single Binary)│                                  │ (Shared React 19 UI)    │
    └────────────┬────────────┘                                  └────────────┬────────────┘
                 │                                                            │
   ┌─────────────┴─────────────┐                                ┌─────────────┴─────────────┐
   ▼                           ▼                                ▼                           ▼
1. Web Developers         2. Vibe Coders               3. CMS Creators            4. Non-Tech Clients
• CLI terminal output     • MCP stdio with Cursor/     • Double-click desktop app • Visual Health Score (0-100)
• CI/CD exit codes          Claude Desktop             • CMS Detection (WP/Framer)• 1-Click PDF / Print export
• Headless Docker         • "Copy AI Fix Prompt"       • Platform-specific fixes  • Plain-English explanations
```

#### 1. "Vibe Coders" (Build with Agents, Never Read Code)
- **The Workflow**: They generate sites with Lovable, Bolt, v0, Cursor Composer, or Windsurf. They do not write Rust or dig through terminal logs.
- **"Copy AI Fix Prompt" Button**: Next to every triggered issue (e.g. missing canonical, unoptimized OpenGraph, invalid schema JSON-LD), the UI provides a 1-click button:
  > *"Act as an expert web engineer. Fix this technical SEO issue on my website: Canonical URL is missing on `/pricing`. Current HTML head: `[...]`. Generate the exact code patch."*
- **MCP Integration**: Vibe coders can alternatively connect `seolens mcp` directly to Cursor or Claude Desktop to let their agent audit and fix issues autonomously.

#### 2. WordPress, Webflow & Framer Creators
- **The Workflow**: Freelancers, agency designers, and marketers who use visual site builders or CMSs and are familiar with tools like Screaming Frog.
- **CMS Auto-Detection**: The crawler detects meta tags, script signatures, and headers for WordPress, Webflow, Framer, Shopify, Wix, and Next.js.
- **Platform-Specific Remediation Tabs**: In the Issue Detail drawer, fixes are translated into actionable CMS steps:
  - *General*: RFC standard explanation and code.
  - *WordPress*: Step-by-step guidance for Yoast SEO or RankMath settings.
  - *Webflow / Framer*: Settings navigation (e.g., Page Settings $\rightarrow$ Custom Code / SEO).

#### 3. Non-Technical Clients & Business Stakeholders
- **Double-Click Installer**: Standard `.dmg` on macOS, `.msi`/`.exe` on Windows. Zero terminal knowledge required.
- **Executive Health Score**: Instant visual 0–100 score ring with plain-English ratings ("Good", "Needs Attention", "Critical Issues Found").
- **Business Impact Translations**: Translates cryptic errors into ROI/business consequences (e.g., instead of just "Canonical points to 404", explains "Google drops this page because it cannot find the preferred master version").
- **1-Click Executive PDF & Print Export**: Generates a clean, client-ready summary document to share with teams.

#### 4. Technical Developers & SEO Specialists
- **High-Performance Virtual Grid**: Smooth 60 FPS scrolling across 50,000+ pages via `@tanstack/react-virtual`.
- **Deep Technical Inspection**: HTTP headers, DOM hierarchy tree, JSON-LD Schema syntax validation, and side-by-side JavaScript SEO Diffing.

---

## 2. Technical Stack Architecture

```mermaid
flowchart TD
    subgraph UI ["React 19 Frontend (ui/ directory)"]
        ReactApp["React 19 + TypeScript"]
        Tailwind["Tailwind CSS"]
        TanStack["@tanstack/react-virtual (50k+ Rows)"]
        Charts["Recharts / Lucide Icons"]
        TauriAPI["@tauri-apps/api (IPC Client)"]
    end

    subgraph TauriApp ["Tauri v2 Desktop Shell (src-tauri/)"]
        WebView["OS Native Webview (WebView2 on Windows / WebKit on macOS)"]
        IPCBridge["Tauri IPC Bridge (Zero Network Sockets)"]
        DialogPlugin["tauri-plugin-dialog (Native File Picker)"]
        NotifyPlugin["tauri-plugin-notification (OS Notifications)"]
    end

    subgraph CoreEngine ["SEO Lens Rust Core Engine"]
        Crawler["Crawler & AIMD Politeness"]
        Rules["120 Rules Engine"]
        SQLite["SQLite Storage (WAL Mode)"]
        DiffEngine["JS SEO Diff Engine"]
        EventBus["Tokio Broadcast Channel (Telemetry)"]
    end

    ReactApp --> TauriAPI
    TauriAPI <-->|Tauri IPC invoke()| IPCBridge
    TauriAPI <--|Tauri listen() Events| IPCBridge
    IPCBridge <--> CoreEngine
    EventBus -->|app.emit()| IPCBridge
    TauriApp --> DialogPlugin
    TauriApp --> NotifyPlugin
```

### 2.1 Why Tauri v2 over Embedded Browser Server (`axum`)

| Feature | Embedded Web Server (`axum` in browser) | Desktop App (Tauri v2) |
|---|---|---|
| **Launch UX** | Requires terminal or background script | Native app icon (Dock / Start Menu) |
| **Port Conflicts** | `localhost:8080` can collide with local dev servers | **Zero sockets**: Uses OS IPC directly |
| **File Export** | Browser download bar, fixed downloads directory | Native OS File Picker ("Save to Desktop...") |
| **OS Notifications** | Browser permissions prompt | Native system banner notifications |
| **Memory / Bundle** | Requires open browser tab + binary (~15MB) | Native OS Webview + binary (~12–18MB) |
| **Chromium Bloat** | N/A | **Zero Chromium**: Uses OS WebView2/WebKit |

---

## 3. UI Screens & User Flows

---

### Screen 1: Audits Overview & Launch Center
The primary workspace when opening the application.

#### UI Elements:
- **Header Bar**: Native macOS/Windows window frame controls, Dark/Light mode toggle, Settings cog.
- **"New Audit" Action Card / Modal**:
  - Target URL input (with URL auto-formatting).
  - Max Pages selector (presets: 100, 500, 1,000, 10,000, Unlimited).
  - Max Depth slider (1 to 10).
  - JavaScript Rendering switch (`--render-js` via local Chrome CDP).
  - Obey `robots.txt` switch.
  - AI / GEO Readiness audit switch.
- **Audits History Table**:
  - Domain, Date, Health Score badge, Total Pages, Duration.
  - Actions: Open Audit, Re-crawl, Export Report, Delete.

---

### Screen 2: Real-Time Live Crawl Telemetry
Displays live progress while a crawl is actively running.

#### UI Elements:
- **Live Progress Ring**: Animated circular gauge displaying % complete.
- **Real-Time KPI Cards**:
  - *Pages Crawled / Discovered*: e.g. `412 / 1,200`.
  - *Current Speed*: Pages per second (e.g. `24 p/s`).
  - *AIMD Delay*: Dynamic politeness backoff in ms.
  - *p95 Response Latency*: TTFB in milliseconds.
- **Live Issues Ticker**: Real-time counter badges for Critical, Alert, and Warning issues found as they happen.
- **Live URL Activity Log**: Scrolling list showing currently fetching URLs.
- **Crawl Controls**: "Pause", "Resume", "Stop & Finalize".

---

### Screen 3: Executive Scorecard & Overview
Comprehensive summary of completed crawl results.

#### UI Elements:
- **Giant Health Score Gauge (0–100)**: Color-coded with plain-English health badge.
- **Status Code Breakdown**: Interactive Donut chart (2xx Green, 3xx Blue, 4xx Red, 5xx Purple).
- **Issue Priority Breakdown**: Stacked severity bar (Critical, Alert, Warning, Notice).
- **6-Pillar Radar Chart**:
  - Technical & Transport
  - On-Page & Headings
  - Indexability & Canonicalization
  - Security & HTTPS
  - Mobile & UX Signals
  - AI & GEO Readiness
- **Detected CMS Banner**: e.g., `"Detected CMS: WordPress 6.5 with Yoast SEO"`.
- **Top 5 Urgent Action Items**: Expandable cards with "Copy AI Fix Prompt" buttons.

---

### Screen 4: All Pages Explorer (High-Performance Virtual Grid)
Primary inspection workspace for all discovered URLs.

#### UI Elements:
- **Instant Search & Filter Bar**: Search by URL, title, or H1. Filter by status (`Broken`, `Redirects`, `Noindex`, `Canonicalized`, `Images`).
- **Virtualized Grid (`@tanstack/react-virtual`)**:
  - Renders only visible DOM rows, supporting 50,000+ pages at 60 FPS without memory leaks.
  - Columns: Status Code, URL, Page Title, Meta Description, H1, Canonical URL, Word Count, Inlinks, Outlinks, Depth, Response Time, Issues Badge.
- **Row Click**: Slides open the **Single Page Detail Drawer**.
- **Native Export**: "Export CSV" opens native OS file dialog.

---

### Screen 5: Issues Explorer
Categorized, actionable list of all triggered SEO rules.

#### UI Elements:
- **Sidebar Filters**: By Severity (Critical, Alert, Warning, Notice) and Category.
- **Issue Cards**:
  - Rule Headline & Severity Badge.
  - Number of affected pages badge.
  - **"Why it matters"** (Business impact for clients).
  - **"How to fix"** with platform tabs:
    - *Code / Developer Fix*: Raw HTML / Nginx / Next.js code snippet.
    - *WordPress Fix*: Step-by-step for Yoast / RankMath.
    - *Webflow / Framer Fix*: Designer settings walkthrough.
  - **"Copy AI Fix Prompt"**: Copies prompt ready to paste into Claude/Cursor/v0.
  - **Affected URLs Table**: List of all pages triggering this issue.

---

### Screen 6: Single Page Detail Drawer
Deep-dive drawer that slides out from the right:

#### Tabs:
1. **Overview**: Status, headers, content length, depth, indexability verdict.
2. **Headings Tree**: Visual outline showing H1 through H6 hierarchy with character counts.
3. **Internal Links Graph**: Table of all incoming and outgoing links with anchor text.
4. **Structured Data (JSON-LD)**: Syntax-highlighted schema inspector with validation badges.
5. **JavaScript SEO Diff**: Side-by-side comparison of Server HTML vs. Client Rendered DOM.

---

## 4. Tauri IPC Command & Event Specification

Instead of HTTP REST and Server-Sent Events, the frontend communicates with the Rust engine via **Tauri IPC (`invoke`)** and the **Tauri Event System**.

### 4.1 Commands (`src-tauri/src/commands.rs`)

```rust
#[tauri::command]
async fn start_crawl(config: CrawlConfig) -> Result<String, String>;

#[tauri::command]
async fn stop_crawl(session_id: String) -> Result<(), String>;

#[tauri::command]
async fn list_crawls() -> Result<Vec<CrawlSummary>, String>;

#[tauri::command]
async fn get_crawl_summary(session_id: String) -> Result<AuditOverview, String>;

#[tauri::command]
async fn get_pages(session_id: String, filter: PageFilter) -> Result<PageResult, String>;

#[tauri::command]
async fn get_page_details(page_id: i64) -> Result<PageDetail, String>;

#[tauri::command]
async fn get_issues(session_id: String, severity: Option<String>) -> Result<Vec<IssueGroup>, String>;

#[tauri::command]
async fn export_report(session_id: String, format: String, destination_path: String) -> Result<(), String>;
```

### 4.2 Real-Time Event Stream

During an active crawl, the Rust engine emits real-time events to the Tauri webview:

```typescript
// Frontend Listener (React 19 hook)
import { listen } from '@tauri-apps/api/event';

useEffect(() => {
  const unlistenProgress = listen<CrawlProgressPayload>('crawl-progress', (event) => {
    setProgress(event.payload);
  });
  
  const unlistenIssue = listen<IssuePayload>('crawl-issue-found', (event) => {
    addLiveIssue(event.payload);
  });

  const unlistenDone = listen<CrawlCompletedPayload>('crawl-completed', (event) => {
    onCrawlComplete(event.payload);
  });

  return () => {
    unlistenProgress.then(f => f());
    unlistenIssue.then(f => f());
    unlistenDone.then(f => f());
  };
}, [sessionId]);
```

---

## 5. Project Directory Layout & Cargo Workspace

The repository is structured as a **2-Member Cargo Workspace** (`.` for the engine/CLI and `src-tauri` for the desktop app). This shares the build cache, ensures a single `Cargo.lock`, and guarantees zero GUI bloat inside the headless CLI binary.

```
seo-lens/
├── Cargo.toml                     # Root workspace manifest & Member 1 (Engine + CLI)
├── ui/                            # React 19 Frontend (Vite + Tailwind)
│   ├── package.json
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── tailwind.config.js
│   ├── index.html
│   └── src/
│       ├── components/            # Scorecards, VirtualGrid, IssueDrawer, CMSBadges
│       ├── hooks/                 # useTauriEvents, useCrawl
│       ├── pages/                 # Overview, LiveCrawl, Explorer, Issues
│       └── App.tsx
├── src-tauri/                     # Member 2: Tauri v2 Desktop Wrapper
│   ├── Cargo.toml                 # Declares: seo-lens = { path = ".." }
│   ├── tauri.conf.json            # Window settings, bundle IDs, icons, plugins
│   ├── src/
│   │   ├── main.rs                # Tauri entry point
│   │   ├── commands.rs            # #[tauri::command] IPC bindings
│   │   └── events.rs              # Tokio -> Tauri event emitter bridge
│   └── icons/                     # Native app icons (.icns, .ico, .png)
├── tests/                         # Integration tests & test fixtures
└── src/                           # Pure Rust Engine & CLI Implementation
    ├── lib.rs                     # Re-usable core engine
    ├── main.rs                    # Headless CLI (`audit`, `mcp`, `report`)
    ├── core/
    ├── crawler/
    ├── parser/
    ├── rules/
    ├── storage/
    ├── graph/
    ├── mcp/
    └── report/
```

### Workspace Manifest Snippets:

**Root `Cargo.toml`**:
```toml
[workspace]
members = [
    ".",           # Member 1: seo-lens core library + CLI binary
    "src-tauri",   # Member 2: Tauri desktop shell
]
resolver = "2"
```

**`src-tauri/Cargo.toml`**:
```toml
[dependencies]
seo-lens = { path = ".." }
tauri = { version = "2", features = [] }
```

---

## 6. Build & Packaging Pipeline

### 6.1 Development Workflow
```bash
# In terminal:
cargo tauri dev
```
- Automatically launches Vite dev server (`http://localhost:5173`) with HMR.
- Compiles the Tauri Rust desktop wrapper.
- Spawns the native desktop window with dev tools enabled.

### 6.2 Production Release Packaging
```bash
cargo tauri build
```
Generates native platform installers in `src-tauri/target/release/bundle/`:
- **macOS**: Universal binary `.dmg` and `.app` (supports Intel + Apple Silicon).
- **Windows**: `.msi` and `.exe` installers with desktop shortcut and uninstaller.
- **Linux**: `.AppImage` and `.deb` packages.

---

## 7. Summary

Replacing the browser-based web server with **Tauri v2** gives **SEO Lens**:
1. **Zero Terminal Barrier**: Non-technical users double-click an installer to run.
2. **Zero Port Conflicts**: No `localhost` collisions with developer environments.
3. **Audience-Specific Superpowers**:
   - "Copy AI Fix Prompt" for vibe coders.
   - CMS Auto-Detection & Platform Remediation for WordPress/Webflow designers.
   - Executive Health Scorecard & 1-Click Exports for clients.
   - High-performance virtualized grid for technical developers.
4. **Lightweight Native Desktop Footprint**: 12–18MB installer with zero Chromium bloat.
