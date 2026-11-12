// The types and the engine's published JSON schema (schema/extract.schema.json) describe the same document.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, test } from "node:test";
import { WIRE_KEYS, parseSiteExtract } from "../dist/esm/validate.js";
import { repositoryRoot } from "../testlib/support.mjs";

const schemaPath = join(repositoryRoot, "schema/extract.schema.json");
const skip = existsSync(schemaPath) ? false : "schema/extract.schema.json does not exist";

// Which definition of the schema describes each type of this package.
const DEFINITIONS = {
  Page: "PageOut",
  PageLink: "PageLink",
  PageError: "PageError",
  Design: "DesignOut",
  Color: "ColorOut",
  ColorEvidence: "ColorEvidence",
  Font: "FontUse",
  Logo: "LogoCandidate",
  Stylesheet: "StylesheetOut",
  Contacts: "ContactsOut",
  Structured: "StructuredBusiness",
  Stats: "StatsOut",
};

describe("engine JSON schema", { skip }, () => {
  const schema = skip ? {} : JSON.parse(readFileSync(schemaPath, "utf8"));
  const definitions = schema.$defs ?? {};

  test("the root object has the keys of SiteExtract", () => {
    assert.deepEqual(Object.keys(schema.properties).sort(), [...WIRE_KEYS.SiteExtract].sort());
  });

  for (const [type, definition] of Object.entries(DEFINITIONS)) {
    test(`${type} has the keys of ${definition}`, () => {
      assert.ok(definitions[definition], `${definition} is in the schema`);
      assert.deepEqual(Object.keys(definitions[definition].properties).sort(), [...WIRE_KEYS[type]].sort());
    });
  }

  test("every definition of the schema is covered", () => {
    const covered = new Set([...Object.values(DEFINITIONS), "Outcome", "LogoReason"]);
    assert.deepEqual(Object.keys(definitions).filter((name) => !covered.has(name)), []);
  });

  test("every outcome of the schema is accepted", () => {
    const outcomes = definitions.Outcome.enum ?? definitions.Outcome.oneOf.map((entry) => entry.const);
    assert.ok(outcomes.length > 0);
    for (const outcome of outcomes) {
      assert.equal(parseSiteExtract({ schema_version: 1, outcome }).outcome, outcome);
    }
  });

  test("the schema version is the one this package reads", () => {
    const version = schema.properties.schema_version;
    const allowed = version.const ?? version.enum?.[0] ?? version.default ?? version.examples?.[0];
    if (allowed !== undefined) assert.equal(allowed, 1);
  });

  test("a validated minimal document carries every required key of the root", () => {
    const result = parseSiteExtract({ schema_version: 1, outcome: "ok" });
    for (const key of schema.required) assert.ok(key in result, key);
  });
});
