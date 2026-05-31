#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT="${1:-/tmp/vegvisir-ghidra-smoke}"
BIN="${2:-/bin/true}"
NAME="${3:-smoke}"
PROG="$(basename "$BIN")"
rm -rf "$PROJECT"
cd "$ROOT"
run(){ echo "--- $*" >&2; bin/ghidra-headless "$@"; }
summary(){ python3 -c 'import sys,json; d=json.load(sys.stdin); print(sys.argv[1], "ok="+str(d.get("ok")), "emitted="+str(d.get("emitted", d.get("action", ""))), "err="+str(d.get("error",""))[:80])' "$1"; }
run status | summary status
run import --binary "$BIN" --project-dir "$PROJECT" --project-name "$NAME" --overwrite --analysis-timeout 120 | summary import
ADDR="$(run list-functions --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --limit 1 | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d["functions"][0]["entry"])')"
echo "ADDR=$ADDR" >&2
run list-functions --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --limit 10 | summary functions
run function-info --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$ADDR" | summary function-info
run list-strings --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --limit 10 | summary strings
run string-refs --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --query '' --limit 5 | summary string-refs
run list-imports --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --limit 10 | summary imports
run list-exports --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --limit 10 | summary exports
run list-segments --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" | summary segments
run xrefs --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$ADDR" --direction to --limit 10 | summary xrefs-to
run callgraph --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --mode edges --limit 10 | summary callgraph
run callgraph --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --mode callers --target "$ADDR" --limit 10 | summary callers
run callgraph --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --mode callees --target "$ADDR" --limit 10 | summary callees
run disassemble --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$ADDR" --limit 10 | summary disasm
run read-bytes --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$ADDR" --length 16 | summary bytes
run search-symbols --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --query str --limit 10 | summary symbols
run list-bookmarks --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --limit 5 | summary bookmarks
run create-bookmark --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$ADDR" --comment smoke --dry-run | summary bookmark-dry
run report --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --limit 10 | summary report
run decompile --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$ADDR" --timeout-seconds 30 --max-chars 2000 | summary decompile
run rename-function --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$ADDR" --new-name vegvisir_smoke_name --dry-run | summary rename-dry
run set-comment --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$ADDR" --comment-type eol --text 'smoke listing note' --dry-run | summary comment-dry
run set-function-comment --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$ADDR" --text 'smoke function note' --dry-run | summary fn-comment-dry
run set-function-signature --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$ADDR" --signature 'int vegvisir_smoke(void)' --dry-run | summary signature-dry
run create-struct --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --name VegvisirSmokeStruct --size 8 --dry-run | summary struct-dry
run apply-data-type --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$ADDR" --type byte --length 1 --dry-run | summary dtype-dry
VARADDR="$(run list-functions --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --limit 80 | python3 -c 'import json,sys; d=json.load(sys.stdin); print(next((f["entry"] for f in d["functions"] if f["name"]=="free"), d["functions"][0]["entry"]))')"
run list-variables --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$VARADDR" --limit 20 | summary variables
if run list-variables --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$VARADDR" --limit 20 | python3 -c 'import json,sys; d=json.load(sys.stdin); raise SystemExit(0 if d.get("variables") else 1)'; then
  VARNAME="$(run list-variables --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$VARADDR" --limit 20 | python3 -c 'import json,sys; print(json.load(sys.stdin)["variables"][0]["name"])')"
  run rename-variable --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --function-address "$VARADDR" --old-name "$VARNAME" --new-name vegvisir_var --dry-run | summary var-rename-dry
  run set-variable-type --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --function-address "$VARADDR" --variable "$VARNAME" --type 'char *' --dry-run | summary var-type-dry
else
  echo 'variables skipped: no decompiler variables found' >&2
fi
run patch-bytes --project-dir "$PROJECT" --project-name "$NAME" --program "$PROG" --address "$ADDR" --hex 90 | summary patch-dry
echo smoke ok
