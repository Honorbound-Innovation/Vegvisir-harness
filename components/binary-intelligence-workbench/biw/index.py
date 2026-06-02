from __future__ import annotations

from pathlib import Path
from typing import Any

from .core import read_json, write_json, utc_now

INDEX_SCHEMA = "biw.case-index.v1"
DETAIL_SCHEMA = "biw.case-detail.v1"


def resolve_case_root(out: Path, case_id: str) -> Path:
    """Resolve an exact or prefix case id under an analysis cases directory."""
    root = out / case_id
    if (root / "case.json").exists():
        return root
    matches = sorted(out.glob(f"{case_id}*/case.json"))
    if matches:
        return matches[0].parent
    return root


def _count_list_artifact(payload: dict[str, Any], keys: tuple[str, ...]) -> int:
    for key in keys:
        value = payload.get(key)
        if isinstance(value, list):
            return len(value)
    return 0


def _safe_rel(path: Path, root: Path) -> str:
    try:
        return str(path.relative_to(root))
    except ValueError:
        return str(path)


def summarize_case(case_root: Path, out: Path | None = None) -> dict[str, Any]:
    case = read_json(case_root / "case.json", {}) or {}
    findings = read_json(case_root / "findings" / "findings.json", {}) or {}
    metadata = read_json(case_root / "artifacts" / "metadata.json", {}) or {}
    strings = read_json(case_root / "artifacts" / "strings.json", {}) or {}
    imports = read_json(case_root / "artifacts" / "imports.json", {}) or {}
    exports = read_json(case_root / "artifacts" / "exports.json", {}) or {}
    functions = read_json(case_root / "artifacts" / "functions.json", {}) or {}
    skill_paths = sorted((case_root / "skills").glob("*.json"))
    function_report_paths = sorted((case_root / "reports" / "functions").glob("*.md"))
    report_paths = sorted((case_root / "reports").glob("*.md"))
    source = case.get("source") or metadata
    base = out or case_root.parent
    return {
        "case_id": case.get("case_id") or case_root.name,
        "title": case.get("title") or source.get("filename") or case_root.name,
        "status": case.get("status", "unknown"),
        "created_at": case.get("created_at"),
        "updated_at": case.get("updated_at"),
        "risk": case.get("risk", {}),
        "extraction": case.get("extraction", {}),
        "source": {
            "filename": source.get("filename"),
            "size_bytes": source.get("size_bytes"),
            "sha256": source.get("sha256"),
            "format": source.get("format"),
            "architecture": source.get("architecture"),
        },
        "counts": {
            "strings": _count_list_artifact(strings, ("strings", "items", "results")),
            "imports": _count_list_artifact(imports, ("imports", "symbols", "items", "results")),
            "exports": _count_list_artifact(exports, ("exports", "symbols", "items", "results")),
            "functions": _count_list_artifact(functions, ("functions", "items", "results")),
            "findings": len(findings.get("findings") or []),
            "skills": len(skill_paths),
            "function_reports": len(function_report_paths),
        },
        "paths": {
            "case": _safe_rel(case_root, base),
            "summary_report": _safe_rel(case_root / "reports" / "summary.md", base) if (case_root / "reports" / "summary.md").exists() else None,
            "full_report": _safe_rel(case_root / "reports" / "full.md", base) if (case_root / "reports" / "full.md").exists() else None,
        },
        "available_reports": [_safe_rel(p, case_root) for p in report_paths],
        "available_skills": [p.stem for p in skill_paths],
    }


def build_case_index(out: Path) -> dict[str, Any]:
    out = out.expanduser().resolve()
    cases = []
    if out.exists():
        for case_json in sorted(out.glob("*/case.json")):
            cases.append(summarize_case(case_json.parent, out))
    return {
        "schema_version": INDEX_SCHEMA,
        "generated_at": utc_now(),
        "cases_root": str(out),
        "case_count": len(cases),
        "cases": cases,
    }


def build_case_detail(out: Path, case_id: str, include_artifacts: bool = False) -> dict[str, Any]:
    out = out.expanduser().resolve()
    root = resolve_case_root(out, case_id)
    if not (root / "case.json").exists():
        raise FileNotFoundError(f"case not found: {case_id}")
    detail: dict[str, Any] = {
        "schema_version": DETAIL_SCHEMA,
        "generated_at": utc_now(),
        "summary": summarize_case(root, out),
        "case": read_json(root / "case.json", {}),
        "findings": read_json(root / "findings" / "findings.json", {}),
        "capability_map": read_json(root / "findings" / "capability-map.json", {}),
        "skill_outputs": [],
        "function_reports": [],
        "reports": {},
    }
    for sp in sorted((root / "skills").glob("*.json")):
        detail["skill_outputs"].append(read_json(sp, {}))
    for fp in sorted((root / "reports" / "functions").glob("*.md")):
        detail["function_reports"].append({"name": fp.stem, "path": str(fp.relative_to(root)), "markdown": fp.read_text(encoding="utf-8", errors="replace")})
    for report_name in ["summary", "full"]:
        rp = root / "reports" / f"{report_name}.md"
        if rp.exists():
            detail["reports"][report_name] = rp.read_text(encoding="utf-8", errors="replace")
    if include_artifacts:
        detail["artifacts"] = {
            name: read_json(root / "artifacts" / f"{name}.json", {})
            for name in ["metadata", "hashes", "strings", "imports", "exports", "functions", "callgraph"]
        }
    return detail


def write_case_index(out: Path, destination: Path | None = None) -> Path:
    out = out.expanduser().resolve()
    payload = build_case_index(out)
    dest = destination.expanduser().resolve() if destination else out / "index.json"
    write_json(dest, payload)
    return dest
