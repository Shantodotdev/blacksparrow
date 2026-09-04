---
name: commit-message-generator
description: Group current file changes into granular commit snapshots and draft repo-style commit messages for this codebase. Use when the user wants commit suggestions, asks how to split a dirty worktree, wants messages that match recent SEO Lens history, or needs commit text plus the exact files to include in each snapshot.
---

# Commit Message Generator

## Overview

Use this skill to turn the current worktree into clean, reviewable commit snapshots.

In this repository, the expected output is not just a commit subject. The useful deliverable is:

1. A granular grouping of changed files into logical snapshots.
2. One commit message per snapshot.
3. A fenced code block that contains only the commit message.
4. A file list outside the code block.

This repo's `AGENTS.md` and engineering standards reinforce the baseline:

- Group changes by logical unit, not by folder alone.
- Use conventional commit types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`.
- Keep the type and scope lowercase.
- Keep the subject after the colon lowercase.
- When a body is useful, write one or more dashed bullets (`- `).
- Body bullets read like sentence-style implementation notes and start with a capital letter after `- `.

---

## Workflow

Follow this sequence every time:

### 1. Read Local Rules First

Read root `AGENTS.md` before drafting commit suggestions.

For this repo, the important local rules are:

- Group changed files into logical snapshots.
- Produce one commit message per snapshot.
- Do not place file paths inside the commit subject.
- Use read-only git inspection only.

### 2. Inspect the Current Worktree

Use read-only inspection to understand the pending work:

- `git status --short`
- `git diff --name-only`
- `git diff --stat`
- `git diff -- <path>`
- `git log --pretty=format:"%s%n%b" -n 5` when style confirmation is useful

Do not guess from filenames alone if the grouping is unclear. Read enough diff context to understand whether a file belongs to a feature, fix, refactor, test, or docs.

### 3. Split Changes into Granular Snapshots

A good snapshot should answer one clear question:

- _What single purpose would this commit represent if reviewed on its own?_

Prefer splitting when:

- `docs/*` changed alongside runtime code.
- Test fixtures or unit tests in `tests/*` were introduced for a specific component.
- Core engine changes (`src/core/`, `src/crawler/`) are mixed with CLI (`src/main.rs`) or Desktop (`src-tauri/`) wiring.
- Unrelated bug fixes were made during the same session.

Avoid splitting when:

- The files are tightly coupled and would not compile or pass tests independently.
- The separation would create broken intermediate snapshots (e.g. model change without its parser implementation).

### 4. Pick the Commit Type by Intent

Choose the type by the dominant purpose of the snapshot:

- `feat`: new capability (e.g., new SEO rule, new crawler feature, new MCP tool).
- `fix`: bug correction or broken behavior repair.
- `refactor`: behavior-preserving restructuring, cleanup, or internal simplification.
- `perf`: performance optimization (e.g., SimHash acceleration, memory layout tuning).
- `test`: test-only work or synthetic fixture additions.
- `docs`: documentation-only change (e.g. in `docs/` or `README.md`).
- `chore`: maintenance, workspace configuration, or dependency updates.

### 5. Pick a Scope Matching Repo Structure

Scopes in this repo are lowercase and module-oriented:

- `core`: domain models, URL normalization, config
- `crawler`: async HTTP client, AIMD politeness, frontier, browser CDP, diff engine
- `parser`: `lol_html` streaming tokenizer, metadata, schema, content extraction
- `rules`: technical SEO checks (single-page, graph, JS diff)
- `graph`: petgraph topology, internal PageRank
- `storage`: SQLite WAL persistence and queries
- `mcp`: Model Context Protocol server, tools, and resources
- `report`: terminal UI, Markdown, JSON, CSV exporters
- `ui`: React 19 frontend components, virtual grid, and styles
- `desktop`: Tauri v2 wrapper, IPC commands, events, native plugins
- `tests`: test fixtures, integration test suites
- `workspace`: Cargo workspace configuration, CI/CD, root manifests

### 6. Write the Subject in Repo Style

Format:

```text
type(scope): lowercase action-oriented subject
```

Guidelines:

- Start with an action verb (`add`, `update`, `implement`, `fix`, `refactor`, `optimize`, `standardize`).
- Keep it concise and specific.
- Describe the actual change, not the coding activity.
- Avoid file paths in the subject.
- Never use a trailing period in the subject line.

### 7. Add a Body When It Adds Value

Add a body when the subject alone hides important implementation details:

- Use dashed bullets (`- `).
- Keep each bullet concrete, implementation-aware, and capitalized.
- Bullets must stay inside the fenced commit block.

---

## Output Format

Always present snapshots in this exact format:

Snapshot 1: `<short human label>`

Files:

- `path/to/file-a`
- `path/to/file-b`

Commit message:

```text
type(scope): lowercase subject

- Capitalized body detail explaining what changed.
- Another capitalized bullet if needed.
```

**Important Rules:**

- File locations must stay outside the fenced code block.
- The fenced block must contain only the commit message text.
- Do not mix commentary or explanations inside the fenced code block.

---

## Examples

### Example 1: New SEO Rule with Test Fixture

Snapshot 1: canonical 404 detection rule

Files:

- `src/rules/page/canonical.rs`
- `tests/fixtures/canonical_404.html`
- `tests/rules_tests.rs`

Commit message:

```text
feat(rules): implement ERR_CANONICAL_TO_4XX_5XX rule

- Added check for canonical link tags pointing to client or server error endpoints.
- Added synthetic test fixture and regression test suite.
```

### Example 2: Crawler AIMD Throttling

Snapshot 1: adaptive politeness controller

Files:

- `src/crawler/aimd.rs`
- `src/crawler/client.rs`

Commit message:

```text
feat(crawler): implement AIMD adaptive congestion controller

- Added additive delay backoff upon detecting HTTP 429 or 503 response codes.
- Added multiplicative decay when origin latency stabilizes below threshold.
```

---

## Anti-Patterns

- One giant commit snapshot that mixes features, tests, and documentation.
- Vague subjects like `misc fixes`, `update code`, or `wip`.
- Putting file paths in the commit subject.
- Putting file lists inside the fenced code block.
- Using uppercase subjects after the colon.
