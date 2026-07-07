# MSP JSON Schema Set

This directory contains the canonical JSON Schema artifacts used by the MSP draft 0.1 local-first reference implementation. The Rust workspace embeds these schemas and uses them for artifact validation plus JSON-RPC/CLI contract tests.

## Schemas

- `manifest.schema.json` — canonical metadata for one portable, verifiable MSP skill.
- `dependency.schema.json` — dependency edge format for skills, packs, schemas, trust anchors, and runtime capabilities.
- `skill-pack.schema.json` — manifest for a versioned bundle of related skills.
- `verification-contract.schema.json` — contract describing how skill success is evaluated.
- `execution-report.schema.json` — runtime-produced evidence report after applying a skill; the Rust reference models artifacts, policy decisions, report errors, command timing, evidence, and check result statuses.
- `trust-policy.schema.json` — host or registry policy controlling whether artifacts may be trusted and loaded.
- `publication-draft.schema.json` — producer-neutral draft envelope used before canonical registry artifacts are written.
- `publication-report.schema.json` — report emitted by mutating publication commands after writing generated registry artifacts.
- `protocol-results.schema.json` — canonical result object contracts for v0.1 JSON-RPC methods, including search, pack discovery, load, compatibility, verification, trust, dependency, and pack-member validation results.

## Design Notes

These schemas intentionally keep MSP focused on skill lifecycle semantics:

1. discover skills;
2. retrieve manifests;
3. resolve dependencies;
4. verify trust and compatibility;
5. load skill bodies;
6. report execution evidence;
7. verify results.

They do not define generic tool execution, agent messaging, host memory policy, or Skiller internals.

## Protocol Result Schema Notes

`protocol-results.schema.json` defines both aggregate and per-result contracts. The embedded Rust validator exposes these definitions as separate `MspSchemaKind` variants so implementations can validate exact method outputs, not just source artifacts. The checked-in JSON-RPC success conformance fixtures in `examples/conformance/jsonrpc/v0.1/` use these result contracts for binary-level replay and schema validation; adjacent error fixtures lock JSON-RPC error envelope/code behavior. The checked-in CLI success/error fixtures in `examples/conformance/cli/v0.1/` use the same result contracts for command-level replay and schema validation, while also locking representative stderr/status failure behavior. The mutating `publish import-skiller` command is covered separately by a temp-registry conformance harness because it writes generated registry artifacts and reports run-specific paths; that harness validates both unsigned default imports and optional Ed25519-signed imports against `publication-report.schema.json`. Current result definitions cover:

- `RegistrySearchResult[]` for `registry.search` and `skills.discover`;
- `PackSearchResult[]` for `packs.discover`;
- `SkillLoadResult` for `skills.load`;
- `SkillCompatibilityResult` for `skills.check_compatibility`;
- `SkillVerificationResult` for `skills.verify_result`;
- `TrustVerifyResult` for `trust.verify` and `packs.verify_trust`;
- `DependencyResolutionResult` for `skills.resolve_dependencies`;
- `DependencyTrustEvaluationResult` for dependency trust evaluation methods;
- `PackMemberValidationResult` for `packs.validate_members`;
- `TrustPolicyEvaluation` for trust-policy evaluation methods.

## Validation Notes

The schemas use JSON Schema draft 2020-12.

Some semantic rules must be enforced outside plain JSON Schema validation, including:

- `primary_format` must appear in `formats`.
- Artifact hashes must match retrieved artifact bytes.
- Signatures must validate over artifact bytes; the Rust reference currently supports Ed25519 signature verification and issuer key binding via `trusted_issuers[].public_key_ref`.
- Dependency graphs must be cycle-free when required by policy.
- Verification result scoring, confidence, warning limits, failure taxonomy application, and per-check/evidence pass/fail semantics are enforced by the verifier, not by the report schema alone.
- Compatibility evaluation must compare host-declared runtime capabilities, model capabilities, tools, permissions, context window, platform, supported formats, and protocol/manifest versions against the manifest.
- Skills must not grant themselves permissions or override host/user/security policy.
- Registry-published versions must be immutable unless explicitly deprecated and republished under a new version.

## Status

Draft 0.1, but actively used by the Rust reference implementation. Artifact schemas validate registry fixtures and imported publication artifacts. Protocol result schemas validate JSON-RPC server outputs and CLI end-to-end outputs in contract tests. Publication report schemas validate mutating publication command outputs in temp-registry conformance tests. The schemas should remain backward-compatible within the v0.1 line unless a deliberate migration is documented.
