# Skiller

Skiller is a Rust CLI for turning technical sources into governed, reusable, agent-ready skills.

The current root implementation is Rust. The earlier Python prototype is preserved under `legacy/python/` for reference while the Rust version reaches and exceeds feature parity.

## Current Rust capabilities

- Local deterministic source ingestion for Markdown, HTML, text, OpenAPI/API specs, CLI specs, CLI help-like files, and repository evidence.
- Explicit API/CLI compiler commands for OpenAPI, lightweight API specs, lightweight CLI specs, and captured CLI help/manpage text.
- Public URL ingestion with conservative same-host crawl limits via `compile-url`.
- Structure-preserving section extraction with source IDs, hashes, citation pointers, warning/command/API-operation detection, and secret-like redaction.
- Candidate skill generation with procedures, guardrails, citations, eval scaffolding, confidence metadata, evidence breakdowns, runtime policy, tool requirements, role suitability, and version applicability fields.
- Bundle writing and loading using reviewable YAML artifacts.
- Bundle validation, structural evals, readiness reports, and hash manifests.
- Runtime routing and skill materialization.
- Forge provider adapter layer with strict request/response envelopes, stored Forge audit artifacts, deterministic `mock` provider, and a `vegvisir` provider boundary for full Vegvisir integration.
- Mock/local Forge pass for schema-safe enhancement, inference records, evidence reports, critique reports, and inferred workflow candidates.
- Built-in domain profile listing.
- Agent profile proposal, proposal indexes, verified agent-pack handoff generation, pack manifests, build reports, and consolidated Agent Builder artifact indexes.
- Filesystem registry publication with readiness gates, provenance, manifest verification, deprecation records, rollback markers, and refreshed registry indexes.
- Static telemetry-based improvement proposals.
- Corpus lifecycle artifacts: manifest, diff, plan, status, and lifecycle-aware agent-pack handoff metadata.
- Behavioral eval coverage reports for skill bundles and agent-ready packs.

## Install / run

```bash
cargo build
cargo run -- compile examples/docs --out dist/example-skills --name example-skills --domain kubernetes
cargo run -- validate dist/example-skills
cargo run -- route dist/example-skills "pod crashloop logs"
cargo run -- load dist/example-skills <skill-id> --mode extended
cargo run -- forge dist/example-skills --out dist/example-skills-forged --domain-profile kubernetes-operations
cargo run -- evidence-report dist/example-skills-forged
cargo run -- propose-agents dist/example-skills-forged --out dist/agents
cargo run -- verify-agent-proposals dist/agents
cargo run -- build-agent-pack dist/example-skills-forged --agent "Cluster Diagnostic Agent" --out dist/cluster-agent --report dist/cluster-agent-build-report.yaml
cargo run -- verify-agent-pack dist/cluster-agent
cargo run -- agent-builder-summary --proposals dist/agents --pack dist/cluster-agent --out dist/agent-builder-summary.yaml
cargo run -- agent-artifact-index dist --out dist/agent-artifacts.yaml
```

## CLI overview

