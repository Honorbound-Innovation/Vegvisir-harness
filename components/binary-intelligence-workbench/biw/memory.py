from __future__ import annotations

from pathlib import Path
from typing import Any

from .core import CasePaths, read_json, utc_now, write_json

SCHEMA_MEMORY = "biw.memory-summary.v1"

SENSITIVE_MARKERS = ["password", "passwd", "secret", "token", "apikey", "api_key", "private key", "credential"]


def _redact_text(text: str, max_len: int = 4000) -> str:
    text = text[:max_len]
    lowered = text.lower()
    if any(marker in lowered for marker in SENSITIVE_MARKERS):
        return "[redacted: summary contained credential-like marker; review local artifacts instead]"
    return text


def build_memory_summary(case_root: Path) -> dict[str, Any]:
    paths = CasePaths(case_root)
    case = read_json(paths.root / "case.json", {})
    findings = read_json(paths.findings / "findings.json", {})
    capability = read_json(paths.findings / "capability-map.json", {})
    summary_path = paths.reports / "summary.md"
    summary_text = summary_path.read_text(encoding="utf-8", errors="replace") if summary_path.exists() else ""
    payload = {
        "schema_version": SCHEMA_MEMORY,
        "created_at": utc_now(),
        "case_id": case.get("case_id") or paths.root.name,
        "title": case.get("title"),
        "status": case.get("status"),
        "source": {
            "filename": (case.get("source") or {}).get("filename"),
            "sha256": (case.get("source") or {}).get("sha256"),
            "format": (case.get("source") or {}).get("format"),
            "architecture": (case.get("source") or {}).get("architecture"),
        },
        "risk": case.get("risk"),
        "finding_titles": [f.get("title") for f in findings.get("findings", [])[:20]],
        "capabilities": capability.get("capabilities") or capability.get("categories") or capability,
        "local_artifact_root": str(paths.root),
        "redaction_policy": "No raw strings, credentials, decompiler output, or bulk artifacts are included in this memory summary.",
        "summary_excerpt": _redact_text(summary_text),
    }
    return payload


def write_memory_summary(case_root: Path) -> Path:
    payload = build_memory_summary(case_root)
    dest = case_root / "memory" / "cms-summary.json"
    write_json(dest, payload)
    links = case_root / "memory" / "cms-links.json"
    if not links.exists():
        write_json(links, {"schema_version": "biw.cms-links.v1", "links": [], "note": "Populate after storing through CMS-v2."})
    return dest
