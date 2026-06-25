# MSP v0.1 Skiller Publication Profile

This document describes the Skiller-to-MSP publication boundary implemented in Phase 5A.

## Scope

Skiller is a producer of skills. MSP is the protocol and registry contract those skills are published into. The v0.1 implementation keeps that boundary explicit: the importer consumes Skiller bundle artifacts and emits canonical MSP artifacts, then validates those artifacts through the same schema and registry paths used by normal MSP consumers.

## Input Boundary

`msp publish import-skiller <bundle> --issuer <issuer>` expects a Skiller bundle boundary containing:

- `package.yaml`;
- `skills/*.yaml`;
- `sources/index.yaml`;
- `graph/dependencies.yaml`.

The importer treats those files as producer input, not as MSP-native artifacts.

## Generated MSP Artifacts

For each imported Skiller skill, the publisher writes:

- one MSP `skill.manifest.json`;
- one Markdown skill body;
- one MSP verification contract.

For the bundle, the publisher writes:

- one MSP `pack.manifest.json`;
- pack membership declarations for generated skills;
- pack dependency metadata derived from the Skiller dependency graph where applicable.

The generated artifacts are intended to be loadable by the local MSP registry immediately after publication.

## Signing

Generated artifacts are unsigned by default. When `--signing-key <path>` is supplied, the importer signs generated skill body bytes and pack proof bytes with Ed25519.

The signing key file accepts either:

- a raw 32-byte Ed25519 seed;
- hex text;
- hex text prefixed with `ed25519-seed:`.

Real signing keys should remain outside the repository and be supplied through the operator environment's secret boundary.

## Publication Reports

The CLI returns a JSON publication report for import operations. Reports validate against `schemas/publication-report.schema.json` and include generated artifact information plus signing metadata when signing is enabled.

Unsigned reports must not contain public-key references. Signed reports include the relevant public-key reference and public-key SHA-256 digest.

## Release Governance

The v0.1 local publisher enforces immutable-by-default same-ID/same-version publication:

- duplicate publication without `--force` is rejected;
- `--force` allows idempotent byte-identical regeneration;
- byte-changing replacement requires `--force --allow-mutable-version`;
- the recommended path for real changes is a new skill or pack version.

The importer can mark generated artifacts deprecated with:

- `--deprecated`;
- `--deprecation-reason`;
- `--replacement-skill`;
- `--replacement-pack`;
- `--sunset-at`.

Deprecation metadata is embedded into generated skill and pack manifests. Pack trust proof bytes cover the deprecation metadata.

## Validation Path

The conformance harness exercises publication into temporary registries and verifies:

- unsigned publication report shape;
- signed publication report shape;
- generated artifact schema validity;
- generated registry loadability;
- duplicate rejection;
- idempotent `--force`;
- immutable-version rejection;
- explicit mutable override behavior;
- generated deprecation metadata;
- signed-policy evaluation.

## Known v0.1 Gaps

The Skiller publication profile is intentionally local-first. Future work remains for:

- Skiller-side multi-version planning;
- richer curation and deduplication;
- generated skill quality scoring;
- remote/federated release governance;
- update signaling across registries;
- model/context/toolset-aware skill compilation.
