# MSP v0.1 Compatibility Rules

This document summarizes the compatibility checks implemented by the local-first MSP v0.1 reference implementation.

## Goal

Compatibility checking answers whether a host runtime can safely and correctly consume a skill before applying it. It is separate from trust verification and execution-result verification.

## Compatibility Inputs

`skills.check_compatibility` evaluates a skill against a runtime query containing:

- MSP protocol version;
- supported manifest versions;
- runtime name and version;
- supported skill body formats;
- runtime capabilities;
- model capabilities;
- available tools;
- tool versions;
- granted permissions;
- context window size;
- platform;
- known runtime metadata.

## Hard Failures

The v0.1 implementation treats these mismatches as hard compatibility failures:

- unsupported MSP protocol version bounds;
- unsupported manifest version;
- unsupported primary skill body format;
- missing required runtime capability;
- missing required model capability;
- missing required tool;
- insufficient required tool version;
- missing required permission;
- insufficient context window;
- unsupported required platform.

## Advisory Warnings

Known runtime metadata is advisory in v0.1. A runtime identity/version mismatch can produce warnings without necessarily making the skill incompatible.

## Result Shape

The compatibility result includes:

- overall `compatible` boolean;
- normalized score;
- per-dimension booleans;
- structured issues;
- warnings.

The result is intended to be machine-readable enough for clients to gate loading or present actionable diagnostics.

## Version Compatibility

MSP keeps several versions distinct:

- protocol version;
- manifest schema version;
- skill version;
- skill pack version;
- verification contract version;
- dependency requirement version.

Implementations should not infer compatibility from naming conventions. Version fields must be parsed and evaluated explicitly.

## Dependency Version Strategies

Local `skill` and `skill_pack` dependency requirements use semantic version matching with these strategies:

- `exact` requires equality;
- `compatible` accepts versions greater than or equal to the requirement within the compatible line;
- `latest_patch` requires the same major/minor and at least the required patch;
- `latest_minor` requires the same major and at least the required minor/patch tuple;
- `manual` is treated as exact by the deterministic local resolver.

Build metadata is ignored. Prerelease resolved versions are rejected unless the dependency edge sets `allow_prerelease`.

## Compatibility vs Trust vs Verification

Compatibility, trust, and verification are separate checks:

- compatibility asks whether the runtime can consume the skill;
- trust asks whether the runtime should load the skill under policy;
- verification asks whether skill use satisfied the execution contract.

A skill can be compatible but untrusted, trusted but incompatible, or both trusted and compatible but still fail execution verification.

## Future Work

Future phases should expand compatibility rules for:

- model-aware skill compilation;
- context-window-aware compression;
- toolset-specific skill variants;
- remote registry compatibility profiles;
- conformance profile negotiation.
