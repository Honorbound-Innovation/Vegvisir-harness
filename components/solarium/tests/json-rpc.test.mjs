import test from "node:test";
import assert from "node:assert/strict";
import { PassThrough } from "node:stream";

import {
  handleJsonRpcRequest,
  SolariumJsonRpcClient,
  launchSolariumServer
} from "../dist/index.js";

test("handleJsonRpcRequest initializes and lists Solarium tools", async () => {
  const init = await handleJsonRpcRequest({ jsonrpc: "2.0", id: 1, method: "initialize", params: {} });
  assert.equal(init.serverInfo.name, "solarium");
  assert.equal(init.capabilities.tools instanceof Object, true);
  assert.equal(init.capabilities.tools.listChanged, false);
  assert.match(init.instructions, /scoped browser automation/);

  const initialized = await handleJsonRpcRequest({ jsonrpc: "2.0", method: "notifications/initialized", params: {} });
  assert.deepEqual(initialized, {});

  const listed = await handleJsonRpcRequest({ jsonrpc: "2.0", id: 2, method: "tools/list", params: {} });
  const names = listed.tools.map((tool) => tool.name);
  assert.ok(names.includes("solarium.browse"));
  assert.ok(names.includes("solarium.scopeCheck"));
  assert.ok(names.includes("solarium.manifest"));
  assert.ok(names.includes("solarium.audit"));
  assert.ok(names.includes("solarium.graphqlAudit"));
  assert.ok(names.includes("solarium.owaspAudit"));
});

test("handleJsonRpcRequest supports MCP-style tools/call", async () => {
  const result = await handleJsonRpcRequest({
    jsonrpc: "2.0",
    id: 1,
    method: "tools/call",
    params: {
      name: "solarium.scopeCheck",
      arguments: {
        url: "https://example.com",
        scope: { allowedHosts: ["example.com"], authorizationNote: "Authorized test" }
      }
    }
  });

  assert.equal(result.structuredContent.allowed, true);
  assert.equal(result.content[0].type, "text");
  assert.equal(result.isError, false);
});

test("handleJsonRpcRequest supports direct solarium.* methods", async () => {
  const result = await handleJsonRpcRequest({
    jsonrpc: "2.0",
    id: 1,
    method: "solarium.scopeCheck",
    params: {
      url: "https://outside.test",
      scope: { allowedHosts: ["example.com"], authorizationNote: "Authorized test" }
    }
  });

  assert.equal(result.allowed, false);
  assert.match(result.reason, /outside allowed scope/);
});

test("handleJsonRpcRequest maps invalid requests to JSON-RPC errors", async () => {
  await assert.rejects(
    () => handleJsonRpcRequest({ jsonrpc: "2.0", id: 1, method: "missing.method", params: {} }),
    /Method not found/
  );

  await assert.rejects(
    () => handleJsonRpcRequest({ jsonrpc: "1.0", id: 1, method: "ping", params: {} }),
    /jsonrpc must be 2.0/
  );

  await assert.rejects(
    () => handleJsonRpcRequest({ jsonrpc: "2.0", id: 1, method: "tools/call", params: { name: "solarium.scopeCheck", arguments: [] } }),
    /params.arguments must be an object/
  );
});

test("SolariumJsonRpcClient resolves responses and rejects JSON-RPC errors", async () => {
  const serverToClient = new PassThrough();
  const clientToServer = new PassThrough();
  const client = new SolariumJsonRpcClient({ input: serverToClient, output: clientToServer, requestTimeoutMs: 500 });

  const firstRequest = onceLine(clientToServer);
  const pending = client.call("ping", {});
  const request = JSON.parse(await firstRequest);
  assert.equal(request.method, "ping");
  serverToClient.write(JSON.stringify({ jsonrpc: "2.0", id: request.id, result: { ok: true } }) + "\n");
  assert.deepEqual(await pending, { ok: true });

  const secondRequest = onceLine(clientToServer);
  const rejected = client.call("bad", {});
  const errorRequest = JSON.parse(await secondRequest);
  serverToClient.write(JSON.stringify({ jsonrpc: "2.0", id: errorRequest.id, error: { code: -32601, message: "Nope" } }) + "\n");
  await assert.rejects(rejected, /Nope/);

  client.close();
});

test("SolariumJsonRpcClient times out unanswered requests", async () => {
  const serverToClient = new PassThrough();
  const clientToServer = new PassThrough();
  const client = new SolariumJsonRpcClient({ input: serverToClient, output: clientToServer, requestTimeoutMs: 10 });

  await assert.rejects(() => client.call("never", {}), /timed out/);
  client.close();
});

test("launchSolariumServer can initialize, list tools, and call scopeCheck", async () => {
  const launched = launchSolariumServer({
    command: process.execPath,
    args: ["dist/cli/index.js", "server"],
    requestTimeoutMs: 5000
  });

  try {
    const init = await launched.client.initialize({ protocolVersion: "2024-11-05", clientInfo: { name: "test", version: "0" }, capabilities: {} });
    assert.equal(init.serverInfo.name, "solarium");
    launched.client.initialized();

    const tools = await launched.client.listTools();
    assert.ok(tools.length >= 10);
    assert.ok(tools.some((tool) => tool.name === "solarium.scopeCheck"));

    const result = await launched.client.callSolariumTool("solarium.scopeCheck", {
      url: "https://example.com",
      scope: { allowedHosts: ["example.com"], authorizationNote: "Authorized adapter test" }
    });
    assert.equal(result.allowed, true);
  } finally {
    await launched.close();
  }
});

function onceLine(stream) {
  return new Promise((resolve) => {
    let buffer = "";
    const onData = (chunk) => {
      buffer += chunk.toString("utf8");
      const newline = buffer.indexOf("\n");
      if (newline === -1) return;
      stream.off("data", onData);
      resolve(buffer.slice(0, newline));
    };
    stream.on("data", onData);
  });
}
