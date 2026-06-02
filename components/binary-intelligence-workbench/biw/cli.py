from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path
from typing import Any

from .core import (
    CasePaths, SCHEMA_CALLGRAPH, SCHEMA_CASE, SCHEMA_EXPORTS, SCHEMA_FUNCTIONS,
    SCHEMA_HASHES, SCHEMA_IMPORTS, SCHEMA_METADATA, SCHEMA_STRINGS, detect_format,
    extract_ascii_strings, make_case_id, parse_nm_symbols, read_json, sha256_file,
    utc_now, write_json,
)
from .ghidra import GhidraUnavailable, ghidra_status, run_ghidra_extract
from .index import build_case_detail, build_case_index, write_case_index
from .server import serve
from .heuristics import build_findings
from .report import generate_full_report, generate_summary
from .explain import explain_function
from .skill import SkillError, list_skills, run_skill
from .diff import build_binary_diff
from .firmware import triage_firmware
from .memory import build_memory_summary, write_memory_summary
from .agent import create_agent_task, list_agents


def normalize_ghidra(kind: str, payload: dict[str, Any]) -> dict[str, Any]:
    if kind == "strings":
        items = payload.get("strings") or payload.get("items") or payload.get("results") or []
        return {"schema_version": SCHEMA_STRINGS, "strings": items, "source": "ghidra"}
    if kind == "imports":
        items = payload.get("imports") or payload.get("items") or payload.get("symbols") or []
        return {"schema_version": SCHEMA_IMPORTS, "imports": items, "source": "ghidra"}
    if kind == "exports":
        items = payload.get("exports") or payload.get("items") or payload.get("symbols") or []
        return {"schema_version": SCHEMA_EXPORTS, "exports": items, "source": "ghidra"}
    if kind == "functions":
        items = payload.get("functions") or payload.get("items") or []
        return {"schema_version": SCHEMA_FUNCTIONS, "functions": items, "source": "ghidra"}
    if kind == "callgraph":
        edges = payload.get("edges") or payload.get("calls") or payload.get("items") or []
        return {"schema_version": SCHEMA_CALLGRAPH, "edges": edges, "source": "ghidra"}
    return payload


def basic_extract(binary: Path, limit: int) -> dict[str, dict[str, Any]]:
    imports, exports, functions = parse_nm_symbols(binary)
    return {
        "strings": {"schema_version": SCHEMA_STRINGS, "strings": extract_ascii_strings(binary, limit=limit), "source": "basic"},
        "imports": {"schema_version": SCHEMA_IMPORTS, "imports": imports, "source": "basic-nm" if imports else "basic"},
        "exports": {"schema_version": SCHEMA_EXPORTS, "exports": exports, "source": "basic-nm" if exports else "basic"},
        "functions": {"schema_version": SCHEMA_FUNCTIONS, "functions": functions, "source": "basic-nm" if functions else "basic"},
        "callgraph": {"schema_version": SCHEMA_CALLGRAPH, "edges": [], "source": "basic"},
    }


