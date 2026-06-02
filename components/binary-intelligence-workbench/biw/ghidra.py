from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path
from typing import Any

from .core import command_exists, write_json


class GhidraUnavailable(RuntimeError):
    pass


def default_wrapper() -> Path | None:
    env = os.environ.get("BIW_GHIDRA_WRAPPER")
    if env:
        p = Path(env).expanduser()
        return p if p.exists() else None

    here = Path(__file__).resolve()
    candidates = [
        # Source-tree layouts used by the imported Vegvisir component sources.
        here.parents[2] / "GhidraHeadlessMCP" / "bin" / "ghidra-headless",
        here.parents[3] / "GhidraHeadlessMCP" / "bin" / "ghidra-headless",
        here.parents[2] / "ghidra-headless-mcp" / "bin" / "ghidra-headless",
        # Vegvisir runtime tool layout.
        Path.home() / ".vegvisir" / "tools" / "bin" / "ghidra-headless",
    ]

    vegvisir_tools = os.environ.get("VEGVISIR_TOOLS")
    if vegvisir_tools:
        candidates.append(Path(vegvisir_tools).expanduser() / "bin" / "ghidra-headless")

    for candidate in candidates:
        if candidate.exists():
            return candidate
    return None


def ghidra_status(wrapper: Path | None = None, timeout: int = 30) -> dict[str, Any]:
    wrapper = wrapper or default_wrapper()
    if not wrapper:
        return {"available": False, "reason": "GhidraHeadlessMCP wrapper not found"}
    try:
        proc = subprocess.run([str(wrapper), "status"], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
        payload = json.loads(proc.stdout) if proc.stdout.strip().startswith("{") else {}
        return {"available": proc.returncode == 0 and bool(payload.get("ok")), "wrapper": str(wrapper), "payload": payload}
    except Exception as exc:
        return {"available": False, "wrapper": str(wrapper), "reason": str(exc)}


def run_ghidra_extract(binary: Path, case_root: Path, project_name: str, limit: int = 500, timeout: int = 600) -> dict[str, Any]:
    wrapper = default_wrapper()
    if not wrapper:
        raise GhidraUnavailable("GhidraHeadlessMCP wrapper not found")
    status = ghidra_status(wrapper)
    if not status.get("available"):
        raise GhidraUnavailable(status.get("reason") or "Ghidra status check failed")

    project_dir = case_root / "ghidra-project"
    program = binary.name
    job: dict[str, Any] = {"mode": "ghidra", "wrapper": str(wrapper), "project_dir": str(project_dir), "project_name": project_name, "program": program, "commands": []}

    def call(args: list[str], name: str) -> dict[str, Any]:
        cmd = [str(wrapper), *args, "--project-dir", str(project_dir), "--project-name", project_name]
        proc = subprocess.run(cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
        rec = {"name": name, "cmd": cmd, "returncode": proc.returncode, "stderr_tail": proc.stderr.splitlines()[-20:]}
        job["commands"].append(rec)
        try:
            payload = json.loads(proc.stdout)
        except Exception:
            payload = {"ok": False, "stdout_tail": proc.stdout.splitlines()[-40:]}
        if proc.returncode != 0:
            payload.setdefault("ok", False)
            payload["stderr_tail"] = proc.stderr.splitlines()[-20:]
        return payload

    imported = call(["import", "--binary", str(binary), "--overwrite"], "import")
    if not imported.get("ok", imported.get("returncode") == 0):
        write_json(case_root / "jobs" / "ghidra-triage.json", job)
        raise RuntimeError(f"Ghidra import failed: {imported}")

    results = {
        "summary": imported,
        "strings": call(["list-strings", "--program", program, "--limit", str(limit)], "list-strings"),
        "imports": call(["list-imports", "--program", program, "--limit", str(limit)], "list-imports"),
        "exports": call(["list-exports", "--program", program, "--limit", str(limit)], "list-exports"),
        "functions": call(["list-functions", "--program", program, "--limit", str(limit)], "list-functions"),
        "callgraph": call(["callgraph", "--program", program, "--mode", "edges", "--limit", str(limit)], "callgraph"),
    }
    write_json(case_root / "jobs" / "ghidra-triage.json", job)
    return results
