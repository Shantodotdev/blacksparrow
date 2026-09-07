# AGENTS.md

Project-wide instructions for AI coding agents working in SEO Lens.

## Repository Overview

`SEO Lens` is a high-performance, local-first website crawler, technical SEO audit engine, and AI-native auditor written in Rust. Its primary CLI executable is `seolens`.

The repository is structured as a **2-Member Cargo Workspace**:

- **Member 1 (Root `.`)**: Core engine library (`src/lib.rs`) and headless CLI binary (`src/main.rs`).
- **Member 2 (`src-tauri`)**: Native cross-platform desktop application shell (Tauri v2).
- **Frontend (`ui/`)**: React 19 + TypeScript + Tailwind CSS desktop UI with `@tanstack/react-virtual`.
- **Tests (`tests/`)**: Integration test suites and synthetic HTML fixtures (`tests/fixtures/`).
- **Documentation (`docs/`)**: 7 permanent technical specifications serving as the binding source of truth.

### Core Modules (`src/`)

- `src/core/`: Domain models (`PageReport`, `IssueFinding`), URL normalization pipeline, and crawl configurations.
- `src/crawler/`: Asynchronous HTTP client, AIMD adaptive congestion controller, URL frontier queue, and decoupled Chrome CDP browser engine.
- `src/parser/`: Streaming HTML tokenizer (`lol_html`), metadata extractor, content reader, and schema validator.
- `src/rules/`: 120 technical SEO audit checks (in-flight single-page, post-crawl graph, and JS SEO diffing).
- `src/graph/`: Directed link topology graph (`petgraph`) and internal PageRank computation.
- `src/storage/`: SQLite WAL-mode persistence layer (`rusqlite`).
- `src/mcp/`: Native Model Context Protocol (MCP) server over `stdio` for AI agent integration.
- `src/report/`: Exporters (Markdown for LLMs, JSON, Screaming Frog CSVs, and ANSI terminal UI).

---

## Working Principles

- **Strict Test-Driven Development (TDD)**: Always write failing automated tests in `tests/` before writing minimal implementation code in `src/`.
- **Architectural Progression**: Follow the architecture and roadmap defined in [`docs/ARCHITECTURE.md`](file:///mnt/Code/PROJECTS/seo-lens/docs/ARCHITECTURE.md). Complete and verify each component before moving to the next.
- **Zero Panics in Library Code**: Never use `unwrap()` or `expect()` in `src/` library modules. All fallible operations must return `Result<T, SeoError>` using `thiserror`.
- **Minimal, Targeted Changes**: Make focused edits. Preserve existing comments, docstrings, and unrelated code.
- **Empirical Measurement**: Do not make up unverified performance claims or benchmark figures. Profile and measure memory and speed empirically.
- **Preserve Long-Term Assets**: Never delete, truncate, or overwrite files in `docs/` or `inspiration/`.
- **Dependency Management**: Agents are authorized to modify and add dependencies in `Cargo.toml` or `package.json` as needed for feature and phase implementations.

---

## Do Not Do Without Explicit User Approval

- Do not run `git commit` automatically unless the user explicitly requested a commit in their prompt.
- Do not run destructive Git commands (`git reset --hard`, `git push --force`, `git clean -f`).
- Do not bundle Chromium into the binary (use decoupled CDP via `chromiumoxide`).
- Do not write implementation code before writing failing tests and test fixtures.
- Do not advance to a new micro-phase without collaborative user sign-off.

### Verification Protocol

- **Mandatory Formatting**: After making any code changes and before handing over to the user, **always run `cargo fmt --all`**.
- **Test Runner Preference**: If `cargo-nextest` is installed on the system (check via `cargo nextest --version`), always prioritize using `cargo nextest run` (or `cargo nextest run --workspace`) for running unit and integration tests because it is faster and provides superior UI output. Fall back to standard `cargo test` if `nextest` is unavailable. Note that doc tests are executed with `cargo test --doc`.
- **Minor / Trivial Tasks**: Run `cargo fmt --all`, `cargo check`, and `cargo clippy`.
- **Phase Work & Major Features**: Run `cargo fmt --all`, run the test suite (preferring `cargo nextest run` if installed, otherwise `cargo test`), and verify changes against [`docs/ARCHITECTURE.md`](file:///mnt/Code/PROJECTS/seo-lens/docs/ARCHITECTURE.md).
- **Super Simple Edits** (e.g. typos, comments): Run `cargo fmt --all`.

---

## Coding Rules & Standards

- **Idiomatic Rust 2021**: Follow standard Rust conventions. All public items must have concise doc comments.
- **Memory Optimization**:
  - Use `compact_str::CompactString` for strings $\le 24$ bytes (URLs, MIME types, tag names).
  - Use `bitflags` for boolean flags and robots directives (`RobotsFlags`).
  - Use streaming parsing (`lol_html`) instead of building in-memory DOM trees.
- **Concurrency**: Use Tokio green tasks, `tokio::sync::mpsc` channels, and SwissTable (`hashbrown`) for fast frontier URL deduplication.
- **Cross-Platform**: Code must build and pass tests cleanly on Linux, macOS, and Windows.
- **Inline Comments**: Include inline comments only for non-obvious algorithms, mathematical formulas (e.g., AIMD, PageRank), or RFC compliance rules—do not narrate obvious code.

---

## Project Documentation (Binding References)

Read the relevant specification in `docs/` before implementing or changing any component:

1. **Roadmap & Architecture**: [`docs/ARCHITECTURE.md`](file:///mnt/Code/PROJECTS/seo-lens/docs/ARCHITECTURE.md)
2. **120 SEO Rules Catalog (Heuristics & Fixes)**: [`docs/SEO_RULES_CATALOG.md`](file:///mnt/Code/PROJECTS/seo-lens/docs/SEO_RULES_CATALOG.md)
3. **Domain Models & SQLite WAL Schema**: [`docs/DATA_MODELS_AND_SCHEMA.md`](file:///mnt/Code/PROJECTS/seo-lens/docs/DATA_MODELS_AND_SCHEMA.md)
4. **Native Desktop Application (Tauri v2 + React 19)**: [`docs/DESKTOP_APP_SPEC.md`](file:///mnt/Code/PROJECTS/seo-lens/docs/DESKTOP_APP_SPEC.md)
5. **Model Context Protocol (MCP) Server**: [`docs/MCP_SPECIFICATION.md`](file:///mnt/Code/PROJECTS/seo-lens/docs/MCP_SPECIFICATION.md)
6. **CLI Commands, Flags & Exporters**: [`docs/CLI_AND_REPORTS_SPEC.md`](file:///mnt/Code/PROJECTS/seo-lens/docs/CLI_AND_REPORTS_SPEC.md)
7. **Crawler Architecture & Congestion Control**: [`docs/CRAWLER_SPEC.md`](file:///mnt/Code/PROJECTS/seo-lens/docs/CRAWLER_SPEC.md)

---

## Commit Message Requests

When asked for commit message suggestions, follow the `commit-message-generator` skill:

- Antigravity Agents: `.agents/skills/commit-message-generator/SKILL.md`
- Codex Agents: `.codex/skills/commit-message-generator/SKILL.md`

Use read-only inspection (`git status --short`, `git diff --name-only`), group changes into granular reviewable snapshots, place file paths outside fenced code blocks, and format commit messages in conventional lowercase format (e.g. `feat(crawler): ...`, `test(rules): ...`).
