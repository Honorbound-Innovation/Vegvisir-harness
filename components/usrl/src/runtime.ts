import { Expr, Pattern, Program, Statement, TopDecl } from "./ast.js";

export interface RuntimeIssue {
  code: string;
  message: string;
  line: number;
  column: number;
}

export interface FactDerivation {
  fact: string;
  value: unknown;
  iteration: number;
  line: number;
  column: number;
  via: string;
  tainted?: boolean;
  depends_on?: string[];
}

export interface QueryResult {
  line: number;
  column: number;
  response_format: string;
  result: unknown;
  tainted?: boolean;
}

export interface DecisionResult {
  effect: "permit" | "deny";
  value: unknown;
  tainted: boolean;
  line: number;
  column: number;
}

export interface RuntimeOptions {
  maxIterations?: number;
  capabilities?: string[];
}

export interface RuntimeResult {
  facts: Record<string, unknown>;
  tainted_facts: string[];
  events: Array<{ name: string; payload: unknown[] }>;
  issues: RuntimeIssue[];
  derivations: FactDerivation[];
  queries: QueryResult[];
  decisions: DecisionResult[];
  iterations: number;
}

interface EvalContext {
  facts: Map<string, unknown>;
  factTaint: Map<string, boolean>;
  events: Array<{ name: string; payload: unknown[] }>;
  issues: RuntimeIssue[];
  derivations: FactDerivation[];
  queries: QueryResult[];
  decisions: DecisionResult[];
  currentIteration: number;
  changedFacts: Set<string>;
  capabilities: Set<string>;
}

function truthy(value: unknown): boolean {
  return Boolean(value);
}

function toNumber(value: unknown): number {
  if (typeof value === "number") return value;
  if (typeof value === "string") {
    const n = Number(value);
    return Number.isNaN(n) ? 0 : n;
  }
  return 0;
}

function toDateValue(value: unknown): Date | undefined {
  if (value instanceof Date) return value;
  if (typeof value === "string" || typeof value === "number") {
    const d = new Date(value);
    if (!Number.isNaN(d.getTime())) return d;
  }
  return undefined;
}

function stableStringify(value: unknown): string {
  const seen = new WeakSet<object>();

  const normalize = (v: unknown): unknown => {
    if (v === null || typeof v !== "object") return v;
    if (seen.has(v as object)) return "[Circular]";
    seen.add(v as object);

    if (Array.isArray(v)) {
      return v.map((item) => normalize(item));
    }

    const out: Record<string, unknown> = {};
    const keys = Object.keys(v as Record<string, unknown>).sort();
    for (const key of keys) {
      out[key] = normalize((v as Record<string, unknown>)[key]);
    }
    return out;
  };

  return JSON.stringify(normalize(value));
}

function valuesEqual(a: unknown, b: unknown): boolean {
  return stableStringify(a) === stableStringify(b);
}

function matchPattern(pattern: Pattern | undefined, value: unknown, scope: Map<string, unknown>): void {
  if (!pattern) return;
  if (pattern.kind === "identifier") {
    if (pattern.name) scope.set(pattern.name, value);
    return;
  }

  if (pattern.kind === "tuple") {
    const arr = Array.isArray(value) ? value : [];
    const items = pattern.items ?? [];
    for (let i = 0; i < items.length; i += 1) {
      matchPattern(items[i], arr[i], scope);
    }
    return;
  }

  const obj = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  for (const field of pattern.fields ?? []) {
    matchPattern(field.pattern, obj[field.key], scope);
  }
}

function bindPatternTaint(pattern: Pattern | undefined, tainted: boolean, scopeTaint: Map<string, boolean>): void {
  if (!pattern) return;
  if (pattern.kind === "identifier") {
    if (pattern.name) scopeTaint.set(pattern.name, tainted);
    return;
  }
  if (pattern.kind === "tuple") {
    for (const p of pattern.items ?? []) {
      bindPatternTaint(p, tainted, scopeTaint);
    }
    return;
  }
  for (const field of pattern.fields ?? []) {
    bindPatternTaint(field.pattern, tainted, scopeTaint);
  }
}

