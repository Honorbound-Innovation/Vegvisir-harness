import { Expr, Program, Statement, TopDecl, TypeExpr } from "./ast.js";
import { UsrlError } from "./errors.js";

const RESERVED = new Set([
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
]);

const BUILTIN_TYPE_ARITY = new Map<string, number>([
  ["set", 1],
  ["map", 2],
  ["range", 1],
  ["optional", 1],
  ["array", 1],
]);

const PRIMITIVE_TYPES = new Set(["bool", "string", "number", "date", "time", "unknown"]);

export interface ValidationIssue {
  code: string;
  message: string;
  line: number;
  column: number;
}

interface DeclContext {
  decls: TopDecl[];
  typeArity: Map<string, number>;
  structs: Map<string, Set<string>>;
}

function flattenDecls(decls: TopDecl[]): TopDecl[] {
  const out: TopDecl[] = [];
  for (const decl of decls) {
    out.push(decl);
    if (decl.declarations) {
      out.push(...flattenDecls(decl.declarations));
    }
  }
  return out;
}

function buildContext(program: Program): DeclContext {
  const decls = flattenDecls(program.declarations);
  const typeArity = new Map<string, number>();
  const structs = new Map<string, Set<string>>();

  for (const [name, arity] of BUILTIN_TYPE_ARITY.entries()) {
    typeArity.set(name, arity);
  }
  for (const primitive of PRIMITIVE_TYPES) {
    typeArity.set(primitive, 0);
  }

  for (const decl of decls) {
    if (!decl.name) continue;
    if (decl.kind === "type") {
      typeArity.set(decl.name, decl.typeParams?.length ?? 0);
    }
    if (decl.kind === "struct") {
      typeArity.set(decl.name, 0);
      structs.set(decl.name, new Set((decl.structFields ?? []).map((f) => f.name)));
    }
  }

  return { decls, typeArity, structs };
}

function normalizeTypeName(name: string): string {
  return name.includes(".") ? name.split(".").at(-1) ?? name : name;
}

function validateTypeExpr(
  expr: TypeExpr | undefined,
  allowedTypeParams: Set<string>,
  ctx: DeclContext,
  issues: ValidationIssue[],
  line: number,
  column: number,
): void {
  if (!expr) return;

  if (expr.kind === "union") {
    for (const variant of expr.variants ?? []) {
      validateTypeExpr(variant, allowedTypeParams, ctx, issues, line, column);
    }
    return;
  }

  if (expr.kind === "variant") {
    for (const field of expr.fields ?? []) {
      validateTypeExpr(field.type, allowedTypeParams, ctx, issues, line, column);
    }
    return;
  }

  if (expr.kind === "object") {
    for (const field of expr.fields ?? []) {
      validateTypeExpr(field.type, allowedTypeParams, ctx, issues, line, column);
    }
    return;
  }

  if (expr.kind === "ref") {
    if (expr.qname === "array") {
      if (!expr.elementType) {
        issues.push({
          code: "TYPE_MISMATCH",
          message: "Array type is missing element type",
          line,
          column,
        });
      }
      validateTypeExpr(expr.elementType, allowedTypeParams, ctx, issues, line, column);
      return;
    }

    const raw = expr.qname ?? "";
    const name = normalizeTypeName(raw);
    const argCount = expr.args?.length ?? 0;

    if (allowedTypeParams.has(name)) {
      if (argCount !== 0) {
        issues.push({
          code: "TYPE_MISMATCH",
          message: `Type parameter '${name}' cannot have type arguments`,
          line,
          column,
        });
      }
      return;
    }

    const expectedArity = ctx.typeArity.get(name);
    if (expectedArity === undefined) {
      issues.push({
        code: "UNBOUND_VARIABLE",
        message: `Unknown type reference '${raw}'`,
        line,
        column,
      });
      return;
    }

    if (argCount !== expectedArity) {
      issues.push({
        code: "TYPE_MISMATCH",
        message: `Type '${raw}' expects ${expectedArity} type argument(s) but got ${argCount}`,
        line,
        column,
      });
    }

    for (const arg of expr.args ?? []) {
      validateTypeExpr(arg, allowedTypeParams, ctx, issues, line, column);
    }
  }
}

