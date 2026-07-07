# Demo 10 — MSP Reference Host Adapter

## Goal

Show that MSP is not a Vegvisir-only internal system by consuming an MSP registry
from a tiny standalone Python reference host.

## One-line pitch

> MSP is a portable skill protocol: a non-Vegvisir host can search, verify, and
> load the same skill artifact.

## Script

```bash
demos/scripts/10-msp-reference-host-adapter.sh
```

The script runs:

```text
demos/reference-host/msp_reference_host.py
```

## What the reference host does

1. Calls the standalone MSP CLI.
2. Searches a registry for a skill matching a task.
3. Runs trust verification.
4. Refuses to load if trust fails.
5. Loads the skill body if hash verification passes.
6. Prints a bounded body preview.

## What this proves

- MSP has a host-neutral consumption surface.
- Vegvisir is one MSP host, not the protocol's reason for existing.
- Other harnesses can integrate through CLI/JSON-RPC/native bindings.

## Scope note

This is a reference adapter, not a claim that Codex, ClaudeCode, OpenClaw, Pi,
or Hermes already have native MSP adapters. It proves the protocol boundary is
usable by a standalone host.
