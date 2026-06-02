# Binary Intelligence Workbench

Binary Intelligence Workbench (BIW) is a Vegvisir-native reverse-engineering and binary triage suite. It combines Ghidra-backed static extraction, deterministic fallback extraction, case files, findings, Skiller-style workflows, reports, a local JSON API, a static Solarium-facing workbench UI, firmware triage, binary diffing, CMS-safe memory summaries, and agent task artifacts.

## Quickstart

```bash
cd binary-intelligence-workbench
python3 -m biw.cli status
python3 -m biw.cli triage /bin/true --out .analysis/cases
python3 -m biw.cli case list --out .analysis/cases
python3 -m biw.cli report full <case-id> --out .analysis/cases --write
```

If `../GhidraHeadlessMCP/bin/ghidra-headless` is available, BIW uses Ghidra extraction. Otherwise it falls back to deterministic static extraction.

## Core commands

```bash
# Binary triage
python3 -m biw.cli triage ./sample.bin --out .analysis/cases
python3 -m biw.cli explain function <case-id> main --out .analysis/cases

# Skills and reports
python3 -m biw.cli skill list
python3 -m biw.cli skill run <case-id> binary.triage.unknown --out .analysis/cases
python3 -m biw.cli report full <case-id> --out .analysis/cases --write

# Solarium/API backend
python3 -m biw.cli case index --out .analysis/cases --write
python3 -m biw.cli case export <case-id> --out .analysis/cases --write
python3 -m biw.cli serve --out .analysis/cases --host 127.0.0.1 --port 8765

# Static Solarium-facing UI
python3 -m http.server 8787 --directory solarium/binary-workbench
# open http://127.0.0.1:8787 while BIW API is running

# Diffing and firmware
python3 -m biw.cli diff ./old.bin ./new.bin --out .analysis/cases
python3 -m biw.cli firmware triage ./firmware.img --out .analysis/cases

# Memory and agent task artifacts
python3 -m biw.cli memory export <case-id> --out .analysis/cases --write
python3 -m biw.cli agent list
python3 -m biw.cli agent plan <case-id> binary-triage-agent "Review this case" --out .analysis/cases
```

## Case outputs

BIW writes stable case directories under `.analysis/cases/<case-id>/` containing:

- `case.json`
- normalized `artifacts/*.json`
- `findings/*.json`
- `skills/*.json`
- `reports/summary.md` and `reports/full.md`
- `reports/functions/*.md`
- `memory/cms-summary.json`
- `jobs/agents/*.json`

## Safety

BIW is static-analysis oriented by default. Do not execute unknown binaries or firmware on the host. Treat extracted strings, paths, symbols, notes, and reports as potentially sensitive. CMS memory exports intentionally omit raw strings/decompiler output and redact credential-like summaries.

## Verification

```bash
cd binary-intelligence-workbench
python3 -m compileall -q biw
python3 -m unittest discover -s tests -v
```
