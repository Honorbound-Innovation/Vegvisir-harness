# MSP Specification Outline

Status: Draft outline 0.1 with local-first Rust reference implementation through Phase 5A
Audience: MSP implementers, skill publishers, registry operators, Skiller integrators, and AI runtime authors

## Reference Implementation Snapshot

The Rust reference implementation currently covers the local-first MSP lifecycle for v0.1:

- artifact schemas for skill manifests, skill packs, dependencies, trust policies, verification contracts, execution reports, publication drafts, publication reports, and protocol result objects;
- local registry indexing, structured skill/pack discovery, manifest retrieval, skill loading, body hash verification, pack loading, pack trust verification, pack member validation, and path-safety checks;
- deterministic dependency resolution for skill and pack graphs, including cycle/missing-dependency reporting and semantic version strategy matching;
- trust policy evaluation for skills, packs, and dependency graphs, including issuer/risk/signing/review/dependency constraints and Ed25519 body-signature verification;
- runtime compatibility evaluation for MSP/manifest versions, supported formats, runtime/model capabilities, tools, tool versions, permissions, context window, platform, and known runtime metadata;
- execution-report verification against verification contracts, including required evidence, check outcomes, score/confidence/warning thresholds, and failure records;
- JSON-RPC 2.0 stdio method dispatch for the implemented method surface;
- CLI coverage for the same local registry, trust, dependency, compatibility, verification, pack, and publication-import operations;
- Phase 5A Skiller publication import from Skiller bundle artifacts into canonical MSP registry artifacts.

Conformance coverage currently includes workspace tests, JSON-RPC method parity/schema-contract tests, CLI end-to-end schema-contract tests against `examples/registry`, and temp-registry publication-report schema validation for Skiller imports. Future work remains for remote/HTTP registries, federation, key transparency, registry trust-anchor resolution, richer Skiller curation/release governance, and model-aware skill compilation.

## 1. Abstract

1.1 Purpose of MSP
1.2 One-sentence definition
1.3 Motivation and ecosystem gap
1.4 Relationship to tools, agents, prompts, and workflows
1.5 Relationship to Skiller and Vegvisir
1.6 Summary of the MSP lifecycle

## 2. Status of This Document

2.1 Draft status
2.2 Normative vs non-normative language
2.3 Intended conformance targets
2.4 Version of this specification
2.5 Change-control expectations

## 3. Scope

3.1 In-scope responsibilities
3.2 Out-of-scope responsibilities
3.3 Non-goals
3.4 Design constraints
3.5 Target audiences and implementers
3.6 Relationship to adjacent protocols

## 4. Terminology

4.1 Skill
4.2 Skill Pack
4.3 Skill Manifest
4.4 Skill Body
4.5 Skill Document
4.6 Skill Dependency
4.7 Skill Activation Rule
4.8 Skill Verification Contract
4.9 Skill Execution Report
4.10 Skill Trust Policy
4.11 Registry
4.12 Host Runtime
4.13 Consumer Runtime
4.14 Publisher
4.15 Skiller
4.16 MSP Client
4.17 MSP Server
4.18 Trust Anchor
4.19 Compatibility Result

## 5. Core Principles

5.1 Narrow protocol definition
5.2 Skills are not tools
5.3 Skills are not agents
5.4 Skills are not merely prompts
5.5 Structured and verifiable skills
5.6 Trust-first delivery
5.7 Verification-first execution
5.8 Explicit versioning
5.9 Implementation independence
5.10 Separation of generation, distribution, and execution
5.11 Host policy supremacy
5.12 No silent permission expansion

## 6. System Model

6.1 High-level architecture
6.2 MSP client/server relationship
6.3 Registry role
6.4 Publisher role
6.5 Skiller role
6.6 Runtime role
6.7 Trust boundary model
6.8 Host policy boundary
6.9 Data flow overview
6.10 Local-first registry model
6.11 Future federation model

## 7. Protocol Lifecycle

7.1 Server discovery and capability inspection
7.2 Registry search
7.3 Skill discovery
7.4 Skill selection
7.5 Manifest retrieval
7.6 Dependency resolution
7.7 Trust verification
7.8 Compatibility checking
7.9 Skill loading
7.10 Host-side skill application
7.11 Execution reporting
7.12 Verification and scoring
7.13 Failure reporting and remediation

## 8. Protocol Transport

