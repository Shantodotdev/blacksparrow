import { stat } from "node:fs/promises";
import { isAbsolute } from "node:path";
import { BINARY_ENV, PLATFORM_PACKAGES, resolveBinary } from "./binary.js";
import { BlackSparrowError } from "./errors.js";
import { runIsolated } from "./run.js";
import {
  SCHEMA_VERSION,
  type BinaryOptions,
  type ExtractOptions,
  type RawExtract,
  type SiteExtract,
} from "./types.js";
import { MAX_PAGES, parseDocument, parseSiteExtract } from "./validate.js";

export { BlackSparrowError, type BlackSparrowErrorCode } from "./errors.js";
export { SCHEMA_VERSION } from "./types.js";
export type * from "./types.js";

const DEFAULT_MAX_PAGES = 6;
const DEFAULT_TIMEOUT_SECONDS = 90;
const DEFAULT_KILL_GRACE_SECONDS = 30;
const DEFAULT_MAX_OUTPUT_BYTES = 16 * 1024 * 1024;
const VERSION_TIMEOUT_MS = 15_000;
const MAX_URL_CHARS = 8192;

function invalid(message: string): BlackSparrowError {
  return new BlackSparrowError("invalid_options", message);
}

/** The URL as `new URL` normalises it when it is an http(s) address, else undefined. */
function httpUrl(value: string): string | undefined {
  if (value.length > MAX_URL_CHARS || /[\u0000-\u001f\u007f]/.test(value)) return undefined;
  try {
    const parsed = new URL(value);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return undefined;
    return parsed.hostname === "" ? undefined : parsed.href;
  } catch {
    return undefined;
  }
}

function integerOption(
  name: string,
  value: number | undefined,
  fallback: number,
  min: number,
  max: number,
): number {
  if (value === undefined) return fallback;
  if (!Number.isInteger(value) || value < min || value > max) {
    throw invalid(`${name} must be an integer from ${min} to ${max}`);
  }
  return value;
}

function textOption(name: string, value: string): string {
  if (typeof value !== "string" || value === "" || /[\u0000-\u001f\u007f]/.test(value)) {
    throw invalid(`${name} must be a non-empty string without control characters`);
  }
  return value;
}

/** A document for a URL that is not an http(s) address; the engine is not run. */
function unsupportedUrl(url: string): RawExtract {
  const document = {
    schema_version: SCHEMA_VERSION,
    generator: "blacksparrow-js",
    start_url: url.slice(0, 2048),
    final_url: null,
    outcome: "unsupported_url",
    outcome_detail: "not an http(s) address",
    pages: [],
    page_errors: [],
    design: null,
    contacts: { phones: [], emails: [], whatsapp: [], facebook: [], instagram: [] },
    structured: {
      names: [], telephones: [], emails: [], addresses: [], same_as: [], logos: [], descriptions: [],
    },
    stats: {
      elapsed_ms: 0, http_fetches: 0, http_bytes: 0, rendered_pages: 0, render_ms: 0,
      blocked_browser_requests: 0,
    },
  };
  return { result: parseSiteExtract(document), raw: document };
}

