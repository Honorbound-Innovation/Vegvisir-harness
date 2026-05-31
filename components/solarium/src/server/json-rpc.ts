import { createInterface } from "node:readline";
import { stdin as defaultInput, stdout as defaultOutput } from "node:process";
import type { Readable, Writable } from "node:stream";
import { browse } from "../agent/actions.js";
import { inspectPage } from "../agent/inspect.js";
import { planActionsFromInspectResult } from "../agent/plan.js";
import { runActions } from "../agent/session.js";
import { runLoop } from "../agent/loop.js";
import { audit } from "../security/audit.js";
import { graphqlAudit } from "../security/graphql-audit.js";
import { owaspAudit } from "../security/owasp-audit.js";
import { crawl } from "../security/crawler.js";
import { checkUrlScope, validateScopePolicy, type ScopePolicy } from "../security/scope.js";
import { createArtifactManifest } from "../reporting/artifacts.js";
import { createEvidenceRunManifest } from "../reporting/evidence.js";
import { createWorkflowSeedFromFiles } from "../skills/workflow-seed.js";
import { replayEvents } from "../reporting/replay.js";
import { getBuiltInProfile, listBuiltInProfiles } from "../browser/profile-store.js";
import { validateSolariumConfig } from "../config/validate.js";
import type { AgentAction, BrowserEngine, BrowserProfileName, InspectResult } from "../types.js";

export interface JsonRpcServerOptions {
  input?: Readable;
  output?: Writable;
  name?: string;
  version?: string;
}

export interface JsonRpcRequest {
  jsonrpc?: "2.0";
  id?: string | number | null;
  method: string;
  params?: unknown;
}

interface JsonRpcSuccess {
  jsonrpc: "2.0";
  id: string | number | null;
  result: unknown;
}

interface JsonRpcFailure {
  jsonrpc: "2.0";
  id: string | number | null;
  error: {
    code: number;
    message: string;
    data?: unknown;
  };
}

interface ToolDefinition {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
}

const SERVER_NAME = "solarium";
const SERVER_VERSION = "0.1.0";
const DEFAULT_PROTOCOL_VERSION = "2024-11-05";
const SERVER_INSTRUCTIONS =
  "Solarium provides scoped browser automation tools for authorized browsing, inspection, sessions, crawling, audits, evidence manifests, and validation. Prefer explicit scope policies for browser-affecting tools; never pass plaintext secrets in tool arguments.";

