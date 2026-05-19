import { existsSync, readFileSync } from "node:fs";
import { dirname, extname, isAbsolute, resolve as resolvePath } from "node:path";

import { Program, TopDecl } from "./ast.js";
import { parseUsrl } from "./parser.js";
import { resolveProgram, type ResolutionResult, type ResolutionIssue } from "./resolver.js";
import { validateProgram, type ValidationIssue } from "./validator.js";

export interface LoadedModule {
  filePath: string;
  program: Program;
  imports: string[];
}

export interface ProjectResolutionResult {
  entry_file: string;
  module_count: number;
  modules: LoadedModule[];
  program: Program;
  validation_issues: ValidationIssue[];
  resolution: ResolutionResult;
  loader_issues: ResolutionIssue[];
}

interface ImportSpec {
  sourcePath: string;
}

function parseImportSpec(name: string): ImportSpec | undefined {
  const selective = name.match(/^\{[^}]+\}\s+from\s+(.+)$/);
  if (selective) {
    return { sourcePath: selective[1].trim() };
  }

  const aliasSplit = name.split(/\s+as\s+/);
  if (aliasSplit.length === 2) {
    return { sourcePath: aliasSplit[0].trim() };
  }

  if (name.trim().length > 0) {
    return { sourcePath: name.trim() };
  }

  return undefined;
}

function extractImports(decls: TopDecl[]): string[] {
  const out: string[] = [];
  for (const decl of decls) {
    if (decl.kind === "import" && decl.name) {
      const spec = parseImportSpec(decl.name);
      if (spec) {
        out.push(spec.sourcePath);
      }
    }
    if (decl.declarations) {
      out.push(...extractImports(decl.declarations));
    }
  }
  return out;
}

function resolveImportPath(baseFile: string, sourcePath: string): string {
  const candidate = isAbsolute(sourcePath)
    ? sourcePath
    : resolvePath(dirname(baseFile), sourcePath);

  if (existsSync(candidate)) {
    return candidate;
  }

  if (!extname(candidate)) {
    const withUsrl = `${candidate}.usrl`;
    if (existsSync(withUsrl)) {
      return withUsrl;
    }
  }

  return candidate;
}

function mergePrograms(modules: LoadedModule[]): Program {
  const declarations: TopDecl[] = [];
  for (const mod of modules) {
    declarations.push(...mod.program.declarations);
  }
  return { declarations };
}

export function resolveProject(entryFile: string): ProjectResolutionResult {
  const root = resolvePath(entryFile);
  const modules: LoadedModule[] = [];
  const visited = new Set<string>();
  const loaderIssues: ResolutionIssue[] = [];

  const loadRecursive = (filePath: string): void => {
    const abs = resolvePath(filePath);
    if (visited.has(abs)) {
      return;
    }
    visited.add(abs);

    if (!existsSync(abs)) {
      loaderIssues.push({
        code: "UNBOUND_VARIABLE",
        message: `Import file not found: ${abs}`,
        line: 1,
        column: 1,
      });
      return;
    }

    const source = readFileSync(abs, "utf8");
    const program = parseUsrl(source);
    const imports = extractImports(program.declarations);
    modules.push({ filePath: abs, program, imports });

    for (const sourceImport of imports) {
      const resolved = resolveImportPath(abs, sourceImport);
      loadRecursive(resolved);
    }
  };

  loadRecursive(root);

  const merged = mergePrograms(modules);
  const validationIssues = validateProgram(merged);
  const resolution = resolveProgram(merged);

  return {
    entry_file: root,
    module_count: modules.length,
    modules,
    program: merged,
    validation_issues: validationIssues,
    resolution,
    loader_issues: loaderIssues,
  };
}
