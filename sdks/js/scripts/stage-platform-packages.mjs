#!/usr/bin/env node
// Turns the cargo-dist release archives into the per-platform npm packages and pins the main
// package's optionalDependencies to the same version.
//
//   node scripts/stage-platform-packages.mjs --artifacts <dir> --version <version> --out <dir>
//
// For each target it needs `blacksparrow-<rust-target>.tar.xz` (`.zip` on Windows) and the
// `.sha256` file next to it, found anywhere under <dir>. The archive's SHA-256 is verified, the
// binary is extracted with the system tools (`tar` for .tar.xz, `unzip` for .zip, falling back to
// `tar -xf`, which reads zip files when it is bsdtar as on macOS and Windows), and one package is
// written per platform into <out>/<package-name> (package.json, README, licenses, bin/). Any
// missing or corrupt archive fails the whole run, and nothing is half-staged silently.
//
// The main package (default: the directory above this script) gets its `version` and every
// optionalDependency set to the exact version. The release workflow then runs
// `npm publish --provenance --access public` in each <out>/<package-name> and finally in the
// main package directory.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync, copyFileSync, existsSync, lstatSync, mkdirSync, mkdtempSync, openSync, closeSync,
  readFileSync, readSync, readdirSync, rmSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

/** Every platform package: the npm name, the Rust target of its archive, and its npm metadata. */
export const PLATFORMS = [
  { name: "blacksparrow-linux-x64-gnu", target: "x86_64-unknown-linux-gnu", os: "linux", cpu: "x64", libc: "glibc" },
  { name: "blacksparrow-linux-arm64-gnu", target: "aarch64-unknown-linux-gnu", os: "linux", cpu: "arm64", libc: "glibc" },
  { name: "blacksparrow-linux-x64-musl", target: "x86_64-unknown-linux-musl", os: "linux", cpu: "x64", libc: "musl" },
  { name: "blacksparrow-linux-arm64-musl", target: "aarch64-unknown-linux-musl", os: "linux", cpu: "arm64", libc: "musl" },
  { name: "blacksparrow-darwin-x64", target: "x86_64-apple-darwin", os: "darwin", cpu: "x64" },
  { name: "blacksparrow-darwin-arm64", target: "aarch64-apple-darwin", os: "darwin", cpu: "arm64" },
  { name: "blacksparrow-win32-x64", target: "x86_64-pc-windows-msvc", os: "win32", cpu: "x64" },
];

const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;
const here = dirname(fileURLToPath(import.meta.url));

class StageError extends Error {}

function archiveName(platform) {
  return `blacksparrow-${platform.target}.${platform.os === "win32" ? "zip" : "tar.xz"}`;
}

function binaryName(platform) {
  return platform.os === "win32" ? "blacksparrow.exe" : "blacksparrow";
}

/** Every file under `root`, without following symlinks. */
function walk(root) {
  const files = [];
  const stack = [root];
  while (stack.length > 0) {
    const directory = stack.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) stack.push(path);
      else files.push(path);
    }
  }
  return files.sort();
}

