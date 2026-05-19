import { UsrlError } from "./errors.js";

export type TokenType =
  | "identifier"
  | "keyword"
  | "string"
  | "number"
  | "punct"
  | "eof";

export interface Token {
  type: TokenType;
  value: string;
  line: number;
  column: number;
}

const KEYWORDS = new Set([
  "namespace",
  "library",
  "using",
  "import",
  "definition",
  "enum",
  "struct",
  "type",
  "template",
  "expand",
  "contract",
  "section",
  "fact",
  "rule",
  "constraint",
  "stage",
  "trigger",
  "extends",
  "derives",
  "as",
  "from",
  "version",
  "metadata",
  "prompt",
  "contract_unit",
]);

const PUNCT = new Set(["{", "}", "(", ")", "[", "]", ";", ":", ",", ".", "=", "@"]);

export function lex(source: string): Token[] {
  const tokens: Token[] = [];
  let i = 0;
  let line = 1;
  let column = 1;

  const push = (type: TokenType, value: string, l = line, c = column) => {
    tokens.push({ type, value, line: l, column: c });
  };

  const advance = (count = 1) => {
    for (let step = 0; step < count; step += 1) {
      if (source[i] === "\n") {
        line += 1;
        column = 1;
      } else {
        column += 1;
      }
      i += 1;
    }
  };

  while (i < source.length) {
    const ch = source[i];

    if (ch === " " || ch === "\t" || ch === "\r" || ch === "\n") {
      advance();
      continue;
    }

    if (ch === "/" && source[i + 1] === "/") {
      while (i < source.length && source[i] !== "\n") {
        advance();
      }
      continue;
    }

    if (ch === "/" && source[i + 1] === "*") {
      advance(2);
      while (i < source.length && !(source[i] === "*" && source[i + 1] === "/")) {
        advance();
      }
      if (i >= source.length) {
        throw new UsrlError("SYNTAX_ERROR", "Unterminated block comment", line, column);
      }
      advance(2);
      continue;
    }

    if (ch === '"') {
      const sl = line;
      const sc = column;
      let value = "";

      if (source.slice(i, i + 3) === '"""') {
        advance(3);
        while (i < source.length && source.slice(i, i + 3) !== '"""') {
          value += source[i];
          advance();
        }
        if (source.slice(i, i + 3) !== '"""') {
          throw new UsrlError("SYNTAX_ERROR", "Unterminated multiline string", sl, sc);
        }
        advance(3);
      } else {
        advance();
        while (i < source.length && source[i] !== '"') {
          if (source[i] === "\\" && i + 1 < source.length) {
            value += source[i];
            advance();
            value += source[i];
            advance();
            continue;
          }
          value += source[i];
          advance();
        }
        if (i >= source.length) {
          throw new UsrlError("SYNTAX_ERROR", "Unterminated string", sl, sc);
        }
        advance();
      }

      push("string", value, sl, sc);
      continue;
    }

    if (/[0-9]/.test(ch)) {
      const sl = line;
      const sc = column;
      let value = "";
      while (i < source.length && /[A-Za-z0-9_.$]/.test(source[i])) {
        value += source[i];
        advance();
      }
      push("number", value, sl, sc);
      continue;
    }

    if (/[A-Za-z_]/.test(ch)) {
      const sl = line;
      const sc = column;
      let value = "";
      while (i < source.length && /[A-Za-z0-9_]/.test(source[i])) {
        value += source[i];
        advance();
      }
      push(KEYWORDS.has(value) ? "keyword" : "identifier", value, sl, sc);
      continue;
    }

    if (PUNCT.has(ch)) {
      push("punct", ch, line, column);
      advance();
      continue;
    }

    if (["?", "+", "-", "*", "%", "!", "<", ">", "|", "&"].includes(ch)) {
      push("punct", ch, line, column);
      advance();
      continue;
    }

    throw new UsrlError("SYNTAX_ERROR", `Unexpected character '${ch}'`, line, column);
  }

  tokens.push({ type: "eof", value: "<eof>", line, column });
  return tokens;
}

export class TokenStream {
  private readonly tokens: Token[];
  private index = 0;

  constructor(tokens: Token[]) {
    this.tokens = tokens;
  }

  peek(offset = 0): Token {
    const idx = this.index + offset;
    return idx >= this.tokens.length ? this.tokens[this.tokens.length - 1] : this.tokens[idx];
  }

  next(): Token {
    const token = this.peek();
    if (this.index < this.tokens.length) {
      this.index += 1;
    }
    return token;
  }

  atEnd(): boolean {
    return this.peek().type === "eof";
  }

  matchValue(value: string): boolean {
    if (this.peek().value === value) {
      this.next();
      return true;
    }
    return false;
  }

  expectValue(value: string, message: string): Token {
    const token = this.peek();
    if (token.value !== value) {
      throw new UsrlError("SYNTAX_ERROR", message, token.line, token.column);
    }
    return this.next();
  }

  expectIdentifier(message: string): Token {
    const token = this.peek();
    if (token.type !== "identifier" && token.type !== "keyword") {
      throw new UsrlError("SYNTAX_ERROR", message, token.line, token.column);
    }
    return this.next();
  }

  consumeToSemicolon(): Token[] {
    const out: Token[] = [];
    let depthParen = 0;
    let depthBracket = 0;
    let depthBrace = 0;

    while (!this.atEnd()) {
      const token = this.peek();
      if (
        token.value === ";" &&
        depthParen === 0 &&
        depthBracket === 0 &&
        depthBrace === 0
      ) {
        this.next();
        return out;
      }
      const consumed = this.next();
      out.push(consumed);
      if (consumed.value === "(") depthParen += 1;
      if (consumed.value === ")") depthParen -= 1;
      if (consumed.value === "[") depthBracket += 1;
      if (consumed.value === "]") depthBracket -= 1;
      if (consumed.value === "{") depthBrace += 1;
      if (consumed.value === "}") depthBrace -= 1;
    }

    const last = out[out.length - 1] ?? this.peek();
    throw new UsrlError("SYNTAX_ERROR", "Expected ';'", last.line, last.column);
  }
}
