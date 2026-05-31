import { writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { mkdir } from "node:fs/promises";
import { assertUrlInScope, checkUrlScope, type ScopePolicy } from "./scope.js";
import type { AuditFinding, AuditSeverity, NetworkPolicySummary } from "../types.js";

export interface GraphqlAuditOptions {
  url: string;
  scope?: ScopePolicy;
  endpoint?: string;
  outputPath?: string;
  timeoutMs?: number;
  includeIntrospectionSchema?: boolean;
  batchCheck?: boolean;
  safeDataProbes?: boolean;
  maxEndpoints?: number;
}

export interface GraphqlEndpointProbe {
  url: string;
  ok: boolean;
  status?: number;
  error?: string;
  typename?: string;
}

export interface GraphqlOperationInventory {
  queryFields: string[];
  mutationFields: string[];
  subscriptionFields: string[];
  sensitiveFields: string[];
  dangerousFields: string[];
}

export interface GraphqlAuditResult {
  url: string;
  endpoint?: string;
  startedAt: string;
  finishedAt: string;
  ok: boolean;
  endpointCandidates: string[];
  endpointProbes: GraphqlEndpointProbe[];
  findings: AuditFinding[];
  summary: Record<AuditSeverity, number>;
  inventory?: GraphqlOperationInventory;
  introspectionSchema?: unknown;
  networkPolicy?: NetworkPolicySummary;
  error?: string;
}

interface GraphqlResponse {
  data?: unknown;
  errors?: Array<{ message?: string; [key: string]: unknown }>;
  [key: string]: unknown;
}

const DEFAULT_TIMEOUT_MS = 8_000;
const SENSITIVE_FIELD_PATTERNS = [/password/i, /token/i, /secret/i, /credential/i, /api[_-]?key/i, /session/i];
const DANGEROUS_FIELD_PATTERNS = [/delete/i, /debug/i, /diagnostic/i, /system/i, /import/i, /upload/i, /exec/i, /command/i, /update/i];

const INTROSPECTION_QUERY = `
query SolariumIntrospection {
  __schema {
    queryType { name fields { name args { name type { kind name ofType { kind name ofType { kind name } } } } } }
    mutationType { name fields { name args { name type { kind name ofType { kind name ofType { kind name } } } } } }
    subscriptionType { name fields { name args { name type { kind name ofType { kind name ofType { kind name } } } } } }
    types { name kind fields { name } }
  }
}`;

export async function graphqlAudit(options: GraphqlAuditOptions): Promise<GraphqlAuditResult> {
  assertUrlInScope(options.url, options.scope);
  const startedAt = new Date().toISOString();
  const findings: AuditFinding[] = [];
  const endpointCandidates = buildEndpointCandidates(options.url, options.endpoint, options.maxEndpoints ?? 6);
  const endpointProbes: GraphqlEndpointProbe[] = [];
  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  let selectedEndpoint: string | undefined;
  let inventory: GraphqlOperationInventory | undefined;
  let introspectionSchema: unknown;

  try {
    for (const candidate of endpointCandidates) {
      assertUrlInScope(candidate, options.scope);
      const probe = await probeEndpoint(candidate, timeoutMs);
      endpointProbes.push(probe);
      if (probe.ok && !selectedEndpoint) selectedEndpoint = candidate;
      await politeDelay(options.scope);
    }

    if (!selectedEndpoint) {
      findings.push({
        id: "graphql-endpoint-not-detected",
        category: "graphql",
        severity: "info",
        title: "GraphQL endpoint was not detected",
        description: "Solarium did not find a candidate endpoint that answered a minimal GraphQL __typename query.",
        recommendation: "If the application uses GraphQL, provide the endpoint explicitly with --endpoint.",
        evidence: { candidates: endpointCandidates, probes: endpointProbes }
      });
      return finishResult(options, startedAt, endpointCandidates, endpointProbes, findings, undefined, undefined, undefined);
    }

    findings.push({
      id: "graphql-endpoint-detected",
      category: "graphql",
      severity: "info",
      title: "GraphQL endpoint detected",
      description: "The endpoint answered a minimal GraphQL query.",
      recommendation: "Ensure GraphQL authorization, query complexity, introspection, batching, and logging policies are appropriate for the environment.",
      evidence: { endpoint: selectedEndpoint }
    });

    const introspection = await postGraphql(selectedEndpoint, { query: INTROSPECTION_QUERY }, timeoutMs);
    await politeDelay(options.scope);
    if (introspection.json?.data && hasSchema(introspection.json.data)) {
      introspectionSchema = options.includeIntrospectionSchema ? introspection.json.data : undefined;
      inventory = buildInventory(introspection.json.data);
      findings.push({
        id: "graphql-introspection-enabled",
        category: "graphql",
        severity: "medium",
        title: "GraphQL introspection is enabled",
        description: "The endpoint returned schema metadata through __schema introspection.",
        recommendation: "Disable introspection in production or restrict it to authenticated/authorized development users.",
        evidence: {
          endpoint: selectedEndpoint,
          queryFields: inventory.queryFields,
          mutationFields: inventory.mutationFields,
          subscriptionFields: inventory.subscriptionFields
        }
      });

      if (inventory.sensitiveFields.length > 0) {
        findings.push({
          id: "graphql-sensitive-field-names-exposed",
          category: "graphql",
          severity: "medium",
          title: "Sensitive-looking GraphQL field names are exposed",
          description: "The introspected schema contains fields whose names suggest secrets, credentials, tokens, passwords, or sessions.",
          recommendation: "Remove sensitive fields from public schemas or enforce strict field-level authorization and redaction.",
          evidence: { fields: inventory.sensitiveFields }
        });
      }

      if (inventory.dangerousFields.length > 0) {
        findings.push({
          id: "graphql-dangerous-operation-surface",
          category: "graphql",
          severity: "high",
          title: "Potentially dangerous GraphQL operations are exposed",
          description: "The schema exposes operation names associated with destructive, debug, system, import, upload, update, or command-like behavior.",
          recommendation: "Require strong authorization for administrative/system operations and remove debug/test operations from deployed environments.",
          evidence: { fields: inventory.dangerousFields }
        });
      }
    } else {
      findings.push({
        id: "graphql-introspection-not-confirmed",
        category: "graphql",
        severity: "info",
        title: "GraphQL introspection was not confirmed",
        description: "The endpoint did not return a usable __schema response to the standard introspection probe.",
        recommendation: "If introspection is intentionally disabled, ensure field suggestions and documentation endpoints do not disclose equivalent schema details.",
        evidence: responseEvidence(introspection)
      });
    }

    const getUrl = new URL(selectedEndpoint);
    getUrl.searchParams.set("query", "query SolariumTypename { __typename }");
    const getProbe = await httpJson(getUrl.toString(), { method: "GET", timeoutMs });
    await politeDelay(options.scope);
    if (getProbe.json?.data && typeof (getProbe.json.data as Record<string, unknown>).__typename === "string") {
      findings.push({
        id: "graphql-get-query-enabled",
        category: "graphql",
        severity: "low",
        title: "GraphQL queries are accepted over GET",
        description: "The endpoint executes GraphQL queries supplied in URL parameters.",
        recommendation: "Prefer POST for GraphQL operations and ensure URL logging/caching/referrer controls do not expose sensitive queries.",
        evidence: { endpoint: selectedEndpoint, status: getProbe.status }
      });
    }

    const suggestion = await postGraphql(selectedEndpoint, { query: "query SolariumSuggestion { __doesNotExist }" }, timeoutMs);
    await politeDelay(options.scope);
    const suggestionMessages = messages(suggestion.json);
    if (suggestionMessages.some((message) => /Did you mean/i.test(message))) {
      findings.push({
        id: "graphql-field-suggestions-enabled",
        category: "graphql",
        severity: "low",
        title: "GraphQL field suggestions are enabled",
        description: "Invalid field queries return suggestions for valid schema fields.",
        recommendation: "Consider disabling suggestions in production error responses when schema secrecy matters.",
        evidence: { endpoint: selectedEndpoint, messages: suggestionMessages.slice(0, 5) }
      });
    }

    if (options.batchCheck ?? true) {
      const batch = await httpJson(selectedEndpoint, {
        method: "POST",
        timeoutMs,
        body: JSON.stringify([
          { query: "query SolariumBatchA { __typename }" },
          { query: "query SolariumBatchB { __typename }" }
        ]),
        headers: { "content-type": "application/json", accept: "application/json" }
      });
      await politeDelay(options.scope);
      if (Array.isArray(batch.json) && batch.json.length === 2) {
        findings.push({
          id: "graphql-batching-enabled",
          category: "graphql",
          severity: "medium",
          title: "GraphQL batching is enabled",
          description: "The endpoint accepted an array of GraphQL operations in one HTTP request.",
          recommendation: "Disable batching unless required, and enforce per-operation authorization, cost limits, and batch size limits.",
          evidence: { endpoint: selectedEndpoint, responseCount: batch.json.length }
        });
      }
    }

    if (options.safeDataProbes && inventory) {
      await runSafeDataProbes(selectedEndpoint, inventory, timeoutMs, findings, options.scope);
    }

    return finishResult(options, startedAt, endpointCandidates, endpointProbes, findings, selectedEndpoint, inventory, introspectionSchema);
  } catch (error) {
    return finishResult(
      options,
      startedAt,
      endpointCandidates,
      endpointProbes,
      findings,
      selectedEndpoint,
      inventory,
      introspectionSchema,
      error instanceof Error ? error.message : String(error)
    );
  }
}

function buildEndpointCandidates(startUrl: string, explicitEndpoint: string | undefined, maxEndpoints: number): string[] {
  const candidates = new Set<string>();
  if (explicitEndpoint) candidates.add(new URL(explicitEndpoint, startUrl).toString());
  const start = new URL(startUrl);
  candidates.add(start.toString());
  candidates.add(new URL("/graphql", start).toString());
  candidates.add(new URL("/graphiql", start).toString());
  candidates.add(new URL("/api/graphql", start).toString());
  candidates.add(new URL("/v1/graphql", start).toString());
  candidates.add(new URL("/query", start).toString());
  return [...candidates].slice(0, Math.max(1, maxEndpoints));
}

async function probeEndpoint(url: string, timeoutMs: number): Promise<GraphqlEndpointProbe> {
  try {
    const response = await postGraphql(url, { query: "query SolariumTypename { __typename }" }, timeoutMs);
    const typename = response.json?.data && typeof (response.json.data as Record<string, unknown>).__typename === "string"
      ? String((response.json.data as Record<string, unknown>).__typename)
      : undefined;
    return { url, ok: Boolean(typename), status: response.status, typename };
  } catch (error) {
    return { url, ok: false, error: error instanceof Error ? error.message : String(error) };
  }
}

async function postGraphql(url: string, body: unknown, timeoutMs: number): Promise<{ status: number; json?: GraphqlResponse; text: string }> {
  return httpJson(url, {
    method: "POST",
    timeoutMs,
    body: JSON.stringify(body),
    headers: { "content-type": "application/json", accept: "application/json" }
  }) as Promise<{ status: number; json?: GraphqlResponse; text: string }>;
}

async function httpJson(url: string, options: { method: "GET" | "POST"; timeoutMs: number; body?: string; headers?: Record<string, string> }): Promise<{ status: number; json?: any; text: string }> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), options.timeoutMs);
  try {
    const response = await fetch(url, {
      method: options.method,
      body: options.body,
      headers: options.headers,
      signal: controller.signal,
      redirect: "manual"
    });
    const text = await response.text();
    let json: any;
    try {
      json = text ? JSON.parse(text) : undefined;
    } catch {
      json = undefined;
    }
    return { status: response.status, json, text: text.slice(0, 4096) };
  } finally {
    clearTimeout(timeout);
  }
}

