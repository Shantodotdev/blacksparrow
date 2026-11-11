import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { closeSync, openSync } from "node:fs";
import { mkdtemp, open, readFile, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BlackSparrowError } from "./errors.js";

const POLL_MS = 100;
const STDERR_TAIL_BYTES = 4096;
const STDERR_MESSAGE_CHARS = 500;
/** The engine writes its own diagnostics here; more than this is a runaway process. */
const MAX_STDERR_BYTES = 32 * 1024 * 1024;
const MAX_STDOUT_BYTES = 1024 * 1024;

const posix = process.platform !== "win32";

/** What to run and how long it may take. */
export interface RunRequest {
  binary: string;
  /** Arguments; `outputPath` is the file the engine may write its document to. */
  args: (outputPath: string) => string[];
  /** Milliseconds until the process is killed. */
  deadlineMs: number;
  /** Largest output file or stdout accepted, in bytes. */
  maxOutputBytes: number;
  env?: Readonly<Record<string, string>> | undefined;
  signal?: AbortSignal | undefined;
}

export interface RunResult {
  /** What the engine wrote to its output file, when it wrote one. */
  file: Buffer | undefined;
  stdout: Buffer;
}

const liveChildren = new Set<ChildProcess>();
let exitHookInstalled = false;

/** Kills every process tree still running when this process exits normally. */
function installExitHook(): void {
  if (exitHookInstalled) return;
  exitHookInstalled = true;
  process.once("exit", () => {
    for (const child of liveChildren) killTree(child);
  });
}

/** Kills the process and everything it started: its process group, or its tree on Windows. */
export function killTree(child: ChildProcess): void {
  const pid = child.pid;
  if (pid === undefined) return;
  if (posix) {
    try {
      process.kill(-pid, "SIGKILL");
    } catch {
      // The group is already gone.
    }
  } else {
    try {
      spawnSync("taskkill", ["/pid", String(pid), "/T", "/F"], {
        stdio: "ignore",
        windowsHide: true,
        timeout: 10_000,
      });
    } catch {
      // taskkill is missing or timed out; the direct kill below is the fallback.
    }
  }
  try {
    child.kill("SIGKILL");
  } catch {
    // Already dead.
  }
}

/** The environment of the engine: PATH, a private HOME and TMPDIR, locale; no inherited secrets. */
export function minimalEnvironment(
  directory: string,
  extra: Readonly<Record<string, string>> | undefined,
  inherited: NodeJS.ProcessEnv = process.env,
): Record<string, string> {
  const environment: Record<string, string> = {};
  const copy = (name: string): void => {
    const value = inherited[name];
    if (value !== undefined && value !== "") environment[name] = value;
  };
  copy("PATH");
  copy("LANG");
  copy("LC_ALL");
  if (posix) {
    environment["HOME"] = directory;
    environment["TMPDIR"] = directory;
  } else {
    for (const name of ["SystemRoot", "windir", "PATHEXT", "ComSpec"]) copy(name);
    environment["USERPROFILE"] = directory;
    environment["HOME"] = directory;
    environment["TEMP"] = directory;
    environment["TMP"] = directory;
    environment["APPDATA"] = directory;
    environment["LOCALAPPDATA"] = directory;
  }
  return { ...environment, ...extra };
}

async function fileSize(path: string): Promise<number> {
  try {
    return (await stat(path)).size;
  } catch {
    return 0;
  }
}

async function tail(path: string, bytes: number): Promise<string> {
  try {
    const handle = await open(path, "r");
    try {
      const { size } = await handle.stat();
      const length = Math.min(size, bytes);
      const buffer = Buffer.alloc(length);
      await handle.read(buffer, 0, length, size - length);
      return buffer.toString("utf8");
    } finally {
      await handle.close();
    }
  } catch {
    return "";
  }
}

type Ending =
  | { kind: "exit"; code: number | null; signal: NodeJS.Signals | null }
  | { kind: "spawn_error"; error: Error }
  | { kind: "timeout" }
  | { kind: "aborted"; reason: unknown }
  | { kind: "too_large"; what: string };

/**
 * Runs the engine in a private directory and returns what it wrote.
 *
 * The process leads its own process group (POSIX) or is killed with its tree (Windows) on a
 * timeout, an abort, an oversized output or any error, so the browsers it started never outlive
 * the call. It gets a minimal environment with HOME and TMPDIR inside the private directory,
 * which is removed afterwards. Output sizes are enforced while it runs.
 *
 * @throws BlackSparrowError On a start failure, a timeout, an abort, a non-zero exit or output
 * over the limit.
 */
