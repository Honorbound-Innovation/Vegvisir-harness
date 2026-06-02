from pathlib import Path
import json
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def run(args):
    return subprocess.run([sys.executable, "-m", "biw.cli", *args], cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


class CliTests(unittest.TestCase):
    def test_basic_triage_creates_case(self):
        with tempfile.TemporaryDirectory() as td:
            tmp_path = Path(td)
            sample = tmp_path / "sample.bin"
            sample.write_bytes(b"\x7fELF" + b"\x00" * 64 + b"http://example.com\x00/bin/sh\x00password=redacted\x00")
            out = tmp_path / "cases"
            proc = run(["triage", str(sample), "--out", str(out), "--no-ghidra"])
            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            cases = list(out.iterdir())
            self.assertEqual(len(cases), 1)
            case_dir = cases[0]
            self.assertTrue((case_dir / "case.json").exists())
            self.assertTrue((case_dir / "artifacts" / "strings.json").exists())
            self.assertTrue((case_dir / "findings" / "findings.json").exists())
            self.assertTrue((case_dir / "reports" / "summary.md").exists())
            case = json.loads((case_dir / "case.json").read_text())
            self.assertEqual(case["schema_version"], "biw.case.v1")
            self.assertEqual(case["status"], "triaged")
            findings = json.loads((case_dir / "findings" / "findings.json").read_text())
            self.assertTrue(findings["findings"])

    def test_case_list_empty(self):
        with tempfile.TemporaryDirectory() as td:
            out = Path(td) / "cases"
            out.mkdir()
            proc = run(["case", "list", "--out", str(out)])
            self.assertEqual(proc.returncode, 0)
            self.assertEqual(json.loads(proc.stdout), [])

    def test_explain_function_metadata_only(self):
        with tempfile.TemporaryDirectory() as td:
            tmp_path = Path(td)
            sample = tmp_path / "sample.bin"
            sample.write_bytes(b"\x7fELF" + b"\x00" * 64 + b"main\x00http://example.com\x00")
            out = tmp_path / "cases"
            triage_proc = run(["triage", str(sample), "--out", str(out), "--no-ghidra", "--case-id", "sample-case"])
            self.assertEqual(triage_proc.returncode, 0, triage_proc.stderr + triage_proc.stdout)
            proc = run(["explain", "function", "sample-case", "main", "--out", str(out), "--no-ghidra"])
            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            reports = list((out / "sample-case" / "reports" / "functions").glob("*.md"))
            artifacts = list((out / "sample-case" / "artifacts" / "decompile").glob("*.json"))
            self.assertEqual(len(reports), 1)
            self.assertEqual(len(artifacts), 1)
            payload = json.loads(artifacts[0].read_text())
            self.assertEqual(payload["schema_version"], "biw.decompile.v1")
            self.assertIn("explanation", payload)

    def test_skill_run_and_full_report(self):
        with tempfile.TemporaryDirectory() as td:
            tmp_path = Path(td)
            sample = tmp_path / "sample.bin"
            sample.write_bytes(b"\x7fELF" + b"\x00" * 64 + b"connect\x00http://example.com\x00")
            out = tmp_path / "cases"
            triage_proc = run(["triage", str(sample), "--out", str(out), "--no-ghidra", "--case-id", "skill-case"])
            self.assertEqual(triage_proc.returncode, 0, triage_proc.stderr + triage_proc.stdout)
            proc = run(["skill", "run", "skill-case", "binary.triage.unknown", "--out", str(out)])
            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            skill_out = out / "skill-case" / "skills" / "binary.triage.unknown.json"
            self.assertTrue(skill_out.exists())
            payload = json.loads(skill_out.read_text())
            self.assertEqual(payload["schema_version"], "biw.skill-output.v1")
            self.assertEqual(payload["skill_id"], "binary.triage.unknown")
            proc = run(["report", "full", "skill-case", "--out", str(out), "--write"])
            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            full = out / "skill-case" / "reports" / "full.md"
            self.assertTrue(full.exists())
            self.assertIn("# Skill Outputs", full.read_text())

    def test_skill_list(self):
        proc = run(["skill", "list"])
        self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
        payload = json.loads(proc.stdout)
        self.assertIn("binary.triage.unknown", [s["id"] for s in payload["skills"]])


if __name__ == "__main__":
    unittest.main()

class IndexAndServerTests(unittest.TestCase):
    def _make_case(self, td: str, case_id: str = "index-case"):
        tmp_path = Path(td)
        sample = tmp_path / "sample.bin"
        sample.write_bytes(b"\x7fELF" + b"\x00" * 64 + b"main\x00connect\x00http://example.com\x00")
        out = tmp_path / "cases"
        proc = run(["triage", str(sample), "--out", str(out), "--no-ghidra", "--case-id", case_id])
        self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
        return out

    def test_case_index_and_export(self):
        with tempfile.TemporaryDirectory() as td:
            out = self._make_case(td)
            proc = run(["case", "index", "--out", str(out)])
            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            payload = json.loads(proc.stdout)
            self.assertEqual(payload["schema_version"], "biw.case-index.v1")
            self.assertEqual(payload["case_count"], 1)
            self.assertEqual(payload["cases"][0]["case_id"], "index-case")
            proc = run(["case", "export", "index-case", "--out", str(out)])
            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            detail = json.loads(proc.stdout)
            self.assertEqual(detail["schema_version"], "biw.case-detail.v1")
            self.assertEqual(detail["summary"]["case_id"], "index-case")
            self.assertIn("findings", detail)
            proc = run(["case", "index", "--out", str(out), "--write"])
            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            self.assertTrue((out / "index.json").exists())

    def test_api_server_index_and_case_detail(self):
        import threading
        import urllib.request
        from biw.server import make_server

        with tempfile.TemporaryDirectory() as td:
            out = self._make_case(td, "api-case")
            server = make_server("127.0.0.1", 0, out, quiet=True)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                base = f"http://127.0.0.1:{server.server_port}"
                with urllib.request.urlopen(base + "/health", timeout=5) as resp:
                    health = json.loads(resp.read().decode("utf-8"))
                self.assertTrue(health["ok"])
                with urllib.request.urlopen(base + "/api/index", timeout=5) as resp:
                    index = json.loads(resp.read().decode("utf-8"))
                self.assertEqual(index["case_count"], 1)
                with urllib.request.urlopen(base + "/api/cases/api-case", timeout=5) as resp:
                    detail = json.loads(resp.read().decode("utf-8"))
                self.assertEqual(detail["summary"]["case_id"], "api-case")
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)