function hasSchema(data: unknown): boolean {
  return Boolean(data && typeof data === "object" && "__schema" in data && (data as any).__schema);
}

function buildInventory(data: unknown): GraphqlOperationInventory {
  const schema = (data as any).__schema ?? {};
  const queryFields = fieldNames(schema.queryType);
  const mutationFields = fieldNames(schema.mutationType);
  const subscriptionFields = fieldNames(schema.subscriptionType);
  const allOperationFields = [...queryFields, ...mutationFields, ...subscriptionFields];
  const typeFields = Array.isArray(schema.types)
    ? schema.types.flatMap((type: any) => Array.isArray(type.fields) ? type.fields.map((field: any) => `${type.name}.${field.name}`) : [])
    : [];
  const allFields = [...allOperationFields, ...typeFields];
  return {
    queryFields,
    mutationFields,
    subscriptionFields,
    sensitiveFields: unique(allFields.filter((field) => SENSITIVE_FIELD_PATTERNS.some((pattern) => pattern.test(field)))),
    dangerousFields: unique(allOperationFields.filter((field) => DANGEROUS_FIELD_PATTERNS.some((pattern) => pattern.test(field))))
  };
}

function fieldNames(type: any): string[] {
  return Array.isArray(type?.fields) ? type.fields.map((field: any) => String(field.name)).filter(Boolean).sort() : [];
}

