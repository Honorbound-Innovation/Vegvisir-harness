from __future__ import annotations

import tarfile
import zipfile
from pathlib import Path
from typing import Any

from .core import CasePaths, detect_format, extract_ascii_strings, make_case_id, sha256_file, slugify, utc_now, write_json
from .heuristics import build_findings
from .report import generate_summary

SCHEMA_FIRMWARE = "biw.firmware.v1"

MAGICS = [
    (b"\x7fELF", "ELF"), (b"MZ", "PE"), (b"#!/bin/sh", "shell-script"), (b"#!/bin/bash", "bash-script"),
    (b"PK\x03\x04", "zip"), (b"\x1f\x8b", "gzip"), (b"hsqs", "squashfs-le"), (b"sqsh", "squashfs-be"),
]


def _scan_embedded(path: Path, data: bytes, max_hits: int = 500) -> list[dict[str, Any]]:
    hits: list[dict[str, Any]] = []
    for magic, kind in MAGICS:
        start = 0
        while len(hits) < max_hits:
            idx = data.find(magic, start)
            if idx < 0:
                break
            hits.append({"offset": idx, "kind": kind, "magic_hex": magic.hex(), "source": path.name})
            start = idx + 1
    return sorted(hits, key=lambda x: x["offset"])


def _archive_inventory(path: Path, max_entries: int = 2000) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    try:
        if zipfile.is_zipfile(path):
            with zipfile.ZipFile(path) as zf:
                for info in zf.infolist()[:max_entries]:
                    out.append({"path": info.filename, "size": info.file_size, "compressed_size": info.compress_size, "kind": "zip-entry"})
        elif tarfile.is_tarfile(path):
            with tarfile.open(path) as tf:
                for member in tf.getmembers()[:max_entries]:
                    out.append({"path": member.name, "size": member.size, "kind": "tar-entry", "is_file": member.isfile()})
    except Exception as exc:
        out.append({"error": str(exc), "kind": "archive-error"})
    return out


def triage_firmware(firmware: Path, out: Path, case_id: str | None = None, limit: int = 2000, overwrite: bool = False) -> tuple[Path, dict[str, Any]]:
    firmware = firmware.expanduser().resolve()
    if not firmware.is_file():
        raise FileNotFoundError(f"firmware image not found: {firmware}")
    sha = sha256_file(firmware)
    cid = case_id or f"firmware-{make_case_id(firmware, sha)}"
    paths = CasePaths(out.expanduser().resolve() / cid)
    if paths.root.exists() and not overwrite:
        raise FileExistsError(f"firmware case already exists: {paths.root}")
    paths.ensure()
    data = firmware.read_bytes()
    sample = data[:4096]
    fmt = detect_format(firmware, sample)
    strings = {"schema_version": "biw.strings.v1", "strings": extract_ascii_strings(firmware, limit=limit), "source": "firmware-basic"}
    imports = {"schema_version": "biw.imports.v1", "imports": [], "source": "firmware-basic"}
    suspicious, capability, findings = build_findings(strings, imports)
    inventory = {
        "schema_version": SCHEMA_FIRMWARE,
        "firmware": {"path": str(firmware), "filename": firmware.name, "size_bytes": firmware.stat().st_size, "sha256": sha, **fmt},
        "embedded_candidates": _scan_embedded(firmware, data),
        "archive_inventory": _archive_inventory(firmware),
        "string_count": len(strings["strings"]),
        "guidance": ["MVP firmware triage does not extract files destructively; use a sandboxed extractor for deeper analysis."],
    }
    now = utc_now()
    case = {
        "schema_version": "biw.case.v1", "case_id": cid, "title": f"Firmware: {firmware.name}", "created_at": now, "updated_at": now,
        "source": {"path": str(firmware), "filename": firmware.name, "size_bytes": firmware.stat().st_size, "sha256": sha, "mime": fmt.get("mime"), "format": fmt.get("format") or "firmware/blob", "architecture": fmt.get("architecture")},
        "status": "firmware-triaged", "tags": ["firmware", fmt.get("format") or "blob"], "risk": findings.get("risk", {"level": "unknown", "score": None, "rationale": []}), "extraction": {"mode": "firmware-basic"},
    }
    write_json(paths.root / "case.json", case)
    write_json(paths.artifacts / "metadata.json", {"schema_version": "biw.metadata.v1", **case["source"]})
    write_json(paths.artifacts / "hashes.json", {"schema_version": "biw.hashes.v1", "sha256": sha})
    write_json(paths.artifacts / "strings.json", strings)
    write_json(paths.artifacts / "imports.json", imports)
    write_json(paths.artifacts / "exports.json", {"schema_version": "biw.exports.v1", "exports": [], "source": "firmware-basic"})
    write_json(paths.artifacts / "functions.json", {"schema_version": "biw.functions.v1", "functions": [], "source": "firmware-basic"})
    write_json(paths.artifacts / "callgraph.json", {"schema_version": "biw.callgraph.v1", "edges": [], "source": "firmware-basic"})
    write_json(paths.artifacts / "firmware-inventory.json", inventory)
    write_json(paths.findings / "suspicious-strings.json", suspicious)
    write_json(paths.findings / "capability-map.json", capability)
    write_json(paths.findings / "findings.json", findings)
    (paths.reports / "summary.md").write_text(generate_firmware_report(case, inventory, findings), encoding="utf-8")
    (paths.notes / "notes.md").write_text(f"# Notes — {cid}\n\n", encoding="utf-8")
    return paths.root, inventory


def generate_firmware_report(case: dict[str, Any], inventory: dict[str, Any], findings: dict[str, Any]) -> str:
    fw = inventory.get("firmware", {})
    lines = [f"# Firmware Triage Report — {fw.get('filename')}", "", "## Case", "", f"- Case ID: `{case.get('case_id')}`", f"- Size: `{fw.get('size_bytes')}` bytes", f"- SHA-256: `{fw.get('sha256')}`", f"- Format guess: `{fw.get('format') or 'blob'}`", "", "## Inventory", "", f"- Strings extracted: `{inventory.get('string_count')}`", f"- Embedded candidates: `{len(inventory.get('embedded_candidates') or [])}`", f"- Archive entries: `{len(inventory.get('archive_inventory') or [])}`", ""]
    if inventory.get("embedded_candidates"):
        lines += ["### Embedded Candidates", ""]
        for hit in inventory["embedded_candidates"][:50]:
            lines.append(f"- Offset `{hit.get('offset')}`: `{hit.get('kind')}`")
        lines.append("")
    if inventory.get("archive_inventory"):
        lines += ["### Archive Inventory Sample", ""]
        for item in inventory["archive_inventory"][:50]:
            lines.append(f"- `{item.get('path')}` size=`{item.get('size')}` kind=`{item.get('kind')}`")
        lines.append("")
    lines += ["## Findings", ""]
    for finding in findings.get("findings") or []:
        lines.append(f"- **{finding.get('title')}** ({finding.get('severity')}): {finding.get('description')}")
    if not findings.get("findings"):
        lines.append("No MVP heuristic findings were generated.")
    lines += ["", "## Safety", "", "Do not boot or execute unknown firmware outside an explicit sandbox/lab environment."]
    return "\n".join(lines) + "\n"