function pushTaintIssue(ctx: EvalContext, message: string, line: number, column: number): void {
  ctx.issues.push({
    code: "INJECTION_PATTERN",
    message,
    line,
    column,
  });
}

function pushCapabilityIssue(ctx: EvalContext, capability: string, line: number, column: number): void {
  ctx.issues.push({
    code: "EXECUTION_ERROR",
    message: `Missing required capability '${capability}'`,
    line,
    column,
  });
}

function collectExprIdentifiers(expr: Expr | undefined, out: Set<string>): void {
  if (!expr) return;
  if (expr.kind === "identifier" && expr.name) {
    out.add(expr.name);
    return;
  }

  collectExprIdentifiers(expr.left, out);
  collectExprIdentifiers(expr.right, out);
  collectExprIdentifiers(expr.test, out);
  collectExprIdentifiers(expr.consequent, out);
  collectExprIdentifiers(expr.alternate, out);
  collectExprIdentifiers(expr.callee, out);
  collectExprIdentifiers(expr.object, out);
  collectExprIdentifiers(expr.index, out);
  collectExprIdentifiers(expr.source, out);
  collectExprIdentifiers(expr.time, out);
  collectExprIdentifiers(expr.confidence, out);
  collectExprIdentifiers(expr.matchExpr, out);
  collectExprIdentifiers(expr.itemExpr, out);
  collectExprIdentifiers(expr.iterable, out);
  collectExprIdentifiers(expr.filter, out);

  for (const a of expr.args ?? []) collectExprIdentifiers(a, out);
  for (const a of expr.callArgs ?? []) collectExprIdentifiers(a.value, out);
  for (const f of expr.fields ?? []) collectExprIdentifiers(f.value, out);
  for (const c of expr.cases ?? []) {
    collectExprIdentifiers(c.guard, out);
    collectExprIdentifiers(c.value, out);
  }
}

