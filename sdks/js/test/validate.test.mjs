// The lenient reader of the engine's document.
import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { BlackSparrowError } from "../dist/esm/index.js";
import { MAX_PAGES, parseDocument, parseSiteExtract } from "../dist/esm/validate.js";

const base = { schema_version: 1, outcome: "ok" };

describe("parseSiteExtract", () => {
  test("a minimal document gets every list and object defaulted", () => {
    const result = parseSiteExtract(base);
    assert.deepEqual(result.pages, []);
    assert.equal(result.design, null);
    assert.equal(result.final_url, null);
    assert.deepEqual(result.contacts, { phones: [], emails: [], whatsapp: [], facebook: [], instagram: [] });
    assert.equal(result.stats.elapsed_ms, 0);
    assert.equal("outcome_detail" in result, false);
  });

  test("version and outcome are checked", () => {
    for (const document of [null, [], "x", 3]) {
      assert.throws(() => parseSiteExtract(document), { code: "invalid_output" });
    }
    assert.throws(() => parseSiteExtract({ outcome: "ok" }), { code: "unsupported_schema_version" });
    assert.throws(() => parseSiteExtract({ ...base, schema_version: "1" }), { code: "unsupported_schema_version" });
    assert.throws(() => parseSiteExtract({ ...base, schema_version: 2 }), BlackSparrowError);
    assert.throws(() => parseSiteExtract({ ...base, outcome: "great" }), { code: "invalid_output" });
    assert.throws(() => parseSiteExtract({ schema_version: 1 }), { code: "invalid_output" });
  });

  test("every outcome of the engine is accepted", () => {
    for (const outcome of ["ok", "unsupported_url", "unresolved", "blocked_address", "unreachable", "bot_protected", "http_error", "not_html"]) {
      assert.equal(parseSiteExtract({ ...base, outcome }).outcome, outcome);
    }
  });

  test("values of the wrong type are dropped or defaulted", () => {
    const result = parseSiteExtract({
      ...base,
      generator: 5,
      start_url: { a: 1 },
      final_url: "ftp://nope/",
      pages: [
        null,
        "page",
        { url: 42 },
        { url: "javascript:alert(1)" },
        {
          url: "https://a.test/",
          status: "200",
          word_count: -3,
          rendered: "yes",
          title: 7,
          markdown: ["x"],
          links: [{ url: "https://a.test/x", text: 5 }, { url: "mailto:a@b.c", text: "mail" }, 3],
          phones: ["+880 1711", 5, "", null],
          emails: "a@b.c",
          whatsapp: ["https://wa.me/1", "https://wa.me/1", "tel:1"],
        },
      ],
      design: { colors: [{ hex: "#abcdef", usage_score: 7, occurrences: 2.5 }, { hex: "red" }, { hex: 5 }], theme_color: "blue", fonts: [{ family: 3 }, { family: "Lora", weights: [400, 0, 1001, "700", 700.5] }] },
      contacts: "none",
      stats: { elapsed_ms: "fast", peak_rss_kib: 100 },
    });
    assert.equal(result.generator, "blacksparrow");
    assert.equal(result.start_url, "");
    assert.equal(result.final_url, null);
    assert.equal(result.pages.length, 1);
    const [page] = result.pages;
    assert.deepEqual(
      { status: page.status, word_count: page.word_count, rendered: page.rendered, title: page.title, markdown: page.markdown },
      { status: 0, word_count: 0, rendered: false, title: null, markdown: "" },
    );
    assert.deepEqual(page.links, [{ url: "https://a.test/x", text: "" }]);
    assert.deepEqual(page.phones, ["+880 1711"]);
    assert.deepEqual(page.emails, []);
    assert.deepEqual(page.whatsapp, ["https://wa.me/1"]);
    assert.equal(page.final_url, "https://a.test/");
    assert.deepEqual(result.design.colors, [{ hex: "#ABCDEF", occurrences: 0, usage_score: 1, evidence: [] }]);
    assert.equal(result.design.theme_color, null);
    assert.deepEqual(result.design.fonts, [{ family: "Lora", selectors: [], weights: [400] }]);
    assert.deepEqual(result.contacts.emails, []);
    assert.equal(result.stats.elapsed_ms, 0);
    assert.equal(result.stats.peak_rss_kib, 100);
  });

  test("lists are capped and long text is truncated", () => {
    const pages = Array.from({ length: 200 }, (_, index) => ({ url: `https://a.test/${index}`, links: Array.from({ length: 500 }, (_, n) => ({ url: `https://a.test/l${n}`, text: "x".repeat(1000) })), markdown: "m".repeat(400_000) }));
    const result = parseSiteExtract({
      ...base,
      pages,
      design: { colors: Array.from({ length: 100 }, (_, n) => ({ hex: `#${String(n).padStart(6, "0")}` })), logos: Array.from({ length: 50 }, () => ({ reason: "icon", url: "https://a.test/l.png" })), fonts: Array.from({ length: 100 }, (_, n) => ({ family: `F${n}` })) },
      contacts: { emails: Array.from({ length: 100 }, (_, n) => `a${n}@b.test`) },
    });
    assert.equal(result.pages.length, MAX_PAGES);
    assert.equal(result.pages[0].links.length, 200);
    assert.equal(result.pages[0].links[0].text.length, 200);
    assert.equal(result.pages[0].markdown.length, 300_000);
    assert.equal(result.design.colors.length, 30);
    assert.equal(result.design.logos.length, 12);
    assert.equal(result.design.fonts.length, 30);
    assert.equal(result.contacts.emails.length, 20);
  });

  test("truncation never cuts a surrogate pair", () => {
    const result = parseSiteExtract({ ...base, pages: [{ url: "https://a.test/", markdown: "a".repeat(299_999) + "😀" }] });
    assert.equal(result.pages[0].markdown, "a".repeat(299_999));
  });

  test("a logo needs a URL or SVG; unknown keys are not copied", () => {
    const result = parseSiteExtract({
      ...base,
      polluted: true,
      design: { logos: [{ reason: "icon" }, { reason: "header_logo", svg: "<svg/>" }, { url: "https://a.test/l.png" }], __proto__x: 1 },
    });
    assert.deepEqual(result.design.logos, [
      { reason: "header_logo", url: null, svg: "<svg/>" },
      { reason: "logo_hint", url: "https://a.test/l.png", svg: null },
    ]);
    assert.equal("polluted" in result, false);
  });

  test("a __proto__ key in the document cannot pollute anything", () => {
    const document = parseDocument('{"schema_version":1,"outcome":"ok","__proto__":{"polluted":1},"pages":[{"url":"https://a.test/","__proto__":{"x":1}}]}');
    const result = parseSiteExtract(document);
    assert.equal({}.polluted, undefined);
    assert.equal(result.pages[0].x, undefined);
    assert.equal(Object.getPrototypeOf(result), Object.prototype);
  });
});

describe("parseDocument", () => {
  test("rejects text that is not JSON", () => {
    assert.throws(() => parseDocument("{"), { code: "invalid_output" });
    assert.throws(() => parseDocument(""), { code: "invalid_output" });
  });

  test("a deeply nested document does not crash the reader", () => {
    const deep = "[".repeat(5000) + "]".repeat(5000);
    const parsed = parseDocument(`{"schema_version":1,"outcome":"ok","deep":${deep}}`);
    assert.equal(parseSiteExtract(parsed).outcome, "ok");
  });
});
