# Vegvisir component systems

This directory contains first-class component systems that are packaged with the
Vegvisir monorepo.

## Binary-intelligence components

The following binary-intelligence component source trees are packaged with the
monorepo:

- `components/solarium` — Solarium browser/tool automation component; first-party Vegvisir-owned component under the Vegvisir MIT License.
- `components/ghidra-headless-mcp` — Ghidra headless MCP bridge component; first-party Vegvisir-owned component under the Vegvisir MIT License.
- `components/binary-intelligence-workbench` — Binary Intelligence Workbench Python analysis/reporting component; first-party Vegvisir-owned component under the Vegvisir MIT License.

Ghidra itself is intentionally **not** vendored in this repository. Install a
normal upstream Ghidra distribution separately and expose it to Vegvisir with
one of:

```bash
export GHIDRA_HOME=/path/to/ghidra_<version>
export GHIDRA_HEADLESS="$GHIDRA_HOME/support/analyzeHeadless"
# or place analyzeHeadless / ghidraRun on PATH
```

These are source components. Runtime products such as virtual environments,
`node_modules`, Gradle caches, build directories, generated distributions,
Ghidra projects, and Playwright browser caches should still be installed under
the user's Vegvisir runtime directory, normally:

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
- dependency caches
- per-project `.vegvisir/` runtime state
- Python bytecode caches such as `__pycache__/` and `*.pyc`

## External runtime policy

Ghidra is treated as an external installed runtime, not as vendored source.
Vegvisir installers, packaging scripts, maintenance scripts, MCP setup, and
agent workflows must not automatically fetch, clone, update, synchronize, or
build upstream Ghidra source.

Future changes to the supported Ghidra runtime contract are manual-only and
require explicit user direction.

## Runtime integration

Installed Vegvisir should continue to materialize executable wrappers, Python
virtual environments, Node dependencies, and MCP runtime configuration under
`~/.vegvisir/tools` / `~/.vegvisir/mcp.json` from these component sources.
The Ghidra wrappers point at an installed Ghidra runtime discovered from
`GHIDRA_HOME`, `GHIDRA_HEADLESS`, or PATH.
