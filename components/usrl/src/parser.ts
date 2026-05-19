import { CallArg, Expr, MatchCase, Pattern, Program, QuerySpec, Statement, TopDecl, TypeExpr } from "./ast.js";
import { lex, Token, TokenStream } from "./lexer.js";
import { UsrlError } from "./errors.js";

const TOP_LEVEL = new Set([
  "using",
  "import",
  "namespace",
  "library",
  "definition",
  "enum",
  "struct",
  "type",
  "template",
  "expand",
  "contract",
]);

const SECTION_ALIAS = new Set([
  "intent",
  "constraints",
  "capabilities",
  "stages",
  "validation",
  "audit",
  "definitions",
  "triggers",
]);

function readQualifiedName(stream: TokenStream): string {
  const parts: string[] = [];
  parts.push(stream.expectIdentifier("Expected identifier").value);
  while (stream.matchValue(".")) {
    parts.push(stream.expectIdentifier("Expected identifier after '.'").value);
  }
  return parts.join(".");
}

function matchComposite(stream: TokenStream, a: string, b: string): boolean {
  if (stream.peek().value === a && stream.peek(1).value === b) {
    stream.next();
    stream.next();
    return true;
  }
  return false;
}

function expectBlockStart(stream: TokenStream, context: string): void {
  stream.expectValue("{", `Expected '{' ${context}`);
}

function skipBlock(stream: TokenStream): void {
  expectBlockStart(stream, "for block");
  let depth = 1;
  while (!stream.atEnd() && depth > 0) {
    const token = stream.next();
    if (token.value === "{") depth += 1;
    if (token.value === "}") depth -= 1;
  }
  if (depth !== 0) {
    const t = stream.peek();
    throw new UsrlError("SYNTAX_ERROR", "Unterminated block", t.line, t.column);
  }
}

function isExpressionStop(stream: TokenStream, stop: Set<string>): boolean {
  const token = stream.peek();
  return token.type === "eof" || stop.has(token.value);
}

function parsePattern(stream: TokenStream): Pattern {
  if (stream.matchValue("(")) {
    const items: Pattern[] = [];
    if (!stream.matchValue(")")) {
      while (true) {
        items.push(parsePattern(stream));
        if (stream.matchValue(",")) continue;
        stream.expectValue(")", "Expected ')' after tuple pattern");
        break;
      }
    }
    return { kind: "tuple", items };
  }

  if (stream.matchValue("{")) {
    const fields: Array<{ key: string; pattern: Pattern }> = [];
    if (!stream.matchValue("}")) {
      while (true) {
        const key = stream.expectIdentifier("Expected object pattern key").value;
        let pattern: Pattern;
        if (stream.matchValue(":")) {
          pattern = parsePattern(stream);
        } else {
          pattern = { kind: "identifier", name: key };
        }
        fields.push({ key, pattern });
        if (stream.matchValue(",")) continue;
        stream.expectValue("}", "Expected '}' after object pattern");
        break;
      }
    }
    return { kind: "object", fields };
  }

  return { kind: "identifier", name: stream.expectIdentifier("Expected pattern identifier").value };
}

function parseExpression(stream: TokenStream, stop = new Set<string>([";"])): Expr {
  return parseLambda(stream, stop);
}

function parseLambda(stream: TokenStream, stop: Set<string>): Expr {
  const left = parseCoalesce(stream, stop);
  if (stop.has("=")) {
    return left;
  }
  if (matchComposite(stream, "=", ">")) {
    const right = parseExpression(stream, stop);
    return { kind: "lambda", left, right };
  }
  return left;
}

function parseCoalesce(stream: TokenStream, stop: Set<string>): Expr {
  let expr = parseConditional(stream, stop);
  while (!isExpressionStop(stream, stop) && matchComposite(stream, "?", "?")) {
    const right = parseConditional(stream, stop);
    expr = { kind: "coalesce", left: expr, right };
  }
  return expr;
}

