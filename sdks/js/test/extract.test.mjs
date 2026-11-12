// End-to-end tests against the real engine binary and the fixture sites in tests/fixtures/sites.
import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { binaryPath, extract, extractRaw, version, BlackSparrowError } from "../dist/esm/index.js";
import { isolateTemporaryDirectory, leftoverDirectories, realBinary, serveSite, testChrome } from "../testlib/support.mjs";

isolateTemporaryDirectory();

/** Every key the engine printed is still there after validation, at every depth. */
function assertNoFieldLost(raw, result, path = "$") {
  if (Array.isArray(raw)) {
    assert.ok(Array.isArray(result), path);
    assert.equal(result.length, raw.length, `${path} length`);
    raw.forEach((item, index) => assertNoFieldLost(item, result[index], `${path}[${index}]`));
  } else if (raw !== null && typeof raw === "object") {
    assert.ok(result !== null && typeof result === "object", path);
    for (const [key, value] of Object.entries(raw)) {
      assert.ok(key in result, `${path}.${key} was lost`);
      assertNoFieldLost(value, result[key], `${path}.${key}`);
    }
  }
}

const binary = realBinary();
const skip = binary === undefined ? "BLACKSPARROW_BINARY is not set and target/debug/blacksparrow does not exist" : false;
const chrome = testChrome();

