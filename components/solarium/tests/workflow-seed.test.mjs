import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, writeFile, rm, readFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";

import {
  renderWorkflowSeedMarkdown,
  createWorkflowSeedFromFiles,
  handleJsonRpcRequest
} from "../dist/index.js";

test("renderWorkflowSeedMarkdown creates safe reusable workflow documentation", () => {
  const markdown = renderWorkflowSeedMarkdown({
    name: "Login smoke test",
    description: "Verify login page loads.",
    actions: [
      { type: "navigate", url: "https://example.com/login" },
      { type: "type", selector: "#password", text: "secret-value" },
      { type: "waitForSelector", selector: ".dashboard" }
    ],
    scope: { allowedHosts: ["example.com"], authorizationNote: "Authorized test" }
  });

  assert.match(markdown, /# Login smoke test/);
  assert.match(markdown, /solarium\.scopeCheck/);
  assert.match(markdown, /target_url/);
  assert.match(markdown, /form_values/);
  assert.match(markdown, /secret refs/);
  assert.doesNotMatch(markdown, /secret-value/);
  assert.match(markdown, /\$\{input\.form_values\}/);
});

test("createWorkflowSeedFromFiles reads actions and evidence and writes markdown", async () => {
  const dir = await mkdtemp(join(tmpdir(), "solarium-seed-"));
  const actionsPath = join(dir, "actions.json");
  const evidencePath = join(dir, "evidence.json");
  const output = join(dir, "seed.md");
  try {
    await writeFile(actionsPath, JSON.stringify([{ type: "navigate", url: "https://example.com" }, { type: "observe" }]));
    await writeFile(evidencePath, JSON.stringify({
      schemaVersion: "solarium.evidence.v1",
      runId: "run-seed",
      kind: "session",
      status: "ok",
      target: { url: "https://example.com" },
      artifacts: { summary: { files: 1, totalBytes: 2 } },
      scope: { allowedHosts: ["example.com"] }
    }));

    const markdown = await createWorkflowSeedFromFiles({ actionsPath, evidencePath, output });
    assert.match(markdown, /run-seed/);
    assert.match(markdown, /Evidence schema/);
    assert.equal(await readFile(output, "utf8"), markdown);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("solarium.workflowSeed tool returns generated markdown", async () => {
  const dir = await mkdtemp(join(tmpdir(), "solarium-seed-tool-"));
  const actionsPath = join(dir, "actions.json");
  try {
    await writeFile(actionsPath, JSON.stringify([{ type: "hover", selector: "nav" }]));
    const result = await handleJsonRpcRequest({
      jsonrpc: "2.0",
      id: 1,
      method: "solarium.workflowSeed",
      params: { actionsPath, name: "Hover nav" }
    });

    assert.match(result.markdown, /# Hover nav/);
    assert.match(result.markdown, /hover/);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
