# CMS Memory Model

BIW memory integration is conservative by default.

## Command

```bash
python3 -m biw.cli memory export <case-id> --out .analysis/cases --write
```

## Output

```text
memory/cms-summary.json
memory/cms-links.json
```

`cms-summary.json` contains case identity, source hash/format, risk summary, finding titles, capability summary, local artifact root, and a redacted summary excerpt.

## Redaction policy

BIW memory summaries do not include:

- raw extracted strings
- credential-like values
- decompiler output
- raw binary bytes
- bulk JSON artifacts

Store the JSON summary through CMS-v2 only after review. Link CMS memory IDs in `cms-links.json`.
