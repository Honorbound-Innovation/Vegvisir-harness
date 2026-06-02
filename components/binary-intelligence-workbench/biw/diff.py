from __future__ import annotations

from pathlib import Path
from typing import Any

from .core import (
    SCHEMA_CALLGRAPH, SCHEMA_EXPORTS, SCHEMA_FUNCTIONS, SCHEMA_HASHES,
    SCHEMA_IMPORTS, SCHEMA_METADATA, SCHEMA_STRINGS, CasePaths, detect_format,
    extract_ascii_strings, make_case_id, parse_nm_symbols, read_json, sha256_file,
    utc_now, write_json,
)
from .heuristics import build_findings, normalize_string_items, normalize_symbol_items
from .report import generate_summary

SCHEMA_DIFF = "biw.diff.v1"


def _basic_artifacts(binary: Path, limit: int) -> dict[str, dict[str, Any]]:
    imports, exports, functions = parse_nm_symbols(binary)
    return {
        "strings": {"schema_version": SCHEMA_STRINGS, "strings": extract_ascii_strings(binary, limit=limit), "source": "basic"},
        "imports": {"schema_version": SCHEMA_IMPORTS, "imports": imports, "source": "basic-nm" if imports else "basic"},
        "exports": {"schema_version": SCHEMA_EXPORTS, "exports": exports, "source": "basic-nm" if exports else "basic"},
        "functions": {"schema_version": SCHEMA_FUNCTIONS, "functions": functions, "source": "basic-nm" if functions else "basic"},
        "callgraph": {"schema_version": SCHEMA_CALLGRAPH, "edges": [], "source": "basic"},
    }


def _symbol_names(payload: dict[str, Any], keys: tuple[str, ...]) -> set[str]:
    names: set[str] = set()
    for item in normalize_symbol_items(payload, keys):
        if isinstance(item, dict):
            value = item.get("name") or item.get("symbol") or item.get("label") or item.get("value")
        else:
            value = str(item)
        if value:
            names.add(str(value))
    return names


def _function_names(payload: dict[str, Any]) -> set[str]:
    out: set[str] = set()
    for item in payload.get("functions") or payload.get("items") or []:
        if isinstance(item, dict):
            value = item.get("name") or item.get("id") or item.get("address")
        else:
            value = str(item)
        if value:
            out.add(str(value))
    return out


def _string_values(payload: dict[str, Any]) -> set[str]:
    out: set[str] = set()
    for item in normalize_string_items(payload):
        if isinstance(item, dict):
            value = item.get("value") or item.get("string") or item.get("text")
        else:
            value = str(item)
        if value:
            out.add(str(value))
    return out


def _case_for_binary(binary: Path, out: Path, case_id: str, role: str, limit: int, overwrite: bool) -> tuple[CasePaths, dict[str, Any], dict[str, dict[str, Any]], dict[str, Any]]:
    sha = sha256_file(binary)
    paths = CasePaths(out / case_id)
    if paths.root.exists() and not overwrite:
        raise FileExistsError(f"case already exists: {paths.root}")
    paths.ensure()
    sample = binary.read_bytes()[:4096]
    fmt = detect_format(binary, sample)
    now = utc_now()
    case = {
        "schema_version": "biw.case.v1",
        "case_id": case_id,
        "title": f"{role}: {binary.name}",
        "created_at": now,
        "updated_at": now,
        "source": {
            "path": str(binary), "filename": binary.name, "size_bytes": binary.stat().st_size,
            "sha256": sha, "mime": fmt.get("mime"), "format": fmt.get("format"), "architecture": fmt.get("architecture"),
        },
        "status": "triaged",
        "tags": ["diff-child", role, *[t for t in [fmt.get("format"), fmt.get("architecture")] if t]],
        "risk": {"level": "unknown", "score": None, "rationale": []},
        "extraction": {"mode": "basic-diff"},
    }
    artifacts = _basic_artifacts(binary, limit)
    for name, artifact in artifacts.items():
        write_json(paths.artifacts / f"{name}.json", artifact)
    suspicious, capability, findings = build_findings(artifacts["strings"], artifacts["imports"])
    case["risk"] = findings.get("risk", case["risk"])
    write_json(paths.root / "case.json", case)
    write_json(paths.artifacts / "hashes.json", {"schema_version": SCHEMA_HASHES, "sha256": sha})
    write_json(paths.artifacts / "metadata.json", {"schema_version": SCHEMA_METADATA, **case["source"]})
    write_json(paths.root / "source" / "original-ref.json", {"path": str(binary), "sha256": sha, "copied": False})
    write_json(paths.findings / "suspicious-strings.json", suspicious)
    write_json(paths.findings / "capability-map.json", capability)
    write_json(paths.findings / "findings.json", findings)
    (paths.reports / "summary.md").write_text(generate_summary(case, artifacts, findings), encoding="utf-8")
    (paths.notes / "notes.md").write_text(f"# Notes — {case_id}\n\n", encoding="utf-8")
    return paths, case, artifacts, findings


