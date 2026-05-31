import { readFile } from "node:fs/promises";
import { validateSolariumJob } from "../config/job.js";
import { validateScopePolicy } from "../security/scope.js";
import type { AgentAction } from "../types.js";

export type SolariumValidationKind = "job" | "scope" | "actions" | "auto";

export interface SolariumValidationIssue {
  path: string;
  message: string;
}

export interface SolariumValidationResult {
  ok: boolean;
  kind: Exclude<SolariumValidationKind, "auto">;
  path?: string;
  issues: SolariumValidationIssue[];
}

export async function validateSolariumFile(path: string, kind: SolariumValidationKind = "auto"): Promise<SolariumValidationResult> {
  const raw = await readFile(path, "utf8");
  const parsed = JSON.parse(raw) as unknown;
  return validateSolariumConfig(parsed, { kind, path });
}

export function validateSolariumConfig(
  value: unknown,
  options: { kind?: SolariumValidationKind; path?: string } = {}
): SolariumValidationResult {
  const kind = options.kind === "auto" || !options.kind ? inferKind(value) : options.kind;
  const issues: SolariumValidationIssue[] = [];

  try {
    switch (kind) {
      case "job":
        validateSolariumJob(value);
        validateJobShape(value, issues);
        break;
      case "scope":
        validateScopePolicy(value);
        break;
      case "actions":
        validateActions(value);
        break;
    }
  } catch (error) {
    issues.push({ path: "$", message: error instanceof Error ? error.message : String(error) });
  }

  return {
    ok: issues.length === 0,
    kind,
    path: options.path,
    issues
  };
}

function inferKind(value: unknown): Exclude<SolariumValidationKind, "auto"> {
  if (Array.isArray(value)) return "actions";
  if (value && typeof value === "object") {
    const candidate = value as Record<string, unknown>;
    if (typeof candidate.mode === "string") return "job";
    if (candidate.allowedHosts !== undefined || candidate.blockedHosts !== undefined || candidate.authorizationNote !== undefined) return "scope";
  }
  return "job";
}

function validateJobShape(value: unknown, issues: SolariumValidationIssue[]): void {
  if (!value || typeof value !== "object" || Array.isArray(value)) return;
  const job = value as Record<string, unknown>;
  const mode = job.mode;

  if ((mode === "browse" || mode === "inspect" || mode === "audit" || mode === "owasp-audit" || mode === "graphql-audit" || mode === "loop") && !isNonEmptyString(job.url)) {
    issues.push({ path: "$.url", message: `${String(mode)} jobs require a non-empty url` });
  }

  if (mode === "crawl" && !isNonEmptyString(job.url) && !isNonEmptyString(job.startUrl)) {
    issues.push({ path: "$.url", message: "crawl jobs require url or startUrl" });
  }

  if (mode === "crawl" && !job.scope && !isNonEmptyString(job.scopePath)) {
    issues.push({ path: "$.scopePath", message: "crawl jobs require scope or scopePath" });
  }

  if (mode === "owasp-audit" && job.options && typeof job.options === "object" && "owaspProfile" in job.options && !["passive", "strict-headers", "active-authorized", "top10-passive", "top10-active-authorized", "top10-passive", "top10-active-authorized"].includes(String((job.options as Record<string, unknown>).owaspProfile))) {
    issues.push({ path: "$.options.owaspProfile", message: "owaspProfile must be passive, strict-headers, active-authorized, top10-passive, or top10-active-authorized" });
  }

  if (mode === "graphql-audit" && !job.scope && !isNonEmptyString(job.scopePath)) {
    issues.push({ path: "$.scopePath", message: "graphql-audit jobs require scope or scopePath" });
  }

  if (mode === "session" && !Array.isArray(job.actions) && !isNonEmptyString(job.actionsPath)) {
    issues.push({ path: "$.actionsPath", message: "session jobs require actions or actionsPath" });
  }

  if (mode === "plan" && !job.inspectResult && !isNonEmptyString(job.inspectResultPath)) {
    issues.push({ path: "$.inspectResultPath", message: "plan jobs require inspectResult or inspectResultPath" });
  }

  if (mode === "replay") {
    const optionEvents = job.options && typeof job.options === "object" ? (job.options as Record<string, unknown>).events : undefined;
    if (!isNonEmptyString(job.events) && !isNonEmptyString(optionEvents)) {
      issues.push({ path: "$.events", message: "replay jobs require events or options.events" });
    }
  }

  if (job.actions !== undefined) {
    try {
      validateActions(job.actions);
    } catch (error) {
      issues.push({ path: "$.actions", message: error instanceof Error ? error.message : String(error) });
    }
  }

  if (job.scope !== undefined) {
    try {
      validateScopePolicy(job.scope);
    } catch (error) {
      issues.push({ path: "$.scope", message: error instanceof Error ? error.message : String(error) });
    }
  }
}

