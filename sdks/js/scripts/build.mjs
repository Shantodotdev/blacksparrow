// Builds the package: ESM into dist/esm, CommonJS into dist/cjs, and declarations for both.
// The two builds differ only in src/require.ts, which needs import.meta.url in ESM and plain
// `require` in CommonJS.
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const tsc = createRequire(import.meta.url).resolve("typescript/bin/tsc");

function run(args) {
  const result = spawnSync(process.execPath, [tsc, ...args], { cwd: root, stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
}

rmSync(join(root, "dist"), { recursive: true, force: true });
rmSync(join(root, ".build"), { recursive: true, force: true });

run(["-p", "tsconfig.json"]);

cpSync(join(root, "src"), join(root, ".build/cjs"), { recursive: true });
writeFileSync(
  join(root, ".build/cjs/require.ts"),
  "/** `require` of this package's own location, to resolve the installed platform package. */\n" +
    "export const moduleRequire: NodeJS.Require = require;\n",
);
run(["-p", "tsconfig.cjs.json"]);
mkdirSync(join(root, "dist/cjs"), { recursive: true });
writeFileSync(join(root, "dist/cjs/package.json"), '{ "type": "commonjs" }\n');
rmSync(join(root, ".build"), { recursive: true, force: true });
