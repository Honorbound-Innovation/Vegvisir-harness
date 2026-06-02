# Case File Schema Guide

BIW schemas are versioned with `biw.<name>.v1` strings in JSON payloads.

Core files:

```text
case.json                         biw.case.v1
artifacts/metadata.json           biw.metadata.v1
artifacts/hashes.json             biw.hashes.v1
artifacts/strings.json            biw.strings.v1
artifacts/imports.json            biw.imports.v1
artifacts/exports.json            biw.exports.v1
artifacts/functions.json          biw.functions.v1
artifacts/callgraph.json          biw.callgraph.v1
findings/findings.json            biw.findings.v1
artifacts/decompile/*.json        biw.decompile.v1
skills/*.json                     biw.skill-output.v1
artifacts/diff.json               biw.diff.v1
artifacts/firmware-inventory.json biw.firmware.v1
memory/cms-summary.json           biw.memory-summary.v1
jobs/agents/*.json                biw.agent-task.v1
```

The current implementation validates schemas through deterministic tests and stable generation paths. Future hardening should add JSON Schema files and migration utilities.
