#!/usr/bin/env bash
set -euo pipefail

# Build a compact repo map and per-file symbol index for low-token navigation.
#
# Usage:
#   ./companion_scripts/vrepo-map.sh [--full|--update] [--diff <snapshot-dir>] [--query <term>] [--symbol <name>] [--watch[=<sec>]] [--interval <sec>] [--snippet-lines <n>] [--snippet-radius <n>] [--export jsonl] [path] [out-dir]
#   ./companion_scripts/vrepo-map.sh --help
#
# Outputs in out-dir:
#   - repo-map.md        : concise human-readable overview
#   - files.tsv          : path, size, mtime, extension, kind, language, symbol_count
#   - directories.tsv    : directory-level summaries
#   - functions.tsv      : per-file symbols with snippets
#   - snippets.tsv       : snippet-only index for symbol hits
#   - index.json         : machine-readable summary
#   - state.json         : compact snapshot state
#   - export.jsonl       : when --export jsonl is used
#   - diff.md/json       : when --diff is used
#   - query.md/json      : when --query is used
#   - symbol.md/json     : when --symbol is used
#
# Notes:
#   - skips .git and .vegvisir by default
#   - update mode rescans only changed files when a prior index exists
#   - uses language-aware heuristics for symbol extraction
#   - diff mode compares against another snapshot directory
#   - query mode indexes the current snapshot for token-cheap navigation
#   - symbol mode performs an exact symbol-name lookup
#   - watch mode reruns the index on a timer for live navigation refreshes

show_help() {
  cat <<'EOF'
Usage:
  vrepo-map.sh [--full|--update] [--diff <snapshot-dir>] [--query <term>] [--symbol <name>] [--watch[=<sec>]] [--interval <sec>] [--snippet-lines <n>] [--snippet-radius <n>] [--export jsonl] [path] [out-dir]
  vrepo-map.sh --help

Examples:
  vrepo-map.sh
  vrepo-map.sh --update
  vrepo-map.sh . .vegvisir/repo-map
  vrepo-map.sh --diff .vegvisir/repo-map
  vrepo-map.sh --query dispatcher
  vrepo-map.sh --symbol run_once
  vrepo-map.sh --watch=5 --query dispatcher
  vrepo-map.sh --update --query "repo map" --diff .vegvisir/repo-map-prev
  vrepo-map.sh --export jsonl
EOF
}

mode="full"
root="."
out_dir=".vegvisir/repo-map"
diff_ref=""
query=""
symbol=""
watch=0
interval=5
snippet_lines=2
snippet_radius=2
export_format=""
root_set=0
out_set=0

