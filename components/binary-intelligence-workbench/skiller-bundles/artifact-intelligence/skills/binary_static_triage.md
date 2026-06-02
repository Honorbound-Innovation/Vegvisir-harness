# Skill: binary.static_triage

## Purpose
Perform first-pass static analysis of an executable binary and create normalized artifacts that support reverse engineering, vulnerability research, malware triage, and reporting.

## Use When
- A case contains ELF, PE, Mach-O, firmware binary, shared object, or stripped executable.
- You need metadata, strings, imports, symbols, sections, and candidate functions before deeper analysis.

## Inputs
- `binary_path`
- `case_output_dir`
- optional `ghidra_available`
- optional `analysis_timeout`
- optional `string_min_length`

## Procedure
1. Record hashes, size, file type, architecture, endianness, and linkage/static/PIE hints.
2. Extract sections/segments and protections when tools support it.
3. Extract strings with offsets and classify notable strings.
4. Extract imports/exports/symbols/functions using available tools.
5. Run Ghidra/headless analysis when configured and safe.
6. Note decompiler limitations, stripped status, and suspicious control-flow patterns.
7. Identify candidate entrypoints, validation functions, command handlers, crypto routines, or payload loaders.
8. Generate a concise triage report and recommended next skills.

## Output Contract
- `artifacts/metadata.json`
- `artifacts/hashes.json`
- `artifacts/strings.json`
- `artifacts/imports.json`
- `artifacts/exports.json`
- `artifacts/functions.json`
- optional `artifacts/sections.json`
- `reports/summary.md`
- `recommended_next_steps.md`

## Safety Boundary
Static analysis only unless the user explicitly authorizes sandboxed execution. Treat unknown binaries as potentially malicious.