function evalCall(name: string, args: unknown[], ctx: EvalContext): unknown {
  if (name === "count") {
    const a = args[0];
    if (Array.isArray(a)) return a.length;
    if (a && typeof a === "object") return Object.keys(a as object).length;
    return 0;
  }
  if (name === "sum") {
    const arr = Array.isArray(args[0]) ? args[0] : [];
    return arr.reduce((acc, x) => acc + toNumber(x), 0);
  }
  if (name === "min") {
    const arr = Array.isArray(args[0]) ? args[0].map(toNumber) : [];
    return arr.length > 0 ? Math.min(...arr) : 0;
  }
  if (name === "max") {
    const arr = Array.isArray(args[0]) ? args[0].map(toNumber) : [];
    return arr.length > 0 ? Math.max(...arr) : 0;
  }
  if (name === "concat") return args.map((x) => String(x ?? "")).join("");
  if (name === "to_string") return String(args[0] ?? "");
  if (name === "distinct") {
    const arr = Array.isArray(args[0]) ? args[0] : [];
    return Array.from(new Set(arr));
  }
  if (name === "union") {
    const a = Array.isArray(args[0]) ? args[0] : [];
    const b = Array.isArray(args[1]) ? args[1] : [];
    return Array.from(new Set([...a, ...b]));
  }
  if (name === "intersect") {
    const a = new Set(Array.isArray(args[0]) ? args[0] : []);
    const b = new Set(Array.isArray(args[1]) ? args[1] : []);
    return Array.from(a).filter((x) => b.has(x));
  }
  if (name === "difference") {
    const a = new Set(Array.isArray(args[0]) ? args[0] : []);
    const b = new Set(Array.isArray(args[1]) ? args[1] : []);
    return Array.from(a).filter((x) => !b.has(x));
  }

  if (name === "date") {
    const d = toDateValue(args[0]);
    return d ? d.toISOString() : undefined;
  }
  if (name === "year") {
    const d = toDateValue(args[0]);
    return d ? d.getUTCFullYear() : undefined;
  }
  if (name === "month") {
    const d = toDateValue(args[0]);
    return d ? d.getUTCMonth() + 1 : undefined;
  }
  if (name === "day") {
    const d = toDateValue(args[0]);
    return d ? d.getUTCDate() : undefined;
  }
  if (name === "before") {
    const a = toDateValue(args[0]);
    const b = toDateValue(args[1]);
    return a && b ? a.getTime() < b.getTime() : false;
  }
  if (name === "after") {
    const a = toDateValue(args[0]);
    const b = toDateValue(args[1]);
    return a && b ? a.getTime() > b.getTime() : false;
  }
  if (name === "during") {
    const t = toDateValue(args[0]);
    const interval = args[1];
    if (!t || !interval || typeof interval !== "object") return false;
    const start = toDateValue((interval as Record<string, unknown>).start);
    const end = toDateValue((interval as Record<string, unknown>).end);
    return Boolean(start && end && t.getTime() >= start.getTime() && t.getTime() <= end.getTime());
  }
  if (name === "always" || name === "eventually") return truthy(args[0]);
  if (name === "until") return truthy(args[0]) && !truthy(args[1]);
  if (name === "since") return truthy(args[0]) && truthy(args[1]);

  if (name === "untrusted") return args[0];
  if (name === "sanitize") return args[0];

  if (name === "changed") {
    const target = String(args[0] ?? "");
    return ctx.changedFacts.has(target);
  }

  if (name === "capability") {
    const cap = String(args[0] ?? "");
    return ctx.capabilities.has(cap);
  }

  if (name === "require_capability") {
    const cap = String(args[0] ?? "");
    if (!ctx.capabilities.has(cap)) {
      pushCapabilityIssue(ctx, cap, 1, 1);
      return false;
    }
    return true;
  }

  return undefined;
}

function exprTainted(expr: Expr | undefined, scopeTaint: Map<string, boolean>, ctx: EvalContext): boolean {
  if (!expr) return false;

  switch (expr.kind) {
    case "literal":
      return false;

    case "identifier":
      return (expr.name ? scopeTaint.get(expr.name) : undefined) === true
        || (expr.name ? ctx.factTaint.get(expr.name) : undefined) === true;

    case "call": {
      const calleeName = expr.callee?.kind === "identifier" ? expr.callee.name : undefined;
      if (calleeName === "untrusted") return true;
      if (calleeName === "sanitize") return false;
      return (expr.callArgs ?? []).some((a) => exprTainted(a.value, scopeTaint, ctx));
    }

    case "array":
    case "set":
      return (expr.elements ?? []).some((e) => exprTainted(e, scopeTaint, ctx));

    case "object":
    case "new":
      return (expr.fields ?? []).some((f) => exprTainted(f.value, scopeTaint, ctx));

    case "match":
      return exprTainted(expr.matchExpr, scopeTaint, ctx)
        || (expr.cases ?? []).some((c) => exprTainted(c.guard, scopeTaint, ctx) || exprTainted(c.value, scopeTaint, ctx));

    case "comprehension":
      return exprTainted(expr.itemExpr, scopeTaint, ctx)
        || exprTainted(expr.iterable, scopeTaint, ctx)
        || exprTainted(expr.filter, scopeTaint, ctx);

    default:
      return exprTainted(expr.left, scopeTaint, ctx)
        || exprTainted(expr.right, scopeTaint, ctx)
        || exprTainted(expr.test, scopeTaint, ctx)
        || exprTainted(expr.consequent, scopeTaint, ctx)
        || exprTainted(expr.alternate, scopeTaint, ctx)
        || exprTainted(expr.callee, scopeTaint, ctx)
        || exprTainted(expr.object, scopeTaint, ctx)
        || exprTainted(expr.index, scopeTaint, ctx)
        || exprTainted(expr.source, scopeTaint, ctx)
        || exprTainted(expr.time, scopeTaint, ctx)
        || exprTainted(expr.confidence, scopeTaint, ctx)
        || exprTainted(expr.itemExpr, scopeTaint, ctx)
        || exprTainted(expr.iterable, scopeTaint, ctx)
        || exprTainted(expr.filter, scopeTaint, ctx)
        || (expr.args ?? []).some((a) => exprTainted(a, scopeTaint, ctx))
        || (expr.callArgs ?? []).some((a) => exprTainted(a.value, scopeTaint, ctx));
  }
}

