# GhidraHeadlessMCP

Headless Ghidra CLI + MCP bridge for Vegvisir-controlled reverse engineering.

This project wraps Ghidra's official `analyzeHeadless` launcher with bounded JSON-emitting
GhidraScripts and a Python MCP bridge.

## Requirements

- Local Ghidra source/distribution prepared for headless execution.
- Default launcher path used by this project:
  `/mnt/storage/Vegvisir-Projects/ghidra/Ghidra/RuntimeScripts/Linux/support/analyzeHeadless`
- Python 3.
- MCP Python package for the bridge. The existing `GhidraMCP/.venv` works.

Override launcher with:

```bash
export GHIDRA_HEADLESS=/path/to/support/analyzeHeadless
```

## CLI

```bash
bin/ghidra-headless status
bin/ghidra-headless import --binary ./sample --project-dir ./projects --project-name sample --overwrite
```

Most commands require:

```text
--project-dir <dir>
--project-name <name>
--program <program-name>
```

## Supported read-only tools

```text
status
import
report
list-functions
function-info
list-strings
string-refs
list-imports
list-exports
list-segments
xrefs
callgraph --mode edges|callers|callees
disassemble
read-bytes
search-symbols
list-bookmarks
list-variables
decompile
```

## Supported mutation tools

Mutation tools are bounded and intended for explicit use. MCP mutation tools default to dry-run.

```text
rename-function
set-comment
set-function-comment
set-function-signature
list-variables
rename-variable
set-variable-type
create-bookmark
create-struct
apply-data-type
patch-bytes        # dry-run by default; use --apply to modify bytes
```

Successful non-dry-run mutations append JSONL records to:

```text
<project-dir>/.vegvisir-ghidra/transactions.jsonl
```

## Decompiler-local mutation notes

`rename-variable` and `set-variable-type` operate on decompiler `HighSymbol` names. Use `list-variables` first to discover valid parameter/local names for a function. These operations support dry-run and append transaction-log records for non-dry-run mutations.

`set-function-signature` parses C-like signatures with Ghidra's `FunctionSignatureParser` and applies them with `ApplyFunctionSignatureCmd`. Use dry-run first to inspect the parsed signature.

## Examples

Import/analyze:

```bash
bin/ghidra-headless import \
  --binary /bin/true \
  --project-dir /tmp/vegvisir-ghidra-test \
  --project-name trueproj \
  --overwrite \
  --analysis-timeout 120
```

List functions:

```bash
bin/ghidra-headless list-functions \
  --project-dir /tmp/vegvisir-ghidra-test \
  --project-name trueproj \
  --program true \
  --limit 100
```

Decompile:

```bash
bin/ghidra-headless decompile \
  --project-dir /tmp/vegvisir-ghidra-test \
  --project-name trueproj \
  --program true \
  --address 00102000
```

Callgraph:

```bash
bin/ghidra-headless callgraph \
  --project-dir /tmp/vegvisir-ghidra-test \
  --project-name trueproj \
  --program true \
  --mode edges \
  --limit 200
```


List variables:

```bash
bin/ghidra-headless list-variables \
  --project-dir /tmp/vegvisir-ghidra-test \
  --project-name trueproj \
  --program true \
  --address 00102030
```

Dry-run variable rename/type:

```bash
bin/ghidra-headless rename-variable \
  --project-dir /tmp/vegvisir-ghidra-test \
  --project-name trueproj \
  --program true \
  --function-address 00102030 \
  --old-name __ptr \
  --new-name buffer \
  --dry-run

bin/ghidra-headless set-variable-type \
  --project-dir /tmp/vegvisir-ghidra-test \
  --project-name trueproj \
  --program true \
  --function-address 00102030 \
  --variable __ptr \
  --type 'char *' \
  --dry-run
```

Dry-run signature/datatype work:

```bash
bin/ghidra-headless set-function-signature \
  --project-dir /tmp/vegvisir-ghidra-test \
  --project-name trueproj \
  --program true \
  --address 00102000 \
  --signature 'int _DT_INIT(void)' \
  --dry-run

bin/ghidra-headless create-struct \
  --project-dir /tmp/vegvisir-ghidra-test \
  --project-name trueproj \
  --program true \
  --name MyStruct \
  --size 16 \
  --dry-run

bin/ghidra-headless apply-data-type \
  --project-dir /tmp/vegvisir-ghidra-test \
  --project-name trueproj \
  --program true \
  --address 00102000 \
  --type byte \
  --length 1 \
  --dry-run
```

Patch dry-run:

```bash
bin/ghidra-headless patch-bytes \
  --project-dir /tmp/vegvisir-ghidra-test \
  --project-name trueproj \
  --program true \
  --address 00102000 \
  --hex 90
```

Apply patch explicitly:

```bash
bin/ghidra-headless patch-bytes \
  --project-dir /tmp/vegvisir-ghidra-test \
  --project-name trueproj \
  --program true \
  --address 00102000 \
  --hex 90 \
  --apply
```

## MCP bridge

Stdio:

```bash
/mnt/storage/Vegvisir-Projects/GhidraMCP/.venv/bin/python \
  bridge_mcp_ghidra_headless.py \
  --transport stdio
```

SSE:

```bash
/mnt/storage/Vegvisir-Projects/GhidraMCP/.venv/bin/python \
  bridge_mcp_ghidra_headless.py \
  --transport sse \
  --mcp-host 127.0.0.1 \
  --mcp-port 18082
```

## Smoke test

```bash
examples/smoke-test.sh /tmp/vegvisir-ghidra-smoke /bin/true smoke
```

The smoke test exercises all supported read-only tools and dry-run mutation paths.

## Safety posture

- Read-only commands use `-readOnly`.
- MCP mutation tools default to dry-run.
- Byte patching requires explicit `--apply` / `apply=True`.
- Non-dry-run mutations are logged to JSONL.
- Outputs are bounded with limits/max chars where practical.
