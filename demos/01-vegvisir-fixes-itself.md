# Demo 01 — Vegvisir Fixes Itself

## Goal

Show Vegvisir as a real engineering harness: it inspects a workspace, fixes a
small failing Rust test, runs verification, and summarizes the result.

## One-line pitch

> Vegvisir is the AI engineering harness I use to build and repair real code,
> including disposable self-fix style fixtures.

## Script

```bash
demos/scripts/01-vegvisir-fixes-itself.sh
```

Safe mode creates a disposable failing Rust project and prints the live command.
To invoke the provider-backed Vegvisir run:

```bash
RUN_LIVE=1 demos/scripts/01-vegvisir-fixes-itself.sh
```

## What to show

1. The failing test in the disposable workspace.
2. Vegvisir inspecting files.
3. Vegvisir editing the minimal source file.
4. `cargo test` passing after the patch.
5. Final summary with changed files and verification evidence.

## What this proves

- Workspace-scoped file inspection works.
- Tool-backed edits work.
- Test execution works.
- Vegvisir can close the loop from failure to verified patch.

## Recording notes

Keep the fixture small. The value is not the complexity of the bug; the value is
the full inspect/edit/test/report loop.