/** Validates the options and builds the engine's command line. */
async function buildArguments(
  url: string,
  options: ExtractOptions,
): Promise<{ args: (outputPath: string) => string[]; deadlineMs: number; maxOutputBytes: number }> {
  if (typeof options !== "object" || options === null) {
    throw invalid("options are required: pass { chromePath } or { render: \"never\" }");
  }
  const render = options.render ?? "auto";
  if (render !== "auto" && render !== "always" && render !== "never") {
    throw invalid('render must be "auto", "always" or "never"');
  }
  const maxPages = integerOption("maxPages", options.maxPages, DEFAULT_MAX_PAGES, 1, MAX_PAGES);
  const timeoutSeconds = integerOption("timeoutSeconds", options.timeoutSeconds, DEFAULT_TIMEOUT_SECONDS, 5, 3600);
  const graceSeconds = integerOption("killGraceSeconds", options.killGraceSeconds, DEFAULT_KILL_GRACE_SECONDS, 0, 600);
  const maxOutputBytes = integerOption(
    "maxOutputBytes",
    options.maxOutputBytes,
    DEFAULT_MAX_OUTPUT_BYTES,
    1024,
    256 * 1024 * 1024,
  );

  const args = [
    "extract",
    "--max-pages",
    String(maxPages),
    "--render",
    render,
    "--timeout-secs",
    String(timeoutSeconds),
  ];

  if (render !== "never" && options.chromePath === undefined) {
    throw invalid('chromePath is required unless render is "never"');
  }
  if (options.chromePath !== undefined) {
    const chromePath = textOption("chromePath", options.chromePath);
    if (!isAbsolute(chromePath)) throw invalid("chromePath must be an absolute path");
    if (render !== "never") {
      const info = await stat(chromePath).catch(() => undefined);
      if (info === undefined || !info.isFile()) throw invalid(`chromePath is not a file: ${chromePath}`);
    }
    args.push(`--chrome-path=${chromePath}`);
  }
  if (options.chromeNoSandbox === true) args.push("--chrome-no-sandbox");
  if (options.allowPrivateNetwork === true) args.push("--allow-private-network");
  if (options.proxy !== undefined) {
    const proxy = textOption("proxy", options.proxy);
    if (httpUrl(proxy) === undefined || new URL(proxy).protocol !== "http:") {
      throw invalid("proxy must be an http://host:port URL");
    }
    args.push(`--proxy=${proxy}`);
  }
  if (options.userAgent !== undefined) {
    args.push(`--user-agent=${textOption("userAgent", options.userAgent).slice(0, 512)}`);
  }
  if (options.pages !== undefined) {
    if (!Array.isArray(options.pages) || options.pages.length >= MAX_PAGES) {
      throw invalid(`pages must be a list of fewer than ${MAX_PAGES} URLs`);
    }
    for (const page of options.pages as readonly unknown[]) {
      const address = typeof page === "string" ? httpUrl(page) : undefined;
      if (address === undefined) throw invalid("pages must contain only http(s) URLs");
      args.push(`--page=${address}`);
    }
  }

  return {
    args: (outputPath) => [...args, `--output=${outputPath}`, "--", url],
    deadlineMs: (timeoutSeconds + graceSeconds) * 1000,
    maxOutputBytes,
  };
}

/**
 * Reads a website and returns the validated result together with the document the engine printed.
 *
 * A site that cannot be read is a normal result whose `outcome` says why. An address that is not
 * an http(s) URL gives outcome `unsupported_url` without running the engine.
 *
 * @throws BlackSparrowError When the options are invalid, the engine cannot start, crashes, times
 * out, is aborted, or prints an unusable document.
 */
export async function extractRaw(url: string, options: ExtractOptions): Promise<RawExtract> {
  if (typeof url !== "string") throw new TypeError("url must be a string");
  const address = httpUrl(url);
  // Options are checked even when the address is not usable, so a wrong call fails the same way
  // for every URL.
  const { args, deadlineMs, maxOutputBytes } = await buildArguments(address ?? url, options);
  if (address === undefined) return unsupportedUrl(url);

  const binary = resolveBinary(options.binary);
  const run = await runIsolated({
    binary,
    args,
    deadlineMs,
    maxOutputBytes,
    env: options.env,
    signal: options.signal,
  });
  if (run.file === undefined) {
    throw new BlackSparrowError("no_output", "Black Sparrow wrote no output");
  }
  const raw = parseDocument(run.file.toString("utf8"));
  return { result: parseSiteExtract(raw), raw };
}

/**
 * Reads a website: its pages as Markdown with links and contacts, its colours, fonts and logo
 * candidates, and schema.org business facts.
 *
 * @throws BlackSparrowError See {@link extractRaw}.
 */
export async function extract(url: string, options: ExtractOptions): Promise<SiteExtract> {
  return (await extractRaw(url, options)).result;
}

/**
 * The version of the engine binary, such as `0.1.0`.
 *
 * @throws BlackSparrowError When the binary is missing, cannot start or prints no version.
 */
export async function version(options: BinaryOptions = {}): Promise<string> {
  const binary = resolveBinary(options.binary);
  const run = await runIsolated({
    binary,
    args: () => ["--version"],
    deadlineMs: VERSION_TIMEOUT_MS,
    maxOutputBytes: 64 * 1024,
    signal: options.signal,
  });
  const match = /^blacksparrow\s+(\S+)/.exec(run.stdout.toString("utf8").trim());
  if (match?.[1] === undefined) {
    throw new BlackSparrowError("invalid_output", "Black Sparrow printed no version");
  }
  return match[1];
}

/**
 * The absolute path of the engine binary that would run: the `binary` option, then
 * `BLACKSPARROW_BINARY`, then the installed platform package.
 *
 * @throws BlackSparrowError When no usable binary is found.
 */
export function binaryPath(options: Pick<BinaryOptions, "binary"> = {}): string {
  return resolveBinary(options.binary);
}

/** The npm packages that ship the binary, one per platform. */
export const platformPackages: readonly string[] = PLATFORM_PACKAGES.map((entry) => entry.name);

/** Name of the environment variable that points to the engine binary. */
export const binaryEnvironmentVariable: string = BINARY_ENV;