const tools: ToolDefinition[] = [
  {
    name: "solarium.browse",
    description: "Open a URL in a controlled browser context and optionally observe, extract text, or capture a screenshot.",
    inputSchema: objectSchema({
      url: { type: "string" },
      scope: scopeSchema(),
      engine: engineSchema(),
      profile: profileSchema(),
      headless: { type: "boolean", default: true },
      screenshotPath: { type: "string" },
      extractText: { type: "boolean" },
      observe: { type: "boolean" },
      maxTextChars: { type: "number", minimum: 0 },
      maxElements: { type: "number", minimum: 0 }
    }, ["url"])
  },
  {
    name: "solarium.inspect",
    description: "Inspect a page and return agent-actionable selector candidates.",
    inputSchema: objectSchema({
      url: { type: "string" },
      scope: scopeSchema(),
      engine: engineSchema(),
      profile: profileSchema(),
      headless: { type: "boolean", default: true },
      includeObservation: { type: "boolean" },
      maxCandidates: { type: "number", minimum: 0 },
      maxTextChars: { type: "number", minimum: 0 },
      maxElements: { type: "number", minimum: 0 }
    }, ["url"])
  },
  {
    name: "solarium.plan",
    description: "Generate conservative session actions from a prior inspect result.",
    inputSchema: objectSchema({
      inspectResult: { type: "object" },
      goal: { type: "string" },
      maxActions: { type: "number", minimum: 0 },
      fillValue: { type: "string" },
      preferKinds: { type: "array", items: { type: "string" } }
    }, ["inspectResult"])
  },
  {
    name: "solarium.session",
    description: "Run a multi-step browser session from an AgentAction array.",
    inputSchema: objectSchema({
      actions: { type: "array", items: { type: "object" } },
      scope: scopeSchema(),
      engine: engineSchema(),
      profile: profileSchema(),
      headless: { type: "boolean", default: true },
      sessionId: { type: "string" },
      evidenceDir: { type: "string" },
      observeAfterEachAction: { type: "boolean" },
      continueOnError: { type: "boolean" },
      maxTextChars: { type: "number", minimum: 0 },
      maxElements: { type: "number", minimum: 0 }
    }, ["actions"])
  },
  {
    name: "solarium.loop",
    description: "Run a bounded inspect-plan-act loop against an authorized URL.",
    inputSchema: objectSchema({
      url: { type: "string" },
      scope: scopeSchema(),
      goal: { type: "string" },
      engine: engineSchema(),
      profile: profileSchema(),
      headless: { type: "boolean", default: true },
      maxIterations: { type: "number", minimum: 0 },
      actionsPerIteration: { type: "number", minimum: 0 },
      evidenceDir: { type: "string" },
      includeObservations: { type: "boolean" },
      screenshotEachIteration: { type: "boolean" },
      stopWhenText: { type: "string" },
      stopWhenUrl: { type: "string" },
      stopWhenSelector: { type: "string" },
      continueOnError: { type: "boolean" }
    }, ["url"])
  },
  {
    name: "solarium.crawl",
    description: "Crawl in-scope pages and inventory links/forms. Requires a scope policy.",
    inputSchema: objectSchema({
      startUrl: { type: "string" },
      scope: scopeSchema(),
      engine: engineSchema(),
      profile: profileSchema(),
      headless: { type: "boolean", default: true },
      maxPages: { type: "number", minimum: 0 },
      maxDepth: { type: "number", minimum: 0 },
      includeObservations: { type: "boolean" },
      evidenceDir: { type: "string" },
      screenshots: { type: "boolean" }
    }, ["startUrl", "scope"])
  },
  {
    name: "solarium.audit",
    description: "Passively audit an authorized page for common defensive web security findings.",
    inputSchema: objectSchema({
      url: { type: "string" },
      scope: scopeSchema(),
      engine: engineSchema(),
      profile: profileSchema(),
      headless: { type: "boolean", default: true },
      includeObservation: { type: "boolean" },
      maxTextChars: { type: "number", minimum: 0 },
      maxElements: { type: "number", minimum: 0 }
    }, ["url"])
  },
  {
    name: "solarium.owaspAudit",
    description: "Run a passive OWASP-mapped browser audit for an authorized page.",
    inputSchema: objectSchema({
      url: { type: "string" },
      scope: scopeSchema(),
      owaspProfile: { type: "string", enum: ["passive", "strict-headers", "active-authorized", "top10-passive", "top10-active-authorized", "top10-passive", "top10-active-authorized"], default: "passive" },
      maxActiveRequests: { type: "number", minimum: 0, maximum: 25 },
      activeDelayMs: { type: "number", minimum: 0 },
      activeRequestTimeoutMs: { type: "number", minimum: 1000 },
      engine: engineSchema(),
      profile: profileSchema(),
      headless: { type: "boolean", default: true },
      outputPath: { type: "string" },
      maxTextChars: { type: "number", minimum: 0 },
      maxElements: { type: "number", minimum: 0 }
    }, ["url"])
  },
  {
    name: "solarium.graphqlAudit",
    description: "Run bounded non-DoS GraphQL endpoint and schema security checks for an authorized target.",
    inputSchema: objectSchema({
      url: { type: "string" },
      scope: scopeSchema(),
      endpoint: { type: "string" },
      outputPath: { type: "string" },
      timeoutMs: { type: "number", minimum: 0 },
      includeIntrospectionSchema: { type: "boolean" },
      batchCheck: { type: "boolean" },
      safeDataProbes: { type: "boolean" },
      maxEndpoints: { type: "number", minimum: 0 }
    }, ["url", "scope"])
  },
  {
    name: "solarium.scopeCheck",
    description: "Validate whether a URL is allowed by a Solarium scope policy.",
    inputSchema: objectSchema({ url: { type: "string" }, scope: scopeSchema() }, ["url", "scope"])
  },
  {
    name: "solarium.validate",
    description: "Validate Solarium config JSON for jobs, scopes, or actions.",
    inputSchema: objectSchema({ value: {}, kind: { type: "string", enum: ["job", "scope", "actions", "auto"] } }, ["value"])
  },
  {
    name: "solarium.profiles",
    description: "List built-in browser profiles.",
    inputSchema: objectSchema({}, [])
  },
  {
    name: "solarium.profile",
    description: "Show one built-in browser profile.",
    inputSchema: objectSchema({ name: profileSchema() }, ["name"])
  },
  {
    name: "solarium.replay",
    description: "Summarize a JSONL Solarium event timeline from disk.",
    inputSchema: objectSchema({ events: { type: "string" } }, ["events"])
  },
  {
    name: "solarium.workflowSeed",
    description: "Generate a reusable Skiller-style Markdown workflow seed from Solarium action JSON and optional evidence manifest.",
    inputSchema: objectSchema({
      actionsPath: { type: "string" },
      evidencePath: { type: "string" },
      output: { type: "string" },
      name: { type: "string" },
      description: { type: "string" },
      source: { type: "string" }
    }, ["actionsPath"])
  },
  {
    name: "solarium.manifest",
    description: "Create a SHA-256 artifact manifest or standardized Solarium evidence run manifest.",
    inputSchema: objectSchema({
      roots: { type: "array", items: { type: "string" } },
      output: { type: "string" },
      includeHidden: { type: "boolean" },
      maxFileBytes: { type: "number", minimum: 0 },
      evidence: { type: "boolean" },
      runId: { type: "string" },
      kind: { type: "string" },
      status: { type: "string", enum: ["ok", "error", "partial"] },
      url: { type: "string" },
      finalUrl: { type: "string" },
      title: { type: "string" },
      scope: { type: "object" },
      actions: { type: "array", items: { type: "object" } },
      metadata: { type: "object" },
      errors: { type: "array", items: { type: "string" } }
    }, ["roots"])
  }
];

