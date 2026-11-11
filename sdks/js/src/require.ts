import { createRequire } from "node:module";

/** `require` of this package's own location, to resolve the installed platform package. */
export const moduleRequire: NodeJS.Require = createRequire(import.meta.url);