def triage(args: argparse.Namespace) -> int:
    binary = Path(args.binary).expanduser().resolve()
    if not binary.exists() or not binary.is_file():
        print(f"error: binary not found or not a file: {binary}", file=sys.stderr)
        return 2
    out = Path(args.out).expanduser().resolve()
    sha = sha256_file(binary)
    case_id = args.case_id or make_case_id(binary, sha)
    paths = CasePaths(out / case_id)
    if paths.root.exists() and not args.overwrite:
        print(f"error: case already exists: {paths.root} (use --overwrite)", file=sys.stderr)
        return 2
    paths.ensure()

    sample = binary.read_bytes()[:4096]
    fmt = detect_format(binary, sample)
    now = utc_now()
    case = {
        "schema_version": SCHEMA_CASE,
        "case_id": case_id,
        "title": binary.name,
        "created_at": now,
        "updated_at": now,
        "source": {
            "path": str(binary), "filename": binary.name, "size_bytes": binary.stat().st_size,
            "sha256": sha, "mime": fmt.get("mime"), "format": fmt.get("format"), "architecture": fmt.get("architecture"),
        },
        "status": "triaging",
        "tags": [t for t in [fmt.get("format"), fmt.get("architecture")] if t],
        "risk": {"level": "unknown", "score": None, "rationale": []},
        "extraction": {"mode": "unknown"},
    }
    write_json(paths.root / "case.json", case)
    write_json(paths.root / "artifacts" / "hashes.json", {"schema_version": SCHEMA_HASHES, "sha256": sha})
    write_json(paths.root / "artifacts" / "metadata.json", {"schema_version": SCHEMA_METADATA, **case["source"]})
    write_json(paths.root / "source" / "original-ref.json", {"path": str(binary), "sha256": sha, "copied": False})

    artifacts: dict[str, dict[str, Any]]
    mode = "basic"
    ghidra_error = None
    if not args.no_ghidra:
        try:
            ghidra = run_ghidra_extract(binary, paths.root, case_id, limit=args.limit, timeout=args.timeout)
            artifacts = {k: normalize_ghidra(k, ghidra.get(k, {})) for k in ["strings", "imports", "exports", "functions", "callgraph"]}
            mode = "ghidra"
        except Exception as exc:
            ghidra_error = str(exc)
            artifacts = basic_extract(binary, args.limit)
            mode = "basic-fallback"
    else:
        artifacts = basic_extract(binary, args.limit)
        mode = "basic"

    for name, artifact in artifacts.items():
        write_json(paths.artifacts / f"{name}.json", artifact)

    suspicious, capability, findings_bundle = build_findings(artifacts["strings"], artifacts["imports"])
    write_json(paths.findings / "suspicious-strings.json", suspicious)
    write_json(paths.findings / "capability-map.json", capability)
    write_json(paths.findings / "findings.json", findings_bundle)

    case["status"] = "triaged"
    case["updated_at"] = utc_now()
    case["risk"] = findings_bundle.get("risk", case["risk"])
    case["extraction"] = {"mode": mode, "ghidra_error": ghidra_error}
    write_json(paths.root / "case.json", case)

    summary = generate_summary(case, artifacts, findings_bundle)
    (paths.reports / "summary.md").write_text(summary, encoding="utf-8")
    (paths.notes / "notes.md").write_text(f"# Notes — {case_id}\n\n", encoding="utf-8")

    print(f"Created case: {paths.root}")
    print(f"Extraction mode: {mode}")
    if ghidra_error:
        print(f"Ghidra unavailable/error; used basic fallback: {ghidra_error}")
    print("Generated: case.json, artifacts/*.json, findings/*.json, reports/summary.md")
    return 0


def case_list(args: argparse.Namespace) -> int:
    out = Path(args.out).expanduser().resolve()
    cases = []
    for p in sorted(out.glob("*/case.json")):
        data = read_json(p, {})
        cases.append({"case_id": data.get("case_id"), "title": data.get("title"), "status": data.get("status"), "risk": data.get("risk", {}).get("level"), "path": str(p.parent)})
    print(json.dumps(cases, indent=2, sort_keys=True))
    return 0


def case_show(args: argparse.Namespace) -> int:
    out = Path(args.out).expanduser().resolve()
    case_path = out / args.case_id / "case.json"
    if not case_path.exists():
        matches = list(out.glob(f"{args.case_id}*/case.json"))
        case_path = matches[0] if matches else case_path
    if not case_path.exists():
        print(f"error: case not found: {args.case_id}", file=sys.stderr)
        return 2
    print(case_path.read_text(encoding="utf-8"))
    return 0



def case_index_cmd(args: argparse.Namespace) -> int:
    out = Path(args.out).expanduser().resolve()
    try:
        if args.write:
            dest = Path(args.dest).expanduser().resolve() if args.dest else None
            written = write_case_index(out, dest)
            print(f"Wrote case index: {written}")
        else:
            print(json.dumps(build_case_index(out), indent=2, sort_keys=True))
    except Exception as exc:
        print(f"error: failed to build case index: {exc}", file=sys.stderr)
        return 1
    return 0


def case_export_cmd(args: argparse.Namespace) -> int:
    out = Path(args.out).expanduser().resolve()
    try:
        payload = build_case_detail(out, args.case_id, include_artifacts=args.include_artifacts)
        if args.write:
            case_root = out / payload["summary"]["case_id"]
            if not (case_root / "case.json").exists():
                # Prefix resolution may have found a longer case id; use path from summary.
                case_root = out / payload["summary"]["paths"]["case"]
            dest = Path(args.dest).expanduser().resolve() if args.dest else case_root / "case-export.json"
            write_json(dest, payload)
            print(f"Wrote case export: {dest}")
        else:
            print(json.dumps(payload, indent=2, sort_keys=True))
    except FileNotFoundError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    except Exception as exc:
        print(f"error: failed to export case: {exc}", file=sys.stderr)
        return 1
    return 0