async function runSafeDataProbes(endpoint: string, inventory: GraphqlOperationInventory, timeoutMs: number, findings: AuditFinding[], scope?: ScopePolicy): Promise<void> {
  if (inventory.queryFields.includes("pastes")) {
    const probe = await postGraphql(endpoint, { query: "query SolariumPrivatePastes { pastes(public:false) { id title public } }" }, timeoutMs);
    await politeDelay(scope);
    const rows = ((probe.json?.data as any)?.pastes ?? []) as unknown[];
    if (Array.isArray(rows) && rows.length > 0) {
      findings.push({
        id: "graphql-private-collection-readable",
        category: "graphql",
        severity: "high",
        title: "Private collection data is readable without authentication",
        description: "A read-only probe for pastes(public:false) returned records.",
        recommendation: "Enforce object and collection authorization on GraphQL resolvers before returning private records.",
        evidence: { query: "pastes(public:false)", count: rows.length, sample: rows.slice(0, 3) }
      });
    }
  }

  if (inventory.queryFields.includes("paste")) {
    const probe = await postGraphql(endpoint, { query: "query SolariumPasteObject { paste(id:2) { id title public } }" }, timeoutMs);
    await politeDelay(scope);
    const paste = (probe.json?.data as any)?.paste;
    if (paste && paste.public === false) {
      findings.push({
        id: "graphql-private-object-idor",
        category: "graphql",
        severity: "high",
        title: "Private object is directly readable by ID",
        description: "A direct object lookup returned a private record without authentication.",
        recommendation: "Apply per-object authorization checks to direct lookup resolvers.",
        evidence: { query: "paste(id:2)", sample: paste }
      });
    }
  }

  if (inventory.queryFields.includes("audits")) {
    const probe = await postGraphql(endpoint, { query: "query SolariumAudits { audits { id gqloperation gqlquery timestamp } }" }, timeoutMs);
    await politeDelay(scope);
    const rows = ((probe.json?.data as any)?.audits ?? []) as unknown[];
    if (Array.isArray(rows) && rows.length > 0) {
      findings.push({
        id: "graphql-audit-log-disclosure",
        category: "graphql",
        severity: "high",
        title: "GraphQL audit log is readable without authentication",
        description: "The audits query returned prior GraphQL operations and timestamps.",
        recommendation: "Restrict operational logs to administrators and redact sensitive query variables before storage/display.",
        evidence: { count: rows.length, sample: rows.slice(0, 3) }
      });
    }
  }

  if (inventory.queryFields.includes("users")) {
    const probe = await postGraphql(endpoint, { query: "query SolariumUsers { users { id username password } }" }, timeoutMs);
    await politeDelay(scope);
    const rows = ((probe.json?.data as any)?.users ?? []) as unknown[];
    if (Array.isArray(rows) && rows.length > 0) {
      findings.push({
        id: "graphql-user-password-field-readable",
        category: "graphql",
        severity: "medium",
        title: "User password field is queryable",
        description: "The users query returns a password field. Values may be masked, but the sensitive field is exposed through the API contract.",
        recommendation: "Remove password/hash fields from public GraphQL types and use dedicated administrative APIs where strictly necessary.",
        evidence: { count: rows.length, sample: rows.slice(0, 3) }
      });
    }
  }
}