function parseConditional(stream: TokenStream, stop: Set<string>): Expr {
  let expr = parseOr(stream, stop);
  if (!isExpressionStop(stream, stop) && stream.peek().value === "?" && stream.peek(1).value !== "." && stream.peek(1).value !== "?") {
    stream.next();
    const trueStop = new Set(stop);
    trueStop.add(":");
    const consequent = parseExpression(stream, trueStop);
    stream.expectValue(":", "Expected ':' in conditional expression");
    const alternate = parseExpression(stream, stop);
    expr = {
      kind: "conditional",
      test: expr,
      consequent,
      alternate,
    };
  }
  return expr;
}

function parseOr(stream: TokenStream, stop: Set<string>): Expr {
  let expr = parseAnd(stream, stop);
  while (!isExpressionStop(stream, stop)) {
    if (stream.peek().value === "or" || matchComposite(stream, "|", "|")) {
      if (stream.peek().value === "or") {
        stream.next();
      }
      const right = parseAnd(stream, stop);
      expr = { kind: "binary", operator: "or", left: expr, right };
      continue;
    }
    break;
  }
  return expr;
}

function parseAnd(stream: TokenStream, stop: Set<string>): Expr {
  let expr = parseComparison(stream, stop);
  while (!isExpressionStop(stream, stop)) {
    if (stream.peek().value === "and" || matchComposite(stream, "&", "&")) {
      if (stream.peek().value === "and") {
        stream.next();
      }
      const right = parseComparison(stream, stop);
      expr = { kind: "binary", operator: "and", left: expr, right };
      continue;
    }
    break;
  }
  return expr;
}

function matchComparisonOperator(stream: TokenStream): string | undefined {
  const one = stream.peek().value;
  const two = `${stream.peek().value}${stream.peek(1).value}`;

  if (["==", "!=", "<=", ">="].includes(two)) {
    stream.next();
    stream.next();
    return two;
  }

  if (["<", ">"].includes(one)) {
    stream.next();
    return one;
  }

  if (["in", "contains", "matches"].includes(one)) {
    stream.next();
    return one;
  }

  return undefined;
}

function parseComparison(stream: TokenStream, stop: Set<string>): Expr {
  let expr = parseRange(stream, stop);
  while (!isExpressionStop(stream, stop)) {
    const op = matchComparisonOperator(stream);
    if (!op) break;
    const right = parseRange(stream, stop);
    expr = { kind: "binary", operator: op, left: expr, right };
  }
  return expr;
}

function parseRange(stream: TokenStream, stop: Set<string>): Expr {
  let expr = parseAdd(stream, stop);
  while (!isExpressionStop(stream, stop) && matchComposite(stream, ".", ".")) {
    const right = parseAdd(stream, stop);
    expr = { kind: "binary", operator: "..", left: expr, right };
  }
  return expr;
}

function parseAdd(stream: TokenStream, stop: Set<string>): Expr {
  let expr = parseMul(stream, stop);
  while (!isExpressionStop(stream, stop)) {
    const op = stream.peek().value;
    if (op !== "+" && op !== "-") break;
    stream.next();
    const right = parseMul(stream, stop);
    expr = { kind: "binary", operator: op, left: expr, right };
  }
  return expr;
}

function parseMul(stream: TokenStream, stop: Set<string>): Expr {
  let expr = parseUnary(stream, stop);
  while (!isExpressionStop(stream, stop)) {
    const op = stream.peek().value;
    if (op !== "*" && op !== "/" && op !== "%") break;
    stream.next();
    const right = parseUnary(stream, stop);
    expr = { kind: "binary", operator: op, left: expr, right };
  }
  return expr;
}

function parseUnary(stream: TokenStream, stop: Set<string>): Expr {
  const op = stream.peek().value;
  if (op === "not" || op === "!" || op === "-" || op === "+") {
    stream.next();
    return { kind: "unary", operator: op, right: parseUnary(stream, stop) };
  }
  return parsePostfix(stream, stop);
}

