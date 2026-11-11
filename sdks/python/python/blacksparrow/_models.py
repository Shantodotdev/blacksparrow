"""The typed result of ``blacksparrow extract``.

The binary's JSON document is untrusted input: the site it read controls every string in it. The
models therefore validate leniently. A value of the wrong type becomes absent (``None``, an empty
list, ``0`` or ``""``), a list loses its invalid items and is capped, a text is trimmed and capped,
and a URL that is not an absolute http(s) URL is dropped. Unknown fields are ignored, so a newer
binary that adds fields keeps working; the models are frozen.

The only reasons validation fails are a missing or unknown ``outcome`` on the document itself, and
(inside a list) an item that lacks its identifying field, such as a page without a valid URL; such
an item is dropped from its list.
"""

from __future__ import annotations

import re
from typing import Annotated, Any, Literal, TypeGuard, TypeVar
from urllib.parse import urlsplit

from pydantic import (
    AliasChoices,
    BaseModel,
    BeforeValidator,
    ConfigDict,
    Field,
    ValidationError,
)

SCHEMA_VERSION: Literal[1] = 1
"""The version of the JSON document these models understand."""

Outcome = Literal[
    "ok",
    "unsupported_url",
    "unresolved",
    "blocked_address",
    "unreachable",
    "bot_protected",
    "http_error",
    "not_html",
]
"""Why a site could or could not be read. Everything except ``"ok"`` means no homepage."""

MAX_URL_CHARS = 4096
MAX_PAGES = 100
MAX_MARKDOWN_CHARS = 300_000
MAX_LINKS = 200
MAX_CONTACTS = 20
MAX_COLORS = 30
MAX_FONTS = 30
MAX_LOGOS = 12
MAX_STYLESHEETS = 50
MAX_PAGE_ERRORS = 100
MAX_INLINE_SVG_CHARS = 200_000

_HEX = re.compile(r"[#][0-9A-F]{6}")
_Model = TypeVar("_Model", bound=BaseModel)


def _text(value: Any, max_chars: int) -> str | None:
    """``value`` as trimmed text of at most ``max_chars``; None when it is not non-empty text."""
    if not isinstance(value, str):
        return None
    stripped = value.strip()
    return stripped[:max_chars] if stripped else None


def web_url(value: Any) -> str | None:
    """``value`` when it is an absolute http(s) URL with a host; None otherwise.

    A protocol-relative URL (``//cdn.example.com/x.png``) is read as https.
    """
    if not isinstance(value, str):
        return None
    candidate = value.strip()
    if candidate.startswith("//"):
        candidate = f"https:{candidate}"
    if not candidate or len(candidate) > MAX_URL_CHARS:
        return None
    if any(char.isspace() or ord(char) < 0x20 for char in candidate):
        return None
    try:
        parts = urlsplit(candidate)
        parts.port  # noqa: B018 - raises ValueError for a malformed port
    except ValueError:
        return None
    if parts.scheme.lower() not in ("http", "https") or not parts.hostname:
        return None
    return candidate


def is_list(value: object) -> TypeGuard[list[Any]]:
    """Whether ``value`` is a list (of anything)."""
    return isinstance(value, list)


def _as_list(value: object) -> list[Any]:
    """``value`` when it is a list; an empty list otherwise."""
    return value if is_list(value) else []


def _count(value: Any) -> int:
    """``value`` when it is a non-negative integer (not a bool); 0 otherwise."""
    if isinstance(value, int) and not isinstance(value, bool) and value >= 0:
        return value
    return 0


def _optional_count(value: Any) -> int | None:
    """Like ``_count``, but None instead of 0."""
    if isinstance(value, int) and not isinstance(value, bool) and value >= 0:
        return value
    return None


def _flag(value: Any) -> bool:
    """``value`` when it is a bool; False otherwise."""
    return value if isinstance(value, bool) else False