function validateExpr(
  expr: Expr | undefined,
  ctx: DeclContext,
  issues: ValidationIssue[],
  line: number,
  column: number,
): void {
  if (!expr) return;

  if (expr.kind === "new") {
    const typeName = normalizeTypeName(expr.typeName ?? "");
    if (!ctx.typeArity.has(typeName)) {
      issues.push({
        code: "UNBOUND_VARIABLE",
        message: `Unknown constructor type '${expr.typeName ?? ""}'`,
        line,
        column,
      });
    }

    const structFields = ctx.structs.get(typeName);
    if (structFields && expr.fields) {
      for (const field of expr.fields) {
        if (!structFields.has(field.key)) {
          issues.push({
            code: "TYPE_MISMATCH",
            message: `Constructor for '${typeName}' has unknown field '${field.key}'`,
            line,
            column,
          });
        }
        validateExpr(field.value, ctx, issues, line, column);
      }
    }
  }

  validateExpr(expr.left, ctx, issues, line, column);
  validateExpr(expr.right, ctx, issues, line, column);
  validateExpr(expr.test, ctx, issues, line, column);
  validateExpr(expr.consequent, ctx, issues, line, column);
  validateExpr(expr.alternate, ctx, issues, line, column);
  validateExpr(expr.callee, ctx, issues, line, column);
  validateExpr(expr.object, ctx, issues, line, column);
  validateExpr(expr.index, ctx, issues, line, column);
  validateExpr(expr.source, ctx, issues, line, column);
  validateExpr(expr.time, ctx, issues, line, column);
  validateExpr(expr.confidence, ctx, issues, line, column);
  validateExpr(expr.matchExpr, ctx, issues, line, column);
  validateExpr(expr.itemExpr, ctx, issues, line, column);
  validateExpr(expr.iterable, ctx, issues, line, column);
  validateExpr(expr.filter, ctx, issues, line, column);

  for (const arg of expr.args ?? []) {
    validateExpr(arg, ctx, issues, line, column);
  }
  for (const arg of expr.callArgs ?? []) {
    validateExpr(arg.value, ctx, issues, line, column);
  }
  for (const field of expr.fields ?? []) {
    validateExpr(field.value, ctx, issues, line, column);
  }
  for (const c of expr.cases ?? []) {
    validateExpr(c.guard, ctx, issues, line, column);
    validateExpr(c.value, ctx, issues, line, column);
  }
}

function validateStatement(statement: Statement, ctx: DeclContext, issues: ValidationIssue[]): void {
  if (statement.name && RESERVED.has(statement.name)) {
    issues.push({
      code: "RESERVED_IDENT",
      message: `Identifier '${statement.name}' is reserved`,
      line: statement.loc.line,
      column: statement.loc.column,
    });
  }

  if (statement.kind === "fact" && !statement.expr) {
    issues.push({
      code: "INVALID_FACT",
      message: `Fact '${statement.name ?? "<anonymous>"}' is missing expression`,
      line: statement.loc.line,
      column: statement.loc.column,
    });
  }

  if (statement.kind === "query") {
    if (!statement.query?.target) {
      issues.push({
        code: "SEMANTIC_ERROR",
        message: "Query statement must include target",
        line: statement.loc.line,
        column: statement.loc.column,
      });
    }
    if (!statement.query?.response_format) {
      issues.push({
        code: "SEMANTIC_ERROR",
        message: "Query statement should include response_format",
        line: statement.loc.line,
        column: statement.loc.column,
      });
    }
  }

  validateExpr(statement.expr, ctx, issues, statement.loc.line, statement.loc.column);
  validateExpr(statement.iterable, ctx, issues, statement.loc.line, statement.loc.column);
  validateExpr(statement.query?.target, ctx, issues, statement.loc.line, statement.loc.column);
  validateExpr(statement.query?.where, ctx, issues, statement.loc.line, statement.loc.column);
  validateExpr(statement.query?.constraints, ctx, issues, statement.loc.line, statement.loc.column);
  validateExpr(statement.query?.response_format, ctx, issues, statement.loc.line, statement.loc.column);
  validateExpr(statement.query?.hint, ctx, issues, statement.loc.line, statement.loc.column);

  for (const param of statement.params ?? []) {
    validateExpr(param.defaultValue, ctx, issues, statement.loc.line, statement.loc.column);
  }
  for (const hint of statement.hints ?? []) {
    validateExpr(hint, ctx, issues, statement.loc.line, statement.loc.column);
  }

  if (statement.body) {
    for (const nested of statement.body) {
      validateStatement(nested, ctx, issues);
    }
  }
  if (statement.elseBody) {
    for (const nested of statement.elseBody) {
      validateStatement(nested, ctx, issues);
    }
  }
}

