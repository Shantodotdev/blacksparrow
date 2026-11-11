// The script that turns release archives into platform packages, run against fake archives.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, createWriteStream, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";
import { PLATFORM_PACKAGES } from "../dist/esm/binary.js";
import { PLATFORMS } from "../scripts/stage-platform-packages.mjs";
import { packageRoot } from "../testlib/support.mjs";

const script = join(packageRoot, "scripts/stage-platform-packages.mjs");
const MAGIC = { linux: "7f454c46", darwin: "cffaedfe", win32: "4d5a9000" };
const root = mkdtempSync(join(tmpdir(), "bs-stage-test-"));
let counter = 0;
after(() => rmSync(root, { recursive: true, force: true }));

const zipAvailable = spawnSync("zip", ["-v"]).status === 0;

function sh(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8" });
  assert.equal(result.status, 0, `${command} ${args.join(" ")}: ${result.stderr}`);
}

/** A directory of fake archives for every target; `change` may alter one before it is packed. */
function fakeArtifacts({ skip = [], layout = "flat", mutate = () => {} } = {}) {
  const dir = join(root, `case-${counter++}`);
  const artifacts = join(dir, "artifacts");
  mkdirSync(artifacts, { recursive: true });
  for (const platform of PLATFORMS) {
    if (skip.includes(platform.target)) continue;
    const stagingRoot = join(dir, "pack", platform.target);
    const inner = join(stagingRoot, `blacksparrow-${platform.target}`);
    mkdirSync(inner, { recursive: true });
    const name = platform.os === "win32" ? "blacksparrow.exe" : "blacksparrow";
    writeFileSync(join(inner, name), Buffer.concat([Buffer.from(MAGIC[platform.os], "hex"), Buffer.from(`fake ${platform.target}`)]));
    writeFileSync(join(inner, "README.md"), "readme");
    writeFileSync(join(inner, "LICENSE-MIT"), "license");
    mutate(platform, inner, name);

    const folder = layout === "nested" ? join(artifacts, `artifacts-${platform.target}`) : artifacts;
    mkdirSync(folder, { recursive: true });
    const archive = join(folder, `blacksparrow-${platform.target}.${platform.os === "win32" ? "zip" : "tar.xz"}`);
    if (platform.os === "win32") sh("zip", ["-q", "-r", archive, `blacksparrow-${platform.target}`], stagingRoot);
    else sh("tar", ["-cJf", archive, `blacksparrow-${platform.target}`], stagingRoot);
    const digest = createHash("sha256").update(readFileSync(archive)).digest("hex");
    writeFileSync(`${archive}.sha256`, `${digest}  ${archive.split("/").pop()}\n`);
  }
  return { dir, artifacts, out: join(dir, "out"), main: mainCopy(dir) };
}

/** A copy of the main package's manifest and licenses, so the script never edits the real one. */
function mainCopy(dir) {
  const main = join(dir, "main");
  mkdirSync(main);
  for (const file of ["package.json", "LICENSE-MIT", "LICENSE-APACHE"]) copyFileSync(join(packageRoot, file), join(main, file));
  return main;
}

function stage(fixture, version = "0.1.0", extra = []) {
  return spawnSync(process.execPath, [script, "--artifacts", fixture.artifacts, "--version", version, "--out", fixture.out, "--main", fixture.main, ...extra], { encoding: "utf8" });
}

