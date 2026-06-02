from __future__ import annotations

import base64
import re
from pathlib import Path
from typing import Any

from .core import SCHEMA_FINDINGS, utc_now

NETWORK_APIS = {"socket", "connect", "send", "recv", "bind", "listen", "accept", "WinHttpOpen", "InternetOpen", "URLDownloadToFile"}
CRYPTO_APIS = {"AES", "DES", "SHA", "MD5", "EVP_", "CryptAcquireContext", "BCrypt", "libsodium"}
EXEC_APIS = {"system", "popen", "execve", "fork", "CreateProcess", "ShellExecute"}
MEMORY_APIS = {"VirtualAlloc", "VirtualProtect", "WriteProcessMemory", "CreateRemoteThread", "mmap", "mprotect", "ptrace"}

URL_RE = re.compile(r"https?://[^\s'\"<>]+", re.I)
IP_RE = re.compile(r"\b(?:\d{1,3}\.){3}\d{1,3}\b")
DOMAIN_RE = re.compile(r"\b[a-z0-9][a-z0-9.-]+\.(?:com|net|org|io|ru|cn|xyz|top|dev|local)\b", re.I)
PATH_RE = re.compile(r"(?:[A-Za-z]:\\|/etc/|/tmp/|/var/|/usr/|/home/|\\\\)")
BASE64_RE = re.compile(r"^[A-Za-z0-9+/]{32,}={0,2}$")
SHELL_TERMS = ["/bin/sh", "cmd.exe", "powershell", "bash", "curl ", "wget "]
PERSISTENCE_TERMS = ["Run\\", "CurrentVersion\\Run", "systemd", "cron", "Startup"]
CREDENTIAL_TERMS = ["password", "passwd", "secret", "token", "apikey", "credential"]


def _values(artifact: dict[str, Any], key: str) -> list[Any]:
    val = artifact.get(key)
    return val if isinstance(val, list) else []


def normalize_string_items(strings_artifact: dict[str, Any]) -> list[dict[str, Any]]:
    for key in ("strings", "items", "results"):
        items = strings_artifact.get(key)
        if isinstance(items, list):
            out = []
            for item in items:
                if isinstance(item, dict):
                    value = item.get("value") or item.get("string") or item.get("text") or ""
                    out.append({**item, "value": str(value)})
                else:
                    out.append({"value": str(item)})
            return out
    return []


def normalize_symbol_items(artifact: dict[str, Any], keys: tuple[str, ...]) -> list[dict[str, Any]]:
    for key in keys:
        items = artifact.get(key)
        if isinstance(items, list):
            out = []
            for item in items:
                if isinstance(item, dict):
                    name = item.get("name") or item.get("symbol") or item.get("label") or ""
                    out.append({**item, "name": str(name)})
                else:
                    out.append({"name": str(item)})
            return out
    return []


