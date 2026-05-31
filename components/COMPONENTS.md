# Vegvisir component systems

This directory contains first-class component systems that are packaged with the
Vegvisir monorepo.

## Vendored binary-intelligence components

The following formerly external projects have been vendored as component source
copies:

- `components/solarium` — Solarium browser/tool automation component.
- `components/ghidra` — Ghidra source tree used by Vegvisir's reverse-engineering tooling.
- `components/ghidra-mcp` — Ghidra UI MCP bridge/extension component.
- `components/ghidra-headless-mcp` — Ghidra headless MCP bridge component.

These are source components. Runtime products such as virtual environments,
`node_modules`, Gradle caches, build directories, generated distributions, and
Playwright browser caches should still be installed under the user's Vegvisir
runtime directory, normally:

```text
~/.vegvisir/tools
```

That keeps the repository authoritative for source and integration logic while
avoiding committing local build/cache artifacts.

## Source snapshot policy

The component copies were imported from:

```text
/mnt/storage/Vegvisir-Projects
```

Excluded from the vendored source copies:

- `.git/`
- `node_modules/`
- `dist/`
- `.venv/`
- `.gradle/`
- `build/`
- dependency caches such as Ghidra's local `dependencies/`
- per-project `.vegvisir/` runtime state

`components/ghidra-mcp` was copied from the current local source state, including
its pre-existing local source edits.

## Runtime integration

Installed Vegvisir should continue to materialize executable wrappers, Python
virtual environments, Node dependencies, Ghidra distributions, and MCP runtime
configuration under `~/.vegvisir/tools` / `~/.vegvisir/mcp.json` from these
component sources.