def _hex_color(value: Any) -> str | None:
    return value if isinstance(value, str) and _HEX.fullmatch(value) else None


def _optional_text(max_chars: int) -> BeforeValidator:
    return BeforeValidator(lambda value: _text(value, max_chars))


def _text_or(default: str, max_chars: int) -> BeforeValidator:
    return BeforeValidator(lambda value: _text(value, max_chars) or default)


def _strings(max_items: int, max_chars: int) -> BeforeValidator:
    """A list of the distinct non-empty strings of a list (or of one string), trimmed and capped."""

    def read(value: Any) -> list[str]:
        items: list[Any] = [value] if isinstance(value, str) else _as_list(value)
        strings: list[str] = []
        for item in items:
            text = _text(item, max_chars)
            if text is not None and text not in strings:
                strings.append(text)
                if len(strings) >= max_items:
                    break
        return strings

    return BeforeValidator(read)


def _urls(max_items: int) -> BeforeValidator:
    """A list of the distinct http(s) URLs of a list."""

    def read(value: Any) -> list[str]:
        urls: list[str] = []
        for item in _as_list(value):
            url = web_url(item)
            if url is not None and url not in urls:
                urls.append(url)
                if len(urls) >= max_items:
                    break
        return urls

    return BeforeValidator(read)


def _list_of(model: type[_Model], max_items: int) -> BeforeValidator:
    """The items of a list that validate as ``model``; invalid items are dropped."""

    def read(value: Any) -> list[_Model]:
        items: list[_Model] = []
        for item in _as_list(value):
            try:
                items.append(model.model_validate(item))
            except ValidationError:
                continue
            if len(items) >= max_items:
                break
        return items

    return BeforeValidator(read)


def _optional_model(model: type[_Model]) -> BeforeValidator:
    """``model`` read from an object; None when the value is not a valid one."""

    def read(value: Any) -> _Model | None:
        try:
            return model.model_validate(value)
        except ValidationError:
            return None

    return BeforeValidator(read)


def _model_or_default(model: type[_Model]) -> BeforeValidator:
    """``model`` read from an object; an empty ``model`` when the value is not a valid one."""

    def read(value: Any) -> _Model:
        try:
            return model.model_validate(value)
        except ValidationError:
            return model.model_validate({})

    return BeforeValidator(read)


def _weights(value: Any) -> list[int]:
    return [
        item
        for item in _as_list(value)
        if isinstance(item, int) and not isinstance(item, bool) and 1 <= item <= 1000
    ][:20]


class _Lenient(BaseModel):
    model_config = ConfigDict(extra="ignore", frozen=True)


class PageLink(_Lenient):
    """A hyperlink on a page.

    Attributes:
        text: The visible text (or label) of the link; empty when it has none.
        url: The absolute http(s) target.
    """

    text: Annotated[str, _text_or("", 200)] = ""
    url: Annotated[str, BeforeValidator(web_url)]


