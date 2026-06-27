#!/usr/bin/env python3
"""Minimal MSP reference host adapter demo.

This is intentionally tiny: it proves that a non-Vegvisir host can consume an
MSP registry by invoking the standalone MSP CLI, verifying trust, and loading a
skill body. Real hosts can replace the subprocess bridge with JSON-RPC or native
library bindings.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


def run_json(cmd: list[str], cwd: Path) -> dict:
    completed = subprocess.run(cmd, cwd=cwd, check=True, text=True, capture_output=True)
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise SystemExit(f"expected JSON from {' '.join(cmd)}\nstdout={completed.stdout}") from exc


def run_text(cmd: list[str], cwd: Path) -> str:
    completed = subprocess.run(cmd, cwd=cwd, check=True, text=True, capture_output=True)
    return completed.stdout


def main() -> int:
    parser = argparse.ArgumentParser(description="Tiny MSP-consuming reference host")
    parser.add_argument("--msp-root", default="/mnt/storage/Projects/MSP")
    parser.add_argument("--registry", default="examples/registry")
    parser.add_argument("--skill-id", default="skill.rust.refactor.module.v1")
    parser.add_argument("--task", default="refactor rust module")
    args = parser.parse_args()

    msp_root = Path(args.msp_root).resolve()
    if not (msp_root / "Cargo.toml").exists():
        raise SystemExit(f"MSP root does not look valid: {msp_root}")

    base = ["cargo", "run", "-q", "-p", "msp-cli", "--", "--registry", args.registry]

    print("[reference-host] Searching MSP registry", flush=True)
    search_output = run_text(base + ["registry", "search", "--task", args.task, "--max-risk", "medium"], msp_root)
    print(search_output.rstrip())

    print("\n[reference-host] Verifying trust", flush=True)
    trust = run_json(base + ["trust", "verify", args.skill_id], msp_root)
    print(json.dumps(trust, indent=2, sort_keys=True))
    if not trust.get("passed"):
        raise SystemExit("skill trust verification failed; refusing to load")

    print("\n[reference-host] Loading skill", flush=True)
    loaded = run_json(base + ["skills", "load", args.skill_id], msp_root)
    if not loaded.get("body_hash_valid"):
        raise SystemExit("body hash invalid; refusing to use skill")

    manifest = loaded.get("manifest", {})
    body = loaded.get("body", "")
    print(f"Loaded: {manifest.get('id')} / {manifest.get('name')}")
    print("\n--- skill body preview ---")
    print("\n".join(body.splitlines()[:24]))
    print("--- end preview ---")
    print("\n[reference-host] A real host would now inject this bounded skill context into its agent workflow.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
