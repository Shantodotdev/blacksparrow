// Helpers shared by the test files: the engine binary, fixture sites over HTTP and fake engines.
import { createServer } from "node:http";
import { chmodSync, existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, extname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

export const packageRoot = join(here, "..");
export const repositoryRoot = join(packageRoot, "../..");
export const sitesRoot = join(repositoryRoot, "tests/fixtures/sites");

/** The real engine binary, or undefined when none is configured. */
export function realBinary() {
  const configured = process.env.BLACKSPARROW_BINARY;
  if (configured) return configured;
  const debug = join(repositoryRoot, "target/debug", process.platform === "win32" ? "blacksparrow.exe" : "blacksparrow");
  return existsSync(debug) ? debug : undefined;
}

/** A Chromium for the render tests, only when BLACKSPARROW_TEST_CHROME names an executable. */
export function testChrome() {
  const chrome = process.env.BLACKSPARROW_TEST_CHROME;
  return chrome && existsSync(chrome) ? chrome : undefined;
}

const contentTypes = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css",
  ".js": "application/javascript",
};

/**
 * Serves tests/fixtures/sites/<site> on a loopback port. `index.html` is also served at `/`.
 * `overrides` maps a URL path to { status, headers, file } to change one response.
 */
export async function serveSite(site, overrides = {}) {
  const root = join(sitesRoot, site);
  const requests = [];
  const server = createServer((request, response) => {
    const path = new URL(request.url ?? "/", "http://localhost").pathname;
    requests.push(path);
    const override = overrides[path];
    let relative = path === "/" ? "index.html" : path.slice(1);
    if (override?.file) relative = override.file;
    const file = join(root, relative);
    if (!file.startsWith(root) || !existsSync(file) || !statSync(file).isFile()) {
      response.writeHead(404, { "content-type": "text/plain" }).end("not found");
      return;
    }
    response
      .writeHead(override?.status ?? 200, {
        "content-type": contentTypes[extname(file)] ?? "application/octet-stream",
        ...override?.headers,
      })
      .end(readFileSync(file));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  return {
    url: `http://127.0.0.1:${port}`,
    requests,
    close: () =>
      new Promise((resolve) => {
        server.closeAllConnections();
        server.close(resolve);
      }),
  };
}

/** Serves a connection that is accepted and never answered. */
export async function serveSilence() {
  const server = createServer(() => {});
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  return {
    url: `http://127.0.0.1:${port}`,
    close: () =>
      new Promise((resolve) => {
        server.closeAllConnections();
        server.close(resolve);
      }),
  };
}

export const VALID_DOCUMENT = JSON.stringify({
  schema_version: 1,
  generator: "blacksparrow fake",
  start_url: "http://127.0.0.1/",
  final_url: "http://127.0.0.1/",
  outcome: "ok",
  pages: [],
  page_errors: [],
  design: null,
  contacts: { phones: [], emails: [], whatsapp: [], facebook: [], instagram: [] },
  structured: { names: [], telephones: [], emails: [], addresses: [], same_as: [], logos: [], descriptions: [] },
  stats: { elapsed_ms: 1, http_fetches: 0, http_bytes: 0, rendered_pages: 0, render_ms: 0, blocked_browser_requests: 0 },
});

/**
 * A POSIX shell script that stands in for the engine. FAKE_MODE (set through the `env` option)
 * picks the behaviour; FAKE_LOG is a file it writes facts to, FAKE_PIDS a file for process ids.
 */
export function fakeEngine() {
  const directory = mkdtempSync(join(tmpdir(), "bs-fake-"));
  const script = join(directory, "blacksparrow");
  writeFileSync(
    script,
    `#!/bin/sh
out=""
for arg in "$@"; do
  case "$arg" in --output=*) out="\${arg#--output=}" ;; esac
done
if [ "$1" = "--version" ]; then echo "blacksparrow 9.9.9"; exit 0; fi
{ printf 'pwd=%s\\n' "$(pwd)"; printf 'home=%s\\n' "$HOME"; printf 'tmpdir=%s\\n' "$TMPDIR"; env | sed 's/^/env:/'; for a in "$@"; do printf 'arg:%s\\n' "$a"; done; } > "$FAKE_LOG"
case "$FAKE_MODE" in
  doc) cat "$FAKE_DOC" > "$out" ;;
  hang) echo "$$" > "$FAKE_PIDS"; sleep 300 & echo "$!" >> "$FAKE_PIDS"; wait ;;
  big) head -c 200000 /dev/zero | tr '\\000' 'a' > "$out"; echo "$$" > "$FAKE_PIDS"; sleep 300 & echo "$!" >> "$FAKE_PIDS"; wait ;;
  crash) echo "boom: details of the crash" >&2; exit 3 ;;
  signal) kill -9 $$ ;;
  nooutput) exit 0 ;;
esac
exit 0
`,
  );
  chmodSync(script, 0o755);
  const log = join(directory, "log.txt");
  const pids = join(directory, "pids.txt");
  const doc = join(directory, "doc.json");
  return {
    binary: script,
    log,
    pids,
    directory,
    env: (mode) => ({ FAKE_MODE: mode, FAKE_LOG: log, FAKE_PIDS: pids, FAKE_DOC: doc }),
    /** Makes mode "doc" write this text as the document. */
    setDocument: (text) => writeFileSync(doc, text),
    readLog: () => readFileSync(log, "utf8"),
    readPids: () =>
      existsSync(pids)
        ? readFileSync(pids, "utf8").split("\n").filter(Boolean).map(Number)
        : [],
  };
}

/** Whether a process still runs; a zombie waiting to be reaped counts as gone. */
export function isAlive(pid) {
  try {
    process.kill(pid, 0);
  } catch {
    return false;
  }
  try {
    const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
    return stat.slice(stat.lastIndexOf(")") + 2, stat.lastIndexOf(")") + 3) !== "Z";
  } catch {
    return true;
  }
}

/** Waits until `check()` is true or `ms` pass. */
export async function waitFor(check, ms = 5000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return true;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  return check();
}

/** Names of the entries in the OS temp directory that the package creates. */
export function leftoverDirectories() {
  return readdirSync(tmpdir()).filter((name) => name.startsWith("blacksparrow-"));
}

/**
 * Points this test process's temporary directory at a private one, removed at exit, so the
 * leftover checks do not see directories of test files running in parallel.
 */
export function isolateTemporaryDirectory() {
  const directory = mkdtempSync(join(tmpdir(), "bs-test-"));
  process.env.TMPDIR = directory;
  process.on("exit", () => rmSync(directory, { recursive: true, force: true }));
  return directory;
}
