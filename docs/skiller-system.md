# Skiller System

Skiller is Vegvisir’s governed skill compiler and lifecycle toolchain. It turns technical sources into reviewable, source-grounded skill bundles that agents can route to, load, evaluate, refine, publish, and hand off to specialist-agent workflows.

Skiller lives in:

```text
components/skiller
```

It is a Rust crate in the root Cargo workspace and can be run directly or through Vegvisir:

```bash
cargo run -p skiller -- --help
vegvisir skiller -- --help
```

## What Skiller Is For

Skiller solves a specific problem: model sessions produce useful operational knowledge, but raw notes are not enough. Reusable agent knowledge needs structure, provenance, validation, lifecycle state, evals, and safe loading.

Skiller produces skill bundles that preserve:

- source inventories
- source hashes
- extracted sections
- citations
- generated candidate skills
- procedures
- guardrails
- tool requirements
- risk metadata
- eval scaffolding
- confidence/evidence records
- Forge request/response audit artifacts
- registry publication metadata
- corpus lifecycle reports
- Agent Builder handoff packages

## Current CLI Command Families

The current Rust CLI supports these command families.

### Compilation

```text
compile
compile-repo
compile-url
compile-openapi
compile-api
compile-cli
compile-cli-help
```

Use these to ingest local docs, repositories, public same-host docs crawls, API specs, CLI specs, or captured CLI help/manpage text.

### Runtime Use

```text
validate
list
route
load
eval
```

Use these to verify a bundle, inspect skills, match a query to relevant skills, materialize skill context, and run deterministic structural evals.

### Forge And Review

```text
forge-provider-status
forge-adapter-preflight
forge-adapter-self-test
forge
forge-request
forge-handoff
forge-validate
forge-apply
infer
critique
evidence-report
review-agent
apply-review
```

Forge is the controlled enhancement path. Skiller emits strict request envelopes and expects strict response envelopes. Responses are validated before application: citations must exist, references must resolve, confidence/evidence fields must be bounded, secret-like content is rejected, and external mutation policy must require approval.

### Agent Builder

```text
domain-profiles
propose-agents
verify-agent-proposals
build-agent-pack
verify-agent-pack
agent-builder-summary
agent-artifact-index
```

These commands turn skill bundles into specialist-agent proposals and handoff packages with manifests and verification reports.

### Registry And Lifecycle

```text
readiness
publish
registry-list
verify-manifest
registry-deprecate
registry-rollback
improve-from-telemetry
corpus-map
corpus-manifest
corpus-diff
corpus-plan
corpus-status
domain-template
bump-version
```

Publication is gated by readiness checks unless deliberately forced. Corpus lifecycle artifacts make changing source corpora reviewable instead of silently flowing changes into published skills.

## Bundle Model

A Skiller bundle is a directory of reviewable YAML artifacts. The exact artifact set depends on which commands have been run, but the model is built around:

- package metadata
- source records
- extracted source sections
- skills
- citations
- evals
- validation/eval/readiness reports
- Forge history
- lifecycle manifests/diffs/plans/status
- registry manifests
- agent proposals and packs

Skiller should not be treated as an opaque binary index. Bundles are meant to be inspected in reviews and by agents.

## Ingestion Behavior

Skiller ingestion is conservative by design.

Local/repo ingestion:

- reads source files and directories
- skips common build/cache/runtime directories
- records source hashes
- extracts sections and citations
- identifies warnings, commands, APIs, CLI operations, and procedural evidence
- redacts secret-like material
- marks repository-derived sections with appropriate trust/visibility metadata

URL ingestion:

- uses conservative same-host crawl limits
- stores excerpts rather than full private docs
- records source origin and hashes
- redacts secret-like material
- should not be used with secret-bearing URLs or private authenticated docs pasted into chat

API/CLI ingestion:

- compiles OpenAPI/API specs into operation-focused skills
- compiles CLI specs/help into command/flag/task skills
- includes anti-hallucination guidance for unsupported flags, undocumented endpoints, and version-sensitive behavior
- marks mutating operations with approval requirements where appropriate

## Forge Boundary

Skiller does not assume a model can mutate skill bundles freely. It uses a strict Forge boundary:

```text
ForgeRequestEnvelope  ──► model/Vegvisir reasoning pass ──► ForgeResponseEnvelope
```

The request envelope contains bundle metadata, selected source-section packets, candidate skills, citation IDs, graph context, pass instructions, output schema, token budget, and risk policy.

