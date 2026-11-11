#!/usr/bin/env node
import { spawn } from "node:child_process";
import { BlackSparrowError } from "./errors.js";
import { resolveBinary } from "./binary.js";

/** Runs the installed engine with this process's arguments, so `npx blacksparrow extract ...` works. */
function main(): void {
  let binary: string;
  try {
    binary = resolveBinary();
  } catch (error) {
    process.stderr.write(`${error instanceof BlackSparrowError ? error.message : String(error)}\n`);
    process.exit(127);
  }

  const child = spawn(binary, process.argv.slice(2), { stdio: "inherit", windowsHide: true });
  const forward = (signal: NodeJS.Signals): void => {
    child.kill(signal);
  };
  process.on("SIGINT", forward);
  process.on("SIGTERM", forward);
  child.once("error", (error) => {
    process.stderr.write(`Black Sparrow could not start: ${error.message}\n`);
    process.exit(127);
  });
  child.once("exit", (code, signal) => {
    if (signal !== null) {
      process.removeListener(signal, forward);
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code ?? 1);
  });
}

main();
