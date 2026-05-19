export type UsrlErrorCode =
  | "SYNTAX_ERROR"
  | "SEMANTIC_ERROR"
  | "RESERVED_IDENT"
  | "INVALID_FACT"
  | "TYPE_MISMATCH"
  | "NON_BOOL_CONSTRAINT"
  | "UNBOUND_VARIABLE"
  | "STEP_GAP"
  | "EMPTY_AGG"
  | "ASSERT_FAIL"
  | "AMBIGUOUS"
  | "CIRCULAR_REFERENCE"
  | "INJECTION_PATTERN"
  | "EXECUTION_ERROR";

export class UsrlError extends Error {
  readonly code: UsrlErrorCode;
  readonly line: number;
  readonly column: number;

  constructor(code: UsrlErrorCode, message: string, line = 1, column = 1) {
    super(`${code}: ${message} (${line}:${column})`);
    this.name = "UsrlError";
    this.code = code;
    this.line = line;
    this.column = column;
  }
}
