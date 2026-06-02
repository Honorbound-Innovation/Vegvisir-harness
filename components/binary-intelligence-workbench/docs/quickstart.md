# BIW CLI Quickstart

## Status

```bash
python3 -m biw.cli status
```

## Triage

```bash
python3 -m biw.cli triage /bin/true --out .analysis/cases
```

For deterministic fallback-only extraction:

```bash
python3 -m biw.cli triage /bin/true --out .analysis/cases --no-ghidra
```

## Inspect Cases

```bash
python3 -m biw.cli case list --out .analysis/cases
python3 -m biw.cli case show true-<hash-prefix> --out .analysis/cases
python3 -m biw.cli report true-<hash-prefix> --out .analysis/cases
```

## Solarium/API-facing case data

Print a normalized case index suitable for a UI case browser:

```bash
python3 -m biw.cli case index --out .analysis/cases
```

Write the index to `.analysis/cases/index.json`:

```bash
python3 -m biw.cli case index --out .analysis/cases --write
```

Export one detailed case document:

```bash
python3 -m biw.cli case export <case-id> --out .analysis/cases
```

Include larger raw artifact payloads when needed:

```bash
python3 -m biw.cli case export <case-id> --out .analysis/cases --include-artifacts --write
```

Start the read-only local JSON API for Solarium or another frontend:

```bash
python3 -m biw.cli serve --out .analysis/cases --host 127.0.0.1 --port 8765
```

Endpoints:

```text
GET /health
GET /api/index
GET /api/cases
GET /api/cases/<case-id>
GET /api/cases/<case-id>?include_artifacts=true
```

The server is intentionally read-only and binds to localhost by default.

## Artifacts Created

```text
.analysis/cases/<case_id>/
  case.json
  source/original-ref.json
  artifacts/metadata.json
  artifacts/hashes.json
  artifacts/strings.json
  artifacts/imports.json
  artifacts/exports.json
  artifacts/functions.json
  artifacts/callgraph.json
  findings/suspicious-strings.json
  findings/capability-map.json
  findings/findings.json
  reports/summary.md
  notes/notes.md
```

## Function explain workflow

After triage, explain/decompile a function with:

```bash
python3 -m biw.cli explain function <case-id> <function-name-or-address> --out .analysis/cases
```

Examples:

```bash
python3 -m biw.cli explain function true-b381cc8c55ec main --out .analysis/cases
python3 -m biw.cli explain function true-b381cc8c55ec 0x00102340 --out .analysis/cases
```

Outputs are written to:

```text
.analysis/cases/<case-id>/artifacts/decompile/<function>.json
.analysis/cases/<case-id>/reports/functions/<function>.md
```

If Ghidra is unavailable, use `--no-ghidra` to create a metadata-only function report. Exact function names or addresses are preferred. If an exact match is not present in `artifacts/functions.json`, BIW falls back to prefix and then substring matching.

## Skiller workflow execution

BIW includes a local starter Skiller bundle under:

```text
skiller-bundles/binary-analysis/
```

List available workflows:

```bash
python3 -m biw.cli skill list
```

Run deterministic skill workflows against a triaged case:

```bash
python3 -m biw.cli skill run <case-id> binary.triage.unknown --out .analysis/cases
python3 -m biw.cli skill run <case-id> binary.explain.function --out .analysis/cases
python3 -m biw.cli skill run <case-id> binary.generate_re_report --out .analysis/cases
```

Skill outputs are saved as structured JSON:

```text
.analysis/cases/<case-id>/skills/<skill-id>.json
```

The current implementation executes a deterministic in-process runner for the starter bundle. It does not require an external Skiller service. The Markdown skill files remain the human-readable workflow contract and the JSON outputs preserve what was run, when, and against which case artifacts.

## Full reports

Print an aggregate report:

```bash
python3 -m biw.cli report full <case-id> --out .analysis/cases
```

Write it to `reports/full.md`:

```bash
python3 -m biw.cli report full <case-id> --out .analysis/cases --write
```

Backward-compatible summary commands are also supported:

```bash
python3 -m biw.cli report <case-id> --out .analysis/cases
python3 -m biw.cli report summary <case-id> --out .analysis/cases
```

## Binary diffing

Create a diff case comparing two binaries/blobs:

```bash
python3 -m biw.cli diff ./old.bin ./new.bin --out .analysis/cases
```

BIW writes:

```text
.analysis/cases/<diff-case>/artifacts/diff.json
.analysis/cases/<diff-case>/reports/summary.md
.analysis/cases/<diff-case>/children/old/
.analysis/cases/<diff-case>/children/new/
```

The MVP diff is name/string based and is intended for patch triage. Use Ghidra-backed function analysis for deeper review.

## Firmware triage

```bash
python3 -m biw.cli firmware triage ./firmware.img --out .analysis/cases
```

Outputs include:

```text
artifacts/firmware-inventory.json
reports/summary.md
```

The firmware MVP inventories archive entries where possible, scans embedded file magics, extracts strings, and applies the same heuristic findings. It does not boot or execute firmware and does not destructively unpack images.

## CMS-safe memory summaries

Print a redacted case memory payload:

```bash
python3 -m biw.cli memory export <case-id> --out .analysis/cases
```

Write it locally for later CMS-v2 storage/linking:

```bash
python3 -m biw.cli memory export <case-id> --out .analysis/cases --write
```

Output:

```text
.analysis/cases/<case-id>/memory/cms-summary.json
.analysis/cases/<case-id>/memory/cms-links.json
```

The summary excludes raw strings, decompiler output, and bulk artifacts.

## Agent task artifacts

List available BIW agent profiles:

```bash
python3 -m biw.cli agent list
```

Plan a bounded task artifact:

```bash
python3 -m biw.cli agent plan <case-id> binary-triage-agent "Review high-risk findings" --out .analysis/cases
```

Output:

```text
.analysis/cases/<case-id>/jobs/agents/<task-id>.json
```

This MVP records executable context for Vegvisir/subagent orchestration; it does not silently launch external agents from the CLI.

## Static Solarium-facing UI

Run the BIW API:

```bash
python3 -m biw.cli serve --out .analysis/cases --host 127.0.0.1 --port 8765
```

Serve the static UI scaffold:

```bash
python3 -m http.server 8787 --directory solarium/binary-workbench
```

Open:

```text
http://127.0.0.1:8787
```

The UI reads:

```text
GET http://127.0.0.1:8765/health
GET http://127.0.0.1:8765/api/index
GET http://127.0.0.1:8765/api/cases/<case-id>
```