8.1 Canonical protocol envelope
8.2 JSON-RPC 2.0 framing
8.3 Stdio transport requirements
8.4 HTTP transport requirements
8.5 Optional future WebSocket transport
8.6 Request IDs and correlation
8.7 Batching policy
8.8 Streaming policy
8.9 Transport security considerations
8.10 Message size and pagination guidance

## 9. Identifiers and Versioning

9.1 MSP protocol version
9.2 Manifest schema version
9.3 Skill version
9.4 Skill pack version
9.5 Verification contract version
9.6 Registry identifier
9.7 Schema URI conventions
9.8 Skill URI conventions
9.9 Content-addressed artifact identifiers
9.10 Stability and compatibility rules
9.11 Deprecation and replacement semantics

## 10. Data Model

10.1 `MspServer`
10.2 `MspRegistry`
10.3 `SkillManifest`
10.4 `SkillPackManifest`
10.5 `SkillDocument`
10.6 `SkillDependency`
10.7 `SkillActivationRule`
10.8 `SkillInputSchema`
10.9 `SkillOutputSchema`
10.10 `SkillVerificationContract`
10.11 `SkillExecutionReport`
10.12 `SkillProvenance`
10.13 `SkillTrustPolicy`
10.14 `SkillCompatibilityResult`
10.15 `SkillLoadResult`
10.16 `SkillVerificationResult`
10.17 `SkillFailureReport`
10.18 `RegistrySearchResult`

## 11. Manifest Specification

11.1 Manifest purpose
11.2 Required fields
11.3 Optional fields
11.4 Identity and naming
11.5 Category and classification
11.6 Capability declarations
11.7 Activation metadata
11.8 Runtime requirements metadata
11.9 Tool requirement declarations
11.10 Permission declarations
11.11 Schema references
11.12 Verification metadata
11.13 Trust metadata
11.14 Dependency metadata
11.15 Deprecation metadata
11.16 Artifact linkage
11.17 Example manifest structure
11.18 Validation rules

## 12. Skill Body Specification

12.1 Skill body purpose
12.2 Supported formats
12.3 Canonical body representation
12.4 LSL as first-class format
12.5 Markdown support
12.6 JSON support
12.7 YAML support
12.8 Body integrity requirements
12.9 Body-to-manifest linkage
12.10 Skill body safety requirements
12.11 Host-policy override prohibition
12.12 Example skill body outline

## 13. Skill Pack Specification

13.1 Skill pack purpose
13.2 Pack identity and versioning
13.3 Pack membership rules
13.4 Pack-level dependencies
13.5 Pack-level trust metadata
13.6 Pack-level verification expectations
13.7 Pack manifest validation rules
13.8 Example skill pack manifest

Pack trust verification hashes and signs canonical pack manifest trust bytes. The canonical projection is the serialized pack manifest after clearing only `trust.hash`, `trust.signed`, and `trust.signature`, so the proof covers pack identity, membership, dependencies, issuer/risk/review metadata, provenance, deprecation, and extensions without creating a self-referential hash.

Publisher-side release governance for local Skiller imports is intentionally conservative. Existing same-id publication directories are rejected unless `--force` is supplied. With `--force`, the publisher compares every generated artifact to the existing bytes and only allows idempotent regeneration by default. A byte-changing same-id/version replacement is treated as a mutable-version override and requires the explicit local escape hatch `--force --allow-mutable-version`; conforming publishers should prefer issuing a new version id instead. `--deprecated` publication flags embed `deprecation` metadata in both generated skill and pack manifests, including optional replacement ids, reason, and sunset timestamp. Pack trust proof bytes include this deprecation metadata.

Pack member validation treats each `pack.skills[].manifest_uri` as registry-root-relative. Implementations MUST reject absolute paths, external URI schemes, `file:` URIs, path prefixes, and `..` escapes. A member is valid only when the referenced manifest exists, parses as a skill manifest, its `id` and `version` match the pack declaration, and the registry index maps that skill id to the same manifest path. When a trust policy is supplied, each member skill is also evaluated against that policy and a disallowed member makes the pack-member validation result invalid.

Pack dependency evaluation treats `pack.dependencies[]` as the pack composition dependency graph. Implementations SHOULD resolve required `skill` dependencies from indexed skills and required `skill_pack` dependencies from indexed packs, report missing required dependencies, detect cycles across skill and pack dependency graphs, apply the dependency policy's transitive-dependency and cycle gates, and evaluate dependency trust when `require_dependency_trust_verification` is enabled. Dependency edge constraints include required issuer, required hash, required signing state, and deterministic version requirement matching for locally resolved skill and pack dependencies.

