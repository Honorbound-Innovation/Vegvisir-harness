# Skill: report.professional_writeup_generate

## Purpose
Convert analysis artifacts, command logs, evidence, and solve notes into a polished, reproducible report suitable for CTF writeups, incident-response summaries, reverse-engineering reports, crypto attack explanations, or technical appendices.

## Use When
- A case has a verified result or enough findings to communicate.
- The user wants Markdown or standalone HTML documentation.
- A report must separate executive summary, technical details, evidence, and remediation.

## Inputs
- `case_dir`
- `challenge_or_incident_metadata`
- `analysis_notes`
- `commands_run`
- `evidence_artifacts`
- `desired_format`: markdown, html, both
- optional `redaction_policy`

## Procedure
1. Establish report audience and format.
2. Build an evidence index from case artifacts and notes.
3. Write a concise executive summary.
4. Describe environment, files, hashes, and tooling.
5. Present the investigation chronologically or by technical layer.
6. Include commands, relevant outputs, and explanations.
7. Explain failed paths and pivots when instructive.
8. Provide verification steps and result.
9. For incidents, include IOCs, remediation, and impact notes.
10. Apply redaction policy for credentials, private data, flags, keys, tokens, and sensitive infrastructure.
11. Generate self-contained dark-mode HTML when requested.

## Output Contract
- `report.md`
- optional `report.html`
- optional `index.html`
- `evidence_index.json`
- `redaction_notes.md`

## Quality Bar
- Evidence-backed claims only.
- Reproducible commands where possible.
- Clear separation between observed fact, inference, and recommendation.
- Professional formatting and readable code/output blocks.

## Safety Boundary
Do not publish credentials, tokens, private keys, sensitive customer data, or unauthorized exploit instructions. For CTF reports, do not store flags in long-term memory unless explicitly allowed by project policy.
