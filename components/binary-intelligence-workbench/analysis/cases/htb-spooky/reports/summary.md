# Binary Intelligence Summary — pass

## Case

- Case ID: `htb-spooky`
- Status: `triaged`
- Created: `2026-05-31T02:51:29Z`
- Source path: `/mnt/storage/Vegvisir-Projects/binary-intelligence-workbench/samples/htb-spooky/rev_spookypass/pass`
- Size: `15912` bytes
- SHA-256: `717409ebbd5e006a6ef01d6a05c6eb57275f5e48a3c4dbf4bc8b0454d4bf0b51`
- Format: `ELF`
- Architecture: `x86_64`

## Extraction Overview

- Strings: `79`
- Imports: `11`
- Exports: `19`
- Functions: `26`
- Extraction mode: `ghidra`

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
- `string` `Before we let you in, you'll need to give us the password: ` in `strings.json`


## Recommended Next Steps

1. Review artifacts under `artifacts/` and findings under `findings/`.
2. If Ghidra was unavailable, rerun with `GHIDRA_HEADLESS` configured for richer extraction.
3. Inspect functions referencing suspicious strings/imports.
4. Preserve only non-secret summaries in CMS memory.

## Safety Note

This report is static-analysis oriented. Do not execute unknown binaries on the host; use an explicit sandbox for dynamic analysis.
