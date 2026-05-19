# USRL Language Reference

USRL is a contract language for describing bounded agent workflows. In Vegvisir, USRL contracts are used for regulated skills and specialized agents where tool use, stages, evidence, and approval requirements need to be explicit.

This reference matches the current parser, validator, and runtime in `components/usrl/src`.

## File Types

USRL currently has three supported contract-library file families:

```text
.usrl  General USRL contract programs.
.pll   Prompt Library files.
.cll   Contract Library files paired with .pll.
```

The CLI validates them with:

```bash
usrl validate ./contract.usrl
usrl validate ./library.pll
usrl validate ./library.cll
usrl validate-pair ./library.pll ./library.cll
```

## Lexical Rules

Whitespace is insignificant outside strings.

Line comments:

```usrl
// comment
```

Block comments:

```usrl
/* comment */
```

Strings:

```usrl
"single line"
"""multi
line"""
```

Numbers are parsed from digit-leading tokens:

```usrl
1
3.14
2026
```

Identifiers start with a letter or underscore and may contain letters, digits, and underscores.

Most statements end with `;`. Blocks use `{ ... }`.

## Top-Level Declarations

A `.usrl` file is a sequence of top-level declarations:

```text
using <qualified.name>;
import "<path>" [as <alias>];
import { <name>, ... } from "<path>";
namespace <qualified.name> { <top-level declarations> }
library <Name> { <statements> }
definition <Name> { <statements> }
enum <Name> { ... }
struct <Name> { <type> <field>; ... }
type <Name><T, ...> = <type-expression>;
template <Name>(<params>) { <statements> }
expand <TemplateName>(<args>);
contract <Name> [extends|derives <qualified.name>] { <statements> }
```

Example:

```usrl
using Vegvisir.Security;
import "shared.usrl" as shared;

namespace Vegvisir.Agents {
  contract AgentRed {
    section Metadata {
      fact ContractId = "agent-red-default";
    }
  }
}
```

`enum` declarations are accepted and skipped by the current parser. `template` declarations parse parameters and a statement body. `expand` parses arguments and records the template name.

## Contracts

Contracts are the main unit used by Vegvisir:

```usrl
contract SecurityAudit {
  section Metadata {
    fact Id = "security-audit";
    fact Purpose = "Review code and report security risks";
  }

  section Capabilities {
    require capability("read_file");
    require capability("run_tests");
  }

  stage Review {
    fact Findings = [];
  }
}
```

Contracts can optionally declare inheritance-like metadata:

```usrl
contract Pentest derives SecurityAudit {
  section Scope {
    fact ExternalNetworkAllowed = false;
  }
}
```

The parser records the contract body. It does not currently preserve the inherited contract name in the AST.

## Sections And Section Aliases

Generic sections:

```usrl
section Metadata {
  fact Name = "Agent Red";
}
```

The parser also accepts these section aliases as section blocks:

```text
intent
constraints
capabilities
stages
validation
audit
definitions
triggers
```

Example:

```usrl
contract Example {
  intent {
    fact Goal = "Keep destructive actions gated";
  }

  constraints {
    deny "external-network";
  }
}
```

## Statements

Statements are valid inside contracts, sections, stages, rules, triggers, and other statement blocks.

### `fact`

Defines or updates a named fact.

```usrl
fact RiskLevel = "high";
fact Tools = ["read_file", "list_files", "run_tests"];
fact HasTests = count(Tools) > 0;
```

Facts can include provenance metadata:

```usrl
fact Evidence = "cargo test passed" @ "terminal" ["2026-05-19T10:00:00Z"] with confidence 0.95;
```

At runtime, facts are stored by name. Later derivations of the same fact replace the previous value when the value changes.

### `let`

Introduces scoped bindings with patterns.

```usrl
let (name, severity) = getFinding();
let { id, role: agentRole } = CurrentAgent;
```

### `assert` And `require`

`assert` and `require` evaluate an expression and record an issue if it is false.

```usrl
assert count(Findings) >= 0;
require capability("read_file");
require not ExternalNetworkAllowed;
```

Runtime issue behavior:

- Failed `assert` emits `ASSERT_FAIL`.
- Failed `require` emits `SEMANTIC_ERROR`.

### `permit` And `deny`

Creates policy decisions. Conflicting permit and deny decisions for the same value are reported.

```usrl
permit "tool:read_file";
deny "tool:run_command:rm-rf";
```

### `emit`

