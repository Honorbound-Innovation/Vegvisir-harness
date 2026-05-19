import test from "node:test";
import assert from "node:assert/strict";

import { parseUsrl, validateProgram } from "../src/index.js";

test("parses canonical top-level declarations", () => {
  const program = parseUsrl(`
    using HarnessOS.CMS;
    import "business_logic.usrl" as biz;

    namespace HarnessOS.Teams {
      contract TeamContract {
        section Metadata {
          fact ContractId = "orchestrator-v1";
        }
      }
    }

    contract Orchestrator {
      section AgentDefinition {
        fact Role = "lead";
      }
    }
  `);

  assert.equal(program.declarations.length, 4);
  assert.equal(program.declarations[0].kind, "using");
  assert.equal(program.declarations[1].kind, "import");
  assert.equal(program.declarations[2].kind, "namespace");
  assert.equal(program.declarations[3].kind, "contract");
  assert.ok(program.declarations[2].declarations);
  assert.equal(program.declarations[2].declarations?.[0]?.kind, "contract");
});

test("detects duplicate declarations", () => {
  const program = parseUsrl(`
    contract Guard { section A { fact X = 1; } }
    contract Guard { section B { fact Y = 2; } }
  `);

  const issues = validateProgram(program);
  assert.ok(issues.some((issue) => issue.message.includes("Duplicate declaration 'Guard'")));
});

test("parses query/if/foreach/parallel statements", () => {
  const program = parseUsrl(`
    contract Advanced {
      section Q {
        query {
          target: can_reach("Internet", "DB", "ANY");
          constraints: not blocked("Internet", "DB");
          response_format: BOOLEAN;
          hint: use_index;
        }

        if (count(users) > 0) {
          emit alert("has-users");
        } else {
          emit alert("empty");
        }

        when count(users) > 1 {
          emit alert("many");
        }

        foreach (u in users) {
          require active(u);
        }

        parallel {
          fact A = 1;
        }
      }
    }
  `);

  const contract = program.declarations[0];
  assert.equal(contract.kind, "contract");
  assert.ok(contract.body);
  const section = contract.body?.find((s) => s.kind === "section");
  assert.ok(section);
  assert.ok(section?.body?.some((s) => s.kind === "query"));
  assert.ok(section?.body?.some((s) => s.kind === "if"));
  assert.ok(section?.body?.some((s) => s.kind === "when"));
  assert.ok(section?.body?.some((s) => s.kind === "foreach"));
  assert.ok(section?.body?.some((s) => s.kind === "parallel"));

  const ifStmt = section?.body?.find((s) => s.kind === "if");
  assert.equal(ifStmt?.expr?.kind, "binary");
  const queryStmt = section?.body?.find((s) => s.kind === "query");
  assert.equal(queryStmt?.query?.target?.kind, "call");
});

test("builds expression AST with precedence and postfix", () => {
  const program = parseUsrl(`
    contract Exprs {
      section S {
        fact A = user?.address.city ?? fallback(1 + 2 * 3);
      }
    }
  `);

  const contract = program.declarations[0];
  const section = contract.body?.[0];
  const fact = section?.body?.[0];
  assert.equal(fact?.kind, "fact");
  assert.equal(fact?.expr?.kind, "coalesce");
  assert.equal(fact?.expr?.left?.kind, "member");
  assert.equal(fact?.expr?.right?.kind, "call");
});

test("parses named arguments and rule params/hints", () => {
  const program = parseUsrl(`
    contract Rules {
      section S {
        rule Build(string id, retries: 3) hint use_index; hint "fast"; {
          emit run(target: id, retry: retries);
        }
      }
    }
  `);

  const rule = program.declarations[0].body?.[0].body?.[0];
  assert.equal(rule?.kind, "rule");
  assert.equal(rule?.params?.length, 2);
  assert.equal(rule?.params?.[0].type, "string");
  assert.equal(rule?.params?.[0].name, "id");
  assert.equal(rule?.hints?.length, 2);

  const emitStmt = rule?.body?.[0];
  assert.equal(emitStmt?.kind, "emit");
  assert.equal(emitStmt?.expr?.kind, "call");
  assert.equal(emitStmt?.expr?.callArgs?.[0]?.name, "target");
});