## 14. Discovery

14.1 Discovery goals
14.2 Search dimensions
14.3 Query inputs
14.4 Runtime capability inputs
14.5 Tool availability inputs
14.6 Trust-policy inputs
14.7 Result ranking
14.8 Result fields
14.9 Deterministic vs heuristic matching
14.10 Pagination
14.11 Discovery failure modes
14.12 Security considerations for discovery

## 15. Loading

15.1 Load request semantics
15.2 Load response semantics
15.3 Format selection
15.4 Integrity checks
15.5 Signature checks
15.6 Provenance checks
15.7 Compatibility checks
15.8 Dependency checks
15.9 No silent mutation rule
15.10 Load failure modes
15.11 Host obligations after load

## 16. Dependency Resolution

16.1 Dependency types
16.2 Dependency declaration format
16.3 Resolution algorithm
16.4 Cycle detection
16.5 Version conflict detection
16.6 Trust dependency validation
16.7 Optional vs required dependencies
16.8 Resolution graph representation
16.9 Missing dependency handling
16.10 Dependency failure modes


Version requirement matching in the v0.1 reference resolver parses `requirement` as semantic version `MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]` for local `skill` and `skill_pack` dependencies. `resolution.strategy` controls matching: `exact` requires equality; `compatible` is the default and accepts versions greater than or equal to the requirement within the compatible line (same major for nonzero major versions, same minor for `0.x`, exact patch for `0.0.x`); `latest_patch` requires same major/minor and at least the required patch; `latest_minor` requires same major and at least the required minor/patch tuple; `manual` is treated as exact by the deterministic local resolver. Build metadata is ignored for comparison. Prerelease resolved versions are rejected unless `allow_prerelease` is true, and a prerelease at the same base version sorts before the stable requirement.

## 17. Compatibility

17.1 Compatibility dimensions
17.2 Protocol version checks
17.3 Manifest version checks
17.4 Runtime capability checks
17.5 Tool availability checks
17.6 Permission availability checks
17.7 Context window checks
17.8 Format compatibility checks
17.9 Trust policy compatibility
17.10 Compatibility result semantics
17.11 Partial compatibility and degradation

The v0.1 reference implementation exposes `skills.check_compatibility`. The request carries a `RuntimeCompatibilityQuery` containing the host MSP version, supported manifest versions, runtime identity/version, supported skill formats, runtime capabilities, model capabilities, available tools, tool versions, granted permissions, context window, and platform. The response is `SkillCompatibilityResult`, including an overall `compatible` boolean, a normalized score, per-dimension booleans, structured issues, and warnings. Required runtime/model capabilities, tools, permissions, context window, format support, and version bounds are hard compatibility failures. Known runtime metadata is advisory and produces warnings rather than hard failure.

## 18. Verification

18.1 Verification contract purpose
18.2 Required verification fields
18.3 Optional verification fields
18.4 Execution report structure
18.5 Evidence model
18.6 Required checks
18.7 Optional checks
18.8 Pass/fail semantics
18.9 Scoring and confidence semantics
18.10 Failure reason taxonomy
18.11 Verification edge cases
18.12 Non-deterministic verification handling
18.13 Verification-result retention guidance

The v0.1 reference verifier validates execution reports against the schema and then evaluates the linked verification contract. The verifier enforces skill id, optional skill version, optional verification contract id, execution status, report errors, required evidence, per-check evidence keys, required check statuses, minimum score, minimum confidence, and allowed warning limits. `SkillVerificationResult` preserves summary fields and also returns structured per-check results, evidence results, criteria evaluation, confidence, and failure records. Contract `failure_taxonomy` entries may supply severity metadata for known failure codes.

## 19. Trust and Security

19.1 Threat model
19.2 Trust anchors
19.3 Content hashes
19.4 Signatures
19.5 Provenance requirements
19.6 Registry trust policy
19.7 Host trust policy
19.8 Risk levels
19.9 Forbidden behaviors
19.10 Policy override prohibition
19.11 Prompt-injection-aware handling
19.12 Dependency trust
19.13 High-risk skill review gates
19.14 Supply-chain security
19.15 Security failure modes
19.16 Privacy and telemetry constraints

## 20. Registry Semantics

20.1 Registry responsibilities
20.2 Immutable published artifacts
20.3 Search and indexing
20.4 Version resolution
20.5 Trust metadata storage
20.6 Deprecation handling
20.7 Publication flow
20.8 Local-first registry model
20.9 Registry export/import
20.10 Federation prerequisites
20.11 Registry auditability requirements

