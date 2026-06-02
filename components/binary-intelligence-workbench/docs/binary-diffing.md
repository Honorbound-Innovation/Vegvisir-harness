# Binary Diffing

## Command

```bash
python3 -m biw.cli diff ./old.bin ./new.bin --out .analysis/cases
```

## Outputs

```text
artifacts/diff.json
reports/summary.md
reports/full.md
children/old/
children/new/
```

## MVP comparison dimensions

- strings added/removed/common
- imports added/removed/common
- exports added/removed/common
- function names added/removed/common
- old/new risk summaries

This is a triage-grade diff. Deep semantic patch analysis should decompile matched functions with Ghidra and use a dedicated Skiller/agent workflow.