The response envelope may contain generated skills, modified skills, review findings, confidence updates, evidence records, human-review requirements, and audit notes.

Skiller then validates the response before applying it. Invalid responses fail closed.

## Vegvisir Integration

Skiller is integrated into Vegvisir in two ways:

1. **CLI passthrough** — `vegvisir skiller -- <args>` runs the Skiller CLI from the monorepo.
2. **Tool-level helpers** — Vegvisir exposes Skiller-related tool calls for compile, validate, route, load, eval, readiness, and Forge request/apply flows where configured.

Responsibility split:

- Skiller owns deterministic bundle generation, bundle validation, lifecycle artifacts, registry manifests, and strict Forge schemas.
- Vegvisir owns runtime execution, model/provider access, memory, tools, approvals, subagents, MCP, and transcript reporting.

## Skiller Versus LSL Skills

Skiller and LSL are related but not identical.

| System | Primary purpose |
| --- | --- |
| Skiller | Compile external technical sources into governed skill bundles and lifecycle artifacts. |
| LSL | Define workspace/data-root linked skill libraries with routeable subskills, dependencies, policies, evals, and runtime loading. |
| USRL | Express contract-bounded workflows with explicit permissions, denials, facts, stages, triggers, and validation semantics. |

A mature workflow can use all three: Skiller extracts and governs knowledge from source corpora, LSL expresses local reusable workflows, and USRL constrains high-risk or regulated execution.

## Common Workflows

### Compile docs into skills

```bash
vegvisir skiller -- compile ./docs \
  --out ./dist/docs-skills \
  --name docs-skills \
  --domain vegvisir-operations
```

### Compile a repository

```bash
vegvisir skiller -- compile-repo . \
  --out ./dist/repo-skills \
  --name vegvisir-repo-skills \
  --domain vegvisir
```

### Validate, route, and load

```bash
vegvisir skiller -- validate ./dist/repo-skills
vegvisir skiller -- route ./dist/repo-skills "how does HBSE provider auth work"
vegvisir skiller -- load ./dist/repo-skills <skill-id> --mode extended
```

### Forge handoff for a reasoning pass

```bash
vegvisir skiller -- forge-handoff ./dist/repo-skills \
  --out ./dist/forge-handoff \
  --pass skill-expansion \
  --domain-profile vegvisir-operations
```

### Validate and apply Forge output

```bash
vegvisir skiller -- forge-validate ./dist/repo-skills \
  --request ./dist/forge-handoff/forge-request.yaml \
  --response ./dist/vegvisir-response.yaml \
  --report ./dist/forge-validation-report.yaml

vegvisir skiller -- forge-apply ./dist/repo-skills \
  --request ./dist/forge-handoff/forge-request.yaml \
  --response ./dist/vegvisir-response.yaml \
  --out ./dist/repo-skills-forged \
  --report ./dist/forge-apply-report.yaml
```

### Registry publication

```bash
vegvisir skiller -- readiness ./dist/repo-skills-forged
vegvisir skiller -- publish ./dist/repo-skills-forged --registry ./dist/registry
vegvisir skiller -- registry-list ./dist/registry
```

## Quality And Safety Rules

- Do not include plaintext secrets in sources, skills, evals, provenance, telemetry, or Forge records.
- Cite sources for generated skills.
- Treat inferred skills as lower confidence until reviewed and evaluated.
- Require approval for skills that mutate external systems.
- Use readiness reports before registry publication.
- Use corpus lifecycle reports when source corpora change.
- Keep generated bundles out of source control unless intentionally publishing examples or fixtures.

## Tests And Checks

For Skiller changes:

```bash
cargo test -p skiller
cargo check -p skiller
```

For integration-level confidence:

```bash
cargo check --workspace
cargo test --workspace -- --test-threads=1
```

## Source References

- `components/skiller/src/lib.rs` — CLI commands and command dispatch
- `components/skiller/src/compiler/` — source ingestion and deterministic compilation
- `components/skiller/src/runtime/` — route/load runtime
- `components/skiller/src/forge/` — Forge request/response boundary
- `components/skiller/src/registry/` — bundle IO, validation, registry publication
- `components/skiller/src/corpus/` — corpus lifecycle artifacts
- `components/skiller/src/agents/` — agent proposal and pack generation
- `components/skiller/README.md` — command overview and examples