export async function runIsolated(request: RunRequest): Promise<RunResult> {
  const { signal } = request;
  if (signal?.aborted) {
    throw new BlackSparrowError("aborted", "Black Sparrow was aborted before it started", {
      cause: signal.reason,
    });
  }

  let directory: string;
  try {
    directory = await mkdtemp(join(tmpdir(), "blacksparrow-"));
  } catch (error) {
    throw new BlackSparrowError("spawn_failed", "Could not create a private temporary directory", {
      cause: error,
    });
  }

  let child: ChildProcess | undefined;
  let ended = false;
  try {
    const outputPath = join(directory, "output.json");
    const stdoutPath = join(directory, "stdout.log");
    const stderrPath = join(directory, "stderr.log");

    const stdoutFd = openSync(stdoutPath, "w");
    const stderrFd = openSync(stderrPath, "w");
    try {
      child = spawn(request.binary, request.args(outputPath), {
        cwd: directory,
        env: minimalEnvironment(directory, request.env),
        stdio: ["ignore", stdoutFd, stderrFd],
        detached: posix,
        windowsHide: true,
      });
    } catch (error) {
      throw new BlackSparrowError("spawn_failed", `Black Sparrow could not start: ${(error as Error).message}`, {
        cause: error,
      });
    } finally {
      closeSync(stdoutFd);
      closeSync(stderrFd);
    }
    installExitHook();
    liveChildren.add(child);
    const started = child;

    const ending = await new Promise<Ending>((resolve) => {
      let settled = false;
      let timer: NodeJS.Timeout | undefined;
      let poll: NodeJS.Timeout | undefined;

      const finish = (value: Ending): void => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        clearTimeout(poll);
        signal?.removeEventListener("abort", onAbort);
        // Whatever ended the run, nothing of the process tree may stay behind. On Windows a
        // process that exited by itself has no tree left to find.
        if (posix || value.kind !== "exit") killTree(started);
        resolve(value);
      };
      const onAbort = (): void => finish({ kind: "aborted", reason: signal?.reason });

      started.once("error", (error) => finish({ kind: "spawn_error", error }));
      started.once("exit", (code, exitSignal) => finish({ kind: "exit", code, signal: exitSignal }));
      timer = setTimeout(() => finish({ kind: "timeout" }), request.deadlineMs);
      signal?.addEventListener("abort", onAbort, { once: true });

      const limits: Array<[string, number, string]> = [
        [outputPath, request.maxOutputBytes, "output"],
        [stdoutPath, Math.min(request.maxOutputBytes, MAX_STDOUT_BYTES), "standard output"],
        [stderrPath, MAX_STDERR_BYTES, "standard error"],
      ];
      const tick = async (): Promise<void> => {
        for (const [path, limit, what] of limits) {
          if (settled) return;
          if ((await fileSize(path)) > limit) {
            finish({ kind: "too_large", what });
            return;
          }
        }
        if (!settled) poll = setTimeout(() => void tick(), POLL_MS);
      };
      poll = setTimeout(() => void tick(), POLL_MS);
    });
    liveChildren.delete(started);
    ended = true;

    const stderr = (await tail(stderrPath, STDERR_TAIL_BYTES)).trim();
    const detail = stderr === "" ? "" : `: ${stderr.slice(-STDERR_MESSAGE_CHARS)}`;
    switch (ending.kind) {
      case "spawn_error":
        throw new BlackSparrowError("spawn_failed", `Black Sparrow could not start: ${ending.error.message}`, {
          cause: ending.error,
        });
      case "timeout":
        throw new BlackSparrowError("timeout", `Black Sparrow did not finish in ${Math.round(request.deadlineMs / 1000)} seconds and was killed${detail}`, {
          stderr,
        });
      case "aborted":
        throw new BlackSparrowError("aborted", "Black Sparrow was aborted", {
          cause: ending.reason,
          stderr,
        });
      case "too_large":
        throw new BlackSparrowError("output_too_large", `Black Sparrow wrote too much ${ending.what} and was killed`, {
          stderr,
        });
      case "exit":
        if (ending.code !== 0) {
          const how =
            ending.code === null ? `was terminated by signal ${ending.signal}` : `exited with code ${ending.code}`;
          throw new BlackSparrowError("exit", `Black Sparrow ${how}${detail}`, {
            exitCode: ending.code,
            signal: ending.signal,
            stderr,
          });
        }
    }

    // A clean exit: read what it wrote, within the limits.
    const [fileSizeBytes, stdoutSize] = await Promise.all([fileSize(outputPath), fileSize(stdoutPath)]);
    if (fileSizeBytes > request.maxOutputBytes || stdoutSize > MAX_STDOUT_BYTES) {
      throw new BlackSparrowError("output_too_large", "Black Sparrow wrote too much output", { stderr });
    }
    const file = fileSizeBytes > 0 ? await readFile(outputPath) : undefined;
    return { file, stdout: await readFile(stdoutPath) };
  } finally {
    if (child !== undefined) {
      liveChildren.delete(child);
      if (!ended) killTree(child);
    }
    await rm(directory, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }).catch(() => undefined);
  }
}
