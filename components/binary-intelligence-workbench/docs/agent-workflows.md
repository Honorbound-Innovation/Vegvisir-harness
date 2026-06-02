# Agent Workflows

BIW defines lightweight agent profiles and task artifacts for Vegvisir orchestration.

## Commands

```bash
python3 -m biw.cli agent list
python3 -m biw.cli agent plan <case-id> binary-triage-agent "Review this case" --out .analysis/cases
```

## Profiles

- `binary-triage-agent`
- `function-analysis-agent`
- `vuln-hunting-agent`
- `firmware-analysis-agent`
- `report-writer-agent`

## Output

Agent task artifacts are written under:

```text
jobs/agents/<task-id>.json
```

They include goal, bounded inputs, expected outputs, case summary, and artifact references. The CLI does not launch hidden background work; Vegvisir can consume these artifacts to spawn controlled subagents.
