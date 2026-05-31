import { mkdir, writeFile, appendFile } from "node:fs/promises";
import { dirname } from "node:path";
import type { AgentAction, AgentStepResult, CrawlPageResult, NetworkPolicySummary } from "../types.js";

export type EventLogEventType =
  | "session.started"
  | "session.step.started"
  | "session.step.finished"
  | "session.finished"
  | "crawl.started"
  | "crawl.page.started"
  | "crawl.page.finished"
  | "crawl.finished"
  | "network.policy.summary"
  | "error";

export interface EventLogEvent<TPayload extends Record<string, unknown> = Record<string, unknown>> {
  type: EventLogEventType | string;
  timestamp: string;
  payload: TPayload;
}

export interface EventLogger {
  emit<TPayload extends Record<string, unknown>>(type: EventLogEventType | string, payload?: TPayload): Promise<void>;
  close(): Promise<void>;
}

export class JsonlEventLogger implements EventLogger {
  private initialized = false;

  constructor(private readonly path: string) {}

  async emit<TPayload extends Record<string, unknown>>(
    type: EventLogEventType | string,
    payload = {} as TPayload
  ): Promise<void> {
    await this.ensureInitialized();
    const event: EventLogEvent<TPayload> = {
      type,
      timestamp: new Date().toISOString(),
      payload
    };
    await appendFile(this.path, `${JSON.stringify(event)}\n`, "utf8");
  }

  async close(): Promise<void> {
    // Reserved for future streaming file handles. appendFile keeps this implementation simple and robust.
  }

  private async ensureInitialized(): Promise<void> {
    if (this.initialized) return;
    await mkdir(dirname(this.path), { recursive: true });
    await writeFile(this.path, "", "utf8");
    this.initialized = true;
  }
}

export function createJsonlEventLogger(path?: string): EventLogger | undefined {
  return path ? new JsonlEventLogger(path) : undefined;
}

export function summarizeAgentAction(action: AgentAction): Record<string, unknown> {
  switch (action.type) {
    case "navigate":
      return { type: action.type, url: action.url, waitUntil: action.waitUntil, timeoutMs: action.timeoutMs };
    case "click":
    case "dblclick":
    case "hover":
      return { type: action.type, selector: action.selector };
    case "type":
      return { type: action.type, selector: action.selector, textLength: action.text.length };
    case "press":
      return { type: action.type, selector: action.selector, key: action.key };
    case "select":
      return {
        type: action.type,
        selector: action.selector,
        valueCount: Array.isArray(action.values) ? action.values.length : 1
      };
    case "check":
    case "uncheck":
    case "submit":
      return { type: action.type, selector: action.selector };
    case "upload":
      return {
        type: action.type,
        selector: action.selector,
        fileCount: Array.isArray(action.files) ? action.files.length : 1
      };
    case "download":
      return { type: action.type, selector: action.selector, path: action.path, timeoutMs: action.timeoutMs };
    case "wait":
      return { type: action.type, ms: action.ms };
    case "waitForSelector":
      return { type: action.type, selector: action.selector, state: action.state, timeoutMs: action.timeoutMs };
    case "waitForUrl":
      return { type: action.type, url: action.url, timeoutMs: action.timeoutMs };
    case "screenshot":
      return { type: action.type, path: action.path, fullPage: action.fullPage };
    case "extract":
      return { type: action.type, selector: action.selector, format: action.format };
    case "observe":
      return { type: action.type };
    default: {
      const exhaustive: never = action;
      return { type: "unsupported", action: exhaustive };
    }
  }
}

export function summarizeAgentStep(step: AgentStepResult): Record<string, unknown> {
  return {
    index: step.index,
    ok: step.ok,
    action: summarizeAgentAction(step.action),
    startedAt: step.startedAt,
    finishedAt: step.finishedAt,
    url: step.url,
    title: step.title,
    screenshotPath: step.screenshotPath,
    extracted: step.extracted
      ? {
          format: step.extracted.format,
          selector: step.action.type === "extract" ? step.action.selector : undefined,
          contentLength: step.extracted.content.length
        }
      : undefined,
    download: step.download
      ? {
          suggestedFilename: step.download.suggestedFilename,
          path: step.download.path,
          url: step.download.url,
          failure: step.download.failure
        }
      : undefined,
    observation: step.observation
      ? {
          links: step.observation.links.length,
          buttons: step.observation.buttons.length,
          inputs: step.observation.inputs.length,
          forms: step.observation.forms.length,
          consoleEvents: step.observation.console.length,
          networkEvents: step.observation.network.length
        }
      : undefined,
    error: step.error
  };
}

export function summarizeCrawlPage(page: CrawlPageResult): Record<string, unknown> {
  return {
    url: page.url,
    finalUrl: page.finalUrl,
    title: page.title,
    depth: page.depth,
    ok: page.ok,
    discoveredLinks: page.discoveredLinks.length,
    forms: page.forms.length,
    observationPath: page.observationPath,
    screenshotPath: page.screenshotPath,
    startedAt: page.startedAt,
    finishedAt: page.finishedAt,
    networkPolicy: page.networkPolicy,
    error: page.error
  };
}

export function summarizeNetworkPolicy(summary?: NetworkPolicySummary): Record<string, unknown> | undefined {
  return summary ? { ...summary } : undefined;
}