while (($#)); do
  case "$1" in
    -h|--help)
      show_help
      exit 0
      ;;
    -u|--update)
      mode="update"
      ;;
    -f|--full)
      mode="full"
      ;;
    --watch)
      watch=1
      ;;
    --watch=*)
      watch=1
      interval="${1#*=}"
      ;;
    -i|--interval)
      shift
      [[ $# -gt 0 ]] || { echo "Missing value for --interval" >&2; exit 1; }
      interval="$1"
      ;;
    --snippet-lines)
      shift
      [[ $# -gt 0 ]] || { echo "Missing value for --snippet-lines" >&2; exit 1; }
      snippet_lines="$1"
      ;;
    --snippet-lines=*)
      snippet_lines="${1#*=}"
      ;;
    --snippet-radius)
      shift
      [[ $# -gt 0 ]] || { echo "Missing value for --snippet-radius" >&2; exit 1; }
      snippet_radius="$1"
      ;;
    --snippet-radius=*)
      snippet_radius="${1#*=}"
      ;;
    --diff)
      shift
      [[ $# -gt 0 ]] || { echo "Missing value for --diff" >&2; exit 1; }
      diff_ref="$1"
      ;;
    --diff=*)
      diff_ref="${1#*=}"
      ;;
    --query)
      shift
      [[ $# -gt 0 ]] || { echo "Missing value for --query" >&2; exit 1; }
      query="$1"
      ;;
    --query=*)
      query="${1#*=}"
      ;;
    --symbol)
      shift
      [[ $# -gt 0 ]] || { echo "Missing value for --symbol" >&2; exit 1; }
      symbol="$1"
      ;;
    --symbol=*)
      symbol="${1#*=}"
      ;;
    --export)
      shift
      [[ $# -gt 0 ]] || { echo "Missing value for --export" >&2; exit 1; }
      export_format="$1"
      ;;
    --export=*)
      export_format="${1#*=}"
      ;;
    -o|--out-dir)
      shift
      [[ $# -gt 0 ]] || { echo "Missing value for --out-dir" >&2; exit 1; }
      out_dir="$1"
      out_set=1
      ;;
    --out-dir=*)
      out_dir="${1#*=}"
      out_set=1
      ;;
    --)
      shift
      while (($#)); do
        if (( root_set == 0 )); then
          root="$1"
          root_set=1
        elif (( out_set == 0 )); then
          out_dir="$1"
          out_set=1
        else
          echo "Unexpected argument: $1" >&2
          exit 1
        fi
        shift
      done
      break
      ;;
    *)
      if (( root_set == 0 )); then
        root="$1"
        root_set=1
      elif (( out_set == 0 )); then
        out_dir="$1"
        out_set=1
      else
        echo "Unexpected argument: $1" >&2
        exit 1
      fi
      ;;
  esac
  shift || true
done

if [[ ! -d "$root" ]]; then
  echo "Path not found: $root" >&2
  exit 1
fi

if ! [[ "$interval" =~ ^[0-9]+$ ]] || [[ "$interval" -lt 1 ]]; then
  echo "Invalid --interval value: $interval" >&2
  exit 1
fi
if ! [[ "$snippet_lines" =~ ^[0-9]+$ ]]; then
  echo "Invalid --snippet-lines value: $snippet_lines" >&2
  exit 1
fi
if ! [[ "$snippet_radius" =~ ^[0-9]+$ ]]; then
  echo "Invalid --snippet-radius value: $snippet_radius" >&2
  exit 1
fi
if [[ -n "$export_format" && "$export_format" != "jsonl" ]]; then
  echo "Unsupported --export format: $export_format (supported: jsonl)" >&2
  exit 1
fi

mkdir -p "$out_dir"

run_once() {
  python3 - "$root" "$out_dir" "$mode" "$diff_ref" "$query" "$symbol" "$snippet_lines" "$snippet_radius" "$export_format" <<'PY'
import json
import os
import re
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_dir = Path(sys.argv[2]).resolve()
mode = sys.argv[3]
diff_ref = sys.argv[4]
query = sys.argv[5]
symbol = sys.argv[6]
snippet_lines = int(sys.argv[7])
snippet_radius = int(sys.argv[8])
export_format = sys.argv[9]

files_tsv = out_dir / "files.tsv"
funcs_tsv = out_dir / "functions.tsv"
snips_tsv = out_dir / "snippets.tsv"
dirs_tsv = out_dir / "directories.tsv"
map_md = out_dir / "repo-map.md"
index_json = out_dir / "index.json"
state_json = out_dir / "state.json"
diff_json = out_dir / "diff.json"
diff_md = out_dir / "diff.md"
query_json = out_dir / "query.json"
query_md = out_dir / "query.md"
symbol_json = out_dir / "symbol.json"
symbol_md = out_dir / "symbol.md"
export_jsonl = out_dir / "export.jsonl"

SKIP_DIRS = {".git", ".vegvisir"}
BINARY_EXTS = {
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".ico", ".pdf",
    ".zip", ".tar", ".gz", ".xz", ".bz2", ".7z", ".jar", ".wasm", ".bin",
    ".exe", ".dll", ".so", ".dylib", ".class"
}
TEXT_SCAN_LIMIT = 2 * 1024 * 1024

LANG_BY_EXT = {
    ".rs": "rust",
    ".py": "python",
    ".js": "javascript",
    ".jsx": "javascript",
    ".ts": "typescript",
    ".tsx": "typescript",
    ".c": "cpp",
    ".h": "cpp",
    ".cc": "cpp",
    ".cpp": "cpp",
    ".hpp": "cpp",
    ".cs": "csharp",
    ".java": "java",
    ".sh": "shell",
    ".bash": "shell",
    ".md": "markdown",
    ".markdown": "markdown",
}

SYMBOL_PATTERNS = {
    "rust": [
        ("fn", re.compile(r"^\s*(?:pub(?:\([^)]+\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("struct", re.compile(r"^\s*(?:pub(?:\([^)]+\))?\s+)?struct\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("enum", re.compile(r"^\s*(?:pub(?:\([^)]+\))?\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("trait", re.compile(r"^\s*(?:pub(?:\([^)]+\))?\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("mod", re.compile(r"^\s*(?:pub(?:\([^)]+\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("type", re.compile(r"^\s*(?:pub(?:\([^)]+\))?\s+)?type\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("impl", re.compile(r"^\s*impl\b.*")),
    ],
    "python": [
        ("async-def", re.compile(r"^\s*async\s+def\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(")),
        ("def", re.compile(r"^\s*def\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(")),
        ("class", re.compile(r"^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
    ],
    "javascript": [
        ("function", re.compile(r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("class", re.compile(r"^\s*(?:export\s+)?class\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("enum", re.compile(r"^\s*(?:export\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("const-arrow", re.compile(r"^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(?:async\s*)?(?:\([^\)]*\)|[A-Za-z_][A-Za-z0-9_]*)\s*=>")),
    ],
    "typescript": [
        ("function", re.compile(r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("class", re.compile(r"^\s*(?:export\s+)?class\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("interface", re.compile(r"^\s*(?:export\s+)?interface\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("type", re.compile(r"^\s*(?:export\s+)?type\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("enum", re.compile(r"^\s*(?:export\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("namespace", re.compile(r"^\s*(?:export\s+)?namespace\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("const-arrow", re.compile(r"^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(?:async\s*)?(?:\([^\)]*\)|[A-Za-z_][A-Za-z0-9_]*)\s*=>")),
    ],
    "cpp": [
        ("namespace", re.compile(r"^\s*namespace\s+([A-Za-z_][A-Za-z0-9_:]*)\b")),
        ("class", re.compile(r"^\s*(?:template\s*<[^>]+>\s*)?(?:class|struct|enum)\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("function", re.compile(r"^\s*(?:template\s*<[^>]+>\s*)?(?:(?:inline|static|constexpr|virtual|friend|extern|typename)\s+)*[A-Za-z_][A-Za-z0-9_:<>,\s\*&~]*\s+([A-Za-z_][A-Za-z0-9_:~]*)\s*\([^;{]*\)\s*(?:const\s*)?(?:noexcept\s*)?(?:->\s*[A-Za-z_][A-Za-z0-9_:<>,\s\*&]+\s*)?(?:\{|;)$")),
    ],
    "csharp": [
        ("namespace", re.compile(r"^\s*namespace\s+([A-Za-z_][A-Za-z0-9_.]*)\b")),
        ("type", re.compile(r"^\s*(?:public|private|protected|internal|static|partial|abstract|sealed|new|readonly|unsafe|virtual|override|async|extern|ref|record|struct|class|interface|enum|delegate|file\s+)*\s*(?:class|struct|record|interface|enum|delegate)\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("method", re.compile(r"^\s*(?:public|private|protected|internal|static|partial|abstract|sealed|new|readonly|unsafe|virtual|override|async|extern|ref|internal\s+protected|protected\s+internal|private\s+protected|file\s+)*\s*[A-Za-z_][A-Za-z0-9_<>,\[\]\.\?\s\*&]+\s+([A-Za-z_][A-Za-z0-9_]*)\s*\([^;{]*\)\s*(?:where\s+[^\{]+)?\s*(?:\{|=>|;)")),
    ],
    "java": [
        ("package", re.compile(r"^\s*package\s+([A-Za-z_][A-Za-z0-9_.]*)\s*;")),
        ("type", re.compile(r"^\s*(?:public|private|protected|abstract|final|static|sealed|non-sealed|strictfp|native|synchronized|default|transient|volatile|record\s+)*\s*(?:class|interface|enum|record)\s+([A-Za-z_][A-Za-z0-9_]*)\b")),
        ("method", re.compile(r"^\s*(?:public|private|protected|abstract|final|static|synchronized|default|native|strictfp|transient|volatile|async\s+)*\s*[A-Za-z_][A-Za-z0-9_<>,\[\]\.\?\s\*&]+\s+([A-Za-z_][A-Za-z0-9_]*)\s*\([^;{]*\)\s*(?:throws\s+[^\{]+)?\s*(?:\{|;)")),
    ],
    "shell": [
        ("function", re.compile(r"^\s*(?:function\s+)?([A-Za-z_][A-Za-z0-9_-]*)\s*\(\s*\)\s*\{")),
        ("function", re.compile(r"^\s*(?:function\s+)?([A-Za-z_][A-Za-z0-9_-]*)\s*\(.*\)\s*\{")),
    ],
}


def classify(path: Path):
    ext = path.suffix.lower()
    lang = LANG_BY_EXT.get(ext, "text")
    kind = lang
    if ext in BINARY_EXTS:
        kind = "binary"
    elif lang == "markdown":
        kind = "markdown"
    elif lang == "text":
        kind = "text"
    return ext if ext else "(none)", kind, lang


def walk_files():
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for filename in filenames:
            path = Path(dirpath) / filename
            rel = path.relative_to(root).as_posix()
            yield rel, path


def safe_read_text(path: Path):
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except Exception:
        try:
            return path.read_text(encoding="latin-1", errors="replace")
        except Exception:
            return ""


def rust_impl_symbols(line: str):
    stripped = line.strip()
    if not stripped.startswith("impl"):
        return []
    tail = stripped[4:].strip()
    if not tail:
        return []
    tail = tail.split(" where ", 1)[0].strip()
    tail = tail.split("{")[0].strip()
    if " for " in tail:
        trait_part, target = tail.rsplit(" for ", 1)
        trait_part = trait_part.strip()
        target = target.strip()
        if trait_part and target:
            return [f"impl {trait_part} for {target}"]
    if tail:
        return [f"impl {tail}"]
    return []


def snippet_window(lines, lineno, radius):
    if not lines:
        return ""
    start = max(1, lineno - radius)
    end = min(len(lines), lineno + radius)
    chunk = []
    for i in range(start, end + 1):
        txt = lines[i - 1].rstrip("\n")
        chunk.append(f"{i}: {txt}")
    return " ⏎ ".join(chunk).replace("\t", " ")


def extract_symbols(rel: str, path: Path, lang: str, kind: str):
    if kind in {"binary", "markdown"}:
        return []
    try:
        size = path.stat().st_size
    except OSError:
        return []
    if size > TEXT_SCAN_LIMIT:
        return []
    patterns = SYMBOL_PATTERNS.get(lang, [])
    if not patterns:
        return []

    text = safe_read_text(path)
    lines = text.splitlines()
    symbols = []
    seen = set()
    for lineno, line in enumerate(lines, 1):
        if lang == "rust":
            for sym in rust_impl_symbols(line):
                key = (lineno, "impl", sym)
                if key not in seen:
                    seen.add(key)
                    symbols.append({
                        "line": lineno,
                        "kind": "impl",
                        "symbol": sym,
                        "snippet": snippet_window(lines, lineno, snippet_radius),
                    })
        for sym_kind, rx in patterns:
            m = rx.match(line)
            if not m:
                continue
            symbol_name = m.group(1) if m.groups() else line.strip()
            if not symbol_name:
                continue
            symbol_name = symbol_name.strip()
            key = (lineno, sym_kind, symbol_name)
            if key in seen:
                continue
            seen.add(key)
            symbols.append({
                "line": lineno,
                "kind": sym_kind,
                "symbol": symbol_name,
                "snippet": snippet_window(lines, lineno, snippet_radius),
            })
            break
    return symbols


def load_index_file(path: Path):
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None


def load_files_tsv(path: Path):
    data = {}
    if not path.exists():
        return data
    with path.open("r", encoding="utf-8") as f:
        for line in f:
            cols = line.rstrip("\n").split("\t")
            if len(cols) >= 7:
                rel, size, mtime, ext, kind, lang, sym_count = cols[:7]
            elif len(cols) == 6:
                rel, size, mtime, ext, kind, lang = cols
                sym_count = "0"
            elif len(cols) == 5:
                rel, size, ext, kind, lang = cols
                mtime = "0"
                sym_count = "0"
            else:
                continue
            data[rel] = {
                "size": int(size),
                "mtime": int(float(mtime)),
                "ext": ext,
                "kind": kind,
                "lang": lang,
                "sym_count": int(sym_count),
            }
    return data


def load_funcs_tsv(path: Path):
    data = defaultdict(list)
    if not path.exists():
        return data
    with path.open("r", encoding="utf-8") as f:
        for line in f:
            cols = line.rstrip("\n").split("\t")
            if len(cols) >= 5:
                rel, lineno, sym_kind, symbol_name, snippet = cols[:5]
            elif len(cols) == 4:
                rel, lineno, sym_kind, symbol_name = cols
                snippet = ""
            elif len(cols) == 3:
                rel, lineno, symbol_name = cols
                sym_kind = "symbol"
                snippet = ""
            else:
                continue
            data[rel].append({"line": int(lineno), "kind": sym_kind, "symbol": symbol_name, "snippet": snippet})
    return data


def dump_json(path: Path, obj):
    with path.open("w", encoding="utf-8") as f:
        json.dump(obj, f, indent=2, sort_keys=True)
        f.write("\n")


def write_jsonl(path: Path, rows):
    with path.open("w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps(row, sort_keys=True))
            f.write("\n")


current = {}
for rel, path in walk_files():
    try:
        st = path.stat()
    except OSError:
        continue
    ext, kind, lang = classify(path)
    current[rel] = {"size": int(st.st_size), "mtime": int(st.st_mtime), "ext": ext, "kind": kind, "lang": lang}

previous = load_files_tsv(files_tsv)
previous_funcs = load_funcs_tsv(funcs_tsv)

full_rescan = mode == "full" or not previous
changed_paths = []
removed_paths = sorted(set(previous) - set(current)) if previous else []
if full_rescan:
    changed_paths = sorted(current)
else:
    for rel, meta in current.items():
        old = previous.get(rel)
        if old is None or any(meta[k] != old.get(k) for k in ("size", "mtime", "kind", "lang")):
            changed_paths.append(rel)
    changed_paths.sort()

file_rows = []
for rel in sorted(current):
    meta = current[rel]
    file_rows.append((rel, meta["size"], meta["mtime"], meta["ext"], meta["kind"], meta["lang"]))

functions = defaultdict(list)
if not full_rescan and previous_funcs:
    for rel, entries in previous_funcs.items():
        if rel in current and rel not in changed_paths:
            functions[rel].extend(entries)

for rel in changed_paths:
    meta = current[rel]
    symbols = extract_symbols(rel, root / rel, meta["lang"], meta["kind"])
    functions[rel] = symbols
    current[rel]["sym_count"] = len(symbols)

for rel in current:
    if rel not in changed_paths:
        current[rel]["sym_count"] = len(functions.get(rel, []))

for rel in removed_paths:
    functions.pop(rel, None)

# Build directory stats recursively.
dir_stats = defaultdict(lambda: {"files": 0, "bytes": 0, "symbols": 0})
for rel, meta in current.items():
    size = int(meta["size"])
    sym_count = len(functions.get(rel, []))
    parent = Path(rel).parent
    parents = ["."]
    if str(parent) != ".":
        accum = []
        for part in parent.parts:
            accum.append(part)
            parents.append("/".join(accum))
    for d in dict.fromkeys(parents):
        dir_stats[d]["files"] += 1
        dir_stats[d]["bytes"] += size
        dir_stats[d]["symbols"] += sym_count

files_list = []
for rel, size, mtime, ext, kind, lang in file_rows:
    files_list.append({
        "path": rel,
        "size": size,
        "mtime": mtime,
        "ext": ext,
        "kind": kind,
        "lang": lang,
        "symbol_count": len(functions.get(rel, [])),
    })

functions_json = {rel: functions[rel] for rel in sorted(functions)}
directories_json = [
    {"path": d, "files": stats["files"], "bytes": stats["bytes"], "symbols": stats["symbols"]}
    for d, stats in sorted(dir_stats.items())
]

out_obj = {
    "root": str(root),
    "out_dir": str(out_dir),
    "mode": mode,
    "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z"),
    "file_count": len(files_list),
    "directory_count": len(directories_json),
    "symbol_count": sum(len(v) for v in functions.values()),
    "changed_files": changed_paths,
    "removed_files": removed_paths,
    "files": files_list,
    "directories": directories_json,
    "functions": functions_json,
}

dump_json(index_json, out_obj)
dump_json(state_json, {
    "mode": mode,
    "root": str(root),
    "out_dir": str(out_dir),
    "file_count": len(files_list),
    "symbol_count": out_obj["symbol_count"],
    "changed_file_count": len(changed_paths),
    "removed_file_count": len(removed_paths),
})

# JSONL export for downstream tooling.
if export_format == "jsonl":
    rows = []
    rows.append({"record_type": "meta", "root": str(root), "out_dir": str(out_dir), "mode": mode, "generated_at": out_obj["generated_at"]})
    for entry in files_list:
        rows.append({"record_type": "file", **entry})
    for entry in directories_json:
        rows.append({"record_type": "directory", **entry})
    for rel in sorted(functions_json):
        for entry in functions_json[rel]:
            rows.append({"record_type": "symbol", "path": rel, **entry})
    write_jsonl(export_jsonl, rows)

# Human-readable summary.
def top_n(items, key, n=20, reverse=True):
    return sorted(items, key=key, reverse=reverse)[:n]

level1 = defaultdict(lambda: {"files": 0, "bytes": 0, "symbols": 0})
for rel, meta in current.items():
    top = rel.split("/", 1)[0]
    level1[top]["files"] += 1
    level1[top]["bytes"] += meta["size"]
    level1[top]["symbols"] += len(functions.get(rel, []))

largest_files = top_n(files_list, key=lambda x: x["size"], n=20)
symbol_heavy_files = top_n(files_list, key=lambda x: x["symbol_count"], n=30)
largest_dirs = top_n(directories_json, key=lambda x: (x["symbols"], x["bytes"], x["files"]), n=30)
changed_preview = changed_paths[:100]

with map_md.open("w", encoding="utf-8") as f:
    f.write("# Repo map\n\n")
    f.write(f"- repo root: {root}\n")
    f.write(f"- output dir: {out_dir}\n")
    f.write(f"- mode: {mode}\n")
    f.write(f"- generated: {out_obj['generated_at']}\n")
    f.write(f"- file count: {len(files_list)}\n")
    f.write(f"- directory count: {len(directories_json)}\n")
    f.write(f"- symbol count: {out_obj['symbol_count']}\n")
    f.write(f"- changed files: {len(changed_paths)}\n")
    f.write(f"- removed files: {len(removed_paths)}\n")
    f.write(f"- snippet radius: {snippet_radius}\n")
    f.write(f"- snippet lines: {snippet_lines}\n\n")

    f.write("## Top-level directories\n")
    for name, stats in sorted(level1.items(), key=lambda kv: (kv[1]["symbols"], kv[1]["bytes"], kv[1]["files"]), reverse=True)[:25]:
        f.write(f"- {name} ({stats['files']} files, {stats['symbols']} symbols, {stats['bytes']} bytes)\n")
    f.write("\n")

    f.write("## Directory hotspots\n")
    for entry in largest_dirs[:25]:
        f.write(f"- {entry['path']} ({entry['files']} files, {entry['symbols']} symbols, {entry['bytes']} bytes)\n")
    f.write("\n")

    f.write("## Largest files\n")
    for entry in largest_files:
        f.write(f"- {entry['path']} ({entry['size']} bytes, {entry['lang']})\n")
    f.write("\n")

    f.write("## Symbol-heavy files\n")
    for entry in symbol_heavy_files:
        if entry['symbol_count'] == 0:
            continue
        f.write(f"- {entry['path']} ({entry['symbol_count']} symbols)\n")
    f.write("\n")

    if changed_preview:
        f.write("## Changed files\n")
        for rel in changed_preview:
            f.write(f"- {rel}\n")
        if len(changed_paths) > len(changed_preview):
            f.write(f"- ... and {len(changed_paths) - len(changed_preview)} more\n")
        f.write("\n")

    f.write("## Files\n")
    for entry in files_list[:250]:
        f.write(f"- {entry['path']} | {entry['size']} bytes | mtime={entry['mtime']} | {entry['lang']} | symbols={entry['symbol_count']}\n")
    f.write("\n")

    f.write("## Symbols\n")
    symbol_lines = 0
    for rel in sorted(functions):
        for entry in sorted(functions[rel], key=lambda e: (e['line'], e['kind'], e['symbol'])):
            snippet = entry.get('snippet', '')
            if snippet:
                f.write(f"- {rel}:{entry['line']} [{entry['kind']}] {entry['symbol']} :: {snippet}\n")
            else:
                f.write(f"- {rel}:{entry['line']} [{entry['kind']}] {entry['symbol']}\n")
            symbol_lines += 1
            if symbol_lines >= 600:
                break
        if symbol_lines >= 600:
            break
    if symbol_lines >= 600:
        f.write("- ... truncated\n")

# Diff mode.
if diff_ref:
    other_path = Path(diff_ref).resolve()
    other_index_path = other_path / "index.json" if other_path.is_dir() else other_path
    other_index = load_index_file(other_index_path)
    if not other_index:
        diff_summary = {"error": f"diff snapshot not found or unreadable: {other_index_path}", "against": str(other_index_path)}
        dump_json(diff_json, diff_summary)
        diff_md.write_text(f"# Repo diff\n\n- error: diff snapshot not found or unreadable: {other_index_path}\n", encoding="utf-8")
    else:
        other_files = {f["path"]: f for f in other_index.get("files", [])}
        other_funcs = other_index.get("functions", {})
        current_files = {f["path"]: f for f in files_list}
        current_funcs = functions_json

        added = sorted(set(current_files) - set(other_files))
        removed = sorted(set(other_files) - set(current_files))
        modified = []
        symbol_changes = []

        for path in sorted(set(current_files) & set(other_files)):
            cur = current_files[path]
            old = other_files[path]
            meta_changed = any(cur.get(k) != old.get(k) for k in ("size", "mtime", "ext", "kind", "lang"))
            cur_syms = {(e["kind"], e["symbol"]) for e in current_funcs.get(path, [])}
            old_syms = {(e.get("kind", "symbol"), e.get("symbol", "")) for e in other_funcs.get(path, [])}
            added_syms = sorted(cur_syms - old_syms)
            removed_syms = sorted(old_syms - cur_syms)
            if meta_changed or added_syms or removed_syms:
                modified.append(path)
                if added_syms or removed_syms:
                    symbol_changes.append({
                        "path": path,
                        "added_symbols": [{"kind": k, "symbol": s} for k, s in added_syms],
                        "removed_symbols": [{"kind": k, "symbol": s} for k, s in removed_syms],
                    })

        diff_obj = {
            "against": str(other_index_path),
            "added_files": added,
            "removed_files": removed,
            "modified_files": modified,
            "symbol_changes": symbol_changes,
            "summary": {
                "added_count": len(added),
                "removed_count": len(removed),
                "modified_count": len(modified),
                "symbol_change_count": len(symbol_changes),
            },
        }
        dump_json(diff_json, diff_obj)
        with diff_md.open("w", encoding="utf-8") as f:
            f.write("# Repo diff\n\n")
            f.write(f"- against: {other_index_path}\n")
            f.write(f"- added files: {len(added)}\n")
            f.write(f"- removed files: {len(removed)}\n")
            f.write(f"- modified files: {len(modified)}\n")
            f.write(f"- symbol-change files: {len(symbol_changes)}\n\n")
            if added:
                f.write("## Added files\n")
                for path in added[:200]:
                    f.write(f"- {path}\n")
                f.write("\n")
            if removed:
                f.write("## Removed files\n")
                for path in removed[:200]:
                    f.write(f"- {path}\n")
                f.write("\n")
            if modified:
                f.write("## Modified files\n")
                for path in modified[:200]:
                    f.write(f"- {path}\n")
                f.write("\n")
            if symbol_changes:
                f.write("## Symbol changes\n")
                for entry in symbol_changes[:100]:
                    f.write(f"- {entry['path']}\n")
                    for sym in entry["added_symbols"][:10]:
                        f.write(f"  - + [{sym['kind']}] {sym['symbol']}\n")
                    for sym in entry["removed_symbols"][:10]:
                        f.write(f"  - - [{sym['kind']}] {sym['symbol']}\n")
                f.write("\n")

# Query mode.
if query:
    needle = query.lower().strip()
    hits = []
    for entry in files_list:
        path = entry["path"]
        path_l = path.lower()
        lang_l = entry["lang"].lower()
        kind_l = entry["kind"].lower()
        score = 0
        reasons = []
        matched_syms = []

        if needle in path_l:
            score += 6
            reasons.append("path")
        if needle in lang_l or needle in kind_l:
            score += 2
            reasons.append("kind/lang")

        for sym in current_funcs.get(path, []):
            hay = f"{sym['symbol']} {sym['kind']} {sym.get('snippet', '')}".lower()
            if needle in hay:
                matched_syms.append(sym)
        if matched_syms:
            score += 3 + len(matched_syms)
            reasons.append("symbols")

        if score:
            hits.append({
                "path": path,
                "score": score,
                "reasons": reasons,
                "file": entry,
                "symbols": matched_syms[:20],
            })

    hits.sort(key=lambda x: (x["score"], x["file"]["symbol_count"], x["file"]["size"], x["path"]), reverse=True)
    query_obj = {"query": query, "hit_count": len(hits), "hits": hits}
    dump_json(query_json, query_obj)
    with query_md.open("w", encoding="utf-8") as f:
        f.write("# Repo query\n\n")
        f.write(f"- query: {query}\n")
        f.write(f"- hits: {len(hits)}\n\n")
        for hit in hits[:40]:
            f.write(f"- {hit['path']} | score={hit['score']} | reasons={', '.join(hit['reasons'])}\n")
            for sym in hit["symbols"][:5]:
                f.write(f"  - [{sym['kind']}] {sym['symbol']} @ line {sym['line']}\n")
                if sym.get("snippet"):
                    f.write(f"    - {sym['snippet']}\n")
        if len(hits) > 40:
            f.write(f"- ... and {len(hits) - 40} more\n")

# Symbol mode (exact lookup).
if symbol:
    needle = symbol.strip()
    hits = []
    for path, entries in current_funcs.items():
        exact = [e for e in entries if e.get("symbol") == needle]
        if exact:
            hits.append({
                "path": path,
                "file": next((f for f in files_list if f["path"] == path), None),
                "symbols": exact,
            })

    hits.sort(key=lambda x: (x["file"]["symbol_count"] if x["file"] else 0, x["path"]), reverse=True)
    symbol_obj = {"symbol": symbol, "hit_count": len(hits), "hits": hits}
    dump_json(symbol_json, symbol_obj)
    with symbol_md.open("w", encoding="utf-8") as f:
        f.write("# Repo symbol lookup\n\n")
        f.write(f"- symbol: {symbol}\n")
        f.write(f"- hits: {len(hits)}\n\n")
        for hit in hits[:40]:
            f.write(f"- {hit['path']}\n")
            for sym in hit["symbols"]:
                f.write(f"  - [{sym['kind']}] line {sym['line']} :: {sym['symbol']}\n")
                if sym.get("snippet"):
                    f.write(f"    - {sym['snippet']}\n")

print(f"Wrote: {map_md}")
print(f"Wrote: {files_tsv}")
print(f"Wrote: {dirs_tsv}")
print(f"Wrote: {funcs_tsv}")
print(f"Wrote: {snips_tsv}")
print(f"Wrote: {index_json}")
print(f"Wrote: {state_json}")
if export_format == "jsonl":
    print(f"Wrote: {export_jsonl}")
if diff_ref:
    print(f"Wrote: {diff_md}")
    print(f"Wrote: {diff_json}")
if query:
    print(f"Wrote: {query_md}")
    print(f"Wrote: {query_json}")
if symbol:
    print(f"Wrote: {symbol_md}")
    print(f"Wrote: {symbol_json}")
PY
}

if [[ "$watch" -eq 0 ]]; then
  run_once
  exit 0
fi

while true; do
  printf '\n== repo-map refresh %s ==\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  run_once
  sleep "$interval"
done
