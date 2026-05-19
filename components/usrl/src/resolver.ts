import { Expr, Pattern, Program, Statement, TopDecl, TypeExpr } from "./ast.js";

export interface ResolutionIssue {
  code: string;
  message: string;
  line: number;
  column: number;
}

export interface SymbolInfo {
  id: string;
  kind: TopDecl["kind"];
  name: string;
  qname: string;
  namespace: string;
  line: number;
  column: number;
  typeParamArity?: number;
}

export interface BoundNode {
  id: string;
  kind: "decl" | "stmt" | "namespace";
  label: string;
  line: number;
  column: number;
}

export interface BoundEdge {
  from: string;
  to: string;
  kind: "contains" | "references";
  detail?: string;
}

export interface BoundGraph {
  nodes: BoundNode[];
  edges: BoundEdge[];
}

export interface ResolvedReference {
  fromNodeId: string;
  kind: "type_ref" | "constructor" | "symbol" | "local" | "unbound";
  text: string;
  targetSymbolId?: string;
  line: number;
  column: number;
}

export interface ResolutionResult {
  symbols: SymbolInfo[];
  references: ResolvedReference[];
  issues: ResolutionIssue[];
  graph: BoundGraph;
}

interface SymbolTable {
  byId: Map<string, SymbolInfo>;
  byQname: Map<string, SymbolInfo>;
  byShortName: Map<string, SymbolInfo[]>;
}

const KNOWN_CALLEES = new Set([
  "count",
  "min",
  "max",
  "sum",
  "avg",
  "map",
  "filter",
  "reduce",
  "distinct",
  "union",
  "intersect",
  "difference",
  "match",
  "concat",
  "to_string",
  "ratio",
  "year",
  "month",
  "day",
  "date",
  "before",
  "after",
  "during",
  "always",
  "eventually",
  "until",
  "since",
]);

function makeSymbolId(qname: string, kind: TopDecl["kind"]): string {
  return `sym:${kind}:${qname}`;
}

function makeStmtId(counter: number): string {
  return `stmt:${counter}`;
}