function evalExpr(expr: Expr | undefined, scope: Map<string, unknown>, ctx: EvalContext): unknown {
  if (!expr) return undefined;

  switch (expr.kind) {
    case "literal": return expr.value;

    case "identifier": {
      if (expr.name && scope.has(expr.name)) return scope.get(expr.name);
      if (expr.name && ctx.facts.has(expr.name)) return ctx.facts.get(expr.name);
      return undefined;
    }

    case "unary": {
      const right = evalExpr(expr.right, scope, ctx);
      if (expr.operator === "not" || expr.operator === "!") return !truthy(right);
      if (expr.operator === "-") return -toNumber(right);
      if (expr.operator === "+") return toNumber(right);
      return undefined;
    }

    case "binary": {
      const left = evalExpr(expr.left, scope, ctx);
      const right = evalExpr(expr.right, scope, ctx);
      switch (expr.operator) {
        case "+": return toNumber(left) + toNumber(right);
        case "-": return toNumber(left) - toNumber(right);
        case "*": return toNumber(left) * toNumber(right);
        case "/": return toNumber(right) === 0 ? 0 : toNumber(left) / toNumber(right);
        case "%": return toNumber(left) % toNumber(right);
        case "..": {
          const from = toNumber(left);
          const to = toNumber(right);
          const out: number[] = [];
          for (let i = from; i <= to; i += 1) out.push(i);
          return out;
        }
        case "==": return valuesEqual(left, right);
        case "!=": return !valuesEqual(left, right);
        case "<": return toNumber(left) < toNumber(right);
        case "<=": return toNumber(left) <= toNumber(right);
        case ">": return toNumber(left) > toNumber(right);
        case ">=": return toNumber(left) >= toNumber(right);
        case "and": return truthy(left) && truthy(right);
        case "or": return truthy(left) || truthy(right);
        case "in": return Array.isArray(right) ? right.some((item) => valuesEqual(item, left)) : false;
        case "contains": return Array.isArray(left) ? left.some((item) => valuesEqual(item, right)) : false;
        default: return undefined;
      }
    }

    case "conditional":
      return truthy(evalExpr(expr.test, scope, ctx))
        ? evalExpr(expr.consequent, scope, ctx)
        : evalExpr(expr.alternate, scope, ctx);

    case "coalesce": {
      const left = evalExpr(expr.left, scope, ctx);
      return left ?? evalExpr(expr.right, scope, ctx);
    }

    case "array":
      return (expr.elements ?? []).map((e) => evalExpr(e, scope, ctx));

    case "set":
      return Array.from(new Set((expr.elements ?? []).map((e) => stableStringify(evalExpr(e, scope, ctx))))).map((x) => JSON.parse(x));

    case "object": {
      const out: Record<string, unknown> = {};
      for (const field of expr.fields ?? []) out[field.key] = evalExpr(field.value, scope, ctx);
      return out;
    }

    case "member": {
      const obj = evalExpr(expr.object, scope, ctx);
      if (!obj || typeof obj !== "object") return undefined;
      return (obj as Record<string, unknown>)[expr.property ?? ""];
    }

    case "safe_member": {
      const obj = evalExpr(expr.object, scope, ctx);
      if (!obj || typeof obj !== "object") return undefined;
      return (obj as Record<string, unknown>)[expr.property ?? ""];
    }

    case "index": {
      const obj = evalExpr(expr.object, scope, ctx);
      const idx = evalExpr(expr.index, scope, ctx);
      if (Array.isArray(obj)) return obj[toNumber(idx)];
      if (obj && typeof obj === "object") return (obj as Record<string, unknown>)[String(idx)];
      return undefined;
    }

    case "call": {
      const calleeName = expr.callee?.kind === "identifier" ? expr.callee.name : undefined;
      const args = (expr.callArgs ?? []).map((a) => evalExpr(a.value, scope, ctx));
      return calleeName ? evalCall(calleeName, args, ctx) : undefined;
    }

    case "new": {
      const out: Record<string, unknown> = { __type: expr.typeName ?? "" };
      for (const field of expr.fields ?? []) out[field.key] = evalExpr(field.value, scope, ctx);
      return out;
    }

    case "lambda":
      return undefined;

    case "provenance":
      return evalExpr(expr.left, scope, ctx);

    case "match": {
      const subject = evalExpr(expr.matchExpr, scope, ctx);
      for (const c of expr.cases ?? []) {
        const caseScope = new Map(scope);
        matchPattern(c.pattern, subject, caseScope);
        const guardOk = c.guard ? truthy(evalExpr(c.guard, caseScope, ctx)) : true;
        if (guardOk) return evalExpr(c.value, caseScope, ctx);
      }
      return undefined;
    }

    case "comprehension": {
      const iterable = evalExpr(expr.iterable, scope, ctx);
      const arr = Array.isArray(iterable) ? iterable : [];
      const out: unknown[] = [];
      for (const item of arr) {
        const inner = new Map(scope);
        matchPattern(expr.pattern, item, inner);
        const filterOk = expr.filter ? truthy(evalExpr(expr.filter, inner, ctx)) : true;
        if (filterOk) out.push(evalExpr(expr.itemExpr, inner, ctx));
      }
      return out;
    }
  }
}

