# Skill: binary.generate_re_report

## Purpose
Generate a concise reverse-engineering report from a BIW case directory.

## Inputs
- `case.json`
- `artifacts/*.json`
- `findings/*.json`
- `reports/functions/*.md` optional
- `notes/notes.md` optional

## Procedure
1. Summarize case identity and artifact coverage.
2. Describe major capabilities with artifact references.
3. Include high-signal strings/imports/functions only.
4. Incorporate reviewed function reports when present.
5. Call out limitations and safe next steps.

## Output Contract
Return Markdown suitable for `reports/full-report.md`.

## Safety Boundary
Reports may include extracted strings. Redact secret-like values before sharing externally or storing in durable memory.