Emits an event. If the emitted value is an array whose first item is a string, that string is used as the event name and the remaining items are payload.

```usrl
emit ["finding", "SQL injection risk", "high"];
emit alert("manual-review-required");
```

Events named `tool:<capability>` are checked against runtime capabilities.

```usrl
emit ["tool:run_tests", "cargo test"];
```

### `query`

Queries evaluate a target expression and record a result.

```usrl
query {
  target: count(Findings) == 0;
  response_format: BOOLEAN;
}
```

Supported query fields:

```text
target
where
constraints
response_format
hint
```

Example:

```usrl
query {
  target: Findings;
  where: AuditComplete;
  constraints: not ExternalNetworkAllowed;
  response_format: OBJECT_TABLE;
  hint: use_index;
}
```

Current response formats handled by the runtime:

```text
BOOLEAN       Converts target to true/false.
FACT          Returns the target value.
OBJECT_TABLE  Returns target if it is an array, otherwise wraps it in an array.
```

Unknown response formats currently return the target value.

### `if`

```usrl
if (RiskLevel == "high") {
  emit ["finding", "high risk"];
} else {
  emit ["finding", "not high risk"];
}
```

### `when`

`when` executes its body only when the condition is truthy.

```usrl
when count(Findings) > 0 {
  fact NeedsReport = true;
}

when (AuditComplete) {
  permit "complete";
}
```

### `foreach`

```usrl
foreach (finding in Findings) {
  emit ["finding", finding];
}

foreach ({ id, severity: sev } in Findings) {
  emit ["finding-severity", id, sev];
}
```

The iterable must evaluate to an array. Non-arrays behave like an empty array.

### `parallel` And `sequence`

These are structural blocks. The current runtime executes their contents as nested statement blocks.

```usrl
sequence {
  fact Step = "collect";
  fact Next = "analyze";
}

parallel {
  fact StaticAnalysis = true;
  fact DependencyAudit = true;
}
```

### `rule`

Rules are named blocks that the runtime executes repeatedly until no facts change or the iteration limit is reached.

```usrl
rule DeriveRisk(string target, severity: "medium") hint use_index; {
  when severity == "high" {
    fact NeedsHumanReview = true;
  }
}
```

Rule parameters may include:

```text
<name>
<type> <name>
<name>: <default-expression>
<type> <name>: <default-expression>
```

Hints are expressions introduced with `hint` and terminated by `;`.

### `constraint`, `stage`, And `trigger`

These are named blocks:

```usrl
constraint WorkspaceOnly {
  require not ExternalNetworkAllowed;
}

stage Review {
  require capability("read_file");
}

trigger OnAuditComplete {
  when changed("AuditComplete") {
    emit ["audit-complete"];
  }
}
```

Current runtime behavior:

- `constraint` and `stage` execute as nested statement blocks.
- `trigger` bodies are not executed inline; they are executed reactively when facts change.

### `return` And Expression Statements

```usrl
return;
return FindingSummary;
some_call("argument");
```

Expression statements are evaluated for side effects in the runtime. Most built-ins are pure.

## Types

### Structs

```usrl
struct AgentSpec {
  string AgentId;
  string Model;
  string[] Skills;
}

contract C {
  section S {
    fact Agent = new AgentSpec {
      AgentId = "agent-red",
      Model = "gpt-5.5",
      Skills = ["risk-check"]
    };
  }
}
```

Constructor field names are validated against the struct definition.

### Type Aliases

```usrl
type UserId = string;
type FindingList = Finding[];
type MapType = map<string, string>;
```

### Generic Types

```usrl
type Box<T> = T;
type Result<T> =
  | Success { value: T; }
  | Error { message: string; };
```

### Object Types

```usrl
type Finding = {
  id: string;
  severity: string;
  description: string;
};
```

### Union Variants

```usrl
type Decision =
  | Allow { target: string; }
  | Deny { target: string; reason: string; };
```

The validator checks duplicate type parameters, duplicate variants, unknown type references, generic arity, and constructor field compatibility.

Known built-in generic arities include:

```text
array<T>
list<T>
set<T>
option<T>
map<K, V>
record<K, V>
result<T, E>
```

## Expressions

### Literals

```usrl
"text"
"""multi
line"""
123
3.14
true
false
null
unknown
```

### Arrays, Sets, And Objects

```usrl
fact A = [1, 2, 3];
fact S = {1, 2, 3};
fact O = { id: "F1", severity: "high" };
```