def analyze_strings(strings_artifact: dict[str, Any]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    suspicious = []
    findings = []
    categories: dict[str, list[dict[str, Any]]] = {}
    for item in normalize_string_items(strings_artifact):
        value = item.get("value", "")
        hit_cats = []
        if URL_RE.search(value): hit_cats.append("url")
        if IP_RE.search(value): hit_cats.append("ip-address")
        if DOMAIN_RE.search(value): hit_cats.append("domain")
        if PATH_RE.search(value): hit_cats.append("filesystem-path")
        if BASE64_RE.match(value): hit_cats.append("base64-like")
        if any(term.lower() in value.lower() for term in SHELL_TERMS): hit_cats.append("shell-command")
        if any(term.lower() in value.lower() for term in PERSISTENCE_TERMS): hit_cats.append("persistence")
        if any(term.lower() in value.lower() for term in CREDENTIAL_TERMS): hit_cats.append("credential-keyword")
        if hit_cats:
            rec = {"value": value, "offset": item.get("offset"), "address": item.get("address"), "categories": hit_cats}
            suspicious.append(rec)
            for c in hit_cats:
                categories.setdefault(c, []).append(rec)
    for c, hits in sorted(categories.items()):
        findings.append({
            "id": f"finding_string_{c.replace('-', '_')}",
            "title": f"Suspicious strings: {c}",
            "severity": "medium" if c in {"url", "ip-address", "shell-command", "persistence", "credential-keyword"} else "low",
            "confidence": "medium",
            "category": "strings",
            "description": f"Found {len(hits)} string(s) matching {c} patterns.",
            "evidence": [{"type": "string", "value": h["value"][:200], "artifact": "strings.json"} for h in hits[:10]],
            "recommended_next_steps": ["Inspect functions referencing these strings in Ghidra."],
            "created_by": "biw.heuristics",
            "created_at": utc_now(),
        })
    return suspicious, findings


def analyze_imports(imports_artifact: dict[str, Any]) -> tuple[dict[str, list[str]], list[dict[str, Any]]]:
    imports = normalize_symbol_items(imports_artifact, ("imports", "symbols", "items", "results"))
    names = [i.get("name", "") for i in imports]
    caps = {"network": [], "crypto": [], "execution": [], "memory_injection": []}
    def canonical(symbol: str) -> str:
        base = symbol.split("@")[0]
        base = base.split("(")[0]
        return base.strip("_ ").lower()

    def exact_or_prefixed(symbol: str, api: str) -> bool:
        # Avoid noisy substring matches such as bindtextdomain -> bind.
        sym = canonical(symbol)
        api_l = api.lower().rstrip("*")
        return sym == api_l or sym.startswith(api_l + "_") or sym.startswith(api_l + "$")

    for name in names:
        low = canonical(name)
        for api in NETWORK_APIS:
            if exact_or_prefixed(name, api): caps["network"].append(name)
        for api in CRYPTO_APIS:
            if api.lower().endswith("_"):
                if low.startswith(api.lower()): caps["crypto"].append(name)
            elif exact_or_prefixed(name, api) or api.lower() in low: caps["crypto"].append(name)
        for api in EXEC_APIS:
            if exact_or_prefixed(name, api): caps["execution"].append(name)
        for api in MEMORY_APIS:
            if exact_or_prefixed(name, api): caps["memory_injection"].append(name)
    caps = {k: sorted(set(v)) for k, v in caps.items() if v}
    findings = []
    severity = {"network": "medium", "crypto": "low", "execution": "medium", "memory_injection": "high"}
    for cat, hits in caps.items():
        findings.append({
            "id": f"finding_import_{cat}",
            "title": f"Potential {cat.replace('_', ' ')} capability",
            "severity": severity.get(cat, "low"),
            "confidence": "medium",
            "category": cat,
            "description": f"Imports or symbols suggest {cat.replace('_', ' ')} behavior.",
            "evidence": [{"type": "import", "value": h, "artifact": "imports.json"} for h in hits[:20]],
            "recommended_next_steps": ["Inspect callers/references for the listed APIs."],
            "created_by": "biw.heuristics",
            "created_at": utc_now(),
        })
    return caps, findings


def risk_from_findings(findings: list[dict[str, Any]]) -> dict[str, Any]:
    weights = {"low": 1, "medium": 3, "high": 7, "critical": 12}
    score = sum(weights.get(f.get("severity"), 0) for f in findings)
    if score >= 12: level = "high"
    elif score >= 4: level = "medium"
    elif score > 0: level = "low"
    else: level = "unknown"
    return {"level": level, "score": score, "rationale": [f.get("title") for f in findings[:8]]}


def build_findings(strings_artifact: dict[str, Any], imports_artifact: dict[str, Any]) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    suspicious_strings, string_findings = analyze_strings(strings_artifact)
    capability_map, import_findings = analyze_imports(imports_artifact)
    findings = string_findings + import_findings
    return (
        {"schema_version": "biw.suspicious_strings.v1", "suspicious_strings": suspicious_strings},
        {"schema_version": "biw.capability_map.v1", "capabilities": capability_map},
        {"schema_version": SCHEMA_FINDINGS, "findings": findings, "risk": risk_from_findings(findings)},
    )
