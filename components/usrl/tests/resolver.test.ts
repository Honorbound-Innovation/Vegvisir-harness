import test from "node:test";
import assert from "node:assert/strict";

import { parseUsrl } from "../src/parser.js";
import { resolveProgram } from "../src/resolver.js";

test("resolves type references and constructors to symbols", () => {
  const program = parseUsrl(`
    namespace A {
      struct AgentSpec {
        string AgentId;
      }

      type SpecAlias = AgentSpec;

      contract C {
        section S {
          fact A = new AgentSpec { AgentId = "x" };
        }
      }
    }
  `);

  const resolution = resolveProgram(program);
  assert.equal(resolution.issues.length, 0);
  assert.ok(resolution.symbols.some((s) => s.qname === "A.AgentSpec"));
  assert.ok(resolution.references.some((r) => r.kind === "type_ref" && r.text === "AgentSpec"));
  assert.ok(resolution.references.some((r) => r.kind === "constructor" && r.text === "AgentSpec"));
  assert.ok(resolution.graph.edges.some((e) => e.kind === "references"));
});

test("reports ambiguous type reference", () => {
  const program = parseUsrl(`
    namespace A { struct User { string Id; } }
    namespace B { struct User { string Id; } }
    type Alias = User;
  `);

  const resolution = resolveProgram(program);
  assert.ok(resolution.issues.some((i) => i.code === "AMBIGUOUS"));
});

test("resolves lexical locals and reports unbound identifiers", () => {
  const program = parseUsrl(`
    contract ScopeDemo {
      section S {
        let user = currentUser();
        fact A = user;
        foreach (item in users) {
          fact B = item;
        }
        fact C = missingVar;
      }
    }
  `);

  const resolution = resolveProgram(program);
  assert.ok(resolution.references.some((r) => r.kind === "local" && r.text === "user"));
  assert.ok(resolution.references.some((r) => r.kind === "local" && r.text === "item"));
  assert.ok(resolution.issues.some((i) => i.code === "UNBOUND_VARIABLE" && i.message.includes("missingVar")));
});