The parser treats `{ key: value }` as an object and `{ value1, value2 }` as a set.

### Constructors

```usrl
fact Agent = new AgentSpec {
  AgentId = "agent-red",
  Model = "gpt-5.5"
};

fact Empty = new AgentSpec();
```

Object constructor fields may use `=` or `:`.

### Member And Index Access

```usrl
fact City = user.address.city;
fact MaybeCity = user?.address.city;
fact First = Findings[0];
fact Severity = Finding["severity"];
```

### Calls And Named Arguments

```usrl
fact Total = sum(Numbers);
emit process(id: id, role: role);
```

Named arguments are accepted by the parser. The current runtime evaluates call argument values positionally for built-ins.

### Operators

From highest to lower practical precedence:

```text
postfix:       call(), member ., safe member ?., index []
unary:         not, !, -, +
multiplicative *, /, %
additive:      +, -
range:         ..
comparison:    ==, !=, <, <=, >, >=, in, contains, matches
logical and:   and, &&
logical or:    or, ||
conditional:   condition ? consequent : alternate
coalesce:      ??
lambda:        left => right
```

Runtime notes:

- `+`, `-`, `*`, `/`, and `%` coerce values with `Number(...)`.
- Division by zero returns `0`.
- `matches` is parsed but not currently evaluated by the runtime.
- `..` creates an inclusive numeric range.
- `in` checks whether the right side array contains the left value.
- `contains` checks whether the left side array contains the right value.

### Match Expressions

```usrl
fact Access = match role {
  case admin => "all";
  case user when active(userId) => "limited";
};
```

### Comprehensions

```usrl
fact Evens = [x for x in 1..10 if x % 2 == 0];
fact Doubled = [x * 2 for x in Evens];
```

### Provenance

```usrl
fact Evidence = "reviewed" @ "manual" ["2026-05-19T12:00:00Z"] with confidence 0.9;
```

The runtime currently evaluates the left value and does not preserve provenance in the runtime result.

## Patterns

Patterns are used by `let`, `foreach`, and `match`.

Identifier pattern:

```usrl
let item = CurrentItem;
```

Tuple pattern:

```usrl
let (name, severity) = FindingPair;
```

Object pattern:

```usrl
let { id, severity: sev } = Finding;
```

Object pattern shorthand binds the field name as the variable name.

## Runtime Built-Ins

The current runtime implements these built-in functions:

```text
count(value)
sum(array)
min(array)
max(array)
concat(...values)
to_string(value)
distinct(array)
union(arrayA, arrayB)
intersect(arrayA, arrayB)
difference(arrayA, arrayB)
date(value)
year(date)
month(date)
day(date)
before(dateA, dateB)
after(dateA, dateB)
during(date, { start: ..., end: ... })
always(value)
eventually(value)
until(valueA, valueB)
since(valueA, valueB)
untrusted(value)
sanitize(value)
changed("FactName")
capability("name")
require_capability("name")
```

Taint behavior:

- `untrusted(value)` marks derived facts as tainted.
- `sanitize(value)` clears taint for the returned value.
- Tainted values used in `if`, `when`, `assert`, `require`, `emit`, `permit`, or `deny` create `INJECTION_PATTERN` issues.

Capability behavior:

- `capability("name")` returns whether the runtime was given the capability.
- `require_capability("name")` returns false and records an `EXECUTION_ERROR` if missing.
- Emitting an event whose name starts with `tool:` checks the corresponding capability.

## Minimal Useful Contract

```usrl
contract AgentRedDefault {
  section Metadata {
    fact Id = "agent-red-default";
    fact Purpose = "Security review with gated risky operations";
  }

  capabilities {
    require capability("read_file");
    require capability("list_files");
    require capability("run_tests");
  }

  constraints {
    fact ExternalNetworkAllowed = false;
    deny "external-network";
  }

  stage Review {
    fact Findings = [];
    query {
      target: count(Findings) == 0;
      response_format: BOOLEAN;
      hint: "true means no findings recorded";
    }
  }
}
```

Validate it:

```bash
usrl validate ./agent-red-default.usrl
```

Inspect resolution:

```bash
usrl resolve ./agent-red-default.usrl
```

Run the runtime:

```bash
usrl run ./agent-red-default.usrl
```

## Prompt Library Files `.pll`

`.pll` files describe prompt units that can be governed by `.cll` contract units.

Shape:

