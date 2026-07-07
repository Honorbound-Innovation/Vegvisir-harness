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

test("resolves retained inheritance and expand references", () => {
  const program = parseUsrl(`
    template Pair(left: "a") { fact Left = left; }
    contract Parent { section S { fact X = 1; } }
    contract Child extends Parent { section S { fact Y = 2; } }
    expand Pair("b");
  `);

  const resolution = resolveProgram(program);
  assert.equal(resolution.issues.length, 0);
  assert.ok(resolution.references.some((r) => r.kind === "symbol" && r.text === "Parent"));
  assert.ok(resolution.references.some((r) => r.kind === "symbol" && r.text === "Pair"));
  assert.ok(resolution.graph.edges.some((e) => e.detail === "symbol:Parent"));
  assert.ok(resolution.graph.edges.some((e) => e.detail === "symbol:Pair"));
});

test("comprehension pattern bindings are scoped in resolver", () => {
  const program = parseUsrl(`
    contract C {
      section S {
        fact Numbers = [1, 2, 3];
        fact Doubled = [x * 2 for x in Numbers];
      }
    }
  `);

  const resolution = resolveProgram(program);
  assert.ok(resolution.references.some((r) => r.kind === "local" && r.text === "x"));
  assert.ok(!resolution.issues.some((i) => i.message.includes("Unresolved identifier 'x'")));
});
