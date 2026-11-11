/** Version of the JSON document this package understands. */
export const SCHEMA_VERSION = 1;

/** How an extraction ended. Anything but `ok` means the site could not be read. */
export type Outcome =
  | "ok"
  | "unsupported_url"
  | "unresolved"
  | "blocked_address"
  | "unreachable"
  | "bot_protected"
  | "http_error"
  | "not_html";

/** When pages are rendered in headless Chromium. */
export type RenderMode = "auto" | "always" | "never";

/** Why a page was read: the homepage, or the rule that chose it. */
export type PageKind =
  | "home"
  | "contact"
  | "about"
  | "catalog"
  | "locations"
  | "ordering"
  | "requested"
  | (string & {});

/** Why a page was rendered. */
export type RenderReason = "js_shell" | "bot_challenge" | "http_error" | "always" | (string & {});

/** Why an image is a logo candidate; the list is ordered best first. */
export type LogoReason =
  | "header_logo"
  | "schema_logo"
  | "logo_hint"
  | "apple_touch_icon"
  | "og_image"
  | "icon"
  | (string & {});

/** Where a colour was seen. */
export type ColorEvidenceType =
  | "css_variable"
  | "stylesheet"
  | "inline_style"
  | "theme_color_meta"
  | (string & {});

/** A hyperlink of a page. */
export interface PageLink {
  text: string;
  /** Absolute http(s) URL. */
  url: string;
}

/** One page that was read. */
export interface Page {
  /** The URL asked for. */
  url: string;
  /** The URL after redirects. */
  final_url: string;
  kind: PageKind;
  /** HTTP status of the plain fetch. */
  status: number;
  /** Whether the content comes from a Chromium render. */
  rendered: boolean;
  render_reason?: RenderReason;
  /** Bot-protection provider seen on the final content. */
  waf?: string;
  title: string | null;
  description: string | null;
  lang: string | null;
  word_count: number;
  /** The page as Markdown. */
  markdown: string;
  links: PageLink[];
  phones: string[];
  emails: string[];
  whatsapp: string[];
  facebook: string[];
  instagram: string[];
}

/** A page that could not be read. */
export interface PageError {
  url: string;
  error: string;
}

/** Where a colour was seen. */
export interface ColorEvidence {
  type: ColorEvidenceType;
  /** The custom property name, for `css_variable`. */
  name?: string;
  sourceUrl?: string;
}

/** A colour of the site with its use. */
export interface Color {
  /** `#RRGGBB`, uppercase. */
  hex: string;
  occurrences: number;
  /** Share of all colour uses, 0 to 1. */
  usage_score: number;
  /** Up to 5 places it was seen. */
  evidence: ColorEvidence[];
}

/** A font family the site applies. */
export interface Font {
  family: string;
  /** Kinds of text it styles: `headings`, `body`, `buttons`, `links`. */
  selectors: string[];
  /** Weights declared for it, ascending. */
  weights: number[];
}

/** A logo candidate: an image URL, or inline SVG markup. */
export interface Logo {
  reason: LogoReason;
  /** Absolute http(s) image URL; null for an inline SVG. */
  url: string | null;
  /** A standalone SVG document; null for an image URL. */
  svg: string | null;
}

/** A stylesheet that was fetched for the design analysis. */
export interface Stylesheet {
  url: string;
  status: number | null;
  bytes: number;
  error?: string;
}

/** The site's design as its homepage and stylesheets show it. */
export interface Design {
  title: string | null;
  description: string | null;
  site_name: string | null;
  /** `#RRGGBB` from the `theme-color` meta tag. */
  theme_color: string | null;
  /** Most used first. */
  colors: Color[];
  /** Families with known roles first. */
  fonts: Font[];
  /** Best first. */
  logos: Logo[];
  stylesheets: Stylesheet[];
}

