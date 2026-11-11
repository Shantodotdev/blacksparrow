import { accessSync, constants, readdirSync, statSync } from "node:fs";
import { dirname, isAbsolute, join } from "node:path";
import { BlackSparrowError } from "./errors.js";
import { moduleRequire } from "./require.js";

export type Libc = "gnu" | "musl";

/** A platform the engine is published for, as one npm package. */
export interface PlatformPackage {
  name: string;
  platform: NodeJS.Platform;
  arch: string;
  libc?: Libc;
}

/** Every platform package; the main package lists them as optional dependencies. */
export const PLATFORM_PACKAGES: readonly PlatformPackage[] = [
  { name: "blacksparrow-linux-x64-gnu", platform: "linux", arch: "x64", libc: "gnu" },
  { name: "blacksparrow-linux-arm64-gnu", platform: "linux", arch: "arm64", libc: "gnu" },
  { name: "blacksparrow-linux-x64-musl", platform: "linux", arch: "x64", libc: "musl" },
  { name: "blacksparrow-linux-arm64-musl", platform: "linux", arch: "arm64", libc: "musl" },
  { name: "blacksparrow-darwin-x64", platform: "darwin", arch: "x64" },
  { name: "blacksparrow-darwin-arm64", platform: "darwin", arch: "arm64" },
  { name: "blacksparrow-win32-x64", platform: "win32", arch: "x64" },
];

/** Name of the environment variable that points to the engine binary. */
export const BINARY_ENV = "BLACKSPARROW_BINARY";

let cachedLibc: Libc | undefined;

/** Whether this Linux runs glibc or musl. */
export function detectLibc(): Libc {
  if (cachedLibc !== undefined) return cachedLibc;
  let libc: Libc | undefined;
  const report = process.report as (NodeJS.ProcessReport & { excludeNetwork?: boolean }) | undefined;
  if (report !== undefined && report !== null) {
    const previous = report.excludeNetwork;
    try {
      // The network section of the report is slow to build and not needed here.
      report.excludeNetwork = true;
      const header = (report.getReport() as { header?: { glibcVersionRuntime?: unknown } }).header;
      libc = typeof header?.glibcVersionRuntime === "string" ? "gnu" : "musl";
    } catch {
      libc = undefined;
    } finally {
      try {
        report.excludeNetwork = previous ?? false;
      } catch {
        // The report settings are not writable in this runtime.
      }
    }
  }
  if (libc === undefined) {
    try {
      libc = readdirSync("/lib").some((entry) => entry.startsWith("ld-musl-")) ? "musl" : "gnu";
    } catch {
      libc = "gnu";
    }
  }
  cachedLibc = libc;
  return libc;
}

/** The platform package for a platform, or undefined when the engine is not published for it. */
export function platformPackageFor(
  platform: NodeJS.Platform,
  arch: string,
  libc: Libc | undefined,
): PlatformPackage | undefined {
  return PLATFORM_PACKAGES.find(
    (candidate) =>
      candidate.platform === platform &&
      candidate.arch === arch &&
      (candidate.libc === undefined || candidate.libc === libc),
  );
}

/** Where a binary is looked for, injectable for tests. */
export interface ResolveContext {
  platform?: NodeJS.Platform;
  arch?: string;
  libc?: Libc;
  env?: NodeJS.ProcessEnv;
  resolve?: (specifier: string) => string;
}

/** Checks that `path` is an absolute path of an executable file. */
function assertExecutable(path: string, source: string): string {
  if (path.includes("\0") || !isAbsolute(path)) {
    throw new BlackSparrowError(
      "binary_not_found",
      `${source} must be an absolute path to the blacksparrow executable`,
    );
  }
  try {
    if (!statSync(path).isFile()) {
      throw new BlackSparrowError("binary_not_found", `${source} is not a file: ${path}`);
    }
    if (process.platform !== "win32") accessSync(path, constants.X_OK);
  } catch (error) {
    if (error instanceof BlackSparrowError) throw error;
    throw new BlackSparrowError(
      "binary_not_found",
      `${source} is not an executable file: ${path}`,
      { cause: error },
    );
  }
  return path;
}

/**
 * Finds the engine binary: the `binary` argument, then `BLACKSPARROW_BINARY`, then the installed
 * platform package.
 *
 * @throws BlackSparrowError When none is usable.
 */
export function resolveBinary(explicit?: string, context: ResolveContext = {}): string {
  const env = context.env ?? process.env;
  if (explicit !== undefined) return assertExecutable(explicit, "The binary option");
  const fromEnv = env[BINARY_ENV];
  if (fromEnv !== undefined && fromEnv !== "") {
    return assertExecutable(fromEnv, BINARY_ENV);
  }

  const platform = context.platform ?? process.platform;
  const arch = context.arch ?? process.arch;
  const libc = platform === "linux" ? (context.libc ?? detectLibc()) : undefined;
  const target = platformPackageFor(platform, arch, libc);
  const description = `${platform} ${arch}${libc === undefined ? "" : ` (${libc === "gnu" ? "glibc" : "musl"})`}`;
  if (target === undefined) {
    throw new BlackSparrowError(
      "binary_not_found",
      `Black Sparrow has no prebuilt binary for ${description}. Build it from source and set ${BINARY_ENV} to its absolute path.`,
    );
  }

  const resolve = context.resolve ?? ((specifier: string) => moduleRequire.resolve(specifier));
  let manifest: string;
  try {
    manifest = resolve(`${target.name}/package.json`);
  } catch (error) {
    throw new BlackSparrowError(
      "binary_not_found",
      `The Black Sparrow binary for ${description} is not installed. Install the package "${target.name}" (it is an optional dependency of blacksparrow; do not install with --omit=optional), or set ${BINARY_ENV} to the absolute path of the binary.`,
      { cause: error },
    );
  }
  const file = join(dirname(manifest), "bin", platform === "win32" ? "blacksparrow.exe" : "blacksparrow");
  return assertExecutable(file, `The binary of ${target.name}`);
}