export async function runJsonRpcServer(options: JsonRpcServerOptions = {}): Promise<void> {
  const input = options.input ?? defaultInput;
  const output = options.output ?? defaultOutput;
  const rl = createInterface({ input, crlfDelay: Infinity });

  for await (const line of rl) {
    const trimmed = line.trim();
    if (!trimmed) continue;

    let request: JsonRpcRequest;
    try {
      request = JSON.parse(trimmed) as JsonRpcRequest;
    } catch (error) {
      writeResponse(output, failure(null, -32700, "Parse error", messageOf(error)));
      continue;
    }

    const id = request.id ?? null;
    const isNotification = request.id === undefined;
    try {
      const result = await handleJsonRpcRequest(request, options);
      if (!isNotification) writeResponse(output, { jsonrpc: "2.0", id, result });
    } catch (error) {
      if (!isNotification) writeResponse(output, failure(id, errorCode(error), messageOf(error)));
    }
  }
}

export async function handleJsonRpcRequest(request: JsonRpcRequest, options: JsonRpcServerOptions = {}): Promise<unknown> {
  if (request.jsonrpc !== undefined && request.jsonrpc !== "2.0") {
    throw rpcError(-32600, "Invalid Request: jsonrpc must be 2.0");
  }
  if (!request.method || typeof request.method !== "string") {
    throw rpcError(-32600, "Invalid Request: method is required");
  }

  switch (request.method) {
    case "initialize": {
      const params = request.params === undefined ? {} : expectObject(request.params, "params");
      const requestedProtocolVersion = typeof params.protocolVersion === "string" ? params.protocolVersion : DEFAULT_PROTOCOL_VERSION;
      return {
        protocolVersion: requestedProtocolVersion,
        serverInfo: { name: options.name ?? SERVER_NAME, version: options.version ?? SERVER_VERSION },
        capabilities: { tools: { listChanged: false } },
        instructions: SERVER_INSTRUCTIONS
      };
    }
    case "notifications/initialized":
      return {};
    case "ping":
      return {};
    case "tools/list":
      return { tools };
    case "tools/call": {
      const params = expectObject(request.params, "params");
      const name = requireString(params.name, "params.name");
      const args = params.arguments === undefined ? {} : expectObject(params.arguments, "params.arguments");
      const result = await callTool(name, args);
      return toolResult(result);
    }
    default: {
      if (request.method.startsWith("solarium.")) {
        return callTool(request.method, paramsAsArgs(request.params));
      }
      throw rpcError(-32601, `Method not found: ${request.method}`);
    }
  }
}