function responseFormatName(queryStmt: Statement): string {
  const rf = queryStmt.query?.response_format;
  if (!rf) return "FACT";
  if (rf.kind === "identifier" && rf.name) return rf.name;
  if (rf.kind === "literal" && typeof rf.value === "string") return rf.value;
  return "FACT";
}

function evaluateQuery(queryStmt: Statement, scope: Map<string, unknown>, scopeTaint: Map<string, boolean>, ctx: EvalContext): QueryResult {
  const whereOk = queryStmt.query?.where ? truthy(evalExpr(queryStmt.query.where, scope, ctx)) : true;
  const constraintsOk = queryStmt.query?.constraints ? truthy(evalExpr(queryStmt.query.constraints, scope, ctx)) : true;
  const target = queryStmt.query?.target ? evalExpr(queryStmt.query.target, scope, ctx) : undefined;
  const tainted = queryStmt.query?.target ? exprTainted(queryStmt.query.target, scopeTaint, ctx) : false;
  const format = responseFormatName(queryStmt);

  const gated = whereOk && constraintsOk;

  let result: unknown;
  if (!gated) {
    result = format === "BOOLEAN" ? false : null;
  } else if (format === "BOOLEAN") {
    result = truthy(target);
  } else if (format === "FACT") {
    result = target;
  } else if (format === "OBJECT_TABLE") {
    result = Array.isArray(target) ? target : [target];
  } else {
    result = target;
  }

  return {
    line: queryStmt.loc.line,
    column: queryStmt.loc.column,
    response_format: format,
    result,
    tainted,
  };
}

function collectRuleStatements(decls: TopDecl[]): Array<{ rule: Statement; namespace: string }> {
  const out: Array<{ rule: Statement; namespace: string }> = [];

  const walkDecls = (items: TopDecl[], namespace: string): void => {
    for (const decl of items) {
      if (decl.kind === "namespace") {
        const ns = decl.name ? (namespace ? `${namespace}.${decl.name}` : decl.name) : namespace;
        if (decl.declarations) walkDecls(decl.declarations, ns);
        continue;
      }

      const walkStatements = (statements: Statement[] | undefined): void => {
        if (!statements) return;
        for (const stmt of statements) {
          if (stmt.kind === "rule") out.push({ rule: stmt, namespace });
          walkStatements(stmt.body);
          walkStatements(stmt.elseBody);
        }
      };

      walkStatements(decl.body);
      if (decl.declarations) walkDecls(decl.declarations, namespace);
    }
  };

  walkDecls(decls, "");
  return out;
}

