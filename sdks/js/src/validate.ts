import { BlackSparrowError } from "./errors.js";
import {
  SCHEMA_VERSION,
  type Color,
  type ColorEvidence,
  type Contacts,
  type Design,
  type Font,
  type Logo,
  type Outcome,
  type Page,
  type PageError,
  type PageLink,
  type SiteExtract,
  type Stats,
  type Structured,
  type Stylesheet,
} from "./types.js";

/** Most pages kept. */
export const MAX_PAGES = 50;
const MAX_PAGE_ERRORS = 50;
const MAX_MARKDOWN_CHARS = 300_000;
const MAX_LINKS = 200;
const MAX_CONTACTS = 20;
const MAX_COLORS = 30;
const MAX_FONTS = 30;
const MAX_LOGOS = 12;
const MAX_STYLESHEETS = 30;
const MAX_INLINE_SVG_CHARS = 200_000;
const MAX_URL_CHARS = 2048;

const OUTCOMES: ReadonlySet<string> = new Set<Outcome>([
  "ok",
  "unsupported_url",
  "unresolved",
  "blocked_address",
  "unreachable",
  "bot_protected",
  "http_error",
  "not_html",
]);
const HEX = /^#[0-9A-F]{6}$/;
// A NUL character is valid JSON (an escape) but breaks text columns and C strings downstream.
const NUL_ESCAPE = /(?<!\\)((?:\\\\)*)\\u0000/g;

type Json = Record<string, unknown>;

function isObject(value: unknown): value is Json {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** The first `max` UTF-16 units of `value`, without cutting a surrogate pair in half. */
function truncate(value: string, max: number): string {
  if (value.length <= max) return value;
  let end = max;
  const last = value.charCodeAt(end - 1);
  if (last >= 0xd800 && last <= 0xdbff) end -= 1;
  return value.slice(0, end);
}

function text(value: unknown, max: number): string | undefined {
  return typeof value === "string" ? truncate(value, max) : undefined;
}

function nonEmptyText(value: unknown, max: number): string | undefined {
  const result = text(value, max)?.trim();
  return result === undefined || result === "" ? undefined : result;
}

function textOr(value: unknown, fallback: string, max: number): string {
  return nonEmptyText(value, max) ?? fallback;
}

function nullableText(value: unknown, max: number): string | null {
  return nonEmptyText(value, max) ?? null;
}

/** An absolute http(s) URL, or undefined. */
function webUrl(value: unknown): string | undefined {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_URL_CHARS) {
    return undefined;
  }
  if (/[\s\u0000-\u001f\u007f]/.test(value)) return undefined;
  try {
    const parsed = new URL(value);
    return (parsed.protocol === "http:" || parsed.protocol === "https:") && parsed.hostname !== ""
      ? value
      : undefined;
  } catch {
    return undefined;
  }
}

function count(value: unknown): number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : 0;
}

function nullableStatus(value: unknown): number | null {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= 999
    ? value
    : null;
}

function fraction(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? Math.min(Math.max(value, 0), 1) : 0;
}

/** The items of an array that `read` accepts, at most `max`; anything else gives an empty list. */
function list<T>(value: unknown, max: number, read: (item: unknown) => T | undefined): T[] {
  const items: T[] = [];
  if (!Array.isArray(value)) return items;
  for (const item of value) {
    const read_ = read(item);
    if (read_ !== undefined) items.push(read_);
    if (items.length >= max) break;
  }
  return items;
}

function strings(value: unknown, max: number, maxChars: number): string[] {
  return list(value, max, (item) => nonEmptyText(item, maxChars));
}

function urls(value: unknown, max: number): string[] {
  const seen = new Set<string>();
  return list(value, max, (item) => {
    const url = webUrl(item);
    if (url === undefined || seen.has(url)) return undefined;
    seen.add(url);
    return url;
  });
}

function readLink(value: unknown): PageLink | undefined {
  if (!isObject(value)) return undefined;
  const url = webUrl(value["url"]);
  return url === undefined ? undefined : { text: text(value["text"], 200) ?? "", url };
}

function readPage(value: unknown): Page | undefined {
  if (!isObject(value)) return undefined;
  const url = webUrl(value["url"]);
  if (url === undefined) return undefined;
  const page: Page = {
    url,
    final_url: webUrl(value["final_url"]) ?? url,
    kind: textOr(value["kind"], "page", 20),
    status: count(value["status"]),
    rendered: value["rendered"] === true,
    title: nullableText(value["title"], 300),
    description: nullableText(value["description"], 2000),
    lang: nullableText(value["lang"], 35),
    word_count: count(value["word_count"]),
    markdown: text(value["markdown"], MAX_MARKDOWN_CHARS) ?? "",
    links: list(value["links"], MAX_LINKS, readLink),
    phones: strings(value["phones"], MAX_CONTACTS, 40),
    emails: strings(value["emails"], MAX_CONTACTS, 200),
    whatsapp: urls(value["whatsapp"], MAX_CONTACTS),
    facebook: urls(value["facebook"], MAX_CONTACTS),
    instagram: urls(value["instagram"], MAX_CONTACTS),
  };
  const reason = nonEmptyText(value["render_reason"], 40);
  if (reason !== undefined) page.render_reason = reason;
  const waf = nonEmptyText(value["waf"], 60);
  if (waf !== undefined) page.waf = waf;
  return page;
}