async function callTool(name: string, args: Record<string, unknown>): Promise<unknown> {
  switch (name) {
    case "solarium.browse":
      return browse({
        url: requireString(args.url, "url"),
        scope: optionalScope(args.scope),
        engine: optionalEngine(args.engine),
        profile: optionalProfile(args.profile),
        headless: optionalBoolean(args.headless, true),
        screenshotPath: optionalString(args.screenshotPath),
        extractText: optionalBoolean(args.extractText, false),
        observe: optionalBoolean(args.observe, false),
        observationOptions: observationOptions(args)
      });
    case "solarium.inspect":
      return inspectPage({
        url: requireString(args.url, "url"),
        scope: optionalScope(args.scope),
        engine: optionalEngine(args.engine),
        profile: optionalProfile(args.profile),
        headless: optionalBoolean(args.headless, true),
        maxCandidates: optionalNumber(args.maxCandidates),
        includeObservation: optionalBoolean(args.includeObservation, false),
        observationOptions: observationOptions(args)
      });
    case "solarium.plan":
      return planActionsFromInspectResult(expectObject(args.inspectResult, "inspectResult") as unknown as InspectResult, {
        goal: optionalString(args.goal),
        maxActions: optionalNumber(args.maxActions),
        fillValue: optionalString(args.fillValue),
        preferKinds: optionalStringArray(args.preferKinds) as never
      });
    case "solarium.session":
      return runActions({
        actions: requireActions(args.actions),
        scope: optionalScope(args.scope),
        engine: optionalEngine(args.engine),
        profile: optionalProfile(args.profile),
        headless: optionalBoolean(args.headless, true),
        sessionId: optionalString(args.sessionId),
        evidenceDir: optionalString(args.evidenceDir),
        observeAfterEachAction: optionalBooleanOrUndefined(args.observeAfterEachAction),
        continueOnError: optionalBoolean(args.continueOnError, false),
        observationOptions: observationOptions(args)
      });
    case "solarium.loop":
      return runLoop({
        url: requireString(args.url, "url"),
        scope: optionalScope(args.scope),
        goal: optionalString(args.goal),
        engine: optionalEngine(args.engine),
        profile: optionalProfile(args.profile),
        headless: optionalBoolean(args.headless, true),
        maxIterations: optionalNumber(args.maxIterations),
        actionsPerIteration: optionalNumber(args.actionsPerIteration),
        evidenceDir: optionalString(args.evidenceDir),
        includeObservations: optionalBoolean(args.includeObservations, false),
        screenshotEachIteration: optionalBoolean(args.screenshotEachIteration, false),
        stopWhenText: optionalString(args.stopWhenText),
        stopWhenUrl: optionalString(args.stopWhenUrl),
        stopWhenSelector: optionalString(args.stopWhenSelector),
        continueOnError: optionalBoolean(args.continueOnError, false)
      });
    case "solarium.crawl":
      return crawl({
        startUrl: requireString(args.startUrl, "startUrl"),
        scope: requiredScope(args.scope),
        engine: optionalEngine(args.engine),
        profile: optionalProfile(args.profile),
        headless: optionalBoolean(args.headless, true),
        maxPages: optionalNumber(args.maxPages),
        maxDepth: optionalNumber(args.maxDepth),
        includeObservations: optionalBoolean(args.includeObservations, false),
        evidenceDir: optionalString(args.evidenceDir),
        screenshots: optionalBoolean(args.screenshots, false)
      });
    case "solarium.audit":
      return audit({
        url: requireString(args.url, "url"),
        scope: optionalScope(args.scope),
        engine: optionalEngine(args.engine),
        profile: optionalProfile(args.profile),
        headless: optionalBoolean(args.headless, true),
        includeObservation: optionalBoolean(args.includeObservation, false),
        observationOptions: observationOptions(args)
      });
    case "solarium.owaspAudit":
      return owaspAudit({
        url: requireString(args.url, "url"),
        scope: optionalScope(args.scope),
        owaspProfile: optionalOwaspProfile(args.owaspProfile),
        engine: optionalEngine(args.engine),
        profile: optionalProfile(args.profile),
        headless: optionalBoolean(args.headless, true),
        outputPath: optionalString(args.outputPath),
        maxActiveRequests: optionalNumber(args.maxActiveRequests),
        activeDelayMs: optionalNumber(args.activeDelayMs),
        activeRequestTimeoutMs: optionalNumber(args.activeRequestTimeoutMs),
        observationOptions: observationOptions(args)
      });
    case "solarium.graphqlAudit":
      return graphqlAudit({
        url: requireString(args.url, "url"),
        scope: requiredScope(args.scope),
        endpoint: optionalString(args.endpoint),
        outputPath: optionalString(args.outputPath),
        timeoutMs: optionalNumber(args.timeoutMs),
        maxEndpoints: optionalNumber(args.maxEndpoints),
        includeIntrospectionSchema: optionalBoolean(args.includeIntrospectionSchema, false),
        batchCheck: optionalBooleanOrUndefined(args.batchCheck),
        safeDataProbes: optionalBoolean(args.safeDataProbes, false)
      });
    case "solarium.scopeCheck":
      return checkUrlScope(requireString(args.url, "url"), requiredScope(args.scope));
    case "solarium.validate":
      return validateSolariumConfig(args.value, { kind: optionalString(args.kind) as never });
    case "solarium.profiles":
      return listBuiltInProfiles();
    case "solarium.profile":
      return requireBuiltInProfile(requireString(args.name, "name") as BrowserProfileName);
    case "solarium.replay":
      return replayEvents(requireString(args.events, "events"));
    case "solarium.workflowSeed":
      return {
        markdown: await createWorkflowSeedFromFiles({
          actionsPath: requireString(args.actionsPath, "actionsPath"),
          evidencePath: optionalString(args.evidencePath),
          output: optionalString(args.output),
          name: optionalString(args.name),
          description: optionalString(args.description),
          source: optionalString(args.source)
        })
      };
    case "solarium.manifest": {
      const roots = requireStringArray(args.roots, "roots");
      if (optionalBoolean(args.evidence, false)) {
        return createEvidenceRunManifest({
          roots,
          output: optionalString(args.output),
          includeHidden: optionalBoolean(args.includeHidden, false),
          maxFileBytes: optionalNumber(args.maxFileBytes),
          runId: optionalString(args.runId) ?? `run-${Date.now()}`,
          kind: (optionalString(args.kind) ?? "manual") as never,
          status: optionalString(args.status) as never,
          url: optionalString(args.url),
          finalUrl: optionalString(args.finalUrl),
          title: optionalString(args.title),
          scope: args.scope,
          actions: args.actions === undefined ? undefined : requireActions(args.actions),
          metadata: args.metadata === undefined ? undefined : expectObject(args.metadata, "metadata"),
          errors: optionalStringArray(args.errors)
        });
      }
      return createArtifactManifest({
        roots,
        output: optionalString(args.output),
        includeHidden: optionalBoolean(args.includeHidden, false),
        maxFileBytes: optionalNumber(args.maxFileBytes)
      });
    }
    default:
      throw rpcError(-32601, `Unknown tool: ${name}`);
  }
}

