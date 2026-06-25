# MSP v0.1 Registry Model

This document describes the local-first registry behavior implemented by the MSP v0.1 Rust reference implementation.

## Scope

The v0.1 registry is a deterministic local filesystem registry. It is intentionally not a remote marketplace, federation layer, or trust-anchor discovery service. Remote registries, HTTP transport, key transparency, and federation are future work.

The registry owns storage and lookup for:

- skill manifests;
- skill bodies;
- verification contracts;
- skill pack manifests;
- local dependency graph resolution;
- local trust/hash/signature verification inputs;
- structured skill and pack discovery.

## Storage Model

A registry root contains versioned skill and pack artifacts. Skill and pack manifests reference companion artifacts with registry-root-relative paths. The reference implementation indexes those manifests and uses the index as the authority for resolving IDs to manifest paths.

A conforming v0.1 local registry should preserve these invariants:

1. A skill ID resolves to exactly one indexed manifest path in the active local registry view.
2. A pack ID resolves to exactly one indexed pack manifest path in the active local registry view.
3. Manifest-declared artifact paths are interpreted relative to the registry root, never relative to the caller's process directory.
4. Loaded artifact bytes must match the manifest trust metadata when hashes or signatures are present.
5. Pack member declarations must match the referenced skill manifest ID and version.

## Path Safety

The reference registry rejects unsafe artifact paths before reading files. Implementations must reject:

- absolute paths;
- `file:` URIs;
- external URI schemes;
- path prefixes that escape the registry root;
- `..` traversal segments.

Pack member validation applies the same rule to `pack.skills[].manifest_uri`.

## Discovery and Indexing

The local registry supports structured search over skill and pack metadata. v0.1 discovery is deterministic over the indexed local corpus and supports filters such as task text, category, issuer, tools, and maximum risk where applicable.

`registry.search` and `skills.discover` are aliases in v0.1. `packs.discover` provides the pack equivalent.

## Immutability Expectations

Published same-ID/same-version artifacts are immutable by default. The local Skiller importer enforces this policy for generated MSP artifacts:

- existing publication directories are rejected unless `--force` is supplied;
- `--force` only permits byte-identical idempotent regeneration by default;
- byte-changing same-ID/same-version replacement requires `--force --allow-mutable-version`;
- conforming publishers should prefer a new version instead of mutable replacement.

This policy keeps local development practical while preserving a protocol-level bias toward reproducibility.

## Pack Member Validation

A pack member is valid only when:

1. its manifest URI is registry-root-relative and path-safe;
2. the referenced manifest exists;
3. the referenced manifest parses as a valid skill manifest;
4. the referenced skill ID and version match the pack declaration;
5. the registry index maps that skill ID to the same manifest path;
6. when a trust policy is supplied, the member passes that policy.

Duplicate pack member IDs are invalid.

## Dependency Evaluation

The v0.1 resolver evaluates local `skill` and `skill_pack` dependencies. It reports missing required dependencies, detects cycles, applies deterministic semantic-version strategy matching, and can evaluate dependency trust when policy requires it.

Supported local version strategies are:

- `exact`;
- `compatible`;
- `latest_patch`;
- `latest_minor`;
- `manual`, treated as exact by the deterministic local resolver.

Prerelease resolved versions are rejected unless the dependency edge allows prerelease versions.

## Future Registry Work

Future phases should add remote registry semantics only after the local registry contract remains stable under conformance tests. Open future areas include:

- HTTP transport;
- remote registry indexing;
- registry trust-anchor resolution;
- key transparency;
- registry federation;
- federated deprecation and update signaling;
- cross-registry conflict and provenance rules.
