from __future__ import annotations

import datetime as _dt
import hashlib
import json
import re
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

SCHEMA_CASE = "biw.case.v1"
SCHEMA_HASHES = "biw.hashes.v1"
SCHEMA_METADATA = "biw.metadata.v1"
SCHEMA_STRINGS = "biw.strings.v1"
SCHEMA_IMPORTS = "biw.imports.v1"
SCHEMA_EXPORTS = "biw.exports.v1"
SCHEMA_FUNCTIONS = "biw.functions.v1"
SCHEMA_CALLGRAPH = "biw.callgraph.v1"
SCHEMA_FINDINGS = "biw.findings.v1"


def utc_now() -> str:
    return _dt.datetime.now(_dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def slugify(value: str) -> str:
    value = re.sub(r"[^A-Za-z0-9._-]+", "-", value).strip("-._")
    return value[:80] or "binary"


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def read_json(path: Path, default: Any = None) -> Any:
    if not path.exists():
        return default
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def detect_format(path: Path, data: bytes) -> dict[str, str | None]:
    if data.startswith(b"\x7fELF"):
        arch = None
        if len(data) > 19:
            machine = int.from_bytes(data[18:20], "little")
            arch = {0x03: "x86", 0x3E: "x86_64", 0x28: "ARM", 0xB7: "AArch64", 0xF3: "RISC-V"}.get(machine, f"ELF machine {machine}")
        return {"format": "ELF", "architecture": arch, "mime": "application/x-elf"}
    if data.startswith(b"MZ"):
        return {"format": "PE", "architecture": None, "mime": "application/vnd.microsoft.portable-executable"}
    if data.startswith(b"\xfe\xed\xfa") or data.startswith(b"\xcf\xfa\xed\xfe") or data.startswith(b"\xca\xfe\xba\xbe"):
        return {"format": "Mach-O", "architecture": None, "mime": "application/x-mach-binary"}
    if data.startswith(b"!<arch>\n"):
        return {"format": "ar archive", "architecture": None, "mime": "application/x-archive"}
    return {"format": "unknown", "architecture": None, "mime": "application/octet-stream"}


def extract_ascii_strings(path: Path, min_len: int = 4, limit: int = 5000) -> list[dict[str, Any]]:
    data = path.read_bytes()
    out: list[dict[str, Any]] = []
    cur = bytearray()
    start = 0
    for i, b in enumerate(data):
        if 32 <= b <= 126 or b in (9,):
            if not cur:
                start = i
            cur.append(b)
        else:
            if len(cur) >= min_len:
                out.append({"offset": start, "value": cur.decode("ascii", errors="replace")})
                if len(out) >= limit:
                    break
            cur.clear()
    if len(out) < limit and len(cur) >= min_len:
        out.append({"offset": start, "value": cur.decode("ascii", errors="replace")})
    return out


def command_exists(cmd: str) -> bool:
    return shutil.which(cmd) is not None


def run_text(cmd: list[str], timeout: int = 30) -> tuple[int, str, str]:
    proc = subprocess.run(cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
    return proc.returncode, proc.stdout, proc.stderr


def parse_nm_symbols(path: Path) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]]]:
    imports: list[dict[str, Any]] = []
    exports: list[dict[str, Any]] = []
    functions: list[dict[str, Any]] = []
    if not command_exists("nm"):
        return imports, exports, functions
    rc, out, _ = run_text(["nm", "-D", "--defined-only", str(path)], timeout=20)
    if rc == 0:
        for line in out.splitlines():
            parts = line.split()
            if len(parts) >= 3:
                typ, name = parts[-2], parts[-1]
                exports.append({"name": name, "type": typ})
    rc, out, _ = run_text(["nm", "-D", "--undefined-only", str(path)], timeout=20)
    if rc == 0:
        for line in out.splitlines():
            parts = line.split()
            if parts:
                imports.append({"name": parts[-1]})
    rc, out, _ = run_text(["nm", "-n", str(path)], timeout=20)
    if rc == 0:
        for line in out.splitlines():
            parts = line.split()
            if len(parts) >= 3 and parts[-2].lower() in {"t", "w"}:
                addr, typ, name = parts[0], parts[-2], parts[-1]
                functions.append({
                    "id": f"func_{addr}", "name": name, "address": "0x" + addr,
                    "size_bytes": None, "namespace": "global", "signature": None,
                    "is_external": typ.isupper() is False and typ.lower() == "u", "is_thunk": False,
                    "callers": [], "callees": [], "string_refs": [], "import_refs": [], "tags": []
                })
    return imports, exports, functions


@dataclass
class CasePaths:
    root: Path

    @property
    def artifacts(self) -> Path: return self.root / "artifacts"
    @property
    def findings(self) -> Path: return self.root / "findings"
    @property
    def reports(self) -> Path: return self.root / "reports"
    @property
    def jobs(self) -> Path: return self.root / "jobs"
    @property
    def notes(self) -> Path: return self.root / "notes"

    def ensure(self) -> None:
        for p in [self.root, self.artifacts, self.findings, self.reports, self.jobs, self.notes, self.root / "source"]:
            p.mkdir(parents=True, exist_ok=True)


def make_case_id(binary: Path, sha: str) -> str:
    return f"{slugify(binary.name)}-{sha[:12]}"