```text
skiller compile <input> --out <bundle> [--name <name>] [--domain <domain>]
skiller compile-url <url> --out <bundle> [--name <name>] [--domain <domain>] [--max-pages <n>]
skiller compile-repo <path> --out <bundle> [--name <name>] [--domain <domain>]
skiller compile-openapi <spec> --out <bundle> [--name <name>] [--domain <domain>]
skiller compile-api <spec> --out <bundle> [--name <name>] [--domain <domain>]
skiller compile-cli <spec> --out <bundle> [--name <name>] [--domain <domain>]
skiller compile-cli-help <help.txt> --out <bundle> [--name <name>] [--domain <domain>]
skiller validate <bundle>
skiller list <bundle>
skiller route <bundle> <query>
skiller load <bundle> <skill-id> --mode card|body|extended
skiller eval <bundle>
skiller forge <bundle> --out <bundle> [--provider mock|vegvisir] [--domain-profile <profile>]
skiller forge-request <bundle> --out <request.yaml> [--pass <pass>] [--domain-profile <profile>]
skiller forge-handoff <bundle> --out <dir> [--pass <pass>] [--domain-profile <profile>]
skiller forge-validate <bundle> --request <request.yaml> --response <response.yaml> [--report <report.yaml>]
skiller forge-apply <bundle> --request <request.yaml> --response <response.yaml> --out <bundle> [--report <report.yaml>]
skiller infer <bundle> --out <bundle>
skiller critique <bundle> --out <report.md>
skiller evidence-report <bundle>
skiller domain-profiles
skiller propose-agents <bundle> --out <dir>
skiller verify-agent-proposals <dir>
skiller build-agent-pack <bundle> --agent <name> --out <dir> [--lifecycle-status <status.yaml>] [--report <report.yaml>]
skiller verify-agent-pack <dir>
skiller agent-builder-summary [--proposals <dir>] [--pack <dir>]... --out <summary.yaml>
skiller agent-artifact-index <root> --out <index.yaml>
skiller readiness <bundle>
skiller publish <bundle> --registry <dir> [--force]
skiller registry-list <registry>
skiller verify-manifest <path>
skiller registry-deprecate <registry> <bundle-id> <version> --reason <reason> [--replacement-version <version>]
skiller registry-rollback <registry> <bundle-id> <to-version> --reason <reason>
skiller improve-from-telemetry <bundle> --out <dir>
skiller corpus-map <bundle> --out <dir>
skiller corpus-manifest <bundle> --out <dir>
skiller corpus-diff <old-manifest.yaml> <new-manifest.yaml> --out <dir>
skiller corpus-plan <corpus-diff.yaml> --out <dir>
skiller corpus-status <bundle> --plan <corpus-plan.yaml> --out <dir>
skiller domain-template <name>
skiller bump-version <bundle> --out <bundle> [--version <version>]
```


## API and CLI skill compilation

Skiller can build tool-use skills directly from API and CLI interface material:

```bash
cargo run -- compile-openapi examples/api/payments-openapi.yaml --out dist/payments-api-skills
cargo run -- compile-api path/to/api.yaml --out dist/api-skills
cargo run -- compile-cli path/to/cli.yaml --out dist/cli-skills
cargo run -- compile-cli-help path/to/help.txt --out dist/cli-help-skills
```

These commands force the source type instead of relying on filename detection. Generated skills include API/CLI role suitability, tool requirements, approval gates for mutating operations, no-secret guardrails, and anti-hallucination guidance for undocumented endpoints, flags, and version support.

## Repository ingestion

Skiller can compile a local repository into source-grounded skills by extracting operational evidence from documentation, API/CLI specs, configuration files, and code comments/signatures:

```bash
cargo run -- compile-repo . \
  --out dist/repo-skills \
  --name repo-skills \
  --domain skiller
```

Repository ingestion is read-only. It skips `.git`, Rust `target/`, Python bytecode, and preserved Python cache files. It stores excerpts only, redacts secret-like material, and treats repository-derived sections as private bundle sources by default.

## URL ingestion

Skiller can compile a public URL into the same governed bundle format used for local files:

```bash
cargo run -- compile-url https://example.com/docs \
  --out dist/url-skills \
  --name url-skills \
  --domain generic-technical-docs \
  --max-pages 3
```

The URL path is intentionally conservative: it uses excerpts-only retention, redacts secret-like material, records the source origin/hash, and only follows same-host links up to `--max-pages`. Private/authenticated docs should be integrated through Vegvisir/HBSE-backed connectors rather than pasted credentials or secret-bearing URLs.

## Vegvisir integration

Skiller is designed so Vegvisir provides the AI reasoning layer. The Rust CLI now has a provider-neutral Forge adapter boundary:

- `ForgeRequestEnvelope` contains bundle metadata, selected source-section packets, candidate skills, citation IDs, graph context, pass instruction, output schema, token budget, and risk policy.
- `ForgeResponseEnvelope` contains generated skills, modified skills, review findings, confidence updates, evidence records, human-review requirements, and audit notes.
- `--provider vegvisir` uses the Vegvisir adapter path and writes the same strict request/response artifacts as `--provider mock`. The current adapter is deterministic until Skiller is wired into Vegvisir as a first-class local tool/provider.
- Forge responses are validated before being applied: citation/section references must exist, new skills need inference records, secret-like material is rejected, and mutating external-system policy must require approval.

Example:

