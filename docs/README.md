# SEO Lens Documentation Hub

Welcome to the technical documentation for **SEO Lens**—a high-performance, local-first website crawler, 120-rule technical SEO audit engine, and AI-native auditor written in Rust.

Whether you are auditing a client website, connecting an AI coding agent via MCP, or contributing new features to the core engine, the guides below cover every aspect of the system.

---

## Documentation Guides

| Guide | Description | Target Audience |
| --- | --- | --- |
| [**Architecture & Roadmap**](./architecture.md) | 2-member workspace layout, asynchronous pipeline design, streaming parser (`lol_html`), and milestone progress. | Contributors, Systems Engineers |
| [**CLI Commands & Flags**](./cli.md) | Reference for all 10 CLI subcommands (`audit`, `inspect`, `mcp`, `report`, `issues`, etc.), flags, and exporters. | Users, DevOps, Automation |
| [**Crawler Engine & AIMD**](./crawler.md) | Asynchronous crawler mechanics, AIMD rate tuning, 8-stage URL normalization, RFC 9309 robots, and streaming XML sitemaps. | Contributors, Network Engineers |
| [**Model Context Protocol (MCP)**](./mcp.md) | Pure Rust stdio MCP server for AI coding agents (Claude, Cursor, Windsurf) with 8 tools and one-click setup prompt. | AI Engineers, Agent Developers |
| [**120 SEO Rules Catalog**](./rules.md) | Complete dictionary of all 120 technical SEO checks across 13 categories, detection heuristics, and remediation guidance. | SEO Specialists, Web Developers |
| [**Storage & SQLite Schema**](./storage.md) | Local-first persistence layer, asynchronous batch writer actor, 7 relational tables, and power-user SQL query cheatsheet. | Database Admins, Power Users |

---

## Recommended Reading Pathways

### "I want to audit a website from the command line"

1. Read [CLI Commands & Flags](./cli.md) for usage examples and output options.
2. Review [120 SEO Rules Catalog](./rules.md) to interpret issue codes and remediation advice.

### "I want my AI assistant (Claude, Cursor, Windsurf) to audit websites"

1. Head to [Model Context Protocol (MCP)](./mcp.md).
2. Copy the prompt to instruct your agent to register `seolens mcp`.

### "I want to contribute code or add a new SEO rule"

1. Read [Architecture & Roadmap](./architecture.md) to understand module boundaries and data flow.
2. Read [CONTRIBUTING.md](../CONTRIBUTING.md) for workflow, testing (`cargo nextest`), and coding standards.
3. Check [120 SEO Rules Catalog](./rules.md) to see where your new rule fits into the catalog.

### "I want to inspect or extract audit data directly"

1. Open [Storage & SQLite Schema](./storage.md) to see table definitions.
2. Query `seolens.db` using the pre-built SQL examples or export to CSV with `seolens report <SESSION_ID> -f csv`.

---

## Documentation Standards

- All documentation in this folder uses lowercase filenames and relative links.
- Specifications reflect the current implementation in `src/` and are maintained as living documentation.