class Page(_Lenient):
    """One page Black Sparrow read.

    Attributes:
        url: The address asked for.
        final_url: The address after redirects.
        kind: ``"home"``, or why the page was chosen: ``"contact"``, ``"about"``, ``"catalog"``,
            ``"locations"``, ``"ordering"`` or ``"requested"`` (a page the caller named).
        status: The HTTP status of the plain fetch; 0 when unknown.
        rendered: Whether the content came from headless Chromium.
        render_reason: Why it was rendered: ``"js_shell"``, ``"bot_challenge"``, ``"http_error"``
            or ``"always"``.
        waf: The bot-protection provider seen on the final content, if any.
        title: The page's ``<title>``.
        description: The meta (or Open Graph) description.
        lang: The ``<html lang>`` value.
        word_count: Words of visible text.
        markdown: The page as Markdown.
        links: The page's http(s) links.
        phones: Phone numbers on the page, as written.
        emails: Email addresses on the page.
        whatsapp: WhatsApp chat links on the page.
        facebook: Canonical Facebook page links on the page.
        instagram: Canonical Instagram profile links on the page.
    """

    url: Annotated[str, BeforeValidator(web_url)]
    final_url: Annotated[str | None, BeforeValidator(web_url)] = None
    kind: Annotated[str, _text_or("page", 20)] = "page"
    status: Annotated[int, BeforeValidator(_count)] = 0
    rendered: Annotated[bool, BeforeValidator(_flag)] = False
    render_reason: Annotated[str | None, _optional_text(40)] = None
    waf: Annotated[str | None, _optional_text(60)] = None
    title: Annotated[str | None, _optional_text(300)] = None
    description: Annotated[str | None, _optional_text(2000)] = None
    lang: Annotated[str | None, _optional_text(35)] = None
    word_count: Annotated[int, BeforeValidator(_count)] = 0
    markdown: Annotated[str, _text_or("", MAX_MARKDOWN_CHARS)] = ""
    links: Annotated[list[PageLink], _list_of(PageLink, MAX_LINKS)] = Field(
        default_factory=list[PageLink]
    )
    phones: Annotated[list[str], _strings(MAX_CONTACTS, 40)] = Field(default_factory=list[str])
    emails: Annotated[list[str], _strings(MAX_CONTACTS, 200)] = Field(default_factory=list[str])
    whatsapp: Annotated[list[str], _urls(MAX_CONTACTS)] = Field(default_factory=list[str])
    facebook: Annotated[list[str], _urls(MAX_CONTACTS)] = Field(default_factory=list[str])
    instagram: Annotated[list[str], _urls(MAX_CONTACTS)] = Field(default_factory=list[str])


class PageError(_Lenient):
    """A page Black Sparrow chose to read but could not.

    Attributes:
        url: The address asked for.
        error: What went wrong.
    """

    url: Annotated[str, BeforeValidator(web_url)]
    error: Annotated[str, _text_or("unknown error", 300)] = "unknown error"


class ColorEvidence(_Lenient):
    """Where a colour was seen.

    Attributes:
        type: ``"css_variable"``, ``"stylesheet"``, ``"inline_style"`` or ``"theme_color_meta"``.
        name: The custom property name, for ``"css_variable"``.
        source_url: The stylesheet or page the colour was seen in (``sourceUrl`` in the document).
    """

    type: Annotated[str, _text_or("stylesheet", 40)] = "stylesheet"
    name: Annotated[str | None, _optional_text(120)] = None
    source_url: Annotated[
        str | None,
        BeforeValidator(web_url),
        Field(validation_alias=AliasChoices("sourceUrl", "source_url")),
    ] = None


class Color(_Lenient):
    """A colour of the site with how much it is used.

    Attributes:
        hex: ``"#RRGGBB"``, upper case.
        occurrences: How many times the colour is written.
        usage_score: Its 0 to 1 share of all colour uses.
        evidence: Where it was seen (up to 5).
    """

    hex: Annotated[str, Field(pattern=_HEX.pattern)]
    occurrences: Annotated[int, BeforeValidator(_count)] = 0
    usage_score: Annotated[float, Field(ge=0, le=1)] = 0.0
    evidence: Annotated[list[ColorEvidence], _list_of(ColorEvidence, 5)] = Field(
        default_factory=list[ColorEvidence]
    )


class Font(_Lenient):
    """A font family of the site.

    Attributes:
        family: The family name as written.
        selectors: The kinds of text it styles: ``"headings"``, ``"body"``, ``"buttons"``,
            ``"links"``.
        weights: The weights declared for it, ascending.
    """

    family: Annotated[str, BeforeValidator(lambda value: _text(value, 120))]
    selectors: Annotated[list[str], _strings(8, 20)] = Field(default_factory=list[str])
    weights: Annotated[list[int], BeforeValidator(_weights)] = Field(default_factory=list[int])