def serve_cmd(args: argparse.Namespace) -> int:
    try:
        serve(args.host, args.port, Path(args.out).expanduser().resolve(), quiet=args.quiet)
    except KeyboardInterrupt:
        print("\nBIW API stopped")
    except Exception as exc:
        print(f"error: server failed: {exc}", file=sys.stderr)
        return 1
    return 0

def report_cmd(args: argparse.Namespace) -> int:
    if not getattr(args, "case_id", None):
        print("error: report requires a case id or subcommand", file=sys.stderr)
        return 2
    out = Path(args.out).expanduser().resolve()
    report = out / args.case_id / "reports" / "summary.md"
    if not report.exists():
        matches = list(out.glob(f"{args.case_id}*/reports/summary.md"))
        report = matches[0] if matches else report
    if not report.exists():
        print(f"error: summary report not found for case: {args.case_id}", file=sys.stderr)
        return 2
    print(report.read_text(encoding="utf-8"))
    return 0



def report_full_cmd(args: argparse.Namespace) -> int:
    out = Path(args.out).expanduser().resolve()
    root = out / args.case_id
    if not (root / "case.json").exists():
        matches = list(out.glob(f"{args.case_id}*/case.json"))
        root = matches[0].parent if matches else root
    if not (root / "case.json").exists():
        print(f"error: case not found: {args.case_id}", file=sys.stderr)
        return 2
    paths = CasePaths(root)
    report = generate_full_report(paths)
    report_path = paths.reports / "full.md"
    if args.write:
        report_path.parent.mkdir(parents=True, exist_ok=True)
        report_path.write_text(report, encoding="utf-8")
        print(f"Wrote full report: {report_path}")
    else:
        print(report)
    return 0