## 21. Skiller Integration

21.1 Skiller as generator/curator
21.2 Skill inference pipeline
21.3 Skill compilation pipeline
21.4 Model-aware compilation
21.5 Context-window-aware compilation
21.6 Toolset-aware compilation
21.7 Deduplication and classification
21.8 Verification contract generation
21.9 Publication pipeline
21.10 Quality scoring
21.11 What Skiller must not do
21.12 Non-Skiller publisher requirements

## 22. Protocol Methods

22.1 Common request envelope
22.2 Common response envelope
22.3 Common error envelope
22.4 `msp.info`
22.5 `registry.search`
22.6 `skills.discover`
22.7 `skills.get_manifest`
22.8 `skills.load`
22.9 `skills.resolve_dependencies`
22.10 `skills.verify_result`
22.10.1 `skills.check_compatibility`
22.11 `packs.discover`
22.12 `packs.get_manifest`
22.13 `packs.load`
22.14 `packs.verify_trust`
22.15 `packs.evaluate_trust`
22.16 `packs.validate_members`
22.17 `packs.evaluate_dependencies`
22.18 `trust.verify`
22.19 `trust.evaluate`
22.20 `trust.evaluate_dependencies`
22.21 Future methods: ranking, composition, diffing, update checks, signing, attestation, and model-specific compilation



### 22.x Implemented v0.1 JSON-RPC Method Contracts

The reference v0.1 server exposes the following JSON-RPC 2.0 method surface. Requests use a JSON object `params` unless otherwise noted. Responses return the listed result object directly in `result`; errors use the JSON-RPC error envelope.

| Method | Required params | Result contract | Notes |
| --- | --- | --- | --- |
| `msp.info` | none | `MspInfo` | Server/reference metadata and advertised method names. |
| `registry.search` | `SkillSearchQuery` | `RegistrySearchResult[]` | Structured local skill search. |
| `skills.discover` | `SkillSearchQuery` | `RegistrySearchResult[]` | Alias of `registry.search` in v0.1. |
| `skills.get_manifest` | `id` or `skill_id` | `SkillManifest` | Returns only the manifest. |
| `skills.load` | `id` or `skill_id` | `SkillLoadResult` | Returns manifest, body, body hash status, optional verification contract, and dependency ids. |
| `skills.resolve_dependencies` | `id` or `skill_id` | `DependencyResolutionResult` | Resolves local skill and pack dependencies deterministically. |
| `skills.verify_result` | `ExecutionReport` or `{ execution_report }` | `SkillVerificationResult` | Verifies runtime evidence against the linked contract. |
| `skills.check_compatibility` | `id` plus `runtime` or query fields | `SkillCompatibilityResult` | Evaluates runtime/model/tool/permission/version compatibility. |
| `packs.discover` | `PackSearchQuery` | `PackSearchResult[]` | Structured pack discovery by task/category/issuer/risk. |
| `packs.get_manifest` | `id` or `pack_id` | `SkillPackManifest` | Returns pack manifest. |
| `packs.load` | `id` or `pack_id` | `SkillPackManifest` | v0.1 pack load is manifest-only. |
| `packs.verify_trust` | `id` or `pack_id` | `TrustVerifyResult` | Verifies canonical pack manifest trust bytes. |
| `packs.evaluate_trust` | `id` or `pack_id`, `policy` | `TrustPolicyEvaluation` | Evaluates pack against inline trust policy. |
| `packs.validate_members` | `id` or `pack_id`, optional `policy` | `PackMemberValidationResult` | Validates member URI/id/version/index integrity and optional member trust. |
| `packs.evaluate_dependencies` | `id` or `pack_id`, `policy` | `DependencyTrustEvaluationResult` | Evaluates pack dependency graph against trust policy. |
| `trust.verify` | `id` or `skill_id` | `TrustVerifyResult` | Verifies skill body hash/signature. |
| `trust.evaluate` | `id` or `skill_id`, `policy` | `TrustPolicyEvaluation` | Evaluates skill against inline trust policy. |
| `trust.evaluate_dependencies` | `id` or `skill_id`, `policy` | `DependencyTrustEvaluationResult` | Evaluates skill dependency graph against trust policy. |

The canonical JSON Schema for the result objects above is `schemas/protocol-results.schema.json`. Artifact schemas remain separate: `manifest.schema.json`, `skill-pack.schema.json`, `verification-contract.schema.json`, `execution-report.schema.json`, `trust-policy.schema.json`, `dependency.schema.json`, `publication-draft.schema.json`, and `publication-report.schema.json`.

