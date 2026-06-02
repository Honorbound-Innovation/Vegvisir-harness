# Binary Intelligence Full Report — crackme

This full report aggregates the BIW summary, skill outputs, function explanations, and analyst notes for one case.

---

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

---

# Skill Outputs

## binary.triage.unknown

- Title: `Skill: binary.triage.unknown`
- Created: `2026-05-31T02:16:57Z`
- Source: `skills/binary.triage.unknown.json`

Case htb-callfuscated is triaged as low risk with 1 heuristic findings.

- Risk level: `low`
- Risk score: `3`

Artifact counts:

- callgraph_edges: `0`
- exports: `0`
- functions: `0`
- imports: `7`
- strings: `325`

Recommended next steps:

- Review high/medium findings and validate evidence manually.
- Run `biw explain function` on functions near suspicious imports or strings.
- Generate `biw report full` after function-level notes are added.

---

# Function Reports

No function reports have been generated yet. Run `biw explain function <case-id> <function>`.

---

# Analyst Notes

## Notes — htb-callfuscated

---

# Handling Guidance

- Treat extracted strings, paths, and symbols as potentially sensitive case artifacts.
- Redact environment-specific or credential-like strings before sharing.
- This report is static-analysis evidence, not a guarantee of benign or malicious behavior.