function normalizeTypeName(name: string): string {
  return name.includes(".") ? name.split(".").at(-1) ?? name : name;
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

function collectSymbols(program: Program): { symbols: SymbolInfo[]; table: SymbolTable; usingNamespaces: string[] } {
  const symbols: SymbolInfo[] = [];
  const byId = new Map<string, SymbolInfo>();
  const byQname = new Map<string, SymbolInfo>();
  const byShortName = new Map<string, SymbolInfo[]>();
  const usingNamespaces: string[] = [];

  const register = (decl: TopDecl, namespace: string): void => {
    if (!decl.name) return;

    if (decl.kind === "using") {
      usingNamespaces.push(decl.name);
      return;
    }

    if (["namespace", "import", "expand"].includes(decl.kind)) {
      return;
    }

    const qname = namespace ? `${namespace}.${decl.name}` : decl.name;
    const info: SymbolInfo = {
      id: makeSymbolId(qname, decl.kind),
      kind: decl.kind,
      name: decl.name,
      qname,
      namespace,
      line: decl.loc.line,
      column: decl.loc.column,
      typeParamArity: decl.kind === "type" ? decl.typeParams?.length ?? 0 : 0,
    };

    symbols.push(info);
    byId.set(info.id, info);
    byQname.set(info.qname, info);

    const arr = byShortName.get(info.name) ?? [];
    arr.push(info);
    byShortName.set(info.name, arr);
  };

  const walkDecls = (decls: TopDecl[], namespace: string): void => {
    for (const decl of decls) {
      if (decl.kind === "namespace") {
        const nsName = decl.name ? (namespace ? `${namespace}.${decl.name}` : decl.name) : namespace;
        if (decl.declarations) {
          walkDecls(decl.declarations, nsName);
        }
        continue;
      }

      register(decl, namespace);

      if (decl.declarations) {
        walkDecls(decl.declarations, namespace);
      }
    }
  };

  walkDecls(program.declarations, "");

  return {
    symbols,
    table: { byId, byQname, byShortName },
    usingNamespaces,
  };
}

function resolveSymbolByName(
  rawName: string,
  namespace: string,
  usingNamespaces: string[],
  table: SymbolTable,
): { symbol?: SymbolInfo; ambiguous?: SymbolInfo[] } {
  if (!rawName) return {};

  if (rawName.includes(".")) {
    const exact = table.byQname.get(rawName);
    return exact ? { symbol: exact } : {};
  }

  const sameNamespace = namespace ? table.byQname.get(`${namespace}.${rawName}`) : undefined;
  if (sameNamespace) {
    return { symbol: sameNamespace };
  }

  for (const usingNs of usingNamespaces) {
    const fromUsing = table.byQname.get(`${usingNs}.${rawName}`);
    if (fromUsing) {
      return { symbol: fromUsing };
    }
  }

  const short = table.byShortName.get(rawName) ?? [];
  if (short.length === 1) {
    return { symbol: short[0] };
  }
  if (short.length > 1) {
    return { ambiguous: short };
  }

  return {};
}

function collectPatternNames(pattern: Pattern | undefined): string[] {
  if (!pattern) return [];
  if (pattern.kind === "identifier") {
    return pattern.name ? [pattern.name] : [];
  }
  if (pattern.kind === "tuple") {
    return (pattern.items ?? []).flatMap((p) => collectPatternNames(p));
  }
  return (pattern.fields ?? []).flatMap((f) => collectPatternNames(f.pattern));
}

function validateTypeExprRefs(
  expr: TypeExpr | undefined,
  ownerNodeId: string,
  ownerNamespace: string,
  ownerTypeParams: Set<string>,
  table: SymbolTable,
  usingNamespaces: string[],
  references: ResolvedReference[],
  issues: ResolutionIssue[],
  line: number,
  column: number,
): void {
  if (!expr) return;

  if (expr.kind === "union") {
    for (const variant of expr.variants ?? []) {
      validateTypeExprRefs(
        variant,
        ownerNodeId,
        ownerNamespace,
        ownerTypeParams,
        table,
        usingNamespaces,
        references,
        issues,
        line,
        column,
      );
    }
    return;
  }

  if (expr.kind === "variant" || expr.kind === "object") {
    for (const field of expr.fields ?? []) {
      validateTypeExprRefs(
        field.type,
        ownerNodeId,
        ownerNamespace,
        ownerTypeParams,
        table,
        usingNamespaces,
        references,
        issues,
        line,
        column,
      );
    }
    return;
  }

  if (expr.kind !== "ref") return;

  if (expr.qname === "array") {
    validateTypeExprRefs(
      expr.elementType,
      ownerNodeId,
      ownerNamespace,
      ownerTypeParams,
      table,
      usingNamespaces,
      references,
      issues,
      line,
      column,
    );
    return;
  }

  const raw = expr.qname ?? "";
  const name = normalizeTypeName(raw);
  if (ownerTypeParams.has(name)) {
    return;
  }

  const resolved = resolveSymbolByName(raw, ownerNamespace, usingNamespaces, table);
  if (resolved.ambiguous) {
    issues.push({
      code: "AMBIGUOUS",
      message: `Ambiguous type reference '${raw}' matches: ${resolved.ambiguous.map((s) => s.qname).join(", ")}`,
      line,
      column,
    });
  } else if (resolved.symbol) {
    references.push({
      fromNodeId: ownerNodeId,
      kind: "type_ref",
      text: raw,
      targetSymbolId: resolved.symbol.id,
      line,
      column,
    });
  }

  for (const arg of expr.args ?? []) {
    validateTypeExprRefs(
      arg,
      ownerNodeId,
      ownerNamespace,
      ownerTypeParams,
      table,
      usingNamespaces,
      references,
      issues,
      line,
      column,
    );
  }
}

function resolveIdentifier(
  text: string,
  context: "value" | "callee",
  scope: Set<string>,
  ownerNodeId: string,
  namespace: string,
  usingNamespaces: string[],
  table: SymbolTable,
  references: ResolvedReference[],
  issues: ResolutionIssue[],
  line: number,
  column: number,
): void {
  if (scope.has(text)) {
    references.push({
      fromNodeId: ownerNodeId,
      kind: "local",
      text,
      line,
      column,
    });
    return;
  }

  const resolved = resolveSymbolByName(text, namespace, usingNamespaces, table);
  if (resolved.ambiguous) {
    issues.push({
      code: "AMBIGUOUS",
      message: `Ambiguous identifier '${text}' matches: ${resolved.ambiguous.map((s) => s.qname).join(", ")}`,
      line,
      column,
    });
    references.push({ fromNodeId: ownerNodeId, kind: "unbound", text, line, column });
    return;
  }

  if (resolved.symbol) {
    references.push({
      fromNodeId: ownerNodeId,
      kind: "symbol",
      text,
      targetSymbolId: resolved.symbol.id,
      line,
      column,
    });
    return;
  }

  if (context === "callee" && (KNOWN_CALLEES.has(text) || /^[a-z_][A-Za-z0-9_]*$/.test(text))) {
    return;
  }

  if (/^[A-Z_][A-Z0-9_]*$/.test(text)) {
    return;
  }

  references.push({ fromNodeId: ownerNodeId, kind: "unbound", text, line, column });
  issues.push({
    code: "UNBOUND_VARIABLE",
    message: `Unresolved identifier '${text}'`,
    line,
    column,
  });
}

function collectExprReferences(
  expr: Expr | undefined,
  ownerNodeId: string,
  namespace: string,
  scope: Set<string>,
  table: SymbolTable,
  usingNamespaces: string[],
  references: ResolvedReference[],
  issues: ResolutionIssue[],
  line: number,
  column: number,
  context: "value" | "callee" = "value",
): void {
  if (!expr) return;

  if (expr.kind === "identifier" && expr.name) {
    resolveIdentifier(
      expr.name,
      context,
      scope,
      ownerNodeId,
      namespace,
      usingNamespaces,
      table,
      references,
      issues,
      line,
      column,
    );
    return;
  }

  if (expr.kind === "new") {
    const typeName = expr.typeName ?? "";
    const resolved = resolveSymbolByName(typeName, namespace, usingNamespaces, table);
    if (resolved.ambiguous) {
      issues.push({
        code: "AMBIGUOUS",
        message: `Ambiguous constructor type '${typeName}' matches: ${resolved.ambiguous.map((s) => s.qname).join(", ")}`,
        line,
        column,
      });
      references.push({ fromNodeId: ownerNodeId, kind: "unbound", text: typeName, line, column });
    } else if (resolved.symbol) {
      references.push({
        fromNodeId: ownerNodeId,
        kind: "constructor",
        text: typeName,
        targetSymbolId: resolved.symbol.id,
        line,
        column,
      });
    }
  }

  if (expr.kind === "call") {
    collectExprReferences(
      expr.callee,
      ownerNodeId,
      namespace,
      scope,
      table,
      usingNamespaces,
      references,
      issues,
      line,
      column,
      "callee",
    );
    for (const arg of expr.callArgs ?? []) {
      collectExprReferences(arg.value, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
    }
    return;
  }

  collectExprReferences(expr.left, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  collectExprReferences(expr.right, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  collectExprReferences(expr.test, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  collectExprReferences(expr.consequent, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  collectExprReferences(expr.alternate, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  collectExprReferences(expr.callee, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  collectExprReferences(expr.object, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  collectExprReferences(expr.index, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  collectExprReferences(expr.source, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  collectExprReferences(expr.time, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  collectExprReferences(expr.confidence, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  collectExprReferences(expr.matchExpr, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  collectExprReferences(expr.itemExpr, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  collectExprReferences(expr.iterable, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  collectExprReferences(expr.filter, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);

  for (const arg of expr.args ?? []) {
    collectExprReferences(arg, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  }
  for (const field of expr.fields ?? []) {
    collectExprReferences(field.value, ownerNodeId, namespace, scope, table, usingNamespaces, references, issues, line, column);
  }
  for (const c of expr.cases ?? []) {
    const caseScope = new Set(scope);
    for (const name of collectPatternNames(c.pattern)) {
      caseScope.add(name);
    }
    collectExprReferences(c.guard, ownerNodeId, namespace, caseScope, table, usingNamespaces, references, issues, line, column);
    collectExprReferences(c.value, ownerNodeId, namespace, caseScope, table, usingNamespaces, references, issues, line, column);
  }
}

export function resolveProgram(program: Program): ResolutionResult {
  const { symbols, table, usingNamespaces } = collectSymbols(program);
  const issues: ResolutionIssue[] = [];
  const references: ResolvedReference[] = [];

  const nodes: BoundNode[] = [];
  const edges: BoundEdge[] = [];
  let stmtCounter = 0;

  for (const symbol of symbols) {
    nodes.push({
      id: symbol.id,
      kind: "decl",
      label: `${symbol.kind} ${symbol.qname}`,
      line: symbol.line,
      column: symbol.column,
    });
  }

  const walkStatements = (statements: Statement[] | undefined, namespace: string, parentNodeId: string, incomingScope: Set<string>): void => {
    if (!statements) return;
    const scope = new Set(incomingScope);

    for (const stmt of statements) {
      const stmtId = makeStmtId(++stmtCounter);
      nodes.push({
        id: stmtId,
        kind: "stmt",
        label: stmt.kind,
        line: stmt.loc.line,
        column: stmt.loc.column,
      });
      edges.push({ from: parentNodeId, to: stmtId, kind: "contains" });

      if (stmt.kind !== "foreach") {
        collectExprReferences(
          stmt.expr,
          stmtId,
          namespace,
          scope,
          table,
          usingNamespaces,
          references,
          issues,
          stmt.loc.line,
          stmt.loc.column,
        );
      }

      collectExprReferences(
        stmt.iterable,
        stmtId,
        namespace,
        scope,
        table,
        usingNamespaces,
        references,
        issues,
        stmt.loc.line,
        stmt.loc.column,
      );
      collectExprReferences(
        stmt.query?.target,
        stmtId,
        namespace,
        scope,
        table,
        usingNamespaces,
        references,
        issues,
        stmt.loc.line,
        stmt.loc.column,
      );
      collectExprReferences(
        stmt.query?.where,
        stmtId,
        namespace,
        scope,
        table,
        usingNamespaces,
        references,
        issues,
        stmt.loc.line,
        stmt.loc.column,
      );
      collectExprReferences(
        stmt.query?.constraints,
        stmtId,
        namespace,
        scope,
        table,
        usingNamespaces,
        references,
        issues,
        stmt.loc.line,
        stmt.loc.column,
      );
      collectExprReferences(
        stmt.query?.response_format,
        stmtId,
        namespace,
        scope,
        table,
        usingNamespaces,
        references,
        issues,
        stmt.loc.line,
        stmt.loc.column,
      );
      collectExprReferences(
        stmt.query?.hint,
        stmtId,
        namespace,
        scope,
        table,
        usingNamespaces,
        references,
        issues,
        stmt.loc.line,
        stmt.loc.column,
      );

      for (const param of stmt.params ?? []) {
        collectExprReferences(
          param.defaultValue,
          stmtId,
          namespace,
          scope,
          table,
          usingNamespaces,
          references,
          issues,
          stmt.loc.line,
          stmt.loc.column,
        );
      }
      for (const hint of stmt.hints ?? []) {
        collectExprReferences(
          hint,
          stmtId,
          namespace,
          scope,
          table,
          usingNamespaces,
          references,
          issues,
          stmt.loc.line,
          stmt.loc.column,
        );
      }

      if (stmt.kind === "let") {
        for (const name of collectPatternNames(stmt.pattern)) {
          scope.add(name);
        }
      }

      if (stmt.kind === "fact" && stmt.name) {
        scope.add(stmt.name);
      }

      if (stmt.kind === "rule") {
        const bodyScope = new Set(scope);
        for (const param of stmt.params ?? []) {
          bodyScope.add(param.name);
        }
        walkStatements(stmt.body, namespace, stmtId, bodyScope);
      } else if (stmt.kind === "foreach") {
        const bodyScope = new Set(scope);
        for (const name of collectPatternNames(stmt.pattern)) {
          bodyScope.add(name);
        }
        walkStatements(stmt.body, namespace, stmtId, bodyScope);
      } else {
        walkStatements(stmt.body, namespace, stmtId, new Set(scope));
      }

      walkStatements(stmt.elseBody, namespace, stmtId, new Set(scope));
    }
  };

  const walkDecls = (decls: TopDecl[], namespace: string, parentNodeId?: string): void => {
    for (const decl of decls) {
      if (decl.kind === "namespace") {
        const nsQname = decl.name ? (namespace ? `${namespace}.${decl.name}` : decl.name) : namespace;
        const nsId = `ns:${nsQname || "root"}`;
        nodes.push({ id: nsId, kind: "namespace", label: `namespace ${nsQname || "root"}`, line: decl.loc.line, column: decl.loc.column });
        if (parentNodeId) {
          edges.push({ from: parentNodeId, to: nsId, kind: "contains" });
        }
        if (decl.declarations) {
          walkDecls(decl.declarations, nsQname, nsId);
        }
        continue;
      }

      const qname = decl.name ? (namespace ? `${namespace}.${decl.name}` : decl.name) : "";
      const symbolNodeId = decl.name && !["using", "import", "expand"].includes(decl.kind)
        ? makeSymbolId(qname, decl.kind)
        : undefined;

      if (parentNodeId && symbolNodeId) {
        edges.push({ from: parentNodeId, to: symbolNodeId, kind: "contains" });
      }

      if (decl.kind === "type" && symbolNodeId) {
        validateTypeExprRefs(
          decl.typeExpr,
          symbolNodeId,
          namespace,
          new Set(decl.typeParams ?? []),
          table,
          usingNamespaces,
          references,
          issues,
          decl.loc.line,
          decl.loc.column,
        );
      }

      if (decl.kind === "struct" && symbolNodeId) {
        for (const field of decl.structFields ?? []) {
          validateTypeExprRefs(
            field.type,
            symbolNodeId,
            namespace,
            new Set(),
            table,
            usingNamespaces,
            references,
            issues,
            decl.loc.line,
            decl.loc.column,
          );
        }
      }

      if (symbolNodeId) {
        walkStatements(decl.body, namespace, symbolNodeId, new Set());
      }

      if (decl.declarations) {
        walkDecls(decl.declarations, namespace, symbolNodeId ?? parentNodeId);
      }
    }
  };

  // Flatten once to ensure root namespace node exists only when needed.
  const allDecls = flattenDecls(program.declarations);
  if (allDecls.length > 0) {
    nodes.push({ id: "ns:root", kind: "namespace", label: "namespace root", line: 1, column: 1 });
    walkDecls(program.declarations, "", "ns:root");
  }

  for (const ref of references) {
    if (ref.targetSymbolId) {
      edges.push({ from: ref.fromNodeId, to: ref.targetSymbolId, kind: "references", detail: `${ref.kind}:${ref.text}` });
    }
  }

  return {
    symbols,
    references,
    issues,
    graph: { nodes, edges },
  };
}