function parseDelimitedExpressions(stream: TokenStream, endToken: string): Expr[] {
  const out: Expr[] = [];
  if (stream.matchValue(endToken)) return out;

  while (true) {
    out.push(parseExpression(stream, new Set([",", endToken])));
    if (stream.matchValue(",")) continue;
    stream.expectValue(endToken, `Expected '${endToken}'`);
    break;
  }

  return out;
}

function parseObjectLiteral(stream: TokenStream): Expr {
  const fields: Array<{ key: string; value: Expr }> = [];
  if (stream.matchValue("}")) {
    return { kind: "object", fields };
  }

  while (true) {
    const keyToken = stream.peek();
    if (keyToken.type !== "identifier" && keyToken.type !== "keyword" && keyToken.type !== "string") {
      throw new UsrlError("SYNTAX_ERROR", "Expected object key", keyToken.line, keyToken.column);
    }
    const key = stream.next().value;
    stream.expectValue(":", "Expected ':' in object literal");
    const value = parseExpression(stream, new Set([",", "}"]));
    fields.push({ key, value });

    if (stream.matchValue(",")) continue;
    stream.expectValue("}", "Expected '}' after object literal");
    break;
  }

  return { kind: "object", fields };
}

function parseSetLiteral(stream: TokenStream): Expr {
  const elements = parseDelimitedExpressions(stream, "}");
  return { kind: "set", elements };
}

function parseMatchExpr(stream: TokenStream, stop: Set<string>): Expr {
  const matchExpr = parseExpression(stream, new Set(["{"]));
  stream.expectValue("{", "Expected '{' in match expression");
  const cases: MatchCase[] = [];

  while (!stream.matchValue("}")) {
    stream.expectValue("case", "Expected 'case' in match expression");
    const pattern = parsePattern(stream);
    let guard: Expr | undefined;
    if (stream.matchValue("when")) {
      guard = parseExpression(stream, new Set(["="]));
    }
    if (!matchComposite(stream, "=", ">")) {
      const t = stream.peek();
      throw new UsrlError("SYNTAX_ERROR", "Expected '=>' in match case", t.line, t.column);
    }
    const value = parseExpression(stream, new Set([";"]));
    stream.expectValue(";", "Expected ';' after match case");
    cases.push({ pattern, guard, value });
  }

  return { kind: "match", matchExpr, cases };
}

function parseArrayOrComprehension(stream: TokenStream): Expr {
  if (stream.matchValue("]")) {
    return { kind: "array", elements: [] };
  }

  const first = parseExpression(stream, new Set([",", "]", "for"]));
  if (stream.matchValue("for")) {
    const pattern = parsePattern(stream);
    stream.expectValue("in", "Expected 'in' in comprehension");
    const iterable = parseExpression(stream, new Set(["if", "]"]));
    let filter: Expr | undefined;
    if (stream.matchValue("if")) {
      filter = parseExpression(stream, new Set(["]"]));
    }
    stream.expectValue("]", "Expected ']' to close comprehension");
    return { kind: "comprehension", itemExpr: first, pattern, iterable, filter };
  }

  const elements: Expr[] = [first];
  while (stream.matchValue(",")) {
    elements.push(parseExpression(stream, new Set([",", "]"])));
  }
  stream.expectValue("]", "Expected ']' after array literal");
  return { kind: "array", elements };
}

function parseNewExpr(stream: TokenStream): Expr {
  const typeParts: string[] = [];
  const first = stream.expectIdentifier("Expected type name after 'new'");
  typeParts.push(first.value);
  while (stream.matchValue(".")) {
    typeParts.push(".");
    typeParts.push(stream.expectIdentifier("Expected type segment").value);
  }
  const typeName = typeParts.join("");

  if (stream.matchValue("(")) {
    const args = parseDelimitedExpressions(stream, ")");
    return { kind: "new", typeName, args };
  }

  if (stream.matchValue("{")) {
    const fields: Array<{ key: string; value: Expr }> = [];
    if (stream.matchValue("}")) {
      return { kind: "new", typeName, fields };
    }
    while (true) {
      const key = stream.expectIdentifier("Expected field name in object constructor").value;
      if (stream.matchValue("=") || stream.matchValue(":")) {
        // accepted
      } else {
        const t = stream.peek();
        throw new UsrlError("SYNTAX_ERROR", "Expected '=' or ':' in object constructor", t.line, t.column);
      }
      const value = parseExpression(stream, new Set([",", "}"]));
      fields.push({ key, value });
      if (stream.matchValue(",")) continue;
      stream.expectValue("}", "Expected '}' in object constructor");
      break;
    }
    return { kind: "new", typeName, fields };
  }

  return { kind: "new", typeName };
}