class Logo(_Lenient):
    """A logo candidate: an image URL, or inline SVG markup.

    Attributes:
        reason: Why this is a candidate, best first: ``"header_logo"``, ``"schema_logo"``,
            ``"logo_hint"``, ``"apple_touch_icon"``, ``"og_image"``, ``"icon"``.
        url: The absolute image URL; None for an inline SVG.
        svg: A standalone SVG document; None for an image URL. Untrusted markup: sanitise it
            before serving it to a browser.
    """

    reason: Annotated[str, _text_or("logo_hint", 40)] = "logo_hint"
    url: Annotated[str | None, BeforeValidator(web_url)] = None
    svg: Annotated[str | None, _optional_text(MAX_INLINE_SVG_CHARS)] = None


class Stylesheet(_Lenient):
    """A stylesheet fetched for the design analysis.

    Attributes:
        url: Where it was fetched from.
        status: The HTTP status, when an answer came.
        bytes: Bytes read.
        error: What went wrong, if anything.
    """

    url: Annotated[str, BeforeValidator(web_url)]
    status: Annotated[int | None, BeforeValidator(_optional_count)] = None
    bytes: Annotated[int, BeforeValidator(_count)] = 0
    error: Annotated[str | None, _optional_text(300)] = None


class Design(_Lenient):
    """The site's design as its homepage and stylesheets show it.

    Attributes:
        title: The homepage title.
        description: The homepage description.
        site_name: The ``og:site_name`` of the homepage.
        theme_color: The ``theme-color`` meta as ``"#RRGGBB"``.
        colors: The site's colours, most used first.
        fonts: The site's font families, those with known roles first.
        logos: Logo candidates, best first.
        stylesheets: The stylesheets read.
    """

    title: Annotated[str | None, _optional_text(300)] = None
    description: Annotated[str | None, _optional_text(2000)] = None
    site_name: Annotated[str | None, _optional_text(300)] = None
    theme_color: Annotated[str | None, BeforeValidator(_hex_color)] = None
    colors: Annotated[list[Color], _list_of(Color, MAX_COLORS)] = Field(default_factory=list[Color])
    fonts: Annotated[list[Font], _list_of(Font, MAX_FONTS)] = Field(default_factory=list[Font])
    logos: Annotated[list[Logo], _list_of(Logo, MAX_LOGOS)] = Field(default_factory=list[Logo])
    stylesheets: Annotated[list[Stylesheet], _list_of(Stylesheet, MAX_STYLESHEETS)] = Field(
        default_factory=list[Stylesheet]
    )


class Contacts(_Lenient):
    """Contacts merged across all pages, in page order.

    Attributes:
        phones: Phone numbers as written.
        emails: Email addresses, lower case.
        whatsapp: WhatsApp chat links.
        facebook: Canonical Facebook page links.
        instagram: Canonical Instagram profile links.
    """

    phones: Annotated[list[str], _strings(MAX_CONTACTS, 40)] = Field(default_factory=list[str])
    emails: Annotated[list[str], _strings(MAX_CONTACTS, 200)] = Field(default_factory=list[str])
    whatsapp: Annotated[list[str], _urls(MAX_CONTACTS)] = Field(default_factory=list[str])
    facebook: Annotated[list[str], _urls(MAX_CONTACTS)] = Field(default_factory=list[str])
    instagram: Annotated[list[str], _urls(MAX_CONTACTS)] = Field(default_factory=list[str])