def build_binary_diff(old_binary: Path, new_binary: Path, out: Path, case_id: str | None = None, limit: int = 1000, overwrite: bool = False) -> tuple[Path, dict[str, Any]]:
    old_binary = old_binary.expanduser().resolve()
    new_binary = new_binary.expanduser().resolve()
    if not old_binary.is_file():
        raise FileNotFoundError(f"old binary not found: {old_binary}")
    if not new_binary.is_file():
        raise FileNotFoundError(f"new binary not found: {new_binary}")
    old_sha = sha256_file(old_binary)
    new_sha = sha256_file(new_binary)
    diff_id = case_id or f"diff-{make_case_id(old_binary, old_sha)}-to-{make_case_id(new_binary, new_sha)}"[:120]
    root = out.expanduser().resolve() / diff_id
    if root.exists() and not overwrite:
        raise FileExistsError(f"diff case already exists: {root}")
    paths = CasePaths(root)
    paths.ensure()

    old_paths, old_case, old_artifacts, old_findings = _case_for_binary(old_binary, root / "children", "old", "old", limit, True)
    new_paths, new_case, new_artifacts, new_findings = _case_for_binary(new_binary, root / "children", "new", "new", limit, True)

    old_strings = _string_values(old_artifacts["strings"])
    new_strings = _string_values(new_artifacts["strings"])
    old_imports = _symbol_names(old_artifacts["imports"], ("imports", "symbols", "items", "results"))
    new_imports = _symbol_names(new_artifacts["imports"], ("imports", "symbols", "items", "results"))
    old_exports = _symbol_names(old_artifacts["exports"], ("exports", "symbols", "items", "results"))
    new_exports = _symbol_names(new_artifacts["exports"], ("exports", "symbols", "items", "results"))
    old_functions = _function_names(old_artifacts["functions"])
    new_functions = _function_names(new_artifacts["functions"])

    def delta(a: set[str], b: set[str], max_items: int = 200) -> dict[str, Any]:
        added = sorted(b - a)
        removed = sorted(a - b)
        common = sorted(a & b)
        return {
            "added_count": len(added), "removed_count": len(removed), "common_count": len(common),
            "added": added[:max_items], "removed": removed[:max_items], "truncated": len(added) > max_items or len(removed) > max_items,
        }

    now = utc_now()
    payload = {
        "schema_version": SCHEMA_DIFF,
        "case_id": diff_id,
        "created_at": now,
        "old": {"path": str(old_binary), "sha256": old_sha, "case_path": str(old_paths.root.relative_to(root))},
        "new": {"path": str(new_binary), "sha256": new_sha, "case_path": str(new_paths.root.relative_to(root))},
        "deltas": {
            "strings": delta(old_strings, new_strings),
            "imports": delta(old_imports, new_imports),
            "exports": delta(old_exports, new_exports),
            "functions": delta(old_functions, new_functions),
        },
        "risk_change": {"old": old_findings.get("risk"), "new": new_findings.get("risk")},
        "notes": ["Diff is heuristic/name-based in MVP mode; use exact Ghidra function matching for deeper patch review."],
    }
    case = {
        "schema_version": "biw.case.v1",
        "case_id": diff_id,
        "title": f"Diff: {old_binary.name} -> {new_binary.name}",
        "created_at": now,
        "updated_at": now,
        "source": {"path": f"{old_binary} -> {new_binary}", "filename": diff_id, "size_bytes": None, "sha256": f"{old_sha[:12]}..{new_sha[:12]}", "mime": "application/vnd.biw.diff", "format": "BIW_DIFF", "architecture": None},
        "status": "diffed",
        "tags": ["diff"],
        "risk": payload["risk_change"].get("new") or {"level": "unknown", "score": None, "rationale": []},
        "extraction": {"mode": "basic-diff"},
    }
    write_json(paths.root / "case.json", case)
    write_json(paths.artifacts / "diff.json", payload)
    report = generate_diff_report(payload)
    (paths.reports / "summary.md").write_text(report, encoding="utf-8")
    (paths.reports / "full.md").write_text(report, encoding="utf-8")
    (paths.notes / "notes.md").write_text(f"# Notes — {diff_id}\n\n", encoding="utf-8")
    return paths.root, payload


def generate_diff_report(payload: dict[str, Any]) -> str:
    lines = [
        f"# Binary Diff Report — {payload.get('case_id')}", "",
        "## Inputs", "",
        f"- Old: `{payload.get('old', {}).get('path')}`", f"- Old SHA-256: `{payload.get('old', {}).get('sha256')}`",
        f"- New: `{payload.get('new', {}).get('path')}`", f"- New SHA-256: `{payload.get('new', {}).get('sha256')}`", "",
        "## Delta Summary", "",
    ]
    for key, delta in (payload.get("deltas") or {}).items():
        lines += [f"### {key.title()}", "", f"- Added: `{delta.get('added_count')}`", f"- Removed: `{delta.get('removed_count')}`", f"- Common: `{delta.get('common_count')}`", ""]
        if delta.get("added"):
            lines += ["Added sample:", *[f"- `{v}`" for v in delta["added"][:25]], ""]
        if delta.get("removed"):
            lines += ["Removed sample:", *[f"- `{v}`" for v in delta["removed"][:25]], ""]
    lines += ["## Review Guidance", "", "- Prioritize added imports, added network/crypto strings, and changed exported interfaces.", "- Run `biw explain function` on changed or suspicious functions when symbols are available."]
    return "\n".join(lines) + "\n"