function parsePrimary(stream: TokenStream, stop: Set<string>): Expr {
  const token = stream.peek();
  if (isExpressionStop(stream, stop)) {
    throw new UsrlError("SYNTAX_ERROR", "Expected expression", token.line, token.column);
  }

  if (token.value === "(") {
    stream.next();
    const expr = parseExpression(stream, new Set([")"]));
    stream.expectValue(")", "Expected ')' after expression");
    return expr;
  }

  if (token.value === "[") {
    stream.next();
    return parseArrayOrComprehension(stream);
  }

  if (token.value === "{") {
    stream.next();
    const next = stream.peek();
    const next2 = stream.peek(1);
    const maybeObjectKey = (next.type === "identifier" || next.type === "keyword" || next.type === "string") && next2.value === ":";
    return maybeObjectKey ? parseObjectLiteral(stream) : parseSetLiteral(stream);
  }

  if (token.value === "new") {
    stream.next();
    return parseNewExpr(stream);
  }

  if (token.value === "match") {
    stream.next();
    return parseMatchExpr(stream, stop);
  }

  if (token.type === "string") {
    stream.next();
    return { kind: "literal", value: token.value };
  }

  if (token.type === "number") {
    stream.next();
    const num = Number(token.value);
    return { kind: "literal", value: Number.isNaN(num) ? token.value : num };
  }

  if (token.value === "true" || token.value === "false") {
    stream.next();
    return { kind: "literal", value: token.value === "true" };
  }

  if (token.value === "null") {
    stream.next();
    return { kind: "literal", value: null };
  }

  if (token.value === "unknown") {
    stream.next();
    return { kind: "literal", value: "unknown" };
  }

  if (token.type === "identifier" || token.type === "keyword") {
    stream.next();
    return { kind: "identifier", name: token.value };
  }

  throw new UsrlError("SYNTAX_ERROR", `Unexpected token in expression: ${token.value}`, token.line, token.column);
}

function parseCallArgs(stream: TokenStream): CallArg[] {
  const args: CallArg[] = [];
  if (stream.matchValue(")")) {
    return args;
  }

  while (true) {
    if ((stream.peek().type === "identifier" || stream.peek().type === "keyword") && stream.peek(1).value === ":") {
      const name = stream.next().value;
      stream.next();
      const value = parseExpression(stream, new Set([",", ")"]));
      args.push({ name, value });
    } else {
      const value = parseExpression(stream, new Set([",", ")"]));
      args.push({ value });
    }

    if (stream.matchValue(",")) continue;
    stream.expectValue(")", "Expected ')' after call arguments");
    break;
  }

  return args;
}

function parsePostfix(stream: TokenStream, stop: Set<string>): Expr {
  let expr = parsePrimary(stream, stop);

  while (!isExpressionStop(stream, stop)) {
    if (matchComposite(stream, "?", ".")) {
      const property = stream.expectIdentifier("Expected property name after '?.'").value;
      expr = { kind: "safe_member", object: expr, property };
      continue;
    }

    if (stream.matchValue(".")) {
      const property = stream.expectIdentifier("Expected property name after '.'").value;
      expr = { kind: "member", object: expr, property };
      continue;
    }

    if (stream.matchValue("(")) {
      const callArgs = parseCallArgs(stream);
      expr = {
        kind: "call",
        callee: expr,
        callArgs,
        args: callArgs.map((a) => a.value),
      };
      continue;
    }

    if (stream.matchValue("[")) {
      const index = parseExpression(stream, new Set(["]"]));
      stream.expectValue("]", "Expected ']' after index expression");
      expr = { kind: "index", object: expr, index };
      continue;
    }

    break;
  }

  return expr;
}

