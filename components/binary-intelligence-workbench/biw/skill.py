from __future__ import annotations

import re
import tomllib
from pathlib import Path
from typing import Any

from .core import CasePaths, read_json, slugify, utc_now, write_json
from .heuristics import normalize_string_items, normalize_symbol_items
from .report import generate_full_report

SKILL_OUTPUT_SCHEMA = "biw.skill-output.v1"
DEFAULT_BUNDLE = Path(__file__).resolve().parents[1] / "skiller-bundles" / "binary-analysis"


class SkillError(RuntimeError):
    pass


def resolve_case(out: Path, case_id: str) -> CasePaths:
    root = out / case_id
    if not (root / "case.json").exists():
        matches = list(out.glob(f"{case_id}*/case.json"))
        if matches:
            root = matches[0].parent
    if not (root / "case.json").exists():
        raise FileNotFoundError(f"case not found: {case_id}")
    return CasePaths(root)


def load_bundle(bundle_path: Path = DEFAULT_BUNDLE) -> dict[str, Any]:
    manifest = bundle_path / "bundle.toml"
    if not manifest.exists():
        raise SkillError(f"bundle manifest not found: {manifest}")
    data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    skills: dict[str, dict[str, Any]] = {}
    for path in sorted((bundle_path / "skills").glob("*.md")):
        text = path.read_text(encoding="utf-8")
        skill_id = extract_skill_id(text) or path.stem.replace("_", ".")
        skills[skill_id] = {
            "id": skill_id,
            "path": str(path),
            "title": extract_title(text) or skill_id,
            "body": text,
        }
    data["path"] = str(bundle_path)
    data["skills"] = skills
    return data


def extract_title(text: str) -> str | None:
    for line in text.splitlines():
        if line.startswith("# "):
            return line[2:].strip()
    return None


def extract_skill_id(text: str) -> str | None:
    patterns = [r"(?im)^skill[_ -]?id\s*[:=]\s*`?([A-Za-z0-9_.:-]+)`?", r"(?im)^id\s*[:=]\s*`?([A-Za-z0-9_.:-]+)`?"]
    for pat in patterns:
        m = re.search(pat, text)
        if m:
            return m.group(1).strip()
    for line in text.splitlines()[:12]:
        m = re.search(r"(?i)^#\s*Skill:\s*(binary\.[A-Za-z0-9_.:-]+)\s*$", line.strip())
        if m:
            return m.group(1)
        m = re.search(r"`(binary\.[A-Za-z0-9_.:-]+)`", line)
        if m:
            return m.group(1)
    return None


def load_case_artifacts(paths: CasePaths) -> dict[str, Any]:
    artifacts = {}
    for name in ["metadata", "hashes", "strings", "imports", "exports", "functions", "callgraph"]:
        artifacts[name] = read_json(paths.artifacts / f"{name}.json", {})
    findings = {
        "findings": read_json(paths.findings / "findings.json", {}),
        "suspicious_strings": read_json(paths.findings / "suspicious-strings.json", {}),
        "capability_map": read_json(paths.findings / "capability-map.json", {}),
    }
    return {"case": read_json(paths.root / "case.json", {}), "artifacts": artifacts, "findings": findings}


def artifact_counts(artifacts: dict[str, Any]) -> dict[str, int]:
    return {
        "strings": len(normalize_string_items(artifacts.get("strings", {}))),
        "imports": len(normalize_symbol_items(artifacts.get("imports", {}), ("imports", "symbols", "items", "results"))),
        "exports": len(normalize_symbol_items(artifacts.get("exports", {}), ("exports", "symbols", "items", "results"))),
        "functions": len((artifacts.get("functions", {}) or {}).get("functions") or (artifacts.get("functions", {}) or {}).get("items") or []),
        "callgraph_edges": len((artifacts.get("callgraph", {}) or {}).get("edges") or (artifacts.get("callgraph", {}) or {}).get("calls") or []),
    }


def run_triage_unknown(context: dict[str, Any]) -> dict[str, Any]:
    case = context["case"]
    artifacts = context["artifacts"]
    findings_bundle = context["findings"].get("findings") or {}
    findings = findings_bundle.get("findings") or []
    risk = findings_bundle.get("risk") or case.get("risk") or {}
    capability = context["findings"].get("capability_map") or {}
    caps = capability.get("capabilities") or capability.get("items") or []
    top_findings = sorted(findings, key=lambda f: {"critical": 4, "high": 3, "medium": 2, "low": 1, "info": 0}.get(str(f.get("severity", "")).lower(), 0), reverse=True)[:10]
    strings = normalize_string_items(artifacts.get("strings", {}))[:20]
    return {
        "summary": f"Case {case.get('case_id')} is triaged as {risk.get('level', 'unknown')} risk with {len(findings)} heuristic findings.",
        "risk": risk,
        "artifact_counts": artifact_counts(artifacts),
        "capabilities": caps,
        "top_findings": top_findings,
        "representative_strings": strings,
        "recommended_next_steps": [
            "Review high/medium findings and validate evidence manually.",
            "Run `biw explain function` on functions near suspicious imports or strings.",
            "Generate `biw report full` after function-level notes are added.",
        ],
    }


