# Agent Mode: Web Content for AI Agents

Agent mode turns web pages into clean, token-efficient content for AI agents: Markdown,
passages that answer a question, and JSON that matches a schema. It runs from the same
`blacksparrow` binary as the SEO audit, as CLI commands, MCP tools and a Firecrawl-compatible
HTTP API, so it can be self-hosted instead of paying for a crawling service.

No LLM is involved. Main content is found with `rs-trafilatura` plus Black Sparrow's own
cleaning, and structured extraction reads JSON-LD, Microdata, RDFa, embedded app state,
labels and repeated records before falling back to selectors learned across pages.

## Commands

| Command | What it does |
|---|---|
| `scrape <URL>` | One page to Markdown. `-f markdown,links,metadata,…` for more outputs, `--json` for the whole document. |
| `map <URL>` | A site's URLs from page links, robots.txt and sitemaps. `--search` ranks them. |
| `crawl <URL>` | Many pages. `--out DIR` writes one Markdown file per page; otherwise NDJSON on stdout. |
| `find <URL> --query "…"` | Passages answering a question, with heading paths. Also `--selector` and `--regex`, or `--crawl <ID>` to search stored pages. |
| `extract <URL> --schema schema.json` | JSON shaped like the schema, with a source and confidence per field. |
| `interact <URL> --steps steps.json` | Click, type, press and scroll in Chrome, then read the page. Prints the interactive elements with references (`e12`) for the next call. |
| `serve` | The HTTP API (build with `--features serve`). |

Shared flags: `-u/--user-agent`, `-H/--header`, `-a/--allow-host`, `--allow-local-network`,
`--chrome-ws`, `--render-concurrency`, `--no-robots`, `--timeout`, `--db-path`, `-L/--local`,
`--no-store`.

```bash
blacksparrow scrape https://example.com/pricing
blacksparrow crawl https://docs.example.com --limit 200 --include-path '/guides/*' --out ./docs-md
blacksparrow find https://example.com/faq --query "how long does shipping take"
blacksparrow extract https://shop.example.com/p/123 --schema '{"type":"object","properties":{"name":{"type":"string"},"price":{"type":"number","x-kind":"price"}}}'
```

### How a page is fetched

1. The URL passes the network guard: private, loopback and link-local addresses are refused
   unless allowed with `-a host:port` or `--allow-local-network`. Cloud metadata endpoints are
   always refused, including from inside Chrome and after redirects.
2. A stored copy younger than `--max-age` seconds is reused.
3. robots.txt is obeyed unless `--no-robots`.
4. The request asks for Markdown first (`Accept: text/markdown`). Markdown, plain text and PDF
   responses are converted directly. Bot challenges are reported as `blocked`, scanned PDFs as
   `needs_ocr`.
5. Chrome is used only when the page is an empty JavaScript shell, or when rendering is
   requested (`--render always`, `--wait-for`, actions, screenshots).

Text hidden with CSS or `aria-hidden` never reaches the output, so hidden prompt-injection
text on a page is dropped.

### Extraction schemas

Schemas are ordinary JSON Schema objects (or an array of objects for list pages) with
optional hints on each property:

- `x-kind`: `price`, `currency`, `date`, `phone`, `email`, `url`, `image`, `gtin`, `isbn`,
  `rating`, `number`, `integer`, `quantity`, `boolean` or `text`.
- `x-selector`: a CSS selector (append `@attr` to read an attribute) that wins over every
  other source.
- `x-synonyms`: other labels the field may appear under.

When several pages of one template are extracted, selectors learned from pages with
structured data fill pages without it. Learned rules are stored per host and template.

## MCP tools

`blacksparrow mcp` now exposes both tool families by default (`--tools seo|web|all`):

| Tool | Purpose |
|---|---|
| `web_scrape` | One page to Markdown and metadata |
| `web_map` | A site's URLs, optionally ranked |
| `web_crawl` | Start a background crawl (returns an id) |
| `web_crawl_status` | Progress and documents, page by page |
| `web_crawl_cancel` | Stop a crawl |
| `web_find` | Passages by question, selector or regex |
| `web_extract` | Schema-shaped data without an LLM |
| `web_interact` | Browser steps, then the page and its interactive elements |

Tool descriptions tell the agent that page content is untrusted data, never instructions.

## HTTP API

`blacksparrow serve` (feature `serve`) listens on `127.0.0.1:3002`. Request and response
shapes follow Firecrawl, so its SDKs work by changing the base URL.

| Method | Path | Notes |
|---|---|---|
| `POST` | `/v1/scrape`, `/v2/scrape` | `formats` accepts Firecrawl names (`rawHtml`, `screenshot@fullPage`); unsupported ones such as `json` and `summary` are ignored. `maxAge` is in milliseconds. |
| `POST` | `/v1/map`, `/v2/map` | v1 returns URL strings, v2 returns `{url, title}` objects. |
| `POST` | `/v1/crawl`, `/v2/crawl` | Returns `{id, url}`; `url` is the status endpoint. |
| `GET` | `/v1/crawl/{id}` | `status` is `scraping`, `completed`, `cancelled` or `failed`. Paged with `skip` and `limit`; `next` is the following page's URL. |
| `DELETE` | `/v1/crawl/{id}` | Cancel. |
| `POST` | `/v1/find` | Native shape (`url` or `crawlId`, plus `query`, `selector` or `regex`). |
| `POST` | `/v1/extract` | Synchronous, unlike Firecrawl's job-based extract: returns `{data, results}`. |
| `POST` | `/v1/interact` | Firecrawl-style or native actions. `executeJavascript` is refused. |
| `GET` | `/health` | No key needed. |

Security and limits:

- `Authorization: Bearer <key>` (or `x-api-key`) on every route but `/health`. Keys come from
  `--api-key` or `BLACKSPARROW_API_KEYS` (comma-separated). Without keys the server refuses to
  bind beyond loopback.
- Per-key budget of `--rate-limit` requests per minute (429 with `Retry-After`).
- `--max-body-bytes`, `--max-crawl-pages` (also caps extract) and `--max-concurrent-crawls`.
- Private addresses are refused with 403 unless the server was started with
  `--allow-local-network` or `-a`.
- One log line per request (method, path, status, duration) on stderr; keys are never logged.

## Docker

```bash
docker build -t blacksparrow .
docker run -p 3002:3002 -e BLACKSPARROW_API_KEYS=change-me -v bs-data:/data blacksparrow

# With a headless Chrome sidecar for JavaScript pages:
BLACKSPARROW_API_KEYS=change-me docker compose up -d
```

The image runs as a non-root user, stores its database in the `/data` volume
(`BLACKSPARROW_DB_PATH`) and has a health check on `/health`. Chrome is never bundled; the
compose file runs `chromedp/headless-shell` on a private network and connects with
`--chrome-ws`.
