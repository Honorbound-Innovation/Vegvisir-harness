from __future__ import annotations

from pathlib import Path
from typing import Any

from .core import CasePaths, read_json, utc_now, write_json

SCHEMA_AGENT_TASK = "biw.agent-task.v1"

AGENTS = {
    "binary-triage-agent": "Review case artifacts, findings, and capabilities; produce evidence-backed triage recommendations.",
    "function-analysis-agent": "Review decompile/function reports; identify behavior, dependencies, and follow-up functions.",
    "vuln-hunting-agent": "Look for memory-unsafe patterns, risky APIs, parsing surfaces, and patch-review targets.",
    "firmware-analysis-agent": "Review firmware inventory, embedded candidates, paths, scripts, and configuration indicators.",
    "report-writer-agent": "Synthesize reviewed findings and notes into an analyst-facing report.",
}


def list_agents() -> dict[str, Any]:
    return {"schema_version": "biw.agent-registry.v1", "agents": [{"id": k, "description": v} for k, v in sorted(AGENTS.items())]}


def create_agent_task(out: Path, case_id: str, agent_id: str, goal: str) -> tuple[Path, dict[str, Any]]:
    if agent_id not in AGENTS:
        raise ValueError(f"unknown BIW agent: {agent_id}")
    root = out / case_id
    if not (root / "case.json").exists():
        matches = list(out.glob(f"{case_id}*/case.json"))
        root = matches[0].parent if matches else root
    if not (root / "case.json").exists():
        raise FileNotFoundError(f"case not found: {case_id}")
    paths = CasePaths(root)
    task_id = f"{agent_id}-{utc_now().replace(':', '').replace('-', '').replace('Z', 'Z')}"
    case = read_json(paths.root / "case.json", {})
    findings = read_json(paths.findings / "findings.json", {})
    payload = {
        "schema_version": SCHEMA_AGENT_TASK,
        "task_id": task_id,
        "agent_id": agent_id,
        "created_at": utc_now(),
        "status": "planned",
        "goal": goal,
        "description": AGENTS[agent_id],
        "case_id": case.get("case_id") or paths.root.name,
        "inputs": {
            "case_json": "case.json",
            "summary_report": "reports/summary.md",
            "findings": "findings/findings.json",
            "skill_outputs_dir": "skills/",
            "function_reports_dir": "reports/functions/",
        },
        "context_summary": {
            "title": case.get("title"),
            "risk": case.get("risk"),
            "finding_count": len(findings.get("findings") or []),
        },
        "outputs_expected": ["evidence-backed observations", "recommended next steps", "artifact references", "uncertainties"],
        "note": "This MVP records an agent task artifact. Execute with Vegvisir subagents or future BIW job runner.",
    }
    dest = paths.jobs / "agents" / f"{task_id}.json"
    write_json(dest, payload)
    return dest, payload
