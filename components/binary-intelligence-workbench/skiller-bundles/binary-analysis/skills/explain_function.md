# Skill: binary.explain.function

## Purpose
Explain a selected function from BIW function metadata and optional Ghidra decompiler output.

## Inputs
- `case.json`
- `artifacts/functions.json`
- `artifacts/decompile/<function>.json`
- `reports/functions/<function>.md` when available

## Procedure
1. State function identity, address, signature, and extraction confidence.
2. Summarize control/data flow visible in the decompiled text.
3. Identify referenced APIs, strings, and suspicious primitives.
4. Separate evidence from inference.
5. List follow-up xrefs, callers, callees, or strings to inspect.

## Output Contract
Return Markdown with:
- Function summary
- Evidence-backed observations
- Possible role in the binary
- Uncertainties/limitations
- Follow-up investigation steps

## Safety Boundary
Do not create exploit or evasion guidance. Keep analysis defensive and evidence-based.