function messages(json?: GraphqlResponse): string[] {
  return Array.isArray(json?.errors) ? json.errors.map((error) => String(error.message ?? "")).filter(Boolean) : [];
}

function responseEvidence(response: { status: number; json?: unknown; text: string }): Record<string, unknown> {
  return { status: response.status, json: response.json, text: response.text.slice(0, 1024) };
}

function finishResult(
  options: GraphqlAuditOptions,
  startedAt: string,
  endpointCandidates: string[],
  endpointProbes: GraphqlEndpointProbe[],
  findings: AuditFinding[],
  endpoint?: string,
  inventory?: GraphqlOperationInventory,
  introspectionSchemaOrError?: unknown,
  maybeError?: string
): GraphqlAuditResult {
  const error = typeof introspectionSchemaOrError === "string" && maybeError === undefined ? introspectionSchemaOrError : maybeError;
  const introspectionSchema = error ? undefined : introspectionSchemaOrError;
  const result: GraphqlAuditResult = {
    url: options.url,
    endpoint,
    startedAt,
    finishedAt: new Date().toISOString(),
    ok: !error,
    endpointCandidates,
    endpointProbes,
    findings,
    summary: summarize(findings),
    inventory,
    introspectionSchema,
    networkPolicy: summarizeScope(options.scope),
    error
  };
  if (options.outputPath) void writeJsonFile(options.outputPath, result);
  return result;
}

function summarize(findings: AuditFinding[]): Record<AuditSeverity, number> {
  return findings.reduce<Record<AuditSeverity, number>>((acc, finding) => {
    acc[finding.severity] = (acc[finding.severity] ?? 0) + 1;
    return acc;
  }, { info: 0, low: 0, medium: 0, high: 0 });
}

function summarizeScope(scope?: ScopePolicy): NetworkPolicySummary | undefined {
  if (!scope) return undefined;
  return { allowedRequests: 0, blockedRequests: 0, rateLimitedRequests: 0 };
}

async function politeDelay(scope?: ScopePolicy): Promise<void> {
  if (!scope?.maxRequestsPerMinute) return;
  const delayMs = Math.ceil(60_000 / scope.maxRequestsPerMinute);
  await new Promise((resolve) => setTimeout(resolve, Math.min(delayMs, 5_000)));
}

function unique(values: string[]): string[] {
  return [...new Set(values)].sort();
}

async function writeJsonFile(path: string, value: unknown): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, JSON.stringify(value, null, 2), "utf8");
}