function readPageError(value: unknown): PageError | undefined {
  if (!isObject(value)) return undefined;
  const url = webUrl(value["url"]);
  return url === undefined ? undefined : { url, error: text(value["error"], 500) ?? "" };
}

function readEvidence(value: unknown): ColorEvidence | undefined {
  if (!isObject(value)) return undefined;
  const evidence: ColorEvidence = { type: textOr(value["type"], "stylesheet", 40) };
  const name = nonEmptyText(value["name"], 120);
  if (name !== undefined) evidence.name = name;
  const sourceUrl = webUrl(value["sourceUrl"]);
  if (sourceUrl !== undefined) evidence.sourceUrl = sourceUrl;
  return evidence;
}

function readColor(value: unknown): Color | undefined {
  if (!isObject(value)) return undefined;
  const hex = typeof value["hex"] === "string" ? value["hex"].toUpperCase() : "";
  if (!HEX.test(hex)) return undefined;
  return {
    hex,
    occurrences: count(value["occurrences"]),
    usage_score: fraction(value["usage_score"]),
    evidence: list(value["evidence"], 5, readEvidence),
  };
}

function readFont(value: unknown): Font | undefined {
  if (!isObject(value)) return undefined;
  const family = nonEmptyText(value["family"], 120);
  if (family === undefined) return undefined;
  return {
    family,
    selectors: strings(value["selectors"], 8, 20),
    weights: list(value["weights"], 20, (weight) =>
      typeof weight === "number" && Number.isInteger(weight) && weight >= 1 && weight <= 1000
        ? weight
        : undefined,
    ),
  };
}

function readLogo(value: unknown): Logo | undefined {
  if (!isObject(value)) return undefined;
  const url = webUrl(value["url"]) ?? null;
  const svg = nonEmptyText(value["svg"], MAX_INLINE_SVG_CHARS) ?? null;
  if (url === null && svg === null) return undefined;
  return { reason: textOr(value["reason"], "logo_hint", 40), url, svg };
}

function readStylesheet(value: unknown): Stylesheet | undefined {
  if (!isObject(value)) return undefined;
  const url = webUrl(value["url"]);
  if (url === undefined) return undefined;
  const sheet: Stylesheet = {
    url,
    status: nullableStatus(value["status"]),
    bytes: count(value["bytes"]),
  };
  const error = nonEmptyText(value["error"], 300);
  if (error !== undefined) sheet.error = error;
  return sheet;
}

function readDesign(value: unknown): Design | null {
  if (!isObject(value)) return null;
  const themeColor =
    typeof value["theme_color"] === "string" ? value["theme_color"].toUpperCase() : "";
  return {
    title: nullableText(value["title"], 300),
    description: nullableText(value["description"], 2000),
    site_name: nullableText(value["site_name"], 200),
    theme_color: HEX.test(themeColor) ? themeColor : null,
    colors: list(value["colors"], MAX_COLORS, readColor),
    fonts: list(value["fonts"], MAX_FONTS, readFont),
    logos: list(value["logos"], MAX_LOGOS, readLogo),
    stylesheets: list(value["stylesheets"], MAX_STYLESHEETS, readStylesheet),
  };
}

function readContacts(value: unknown): Contacts {
  const source = isObject(value) ? value : {};
  return {
    phones: strings(source["phones"], MAX_CONTACTS, 40),
    emails: strings(source["emails"], MAX_CONTACTS, 200),
    whatsapp: urls(source["whatsapp"], MAX_CONTACTS),
    facebook: urls(source["facebook"], MAX_CONTACTS),
    instagram: urls(source["instagram"], MAX_CONTACTS),
  };
}

function readStructured(value: unknown): Structured {
  const source = isObject(value) ? value : {};
  return {
    names: strings(source["names"], MAX_CONTACTS, 200),
    telephones: strings(source["telephones"], MAX_CONTACTS, 40),
    emails: strings(source["emails"], MAX_CONTACTS, 200),
    addresses: strings(source["addresses"], MAX_CONTACTS, 500),
    same_as: urls(source["same_as"], MAX_CONTACTS),
    logos: urls(source["logos"], MAX_CONTACTS),
    descriptions: strings(source["descriptions"], 5, 2000),
  };
}

function readStats(value: unknown): Stats {
  const source = isObject(value) ? value : {};
  const stats: Stats = {
    elapsed_ms: count(source["elapsed_ms"]),
    http_fetches: count(source["http_fetches"]),
    http_bytes: count(source["http_bytes"]),
    rendered_pages: count(source["rendered_pages"]),
    render_ms: count(source["render_ms"]),
    blocked_browser_requests: count(source["blocked_browser_requests"]),
  };
  if (typeof source["peak_rss_kib"] === "number") stats.peak_rss_kib = count(source["peak_rss_kib"]);
  return stats;
}