class Structured(_Lenient):
    """schema.org business facts the pages state in JSON-LD.

    Attributes:
        names: ``name`` values of organisation-like nodes.
        telephones: ``telephone`` values.
        emails: ``email`` values.
        addresses: Postal addresses, one string each.
        same_as: ``sameAs`` profile URLs.
        logos: ``logo`` URLs.
        descriptions: ``description`` values.
    """

    names: Annotated[list[str], _strings(MAX_CONTACTS, 200)] = Field(default_factory=list[str])
    telephones: Annotated[list[str], _strings(MAX_CONTACTS, 40)] = Field(default_factory=list[str])
    emails: Annotated[list[str], _strings(MAX_CONTACTS, 200)] = Field(default_factory=list[str])
    addresses: Annotated[list[str], _strings(MAX_CONTACTS, 500)] = Field(default_factory=list[str])
    same_as: Annotated[list[str], _urls(MAX_CONTACTS)] = Field(default_factory=list[str])
    logos: Annotated[list[str], _urls(MAX_CONTACTS)] = Field(default_factory=list[str])
    descriptions: Annotated[list[str], _strings(5, 2000)] = Field(default_factory=list[str])


class Stats(_Lenient):
    """What the run cost.

    Attributes:
        elapsed_ms: Wall-clock milliseconds.
        http_fetches: HTTP requests answered.
        http_bytes: Body bytes read over HTTP.
        rendered_pages: Pages rendered in Chromium.
        render_ms: Milliseconds spent rendering, browser start included.
        blocked_browser_requests: Browser requests refused (images, fonts, media, refused
            addresses).
        peak_rss_kib: Peak resident memory of the binary in KiB (Linux only; Chromium excluded).
    """

    elapsed_ms: Annotated[int, BeforeValidator(_count)] = 0
    http_fetches: Annotated[int, BeforeValidator(_count)] = 0
    http_bytes: Annotated[int, BeforeValidator(_count)] = 0
    rendered_pages: Annotated[int, BeforeValidator(_count)] = 0
    render_ms: Annotated[int, BeforeValidator(_count)] = 0
    blocked_browser_requests: Annotated[int, BeforeValidator(_count)] = 0
    peak_rss_kib: Annotated[int | None, BeforeValidator(_optional_count)] = None


class SiteExtract(_Lenient):
    """Black Sparrow's reading of a website.

    A site that could not be read is a normal result: ``outcome`` says why and ``pages`` is empty.

    Attributes:
        schema_version: The document version; always ``SCHEMA_VERSION``.
        generator: The binary that wrote the document, such as ``"blacksparrow 0.1.0"``.
        start_url: The address asked for.
        final_url: The homepage address after redirects.
        outcome: How the reading ended.
        outcome_detail: Details of a failed outcome.
        pages: The pages read, the homepage (kind ``"home"``) first; empty when the homepage
            could not be read.
        page_errors: Pages that were chosen but could not be read.
        design: The site's design; None when the homepage could not be read.
        contacts: Contacts across all pages.
        structured: schema.org business facts across all pages.
        stats: What the run cost.
    """

    schema_version: Literal[1] = SCHEMA_VERSION
    generator: Annotated[str | None, _optional_text(100)] = None
    start_url: Annotated[str | None, BeforeValidator(web_url)] = None
    final_url: Annotated[str | None, BeforeValidator(web_url)] = None
    outcome: Outcome
    outcome_detail: Annotated[str | None, _optional_text(300)] = None
    pages: Annotated[list[Page], _list_of(Page, MAX_PAGES)] = Field(default_factory=list[Page])
    page_errors: Annotated[list[PageError], _list_of(PageError, MAX_PAGE_ERRORS)] = Field(
        default_factory=list[PageError]
    )
    design: Annotated[Design | None, _optional_model(Design)] = None
    contacts: Annotated[Contacts, _model_or_default(Contacts)] = Field(default_factory=Contacts)
    structured: Annotated[Structured, _model_or_default(Structured)] = Field(
        default_factory=Structured
    )
    stats: Annotated[Stats, _model_or_default(Stats)] = Field(default_factory=Stats)

    @property
    def ok(self) -> bool:
        """Whether the homepage was read (``outcome`` is ``"ok"``)."""
        return self.outcome == "ok"

    @property
    def homepage(self) -> Page | None:
        """The page of kind ``"home"``; None when the homepage was not read."""
        return next((page for page in self.pages if page.kind == "home"), None)