function collectTriggerStatements(decls: TopDecl[]): Array<{ trigger: Statement; namespace: string }> {
  const out: Array<{ trigger: Statement; namespace: string }> = [];

  const walkDecls = (items: TopDecl[], namespace: string): void => {
    for (const decl of items) {
      if (decl.kind === "namespace") {
        const ns = decl.name ? (namespace ? `${namespace}.${decl.name}` : decl.name) : namespace;
        if (decl.declarations) walkDecls(decl.declarations, ns);
        continue;
      }

      const walkStatements = (statements: Statement[] | undefined): void => {
        if (!statements) return;
        for (const stmt of statements) {
          if (stmt.kind === "trigger") out.push({ trigger: stmt, namespace });
          walkStatements(stmt.body);
          walkStatements(stmt.elseBody);
        }
      };

      walkStatements(decl.body);
      if (decl.declarations) walkDecls(decl.declarations, namespace);
    }
  };

  walkDecls(decls, "");
  return out;
}

function executeStatements(
  statements: Statement[] | undefined,
  scope: Map<string, unknown>,
  scopeTaint: Map<string, boolean>,
  ctx: EvalContext,
): boolean {
  if (!statements) return false;
  let changed = false;

  for (const stmt of statements) {
    switch (stmt.kind) {
      case "section":
      case "parallel":
      case "sequence":
      case "stage":
      case "constraint": {
        const nestedScope = new Map(scope);
        const nestedTaint = new Map(scopeTaint);
        if (executeStatements(stmt.body, nestedScope, nestedTaint, ctx)) changed = true;
        break;
      }

      case "trigger": {
        // Trigger bodies are executed reactively by the driver, not inline.
        break;
      }

      case "fact": {
        const value = evalExpr(stmt.expr, scope, ctx);
        const tainted = exprTainted(stmt.expr, scopeTaint, ctx);
        if (stmt.name) {
          const prev = ctx.facts.get(stmt.name);
          if (!valuesEqual(prev, value)) {
            ctx.facts.set(stmt.name, value);
            ctx.factTaint.set(stmt.name, tainted);
            ctx.changedFacts.add(stmt.name);
            changed = true;

            const deps = new Set<string>();
            collectExprIdentifiers(stmt.expr, deps);
            const depends_on = Array.from(deps).filter((d) => d !== stmt.name && ctx.facts.has(d));

            ctx.derivations.push({
              fact: stmt.name,
              value,
              tainted,
              iteration: ctx.currentIteration,
              line: stmt.loc.line,
              column: stmt.loc.column,
              via: "fact",
              depends_on,
            });
          }
          scope.set(stmt.name, value);
          scopeTaint.set(stmt.name, tainted);
        }
        break;
      }

      case "let": {
        const value = evalExpr(stmt.expr, scope, ctx);
        const tainted = exprTainted(stmt.expr, scopeTaint, ctx);
        matchPattern(stmt.pattern, value, scope);
        bindPatternTaint(stmt.pattern, tainted, scopeTaint);
        break;
      }

      case "if": {
        const condTainted = exprTainted(stmt.expr, scopeTaint, ctx);
        if (condTainted) pushTaintIssue(ctx, "tainted value used in if condition", stmt.loc.line, stmt.loc.column);
        const ok = truthy(evalExpr(stmt.expr, scope, ctx));
        const branchScope = new Map(scope);
        const branchTaint = new Map(scopeTaint);
        if (ok) {
          if (executeStatements(stmt.body, branchScope, branchTaint, ctx)) changed = true;
        } else if (executeStatements(stmt.elseBody, branchScope, branchTaint, ctx)) {
          changed = true;
        }
        break;
      }

      case "when": {
        const condTainted = exprTainted(stmt.expr, scopeTaint, ctx);
        if (condTainted) pushTaintIssue(ctx, "tainted value used in when condition", stmt.loc.line, stmt.loc.column);
        const ok = truthy(evalExpr(stmt.expr, scope, ctx));
        if (ok) {
          const inner = new Map(scope);
          const innerTaint = new Map(scopeTaint);
          if (executeStatements(stmt.body, inner, innerTaint, ctx)) changed = true;
        }
        break;
      }

      case "foreach": {
        const iterable = evalExpr(stmt.iterable, scope, ctx);
        const iterableTainted = exprTainted(stmt.iterable, scopeTaint, ctx);
        const arr = Array.isArray(iterable) ? iterable : [];
        for (const item of arr) {
          const inner = new Map(scope);
          const innerTaint = new Map(scopeTaint);
          matchPattern(stmt.pattern, item, inner);
          bindPatternTaint(stmt.pattern, iterableTainted, innerTaint);
          if (executeStatements(stmt.body, inner, innerTaint, ctx)) changed = true;
        }
        break;
      }

      case "rule": {
        break;
      }

      case "assert":
      case "require": {
        if (exprTainted(stmt.expr, scopeTaint, ctx)) {
          pushTaintIssue(ctx, `tainted value used in ${stmt.kind}`, stmt.loc.line, stmt.loc.column);
        }
        const ok = truthy(evalExpr(stmt.expr, scope, ctx));
        if (!ok) {
          ctx.issues.push({
            code: stmt.kind === "assert" ? "ASSERT_FAIL" : "SEMANTIC_ERROR",
            message: `${stmt.kind} failed`,
            line: stmt.loc.line,
            column: stmt.loc.column,
          });
        }
        break;
      }

      case "emit": {
        const tainted = exprTainted(stmt.expr, scopeTaint, ctx);
        if (tainted) pushTaintIssue(ctx, "tainted value used in emit", stmt.loc.line, stmt.loc.column);
        const value = evalExpr(stmt.expr, scope, ctx);
        if (Array.isArray(value) && typeof value[0] === "string") {
          const eventName = String(value[0]);
          if (eventName.startsWith("tool:")) {
            const cap = eventName.slice("tool:".length);
            if (!ctx.capabilities.has(cap)) {
              pushCapabilityIssue(ctx, cap, stmt.loc.line, stmt.loc.column);
            }
          }
          ctx.events.push({ name: eventName, payload: value.slice(1) });
        } else {
          ctx.events.push({ name: "event", payload: [value] });
        }
        break;
      }

      case "query": {
        ctx.queries.push(evaluateQuery(stmt, scope, scopeTaint, ctx));
        break;
      }

      case "permit":
      case "deny": {
        const tainted = exprTainted(stmt.expr, scopeTaint, ctx);
        if (tainted) pushTaintIssue(ctx, `tainted value used in ${stmt.kind}`, stmt.loc.line, stmt.loc.column);
        const value = evalExpr(stmt.expr, scope, ctx);
        ctx.decisions.push({
          effect: stmt.kind,
          value,
          tainted,
          line: stmt.loc.line,
          column: stmt.loc.column,
        });
        break;
      }

      case "return":
      case "expr": {
        evalExpr(stmt.expr, scope, ctx);
        break;
      }
    }
  }

  return changed;
}

