# Binary Intelligence Workbench — MVP Architecture

The current implementation is the CLI/backend spine for the larger Solarium + Ghidra + Skiller binary analysis suite.

## Components

- `biw.cli`: command-line entrypoint.
- `biw.core`: schemas, hashing, case paths, basic binary identification, deterministic fallback extraction.
- `biw.ghidra`: opportunistic integration with `../GhidraHeadlessMCP/bin/ghidra-headless`.
- `biw.heuristics`: suspicious string and import/capability detection.
- `biw.explain`: function selection, decompile artifact generation, and function Markdown reports.
- `biw.skill`: deterministic local execution for the starter binary-analysis Skiller workflows.
- `biw.report`: Markdown summary and aggregate full-report generation.
- `biw.index`: normalized case index and detailed case export payloads for UI/API consumers.
- `biw.server`: read-only localhost JSON API for Solarium or other frontends.

## Extraction Modes

### Ghidra mode

If the Ghidra wrapper and `analyzeHeadless` are available, `biw triage` imports the binary into a per-case Ghidra project and asks existing Vegvisir Ghidra scripts for strings, imports, exports, functions, and callgraph data.

### Basic fallback mode

If Ghidra is unavailable or `--no-ghidra` is passed, BIW still creates a valid case using:

- SHA-256 hashing.
- Magic-byte format detection.
- ASCII string extraction.
- `nm`-based symbol extraction when available.

This fallback exists for development, CI, and environments where Ghidra is not installed.

## Case data model

A case lives under:

```text
.analysis/cases/<case_id>/
```

Important paths:

```text
case.json                         canonical case record
artifacts/*.json                  extraction artifacts
findings/*.json                   heuristic findings and capability map
artifacts/decompile/*.json        function decompile/explain artifacts
reports/summary.md                concise triage report
reports/full.md                   aggregate report
reports/functions/*.md            function reports
skills/*.json                     deterministic Skiller workflow outputs
notes/notes.md                    analyst notes
```

## UI/API boundary

The Solarium-facing layer is intentionally normalized and read-only by default.

CLI exports:

```bash
biw case index --out .analysis/cases
biw case index --out .analysis/cases --write
biw case export <case-id> --out .analysis/cases
biw case export <case-id> --out .analysis/cases --include-artifacts
```

Local API:

```bash
biw serve --out .analysis/cases --host 127.0.0.1 --port 8765
```

Endpoints:

```text
GET /health
GET /api/index
GET /api/cases
GET /api/cases/<case-id>
GET /api/cases/<case-id>?include_artifacts=true
```

The default API host is `127.0.0.1`. It performs no mutation and should remain read-only unless a future explicit approval/auth model is added.

## Skiller boundary

The repository contains a starter bundle under:

```text
skiller-bundles/binary-analysis/
```

`biw.skill` currently executes deterministic in-process implementations for the starter workflow IDs. This makes the workflow testable without an external Skiller runtime while preserving the Markdown skill files as the human-readable contracts.

## Safety

The MVP performs static analysis only. It does not execute analyzed binaries. Dynamic analysis should be added later only through explicit sandbox workflows.

Reports, JSON exports, and API responses may contain extracted binary strings, filesystem paths, symbol names, and analyst notes. Treat them as case artifacts and redact before sharing.