```bash
cargo run -- forge dist/example-skills \
  --out dist/example-skills-forged \
  --provider vegvisir \
  --domain-profile kubernetes-operations
```

The forged bundle stores `forge_requests.yaml`, `forge_responses.yaml`, `forge_summary.yaml`, and `forge_summary.md` so Vegvisir-powered changes remain reviewable and auditable. `skiller validate` revalidates the stored Forge history and summary artifacts.

For first-class Vegvisir integration, Skiller can export a complete handoff directory for Vegvisir:

```bash
cargo run -- forge-handoff dist/example-skills \
  --out dist/vegvisir-handoff \
  --pass skill-expansion \
  --domain-profile kubernetes-operations
```

This writes:

- `forge-request.yaml` — strict `ForgeRequestEnvelope` for Vegvisir
- `forge-response-template.yaml` — empty, schema-shaped response envelope
- `vegvisir-prompt.md` — grounded prompt/instructions for the Vegvisir reasoning pass

The explicit request/apply commands remain available for tool integration:

```bash
cargo run -- forge-request dist/example-skills \
  --out dist/vegvisir-request.yaml \
  --pass skill-expansion \
  --domain-profile kubernetes-operations

# Vegvisir reads the request and writes a ForgeResponseEnvelope.
cargo run -- forge-validate dist/example-skills \
  --request dist/vegvisir-request.yaml \
  --response dist/vegvisir-response.yaml \
  --report dist/vegvisir-validation-report.yaml

cargo run -- forge-apply dist/example-skills \
  --request dist/vegvisir-request.yaml \
  --response dist/vegvisir-response.yaml \
  --out dist/example-skills-forged \
  --report dist/vegvisir-apply-report.yaml
```

`forge-validate` and `forge-apply` validate the response before mutating any bundle: request/pass IDs must match, citations and source sections must exist, new skills require inference records, secret-like material is rejected, confidence/evidence scores must be bounded, and external mutation policies must require approval. Optional report outputs are machine-readable YAML for GUI/desktop workflows.


## Corpus lifecycle workflow

Skiller can emit deterministic corpus lifecycle artifacts for large or evolving documentation sets:

```bash
skiller corpus-manifest dist/example-skills --out dist/lifecycle/v1
skiller corpus-manifest dist/example-skills-v2 --out dist/lifecycle/v2
skiller corpus-diff dist/lifecycle/v1/corpus-manifest.yaml dist/lifecycle/v2/corpus-manifest.yaml --out dist/lifecycle/diff
skiller corpus-plan dist/lifecycle/diff/corpus-diff.yaml --out dist/lifecycle/plan
skiller corpus-status dist/example-skills-v2 --plan dist/lifecycle/plan/corpus-plan.yaml --out dist/lifecycle/status
```

The manifest records source, section, and skill set hashes. The diff reports added, removed, and changed sources. The plan turns changes into deterministic review actions. The status command combines validation, readiness, and lifecycle action state so changed corpora do not silently flow into publication or Agent Builder defaults.

`build-agent-pack` can include lifecycle state:

```bash
skiller build-agent-pack dist/example-skills-v2 \
  --agent "Cluster Diagnostic Agent" \
  --out dist/cluster-agent \
  --lifecycle-status dist/lifecycle/status/corpus-status.yaml
```

## Registry lifecycle

Publication is gated by readiness checks unless `--force` is used deliberately:

```bash
cargo run -- readiness dist/example-skills-reviewed
cargo run -- publish dist/example-skills-reviewed --registry dist/local-registry
cargo run -- registry-list dist/local-registry
cargo run -- verify-manifest dist/local-registry/<bundle-id>/<version>
```

Registry entries can now be deprecated without deleting them:

```bash
cargo run -- registry-deprecate dist/local-registry <bundle-id> 0.1.0 \
  --reason "superseded by reviewed 0.1.1" \
  --replacement-version 0.1.1
```

Rollback records mark a previous version as the active rollback target for consumers that honor registry metadata:

```bash
cargo run -- registry-rollback dist/local-registry <bundle-id> 0.1.0 \
  --reason "0.1.1 failed verifier review"
```

`registry-list` refreshes `index.yaml` and reports manifest validity, deprecation state, deprecation reason, and active rollback version metadata.

## Bundle layout

