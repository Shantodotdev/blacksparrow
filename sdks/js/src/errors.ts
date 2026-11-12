/** Why a Black Sparrow call failed. */
export type BlackSparrowErrorCode =
  | "invalid_options"
  | "binary_not_found"
  | "spawn_failed"
  | "timeout"
  | "aborted"
  | "exit"
  | "output_too_large"
  | "no_output"
  | "invalid_output"
  | "unsupported_schema_version";

export interface BlackSparrowErrorDetails {
  cause?: unknown;
  /** Exit code of the engine, when it exited by itself. */
  exitCode?: number | null;
  /** Signal that ended the engine, when it was signalled. */
  signal?: string | null;
  /** The last bytes the engine wrote to its standard error. */
  stderr?: string;
}

/**
 * The engine did not produce a usable result: it could not start, crashed, hung, was aborted,
 * or printed an unusable document. A site that cannot be read is not this error; it is an
 * `outcome` of the result.
 */
export class BlackSparrowError extends Error {
  readonly code: BlackSparrowErrorCode;
  readonly exitCode: number | null | undefined;
  readonly signal: string | null | undefined;
  readonly stderr: string | undefined;

  constructor(code: BlackSparrowErrorCode, message: string, details: BlackSparrowErrorDetails = {}) {
    super(message, details.cause === undefined ? undefined : { cause: details.cause });
    this.name = "BlackSparrowError";
    this.code = code;
    this.exitCode = details.exitCode;
    this.signal = details.signal;
    this.stderr = details.stderr;
  }
}