describe("real engine", { skip }, () => {
  before(() => {
    process.env.BLACKSPARROW_BINARY = binary;
  });

  test("version() and binaryPath() use BLACKSPARROW_BINARY", async () => {
    assert.equal(binaryPath(), binary);
    assert.match(await version(), /^\d+\.\d+\.\d+/);
  });

  describe("bakery site", () => {
    let site;
    let result;
    let raw;
    before(async () => {
      site = await serveSite("bakery");
      ({ result, raw } = await extractRaw(`${site.url}/`, {
        render: "never",
        allowPrivateNetwork: true,
        maxPages: 4,
        timeoutSeconds: 30,
      }));
    });
    after(() => site.close());

    test("reads the homepage and the pages that matter", () => {
      assert.equal(result.schema_version, 1);
      assert.equal(result.outcome, "ok");
      assert.deepEqual(
        result.pages.map((page) => [page.kind, new URL(page.url).pathname]),
        [
          ["home", "/"],
          ["contact", "/contact.html"],
          ["about", "/about.html"],
          ["catalog", "/shop.html"],
        ],
      );
      assert.equal(result.pages[0].lang, "bn");
      assert.ok(result.pages[0].title.includes("Sweet Crumbs"));
      assert.ok(result.pages[1].markdown.includes("Dhanmondi"));
      assert.ok(result.pages.every((page) => page.rendered === false));
      assert.deepEqual(result.page_errors, []);
    });

    test("reads contacts and business facts", () => {
      assert.deepEqual(result.contacts.emails, ["hello@sweetcrumbs.com.bd"]);
      assert.deepEqual(result.contacts.whatsapp, ["https://wa.me/8801711223344"]);
      assert.deepEqual(result.contacts.facebook, ["https://www.facebook.com/SweetCrumbsBD"]);
      assert.deepEqual(result.contacts.instagram, ["https://www.instagram.com/sweetcrumbs.bd/"]);
      assert.deepEqual(result.structured.names, ["Sweet Crumbs Bakery"]);
    });

    test("reads the design", () => {
      const { design } = result;
      assert.equal(design.theme_color, "#C0392B");
      assert.equal(design.site_name, "Sweet Crumbs");
      const brand = design.colors.find((color) => color.hex === "#C0392B");
      assert.ok(brand, "brand colour");
      assert.ok(brand.evidence.length > 0);
      assert.ok(design.fonts.some((font) => font.family === "Playfair Display" && font.selectors.includes("headings")));
      assert.ok(design.logos.length > 0);
      assert.equal(design.stylesheets.length, 2);
    });

    test("the raw document has every key the validated result has", () => {
      assert.equal(raw.schema_version, 1);
      assertNoFieldLost(raw, result);
      assert.ok(result.stats.http_fetches >= 4);
      assert.equal(result.stats.rendered_pages, 0);
    });

    test("cart and privacy pages are never requested", () => {
      assert.ok(!site.requests.some((path) => path === "/cart" || path === "/privacy"), site.requests.join(" "));
    });
  });

  test("pages chosen by the caller replace the rules", async () => {
    const site = await serveSite("bakery");
    try {
      const result = await extract(`${site.url}/`, {
        render: "never",
        allowPrivateNetwork: true,
        pages: [`${site.url}/shop.html`, `${site.url}/missing.html`],
        timeoutSeconds: 30,
      });
      assert.equal(result.outcome, "ok");
      assert.equal(result.pages.length, 2);
      assert.equal(result.pages[1].kind, "requested");
      assert.equal(result.page_errors.length, 1);
      assert.ok(result.page_errors[0].url.endsWith("/missing.html"));
    } finally {
      await site.close();
    }
  });

  test("a JavaScript shell is returned unrendered when render is never", async () => {
    const site = await serveSite("spa");
    try {
      const result = await extract(`${site.url}/`, { render: "never", allowPrivateNetwork: true, timeoutSeconds: 30 });
      assert.equal(result.outcome, "ok");
      assert.equal(result.pages[0].rendered, false);
      assert.equal(result.stats.rendered_pages, 0);
    } finally {
      await site.close();
    }
  });

  test("a bot challenge is a normal result with outcome bot_protected", async () => {
    const site = await serveSite("hostile", {
      "/": { file: "challenge.html", status: 403, headers: { "cf-mitigated": "challenge" } },
    });
    try {
      const result = await extract(`${site.url}/`, { render: "never", allowPrivateNetwork: true, timeoutSeconds: 30 });
      assert.equal(result.outcome, "bot_protected");
      assert.ok(result.outcome_detail);
      assert.equal(result.pages[0].status, 403);
    } finally {
      await site.close();
    }
  });

  test("private addresses are refused unless allowed", async () => {
    const site = await serveSite("bakery");
    try {
      const result = await extract(`${site.url}/`, { render: "never", timeoutSeconds: 30 });
      assert.equal(result.outcome, "blocked_address");
      assert.deepEqual(result.pages, []);
      assert.equal(result.design, null);
      assert.deepEqual(site.requests, []);
    } finally {
      await site.close();
    }
  });

  test("an address that is not http(s) gives unsupported_url without running the engine", async () => {
    for (const url of ["ftp://example.com/", "javascript:alert(1)", "example.com", "", "http://", "file:///etc/passwd"]) {
      const result = await extract(url, { render: "never", binary: "/does/not/exist" }).catch((error) => error);
      // The engine binary is never needed for an unsupported address.
      assert.equal(result.outcome, "unsupported_url", url);
    }
  });

  test("an address that starts with a dash cannot become an option", async () => {
    const result = await extract("--version", { render: "never" });
    assert.equal(result.outcome, "unsupported_url");
  });

  test("the temporary directory is removed", async () => {
    const before = new Set(leftoverDirectories());
    await extract("http://127.0.0.1:9/", { render: "never", timeoutSeconds: 30 });
    const left = leftoverDirectories().filter((name) => !before.has(name));
    assert.deepEqual(left, []);
  });

  test("render needs a Chromium", async () => {
    await assert.rejects(extract("https://example.com/", {}), (error) => {
      assert.ok(error instanceof BlackSparrowError);
      assert.equal(error.code, "invalid_options");
      return true;
    });
    await assert.rejects(extract("https://example.com/", { chromePath: "/does/not/exist" }), {
      code: "invalid_options",
    });
  });

  describe("rendering", { skip: chrome === undefined ? "BLACKSPARROW_TEST_CHROME is not set" : false }, () => {
    test("a JavaScript shell is rendered in Chromium", async () => {
      const site = await serveSite("spa");
      try {
        const result = await extract(`${site.url}/`, {
          chromePath: chrome,
          chromeNoSandbox: true,
          allowPrivateNetwork: true,
          timeoutSeconds: 60,
        });
        assert.equal(result.outcome, "ok");
        const home = result.pages[0];
        assert.equal(home.rendered, true);
        assert.equal(home.render_reason, "js_shell");
        assert.ok(home.markdown.includes("Bespoke suits"));
        assert.deepEqual(home.emails, ["orders@dhakatailors.test"]);
        assert.ok(result.stats.rendered_pages >= 1);
      } finally {
        await site.close();
      }
    });
  });
});