function toolResult(result: unknown): Record<string, unknown> {
  return {
    content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
    structuredContent: result,
    isError: false
  };
}

function paramsAsArgs(params: unknown): Record<string, unknown> {
  if (params === undefined) return {};
  if (Array.isArray(params)) throw rpcError(-32602, "Positional params are not supported; use named object params");
  return expectObject(params, "params");
}

function writeResponse(output: Writable, response: JsonRpcSuccess | JsonRpcFailure): void {
  output.write(`${JSON.stringify(response)}\n`);
}

function failure(id: string | number | null, code: number, message: string, data?: unknown): JsonRpcFailure {
  return { jsonrpc: "2.0", id, error: { code, message, ...(data === undefined ? {} : { data }) } };
}

function rpcError(code: number, message: string): Error & { code: number } {
  const error = new Error(message) as Error & { code: number };
  error.code = code;
  return error;
}

function errorCode(error: unknown): number {
  return typeof error === "object" && error !== null && "code" in error && typeof (error as { code: unknown }).code === "number"
    ? (error as { code: number }).code
    : -32603;
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function objectSchema(properties: Record<string, unknown>, required: string[]): Record<string, unknown> {
  return { type: "object", properties, required, additionalProperties: true };
}

function scopeSchema(): Record<string, unknown> {
  return {
    type: "object",
    properties: {
      allowedHosts: { type: "array", items: { type: "string" } },
      blockedHosts: { type: "array", items: { type: "string" } },
      maxRequestsPerMinute: { type: "number", minimum: 0 },
      authorizationNote: { type: "string" }
    },
    additionalProperties: true
  };
}

function engineSchema(): Record<string, unknown> {
  return { type: "string", enum: ["chromium", "firefox", "webkit"] };
}

function profileSchema(): Record<string, unknown> {
  return { type: "string", enum: ["chrome-stable", "firefox-stable", "safari-desktop", "edge-stable", "generic-desktop"] };
}

function expectObject(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw rpcError(-32602, `${label} must be an object`);
  return value as Record<string, unknown>;
}

function requireString(value: unknown, label: string): string {
  if (typeof value !== "string" || value.trim().length === 0) throw rpcError(-32602, `${label} must be a non-empty string`);
  return value;
}

function optionalString(value: unknown): string | undefined {
  if (value === undefined || value === null) return undefined;
  return requireString(value, "string value");
}

function optionalBoolean(value: unknown, fallback: boolean): boolean {
  if (value === undefined || value === null) return fallback;
  if (typeof value !== "boolean") throw rpcError(-32602, "Expected boolean value");
  return value;
}

function optionalBooleanOrUndefined(value: unknown): boolean | undefined {
  if (value === undefined || value === null) return undefined;
  if (typeof value !== "boolean") throw rpcError(-32602, "Expected boolean value");
  return value;
}

function optionalNumber(value: unknown): number | undefined {
  if (value === undefined || value === null) return undefined;
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) throw rpcError(-32602, "Expected non-negative number value");
  return value;
}