function parseQuery(stream: TokenStream, startLine: number, startCol: number): Statement {
  expectBlockStart(stream, "after query");
  const query: QuerySpec = {};

  while (!stream.matchValue("}")) {
    const key = stream.expectIdentifier("Expected query item key").value;
    stream.expectValue(":", "Expected ':' after query key");
    const value = parseExpression(stream, new Set([";"]));
    stream.expectValue(";", "Expected ';' after query item");

    if (key === "target") query.target = value;
    else if (key === "where") query.where = value;
    else if (key === "constraints") query.constraints = value;
    else if (key === "response_format") query.response_format = value;
    else if (key === "hint") query.hint = value;
  }

  return { kind: "query", query, loc: { line: startLine, column: startCol } };
}

function parseParamList(stream: TokenStream): Array<{ type?: string; name: string; defaultValue?: Expr }> {
  const params: Array<{ type?: string; name: string; defaultValue?: Expr }> = [];
  stream.expectValue("(", "Expected '(' for parameter list");
  if (stream.matchValue(")")) {
    return params;
  }

  while (true) {
    const first = stream.expectIdentifier("Expected parameter identifier").value;
    let type: string | undefined;
    let name = first;

    if ((stream.peek().type === "identifier" || stream.peek().type === "keyword") && stream.peek().value !== ":") {
      type = first;
      name = stream.next().value;
    }

    let defaultValue: Expr | undefined;
    if (stream.matchValue(":")) {
      defaultValue = parseExpression(stream, new Set([",", ")"]));
    }

    params.push({ type, name, defaultValue });

    if (stream.matchValue(",")) continue;
    stream.expectValue(")", "Expected ')' after parameter list");
    break;
  }

  return params;
}

function parseRuleHints(stream: TokenStream): Expr[] {
  const hints: Expr[] = [];
  while (stream.matchValue("hint")) {
    const value = parseExpression(stream, new Set([";"]));
    stream.expectValue(";", "Expected ';' after hint");
    hints.push(value);
  }
  return hints;
}

function parseTypeRef(stream: TokenStream): TypeExpr {
  const parts: string[] = [];
  parts.push(stream.expectIdentifier("Expected type identifier").value);
  while (stream.matchValue(".")) {
    parts.push(".");
    parts.push(stream.expectIdentifier("Expected type segment").value);
  }

  const qname = parts.join("");
  const args: TypeExpr[] = [];
  if (stream.matchValue("<")) {
    if (!stream.matchValue(">")) {
      while (true) {
        args.push(parseTypeExpr(stream, new Set([",", ">"])));
        if (stream.matchValue(",")) continue;
        stream.expectValue(">", "Expected '>' after type arguments");
        break;
      }
    }
  }

  let ref: TypeExpr = { kind: "ref", qname, args };
  while (stream.matchValue("[")) {
    stream.expectValue("]", "Expected ']' after '[' in array type");
    ref = { kind: "ref", qname: "array", elementType: ref };
  }

  return ref;
}

function parseTypeObjectBody(stream: TokenStream): TypeExpr {
  stream.expectValue("{", "Expected '{' for object type body");
  const fields: Array<{ key: string; type: TypeExpr }> = [];
  while (!stream.matchValue("}")) {
    const key = stream.expectIdentifier("Expected object type field key").value;
    stream.expectValue(":", "Expected ':' in object type field");
    const type = parseTypeExpr(stream, new Set([";", ",", "}"]));
    fields.push({ key, type });
    stream.matchValue(";");
    stream.matchValue(",");
  }
  return { kind: "object", fields };
}

