#!/usr/bin/env python3
"""MCP bridge for the Vegvisir Ghidra headless CLI MVP."""
from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
from typing import Any, Optional

from mcp.server.fastmcp import FastMCP

ROOT = pathlib.Path(__file__).resolve().parent
CLI = ROOT / "bin" / "ghidra-headless"

mcp = FastMCP("ghidra-headless-mcp")


def run_cli(args: list[str], timeout: int = 900) -> dict[str, Any]:
    proc = subprocess.run([str(CLI), *args], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
    try:
        payload = json.loads(proc.stdout)
    except Exception:
        payload = {
            "ok": False,
            "error": "CLI did not return JSON",
            "stdout": proc.stdout[-4000:],
        }
    payload.setdefault("ok", proc.returncode == 0)
    payload["returncode"] = proc.returncode
    if proc.stderr.strip():
        payload["stderr_tail"] = proc.stderr.strip().splitlines()[-20:]
    return payload


@mcp.tool()
def ghidra_status() -> dict[str, Any]:
    """Check headless Ghidra launcher availability."""
    return run_cli(["status"], timeout=90)


@mcp.tool()
def ghidra_import_binary(
    binary: str,
    project_dir: str,
    project_name: str,
    overwrite: bool = False,
    no_analysis: bool = False,
    analysis_timeout: int = 120,
) -> dict[str, Any]:
    """Import a binary into a Ghidra project and optionally run auto-analysis."""
    args = [
        "import",
        "--binary", binary,
        "--project-dir", project_dir,
        "--project-name", project_name,
        "--analysis-timeout", str(analysis_timeout),
    ]
    if overwrite:
        args.append("--overwrite")
    if no_analysis:
        args.append("--no-analysis")
    return run_cli(args, timeout=max(300, analysis_timeout + 180))


@mcp.tool()
def ghidra_list_functions(project_dir: str, project_name: str, program: str, limit: int = 200) -> dict[str, Any]:
    """List functions in a Ghidra program."""
    return run_cli([
        "list-functions",
        "--project-dir", project_dir,
        "--project-name", project_name,
        "--program", program,
        "--limit", str(limit),
    ])


@mcp.tool()
def ghidra_list_strings(project_dir: str, project_name: str, program: str, limit: int = 200, min_len: int = 4) -> dict[str, Any]:
    """List defined strings in a Ghidra program."""
    return run_cli([
        "list-strings",
        "--project-dir", project_dir,
        "--project-name", project_name,
        "--program", program,
        "--limit", str(limit),
        "--min-len", str(min_len),
    ])



@mcp.tool()
def ghidra_list_variables(project_dir: str, project_name: str, program: str, function: Optional[str] = None, address: Optional[str] = None, limit: int = 200) -> dict[str, Any]:
    """List decompiler local/parameter variables for a function."""
    if not function and not address:
        return {"ok": False, "error": "function or address is required"}
    args = ["list-variables", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--limit", str(limit)]
    if function:
        args += ["--function", function]
    else:
        args += ["--address", address or ""]
    return run_cli(args)

@mcp.tool()
def ghidra_decompile(
    project_dir: str,
    project_name: str,
    program: str,
    function: Optional[str] = None,
    address: Optional[str] = None,
    timeout_seconds: int = 30,
    max_chars: int = 20000,
) -> dict[str, Any]:
    """Decompile a function by name or address."""
    if not function and not address:
        return {"ok": False, "error": "function or address is required"}
    args = [
        "decompile",
        "--project-dir", project_dir,
        "--project-name", project_name,
        "--program", program,
        "--timeout-seconds", str(timeout_seconds),
        "--max-chars", str(max_chars),
    ]
    if function:
        args += ["--function", function]
    else:
        args += ["--address", address or ""]
    return run_cli(args, timeout=max(120, timeout_seconds + 90))



@mcp.tool()
def ghidra_list_imports(project_dir: str, project_name: str, program: str, limit: int = 200) -> dict[str, Any]:
    """List imported/external symbols in a Ghidra program."""
    return run_cli(["list-imports", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--limit", str(limit)])


@mcp.tool()
def ghidra_list_exports(project_dir: str, project_name: str, program: str, limit: int = 200) -> dict[str, Any]:
    """List exported/entry-point symbols in a Ghidra program."""
    return run_cli(["list-exports", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--limit", str(limit)])


@mcp.tool()
def ghidra_list_segments(project_dir: str, project_name: str, program: str) -> dict[str, Any]:
    """List memory blocks/segments for a Ghidra program."""
    return run_cli(["list-segments", "--project-dir", project_dir, "--project-name", project_name, "--program", program])


@mcp.tool()
def ghidra_function_info(project_dir: str, project_name: str, program: str, function: Optional[str] = None, address: Optional[str] = None) -> dict[str, Any]:
    """Get metadata for a function by name or address."""
    if not function and not address:
        return {"ok": False, "error": "function or address is required"}
    args = ["function-info", "--project-dir", project_dir, "--project-name", project_name, "--program", program]
    if function:
        args += ["--function", function]
    else:
        args += ["--address", address or ""]
    return run_cli(args)


@mcp.tool()
def ghidra_xrefs(project_dir: str, project_name: str, program: str, address: str, direction: str = "to", limit: int = 200) -> dict[str, Any]:
    """List xrefs to or from an address."""
    if direction not in {"to", "from"}:
        return {"ok": False, "error": "direction must be 'to' or 'from'"}
    return run_cli(["xrefs", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--address", address, "--direction", direction, "--limit", str(limit)])


@mcp.tool()
def ghidra_disassemble(project_dir: str, project_name: str, program: str, address: str, limit: int = 80) -> dict[str, Any]:
    """Disassemble instructions starting at an address."""
    return run_cli(["disassemble", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--address", address, "--limit", str(limit)])


@mcp.tool()
def ghidra_read_bytes(project_dir: str, project_name: str, program: str, address: str, length: int = 64) -> dict[str, Any]:
    """Read bytes from program memory at an address. Length is capped by the CLI."""
    return run_cli(["read-bytes", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--address", address, "--length", str(length)])


@mcp.tool()
def ghidra_search_symbols(project_dir: str, project_name: str, program: str, query: str, limit: int = 100, case_sensitive: bool = False) -> dict[str, Any]:
    """Search symbols by substring."""
    args = ["search-symbols", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--query", query, "--limit", str(limit)]
    if case_sensitive:
        args.append("--case-sensitive")
    return run_cli(args)


@mcp.tool()
def ghidra_rename_function(project_dir: str, project_name: str, program: str, address: str, new_name: str, dry_run: bool = True) -> dict[str, Any]:
    """Rename a function by address. Defaults to dry-run for safety."""
    args = ["rename-function", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--address", address, "--new-name", new_name]
    if dry_run:
        args.append("--dry-run")
    return run_cli(args)


@mcp.tool()
def ghidra_set_comment(project_dir: str, project_name: str, program: str, address: str, text: str, comment_type: str = "eol", dry_run: bool = True) -> dict[str, Any]:
    """Set a listing comment at an address. Defaults to dry-run for safety."""
    if comment_type not in {"eol", "pre", "post", "plate", "repeatable"}:
        return {"ok": False, "error": "invalid comment_type"}
    args = ["set-comment", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--address", address, "--comment-type", comment_type, "--text", text]
    if dry_run:
        args.append("--dry-run")
    return run_cli(args)

@mcp.tool()
def ghidra_callgraph(project_dir: str, project_name: str, program: str, mode: str = "edges", target: str = "", limit: int = 500) -> dict[str, Any]:
    """List callgraph edges, callers, or callees."""
    if mode not in {"edges", "callers", "callees"}:
        return {"ok": False, "error": "mode must be edges, callers, or callees"}
    return run_cli(["callgraph", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--mode", mode, "--target", target, "--limit", str(limit)])

@mcp.tool()
def ghidra_string_refs(project_dir: str, project_name: str, program: str, query: str = "", limit: int = 200) -> dict[str, Any]:
    """Find strings and their xrefs."""
    return run_cli(["string-refs", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--query", query, "--limit", str(limit)])

@mcp.tool()
def ghidra_list_bookmarks(project_dir: str, project_name: str, program: str, limit: int = 200) -> dict[str, Any]:
    """List bookmarks."""
    return run_cli(["list-bookmarks", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--limit", str(limit)])

@mcp.tool()
def ghidra_create_bookmark(project_dir: str, project_name: str, program: str, address: str, comment: str, bookmark_type: str = "Vegvisir", category: str = "Analysis", dry_run: bool = True) -> dict[str, Any]:
    """Create a bookmark. Defaults to dry-run."""
    args = ["create-bookmark", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--address", address, "--bookmark-type", bookmark_type, "--category", category, "--comment", comment]
    if dry_run:
        args.append("--dry-run")
    return run_cli(args)

@mcp.tool()
def ghidra_report(project_dir: str, project_name: str, program: str, limit: int = 100) -> dict[str, Any]:
    """Return a bounded summary report for the current program."""
    return run_cli(["report", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--limit", str(limit)])

@mcp.tool()
def ghidra_set_function_comment(project_dir: str, project_name: str, program: str, address: str, text: str, dry_run: bool = True) -> dict[str, Any]:
    """Set the function-level comment at an address. Defaults to dry-run."""
    args = ["set-function-comment", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--address", address, "--text", text]
    if dry_run:
        args.append("--dry-run")
    return run_cli(args)

@mcp.tool()
def ghidra_set_function_signature(project_dir: str, project_name: str, program: str, address: str, signature: str, dry_run: bool = True) -> dict[str, Any]:
    """Set a function signature by parsing C-like signature text. Defaults to dry-run."""
    args = ["set-function-signature", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--address", address, "--signature", signature]
    if dry_run:
        args.append("--dry-run")
    return run_cli(args)

@mcp.tool()
def ghidra_rename_variable(project_dir: str, project_name: str, program: str, function_address: str, old_name: str, new_name: str, dry_run: bool = True) -> dict[str, Any]:
    """Rename a decompiler local/parameter variable by function address and current name. Defaults to dry-run."""
    args = ["rename-variable", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--function-address", function_address, "--old-name", old_name, "--new-name", new_name]
    if dry_run:
        args.append("--dry-run")
    return run_cli(args)

@mcp.tool()
def ghidra_set_variable_type(project_dir: str, project_name: str, program: str, function_address: str, variable: str, type_name: str, dry_run: bool = True) -> dict[str, Any]:
    """Set a decompiler local/parameter variable data type by function address and name. Defaults to dry-run."""
    args = ["set-variable-type", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--function-address", function_address, "--variable", variable, "--type", type_name]
    if dry_run:
        args.append("--dry-run")
    return run_cli(args)


@mcp.tool()
def ghidra_apply_data_type(project_dir: str, project_name: str, program: str, address: str, type_name: str, length: int = -1, dry_run: bool = True) -> dict[str, Any]:
    """Apply a data type at an address. Defaults to dry-run."""
    args = ["apply-data-type", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--address", address, "--type", type_name, "--length", str(length)]
    if dry_run:
        args.append("--dry-run")
    return run_cli(args)

@mcp.tool()
def ghidra_create_struct(project_dir: str, project_name: str, program: str, name: str, size: int, dry_run: bool = True) -> dict[str, Any]:
    """Create a simple byte-backed structure data type. Defaults to dry-run."""
    args = ["create-struct", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--name", name, "--size", str(size)]
    if dry_run:
        args.append("--dry-run")
    return run_cli(args)

@mcp.tool()
def ghidra_patch_bytes(project_dir: str, project_name: str, program: str, address: str, hex_bytes: str, apply: bool = False) -> dict[str, Any]:
    """Patch bytes. Dry-run by default; set apply=True to modify the Ghidra database."""
    args = ["patch-bytes", "--project-dir", project_dir, "--project-name", project_name, "--program", program, "--address", address, "--hex", hex_bytes]
    if apply:
        args.append("--apply")
    return run_cli(args)


def main() -> None:
    parser = argparse.ArgumentParser(description="Ghidra headless MCP bridge")
    parser.add_argument("--transport", choices=["stdio", "sse"], default="stdio")
    parser.add_argument("--mcp-host", default="127.0.0.1")
    parser.add_argument("--mcp-port", type=int, default=18082)
    args = parser.parse_args()
    if args.transport == "sse":
        mcp.settings.host = args.mcp_host
        mcp.settings.port = args.mcp_port
        mcp.run(transport="sse")
    else:
        mcp.run(transport="stdio")


if __name__ == "__main__":
    main()