## 23. Error Model

23.1 Error categories
23.2 Error code namespace
23.3 Validation errors
23.4 Trust errors
23.5 Compatibility errors
23.6 Dependency errors
23.7 Verification errors
23.8 Registry errors
23.9 Transport errors
23.10 Recovery guidance
23.11 Human-readable diagnostics
23.12 Machine-readable remediation metadata

## 24. Conformance

The v0.1 reference conformance baseline is executable, not only descriptive:

- artifact fixtures in `examples/registry`, `examples/policies`, and `examples/reports` validate against embedded schemas;
- server contract tests call every method advertised by `msp.info`, validate representative results against `schemas/protocol-results.schema.json`, and lock alias behavior such as `registry.search`/`skills.discover` and `packs.get_manifest`/`packs.load`;
- checked-in JSON-RPC conformance fixtures in `examples/conformance/jsonrpc/v0.1/` provide one success request/response pair for every implemented method plus representative error request/response pairs;
- checked-in CLI conformance fixtures in `examples/conformance/cli/v0.1/` provide success/error command/stdout/stderr/status fixtures for the stable local command surface and representative command failures;
- `crates/msp-server/tests/jsonrpc_conformance.rs` replays JSON-RPC fixtures through the compiled `msp-server` binary, validates schema/error contracts, and fails on byte-for-byte response drift;
- `crates/msp-cli/tests/cli_conformance.rs` replays CLI fixtures through the compiled `msp-cli` binary, validates schema contracts, and fails on stdout/stderr/status drift;
- `crates/msp-cli/tests/publish_import_conformance.rs` invokes `publish import-skiller` against a unique temporary registry, validates unsigned and signed publication report shape, generated registry loadability, duplicate rejection, idempotent `--force`, immutable-version rejection, explicit `--allow-mutable-version` override, generated deprecation metadata, and signed-policy evaluation;
- CLI contract tests invoke the compiled `msp-cli` binary, parse stdout JSON, validate major command outputs against canonical schemas, and lock representative error behavior;
- workspace tests exercise registry indexing/loading, path-safety rejection, trust verification/evaluation, dependency resolution, compatibility checks, verification scoring, and Skiller publication import roundtrips.

24.1 Conforming MSP server requirements
24.2 Conforming MSP client requirements
24.3 Conforming registry requirements
24.4 Conforming skill publisher requirements
24.5 Conforming Skiller publisher profile
24.6 Required tests
24.7 Optional features
24.8 Versioned conformance profiles
24.9 Interoperability test suite
24.10 Security conformance requirements

## 25. Examples

25.1 Minimal discovery example
25.2 Minimal manifest example
25.3 Minimal load example
25.4 Dependency resolution example
25.5 Trust verification example
25.6 Execution report example
25.7 Verification example
25.8 Skiller publication example
25.9 End-to-end workflow example
25.10 Failure example

## 26. Implementation Guidance

Implementers should treat `schemas/protocol-results.schema.json` as the contract for machine-readable method outputs and should add contract tests for every exposed transport or command surface. The reference server and CLI both validate outputs against the same embedded schema definitions to prevent drift between Rust structs, JSON-RPC responses, CLI stdout, and the formal schema set.

26.1 Reference architecture
26.2 Recommended module boundaries
26.3 Storage model guidance
26.4 Schema validation guidance
26.5 Testing guidance
26.6 Logging and observability
26.7 Backward compatibility guidance
26.8 Migration guidance
26.9 Local development registry guidance
26.10 Production deployment guidance

## 27. Governance and Evolution

27.1 Specification ownership
27.2 Change process
27.3 Deprecation process
27.4 Extension mechanism
27.5 Extension registry
27.6 Registry federation roadmap
27.7 Future protocol phases
27.8 Compatibility promise

## 28. Appendices

28.1 JSON schema index
28.2 `manifest.schema.json`
28.3 `dependency.schema.json`
28.4 `skill-pack.schema.json`
28.5 `verification-contract.schema.json`
28.6 `execution-report.schema.json`
28.7 `trust-policy.schema.json`
28.8 `publication-draft.schema.json`
28.9 `publication-report.schema.json`
28.10 `protocol-results.schema.json`
28.11 Example manifests
28.12 Example verification contracts
28.13 Example execution reports
28.14 Threat model notes
28.15 Glossary
28.16 Open questions
