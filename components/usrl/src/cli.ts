#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { parseUsrl } from "./parser.js";
import { parsePll, parseCll, validatePair, validatePll, validateCll } from "./linked.js";
import { validateProgram } from "./validator.js";
import { resolveProject } from "./project-resolver.js";
import { evaluateProgram } from "./runtime.js";
import { applyJsrtFrames, parseJsrtDocument } from "./jsrt.js";
import { UsrlError } from "./errors.js";

function usage(): string {
  return [
    "USRL CLI",
    "",
    "Commands:",
    "  usrl validate <file.usrl|file.pll|file.cll>",
    "  usrl validate-pair <file.pll> <file.cll>",
    "  usrl resolve <file.usrl>",
    "  usrl run <file.usrl>",
    "  usrl jsrt-validate <file.jsrt|file.json>",
    "  usrl jsrt-apply <file.jsrt|file.json>",
  ].join("\n");
}

function read(pathLike: string): string {
  return readFileSync(resolve(pathLike), "utf8");
}

function printIssues(issues: Array<{ code: string; message: string }>): void {
  for (const issue of issues) {
    console.error(`${issue.code}: ${issue.message}`);
  }
}

function run(): number {
  const [, , command, ...rest] = process.argv;
  if (!command) {
    console.error(usage());
    return 1;
  }

  try {
    if (command === "validate") {
      if (rest.length !== 1) {
        console.error("validate requires exactly one file path");
        console.error(usage());
        return 1;
      }

      const filePath = rest[0];
      const source = read(filePath);

      if (filePath.endsWith(".usrl")) {
        const program = parseUsrl(source);
        const issues = validateProgram(program);
        if (issues.length > 0) {
          printIssues(issues);
          return 2;
        }
        console.log(`OK: ${filePath}`);
        return 0;
      }

      if (filePath.endsWith(".pll")) {
        const pll = parsePll(source);
        const issues = validatePll(pll);
        if (issues.length > 0) {
          printIssues(issues);
          return 2;
        }
        console.log(`OK: ${filePath}`);
        return 0;
      }

      if (filePath.endsWith(".cll")) {
        const cll = parseCll(source);
        const issues = validateCll(cll);
        if (issues.length > 0) {
          printIssues(issues);
          return 2;
        }
        console.log(`OK: ${filePath}`);
        return 0;
      }

      console.error("Unsupported file extension. Use .usrl, .pll, or .cll");
      return 1;
    }

    if (command === "validate-pair") {
      if (rest.length !== 2) {
        console.error("validate-pair requires two file paths: <file.pll> <file.cll>");
        console.error(usage());
        return 1;
      }
      const [pllPath, cllPath] = rest;
      if (!pllPath.endsWith(".pll") || !cllPath.endsWith(".cll")) {
        console.error("validate-pair expects a .pll file then a .cll file");
        return 1;
      }

      const pll = parsePll(read(pllPath));
      const cll = parseCll(read(cllPath));
      const issues = validatePair(pll, cll);
      if (issues.length > 0) {
        printIssues(issues);
        return 2;
      }
      console.log(`OK: ${pllPath} + ${cllPath}`);
      return 0;
    }

    if (command === "resolve") {
      if (rest.length !== 1) {
        console.error("resolve requires exactly one .usrl file path");
        console.error(usage());
        return 1;
      }
      const filePath = rest[0];
      if (!filePath.endsWith(".usrl")) {
        console.error("resolve expects a .usrl file");
        return 1;
      }

      const project = resolveProject(filePath);
      const payload = {
        file: filePath,
        entry_file: project.entry_file,
        module_count: project.module_count,
        modules: project.modules.map((m) => ({ filePath: m.filePath, imports: m.imports })),
        validation_issue_count: project.validation_issues.length,
        resolution_issue_count: project.resolution.issues.length,
        loader_issue_count: project.loader_issues.length,
        symbol_count: project.resolution.symbols.length,
        reference_count: project.resolution.references.length,
        graph_node_count: project.resolution.graph.nodes.length,
        graph_edge_count: project.resolution.graph.edges.length,
        validation_issues: project.validation_issues,
        resolution_issues: project.resolution.issues,
        loader_issues: project.loader_issues,
        symbols: project.resolution.symbols,
        references: project.resolution.references,
        graph: project.resolution.graph,
      };
      console.log(JSON.stringify(payload, null, 2));
      return project.validation_issues.length > 0 || project.resolution.issues.length > 0 || project.loader_issues.length > 0 ? 2 : 0;
    }

    if (command === "run") {
      if (rest.length !== 1) {
        console.error("run requires exactly one .usrl file path");
        console.error(usage());
        return 1;
      }
      const filePath = rest[0];
      if (!filePath.endsWith(".usrl")) {
        console.error("run expects a .usrl file");
        return 1;
      }

      const project = resolveProject(filePath);
      const runtime = evaluateProgram(project.program);
      const payload = {
        file: filePath,
        entry_file: project.entry_file,
        module_count: project.module_count,
        loader_issue_count: project.loader_issues.length,
        validation_issue_count: project.validation_issues.length,
        resolution_issue_count: project.resolution.issues.length,
        runtime_issue_count: runtime.issues.length,
        iterations: runtime.iterations,
        fact_count: Object.keys(runtime.facts).length,
        tainted_facts: runtime.tainted_facts,
        event_count: runtime.events.length,
        query_count: runtime.queries.length,
        derivation_count: runtime.derivations.length,
        decision_count: runtime.decisions.length,
        facts: runtime.facts,
        events: runtime.events,
        queries: runtime.queries,
        derivations: runtime.derivations,
        decisions: runtime.decisions,
        loader_issues: project.loader_issues,
        validation_issues: project.validation_issues,
        resolution_issues: project.resolution.issues,
        runtime_issues: runtime.issues,
      };
      console.log(JSON.stringify(payload, null, 2));
      return project.loader_issues.length > 0 ||
        project.validation_issues.length > 0 ||
        project.resolution.issues.length > 0 ||
        runtime.issues.length > 0
        ? 2
        : 0;
    }

    if (command === "jsrt-validate" || command === "jsrt-apply") {
      if (rest.length !== 1) {
        console.error(`${command} requires exactly one file path`);
        console.error(usage());
        return 1;
      }
      const filePath = rest[0];
      const raw = read(filePath);
      let doc: unknown;
      try {
        doc = JSON.parse(raw);
      } catch {
        console.error("Invalid JSON");
        return 2;
      }

      const parsed = parseJsrtDocument(doc, { hmac_secret: process.env.JSRT_HMAC_SECRET });
      const jsrtContext = {
        profile: parsed.document.profile,
        prompt_registry: parsed.document.prompt_registry,
        contract_registry: parsed.document.contract_registry,
        hmac_secret: process.env.JSRT_HMAC_SECRET,
      };
      if (command === "jsrt-validate") {
        const payload = {
          file: filePath,
          frame_count: parsed.document.frames.length,
          issue_count: parsed.issues.length,
          issues: parsed.issues,
        };
        console.log(JSON.stringify(payload, null, 2));
        return parsed.issues.length > 0 ? 2 : 0;
      }

      const applied = applyJsrtFrames(parsed.document.frames, jsrtContext);
      const allIssues = [...parsed.issues, ...applied.issues];
      const payload = {
        file: filePath,
        frame_count: parsed.document.frames.length,
        accepted_count: applied.accepted.length,
        rejected_count: applied.rejected.length,
        issue_count: allIssues.length,
        accepted: applied.accepted,
        rejected: applied.rejected,
        snapshot: applied.snapshot ?? null,
        issues: allIssues,
      };
      console.log(JSON.stringify(payload, null, 2));
      return allIssues.length > 0 ? 2 : 0;
    }

    console.error(`Unknown command: ${command}`);
    console.error(usage());
    return 1;
  } catch (error) {
    if (error instanceof UsrlError) {
      console.error(error.message);
      return 2;
    }
    console.error(error instanceof Error ? error.message : String(error));
    return 2;
  }
}

const code = run();
if (code !== 0) {
  process.exit(code);
}