function parseTypeVariantOrPrimary(stream: TokenStream, stop: Set<string>): TypeExpr {
  const start = stream.peek();
  if ((start.type === "identifier" || start.type === "keyword") && stream.peek(1).value === "{") {
    const name = stream.next().value;
    const body = parseTypeObjectBody(stream);
    return { kind: "variant", name, fields: body.fields };
  }

  if (start.value === "{") {
    return parseTypeObjectBody(stream);
  }

  const ref = parseTypeRef(stream);
  if (isExpressionStop(stream, stop)) {
    return ref;
  }

  return ref;
}

function parseTypeExpr(stream: TokenStream, stop = new Set<string>([";"])): TypeExpr {
  const variants: TypeExpr[] = [];
  if (stream.matchValue("|")) {
    variants.push(parseTypeVariantOrPrimary(stream, new Set(["|", ...stop])));
    while (stream.matchValue("|")) {
      variants.push(parseTypeVariantOrPrimary(stream, new Set(["|", ...stop])));
    }
    return { kind: "union", variants };
  }

  let first = parseTypeVariantOrPrimary(stream, new Set(["|", ...stop]));
  if (stream.matchValue("|")) {
    variants.push(first);
    do {
      variants.push(parseTypeVariantOrPrimary(stream, new Set(["|", ...stop])));
    } while (stream.matchValue("|"));
    first = { kind: "union", variants };
  }
  return first;
}

function parseTypeParams(stream: TokenStream): string[] {
  const params: string[] = [];
  if (!stream.matchValue("<")) {
    return params;
  }
  if (stream.matchValue(">")) {
    return params;
  }
  while (true) {
    params.push(stream.expectIdentifier("Expected type parameter").value);
    if (stream.matchValue(",")) continue;
    stream.expectValue(">", "Expected '>' after type parameters");
    break;
  }
  return params;
}

function parseStructFields(stream: TokenStream): Array<{ name: string; type: TypeExpr }> {
  const fields: Array<{ name: string; type: TypeExpr }> = [];
  stream.expectValue("{", "Expected '{' for struct body");
  while (!stream.matchValue("}")) {
    const type = parseTypeRef(stream);
    const name = stream.expectIdentifier("Expected struct field name").value;
    stream.expectValue(";", "Expected ';' after struct field");
    fields.push({ name, type });
  }
  return fields;
}

