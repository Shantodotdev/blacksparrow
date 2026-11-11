# Changelog

All notable changes to Black Sparrow are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/). The crate, the Python package and the npm packages share one version number.

## [0.1.0] - 2026-11-12

First stable release.

### Added

- `blacksparrow extract <url>`: reads a business website (English and Bangla) into one JSON document with Markdown pages, contacts, social links, colours with evidence, fonts with roles, logo candidates, schema.org business facts and the cost of the run. Headless Chromium renders only JavaScript shells and bot-challenge pages, through an address-checking proxy; Chromium itself is not bundled (`--chrome-path`, `CHROME_PATH`).
- `blacksparrow schema extract`: prints the JSON Schema (draft 2020-12) of the `extract` output document (`schema_version` 1). The committed copy is `schema/extract.schema.json`; CI fails when it drifts from the Rust types, and tests validate real `extract` output against it.
- `audit --render-js`: crawls links that only exist after scripts run, using headless Chromium.
- Python package `blacksparrow` (wheels with the binary inside, typed API), attached to the GitHub release, and npm package `blacksparrow` (platform binaries as optional dependencies, typed API), not published yet. Both honour `BLACKSPARROW_BINARY`.
- Release archives for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin` and `x86_64-pc-windows-msvc`, each with a SHA-256 file and a GitHub build attestation, plus shell, PowerShell and MSI installers.
- SSRF protection for crawling and extraction: private, loopback and link-local addresses are refused unless explicitly allowed, redirects are re-checked at every hop, and DNS answers are checked at connect time.
- 15 MB streaming limit for HTML and a 50 MB ceiling for decompressed sitemaps.

### Changed

- TLS uses rustls instead of OpenSSL, so the binary has no system library dependencies and the musl builds need none.
- `blacksparrow schema` keeps validating structured data (`blacksparrow schema <url-or-file>`); `schema extract` is the new subcommand beside it.

### Fixed

- The integration test for blocked browser requests asserted on image requests, which Chromium never makes with images disabled; it now checks the resource types the request interceptor actually refuses (fonts, media, beacons).

## [0.1.0-rc.2] and [0.1.0-rc.1]

Release candidates of the SEO audit engine: crawler with adaptive rate limiting, 120 technical SEO rules, link graph and PageRank, SQLite storage, MCP server, and HTML, CSV, Markdown and JSON exports.

[0.1.0]: https://github.com/Shantodotdev/blacksparrow/releases/tag/v0.1.0
[0.1.0-rc.2]: https://github.com/Shantodotdev/blacksparrow/releases/tag/v0.1.0-rc.2
[0.1.0-rc.1]: https://github.com/Shantodotdev/blacksparrow/releases/tag/v0.1.0-rc.1
