# MSP — Model Skill Protocol

MSP is a protocol for portable, verifiable AI skills. This repository contains the draft specification, JSON Schemas, and the Rust local-first reference implementation.

Current state: **v0.1 reference implementation through Phase 5A**. The core protocol, local registry, trust/dependency/verification/compatibility layers, JSON-RPC stdio server, CLI surface, and local Skiller publication importer are implemented and covered by workspace tests plus JSON-RPC/CLI contract tests. Phase 6 model-aware compilation and Phase 7 federation remain future work.

## Current v0.1 Reference Implementation

The Rust workspace currently provides:

- `msp-core` — protocol data types, embedded JSON Schema validation, manifests, hashes, verification contracts, trust policies, execution reports.
- `msp-registry` — local filesystem registry indexing, discovery, skill loading, pack loading, dependency resolution, pack dependency evaluation, trust/body hash checks, pack trust checks, trust-policy evaluation, execution-report verification.
- `msp-cli` — command-line interface for local registry and publication operations.
- `msp-server` — JSON-RPC 2.0 stdio server for core MSP methods.
- `msp-publisher` — producer-side publication helpers, including Skiller bundle import into canonical MSP registry artifacts.

## Quick Start

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Index the reference registry:

```bash
cargo run -p msp-cli -- registry index
```

Discover a Rust refactoring skill:

```bash
cargo run -p msp-cli -- registry search \
  --task "refactor a rust module" \
  --tool read_file \
  --tool write_file
```

Load a skill and verify its body hash:

```bash
cargo run -p msp-cli -- skills load skill.rust.refactor.module.v1
```

Evaluate a skill against the local reference trust policy:

```bash
cargo run -p msp-cli -- trust evaluate skill.rust.refactor.module.v1 \
  --policy examples/policies/local-reference.trust-policy.json
```

Evaluate a skill and its dependency graph against the same policy:

```bash
cargo run -p msp-cli -- trust evaluate-dependencies skill.rust.refactor.module.v1 \
  --policy examples/policies/local-reference.trust-policy.json
```

Verify a sample execution report:

```bash
cargo run -p msp-cli -- skills verify-result examples/reports/rust-refactor.report.json
```

The verification result includes the legacy summary fields (`passed`, `score`, `failed_checks`, `warnings`) plus structured per-check results, evidence results, criteria evaluation, confidence, and failure records. The verifier now enforces contract/report skill linkage, optional contract id linkage, skill version linkage, report status, report errors, required evidence, check evidence keys, minimum score, minimum confidence, and warning limits.

Check runtime compatibility for a skill:

```bash
cargo run -p msp-cli -- skills check-compatibility skill.rust.refactor.module.v1 \
  --msp-version 0.1.0 \
  --manifest-version 0.1.0 \
  --runtime-name msp-reference \
  --format markdown \
  --runtime-capability workspace_read \
  --runtime-capability workspace_write \
  --model-capability code_generation \
  --model-capability tool_use \
  --tool read_file \
  --tool write_file \
  --permission workspace_read \
  --permission workspace_write \
  --context-window 128000
```

Compatibility evaluation checks protocol/manifest versions, supported formats, runtime capabilities, model capabilities, required tools and tool minimum versions, permissions, context window, platform, and known runtime metadata.

Call the JSON-RPC stdio server:

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"msp.info","params":{}}' \
  | cargo run -p msp-server -- --registry examples/registry
```

Evaluate trust over JSON-RPC with an inline policy:

```bash
jq -nc \
  --slurpfile policy examples/policies/local-reference.trust-policy.json \
  '{jsonrpc:"2.0",id:1,method:"trust.evaluate",params:{id:"skill.rust.refactor.module.v1",policy:$policy[0]}}' \
  | cargo run -p msp-server -- --registry examples/registry
```


Import a Skiller bundle into an MSP local registry:

```bash
cargo run -p msp-cli -- --registry dist/msp-registry \
  publish import-skiller /path/to/skiller-bundle \
  --issuer local-dev
```

Optionally sign the generated skill bodies and pack manifest proof bytes with a local Ed25519 seed file:

```bash
cargo run -p msp-cli -- --registry dist/msp-registry \
  publish import-skiller /path/to/skiller-bundle \
  --issuer local-dev \
  --signing-key /path/to/ed25519.seed
