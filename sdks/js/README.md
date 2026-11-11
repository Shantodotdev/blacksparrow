# blacksparrow

Read a business website the way a brand analyst would. Give it a URL and get back the homepage and
the few pages that matter (contact, about, catalog, locations) as Markdown with their links and
contacts, the site's colours, fonts and logo candidates, and its schema.org business facts.

This package is a typed Node.js API around the prebuilt [Black Sparrow](https://github.com/Shantodotdev/blacksparrow)
engine, a Rust program. The binary ships through per-platform packages that npm installs for you.
There are no runtime dependencies.

- Node.js 20 or newer, ESM and CommonJS
- Linux (glibc and musl, x64 and arm64), macOS (x64 and arm64), Windows (x64)
- Pages that need JavaScript, or that show a bot challenge, are rendered in headless Chromium. You
  bring the Chromium; it is only started for pages that need it.

The package is not on npm yet. Until it is, build it from this folder (`npm ci && npm run build`)
and point it at a binary downloaded from the
[GitHub release](https://github.com/Shantodotdev/blacksparrow/releases) with `BLACKSPARROW_BINARY`.
Once published, `npm install blacksparrow` installs the binary for your platform as well.

## Quick start

```ts
import { extract } from "blacksparrow";

const site = await extract("https://example.com", {
  chromePath: "/usr/bin/chromium", // any Chromium or chrome-headless-shell
  maxPages: 6,
  timeoutSeconds: 90,
});

if (site.outcome !== "ok") {
  console.log(
    `Could not read the site: ${site.outcome} (${site.outcome_detail})`,
  );
} else {
  const home = site.pages[0]; // kind "home"
  console.log(home.title, home.markdown.slice(0, 200));
  console.log(
    site.contacts.emails,
    site.contacts.phones,
    site.contacts.whatsapp,
  );
  console.log(site.design?.colors.slice(0, 3).map((color) => color.hex));
  console.log(site.design?.fonts.map((font) => font.family));
  console.log(site.design?.logos[0]);
}
```

Without Chromium, read plain HTTP only:

```ts
const site = await extract("https://example.com", { render: "never" });
```

CommonJS works too:

```js
const { extract } = require("blacksparrow");
```

## API

### `extract(url, options): Promise<SiteExtract>`

Reads the site and returns the result, validated and capped (see [Safety](#safety)).

| Option                | Default  |                                                                                                                                      |
| --------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `render`              | `"auto"` | `"auto"` renders only JavaScript shells and bot challenges, `"always"` renders every HTML page, `"never"` uses plain HTTP only.      |
| `chromePath`          |          | Absolute path of a Chromium or chrome-headless-shell. Required unless `render` is `"never"`.                                         |
| `maxPages`            | `6`      | Pages read in total, homepage included (1 to 50).                                                                                    |
| `timeoutSeconds`      | `90`     | The engine's own deadline for the whole site (5 to 3600). Work that cannot finish in time is skipped and the rest is still returned. |
| `killGraceSeconds`    | `30`     | Seconds beyond `timeoutSeconds` before the process is killed and the call rejects with code `timeout`.                               |
| `pages`               |          | Pages to read after the homepage instead of choosing them by rules.                                                                  |
| `allowPrivateNetwork` | `false`  | Allows private, loopback and link-local addresses. Only for local test sites.                                                        |
| `proxy`               |          | Egress proxy for all traffic, `http://[user:pass@]host:port`. Credentials appear in the engine's command line.                       |
| `userAgent`           |          | User-Agent of plain fetches.                                                                                                         |
| `chromeNoSandbox`     | `false`  | Runs Chromium without its sandbox, for containers without user namespaces.                                                           |
| `maxOutputBytes`      | 16 MiB   | Largest document accepted from the engine.                                                                                           |
| `signal`              |          | An `AbortSignal`. Aborting kills the engine and every browser it started.                                                            |
| `binary`              |          | Path of the engine binary (see [Which binary runs](#which-binary-runs)).                                                             |
| `env`                 |          | Extra environment variables for the engine.                                                                                          |

The engine refuses private, loopback and link-local addresses for every request, also inside the
browser, unless `allowPrivateNetwork` is set. Do not set it for URLs you do not control.

### `extractRaw(url, options): Promise<{ result, raw }>`

Like `extract`, and also returns the document the engine printed, parsed (NUL characters removed),
for storing as a raw record.

### `version(): Promise<string>`

The version of the engine binary.

### `binaryPath(): string`

The absolute path of the binary that would run.

### The result

`SiteExtract` mirrors the engine's JSON document (`schema_version` 1; the JSON Schema is
[`schema/extract.schema.json`](https://github.com/Shantodotdev/blacksparrow/blob/main/schema/extract.schema.json)),
with the same snake_case names:

- `outcome`: how it ended, see below. `outcome_detail` explains a failed one.
- `pages`: each page read, homepage first. A page has `url`, `final_url`, `kind` (`home`,
  `contact`, `about`, `catalog`, `locations`, `ordering`, `requested`), `status`, `rendered`,
  `title`, `description`, `lang`, `word_count`, `markdown`, `links`, and its `phones`, `emails`,
  `whatsapp`, `facebook` and `instagram` contacts.
- `page_errors`: pages that could not be read.
- `design`: `colors` (hex, occurrences, usage score, where each was seen), `fonts` (family, the
  kinds of text it styles, weights), `logos` (image URL or inline SVG, best first), `theme_color`,
  `site_name`, and the stylesheets read. `null` when the homepage could not be read.
- `contacts`: contacts merged across pages.
- `structured`: schema.org business facts (names, telephones, emails, addresses, `sameAs` links).
- `stats`: time, fetches, bytes and renders the run cost.

### Outcomes and errors

A site that cannot be read is a normal result, not an exception:

| `outcome`         | Meaning                                                 |
| ----------------- | ------------------------------------------------------- |
| `ok`              | The homepage was read.                                  |
| `unsupported_url` | Not an http(s) address. The engine is not even started. |
| `unresolved`      | The host name does not resolve.                         |
| `blocked_address` | The host is a private or reserved address.              |
| `unreachable`     | Connection, timeout or redirect failure.                |
| `bot_protected`   | The homepage is a bot challenge, also after rendering.  |
| `http_error`      | The homepage answered with an HTTP error.               |
| `not_html`        | The homepage is not an HTML page.                       |

`BlackSparrowError` is thrown when the engine itself fails. Its `code` is one of:
`invalid_options`, `binary_not_found`, `spawn_failed`, `timeout`, `aborted` (the abort reason is
the error's `cause`), `exit` (with `exitCode`, `signal` and the end of `stderr`), `output_too_large`,
`no_output`, `invalid_output`, `unsupported_schema_version`.

```ts
import { extract, BlackSparrowError } from "blacksparrow";

try {
  const site = await extract(url, { chromePath, timeoutSeconds: 60 });
} catch (error) {
  if (error instanceof BlackSparrowError && error.code === "timeout") {
    // retry later
  } else {
    throw error;
  }
}
```

## Which binary runs

1. The `binary` option, an absolute path to an executable file.
2. The `BLACKSPARROW_BINARY` environment variable, an absolute path to an executable file.
3. The installed platform package: `blacksparrow-linux-x64-gnu`, `blacksparrow-linux-arm64-gnu`,
   `blacksparrow-linux-x64-musl`, `blacksparrow-linux-arm64-musl`, `blacksparrow-darwin-x64`,
   `blacksparrow-darwin-arm64` or `blacksparrow-win32-x64`. Linux glibc and musl are told apart at
   run time.

If none is usable, an error names the package to install. Do not install with `--omit=optional`.
For any other platform, build the engine from source and set `BLACKSPARROW_BINARY`.

The package also installs a `blacksparrow` command that runs the same binary, for example
`npx blacksparrow extract https://example.com --render never`.

## Safety

The engine reads untrusted sites and runs a browser, so the package treats both as hostile:

- The URL is checked first and passed after `--`, so it can never become an option.
- The engine runs in its own process group (on Windows its process tree is killed with
  `taskkill /T /F`). A timeout, an abort or any error kills the whole group, so the browsers it
  started do not outlive the call.
- Its environment holds only `PATH`, locale variables and a private `HOME` and `TMPDIR` in a
  temporary directory that is removed afterwards. Your secrets are not inherited; add what the
  engine needs through `env`.
- Output goes to a file whose size is checked while the engine runs, and the engine is killed when
  it passes `maxOutputBytes`.
- The document is validated leniently: a field of the wrong type is dropped, lists and text are
  capped, NUL characters are removed, only http(s) URLs are kept and `schema_version` must be 1.
  Treat the content itself (Markdown, titles, SVG logos) as untrusted input.

If your own process is killed by a signal without a handler, Node cannot clean up, and a running
engine may outlive it. Handle `SIGINT` and `SIGTERM` in services, or pass an `AbortSignal`.

## Types

All result types are exported (`SiteExtract`, `Page`, `Design`, `Color`, `Font`, `Logo`, ...), as are
`ExtractOptions`, `Outcome` and `BlackSparrowErrorCode`.

## License

MIT OR Apache-2.0