function parseStatementsBlock(stream: TokenStream): Statement[] {
  expectBlockStart(stream, "for block");
  const out: Statement[] = [];

  while (!stream.matchValue("}")) {
    const token = stream.peek();
    if (token.type === "eof") {
      throw new UsrlError("SYNTAX_ERROR", "Unterminated block", token.line, token.column);
    }

    if (token.value === "section" || SECTION_ALIAS.has(token.value)) {
      const start = stream.next();
      const sectionName = start.value === "section"
        ? stream.expectIdentifier("Expected section name").value
        : start.value;
      const body = parseStatementsBlock(stream);
      out.push({ kind: "section", name: sectionName, body, loc: { line: start.line, column: start.column } });
      continue;
    }

    if (token.value === "fact") {
      const start = stream.next();
      const name = stream.expectIdentifier("Expected fact identifier").value;
      stream.expectValue("=", "Expected '=' in fact declaration");
      let expr = parseExpression(stream, new Set([";", "@"]));
      if (stream.matchValue("@")) {
        const sourceExpr = parseExpression(stream, new Set([";", "[", "with"]));
        let time: Expr | undefined;
        let confidence: Expr | undefined;
        if (stream.matchValue("[")) {
          time = parseExpression(stream, new Set(["]"]));
          stream.expectValue("]", "Expected ']' after time qualifier");
        }
        if (stream.matchValue("with")) {
          stream.expectValue("confidence", "Expected 'confidence' after 'with'");
          confidence = parseExpression(stream, new Set([";"]));
        }
        expr = { kind: "provenance", left: expr, source: sourceExpr, time, confidence };
      }
      stream.expectValue(";", "Expected ';' after fact declaration");
      out.push({ kind: "fact", name, expr, loc: { line: start.line, column: start.column } });
      continue;
    }

    if (token.value === "let") {
      const start = stream.next();
      const pattern = parsePattern(stream);
      stream.expectValue("=", "Expected '=' in let statement");
      const expr = parseExpression(stream, new Set([";"]));
      stream.expectValue(";", "Expected ';' after let statement");
      out.push({ kind: "let", pattern, expr, loc: { line: start.line, column: start.column } });
      continue;
    }

    if (
      token.value === "assert" ||
      token.value === "require" ||
      token.value === "permit" ||
      token.value === "deny" ||
      token.value === "emit" ||
      token.value === "return"
    ) {
      const start = stream.next();
      let expr: Expr | undefined;
      if (start.value === "return" && stream.peek().value === ";") {
        expr = undefined;
      } else {
        expr = parseExpression(stream, new Set([";"]));
      }
      stream.expectValue(";", `Expected ';' after ${start.value}`);
      out.push({ kind: start.value as Statement["kind"], expr, loc: { line: start.line, column: start.column } });
      continue;
    }

    if (token.value === "query") {
      const start = stream.next();
      out.push(parseQuery(stream, start.line, start.column));
      continue;
    }

    if (token.value === "if") {
      const start = stream.next();
      stream.expectValue("(", "Expected '(' after if");
      const condExpr = parseExpression(stream, new Set([")"]));
      stream.expectValue(")", "Expected ')' after if condition");
      const body = parseStatementsBlock(stream);
      let elseBody: Statement[] | undefined;
      if (stream.matchValue("else")) {
        elseBody = parseStatementsBlock(stream);
      }
      out.push({ kind: "if", expr: condExpr, body, elseBody, loc: { line: start.line, column: start.column } });
      continue;
    }

    if (token.value === "when") {
      const start = stream.next();
      let condExpr: Expr;
      if (stream.matchValue("(")) {
        condExpr = parseExpression(stream, new Set([")"]));
        stream.expectValue(")", "Expected ')' after when condition");
      } else {
        condExpr = parseExpression(stream, new Set(["{"]));
      }
      const body = parseStatementsBlock(stream);
      out.push({ kind: "when", expr: condExpr, body, loc: { line: start.line, column: start.column } });
      continue;
    }

    if (token.value === "foreach") {
      const start = stream.next();
      stream.expectValue("(", "Expected '(' after foreach");
      const pattern = parsePattern(stream);
      stream.expectValue("in", "Expected 'in' in foreach statement");
      const iterable = parseExpression(stream, new Set([")"]));
      stream.expectValue(")", "Expected ')' after foreach clause");
      const body = parseStatementsBlock(stream);
      out.push({ kind: "foreach", pattern, iterable, body, loc: { line: start.line, column: start.column } });
      continue;
    }

    if (token.value === "parallel" || token.value === "sequence") {
      const start = stream.next();
      const body = parseStatementsBlock(stream);
      out.push({ kind: start.value as Statement["kind"], body, loc: { line: start.line, column: start.column } });
      continue;
    }

    if (token.value === "rule" || token.value === "constraint" || token.value === "stage" || token.value === "trigger") {
      const start = stream.next();
      const name = stream.expectIdentifier(`Expected ${start.value} identifier`).value;
      let params: Array<{ type?: string; name: string; defaultValue?: Expr }> | undefined;
      let hints: Expr[] | undefined;
      if (start.value === "rule") {
        if (stream.peek().value === "(") {
          params = parseParamList(stream);
        }
        const parsedHints = parseRuleHints(stream);
        if (parsedHints.length > 0) {
          hints = parsedHints;
        }
      } else {
        while (!stream.atEnd() && stream.peek().value !== "{") {
          stream.next();
        }
      }
      const body = parseStatementsBlock(stream);
      out.push({ kind: start.value as Statement["kind"], name, params, hints, body, loc: { line: start.line, column: start.column } });
      continue;
    }

    const start = stream.peek();
    const expr = parseExpression(stream, new Set([";"]));
    stream.expectValue(";", "Expected ';' after expression statement");
    out.push({ kind: "expr", expr, loc: { line: start.line, column: start.column } });
  }

  return out;
}