def skill_list_cmd(args: argparse.Namespace) -> int:
    try:
        bundle = list_skills(Path(args.bundle).expanduser().resolve() if args.bundle else None) if args.bundle else list_skills()
    except Exception as exc:
        print(f"error: failed to list skills: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(bundle, indent=2, sort_keys=True))
    return 0


def skill_run_cmd(args: argparse.Namespace) -> int:
    out = Path(args.out).expanduser().resolve()
    bundle = Path(args.bundle).expanduser().resolve() if args.bundle else None
    try:
        output_path, payload = run_skill(out, args.case_id, args.skill_id, bundle) if bundle else run_skill(out, args.case_id, args.skill_id)
    except FileNotFoundError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    except SkillError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    except Exception as exc:
        print(f"error: skill run failed: {exc}", file=sys.stderr)
        return 1
    print(f"Created skill output: {output_path}")
    print(f"Skill: {payload.get('skill_id')}")
    print(f"Summary: {(payload.get('result') or {}).get('summary')}")
    return 0


def diff_cmd(args: argparse.Namespace) -> int:
    out = Path(args.out).expanduser().resolve()
    try:
        root, payload = build_binary_diff(
            Path(args.old_binary), Path(args.new_binary), out,
            case_id=args.case_id, limit=args.limit, overwrite=args.overwrite,
        )
    except FileNotFoundError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    except FileExistsError as exc:
        print(f"error: {exc} (use --overwrite)", file=sys.stderr)
        return 2
    except Exception as exc:
        print(f"error: diff failed: {exc}", file=sys.stderr)
        return 1
    print(f"Created diff case: {root}")
    print(f"Added strings: {payload['deltas']['strings']['added_count']} removed strings: {payload['deltas']['strings']['removed_count']}")
    print("Generated: artifacts/diff.json, reports/summary.md, reports/full.md")
    return 0


def firmware_triage_cmd(args: argparse.Namespace) -> int:
    out = Path(args.out).expanduser().resolve()
    try:
        root, payload = triage_firmware(Path(args.firmware), out, case_id=args.case_id, limit=args.limit, overwrite=args.overwrite)
    except FileNotFoundError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    except FileExistsError as exc:
        print(f"error: {exc} (use --overwrite)", file=sys.stderr)
        return 2
    except Exception as exc:
        print(f"error: firmware triage failed: {exc}", file=sys.stderr)
        return 1
    print(f"Created firmware case: {root}")
    print(f"Embedded candidates: {len(payload.get('embedded_candidates') or [])}")
    print("Generated: artifacts/firmware-inventory.json, findings/*.json, reports/summary.md")
    return 0


def memory_export_cmd(args: argparse.Namespace) -> int:
    out = Path(args.out).expanduser().resolve()
    root = out / args.case_id
    if not (root / "case.json").exists():
        matches = list(out.glob(f"{args.case_id}*/case.json"))
        root = matches[0].parent if matches else root
    if not (root / "case.json").exists():
        print(f"error: case not found: {args.case_id}", file=sys.stderr)
        return 2
    try:
        if args.write:
            dest = write_memory_summary(root)
            print(f"Wrote CMS-safe memory summary: {dest}")
        else:
            print(json.dumps(build_memory_summary(root), indent=2, sort_keys=True))
    except Exception as exc:
        print(f"error: memory export failed: {exc}", file=sys.stderr)
        return 1
    return 0


def agent_list_cmd(args: argparse.Namespace) -> int:
    print(json.dumps(list_agents(), indent=2, sort_keys=True))
    return 0


def agent_plan_cmd(args: argparse.Namespace) -> int:
    out = Path(args.out).expanduser().resolve()
    try:
        dest, payload = create_agent_task(out, args.case_id, args.agent_id, args.goal)
    except FileNotFoundError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    except Exception as exc:
        print(f"error: agent task failed: {exc}", file=sys.stderr)
        return 1
    print(f"Created agent task: {dest}")
    print(f"Task: {payload.get('task_id')} status={payload.get('status')}")
    return 0

def status_cmd(args: argparse.Namespace) -> int:
    print(json.dumps({"ghidra": ghidra_status()}, indent=2, sort_keys=True))
    return 0


def explain_function_cmd(args: argparse.Namespace) -> int:
    out = Path(args.out).expanduser().resolve()
    try:
        decompile_path, report_path, artifact = explain_function(
            out=out,
            case_id=args.case_id,
            selector=args.function,
            timeout=args.timeout,
            max_chars=args.max_chars,
            no_ghidra=args.no_ghidra,
        )
    except FileNotFoundError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    except Exception as exc:
        print(f"error: explain failed: {exc}", file=sys.stderr)
        return 1
    print(f"Created decompile artifact: {decompile_path}")
    print(f"Created function report: {report_path}")
    print(f"Mode: {artifact.get('mode')} ok={artifact.get('ok')}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(prog="biw", description="Binary Intelligence Workbench CLI")
    sub = ap.add_subparsers(dest="command", required=True)
    p = sub.add_parser("status", help="show integration status")
    p.set_defaults(func=status_cmd)
    p = sub.add_parser("triage", help="triage a binary into a BIW case")
    p.add_argument("binary")
    p.add_argument("--out", default=".analysis/cases")
    p.add_argument("--case-id")
    p.add_argument("--limit", type=int, default=1000)
    p.add_argument("--timeout", type=int, default=600)
    p.add_argument("--no-ghidra", action="store_true", help="skip Ghidra and use deterministic basic extraction")
    p.add_argument("--overwrite", action="store_true")
    p.set_defaults(func=triage)
    p = sub.add_parser("case", help="case operations")
    csub = p.add_subparsers(dest="case_command", required=True)
    cp = csub.add_parser("list")
    cp.add_argument("--out", default=".analysis/cases")
    cp.set_defaults(func=case_list)
    cp = csub.add_parser("show")
    cp.add_argument("case_id")
    cp.add_argument("--out", default=".analysis/cases")
    cp.set_defaults(func=case_show)
    cp = csub.add_parser("index", help="print or write a Solarium/API-friendly case index")
    cp.add_argument("--out", default=".analysis/cases")
    cp.add_argument("--write", action="store_true", help="write index.json instead of printing JSON")
    cp.add_argument("--dest", help="optional destination path when using --write")
    cp.set_defaults(func=case_index_cmd)
    cp = csub.add_parser("export", help="print or write a detailed case JSON export")
    cp.add_argument("case_id")
    cp.add_argument("--out", default=".analysis/cases")
    cp.add_argument("--include-artifacts", action="store_true", help="include large raw artifact JSON payloads")
    cp.add_argument("--write", action="store_true", help="write case-export.json instead of printing JSON")
    cp.add_argument("--dest", help="optional destination path when using --write")
    cp.set_defaults(func=case_export_cmd)
    p = sub.add_parser("report", help="print or generate case reports")
    rsub = p.add_subparsers(dest="report_command")
    rp = rsub.add_parser("summary", help="print a case summary report")
    rp.add_argument("case_id")
    rp.add_argument("--out", default=".analysis/cases")
    rp.set_defaults(func=report_cmd)
    rp = rsub.add_parser("full", help="print or write an aggregate full case report")
    rp.add_argument("case_id")
    rp.add_argument("--out", default=".analysis/cases")
    rp.add_argument("--write", action="store_true", help="write reports/full.md instead of printing to stdout")
    rp.set_defaults(func=report_full_cmd)

    p = sub.add_parser("skill", help="list and run local BIW Skiller workflows")
    ssub = p.add_subparsers(dest="skill_command", required=True)
    sp = ssub.add_parser("list", help="list local binary-analysis skills")
    sp.add_argument("--bundle", help="override bundle path")
    sp.set_defaults(func=skill_list_cmd)
    sp = ssub.add_parser("run", help="run a deterministic skill workflow against a case")
    sp.add_argument("case_id")
    sp.add_argument("skill_id")
    sp.add_argument("--out", default=".analysis/cases")
    sp.add_argument("--bundle", help="override bundle path")
    sp.set_defaults(func=skill_run_cmd)


    p = sub.add_parser("diff", help="compare two binaries and create a BIW diff case")
    p.add_argument("old_binary")
    p.add_argument("new_binary")
    p.add_argument("--out", default=".analysis/cases")
    p.add_argument("--case-id")
    p.add_argument("--limit", type=int, default=1000)
    p.add_argument("--overwrite", action="store_true")
    p.set_defaults(func=diff_cmd)

    p = sub.add_parser("firmware", help="firmware analysis workflows")
    fsub = p.add_subparsers(dest="firmware_command", required=True)
    fp = fsub.add_parser("triage", help="triage a firmware/blob image into a BIW case")
    fp.add_argument("firmware")
    fp.add_argument("--out", default=".analysis/cases")
    fp.add_argument("--case-id")
    fp.add_argument("--limit", type=int, default=2000)
    fp.add_argument("--overwrite", action="store_true")
    fp.set_defaults(func=firmware_triage_cmd)

    p = sub.add_parser("memory", help="CMS-safe memory integration helpers")
    msub = p.add_subparsers(dest="memory_command", required=True)
    mp = msub.add_parser("export", help="print or write a redacted CMS-safe case memory summary")
    mp.add_argument("case_id")
    mp.add_argument("--out", default=".analysis/cases")
    mp.add_argument("--write", action="store_true")
    mp.set_defaults(func=memory_export_cmd)

    p = sub.add_parser("agent", help="BIW agent task planning artifacts")
    asub = p.add_subparsers(dest="agent_command", required=True)
    ap2 = asub.add_parser("list", help="list available BIW agent profiles")
    ap2.set_defaults(func=agent_list_cmd)
    ap2 = asub.add_parser("plan", help="create a bounded agent task artifact for a case")
    ap2.add_argument("case_id")
    ap2.add_argument("agent_id")
    ap2.add_argument("goal")
    ap2.add_argument("--out", default=".analysis/cases")
    ap2.set_defaults(func=agent_plan_cmd)

    p = sub.add_parser("serve", help="serve a read-only local JSON API for Solarium/workbench UI")
    p.add_argument("--out", default=".analysis/cases", help="case root to serve")
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=8765)
    p.add_argument("--quiet", action="store_true", help="suppress per-request HTTP logs")
    p.set_defaults(func=serve_cmd)

    p = sub.add_parser("explain", help="explain case artifacts such as functions")
    esub = p.add_subparsers(dest="explain_command", required=True)
    ep = esub.add_parser("function", help="decompile/explain one function from a case")
    ep.add_argument("case_id")
    ep.add_argument("function", help="function name, id, address, or substring")
    ep.add_argument("--out", default=".analysis/cases")
    ep.add_argument("--timeout", type=int, default=300)
    ep.add_argument("--max-chars", type=int, default=20000)
    ep.add_argument("--no-ghidra", action="store_true", help="create a metadata-only function report without invoking Ghidra")
    ep.set_defaults(func=explain_function_cmd)
    return ap


def main(argv: list[str] | None = None) -> int:
    if argv is None:
        argv = sys.argv[1:]
    else:
        argv = list(argv)
    # Backward compatibility: `biw report <case-id>` means `biw report summary <case-id>`.
    if len(argv) >= 2 and argv[0] == "report" and argv[1] not in {"summary", "full", "-h", "--help"} and not argv[1].startswith("-"):
        argv.insert(1, "summary")
    args = build_parser().parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
