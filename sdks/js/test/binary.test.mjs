// Finding the engine binary.
import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, describe, test } from "node:test";
import { PLATFORM_PACKAGES, detectLibc, platformPackageFor, resolveBinary } from "../dist/esm/binary.js";

const root = mkdtempSync(join(tmpdir(), "bs-binary-test-"));
after(() => rmSync(root, { recursive: true, force: true }));

/** A fake installed platform package; returns the resolver that finds it. */
function installed(name, platform = "linux") {
  const directory = join(root, name);
  mkdirSync(join(directory, "bin"), { recursive: true });
  writeFileSync(join(directory, "package.json"), "{}");
  const file = join(directory, "bin", platform === "win32" ? "blacksparrow.exe" : "blacksparrow");
  writeFileSync(file, "x");
  chmodSync(file, 0o755);
  return { file, resolve: (specifier) => (specifier === `${name}/package.json` ? join(directory, "package.json") : assert.fail(specifier)) };
}

describe("platformPackageFor", () => {
  test("maps every supported platform to its package", () => {
    const expected = [
      ["linux", "x64", "gnu", "blacksparrow-linux-x64-gnu"],
      ["linux", "arm64", "gnu", "blacksparrow-linux-arm64-gnu"],
      ["linux", "x64", "musl", "blacksparrow-linux-x64-musl"],
      ["linux", "arm64", "musl", "blacksparrow-linux-arm64-musl"],
      ["darwin", "x64", undefined, "blacksparrow-darwin-x64"],
      ["darwin", "arm64", undefined, "blacksparrow-darwin-arm64"],
      ["win32", "x64", undefined, "blacksparrow-win32-x64"],
    ];
    for (const [platform, arch, libc, name] of expected) {
      assert.equal(platformPackageFor(platform, arch, libc)?.name, name);
    }
    assert.equal(PLATFORM_PACKAGES.length, expected.length);
  });

  test("unsupported platforms map to nothing", () => {
    assert.equal(platformPackageFor("win32", "arm64", undefined), undefined);
    assert.equal(platformPackageFor("freebsd", "x64", undefined), undefined);
    assert.equal(platformPackageFor("linux", "ia32", "gnu"), undefined);
  });
});

describe("resolveBinary", () => {
  const none = { env: {} };

  test("the binary argument wins over the environment", () => {
    const a = installed("blacksparrow-linux-x64-gnu");
    assert.equal(resolveBinary(a.file, { env: { BLACKSPARROW_BINARY: "/does/not/exist" } }), a.file);
  });

  test("BLACKSPARROW_BINARY must be an absolute path of an executable file", () => {
    const a = installed("blacksparrow-linux-x64-gnu");
    assert.equal(resolveBinary(undefined, { env: { BLACKSPARROW_BINARY: a.file } }), a.file);
    for (const bad of ["blacksparrow", "./blacksparrow", "/does/not/exist", root, `${a.file}\0x`]) {
      assert.throws(() => resolveBinary(undefined, { env: { BLACKSPARROW_BINARY: bad } }), { code: "binary_not_found" }, bad);
    }
    if (process.platform !== "win32") {
      const plain = join(root, "plain-file");
      writeFileSync(plain, "x");
      chmodSync(plain, 0o644);
      assert.throws(() => resolveBinary(undefined, { env: { BLACKSPARROW_BINARY: plain } }), { code: "binary_not_found" });
    }
  });

  test("an empty BLACKSPARROW_BINARY is ignored", () => {
    const a = installed("blacksparrow-linux-x64-gnu");
    assert.equal(resolveBinary(undefined, { env: { BLACKSPARROW_BINARY: "" }, platform: "linux", arch: "x64", libc: "gnu", resolve: a.resolve }), a.file);
  });

  test("the installed platform package is used", () => {
    const musl = installed("blacksparrow-linux-arm64-musl");
    assert.equal(resolveBinary(undefined, { ...none, platform: "linux", arch: "arm64", libc: "musl", resolve: musl.resolve }), musl.file);
    const win = installed("blacksparrow-win32-x64", "win32");
    if (process.platform === "win32") {
      assert.equal(resolveBinary(undefined, { ...none, platform: "win32", arch: "x64", resolve: win.resolve }), win.file);
    }
  });

  test("the error names the platform package to install", () => {
    const missing = () => {
      throw Object.assign(new Error("Cannot find module"), { code: "MODULE_NOT_FOUND" });
    };
    assert.throws(
      () => resolveBinary(undefined, { ...none, platform: "linux", arch: "x64", libc: "musl", resolve: missing }),
      (error) => {
        assert.equal(error.code, "binary_not_found");
        assert.match(error.message, /"blacksparrow-linux-x64-musl"/);
        assert.match(error.message, /BLACKSPARROW_BINARY/);
        assert.match(error.message, /musl/);
        assert.equal(error.cause.code, "MODULE_NOT_FOUND");
        return true;
      },
    );
    assert.throws(() => resolveBinary(undefined, { ...none, platform: "darwin", arch: "arm64", resolve: missing }), /"blacksparrow-darwin-arm64"/);
  });

  test("an unsupported platform says so", () => {
    assert.throws(() => resolveBinary(undefined, { ...none, platform: "freebsd", arch: "x64" }), /no prebuilt binary for freebsd x64/);
  });

  test("a package without its binary is reported", () => {
    const directory = join(root, "blacksparrow-darwin-x64");
    mkdirSync(directory, { recursive: true });
    writeFileSync(join(directory, "package.json"), "{}");
    assert.throws(
      () => resolveBinary(undefined, { ...none, platform: "darwin", arch: "x64", resolve: () => join(directory, "package.json") }),
      { code: "binary_not_found" },
    );
  });
});

describe("detectLibc", () => {
  test("answers gnu or musl for this machine", () => {
    assert.ok(["gnu", "musl"].includes(detectLibc()));
    if (process.platform === "linux") {
      const header = process.report.getReport().header;
      assert.equal(detectLibc(), typeof header.glibcVersionRuntime === "string" ? "gnu" : "musl");
    }
  });
});
