from __future__ import annotations

import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, urlparse

from .index import build_case_detail, build_case_index


class BIWRequestHandler(BaseHTTPRequestHandler):
    server_version = "BIWHTTP/0.1"

    @property
    def cases_root(self) -> Path:
        return self.server.cases_root  # type: ignore[attr-defined]

    def log_message(self, fmt: str, *args: Any) -> None:
        if getattr(self.server, "quiet", False):  # type: ignore[attr-defined]
            return
        super().log_message(fmt, *args)

    def _send_json(self, status: int, payload: Any) -> None:
        body = json.dumps(payload, indent=2, sort_keys=True).encode("utf-8") + b"\n"
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def _send_text(self, status: int, text: str, content_type: str = "text/plain; charset=utf-8") -> None:
        body = text.encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self) -> None:  # noqa: N802
        self.send_response(204)
        self.send_header("Allow", "GET, OPTIONS")
        self.send_header("Access-Control-Allow-Origin", "http://127.0.0.1")
        self.send_header("Access-Control-Allow-Methods", "GET, OPTIONS")
        self.end_headers()

    def do_GET(self) -> None:  # noqa: N802
        parsed = urlparse(self.path)
        path = parsed.path.rstrip("/") or "/"
        query = parse_qs(parsed.query)
        try:
            if path == "/":
                self._send_json(200, {
                    "service": "Binary Intelligence Workbench API",
                    "version": "0.1",
                    "cases_root": str(self.cases_root),
                    "endpoints": ["/health", "/api/index", "/api/cases", "/api/cases/{case_id}"],
                })
                return
            if path == "/health":
                self._send_json(200, {"ok": True, "cases_root": str(self.cases_root)})
                return
            if path in {"/api/index", "/api/cases"}:
                self._send_json(200, build_case_index(self.cases_root))
                return
            if path.startswith("/api/cases/"):
                case_id = path.split("/", 3)[3]
                include_artifacts = (query.get("include_artifacts") or ["false"])[0].lower() in {"1", "true", "yes"}
                self._send_json(200, build_case_detail(self.cases_root, case_id, include_artifacts=include_artifacts))
                return
            self._send_json(404, {"error": "not found", "path": parsed.path})
        except FileNotFoundError as exc:
            self._send_json(404, {"error": str(exc)})
        except Exception as exc:  # Keep local workbench API robust during UI iteration.
            self._send_json(500, {"error": f"internal error: {exc}"})


def make_server(host: str, port: int, cases_root: Path, quiet: bool = False) -> ThreadingHTTPServer:
    server = ThreadingHTTPServer((host, port), BIWRequestHandler)
    server.cases_root = cases_root.expanduser().resolve()  # type: ignore[attr-defined]
    server.quiet = quiet  # type: ignore[attr-defined]
    return server


def serve(host: str, port: int, cases_root: Path, quiet: bool = False) -> None:
    server = make_server(host, port, cases_root, quiet=quiet)
    try:
        print(f"BIW API serving {server.cases_root} at http://{host}:{server.server_port}")  # type: ignore[attr-defined]
        server.serve_forever()
    finally:
        server.server_close()