function executeTriggerPass(
  triggers: Array<{ trigger: Statement; namespace: string }>,
  ctx: EvalContext,
): boolean {
  if (ctx.changedFacts.size === 0) return false;
  let changed = false;
  for (const { trigger } of triggers) {
    const scope = new Map<string, unknown>();
    const scopeTaint = new Map<string, boolean>();
    if (executeStatements(trigger.body, scope, scopeTaint, ctx)) {
      changed = true;
    }
  }
  return changed;
}

function executeNonRuleStatements(decls: TopDecl[], ctx: EvalContext): boolean {
  let changed = false;

  const walkDecls = (items: TopDecl[]): void => {
    for (const decl of items) {
      if (decl.kind === "namespace") {
        if (decl.declarations) walkDecls(decl.declarations);
        continue;
      }

      const stripRulesAndTriggers = (statements: Statement[] | undefined): Statement[] => {
        if (!statements) return [];
        const out: Statement[] = [];
        for (const s of statements) {
          if (s.kind === "rule" || s.kind === "trigger") continue;
          out.push({
            ...s,
            body: stripRulesAndTriggers(s.body),
            elseBody: stripRulesAndTriggers(s.elseBody),
          });
        }
        return out;
      };

      const scope = new Map<string, unknown>();
      const scopeTaint = new Map<string, boolean>();
      if (executeStatements(stripRulesAndTriggers(decl.body), scope, scopeTaint, ctx)) changed = true;

      if (decl.declarations) walkDecls(decl.declarations);
    }
  };

  walkDecls(decls);
  return changed;
}