def run_explain_function(context: dict[str, Any]) -> dict[str, Any]:
    paths: CasePaths = context["paths"]
    decompile_dir = paths.artifacts / "decompile"
    reports_dir = paths.reports / "functions"
    decompiles = []
    for path in sorted(decompile_dir.glob("*.json")):
        payload = read_json(path, {})
        decompiles.append({
            "artifact": str(path.relative_to(paths.root)),
            "function": payload.get("function") or payload.get("selector") or path.stem,
            "mode": payload.get("mode"),
            "ok": payload.get("ok"),
            "explanation": payload.get("explanation"),
            "report": str((reports_dir / f"{path.stem}.md").relative_to(paths.root)) if (reports_dir / f"{path.stem}.md").exists() else None,
        })
    return {
        "summary": f"Collected {len(decompiles)} function explanation artifact(s).",
        "function_explanations": decompiles,
        "recommended_next_steps": [
            "Review decompiler output for correctness before relying on conclusions.",
            "Prefer exact function addresses/names for repeatable analysis.",
        ],
    }


def run_generate_re_report(context: dict[str, Any]) -> dict[str, Any]:
    paths: CasePaths = context["paths"]
    report_path = paths.reports / "full.md"
    report = generate_full_report(paths)
    report_path.write_text(report, encoding="utf-8")
    return {
        "summary": "Generated full reverse-engineering report.",
        "report": str(report_path.relative_to(paths.root)),
        "recommended_next_steps": ["Review `reports/full.md` and redact sensitive extracted strings before sharing."],
    }


RUNNERS = {
    "binary.triage.unknown": run_triage_unknown,
    "binary.explain.function": run_explain_function,
    "binary.generate_re_report": run_generate_re_report,
}

ALIASES = {
    "binary.explain_function": "binary.explain.function",
    "binary.generate.re_report": "binary.generate_re_report",
    "triage": "binary.triage.unknown",
    "explain": "binary.explain.function",
    "report": "binary.generate_re_report",
}


def run_skill(out: Path, case_id: str, skill_id: str, bundle_path: Path = DEFAULT_BUNDLE) -> tuple[Path, dict[str, Any]]:
    paths = resolve_case(out, case_id)
    bundle = load_bundle(bundle_path)
    skill_id = ALIASES.get(skill_id, skill_id)
    if skill_id not in RUNNERS:
        available = sorted(RUNNERS)
        raise SkillError(f"unsupported skill: {skill_id}; available: {', '.join(available)}")
    context = load_case_artifacts(paths)
    context["paths"] = paths
    result = RUNNERS[skill_id](context)
    skill_meta = bundle.get("skills", {}).get(skill_id, {"id": skill_id, "title": skill_id})
    payload = {
        "schema_version": SKILL_OUTPUT_SCHEMA,
        "skill_id": skill_id,
        "skill_title": skill_meta.get("title"),
        "bundle": {"name": bundle.get("name"), "version": bundle.get("version"), "path": bundle.get("path")},
        "case_id": context["case"].get("case_id"),
        "created_at": utc_now(),
        "inputs": {
            "case": "case.json",
            "artifacts": sorted([p.name for p in paths.artifacts.glob("*.json")]),
            "findings": sorted([p.name for p in paths.findings.glob("*.json")]),
        },
        "result": result,
    }
    out_path = paths.root / "skills" / f"{slugify(skill_id)}.json"
    write_json(out_path, payload)
    return out_path, payload


def list_skills(bundle_path: Path = DEFAULT_BUNDLE) -> dict[str, Any]:
    bundle = load_bundle(bundle_path)
    return {
        "bundle": {"name": bundle.get("name"), "version": bundle.get("version"), "path": bundle.get("path")},
        "skills": sorted([{"id": sid, "title": meta.get("title"), "path": meta.get("path")} for sid, meta in bundle.get("skills", {}).items()], key=lambda x: x["id"]),
        "executable": sorted(RUNNERS),
    }
