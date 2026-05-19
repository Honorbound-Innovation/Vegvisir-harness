import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { resolveProject } from "../src/project-resolver.js";

test("resolves across imported files", () => {
  const dir = mkdtempSync(join(tmpdir(), "usrl-project-"));
  const lib = join(dir, "lib.usrl");
  const main = join(dir, "main.usrl");

  writeFileSync(
    lib,
    `
      namespace Lib {
        struct AgentSpec { string AgentId; }
      }
    `,
    "utf8",
  );

  writeFileSync(
    main,
    `
      import "./lib.usrl";
      contract C {
        section S {
          fact A = new AgentSpec { AgentId = "x" };
        }
      }
    `,
    "utf8",
  );

  const result = resolveProject(main);
  assert.equal(result.loader_issues.length, 0);
  assert.equal(result.module_count, 2);
  assert.ok(result.resolution.references.some((r) => r.kind === "constructor" && r.text === "AgentSpec"));
});

test("reports missing import files", () => {
  const dir = mkdtempSync(join(tmpdir(), "usrl-project-"));
  const main = join(dir, "main.usrl");

  writeFileSync(
    main,
    `
      import "./missing.usrl";
      contract C { section S { fact A = 1; } }
    `,
    "utf8",
  );

  const result = resolveProject(main);
  assert.ok(result.loader_issues.length > 0);
  assert.ok(result.loader_issues[0].message.includes("Import file not found"));
});
