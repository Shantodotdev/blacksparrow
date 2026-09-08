# Contributing to SEO Lens

Thank you for your interest in contributing to **SEO Lens**! We welcome bug reports, feature requests, documentation improvements, and code contributions from developers of all experience levels.

---

## 1. Code of Conduct

We are committed to providing a welcoming, inclusive, and harassment-free environment. Please be respectful and constructive in all interactions, issues, and pull requests.

---

## 2. Getting Started

### Prerequisites

1. **Rust Toolchain (1.80+)**:

   - **Linux & macOS**:

     ```bash
     curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
     source "$HOME/.cargo/env"
     ```

   - **Windows**: Download and run the official installer from [rustup.rs](https://rustup.rs/).
   - **Existing Installations**: Verify or update to the latest stable release:

     ```bash
     rustup update stable
     ```

2. **C Build Tools & OpenSSL** (Required to compile bundled SQLite and OpenSSL dependencies):

   - **Ubuntu / Debian**:

     ```bash
     sudo apt update && sudo apt install -y build-essential pkg-config libssl-dev
     ```

   - **Fedora / RHEL**:

     ```bash
     sudo dnf install -y gcc pkg-config openssl-devel
     ```

   - **macOS**:

     ```bash
     xcode-select --install
     ```

   - **Windows**: Install the **Desktop development with C++** workload via the Visual Studio Installer.

3. **Cargo Nextest (Recommended)**:
   Used for fast parallel integration testing:

   ```bash
   cargo install cargo-nextest --locked
   ```

4. **Git**

### Setting Up Your Local Repository

```bash
# 1. Fork and clone the repository
git clone https://github.com/Shantodotdev/seo-lens.git
cd seo-lens

# 2. Verify that everything builds and tests pass
cargo check
cargo nextest run    # or 'cargo test'
```

---

## 3. Development Workflow

1. **Create a branch**:

   ```bash
   git checkout -b feat/my-feature
   # or
   git checkout -b fix/issue-description
   ```

2. **Follow Test-Driven Development (TDD)**:
   - Write failing automated tests in `tests/` before writing minimal implementation code in `src/`.

3. **Format and lint before committing**:

   ```bash
   # Format all Rust files
   cargo fmt --all

   # Check for compiler and clippy warnings
   cargo check
   cargo clippy -- -D warnings

   # Run the full test suite
   cargo nextest run
   ```

4. **Commit using Conventional Commits**:
   - `feat(crawler): add support for Brotli compression`
   - `fix(parser): handle malformed self-closing tags`
   - `docs(rules): clarify remediation advice for canonical loops`
   - `test(graph): add cyclic redirect test fixture`

---

## 4. How to Add a New SEO Rule

SEO Lens has a modular rules engine. Adding a new rule takes just 4 steps:

### Step 1: Register the Rule in the Catalog (`src/rules/catalog.rs`)

Add your strongly typed `RuleId` and definition with severity, category, description, and fix advice:

```rust
// In src/rules/catalog.rs:
RuleDefinition {
    id: RuleId::WarnCustomCheck,
    category: IssueCategory::Links,
    severity: Severity::Warning,
    title: "Custom link defect detected",
    description: "Explanation of why this defect harms technical SEO.",
    fix_advice: "Actionable instructions for the developer on how to fix it.",
}
```

### Step 2: Implement the Evaluation Logic

- **In-Flight Document Rule**: Implement in `src/rules/page/` (e.g. `src/rules/page/links.rs`). It evaluates a single `ParsedPage` streamingly during crawling.
- **Site-Wide Graph Rule**: Implement in `src/rules/graph/` (e.g. `src/rules/graph/orphans.rs`). It evaluates the `petgraph` site graph post-crawl.

### Step 3: Write Integration Tests (`tests/rules_tests.rs`)

Add a synthetic HTML fixture in `tests/fixtures/` and verify that your rule triggers as expected:

```rust
#[tokio::test]
async fn test_custom_rule_detection() {
    let report = parse_and_audit("tests/fixtures/sample_page.html").await;
    assert!(report.issues.iter().any(|i| i.code == "WARN_CUSTOM_CHECK"));
}
```

### Step 4: Document the Rule in `docs/rules.md`

Add your rule code, severity, detection heuristic, and fix instructions under the relevant category in [`docs/rules.md`](./docs/rules.md).

---

## 5. Coding Standards & Conventions

- **Zero Panics in Library Code**: Never use `unwrap()` or `expect()` in `src/` library modules. Return `Result<T, SeoError>` using `thiserror`.
- **Memory Optimization**:
  - Use `compact_str::CompactString` for strings $\le 24$ bytes (URLs, tags, MIME types) to keep memory stack-inlined.
  - Use `bitflags` for boolean flags and robots directives.
  - Parse HTML streamingly with `lol_html` rather than constructing large in-memory DOMs.
- **Documentation**: All `pub` structs, traits, and functions must have concise doc comments (`///`).
- **Relative Links**: Always use relative paths when linking files in markdown documents.

---

## 6. Pull Request Guidelines

Before submitting your PR, verify:

- [ ] `cargo fmt --all` produces no diffs.
- [ ] `cargo clippy -- -D warnings` reports zero warnings.
- [ ] `cargo nextest run` (or `cargo test`) passes completely.
- [ ] Any new public APIs or CLI flags are documented in `docs/`.
