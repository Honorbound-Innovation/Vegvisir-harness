#!/usr/bin/env python3
"""Minimal deterministic benchmark manifest runner.

This runner prints benchmark task metadata as JSON. Provider/network execution is intentionally not automatic; use it to select repeatable tasks and collect reports under benchmarks/reports/.
"""
import json
from pathlib import Path

tasks = []
for path in sorted((Path(__file__).parent / "tasks").glob("*.json")):
    tasks.append(json.loads(path.read_text()))
print(json.dumps({"task_count": len(tasks), "tasks": tasks}, indent=2))