function optionalEngine(value: unknown): BrowserEngine | undefined {
  if (value === undefined || value === null) return undefined;
  if (!["chromium", "firefox", "webkit"].includes(String(value))) throw rpcError(-32602, "engine must be chromium, firefox, or webkit");
  return value as BrowserEngine;
}

function optionalOwaspProfile(value: unknown): "passive" | "strict-headers" | "active-authorized" | "top10-passive" | "top10-active-authorized" | undefined {
  if (value === undefined || value === null) return undefined;
  const profile = requireString(value, "owaspProfile");
  if (profile !== "passive" && profile !== "strict-headers" && profile !== "active-authorized" && profile !== "top10-passive" && profile !== "top10-active-authorized") throw rpcError(-32602, "owaspProfile must be passive, strict-headers, active-authorized, top10-passive, or top10-active-authorized");
  return profile;
}

function optionalProfile(value: unknown): BrowserProfileName | undefined {
  if (value === undefined || value === null) return undefined;
  const stringValue = requireString(value, "profile");
  return stringValue as BrowserProfileName;
}

function optionalScope(value: unknown): ScopePolicy | undefined {
  if (value === undefined || value === null) return undefined;
  return requiredScope(value);
}

function requiredScope(value: unknown): ScopePolicy {
  return validateScopePolicy(value);
}

function requireActions(value: unknown): AgentAction[] {
  if (!Array.isArray(value)) throw rpcError(-32602, "actions must be an array");
  return value as AgentAction[];
}

function optionalStringArray(value: unknown): string[] | undefined {
  if (value === undefined || value === null) return undefined;
  return requireStringArray(value, "array value");
}

function requireStringArray(value: unknown, label: string): string[] {
  if (!Array.isArray(value)) throw rpcError(-32602, `${label} must be an array`);
  return value.map((item, index) => requireString(item, `${label}[${index}]`));
}

function observationOptions(args: Record<string, unknown>): { maxTextChars?: number; maxElements?: number } {
  return {
    maxTextChars: optionalNumber(args.maxTextChars),
    maxElements: optionalNumber(args.maxElements)
  };
}

function requireBuiltInProfile(name: BrowserProfileName): NonNullable<ReturnType<typeof getBuiltInProfile>> {
  const profile = getBuiltInProfile(name);
  if (!profile) throw rpcError(-32602, `Unknown browser profile: ${name}`);
  return profile;
}