function sha256Of(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

/** The expected digest from a cargo-dist `.sha256` file: `<hex>  <name>`, `<hex> *<name>` or `<hex>`. */
function expectedDigest(path, archive) {
  const [digest, named] = readFileSync(path, "utf8").trim().split(/\s+/, 2);
  if (!/^[0-9a-fA-F]{64}$/.test(digest ?? "")) {
    throw new StageError(`${path} does not hold a SHA-256 digest`);
  }
  if (named !== undefined && basename(named.replace(/^\*/, "")) !== basename(archive)) {
    throw new StageError(`${path} is the digest of ${named}, not of ${basename(archive)}`);
  }
  return digest.toLowerCase();
}

function run(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8" });
  if (result.error) {
    const error = new StageError(`could not run ${command}: ${result.error.message}`);
    error.missing = result.error.code === "ENOENT";
    throw error;
  }
  if (result.status !== 0) {
    throw new StageError(`${command} ${args.join(" ")} failed (${result.status}): ${result.stderr.trim()}`);
  }
}

function extract(archive, directory) {
  if (archive.endsWith(".zip")) {
    try {
      run("unzip", ["-q", "-o", archive, "-d", directory]);
      return;
    } catch (error) {
      if (!error.missing) throw error;
    }
  }
  run("tar", ["-xf", archive, "-C", directory]);
}

/** Whether the file starts like an executable of the platform: ELF, Mach-O or PE. */
function looksExecutable(path, platform) {
  const header = Buffer.alloc(4);
  const fd = openSync(path, "r");
  try {
    readSync(fd, header, 0, 4, 0);
  } finally {
    closeSync(fd);
  }
  const hex = header.toString("hex");
  if (platform.os === "linux") return hex === "7f454c46";
  if (platform.os === "darwin") return ["cffaedfe", "cefaedfe", "feedfacf", "feedface", "cafebabe"].includes(hex);
  return hex.startsWith("4d5a");
}

function findBinary(directory, platform) {
  const wanted = binaryName(platform);
  const matches = walk(directory).filter((file) => basename(file) === wanted);
  if (matches.length !== 1) {
    throw new StageError(`${archiveName(platform)} holds ${matches.length} files named ${wanted}; expected exactly one`);
  }
  const file = matches[0];
  if (!lstatSync(file).isFile()) throw new StageError(`${wanted} in ${archiveName(platform)} is not a regular file`);
  return file;
}

function platformReadme(platform, version) {
  const libc = platform.libc === undefined ? "" : ` (${platform.libc})`;
  return `# ${platform.name}

The Black Sparrow ${version} binary for ${platform.os} ${platform.cpu}${libc}.

This package is installed automatically as an optional dependency of
[\`blacksparrow\`](https://www.npmjs.com/package/blacksparrow). Install that package instead of
this one.
`;
}

function platformManifest(platform, version, main) {
  const manifest = {
    name: platform.name,
    version,
    description: `Black Sparrow binary for ${platform.os} ${platform.cpu}${platform.libc === undefined ? "" : ` (${platform.libc})`}. Install "blacksparrow" instead.`,
    license: main.license,
    author: main.author,
    homepage: main.homepage,
    bugs: main.bugs,
    repository: { type: main.repository.type, url: main.repository.url },
    os: [platform.os],
    cpu: [platform.cpu],
    ...(platform.libc === undefined ? {} : { libc: [platform.libc] }),
    files: ["bin", "README.md", "LICENSE-MIT", "LICENSE-APACHE"],
    preferUnplugged: true,
    publishConfig: { access: "public" },
  };
  return `${JSON.stringify(manifest, null, 2)}\n`;
}

/**
 * Stages every platform package.
 * @returns {string[]} The names of the staged packages.
 */
export function stage({ artifacts, version, out, main }) {
  if (!SEMVER.test(version)) throw new StageError(`--version ${JSON.stringify(version)} is not a semantic version`);
  const mainManifestPath = join(main, "package.json");
  const mainManifest = JSON.parse(readFileSync(mainManifestPath, "utf8"));
  if (!existsSync(artifacts)) throw new StageError(`--artifacts ${artifacts} does not exist`);

  // Find and verify every archive first, so a missing one stops the run before anything is written.
  const files = walk(artifacts);
  const problems = [];
  const found = [];
  for (const platform of PLATFORMS) {
    const archive = archiveName(platform);
    const archives = files.filter((file) => basename(file) === archive);
    const digests = files.filter((file) => basename(file) === `${archive}.sha256`);
    if (archives.length !== 1) {
      problems.push(`${archive}: found ${archives.length} copies under ${artifacts}, expected one`);
      continue;
    }
    if (digests.length !== 1) {
      problems.push(`${archive}.sha256: found ${digests.length} copies under ${artifacts}, expected one`);
      continue;
    }
    try {
      const expected = expectedDigest(digests[0], archives[0]);
      const actual = sha256Of(archives[0]);
      if (expected !== actual) {
        problems.push(`${archive}: SHA-256 mismatch (expected ${expected}, got ${actual})`);
        continue;
      }
    } catch (error) {
      problems.push(error.message);
      continue;
    }
    found.push({ platform, archive: archives[0] });
  }
  if (problems.length > 0) throw new StageError(`cannot stage ${version}:\n  ${problems.join("\n  ")}`);

  const work = mkdtempSync(join(tmpdir(), "blacksparrow-stage-"));
  try {
    // Extract and check every binary before anything is written to <out>.
    const ready = found.map(({ platform, archive }) => {
      const directory = join(work, platform.target);
      mkdirSync(directory);
      extract(archive, directory);
      const binary = findBinary(directory, platform);
      if (!looksExecutable(binary, platform)) {
        throw new StageError(`${binaryName(platform)} in ${basename(archive)} is not a ${platform.os} executable`);
      }
      return { platform, binary };
    });

    mkdirSync(out, { recursive: true });
    for (const { platform, binary } of ready) {
      const packageDirectory = join(out, platform.name);
      rmSync(packageDirectory, { recursive: true, force: true });
      mkdirSync(join(packageDirectory, "bin"), { recursive: true });
      const target = join(packageDirectory, "bin", binaryName(platform));
      copyFileSync(binary, target);
      chmodSync(target, 0o755);
      writeFileSync(join(packageDirectory, "package.json"), platformManifest(platform, version, mainManifest));
      writeFileSync(join(packageDirectory, "README.md"), platformReadme(platform, version));
      for (const license of ["LICENSE-MIT", "LICENSE-APACHE"]) {
        copyFileSync(join(main, license), join(packageDirectory, license));
      }
    }
  } finally {
    rmSync(work, { recursive: true, force: true });
  }

  mainManifest.version = version;
  mainManifest.optionalDependencies = Object.fromEntries(PLATFORMS.map((platform) => [platform.name, version]).sort());
  writeFileSync(mainManifestPath, `${JSON.stringify(mainManifest, null, 2)}\n`);
  return PLATFORMS.map((platform) => platform.name);
}

function main() {
  const { values } = parseArgs({
    options: {
      artifacts: { type: "string" },
      version: { type: "string" },
      out: { type: "string" },
      main: { type: "string" },
    },
    strict: true,
  });
  for (const required of ["artifacts", "version", "out"]) {
    if (!values[required]) throw new StageError(`--${required} is required`);
  }
  const names = stage({
    artifacts: resolve(values.artifacts),
    version: values.version.replace(/^v/, ""),
    out: resolve(values.out),
    main: resolve(values.main ?? join(here, "..")),
  });
  console.log(`Staged ${names.length} platform packages at ${values.version.replace(/^v/, "")} in ${resolve(values.out)}:`);
  for (const name of names) console.log(`  ${name}`);
}

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof StageError || error?.code === "ERR_PARSE_ARGS_UNKNOWN_OPTION" ? `error: ${error.message}` : error);
    process.exit(1);
  }
}