describe("stage-platform-packages", { skip: zipAvailable ? false : "the zip tool is not installed" }, () => {
  test("stages one package per platform and pins the main package", () => {
    const fixture = fakeArtifacts();
    const result = stage(fixture, "v1.2.3-rc.1");
    assert.equal(result.status, 0, result.stderr);

    assert.deepEqual(readdirSync(fixture.out).sort(), PLATFORMS.map((platform) => platform.name).sort());
    for (const platform of PLATFORMS) {
      const directory = join(fixture.out, platform.name);
      const manifest = JSON.parse(readFileSync(join(directory, "package.json"), "utf8"));
      assert.equal(manifest.name, platform.name);
      assert.equal(manifest.version, "1.2.3-rc.1");
      assert.deepEqual(manifest.os, [platform.os]);
      assert.deepEqual(manifest.cpu, [platform.cpu]);
      assert.deepEqual(manifest.libc, platform.libc === undefined ? undefined : [platform.libc]);
      assert.equal(manifest.license, "MIT OR Apache-2.0");
      assert.match(manifest.repository.url, /blacksparrow\.git$/);
      assert.deepEqual(manifest.files, ["bin", "README.md", "LICENSE-MIT", "LICENSE-APACHE"]);

      const binary = join(directory, "bin", platform.os === "win32" ? "blacksparrow.exe" : "blacksparrow");
      assert.ok(readFileSync(binary).toString("latin1").includes(`fake ${platform.target}`));
      if (platform.os !== "win32") assert.equal(statSync(binary).mode & 0o777, 0o755);
      assert.ok(existsSync(join(directory, "README.md")));
      assert.ok(existsSync(join(directory, "LICENSE-MIT")) && existsSync(join(directory, "LICENSE-APACHE")));
    }

    const main = JSON.parse(readFileSync(join(fixture.main, "package.json"), "utf8"));
    assert.equal(main.version, "1.2.3-rc.1");
    assert.deepEqual(main.optionalDependencies, Object.fromEntries(PLATFORMS.map((platform) => [platform.name, "1.2.3-rc.1"]).sort()));
  });

  test("finds archives in the per-artifact folders of download-artifact", () => {
    const fixture = fakeArtifacts({ layout: "nested" });
    const result = stage(fixture);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(readdirSync(fixture.out).length, 7);
  });

  test("fails loudly when a target is missing and stages nothing", () => {
    const fixture = fakeArtifacts({ skip: ["aarch64-unknown-linux-musl", "x86_64-pc-windows-msvc"] });
    const before = readFileSync(join(fixture.main, "package.json"), "utf8");
    const result = stage(fixture);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /blacksparrow-aarch64-unknown-linux-musl\.tar\.xz: found 0/);
    assert.match(result.stderr, /blacksparrow-x86_64-pc-windows-msvc\.zip: found 0/);
    assert.equal(existsSync(fixture.out), false);
    assert.equal(readFileSync(join(fixture.main, "package.json"), "utf8"), before);
  });

  test("fails when a digest file is missing or does not match", () => {
    const missing = fakeArtifacts();
    rmSync(join(missing.artifacts, "blacksparrow-x86_64-apple-darwin.tar.xz.sha256"));
    let result = stage(missing);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /x86_64-apple-darwin\.tar\.xz\.sha256: found 0/);

    const corrupt = fakeArtifacts();
    const archive = join(corrupt.artifacts, "blacksparrow-x86_64-unknown-linux-gnu.tar.xz");
    writeFileSync(archive, Buffer.concat([readFileSync(archive), Buffer.from("tampered")]));
    result = stage(corrupt);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /x86_64-unknown-linux-gnu\.tar\.xz: SHA-256 mismatch/);
    assert.equal(existsSync(corrupt.out), false);

    const wrongName = fakeArtifacts();
    const digestFile = join(wrongName.artifacts, "blacksparrow-aarch64-apple-darwin.tar.xz.sha256");
    writeFileSync(digestFile, readFileSync(digestFile, "utf8").replace("aarch64-apple-darwin", "other"));
    result = stage(wrongName);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /digest of/);
  });

  test("fails when the binary is not an executable of the platform", () => {
    const fixture = fakeArtifacts({
      mutate: (platform, inner, name) => {
        if (platform.target === "aarch64-apple-darwin") writeFileSync(join(inner, name), "#!/bin/sh\necho hi\n");
      },
    });
    const result = stage(fixture);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /is not a darwin executable/);
    assert.equal(existsSync(fixture.out), false, "nothing is staged when one binary is wrong");
  });

  test("fails when an archive holds no binary or two", () => {
    const none = fakeArtifacts({ mutate: (platform, inner, name) => platform.os === "linux" && rmSync(join(inner, name)) });
    let result = stage(none);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /holds 0 files named blacksparrow/);

    const two = fakeArtifacts({
      mutate: (platform, inner, name) => {
        if (platform.target === "x86_64-pc-windows-msvc") {
          mkdirSync(join(inner, "extra"));
          copyFileSync(join(inner, name), join(inner, "extra", name));
        }
      },
    });
    result = stage(two);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /holds 2 files named blacksparrow\.exe/);
  });

  test("rejects a bad version and missing arguments", () => {
    const fixture = fakeArtifacts();
    assert.notEqual(stage(fixture, "one.two").status, 0);
    const missing = spawnSync(process.execPath, [script, "--version", "0.1.0"], { encoding: "utf8" });
    assert.notEqual(missing.status, 0);
    assert.match(missing.stderr, /--artifacts is required/);
  });
});

describe("platform list", () => {
  test("the script, the library and the main package agree", () => {
    const manifest = JSON.parse(readFileSync(join(packageRoot, "package.json"), "utf8"));
    // The committed manifest has none (the registry cannot resolve unpublished packages for `npm ci`);
    // the staging script writes them at release time. If someone adds them, they must be exact.
    const optional = manifest.optionalDependencies ?? {};
    if (Object.keys(optional).length > 0) {
      assert.deepEqual(Object.keys(optional).sort(), PLATFORMS.map((platform) => platform.name).sort());
      assert.deepEqual(new Set(Object.values(optional)), new Set([manifest.version]), "optionalDependencies pin the package's own version");
    }
    assert.deepEqual(
      PLATFORM_PACKAGES.map(({ name, platform, arch, libc }) => ({ name, os: platform, cpu: arch, libc: libc === undefined ? undefined : libc === "gnu" ? "glibc" : "musl" })).sort((a, b) => a.name.localeCompare(b.name)),
      PLATFORMS.map(({ name, os, cpu, libc }) => ({ name, os, cpu, libc })).sort((a, b) => a.name.localeCompare(b.name)),
    );
  });
});