function checkDecisionConflicts(ctx: EvalContext): void {
  const permits = new Set(ctx.decisions.filter((d) => d.effect === "permit").map((d) => stableStringify(d.value)));
  const denies = new Set(ctx.decisions.filter((d) => d.effect === "deny").map((d) => stableStringify(d.value)));

  for (const v of permits) {
    if (denies.has(v)) {
      ctx.issues.push({
        code: "SEMANTIC_ERROR",
        message: `Conflicting permit/deny decision for value ${v}`,
        line: 1,
        column: 1,
      });
    }
  }
}

export function evaluateProgram(program: Program, options: RuntimeOptions = {}): RuntimeResult {
  const maxIterations = options.maxIterations ?? 32;
  const ctx: EvalContext = {
    facts: new Map<string, unknown>(),
    factTaint: new Map<string, boolean>(),
    events: [],
    issues: [],
    derivations: [],
    queries: [],
    decisions: [],
    currentIteration: 0,
    changedFacts: new Set<string>(),
    capabilities: new Set(options.capabilities ?? []),
  };

  const rules = collectRuleStatements(program.declarations);
  const triggers = collectTriggerStatements(program.declarations);

  executeNonRuleStatements(program.declarations, ctx);
  if (executeTriggerPass(triggers, ctx)) {
    // trigger caused changes in seed pass
  }

  let iterations = 0;
  let changed = true;
  while (changed && iterations < maxIterations) {
    changed = false;
    iterations += 1;
    ctx.currentIteration = iterations;
    ctx.changedFacts.clear();

    for (const { rule } of rules) {
      const scope = new Map<string, unknown>();
      const scopeTaint = new Map<string, boolean>();
      for (const p of rule.params ?? []) {
        scope.set(p.name, evalExpr(p.defaultValue, scope, ctx));
        scopeTaint.set(p.name, exprTainted(p.defaultValue, scopeTaint, ctx));
      }
      if (executeStatements(rule.body, scope, scopeTaint, ctx)) {
        changed = true;
      }
    }

    if (executeTriggerPass(triggers, ctx)) {
      changed = true;
    }
  }

  if (changed && iterations >= maxIterations) {
    ctx.issues.push({
      code: "STEP_GAP",
      message: `Fixed-point not reached within ${maxIterations} iterations`,
      line: 1,
      column: 1,
    });
  }

  checkDecisionConflicts(ctx);

  return {
    facts: Object.fromEntries(ctx.facts.entries()),
    tainted_facts: Array.from(ctx.factTaint.entries()).filter(([, t]) => t).map(([k]) => k),
    events: ctx.events,
    issues: ctx.issues,
    derivations: ctx.derivations,
    queries: ctx.queries,
    decisions: ctx.decisions,
    iterations,
  };
}