```text
package.yaml
skills/<skill-id>.yaml
sources/index.yaml
sources/sections.yaml
candidates.yaml
graph/concepts.yaml
graph/dependencies.yaml
audit/events.yaml
forge_requests.yaml
forge_responses.yaml
MANIFEST.sha256
README.md
```

## Verifier review and Agent Builder handoff

The Rust implementation includes a deterministic verifier-agent style review command. It is not a substitute for Vegvisir reasoning or human approval, but it gives the Forge workflow a governed review artifact before publication:

```bash
skiller review-agent dist/example-skills-forged --out reports/review --agent verifier
```

The command writes:

- `verifier-review.yaml` — structured per-skill decisions, blockers, warnings, and required changes
- `verifier-review.md` — human-readable review report

Verifier decisions can be applied to a new staged bundle without mutating the original:

```bash
skiller apply-review dist/example-skills-forged \
  --review reports/review/verifier-review.yaml \
  --out dist/example-skills-reviewed
```

`apply-review` records an audit event, updates per-skill review metadata, promotes approved skills to `Reviewed` / Level 3 verified, keeps changed skills in `NeedsReview`, and marks unsafe skills as `Unsafe` with approval/rollback gates.

Agent packs include selected, required, optional, and forbidden skill groups, exported eval cases, tool permissions, runtime policy, context policy, memory policy, and approval policy for Vegvisir Agent Builder ingestion.

Generated proposal directories include:

- individual `<agent>.yaml` proposal files
- `agent-proposals-index.yaml`
- `agent-proposals-index.md`

Verify them with:

```bash
skiller verify-agent-proposals dist/agents
```

Generated agent-pack directories include:

- `agent-pack.yaml`
- `agent-pack-manifest.yaml`
- `agent-pack-manifest.md`

Build reports and verification are available for GUI/desktop workflows:

```bash
skiller build-agent-pack dist/example-skills-forged \
  --agent "Cluster Diagnostic Agent" \
  --out dist/cluster-agent \
  --report dist/cluster-agent-build-report.yaml

skiller verify-agent-pack dist/cluster-agent
```

To consolidate Agent Builder artifacts:

```bash
skiller agent-builder-summary \
  --proposals dist/agents \
  --pack dist/cluster-agent \
  --out dist/agent-builder-summary.yaml

skiller agent-artifact-index dist --out dist/agent-artifacts.yaml
```

These commands emit YAML and Markdown summaries that expose selection rationale, readiness, lifecycle state, eval status, tool permissions, verification errors, and generated file references.

## Language pivot note

The Python implementation was an initial prototype and is intentionally preserved in `legacy/python/`. New development should target the Rust implementation at the repository root.


## Forge provider status and live adapter hook

Provider status can be inspected explicitly:

```bash
skiller forge-provider-status
skiller forge-provider-status --provider vegvisir
```

The current `vegvisir` Forge provider is a structured-envelope adapter. When no live adapter is configured, it uses deterministic strict-envelope fallback behavior.

To configure a Vegvisir-managed live adapter:

```bash
export SKILLER_VEGVISIR_FORGE_ADAPTER=/path/to/vegvisir-forge-adapter
skiller forge-adapter-preflight
skiller forge-adapter-self-test
skiller forge <bundle> --out <forged> --provider vegvisir
```

`forge-adapter-preflight` checks that the configured adapter path exists and is executable. `forge-adapter-self-test` sends a tiny synthetic `ForgeRequestEnvelope` to the adapter and validates that the returned `ForgeResponseEnvelope` obeys the strict schema before any real corpus is forged.

Use `forge-handoff`, `forge-validate --report`, and `forge-apply --report` for external or GUI-mediated reasoning workflows.


### Vegvisir Forge adapter bounds

When using the live adapter hook, set `SKILLER_VEGVISIR_FORGE_ADAPTER` to a Vegvisir-managed executable that reads a `ForgeRequestEnvelope` YAML from stdin and writes a `ForgeResponseEnvelope` YAML to stdout. Adapter execution is bounded by `SKILLER_VEGVISIR_FORGE_ADAPTER_TIMEOUT_SECS` (default `120`, maximum `900`). Keep provider credentials behind Vegvisir/HBSE; do not place secrets in adapter arguments, Forge requests, or logs.