function parseImportDecl(stream: TokenStream, start: Token): TopDecl {
  let name = "";
  if (stream.peek().type === "string") {
    name = stream.next().value;
    if (stream.matchValue("as")) {
      name += ` as ${stream.expectIdentifier("Expected alias").value}`;
    }
  } else if (stream.matchValue("{")) {
    const names: string[] = [];
    while (!stream.matchValue("}")) {
      names.push(stream.expectIdentifier("Expected import name").value);
      stream.matchValue(",");
    }
    stream.expectValue("from", "Expected 'from' in selective import");
    const from = stream.peek();
    if (from.type !== "string") {
      throw new UsrlError("SYNTAX_ERROR", "Expected import path string", from.line, from.column);
    }
    name = `{${names.join(",")}} from ${stream.next().value}`;
  } else {
    const bad = stream.peek();
    throw new UsrlError("SYNTAX_ERROR", "Invalid import declaration", bad.line, bad.column);
  }
  stream.expectValue(";", "Expected ';' after import");
  return { kind: "import", name, loc: { line: start.line, column: start.column } };
}

function parseTopDecl(stream: TokenStream): TopDecl {
  const token = stream.peek();
  if (!TOP_LEVEL.has(token.value)) {
    throw new UsrlError("SYNTAX_ERROR", `Unexpected token '${token.value}' at top level`, token.line, token.column);
  }

  const start = stream.next();

  if (start.value === "using") {
    const name = readQualifiedName(stream);
    stream.expectValue(";", "Expected ';' after using");
    return { kind: "using", name, loc: { line: start.line, column: start.column } };
  }

  if (start.value === "import") {
    return parseImportDecl(stream, start);
  }

  if (start.value === "namespace") {
    const name = readQualifiedName(stream);
    expectBlockStart(stream, "after namespace name");
    const declarations: TopDecl[] = [];
    while (!stream.matchValue("}")) {
      const t = stream.peek();
      if (t.type === "eof") {
        throw new UsrlError("SYNTAX_ERROR", "Unterminated namespace block", t.line, t.column);
      }
      declarations.push(parseTopDecl(stream));
    }
    return { kind: "namespace", name, declarations, loc: { line: start.line, column: start.column } };
  }

  if (start.value === "expand") {
    const name = stream.expectIdentifier("Expected template name after expand").value;
    stream.expectValue("(", "Expected '(' after expand name");
    parseDelimitedExpressions(stream, ")");
    stream.expectValue(";", "Expected ';' after expand");
    return { kind: "expand", name, loc: { line: start.line, column: start.column } };
  }

  const name = stream.expectIdentifier(`Expected ${start.value} name`).value;

  if (start.value === "type") {
    const typeParams = parseTypeParams(stream);
    stream.expectValue("=", "Expected '=' in type declaration");
    const typeExpr = parseTypeExpr(stream, new Set([";"]));
    stream.expectValue(";", "Expected ';' after type declaration");
    return { kind: "type", name, typeParams, typeExpr, loc: { line: start.line, column: start.column } };
  }

  if (start.value === "contract" && (stream.peek().value === "extends" || stream.peek().value === "derives")) {
    stream.next();
    readQualifiedName(stream);
  }

  if (start.value === "template") {
    parseParamList(stream);
  }

  if (start.value === "enum") {
    skipBlock(stream);
    return { kind: "enum", name, loc: { line: start.line, column: start.column } };
  }

  if (start.value === "struct") {
    const structFields = parseStructFields(stream);
    return { kind: "struct", name, structFields, loc: { line: start.line, column: start.column } };
  }

  const body = parseStatementsBlock(stream);
  return { kind: start.value as TopDecl["kind"], name, body, loc: { line: start.line, column: start.column } };
}

export function parseUsrl(source: string): Program {
  const stream = new TokenStream(lex(source));
  const declarations: TopDecl[] = [];

  while (!stream.atEnd()) {
    if (stream.peek().type === "eof") break;
    declarations.push(parseTopDecl(stream));
  }

  return { declarations };
}