```text
version "2.0";
library PromptLibrary <Name> {
  metadata { ... }
  prompt <Name> { ... }
}
```

Required metadata fields:

```text
Id
Name
Version
Created
Modified
Author
Source
Purpose
```

Required prompt fields:

```text
Id
Title
Purpose
Type
Inputs
Outputs
Body
Links
```

Example:

```usrl
version "2.0";
library PromptLibrary SecurityPrompts {
  metadata {
    Id: "security-pack";
    Name: "Security Prompt Pack";
    Version: "1.0.0";
    Created: "2026-05-19T00:00:00Z";
    Modified: "2026-05-19T00:00:00Z";
    Author: "Honorbound Innovation, LLC";
    Source: "security-prompts.pll";
    Purpose: "Security agent prompts";
  }

  prompt ReviewRust {
    Id: "P.ReviewRust";
    Title: "Review Rust Code";
    Purpose: "Find security and correctness issues";
    Type: "Review";
    Inputs: [];
    Outputs: [];
    Body: "Review this Rust code for security risks.";
    Links: [];
  }
}
```

## Contract Library Files `.cll`

`.cll` files describe contract units that govern prompts.

Shape:

```text
version "2.0";
library ContractLibrary <Name> {
  metadata { ... }
  contract_unit <Name> { ... }
}
```

Required metadata fields are the same as `.pll`.

Required contract fields:

```text
Id
Title
Purpose
Scope
Rules
Targets
Validation
Links
```

`Rules` and `Validation` must be non-empty arrays.

Example:

```usrl
version "2.0";
library ContractLibrary SecurityContracts {
  metadata {
    Id: "security-pack";
    Name: "Security Contract Pack";
    Version: "1.0.0";
    Created: "2026-05-19T00:00:00Z";
    Modified: "2026-05-19T00:00:00Z";
    Author: "Honorbound Innovation, LLC";
    Source: "security-contracts.cll";
    Purpose: "Govern security prompts";
  }

  contract_unit ReviewRustContract {
    Id: "C.ReviewRust";
    Title: "Govern Rust review prompt";
    Purpose: "Require validation for the Rust review prompt";
    Scope: "Prompt";
    Rules: [
      { Id: "R1"; Category: "Required"; Condition: "true"; Action: "accept"; Severity: "Error"; Target: "P.ReviewRust"; }
    ];
    Targets: ["P.ReviewRust"];
    Validation: [
      { Type: "schema"; Blocking: true; }
    ];
    Links: [
      { Type: "Governs"; Target: "P.ReviewRust"; Blocking: true; }
    ];
  }
}
```

Pair validation checks:

- both files have valid required metadata
- prompt units have required prompt fields
- contract units have required contract fields
- blocking `Governs` or `Targets` links resolve to prompt IDs in the paired `.pll`

## JSRT Trace Files

USRL also includes JSRT trace validation and application:

```bash
usrl jsrt-validate ./trace.jsrt
usrl jsrt-apply ./trace.jsrt
```

JSRT is JSON-based and governed by:

```text
components/usrl/jsrt.schema.json
components/usrl/jsrt.profiles.json
components/usrl/jsrt.transitions.json
components/usrl/jsrt.errors.json
```

If `JSRT_HMAC_SECRET` is set, JSRT validation uses it for checksum/signature handling:

```bash
JSRT_HMAC_SECRET="$SECRET" usrl jsrt-validate ./trace.jsrt
```

## Authoring Workflow

1. Write a small contract with one purpose.
2. Validate syntax:

```bash
usrl validate ./contract.usrl
```

3. Resolve imports and references:

```bash
usrl resolve ./contract.usrl
```

4. Run the contract:

```bash
usrl run ./contract.usrl
```

5. Import it into CMS-v2 if it should be remembered by Vegvisir:

```bash
cms-v2 import-usrl --require-validation --ingest ./contract.usrl
```

6. Bind it to a Vegvisir agent:

```text
/agent bind-usrl agent-red security-audit
```

## Current Limitations

These are implementation facts, not design goals:

- `enum` bodies are accepted but skipped.
- Contract `extends` and `derives` names are parsed but not preserved in the current AST.
- `matches` is parsed as a comparison operator but is not implemented in runtime evaluation.
- Named call arguments are parsed, but runtime built-ins consume argument values positionally.
- Provenance syntax is parsed, but the current runtime returns the fact value rather than a provenance object.
- `parallel` is structural in the current runtime; it does not spawn concurrent execution.
