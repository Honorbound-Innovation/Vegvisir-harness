import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

function runCli(args: string[]): { status: number | null; stdout: string; stderr: string } {
  const cliPath = resolve(process.cwd(), "dist/src/cli.js");
  const result = spawnSync(process.execPath, [cliPath, ...args], { encoding: "utf8" });
  return { status: result.status, stdout: result.stdout, stderr: result.stderr };
}

test("cli validate handles .usrl success", () => {
  const dir = mkdtempSync(join(tmpdir(), "usrl-cli-"));
  const file = join(dir, "ok.usrl");
  writeFileSync(
    file,
    `contract Guard { section A { fact X = 1; query { target: can(X); response_format: BOOLEAN; } } }`,
    "utf8",
  );

  const result = runCli(["validate", file]);
  assert.equal(result.status, 0);
  assert.match(result.stdout, /OK:/);
});

test("cli validate-pair reports unresolved target", () => {
  const dir = mkdtempSync(join(tmpdir(), "usrl-cli-"));
  const pll = join(dir, "a.pll");
  const cll = join(dir, "a.cll");

  writeFileSync(
    pll,
    `
      version "2.0";
      library PromptLibrary A {
        metadata {
          Id: "pair-1"; Name: "A"; Version: "1"; Created: "2026-01-01"; Modified: "2026-01-01";
          Author: "x"; Source: "s"; Purpose: "p";
        }
        prompt P {
          Id: "P.A"; Title: "t"; Purpose: "p"; Type: "Implementation";
          Inputs: []; Outputs: []; Body: "b"; Links: [];
        }
      }
    `,
    "utf8",
  );

  writeFileSync(
    cll,
    `
      version "2.0";
      library ContractLibrary A {
        metadata {
          Id: "pair-1"; Name: "A"; Version: "1"; Created: "2026-01-01"; Modified: "2026-01-01";
          Author: "x"; Source: "s"; Purpose: "p";
        }
        contract_unit C {
          Id: "C.A"; Title: "t"; Purpose: "p"; Scope: "Prompt";
          Rules: [{ Id: "R1"; Category: "Required"; Condition: "true"; Action: "accept"; Severity: "Error"; Target: "P.Missing"; }];
          Targets: ["P.Missing"];
          Validation: [{ Type: "schema"; Blocking: true; }];
          Links: [{ Type: "Governs"; Target: "P.Missing"; Blocking: true; }];
        }
      }
    `,
    "utf8",
  );

  const result = runCli(["validate-pair", pll, cll]);
  assert.equal(result.status, 2);
  assert.match(result.stderr, /unresolved target 'P\.Missing'/);
});

test("cli resolve prints graph summary json", () => {
  const dir = mkdtempSync(join(tmpdir(), "usrl-cli-"));
  const file = join(dir, "resolve.usrl");
  writeFileSync(
    file,
    `
      struct AgentSpec { string AgentId; }
      contract C { section S { fact A = new AgentSpec { AgentId = "x" }; } }
    `,
    "utf8",
  );

  const result = runCli(["resolve", file]);
  assert.equal(result.status, 0);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.file, file);
  assert.equal(payload.validation_issue_count, 0);
  assert.equal(payload.resolution_issue_count, 0);
  assert.ok(payload.symbol_count >= 2);
  assert.ok(payload.reference_count >= 1);
});

test("cli resolve loads imported modules", () => {
  const dir = mkdtempSync(join(tmpdir(), "usrl-cli-"));
  const lib = join(dir, "lib.usrl");
  const main = join(dir, "main.usrl");

  writeFileSync(lib, `namespace Lib { struct AgentSpec { string AgentId; } }`, "utf8");
  writeFileSync(
    main,
    `
      import "./lib.usrl";
      contract C { section S { fact A = new AgentSpec { AgentId = "x" }; } }
    `,
    "utf8",
  );

  const result = runCli(["resolve", main]);
  assert.equal(result.status, 0);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.module_count, 2);
  assert.equal(payload.loader_issue_count, 0);
});

test("cli run executes runtime and returns facts", () => {
  const dir = mkdtempSync(join(tmpdir(), "usrl-cli-"));
  const file = join(dir, "run.usrl");
  writeFileSync(
    file,
    `
      contract C {
        section S {
          fact Numbers = untrusted([1, 2, 3]);
          query { target: sum(Numbers); response_format: FACT; }
          rule Derive {
            when count(Numbers) > 0 {
              fact Total = sum(Numbers);
            }
          }
        }
      }
    `,
    "utf8",
  );

  const result = runCli(["run", file]);
  assert.equal(result.status, 2);
  const payload = JSON.parse(result.stdout);
  assert.ok(payload.runtime_issue_count >= 1);
  assert.equal(payload.facts.Total, 6);
  assert.equal(payload.query_count, 1);
  assert.ok(payload.derivation_count >= 2);
  assert.ok(payload.tainted_facts.includes("Numbers"));
});

test("cli jsrt-validate and jsrt-apply handle stream files", () => {
  const dir = mkdtempSync(join(tmpdir(), "usrl-cli-"));
  const file = join(dir, "session.jsrt");
  writeFileSync(
    file,
    JSON.stringify(
      [
        {
          jsrt_version: "0.1",
          frame_id: "f1",
          session_id: "s1",
          sequence: 1,
          frame_type: "SessionCreate",
          timestamp: "2026-04-10T00:00:00Z",
          state: "Created",
          payload: { profile: "StrictDeterministic", purpose: "cli-session" },
        },
        {
          jsrt_version: "0.1",
          frame_id: "f2",
          session_id: "s1",
          sequence: 2,
          frame_type: "SessionInit",
          timestamp: "2026-04-10T00:00:01Z",
          state: "Initialized",
          payload: {},
        },
      ],
      null,
      2,
    ),
    "utf8",
  );

  const validateResult = runCli(["jsrt-validate", file]);
  assert.equal(validateResult.status, 0);
  const validatePayload = JSON.parse(validateResult.stdout);
  assert.equal(validatePayload.issue_count, 0);
  assert.equal(validatePayload.frame_count, 2);

  const applyResult = runCli(["jsrt-apply", file]);
  assert.equal(applyResult.status, 0);
  const applyPayload = JSON.parse(applyResult.stdout);
  assert.equal(applyPayload.accepted_count, 2);
  assert.equal(applyPayload.rejected_count, 0);
  assert.equal(applyPayload.snapshot.state, "Initialized");
});