test("parses let patterns, foreach patterns, match, and comprehensions", () => {
  const program = parseUsrl(`
    contract Patterns {
      section S {
        let (a, b) = getPair();
        foreach ({ id, role: r } in users) {
          emit process(id: id, role: r);
        }
        fact Choice = match role {
          case admin => "all";
          case user when active(userId) => "limited";
        };
        fact Evens = [x * 2 for x in 1..10 if x % 2 == 0];
      }
    }
  `);

  const section = program.declarations[0].body?.[0];
  assert.equal(section?.kind, "section");

  const letStmt = section?.body?.[0];
  assert.equal(letStmt?.kind, "let");
  assert.equal(letStmt?.pattern?.kind, "tuple");

  const foreachStmt = section?.body?.[1];
  assert.equal(foreachStmt?.kind, "foreach");
  assert.equal(foreachStmt?.pattern?.kind, "object");
  assert.equal(foreachStmt?.iterable?.kind, "identifier");

  const matchFact = section?.body?.[2];
  assert.equal(matchFact?.kind, "fact");
  assert.equal(matchFact?.expr?.kind, "match");
  assert.equal(matchFact?.expr?.cases?.length, 2);
  assert.equal(matchFact?.expr?.cases?.[1]?.guard?.kind, "call");

  const compFact = section?.body?.[3];
  assert.equal(compFact?.kind, "fact");
  assert.equal(compFact?.expr?.kind, "comprehension");
  assert.equal(compFact?.expr?.pattern?.kind, "identifier");
});

test("parses type aliases and ADT unions", () => {
  const program = parseUsrl(`
    type UserId = string;
    type Result<T> =
      | Success { value: T; }
      | Error { message: string; };
  `);

  assert.equal(program.declarations[0].kind, "type");
  assert.equal(program.declarations[0].name, "UserId");
  assert.equal(program.declarations[0].typeExpr?.kind, "ref");
  assert.equal(program.declarations[0].typeExpr?.qname, "string");

  const resultType = program.declarations[1];
  assert.equal(resultType.kind, "type");
  assert.equal(resultType.typeParams?.[0], "T");
  assert.equal(resultType.typeExpr?.kind, "union");
  assert.equal(resultType.typeExpr?.variants?.length, 2);
  assert.equal(resultType.typeExpr?.variants?.[0]?.kind, "variant");
  assert.equal(resultType.typeExpr?.variants?.[0]?.name, "Success");
});

test("validates duplicate type params and variants", () => {
  const program = parseUsrl(`
    type Result<T, T> =
      | Ok { value: T; }
      | Ok { value: string; };
  `);

  const issues = validateProgram(program);
  assert.ok(issues.some((i) => i.message.includes("duplicate type parameter 'T'")));
  assert.ok(issues.some((i) => i.message.includes("duplicate variant 'Ok'")));
});

test("parses struct fields and validates constructor compatibility", () => {
  const program = parseUsrl(`
    struct AgentSpec {
      string AgentId;
      string Model;
      string[] Skills;
    }

    contract C {
      section S {
        fact A = new AgentSpec {
          AgentId = "orchestrator",
          Model = "opus",
          Skills = ["delegate"]
        };
      }
    }
  `);

  const structDecl = program.declarations[0];
  assert.equal(structDecl.kind, "struct");
  assert.equal(structDecl.structFields?.length, 3);
  assert.equal(structDecl.structFields?.[2]?.type.kind, "ref");
  assert.equal(structDecl.structFields?.[2]?.type.qname, "array");

  const issues = validateProgram(program);
  assert.equal(issues.length, 0);
});

test("validates unknown type refs, arity mismatch, and bad constructor fields", () => {
  const program = parseUsrl(`
    struct AgentSpec {
      UnknownType Bad;
    }

    type Box<T> = T;
    type BadMap = map<string>;

    contract C {
      section S {
        fact A = new MissingType();
        fact B = new AgentSpec { AgentId = "x", Nope = "y" };
      }
    }
  `);

  const issues = validateProgram(program);
  assert.ok(issues.some((i) => i.message.includes("Unknown type reference 'UnknownType'")));
  assert.ok(issues.some((i) => i.message.includes("expects 2 type argument(s) but got 1")));
  assert.ok(issues.some((i) => i.message.includes("Unknown constructor type 'MissingType'")));
  assert.ok(issues.some((i) => i.message.includes("unknown field 'AgentId'")));
  assert.ok(issues.some((i) => i.message.includes("unknown field 'Nope'")));
});
