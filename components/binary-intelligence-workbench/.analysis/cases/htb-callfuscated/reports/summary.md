# Binary Intelligence Summary — crackme

## Case

- Case ID: `htb-callfuscated`
- Status: `triaged`
- Created: `2026-05-31T02:16:55Z`
- Source path: `/mnt/storage/Vegvisir-Projects/binary-intelligence-workbench/samples/htb-callfuscated/crackme`
- Size: `59528` bytes
- SHA-256: `2389a35ac38bff9546a4c356c7d22633b22e8a1616b61adb85991afda40a3e36`
- Format: `ELF`
- Architecture: `x86_64`

## Extraction Overview

- Strings: `325`
- Imports: `7`
- Exports: `0`
- Functions: `0`
- Extraction mode: `basic-fallback`

## Heuristic Risk

- Level: `low`
- Score: `3`

### Rationale

- Suspicious strings: credential-keyword

## Findings

### Suspicious strings: credential-keyword

- Severity: `medium`
- Confidence: `medium`
- Category: `strings`
- Description: Found 1 string(s) matching credential-keyword patterns.

Evidence:
- `string` `To register enter your password: ` in `strings.json`


## Recommended Next Steps

1. Review artifacts under `artifacts/` and findings under `findings/`.
2. If Ghidra was unavailable, rerun with `GHIDRA_HEADLESS` configured for richer extraction.
3. Inspect functions referencing suspicious strings/imports.
4. Preserve only non-secret summaries in CMS memory.

## Safety Note

This report is static-analysis oriented. Do not execute unknown binaries on the host; use an explicit sandbox for dynamic analysis.
