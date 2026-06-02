from __future__ import annotations

import hashlib
import json
import subprocess
from pathlib import Path
from typing import Any

from .core import read_json, utc_now, write_json
from .ghidra import default_wrapper, ghidra_status

SCHEMA_DECOMPILE = "biw.decompile.v1"
SCHEMA_FUNCTION_EXPLANATION = "biw.function_explanation.v1"


def find_case_root(out: Path, case_id: str) -> Path | None:
    exact = out / case_id
    if (exact / "case.json").exists():
        return exact
    matches = sorted(out.glob(f"{case_id}*/case.json"))
    return matches[0].parent if matches else None


def load_case(out: Path, case_id: str) -> tuple[Path, dict[str, Any]]:
    root = find_case_root(out, case_id)
    if not root:
        raise FileNotFoundError(f"case not found: {case_id}")
    return root, read_json(root / "case.json", {})


def _function_candidates(functions_artifact: dict[str, Any]) -> list[dict[str, Any]]:
    items = functions_artifact.get("functions") or functions_artifact.get("items") or functions_artifact.get("results") or []
    return items if isinstance(items, list) else []


def resolve_function(case_root: Path, selector: str) -> dict[str, Any] | None:
    functions = _function_candidates(read_json(case_root / "artifacts" / "functions.json", {}))
    needle = selector.lower()
    for fn in functions:
        values = [
            str(fn.get("id") or ""),
            str(fn.get("name") or ""),
            str(fn.get("address") or ""),
            str(fn.get("entry") or ""),
            str(fn.get("entry_point") or ""),
        ]
        if any(v.lower() == needle for v in values):
            return fn
    # If no exact match exists, prefer prefix matches before broad substring
    # matches. This avoids surprising choices such as selector `main` matching
    # unrelated imported names when a real `main` symbol is absent.
    for fn in functions:
        name = str(fn.get("name") or "").lower()
        if name and name.startswith(needle):
            return fn
    for fn in functions:
        name = str(fn.get("name") or "").lower()
        if name and needle in name:
            return fn
    return None


def function_label(fn: dict[str, Any] | None, selector: str) -> str:
    if not fn:
        return selector
    return str(fn.get("name") or fn.get("id") or fn.get("address") or selector)


def artifact_slug(value: str) -> str:
    raw = value.encode("utf-8", errors="replace")
    digest = hashlib.sha1(raw).hexdigest()[:10]
    safe = "".join(ch if ch.isalnum() or ch in "._-" else "-" for ch in value).strip("-._")[:60]
    return f"{safe or 'function'}-{digest}"


def _ghidra_common(case_root: Path, case: dict[str, Any]) -> tuple[Path, Path, str, str]:
    wrapper = default_wrapper()
    if not wrapper:
        raise RuntimeError("GhidraHeadlessMCP wrapper not found")
    status = ghidra_status(wrapper)
    if not status.get("available"):
        raise RuntimeError(status.get("reason") or "Ghidra status check failed")
    project_dir = case_root / "ghidra-project"
    project_name = str(case.get("case_id") or case_root.name)
    program = str(case.get("source", {}).get("filename") or case.get("title") or "")
    if not program:
        raise RuntimeError("case is missing source filename/program name")
    return wrapper, project_dir, project_name, program


