// How the engine process is run, with a shell script standing in for the engine (POSIX only).
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { describe, test } from "node:test";
import { BlackSparrowError, extract, extractRaw, version } from "../dist/esm/index.js";
import { isolateTemporaryDirectory, fakeEngine, isAlive, leftoverDirectories, VALID_DOCUMENT, waitFor } from "../testlib/support.mjs";

isolateTemporaryDirectory();

const skip = process.platform === "win32" ? "needs a POSIX shell" : false;

function setup(mode) {
  const fake = fakeEngine();
  fake.setDocument(VALID_DOCUMENT);
  return { fake, options: { render: "never", binary: fake.binary, env: fake.env(mode), timeoutSeconds: 5, killGraceSeconds: 0 } };
}

const URL_TO_READ = "https://example.test/";

describe("engine process", { skip }, () => {
  test("a good run returns the validated document and the raw one", async () => {
    const { fake, options } = setup("doc");
    const { result, raw } = await extractRaw(URL_TO_READ, options);
    assert.equal(result.outcome, "ok");
    assert.equal(raw.generator, "blacksparrow fake");
    assert.equal(await version({ binary: fake.binary }), "9.9.9");
  });

  test("command line: options are separate flags and the URL comes after --", async () => {
    const { fake, options } = setup("doc");
    await extract("https://example.test/a?b=1", {
      ...options,
      maxPages: 3,
      allowPrivateNetwork: true,
      userAgent: "-weird agent",
      proxy: "http://user:pw@proxy.test:3128",
      pages: ["https://example.test/contact"],
      chromePath: process.execPath,
      timeoutSeconds: 7,
    });
    const args = fake
      .readLog()
      .split("\n")
      .filter((line) => line.startsWith("arg:"))
      .map((line) => line.slice(4));
    const output = args.find((arg) => arg.startsWith("--output="));
    assert.ok(output);
    assert.deepEqual(args.slice(0, 7), ["extract", "--max-pages", "3", "--render", "never", "--timeout-secs", "7"]);
    assert.ok(args.includes(`--chrome-path=${process.execPath}`));
    assert.ok(args.includes("--allow-private-network"));
    assert.ok(args.includes("--user-agent=-weird agent"));
    assert.ok(args.includes("--proxy=http://user:pw@proxy.test:3128"));
    assert.ok(args.includes("--page=https://example.test/contact"));
    assert.deepEqual(args.slice(-2), ["--", "https://example.test/a?b=1"]);
  });

  test("the engine gets a minimal environment and a private working directory", async () => {
    const { fake, options } = setup("doc");
    process.env.BS_TEST_SECRET = "hunter2";
    process.env.DATABASE_URL = "postgres://secret";
    try {
      const before = new Set(leftoverDirectories());
      await extract(URL_TO_READ, { ...options, env: { ...options.env, EXTRA: "yes" } });
      const log = fake.readLog();
      assert.ok(!log.includes("hunter2") && !log.includes("DATABASE_URL"), log);
      assert.match(log, /^env:PATH=/m);
      assert.match(log, /^env:EXTRA=yes$/m);
      const pwd = /^pwd=(.*)$/m.exec(log)[1];
      assert.equal(/^home=(.*)$/m.exec(log)[1], pwd);
      assert.equal(/^tmpdir=(.*)$/m.exec(log)[1], pwd);
      assert.match(pwd, /blacksparrow-/);
      assert.equal(existsSync(pwd), false, "the private directory is removed");
      assert.deepEqual(leftoverDirectories().filter((name) => !before.has(name)), []);
    } finally {
      delete process.env.BS_TEST_SECRET;
      delete process.env.DATABASE_URL;
    }
  });

  test("abort kills the whole process tree and rejects with code aborted", async () => {
    const { fake, options } = setup("hang");
    const controller = new AbortController();
    const running = extract(URL_TO_READ, { ...options, timeoutSeconds: 60, signal: controller.signal });
    assert.ok(await waitFor(() => fake.readPids().length === 2), "the engine and its child started");
    const pids = fake.readPids();
    assert.ok(pids.every(isAlive));
    controller.abort(new Error("stop"));
    await assert.rejects(running, (error) => {
      assert.ok(error instanceof BlackSparrowError);
      assert.equal(error.code, "aborted");
      assert.equal(error.cause.message, "stop");
      return true;
    });
    assert.ok(await waitFor(() => pids.every((pid) => !isAlive(pid))), `still running: ${pids.filter(isAlive)}`);
  });

  test("an already aborted signal never starts the engine", async () => {
    const { fake, options } = setup("hang");
    await assert.rejects(extract(URL_TO_READ, { ...options, signal: AbortSignal.abort() }), { code: "aborted" });
    assert.deepEqual(fake.readPids(), []);
  });

  test("the timeout kills the whole process tree", async () => {
    const { fake, options } = setup("hang");
    const started = Date.now();
    await assert.rejects(extract(URL_TO_READ, options), { code: "timeout" });
    assert.ok(Date.now() - started < 9000);
    const pids = fake.readPids();
    assert.equal(pids.length, 2);
    assert.ok(await waitFor(() => pids.every((pid) => !isAlive(pid))), `still running: ${pids.filter(isAlive)}`);
  });

  test("output over the limit kills the engine while it runs", async () => {
    const { fake, options } = setup("big");
    const before = new Set(leftoverDirectories());
    const started = Date.now();
    await assert.rejects(extract(URL_TO_READ, { ...options, timeoutSeconds: 60, maxOutputBytes: 2048 }), {
      code: "output_too_large",
    });
    assert.ok(Date.now() - started < 5000, "did not wait for the process to end");
    const pids = fake.readPids();
    assert.ok(await waitFor(() => pids.every((pid) => !isAlive(pid))));
    assert.deepEqual(leftoverDirectories().filter((name) => !before.has(name)), []);
  });

  test("a crash carries the exit code and the end of stderr", async () => {
    const { options } = setup("crash");
    await assert.rejects(extract(URL_TO_READ, options), (error) => {
      assert.equal(error.code, "exit");
      assert.equal(error.exitCode, 3);
      assert.match(error.stderr, /boom: details of the crash/);
      assert.match(error.message, /exited with code 3/);
      return true;
    });
  });

  test("a signalled engine is reported with its signal", async () => {
    const { options } = setup("signal");
    await assert.rejects(extract(URL_TO_READ, options), (error) => {
      assert.equal(error.code, "exit");
      assert.equal(error.signal, "SIGKILL");
      assert.equal(error.exitCode, null);
      return true;
    });
  });

  test("no output, garbage and an unknown schema version are errors", async () => {
    const none = setup("nooutput");
    await assert.rejects(extract(URL_TO_READ, none.options), { code: "no_output" });

    const garbage = setup("doc");
    garbage.fake.setDocument("not json at all");
    await assert.rejects(extract(URL_TO_READ, garbage.options), { code: "invalid_output" });

    const wrong = setup("doc");
    wrong.fake.setDocument('{"schema_version":2,"outcome":"ok"}');
    await assert.rejects(extract(URL_TO_READ, wrong.options), { code: "unsupported_schema_version" });

    const noOutcome = setup("doc");
    noOutcome.fake.setDocument('{"schema_version":1,"outcome":"fine"}');
    await assert.rejects(extract(URL_TO_READ, noOutcome.options), { code: "invalid_output" });
  });

  test("NUL characters are removed from values and keys, escaped backslashes are kept", async () => {
    const { fake, options } = setup("doc");
    fake.setDocument(
      String.raw`{"schema_version":1,"outcome":"ok","start_url":"http://a.test/","generator":"x\u0000y","pages":[{"url":"http://a.test/","kind":"ho\u0000me","markdown":"a\u0000b \\u0000 kept"}],"k\u0000ey":1}`,
    );
    const { result, raw } = await extractRaw(URL_TO_READ, options);
    assert.equal(result.generator, "xy");
    assert.equal(result.pages[0].kind, "home");
    assert.equal(result.pages[0].markdown, "ab \\u0000 kept");
    assert.equal(raw.key, 1);
    assert.deepEqual(Object.keys(raw).sort(), ["generator", "key", "outcome", "pages", "schema_version", "start_url"]);
  });

  test("a binary that cannot start is spawn_failed", async () => {
    const fake = fakeEngine();
    // An executable file the kernel cannot run: no shebang and not a binary.
    const { writeFileSync, chmodSync } = await import("node:fs");
    const bad = `${fake.directory}/bad`;
    writeFileSync(bad, "\u0000\u0001garbage");
    chmodSync(bad, 0o755);
    await assert.rejects(extract(URL_TO_READ, { render: "never", binary: bad, timeoutSeconds: 5 }), (error) => {
      assert.ok(["spawn_failed", "exit"].includes(error.code), error.code);
      return true;
    });
  });

  test("binary options must be absolute paths of executables", async () => {
    await assert.rejects(extract(URL_TO_READ, { render: "never", binary: "relative/blacksparrow" }), {
      code: "binary_not_found",
    });
    await assert.rejects(extract(URL_TO_READ, { render: "never", binary: "/does/not/exist" }), {
      code: "binary_not_found",
    });
    await assert.rejects(extract(URL_TO_READ, { render: "never", binary: "/tmp" }), { code: "binary_not_found" });
  });

  test("invalid options are rejected before anything runs", async () => {
    const { fake, options } = setup("hang");
    for (const bad of [
      { maxPages: 0 },
      { maxPages: 1.5 },
      { maxPages: 9999 },
      { timeoutSeconds: 1 },
      { proxy: "socks5://p.test:1080" },
      { proxy: "not a url" },
      { pages: ["ftp://x.test/"] },
      { userAgent: "line\nbreak" },
      { render: "sometimes" },
      { maxOutputBytes: 1 },
    ]) {
      await assert.rejects(extract(URL_TO_READ, { ...options, ...bad }), { code: "invalid_options" }, JSON.stringify(bad));
    }
    await assert.rejects(extract(URL_TO_READ, undefined), { code: "invalid_options" });
    await assert.rejects(extract(42, options), TypeError);
    assert.deepEqual(fake.readPids(), []);
  });
});
