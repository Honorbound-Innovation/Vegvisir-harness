import test from "node:test";
import assert from "node:assert/strict";

import { parseUsrl } from "../src/parser.js";
import { evaluateProgram } from "../src/runtime.js";

test("evaluates facts and fixed-point rules", () => {
  const program = parseUsrl(`
    contract Runtime {
      section Seed {
        fact Numbers = [1, 2, 3, 4];
      }

      section Rules {
        rule Derive {
          when count(Numbers) > 0 {
            fact Total = sum(Numbers);
            if (Total > 5) {
              fact Status = "ok";
            }
          }
        }
      }
    }
  `);

  const result = evaluateProgram(program);
  assert.equal(result.issues.length, 0);
  assert.equal(result.facts.Total, 10);
  assert.equal(result.facts.Status, "ok");
  assert.ok(result.iterations >= 1);
});

test("supports foreach and comprehensions in runtime", () => {
  const program = parseUsrl(`
    contract Runtime {
      section Seed {
        fact Numbers = [1, 2, 3, 4];
        fact Evens = [x for x in Numbers if x % 2 == 0];
      }

      section Pass {
        foreach (n in Evens) {
          fact LastEven = n;
        }
      }
    }
  `);

  const result = evaluateProgram(program);
  assert.equal(result.facts.LastEven, 4);
  assert.deepEqual(result.facts.Evens, [2, 4]);
});

test("captures query results and fact derivations", () => {
  const program = parseUsrl(`
    contract Runtime {
      section S {
        fact Numbers = [1, 2, 3];
        query {
          target: sum(Numbers);
          response_format: FACT;
        }
        query {
          target: count(Numbers) > 2;
          response_format: BOOLEAN;
        }
      }
    }
  `);

  const result = evaluateProgram(program);
  assert.equal(result.queries.length, 2);
  assert.equal(result.queries[0].result, 6);
  assert.equal(result.queries[1].result, true);
  assert.ok(result.derivations.some((d) => d.fact === "Numbers"));
});

test("tracks taint and flags unsafe tainted control/sinks", () => {
  const program = parseUsrl(`
    contract Runtime {
      section S {
        fact Raw = untrusted("DROP");
        if (Raw == "DROP") {
          emit Raw;
        }
        fact Safe = sanitize(Raw);
        emit Safe;
      }
    }
  `);

  const result = evaluateProgram(program);
  assert.ok(result.tainted_facts.includes("Raw"));
  assert.ok(!result.tainted_facts.includes("Safe"));
  assert.ok(result.issues.some((i) => i.code === "INJECTION_PATTERN" && i.message.includes("if condition")));
  assert.ok(result.issues.some((i) => i.code === "INJECTION_PATTERN" && i.message.includes("emit")));
});

test("supports triggers, temporal calls, and deontic conflicts", () => {
  const program = parseUsrl(`
    contract Runtime {
      section Seed {
        fact Start = date("2026-01-01T00:00:00Z");
        fact End = date("2026-01-02T00:00:00Z");
        fact Now = date("2026-01-01T12:00:00Z");
      }

      section Rules {
        rule R {
          when before(Start, End) {
            fact Ok = true;
          }
          permit "action:a";
          deny "action:a";
        }
      }

      section Triggers {
        trigger OnOk {
          when changed("Ok") {
            fact Triggered = true;
          }
        }
      }
    }
  `);

  const result = evaluateProgram(program);
  assert.equal(result.facts.Ok, true);
  assert.equal(result.facts.Triggered, true);
  assert.ok(result.decisions.length >= 2);
  assert.ok(result.issues.some((i) => i.message.includes("Conflicting permit/deny decision")));
});
