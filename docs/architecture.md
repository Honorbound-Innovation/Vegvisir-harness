# Architecture

Vegvisir is a Rust AI harness composed of provider adapters, a TUI application shell, guardrails, CMS-v2 memory/ECM context preparation, HBSE secret boundaries, run artifacts, subagents, skills/LSL, verification, and evals.

CMS owns long-term memory. ECM owns active context exposure. Response generation belongs to the provider adapter.
