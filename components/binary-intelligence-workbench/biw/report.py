from __future__ import annotations

from pathlib import Path
from typing import Any

from .core import CasePaths, read_json
from .heuristics import normalize_string_items, normalize_symbol_items


def generate_summary(case: dict[str, Any], artifacts: dict[str, dict[str, Any]], findings_bundle: dict[str, Any]) -> str:
    source = case.get("source", {})
    findings = findings_bundle.get("findings", [])
    risk = findings_bundle.get("risk", {})
    strings_count = len(normalize_string_items(artifacts.get("strings", {})))
    imports_count = len(normalize_symbol_items(artifacts.get("imports", {}), ("imports", "symbols", "items", "results")))
    exports_count = len(normalize_symbol_items(artifacts.get("exports", {}), ("exports", "symbols", "items", "results")))
    functions = artifacts.get("functions", {}).get("functions") or artifacts.get("functions", {}).get("items") or []
    lines = [
        f"# Binary Intelligence Summary — {case.get('title')}",
        "",
        "## Case",
        "",
        f"- Case ID: `{case.get('case_id')}`",
        f"- Status: `{case.get('status')}`",
        f"- Created: `{case.get('created_at')}`",
        f"- Source path: `{source.get('path')}`",
        f"- Size: `{source.get('size_bytes')}` bytes",
        f"- SHA-256: `{source.get('sha256')}`",
        f"- Format: `{source.get('format') or 'unknown'}`",
        f"- Architecture: `{source.get('architecture') or 'unknown'}`",
        "",
        "## Extraction Overview",
        "",
        f"- Strings: `{strings_count}`",
        f"- Imports: `{imports_count}`",
        f"- Exports: `{exports_count}`",
        f"- Functions: `{len(functions) if isinstance(functions, list) else 0}`",
        f"- Extraction mode: `{case.get('extraction', {}).get('mode', 'unknown')}`",
        "",
        "## Heuristic Risk",
        "",
        f"- Level: `{risk.get('level', 'unknown')}`",
        f"- Score: `{risk.get('score')}`",
    ]
    rationale = risk.get("rationale") or []
    if rationale:
        lines += ["", "### Rationale", ""]
        lines += [f"- {r}" for r in rationale]
    lines += ["", "## Findings", ""]
    if findings:
        for f in findings:
            lines += [
                f"### {f.get('title')}",
                "",
                f"- Severity: `{f.get('severity')}`",
                f"- Confidence: `{f.get('confidence')}`",
                f"- Category: `{f.get('category')}`",
                f"- Description: {f.get('description')}",
                "",
                "Evidence:",
            ]
            for ev in (f.get("evidence") or [])[:10]:
                lines.append(f"- `{ev.get('type')}` `{ev.get('value')}` in `{ev.get('artifact')}`")
            lines.append("")
    else:
        lines.append("No heuristic findings were generated. This does not prove the binary is safe; it means the MVP rules found no obvious indicators.")
    lines += [
        "",
        "## Recommended Next Steps",
        "",
        "1. Review artifacts under `artifacts/` and findings under `findings/`.",
        "2. If Ghidra was unavailable, rerun with `GHIDRA_HEADLESS` configured for richer extraction.",
        "3. Inspect functions referencing suspicious strings/imports.",
        "4. Preserve only non-secret summaries in CMS memory.",
        "",
        "## Safety Note",
        "",
        "This report is static-analysis oriented. Do not execute unknown binaries on the host; use an explicit sandbox for dynamic analysis.",
    ]
    return "\n".join(lines) + "\n"


def _load_artifacts(paths: CasePaths) -> dict[str, dict[str, Any]]:
    return {name: read_json(paths.artifacts / f"{name}.json", {}) for name in ["strings", "imports", "exports", "functions", "callgraph"]}


def _append_markdown_file(lines: list[str], title: str, path: Path, root: Path) -> None:
    lines += [f"## {title}", "", f"Source: `{path.relative_to(root)}`", ""]
    text = path.read_text(encoding="utf-8", errors="replace").strip()
    if text.startswith("#"):
        # Demote embedded headings by one level to keep the full report structure readable.
        text = "\n".join("#" + line if line.startswith("#") else line for line in text.splitlines())
    lines += [text or "_Empty report._", ""]


def generate_full_report(paths: CasePaths) -> str:
    case = read_json(paths.root / "case.json", {})
    artifacts = _load_artifacts(paths)
    findings_bundle = read_json(paths.findings / "findings.json", {})
    lines = [
        f"# Binary Intelligence Full Report — {case.get('title') or paths.root.name}",
        "",
        "This full report aggregates the BIW summary, skill outputs, function explanations, and analyst notes for one case.",
        "",
        "---",
        "",
    ]
    lines.append(generate_summary(case, artifacts, findings_bundle).strip())
    lines += ["", "---", "", "# Skill Outputs", ""]
    skill_paths = sorted((paths.root / "skills").glob("*.json"))
    if not skill_paths:
        lines += ["No skill outputs have been generated yet. Run `biw skill run <case-id> <skill-id>`.", ""]
    for sp in skill_paths:
        payload = read_json(sp, {})
        result = payload.get("result") or {}
        lines += [
            f"## {payload.get('skill_id')}",
            "",
            f"- Title: `{payload.get('skill_title')}`",
            f"- Created: `{payload.get('created_at')}`",
            f"- Source: `{sp.relative_to(paths.root)}`",
            "",
            f"{result.get('summary', '_No summary provided._')}",
            "",
        ]
        if result.get("risk"):
            risk = result["risk"]
            lines += [f"- Risk level: `{risk.get('level')}`", f"- Risk score: `{risk.get('score')}`", ""]
        if result.get("artifact_counts"):
            lines += ["Artifact counts:", ""]
            for key, value in sorted(result["artifact_counts"].items()):
                lines.append(f"- {key}: `{value}`")
            lines.append("")
        if result.get("recommended_next_steps"):
            lines += ["Recommended next steps:", ""]
            for item in result["recommended_next_steps"]:
                lines.append(f"- {item}")
            lines.append("")
    lines += ["---", "", "# Function Reports", ""]
    function_reports = sorted((paths.reports / "functions").glob("*.md"))
    if not function_reports:
        lines += ["No function reports have been generated yet. Run `biw explain function <case-id> <function>`.", ""]
    for fp in function_reports:
        _append_markdown_file(lines, fp.stem, fp, paths.root)
    notes = paths.notes / "notes.md"
    lines += ["---", "", "# Analyst Notes", ""]
    if notes.exists():
        text = notes.read_text(encoding="utf-8", errors="replace").strip()
        if text.startswith("#"):
            text = "\n".join("#" + line if line.startswith("#") else line for line in text.splitlines())
        lines += [text or "_No notes._", ""]
    else:
        lines += ["_No notes file exists._", ""]
    lines += [
        "---",
        "",
        "# Handling Guidance",
        "",
        "- Treat extracted strings, paths, and symbols as potentially sensitive case artifacts.",
        "- Redact environment-specific or credential-like strings before sharing.",
        "- This report is static-analysis evidence, not a guarantee of benign or malicious behavior.",
    ]
    return "\n".join(lines).rstrip() + "\n"