```

The signing key file accepts either a raw 32-byte Ed25519 seed or hex text, optionally prefixed with `ed25519-seed:`. Keep real signing keys outside the repository and provide them through the local secret boundary used by your operator environment.

`publish import-skiller` reads the Skiller bundle artifact boundary (`package.yaml`, `skills/*.yaml`, `sources/index.yaml`, and `graph/dependencies.yaml`) and writes one MSP `skill.manifest.json` plus Markdown body and verification contract per Skiller skill, and one MSP `pack.manifest.json` for the Skiller bundle. This keeps Skiller as a producer while MSP owns validation, hashing, manifest shape, pack membership, dependency edges, registry loadability, and optional Ed25519 signing. Generated artifacts are unsigned by default and signed when `--signing-key` is provided. Published ids are immutable by default: `--force` only permits idempotent regeneration when generated bytes are identical; byte-changing replacement of an existing same-id/version publication requires the explicit local override `--force --allow-mutable-version`, or preferably a new version id. The importer can also mark generated artifacts deprecated with `--deprecated`, `--deprecation-reason`, `--replacement-skill`, `--replacement-pack`, and `--sunset-at`. The CLI publication path is covered by a temp-registry conformance harness using `examples/skiller-bundles/sample-rust-bundle/`.

Discover skill packs:

```bash
cargo run -p msp-cli -- packs discover \
  --task "rust engineering" \
  --category software_engineering/rust
```

`packs discover` returns structured pack search results including pack id, version, category, risk level, issuer, score, and member counts. The same search path backs the JSON-RPC `packs.discover` method.

Verify and evaluate a pack:

```bash
cargo run -p msp-cli -- packs verify-trust pack.rust.engineering.v1
cargo run -p msp-cli -- packs validate-members pack.rust.engineering.v1
cargo run -p msp-cli -- packs evaluate-trust pack.rust.engineering.v1 \
  --policy examples/policies/local-reference.trust-policy.json
cargo run -p msp-cli -- packs evaluate-dependencies pack.rust.engineering.v1 \
  --policy examples/policies/local-reference.trust-policy.json
```

`packs validate-members --policy <policy>` additionally evaluates each member skill against the policy; with the local reference policy, the Rust refactor skill is structurally valid but returns `valid: false` because that policy requires review for workspace-writing skills.


### Dependency Version Strategies

For local `skill` and `skill_pack` dependencies, `requirement` is parsed as a semantic version and compared with the resolved manifest version according to `resolution.strategy`:

- `exact` — resolved version must equal the requirement.
- `compatible` — default; resolved version must be at least the requirement and within the same compatible line (`^`-style: same major for `1.x`, same minor for `0.x`, exact patch for `0.0.x`).
- `latest_patch` — resolved version must be at least the requirement with the same major and minor.
- `latest_minor` — resolved version must be at least the requirement with the same major.
- `manual` — treated as exact in the local deterministic resolver; hosts may use it as a policy signal for human-curated resolution.

Build metadata is ignored for comparison. Prerelease resolved versions are rejected unless `resolution.allow_prerelease` is true; a prerelease at the same base version still sorts before the stable requirement.

## v0.1 Release Handoff Docs

- `docs/registry.md` — local registry storage rules, path safety, immutability expectations, pack member validation, and dependency evaluation.
- `docs/skiller-publication.md` — Skiller bundle import boundary, generated MSP artifacts, signing, publication reports, and release governance.
- `docs/threat-model.md` — v0.1 security boundary, primary threats, mitigations, and host runtime obligations.
- `docs/compatibility.md` — runtime compatibility inputs, hard failures, advisory warnings, version rules, and distinction from trust/verification.

## Protocol and Reference Artifacts

- `spec.md` — formal specification outline plus implemented v0.1 method contracts.
- `plan.md` — implementation strategy, milestone status, and next work.
- `schemas/` — draft JSON Schema set for artifacts, publication reports, and protocol result objects.
- `examples/registry/` — reference local skill registry fixture.
- `examples/policies/` — trust-policy fixture.
- `examples/reports/` — execution-report fixture.
- `examples/conformance/jsonrpc/v0.1/` — checked-in JSON-RPC success and error request/response conformance fixtures for every implemented method and representative failure paths.
- `examples/conformance/cli/v0.1/` — checked-in CLI success/error command/stdout/stderr/status conformance fixtures for the stable local command surface and representative failure paths.
- `examples/skiller-bundles/` — deterministic Skiller bundle fixtures used by publisher/import conformance tests.
- `crates/msp-server/src/main.rs` tests — JSON-RPC dispatch parity and schema-contract tests.
- `crates/msp-server/tests/jsonrpc_conformance.rs` — binary-level replay test for checked-in JSON-RPC success/error conformance fixtures.
- `crates/msp-cli/tests/cli_conformance.rs` — binary-level replay test for checked-in CLI conformance fixtures.
- `crates/msp-cli/tests/publish_import_conformance.rs` — temp-registry CLI conformance test for `publish import-skiller`, unsigned and signed publication-report schema validation, generated artifact validation, duplicate rejection, idempotent `--force`, immutable-version rejection, explicit `--allow-mutable-version` override, and generated deprecation metadata.
- `crates/msp-cli/tests/cli_contracts.rs` — CLI end-to-end contract tests against the reference registry.

## Supported v0.1 Method Surface

The reference server exposes the initial JSON-RPC method names:

- `msp.info`
- `registry.search`
- `skills.discover`
- `skills.get_manifest`
- `skills.load`
- `skills.resolve_dependencies`
- `skills.verify_result`
- `skills.check_compatibility`
- `packs.discover`
- `packs.get_manifest`
- `packs.load`
- `packs.verify_trust`
- `packs.evaluate_trust`
- `packs.validate_members`
- `packs.evaluate_dependencies`
- `trust.verify`
- `trust.evaluate`
- `trust.evaluate_dependencies`

## Validation and Security Model Notes

The Rust reference implementation embeds the MSP JSON Schemas and validates raw JSON before deserializing core artifacts. Runtime schema validation is currently enforced for:

- skill manifests
- skill pack manifests
- trust policies
- verification contracts
- execution reports
- dependency schemas used through manifest/pack `$ref`s
- publication drafts
- protocol result objects used by JSON-RPC and CLI contract tests

The local registry intentionally only loads relative artifact paths inside the registry root. Absolute paths, `file://` URIs, external URI schemes, and `..` path escapes are rejected before file reads. Pack member validation treats `pack.skills[].manifest_uri` as registry-root-relative, verifies that each referenced skill manifest exists, matches the declared skill id/version, is the same manifest indexed for that skill id, and can optionally pass trust-policy evaluation. Dependency evaluation validates skill and pack dependency graphs, resolves required `skill` and `skill_pack` dependencies from the local registry, checks deterministic semver requirements using each edge's `resolution.strategy`, detects cycles, applies transitive-dependency policy, evaluates dependency trust when required by policy, and enforces dependency issuer/hash/signature constraints. The v0.1 resolver supports `exact`, `compatible`, `latest_patch`, `latest_minor`, and `manual`; prerelease resolved versions are rejected unless `allow_prerelease` is true.

The v0.1 trust policy evaluator is deterministic and local. It supports issuer checks, issuer public-key binding, risk thresholds, signature-required gates, forbidden-behavior warnings, and simple structured rules over skill/pack id, issuer, category, risk, signing state, and skill permissions. Dependency trust evaluation also checks dependency graph resolution, cycles, transitive-dependency policy, dependency policy trust gates, and per-edge trust constraints such as required issuer, required hash, and required signing state. Signed skills use detached Ed25519 signatures encoded as `ed25519:<base64-public-key>:<base64-signature>`, and trust verification checks both the artifact hash and the signature over the body artifact bytes. Pack trust verification checks the pack manifest over canonical JSON bytes with only `trust.hash`, `trust.signed`, and `trust.signature` cleared, avoiding self-referential hashes while covering pack metadata, skill membership, dependencies, provenance, deprecation, and extensions. Trusted issuer key bindings use `trusted_issuers[].public_key_ref` with either `ed25519:<base64-public-key>` or `sha256:<hex-public-key-digest>`.

## Status

MSP is currently a **local-first v0.1 reference implementation through Phase 5A**:

- complete through the core local skill lifecycle: discovery, manifest retrieval, loading, dependency resolution, trust verification/evaluation, compatibility checking, execution-report verification, and result schemas;
- includes local pack discovery/loading/trust/member/dependency evaluation;
- includes JSON-RPC 2.0 stdio, checked-in JSON-RPC success/error conformance fixtures, checked-in CLI success/error conformance fixtures, binary replay tests, temp-registry `publish import-skiller` conformance, and CLI contract coverage for the implemented method families;
- includes a first Skiller bundle publication/import path that emits canonical MSP skill, body, verification-contract, and pack artifacts.

Still future: remote registries, federation, key transparency, registry trust-anchor resolution, HTTP transport, model-aware skill compilation, richer Skiller curation/deduplication, multi-version authoring workflows, and federated release/update signaling.
