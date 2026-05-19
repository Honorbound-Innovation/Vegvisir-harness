# USRL Usage

USRL is the contract layer used by Vegvisir for tightly bounded workflows. It gives agents a way to run inside explicit rules: allowed tools, stages, preconditions, required evidence, denied targets, and approval requirements.

The implementation is stored in:

```text
components/usrl
```

This page is based on the current USRL CLI implementation in `components/usrl/src/cli.ts`.

## Installed Command

```bash
usrl
```

The root installer builds the TypeScript package and installs a wrapper that runs the built CLI from:

```text
$prefix/share/vegvisir/usrl/dist/src/cli.js
```

## Build From Source

```bash
cd components/usrl
npm install
npm run build
npm test
```

Run the source CLI directly:

```bash
node dist/src/cli.js
```

## CLI Help Tree

```text
USRL CLI

Commands:
  usrl validate <file.usrl|file.pll|file.cll>
  usrl validate-pair <file.pll> <file.cll>
  usrl resolve <file.usrl>
  usrl run <file.usrl>
  usrl jsrt-validate <file.jsrt|file.json>
  usrl jsrt-apply <file.jsrt|file.json>
```

The current CLI prints this usage text when called with no command or an unknown command.

For syntax, contract structure, expressions, built-ins, `.pll/.cll` libraries, and complete authoring examples, see [USRL language reference](usrl-language-reference.md).

## Commands

### `validate`

Validates one `.usrl`, `.pll`, or `.cll` file.

```bash
usrl validate ./contracts/security-audit.usrl
usrl validate ./contracts/security-policy.pll
usrl validate ./contracts/security-contract.cll
```

Exit behavior:

- `0` when validation succeeds.
- `1` for CLI usage errors such as missing files or unsupported extensions.
- `2` when parsing or validation finds contract issues.

### `validate-pair`

Validates a paired policy library and contract library.

```bash
usrl validate-pair ./contracts/security-policy.pll ./contracts/security-contract.cll
```

The first path must be `.pll`; the second must be `.cll`.

### `resolve`

Resolves a `.usrl` project, including imports and symbol references, and prints JSON.

```bash
usrl resolve ./contracts/security-audit.usrl
```

The JSON includes:

- entry file
- module count
- imported modules
- validation issue count
- resolution issue count
- loader issue count
- symbol and reference counts
- resolution graph
- validation, loader, and resolution issues

Use this when a contract validates alone but imported symbols or module paths need debugging.

### `run`

Evaluates a `.usrl` program and prints runtime JSON.

```bash
usrl run ./contracts/security-audit.usrl
```

The JSON includes:

- runtime issue count
- iteration count
- facts
- tainted facts
- events
- queries
- derivations
- decisions
- loader, validation, resolution, and runtime issues

Use this when checking whether the contract reaches the expected policy decisions.

### `jsrt-validate`

Validates a JSRT trace document.

```bash
usrl jsrt-validate ./trace.jsrt
usrl jsrt-validate ./trace.json
```

If `JSRT_HMAC_SECRET` is set, the validator uses it while parsing frames:

```bash
JSRT_HMAC_SECRET="$SECRET" usrl jsrt-validate ./trace.jsrt
```

The output JSON includes frame count and issues.

### `jsrt-apply`

Validates and applies a JSRT trace document, then prints accepted/rejected frames and the resulting snapshot.

```bash
usrl jsrt-apply ./trace.jsrt
```

If `JSRT_HMAC_SECRET` is set, the applier uses it while parsing frames:

```bash
JSRT_HMAC_SECRET="$SECRET" usrl jsrt-apply ./trace.jsrt
```

## How Vegvisir Uses USRL

Vegvisir binds USRL contracts to custom agents and regulated skills. A contract should describe what the agent is allowed to do and what evidence must exist before it proceeds.

Agent commands:

```text
/agent bind-usrl <agent-id> <contract-id-or-skill-name>
/agent unbind-usrl <agent-id> <contract-id-or-skill-name>
/agent show <agent-id>
/agent use <agent-id>
```

Examples:

```text
/agent bind-usrl agent-red security-audit
/agent show agent-red
/agent use agent-red
```

CMS-v2 can import USRL contracts as structured memory:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" import-usrl \
  --user user:default \
  --project /path/to/project \
  --require-validation \
  --ingest \
  ./contracts/security-audit.usrl
```

## Contract Design Checklist

A useful USRL contract should define:

- workflow goal
- allowed operation types
- allowed tools
- denied tools or denied targets
- required stage order
- preconditions for risky stages
- required evidence before completion
- approval requirements
- expected outputs
- failure and stop conditions

For a security agent, separate these tiers:

- passive review
- local static analysis
- dependency audit
- exploit proof-of-concept generation
- destructive testing
- external network activity

Each tier should have different permissions, evidence requirements, and approval rules.

## Skills And Contracts

Vegvisir supports Markdown skills and USRL-backed skills.

Markdown skills are flexible instructions. USRL-backed skills are better when the workflow needs hard gates.

Use USRL when a workflow needs:

- regulated execution order
- explicit tool restrictions
- required evidence
- reproducible policy decisions
- clear denial behavior
- auditability

## Runtime Expectations

USRL narrows authority. Binding a contract to an agent does not mean every action is allowed. A risky action should satisfy the agent profile, USRL contract, Vegvisir runtime policy, and approval policy.

For high-risk tasks, the runtime should check:

- whether the active agent has the tool
- whether the USRL contract permits the operation
- whether the current stage allows it
- whether required preconditions and evidence exist
- whether human approval is required

## Installed Location

The root installer copies USRL into:

```text
$prefix/share/vegvisir/usrl
```

It writes an environment example containing:

```bash
export VEGVISIR_USRL_VALIDATOR_ROOT="$prefix/share/vegvisir/usrl"
```

Set that variable for services or shells that need to find the bundled validator explicitly.