class CompletionMilestoneTests(unittest.TestCase):
    def test_diff_firmware_memory_and_agent_commands(self):
        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            old = tmp / "old.bin"
            new = tmp / "new.bin"
            old.write_bytes(b"\x7fELF" + b"\x00" * 64 + b"alpha\x00connect\x00")
            new.write_bytes(b"\x7fELF" + b"\x00" * 64 + b"alpha\x00beta\x00http://example.com\x00")
            out = tmp / "cases"

            proc = run(["diff", str(old), str(new), "--out", str(out), "--case-id", "diff-case"])
            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            self.assertTrue((out / "diff-case" / "artifacts" / "diff.json").exists())

            firmware = tmp / "firmware.img"
            firmware.write_bytes(b"FIRMWARE" + b"\x00" * 32 + b"\x7fELF" + b"/etc/passwd\x00http://device.local\x00")
            proc = run(["firmware", "triage", str(firmware), "--out", str(out), "--case-id", "fw-case"])
            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            inv = json.loads((out / "fw-case" / "artifacts" / "firmware-inventory.json").read_text())
            self.assertEqual(inv["schema_version"], "biw.firmware.v1")
            self.assertTrue(inv["embedded_candidates"])

            proc = run(["memory", "export", "fw-case", "--out", str(out), "--write"])
            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            self.assertTrue((out / "fw-case" / "memory" / "cms-summary.json").exists())

            proc = run(["agent", "list"])
            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            agents = json.loads(proc.stdout)
            self.assertTrue(agents["agents"])
            proc = run(["agent", "plan", "fw-case", "firmware-analysis-agent", "Review firmware indicators", "--out", str(out)])
            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            self.assertTrue(list((out / "fw-case" / "jobs" / "agents").glob("*.json")))
