import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, writeFile, rm, readFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";

import {
  createArtifactManifest,
  createEvidenceRunManifest,
  summarizeEvidenceAction,
  handleJsonRpcRequest
} from "../dist/index.js";

test("createArtifactManifest inventories and classifies files", async () => {
  const dir = await mkdtemp(join(tmpdir(), "solarium-artifacts-"));
  try {
    await writeFile(join(dir, "shot.png"), "png-ish");
    await writeFile(join(dir, "run.events.jsonl"), "{}\n");
    await writeFile(join(dir, "report.md"), "# Report\n");

    const manifest = await createArtifactManifest({ roots: [dir], baseDir: "/" });
    assert.equal(manifest.summary.files, 3);
    assert.equal(manifest.summary.byKind.screenshot, 1);
    assert.equal(manifest.summary.byKind["event-log"], 1);
    assert.equal(manifest.summary.byKind.report, 1);
    assert.ok(manifest.entries.every((entry) => entry.sha256));
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("createEvidenceRunManifest wraps artifacts with run metadata", async () => {
  const dir = await mkdtemp(join(tmpdir(), "solarium-evidence-"));
  const output = join(dir, "evidence.json");
  try {
    await writeFile(join(dir, "actions.json"), "[]\n");
    const manifest = await createEvidenceRunManifest({
      roots: [dir],
      output,
      runId: "run-1",
      kind: "session",
      status: "partial",
      url: "https://example.com",
      scope: { allowedHosts: ["example.com"] },
      actions: [
        { type: "navigate", url: "https://example.com" },
        { type: "type", selector: "#q", text: "not written to evidence summary" },
        { type: "screenshot", path: "shot.png" }
      ],
      metadata: { source: "test" },
      errors: ["example error"]
    });

    assert.equal(manifest.schemaVersion, "solarium.evidence.v1");
    assert.equal(manifest.runId, "run-1");
    assert.equal(manifest.kind, "session");
    assert.equal(manifest.status, "partial");
    assert.equal(manifest.target.url, "https://example.com");
    assert.equal(manifest.actions.length, 3);
    assert.equal(manifest.actions[1].selector, "#q");
    assert.equal("text" in manifest.actions[1], false);
    assert.equal(manifest.errors[0], "example error");

    const written = JSON.parse(await readFile(output, "utf8"));
    assert.equal(written.schemaVersion, "solarium.evidence.v1");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("summarizeEvidenceAction captures routing fields without sensitive values", () => {
  assert.deepEqual(summarizeEvidenceAction({ type: "waitForUrl", url: "**/dashboard", timeoutMs: 1000 }, 2), {
    index: 2,
    type: "waitForUrl",
    url: "**/dashboard",
    timeoutMs: 1000
  });

  const typed = summarizeEvidenceAction({ type: "type", selector: "#password", text: "secret" }, 3);
  assert.deepEqual(typed, { index: 3, type: "type", selector: "#password" });
});

test("solarium.manifest tool can create evidence run manifests", async () => {
  const dir = await mkdtemp(join(tmpdir(), "solarium-tool-evidence-"));
  try {
    await writeFile(join(dir, "result.json"), "{}\n");
    const result = await handleJsonRpcRequest({
      jsonrpc: "2.0",
      id: 1,
      method: "solarium.manifest",
      params: {
        roots: [dir],
        evidence: true,
        runId: "rpc-run",
        kind: "manual",
        url: "https://example.com",
        actions: [{ type: "hover", selector: "nav" }]
      }
    });

    assert.equal(result.schemaVersion, "solarium.evidence.v1");
    assert.equal(result.runId, "rpc-run");
    assert.equal(result.actions[0].type, "hover");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