def run_ghidra_function_info(case_root: Path, case: dict[str, Any], fn: dict[str, Any] | None, selector: str, timeout: int = 300) -> dict[str, Any]:
    wrapper, project_dir, project_name, program = _ghidra_common(case_root, case)
    args = [str(wrapper), "function-info", "--project-dir", str(project_dir), "--project-name", project_name, "--program", program]
    address = (fn or {}).get("address") or selector if str(selector).lower().startswith("0x") else (fn or {}).get("address")
    name = (fn or {}).get("name") or selector
    if address:
        args += ["--address", str(address)]
    else:
        args += ["--function", str(name)]
    proc = subprocess.run(args, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
    try:
        payload = json.loads(proc.stdout)
    except Exception:
        payload = {"ok": False, "stdout_tail": proc.stdout.splitlines()[-40:]}
    payload.setdefault("command", args)
    payload.setdefault("returncode", proc.returncode)
    if proc.stderr:
        payload.setdefault("stderr_tail", proc.stderr.splitlines()[-20:])
    return payload


def run_ghidra_decompile(case_root: Path, case: dict[str, Any], fn: dict[str, Any] | None, selector: str, timeout: int = 300, max_chars: int = 20000) -> dict[str, Any]:
    wrapper, project_dir, project_name, program = _ghidra_common(case_root, case)
    args = [str(wrapper), "decompile", "--project-dir", str(project_dir), "--project-name", project_name, "--program", program, "--timeout-seconds", str(timeout), "--max-chars", str(max_chars)]
    address = (fn or {}).get("address") or selector if str(selector).lower().startswith("0x") else (fn or {}).get("address")
    name = (fn or {}).get("name") or selector
    if address:
        args += ["--address", str(address)]
    else:
        args += ["--function", str(name)]
    proc = subprocess.run(args, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout + 30)
    try:
        payload = json.loads(proc.stdout)
    except Exception:
        payload = {"ok": False, "stdout_tail": proc.stdout.splitlines()[-40:]}
    payload.setdefault("command", args)
    payload.setdefault("returncode", proc.returncode)
    if proc.stderr:
        payload.setdefault("stderr_tail", proc.stderr.splitlines()[-20:])
    return payload


def extract_decompiled_text(payload: dict[str, Any]) -> str:
    for key in ("decompiled", "decompiled_code", "code", "c", "text", "body"):
        val = payload.get(key)
        if isinstance(val, str) and val.strip():
            return val
    result = payload.get("result")
    if isinstance(result, dict):
        for key in ("decompiled", "decompiled_code", "code", "c", "text", "body"):
            val = result.get(key)
            if isinstance(val, str) and val.strip():
                return val
    return ""


def build_function_explanation(case: dict[str, Any], fn: dict[str, Any] | None, selector: str, decompile_artifact: dict[str, Any], info_artifact: dict[str, Any]) -> dict[str, Any]:
    code = extract_decompiled_text(decompile_artifact)
    label = function_label(fn, selector)
    lines = [line.rstrip() for line in code.splitlines() if line.strip()]
    calls: list[str] = []
    suspicious_terms: list[str] = []
    for line in lines:
        for term in ("system", "exec", "popen", "socket", "connect", "send", "recv", "open", "read", "write", "strcpy", "memcpy", "malloc", "free"):
            if term in line and term not in suspicious_terms:
                suspicious_terms.append(term)
        if "(" in line and ")" in line:
            token = line.split("(", 1)[0].strip().split()[-1] if line.split("(", 1)[0].strip().split() else ""
            if token and token.isidentifier() and token not in calls and token not in {"if", "for", "while", "switch", "return"}:
                calls.append(token)
    summary = "Decompiled code was captured for review." if code else "No decompiled code was available; explanation is based on function metadata only."
    if suspicious_terms:
        summary += " Notable API/term indicators appear in the function body: " + ", ".join(suspicious_terms[:12]) + "."
    return {
        "schema_version": SCHEMA_FUNCTION_EXPLANATION,
        "case_id": case.get("case_id"),
        "function_selector": selector,
        "function": fn or {"selector": selector},
        "generated_at": utc_now(),
        "summary": summary,
        "observed_calls_or_tokens": calls[:50],
        "notable_terms": suspicious_terms,
        "confidence": "medium" if code else "low",
        "limitations": [
            "This MVP explanation is deterministic and evidence-oriented; it is not a full semantic proof.",
            "Decompiler output may be incomplete or misleading for optimized/obfuscated binaries.",
        ],
    }


def render_function_report(case: dict[str, Any], fn: dict[str, Any] | None, selector: str, decompile_artifact: dict[str, Any], info_artifact: dict[str, Any], explanation: dict[str, Any]) -> str:
    label = function_label(fn, selector)
    code = extract_decompiled_text(decompile_artifact)
    lines = [
        f"# Function Analysis — {label}",
        "",
        "## Case",
        "",
        f"- Case ID: `{case.get('case_id')}`",
        f"- Binary: `{case.get('title')}`",
        f"- Selector: `{selector}`",
        f"- Function: `{label}`",
        f"- Address: `{(fn or {}).get('address') or (fn or {}).get('entry') or 'unknown'}`",
        "",
        "## Summary",
        "",
        explanation.get("summary", "No summary generated."),
        "",
        "## Notable Terms",
        "",
    ]
    terms = explanation.get("notable_terms") or []
    lines += [f"- `{t}`" for t in terms] if terms else ["No MVP notable terms detected in the available decompiler text."]
    lines += ["", "## Function Metadata", "", "```json", json.dumps(fn or {"selector": selector}, indent=2, sort_keys=True), "```", "", "## Ghidra Function Info", "", "```json", json.dumps(info_artifact, indent=2, sort_keys=True)[:12000], "```", ""]
    if code:
        lines += ["## Decompiled Code", "", "```c", code, "```", ""]
    else:
        lines += ["## Decompiled Code", "", "No decompiled code was available. If this was a fallback/basic case, rerun triage with Ghidra enabled.", ""]
    lines += ["## Safety / Review Notes", "", "- Treat this as static reverse-engineering assistance, not a behavioral guarantee.", "- Verify important conclusions against disassembly, xrefs, and runtime-safe sandbox evidence.", ""]
    return "\n".join(lines)


def explain_function(out: Path, case_id: str, selector: str, timeout: int = 300, max_chars: int = 20000, no_ghidra: bool = False) -> tuple[Path, Path, dict[str, Any]]:
    case_root, case = load_case(out, case_id)
    fn = resolve_function(case_root, selector)
    slug = artifact_slug(function_label(fn, selector))
    decompile_dir = case_root / "artifacts" / "decompile"
    report_dir = case_root / "reports" / "functions"
    decompile_dir.mkdir(parents=True, exist_ok=True)
    report_dir.mkdir(parents=True, exist_ok=True)

    info_artifact: dict[str, Any] = {"ok": False, "reason": "not-run"}
    if no_ghidra:
        decompile_artifact = {"schema_version": SCHEMA_DECOMPILE, "ok": False, "mode": "metadata-only", "reason": "--no-ghidra requested", "function": fn or {"selector": selector}, "created_at": utc_now()}
    else:
        try:
            info_artifact = run_ghidra_function_info(case_root, case, fn, selector, timeout=timeout)
            raw = run_ghidra_decompile(case_root, case, fn, selector, timeout=timeout, max_chars=max_chars)
            decompile_artifact = {"schema_version": SCHEMA_DECOMPILE, "ok": bool(raw.get("ok", raw.get("returncode") == 0)), "mode": "ghidra", "function": fn or {"selector": selector}, "created_at": utc_now(), "payload": raw}
        except Exception as exc:
            decompile_artifact = {"schema_version": SCHEMA_DECOMPILE, "ok": False, "mode": "metadata-only-fallback", "reason": str(exc), "function": fn or {"selector": selector}, "created_at": utc_now()}
    explanation = build_function_explanation(case, fn, selector, decompile_artifact.get("payload", decompile_artifact), info_artifact)
    decompile_artifact["explanation"] = explanation
    decompile_path = decompile_dir / f"{slug}.json"
    report_path = report_dir / f"{slug}.md"
    write_json(decompile_path, decompile_artifact)
    report_path.write_text(render_function_report(case, fn, selector, decompile_artifact.get("payload", decompile_artifact), info_artifact, explanation), encoding="utf-8")
    return decompile_path, report_path, decompile_artifact