export function validateActions(value: unknown): AgentAction[] {
  if (!Array.isArray(value)) {
    throw new Error("Actions config must be a JSON array");
  }

  return value.map((action, index) => validateAction(action, index));
}

function validateAction(action: unknown, index: number): AgentAction {
  if (!action || typeof action !== "object" || Array.isArray(action)) {
    throw new Error(`Action ${index} must be an object`);
  }
  const candidate = action as Record<string, unknown>;

  switch (candidate.type) {
    case "navigate":
      requireString(candidate.url, `Action ${index}.url`);
      return candidate as AgentAction;
    case "click":
    case "dblclick":
    case "hover":
      requireString(candidate.selector, `Action ${index}.selector`);
      return candidate as AgentAction;
    case "type":
      requireString(candidate.selector, `Action ${index}.selector`);
      requireString(candidate.text, `Action ${index}.text`);
      return candidate as AgentAction;
    case "press":
      requireString(candidate.selector, `Action ${index}.selector`);
      requireString(candidate.key, `Action ${index}.key`);
      return candidate as AgentAction;
    case "select":
      requireString(candidate.selector, `Action ${index}.selector`);
      if (Array.isArray(candidate.values)) {
        candidate.values.forEach((value, valueIndex) => requireString(value, `Action ${index}.values[${valueIndex}]`));
      } else {
        requireString(candidate.values, `Action ${index}.values`);
      }
      return candidate as AgentAction;
    case "check":
    case "uncheck":
    case "submit":
      requireString(candidate.selector, `Action ${index}.selector`);
      return candidate as AgentAction;
    case "upload":
      requireString(candidate.selector, `Action ${index}.selector`);
      if (Array.isArray(candidate.files)) {
        candidate.files.forEach((file, fileIndex) => requireString(file, `Action ${index}.files[${fileIndex}]`));
      } else {
        requireString(candidate.files, `Action ${index}.files`);
      }
      return candidate as AgentAction;
    case "download":
      requireString(candidate.selector, `Action ${index}.selector`);
      if (candidate.path !== undefined) requireString(candidate.path, `Action ${index}.path`);
      if (candidate.timeoutMs !== undefined) requireNonNegativeNumber(candidate.timeoutMs, `Action ${index}.timeoutMs`);
      return candidate as AgentAction;
    case "wait":
      requireNonNegativeNumber(candidate.ms, `Action ${index}.ms`);
      return candidate as AgentAction;
    case "waitForSelector":
      requireString(candidate.selector, `Action ${index}.selector`);
      if (candidate.state !== undefined && !["attached", "detached", "visible", "hidden"].includes(String(candidate.state))) {
        throw new Error(`Action ${index}.state must be attached, detached, visible, or hidden`);
      }
      if (candidate.timeoutMs !== undefined) requireNonNegativeNumber(candidate.timeoutMs, `Action ${index}.timeoutMs`);
      return candidate as AgentAction;
    case "waitForUrl":
      requireString(candidate.url, `Action ${index}.url`);
      if (candidate.timeoutMs !== undefined) requireNonNegativeNumber(candidate.timeoutMs, `Action ${index}.timeoutMs`);
      return candidate as AgentAction;
    case "screenshot":
      if (candidate.path !== undefined) requireString(candidate.path, `Action ${index}.path`);
      return candidate as AgentAction;
    case "extract":
      if (candidate.selector !== undefined) requireString(candidate.selector, `Action ${index}.selector`);
      if (candidate.format !== undefined && !["text", "html", "markdown"].includes(String(candidate.format))) {
        throw new Error(`Action ${index}.format must be text, html, or markdown`);
      }
      return candidate as AgentAction;
    case "observe":
      return candidate as AgentAction;
    default:
      throw new Error(`Unsupported action type at index ${index}: ${String(candidate.type)}`);
  }
}

function requireString(value: unknown, label: string): void {
  if (!isNonEmptyString(value)) throw new Error(`${label} must be a non-empty string`);
}

function requireNonNegativeNumber(value: unknown, label: string): void {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
    throw new Error(`${label} must be a non-negative number`);
  }
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}
