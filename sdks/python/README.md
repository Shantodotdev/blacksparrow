# blacksparrow

Read a business website into one typed document, from Python.

`blacksparrow` wraps [Black Sparrow](https://github.com/Shantodotdev/blacksparrow), a fast Rust
crawler. Its `extract` command reads the homepage and the few pages that matter (contact, about,
catalog, locations), renders a page in headless Chromium only when it is a JavaScript shell or a
bot challenge, and returns:

- each page as Markdown, with its links, phone numbers, emails, WhatsApp, Facebook and Instagram links
- the site's colours (with usage and evidence), fonts, and logo candidates
- schema.org business facts from JSON-LD
- what the run cost

The Black Sparrow binary ships **inside the wheel**, like `ruff`: nothing is built, and nothing is
downloaded at run time. The wheels are attached to each
[GitHub release](https://github.com/Shantodotdev/blacksparrow/releases) (they are not on PyPI yet).
Install the one for your platform by its URL:

```bash
pip install "blacksparrow @ https://github.com/Shantodotdev/blacksparrow/releases/download/v0.1.0/<wheel file name>"
```

In a uv project, list one URL per platform with a marker, so `uv.lock` pins each wheel with its hash:

```toml
[project]
dependencies = ["blacksparrow==0.1.0"]

[tool.uv.sources]
blacksparrow = [
  { url = "https://github.com/.../blacksparrow-0.1.0-py3-none-manylinux_2_17_x86_64.manylinux2014_x86_64.whl", marker = "sys_platform == 'linux' and platform_machine == 'x86_64'" },
  { url = "https://github.com/.../blacksparrow-0.1.0-py3-none-manylinux_2_17_aarch64.manylinux2014_aarch64.whl", marker = "sys_platform == 'linux' and platform_machine == 'aarch64'" },
]
```

Copy the exact file names from the release page. Wheels exist for Linux (glibc and musl, x86_64 and aarch64), macOS (Intel and Apple silicon)
and Windows (x64). Python 3.10 or newer. The only dependency is `pydantic>=2`.

## Quick start

```python
import blacksparrow

site = blacksparrow.extract(
    "https://example.com",
    chrome_path="/usr/bin/chromium",  # used only for pages that need JavaScript
    max_pages=6,  # homepage included
    timeout_seconds=90,
)

if site.ok:
    home = site.homepage
    print(home.title, home.word_count)
    print(home.markdown[:500])
    print(site.contacts.emails, site.contacts.phones)
    print([c.hex for c in site.design.colors[:3]])
    print([f.family for f in site.design.fonts])
else:
    # A site that cannot be read is a result, not an exception.
    print(site.outcome, site.outcome_detail)
```

Without a browser, pass `render="never"` and `chrome_path=None`. Plain HTTP is enough for most sites.

### Arguments

| Argument                | Meaning                                                                                                                                                        |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `url`                   | The website. Anything that is not an absolute `http(s)` address returns outcome `unsupported_url` without running anything.                                    |
| `chrome_path`           | Absolute path of a Chromium or chrome-headless-shell executable. **Required** unless `render="never"` (then pass `None`).                                      |
| `render`                | `"auto"` (default: only JavaScript shells and bot challenges), `"always"` or `"never"`.                                                                        |
| `max_pages`             | Pages read in total, homepage included (1 to 100). **Required.**                                                                                               |
| `timeout_seconds`       | The binary's deadline for the whole site (at least 5). **Required.** The process is killed 30 seconds after it if it still runs.                               |
| `allow_private_network` | Allow loopback and private addresses. Off by default: they are refused for every request, also inside the browser. Turn it on only for sites you run yourself. |
| `proxy`                 | `http://[user:pass@]host:port` egress proxy for all traffic. Credentials in the URL are visible to local users in the process list.                            |
| `user_agent`            | User-Agent for plain fetches.                                                                                                                                  |
| `chrome_no_sandbox`     | Run Chromium without its sandbox. Needed in containers without user namespaces.                                                                                |
| `pages`                 | Pages to read after the homepage instead of choosing them by rules.                                                                                            |
| `env`                   | Extra environment variables for the process (see below).                                                                                                       |
| `max_output_bytes`      | Largest document accepted (default 16 MiB).                                                                                                                    |
| `cancel`                | A `threading.Event`; set it from another thread to stop the reading.                                                                                           |
| `binary`                | Absolute path of the binary to run instead of the bundled one.                                                                                                 |

## Async

```python
site = await blacksparrow.extract_async(
    "https://example.com", chrome_path=None, render="never", max_pages=6, timeout_seconds=60
)
```

The process runs under asyncio's subprocess support, so the event loop is never blocked. Cancelling
the task kills the process group (and so the browsers) before `CancelledError` propagates.

## The raw document

Applications that keep their own record of a run store the document as the binary printed it:

```python
site, raw = blacksparrow.extract_raw(
    "https://example.com", chrome_path=None, render="never", max_pages=6, timeout_seconds=60
)
```

`raw` is the decoded JSON (NUL characters removed). It carries everything the models drop, such as the
fields of a newer binary. `extract_raw_async` is the async twin.

## Results

`SiteExtract` and its parts (`Page`, `PageLink`, `PageError`, `Design`, `Color`, `ColorEvidence`,
`Font`, `Logo`, `Stylesheet`, `Contacts`, `Structured`, `Stats`) are frozen pydantic models.

The document is **untrusted input**: the website controls every string in it. The models validate
leniently. A value of the wrong type becomes absent (`None`, `[]`, `0`, `""`), lists lose their invalid
items and are capped, texts are trimmed and capped, URLs that are not absolute `http(s)` URLs are
dropped, and unknown fields are ignored. Logo `svg` markup is passed through as text: sanitise it
before you serve it to a browser.

`site.outcome` is one of `ok`, `unsupported_url`, `unresolved`, `blocked_address`, `unreachable`,
`bot_protected`, `http_error`, `not_html`. `site.homepage` is the page of kind `"home"`, or `None`.
Anything but `ok` means the homepage was not read; a page may still be present, for example the error
page of an `http_error`.

The JSON Schema of the document (`schema_version` 1) is printed by `blacksparrow schema extract`.

## Errors

| Error                     | When                                                                                                                                   |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `BlackSparrowError`       | The binary could not start, crashed, printed too much or no usable document, or printed a `schema_version` this package does not know. |
| `BinaryNotFoundError`     | No usable binary (subclass of `BlackSparrowError`).                                                                                    |
| `BlackSparrowTimeout`     | The process outlived its deadline (subclass).                                                                                          |
| `BlackSparrowCancelled`   | The `cancel` event was set (subclass).                                                                                                 |
| `TypeError`, `ValueError` | An argument is wrong; nothing was run.                                                                                                 |

## How the process is run

Reading a website means running a program on input chosen by a stranger, so it is contained:

- It leads its own process group (session on POSIX, process group on Windows). On a timeout, a cancel,
  an exceeded limit or any error the whole group is killed, so Chromium and its helpers die with it.
- Its environment is minimal: `PATH`, the locale variables (and the few variables Windows needs to start
  a program). `HOME` and the temp directories point at a private, per-run temporary directory that is
  removed afterwards. Secrets in your environment are not inherited. Pass what the binary needs through
  `env=`, for example `{"SSL_CERT_FILE": "/etc/ssl/certs/ca.pem"}`.
- Its document goes to a file in that directory. The size is checked while it runs, and the process is
  stopped when the file or its standard error grows past a limit.
- The address is passed after `--`, and every option value as `--option=value`, so nothing a caller
  provides can become another option.

## Finding the binary

In this order, never searching `PATH`:

1. the `binary=` argument,
2. the `BLACKSPARROW_BINARY` environment variable,
3. the binary installed by the wheel (in the environment's scripts directory, found the way `ruff`
   finds its own).

The first two must be absolute paths to executable files. `blacksparrow.binary_path()` returns the
path that will run, and `blacksparrow.version()` its version. `python -m blacksparrow ...` runs the
binary with your arguments, for the full command line (`python -m blacksparrow extract --help`).

## License

MIT OR Apache-2.0, at your option.
