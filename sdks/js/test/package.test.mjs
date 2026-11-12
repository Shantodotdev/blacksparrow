// The published shape of the package: both module formats, licenses, files, no runtime dependencies.
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, test } from "node:test";
import { packageRoot, repositoryRoot } from "../testlib/support.mjs";

const manifest = JSON.parse(readFileSync(join(packageRoot, "package.json"), "utf8"));
const API = ["BlackSparrowError", "SCHEMA_VERSION", "binaryEnvironmentVariable", "binaryPath", "extract", "extractRaw", "platformPackages", "version"];

describe("package", () => {
  test("has no runtime dependencies besides the platform packages", () => {
    assert.equal(manifest.dependencies, undefined);
    assert.equal(manifest.peerDependencies, undefined);
    assert.ok(Object.keys(manifest.optionalDependencies ?? {}).every((name) => name.startsWith("blacksparrow-")));
  });

  test("declares its package metadata", () => {
    assert.equal(manifest.license, "MIT OR Apache-2.0");
    assert.equal(manifest.engines.node, ">=20");
    assert.equal(manifest.repository.type, "git");
    assert.match(manifest.repository.url, /^git\+https:\/\/github\.com\/Shantodotdev\/blacksparrow\.git$/);
    assert.equal(manifest.repository.directory, "sdks/js");
    for (const path of [manifest.main, manifest.module, manifest.types, manifest.bin.blacksparrow]) {
      assert.ok(existsSync(join(packageRoot, path)), `${path} is built`);
    }
    for (const condition of ["import", "require"]) {
      assert.ok(existsSync(join(packageRoot, manifest.exports["."][condition].types)));
      assert.ok(existsSync(join(packageRoot, manifest.exports["."][condition].default)));
    }
  });

  test("ships the same licenses as the crate", () => {
    for (const license of ["LICENSE-MIT", "LICENSE-APACHE"]) {
      assert.equal(readFileSync(join(packageRoot, license), "utf8"), readFileSync(join(repositoryRoot, license), "utf8"), license);
      assert.ok(manifest.files.includes(license));
    }
  });

  test("the version matches the crate's", () => {
    const cargo = readFileSync(join(repositoryRoot, "Cargo.toml"), "utf8");
    const crate = /^version = "([^"]+)"/m.exec(cargo)?.[1];
    assert.ok(crate);
    assert.equal(manifest.version.split("-")[0], crate.split("-")[0], "package and crate versions share major.minor.patch");
  });

  test("the ESM entry point exports the public API", async () => {
    const module = await import("../dist/esm/index.js");
    assert.deepEqual(Object.keys(module).sort(), API);
  });

  test("the CommonJS entry point exports the same API", () => {
    const output = execFileSync(process.execPath, ["-e", "console.log(JSON.stringify(Object.keys(require('./dist/cjs/index.js')).sort()))"], { cwd: packageRoot, encoding: "utf8" });
    assert.deepEqual(JSON.parse(output), API);
  });

  test("the package resolves itself through its exports map in both formats", () => {
    const script = "Promise.all([import('blacksparrow'), Promise.resolve(require('blacksparrow'))]).then(([a, b]) => console.log(typeof a.extract, typeof b.extract, a.BlackSparrowError === b.BlackSparrowError))";
    const result = spawnSync(process.execPath, ["-e", script], { cwd: packageRoot, encoding: "utf8" });
    // Self-reference works because package.json has an exports map.
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.trim(), "function function false");
  });

  test("the command line shim forwards to the engine", { skip: process.env.BLACKSPARROW_BINARY ? false : "BLACKSPARROW_BINARY is not set" }, () => {
    const result = spawnSync(process.execPath, [join(packageRoot, manifest.bin.blacksparrow), "--version"], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /^blacksparrow \d+\.\d+\.\d+/);
    const failing = spawnSync(process.execPath, [join(packageRoot, manifest.bin.blacksparrow), "--version"], { encoding: "utf8", env: { PATH: process.env.PATH, BLACKSPARROW_BINARY: "/does/not/exist" } });
    assert.equal(failing.status, 127);
    assert.match(failing.stderr, /BLACKSPARROW_BINARY/);
  });
});
