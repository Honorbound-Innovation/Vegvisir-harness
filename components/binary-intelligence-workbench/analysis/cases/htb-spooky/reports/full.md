# Binary Intelligence Full Report — pass

This full report aggregates the BIW summary, skill outputs, function explanations, and analyst notes for one case.

---

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

---

# Skill Outputs

## binary.triage.unknown

- Title: `Skill: binary.triage.unknown`
- Created: `2026-05-31T02:51:57Z`
- Source: `skills/binary.triage.unknown.json`

Case htb-spooky is triaged as low risk with 1 heuristic findings.

- Risk level: `low`
- Risk score: `3`

Artifact counts:

- callgraph_edges: `19`
- exports: `19`
- functions: `26`
- imports: `11`
- strings: `79`

Recommended next steps:

- Review high/medium findings and validate evidence manually.
- Run `biw explain function` on functions near suspicious imports or strings.
- Generate `biw report full` after function-level notes are added.

---

# Function Reports

No function reports have been generated yet. Run `biw explain function <case-id> <function>`.

---

# Analyst Notes

## Notes — htb-spooky

---

# Handling Guidance

- Treat extracted strings, paths, and symbols as potentially sensitive case artifacts.
- Redact environment-specific or credential-like strings before sharing.
- This report is static-analysis evidence, not a guarantee of benign or malicious behavior.