/** Contacts merged across pages, in page order. */
export interface Contacts {
  phones: string[];
  emails: string[];
  whatsapp: string[];
  facebook: string[];
  instagram: string[];
}

/** schema.org business facts the pages state in JSON-LD. */
export interface Structured {
  names: string[];
  telephones: string[];
  emails: string[];
  addresses: string[];
  same_as: string[];
  logos: string[];
  descriptions: string[];
}

/** What the extraction cost. */
export interface Stats {
  elapsed_ms: number;
  http_fetches: number;
  http_bytes: number;
  rendered_pages: number;
  render_ms: number;
  blocked_browser_requests: number;
  /** Peak resident memory of the engine in KiB (Linux only; Chromium not included). */
  peak_rss_kib?: number;
}

/** The engine's reading of a website, validated and capped by this package. */
export interface SiteExtract {
  schema_version: typeof SCHEMA_VERSION;
  /** `blacksparrow <version>`. */
  generator: string;
  /** The URL asked for. */
  start_url: string;
  /** The homepage after redirects. */
  final_url: string | null;
  outcome: Outcome;
  /** Details of a failed outcome. */
  outcome_detail?: string;
  /** Pages read, homepage (kind `home`) first; empty when the homepage could not be read. */
  pages: Page[];
  /** Pages that could not be read. */
  page_errors: PageError[];
  /** Null when the homepage could not be read. */
  design: Design | null;
  contacts: Contacts;
  structured: Structured;
  stats: Stats;
}

/** Settings shared by every render mode. */
export interface ExtractOptionsBase {
  /** Pages read in total, homepage included, 1 to 50. Default 6. */
  maxPages?: number;
  /**
   * The engine's own deadline for the whole site, in seconds, 5 to 3600. Default 90. The process
   * is killed `killGraceSeconds` after it.
   */
  timeoutSeconds?: number;
  /** Seconds beyond `timeoutSeconds` before the process is killed. Default 30. */
  killGraceSeconds?: number;
  /** Pages to read after the homepage instead of choosing them by rules (http(s) URLs). */
  pages?: readonly string[];
  /** Allows private and loopback addresses. Only for local test sites. */
  allowPrivateNetwork?: boolean;
  /**
   * Egress proxy for all traffic, `http://[user:pass@]host:port`. Credentials appear in the
   * engine's command line.
   */
  proxy?: string;
  /** User-Agent of plain fetches. */
  userAgent?: string;
  /** Runs Chromium without its sandbox, for containers without user namespaces. */
  chromeNoSandbox?: boolean;
  /** Largest document read from the engine, in bytes. Default 16 MiB. */
  maxOutputBytes?: number;
  /** Stops the extraction and kills the process tree. */
  signal?: AbortSignal;
  /** The engine binary; overrides `BLACKSPARROW_BINARY` and the installed platform package. */
  binary?: string;
  /** Extra environment variables for the engine, added to the minimal environment. */
  env?: Readonly<Record<string, string>>;
}

/**
 * Settings of one extraction. Rendering needs a Chromium: `chromePath` is required unless
 * `render` is `"never"`.
 */
export type ExtractOptions = ExtractOptionsBase &
  (
    | {
        /** Default `"auto"`: only JavaScript shells and bot challenges are rendered. */
        render?: "auto" | "always";
        /** Absolute path of a Chromium or chrome-headless-shell executable. */
        chromePath: string;
      }
    | {
        render: "never";
        chromePath?: string;
      }
  );

/** An extraction result with the document the engine printed. */
export interface RawExtract {
  /** The validated, capped result. */
  result: SiteExtract;
  /** The parsed document as the engine printed it (NUL characters removed). */
  raw: unknown;
}

/** Options of `binaryPath` and `version`. */
export interface BinaryOptions {
  /** The engine binary; overrides `BLACKSPARROW_BINARY` and the installed platform package. */
  binary?: string;
  /** Stops `version()` and kills the process tree. */
  signal?: AbortSignal;
}
