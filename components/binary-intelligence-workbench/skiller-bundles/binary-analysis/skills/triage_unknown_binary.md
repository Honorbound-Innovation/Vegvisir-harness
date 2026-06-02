# Skill: binary.triage.unknown

## Purpose
Analyze a BIW case for an unknown binary using normalized artifacts and produce an evidence-backed triage summary.

## Inputs
- `case.json`
- `artifacts/metadata.json`
- `artifacts/strings.json`
- `artifacts/imports.json`
- `artifacts/exports.json`
- `artifacts/functions.json`
- `findings/findings.json`

## Procedure
1. Identify binary format, architecture, size, and hash.
2. Review extraction mode and note any fallback/partial-analysis limitations.
3. Summarize obvious capabilities from imports and strings.
4. Rank findings by severity and confidence.
5. Recommend functions, imports, or strings for deeper inspection.
6. Avoid claims not backed by artifacts.

## Output Contract
Return Markdown with:
- Executive summary
- Evidence table
- Capability assessment
- Risk level with rationale
- Recommended next commands

## Safety Boundary
Static analysis only. Do not instruct execution of unknown binaries outside an explicit sandbox.