function validateDecl(decl: TopDecl, ctx: DeclContext, issues: ValidationIssue[]): void {
  if (decl.name && RESERVED.has(decl.name)) {
    issues.push({
      code: "RESERVED_IDENT",
      message: `Identifier '${decl.name}' is reserved`,
      line: decl.loc.line,
      column: decl.loc.column,
    });
  }

  if (decl.body) {
    for (const statement of decl.body) {
      validateStatement(statement, ctx, issues);
    }
  }

  if (decl.declarations) {
    for (const nested of decl.declarations) {
      validateDecl(nested, ctx, issues);
    }
  }

  if (decl.kind === "type") {
    const params = decl.typeParams ?? [];
    const seenParams = new Set<string>();
    for (const param of params) {
      if (seenParams.has(param)) {
        issues.push({
          code: "SEMANTIC_ERROR",
          message: `Type '${decl.name ?? "<anonymous>"}' has duplicate type parameter '${param}'`,
          line: decl.loc.line,
          column: decl.loc.column,
        });
      }
      seenParams.add(param);
    }

    const variantNames = new Set<string>();
    const collectVariants = (expr: TypeExpr | undefined): void => {
      if (!expr) return;
      if (expr.kind === "union" && expr.variants) {
        for (const variant of expr.variants) {
          collectVariants(variant);
        }
      } else if (expr.kind === "variant" && expr.name) {
        if (variantNames.has(expr.name)) {
          issues.push({
            code: "SEMANTIC_ERROR",
            message: `Type '${decl.name ?? "<anonymous>"}' has duplicate variant '${expr.name}'`,
            line: decl.loc.line,
            column: decl.loc.column,
          });
        }
        variantNames.add(expr.name);
      }
    };
    collectVariants(decl.typeExpr);

    validateTypeExpr(decl.typeExpr, seenParams, ctx, issues, decl.loc.line, decl.loc.column);
  }

  if (decl.kind === "struct") {
    const fieldNames = new Set<string>();
    for (const field of decl.structFields ?? []) {
      if (fieldNames.has(field.name)) {
        issues.push({
          code: "SEMANTIC_ERROR",
          message: `Struct '${decl.name ?? "<anonymous>"}' has duplicate field '${field.name}'`,
          line: decl.loc.line,
          column: decl.loc.column,
        });
      }
      fieldNames.add(field.name);
      validateTypeExpr(field.type, new Set(), ctx, issues, decl.loc.line, decl.loc.column);
    }
  }
}

export function validateProgram(program: Program): ValidationIssue[] {
  const issues: ValidationIssue[] = [];
  const seen = new Map<string, TopDecl>();
  const ctx = buildContext(program);

  for (const decl of program.declarations) {
    validateDecl(decl, ctx, issues);

    if (!decl.name) {
      continue;
    }

    const key = `${decl.kind}:${decl.name}`;
    const existing = seen.get(key);
    if (existing) {
      issues.push({
        code: "SEMANTIC_ERROR",
        message: `Duplicate declaration '${decl.name}' of kind '${decl.kind}'`,
        line: decl.loc.line,
        column: decl.loc.column,
      });
    } else {
      seen.set(key, decl);
    }
  }

  return issues;
}

export function assertValidProgram(program: Program): void {
  const issues = validateProgram(program);
  if (issues.length > 0) {
    const issue = issues[0];
    throw new UsrlError(
      issue.code === "RESERVED_IDENT" ? "RESERVED_IDENT" : "SEMANTIC_ERROR",
      issue.message,
      issue.line,
      issue.column,
    );
  }
}