/**
 * Reads the engine's document leniently: a field of the wrong type is dropped or defaulted, lists
 * are capped, text is truncated and only http(s) URLs are kept. The document must still carry the
 * schema version this package understands and a known outcome.
 *
 * @throws BlackSparrowError When the document is not an object, has another `schema_version`, or
 * has no known `outcome`.
 */
export function parseSiteExtract(document: unknown): SiteExtract {
  if (!isObject(document)) {
    throw new BlackSparrowError("invalid_output", "Black Sparrow answered with a document that is not an object");
  }
  if (document["schema_version"] !== SCHEMA_VERSION) {
    throw new BlackSparrowError(
      "unsupported_schema_version",
      `Black Sparrow answered with schema_version ${JSON.stringify(document["schema_version"])}; this package reads version ${SCHEMA_VERSION}. Update the package or the binary.`,
    );
  }
  const outcome = document["outcome"];
  if (typeof outcome !== "string" || !OUTCOMES.has(outcome)) {
    throw new BlackSparrowError("invalid_output", "Black Sparrow answered with an unknown outcome");
  }
  const extract: SiteExtract = {
    schema_version: SCHEMA_VERSION,
    generator: textOr(document["generator"], "blacksparrow", 100),
    start_url: text(document["start_url"], MAX_URL_CHARS) ?? "",
    final_url: webUrl(document["final_url"]) ?? null,
    outcome: outcome as Outcome,
    pages: list(document["pages"], MAX_PAGES, readPage),
    page_errors: list(document["page_errors"], MAX_PAGE_ERRORS, readPageError),
    design: readDesign(document["design"]),
    contacts: readContacts(document["contacts"]),
    structured: readStructured(document["structured"]),
    stats: readStats(document["stats"]),
  };
  const detail = nonEmptyText(document["outcome_detail"], 300);
  if (detail !== undefined) extract.outcome_detail = detail;
  return extract;
}

/**
 * Parses the engine's output text: NUL characters are removed (also from keys) before parsing.
 *
 * @throws BlackSparrowError When the text is not JSON.
 */
export function parseDocument(textContent: string): unknown {
  try {
    return JSON.parse(textContent.replace(NUL_ESCAPE, "$1"));
  } catch (error) {
    throw new BlackSparrowError("invalid_output", "Black Sparrow printed no JSON", { cause: error });
  }
}

/** Every key each validated object can carry, by type. Used to keep the types and the engine's JSON schema aligned. */
export const WIRE_KEYS = {
  SiteExtract: [
    "schema_version", "generator", "start_url", "final_url", "outcome", "outcome_detail",
    "pages", "page_errors", "design", "contacts", "structured", "stats",
  ],
  Page: [
    "url", "final_url", "kind", "status", "rendered", "render_reason", "waf", "title",
    "description", "lang", "word_count", "markdown", "links", "phones", "emails", "whatsapp",
    "facebook", "instagram",
  ],
  PageLink: ["text", "url"],
  PageError: ["url", "error"],
  Design: [
    "title", "description", "site_name", "theme_color", "colors", "fonts", "logos", "stylesheets",
  ],
  Color: ["hex", "occurrences", "usage_score", "evidence"],
  ColorEvidence: ["type", "name", "sourceUrl"],
  Font: ["family", "selectors", "weights"],
  Logo: ["reason", "url", "svg"],
  Stylesheet: ["url", "status", "bytes", "error"],
  Contacts: ["phones", "emails", "whatsapp", "facebook", "instagram"],
  Structured: ["names", "telephones", "emails", "addresses", "same_as", "logos", "descriptions"],
  Stats: [
    "elapsed_ms", "http_fetches", "http_bytes", "rendered_pages", "render_ms",
    "blocked_browser_requests", "peak_rss_kib",
  ],
} as const satisfies Record<string, readonly string[]>;

type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;
type Check<T extends true> = T;
type Keys<K extends keyof typeof WIRE_KEYS> = (typeof WIRE_KEYS)[K][number];

// Compile-time proof that WIRE_KEYS lists exactly the keys of the exported types.
export type WireKeysMatchTypes = [
  Check<Same<keyof SiteExtract, Keys<"SiteExtract">>>,
  Check<Same<keyof Page, Keys<"Page">>>,
  Check<Same<keyof PageLink, Keys<"PageLink">>>,
  Check<Same<keyof PageError, Keys<"PageError">>>,
  Check<Same<keyof Design, Keys<"Design">>>,
  Check<Same<keyof Color, Keys<"Color">>>,
  Check<Same<keyof ColorEvidence, Keys<"ColorEvidence">>>,
  Check<Same<keyof Font, Keys<"Font">>>,
  Check<Same<keyof Logo, Keys<"Logo">>>,
  Check<Same<keyof Stylesheet, Keys<"Stylesheet">>>,
  Check<Same<keyof Contacts, Keys<"Contacts">>>,
  Check<Same<keyof Structured, Keys<"Structured">>>,
  Check<Same<keyof Stats, Keys<"Stats">>>,
];
