import { writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { mkdir } from "node:fs/promises";
import type { AgentAction, NetworkPolicySummary } from "../types.js";
import { createArtifactManifest, type ArtifactManifest, type ArtifactManifestOptions } from "./artifacts.js";

export type EvidenceRunKind = "browse" | "inspect" | "session" | "loop" | "crawl" | "audit" | "replay" | "manual";

export interface EvidenceManifestOptions extends Omit<ArtifactManifestOptions, "output"> {
  runId: string;
  kind: EvidenceRunKind;
  output?: string;
  startedAt?: string;
  finishedAt?: string;
  status?: "ok" | "error" | "partial";
  url?: string;
  finalUrl?: string;
  title?: string;
  scope?: unknown;
  actions?: AgentAction[];
  networkPolicy?: NetworkPolicySummary;
  metadata?: Record<string, unknown>;
  errors?: string[];
}

export interface EvidenceActionSummary {
  index: number;
  type: AgentAction["type"];
  selector?: string;
  url?: string;
  path?: string;
  timeoutMs?: number;
}

export interface EvidenceRunManifest {
  schemaVersion: "solarium.evidence.v1";
  runId: string;
  kind: EvidenceRunKind;
  generatedAt: string;
  startedAt?: string;
  finishedAt?: string;
  status: "ok" | "error" | "partial";
  target?: {
    url?: string;
    finalUrl?: string;
    title?: string;
  };
  scope?: unknown;
  actions?: EvidenceActionSummary[];
  networkPolicy?: NetworkPolicySummary;
  artifacts: ArtifactManifest;
  errors: string[];
  metadata: Record<string, unknown>;
}

export async function createEvidenceRunManifest(options: EvidenceManifestOptions): Promise<EvidenceRunManifest> {
  const artifacts = await createArtifactManifest({
    roots: options.roots,
    includeHidden: options.includeHidden,
    maxFileBytes: options.maxFileBytes,
    baseDir: options.baseDir
  });

  const manifest: EvidenceRunManifest = {
    schemaVersion: "solarium.evidence.v1",
    runId: options.runId,
    kind: options.kind,
    generatedAt: new Date().toISOString(),
    startedAt: options.startedAt,
    finishedAt: options.finishedAt,
    status: options.status ?? (options.errors?.length ? "error" : "ok"),
    target: options.url || options.finalUrl || options.title ? { url: options.url, finalUrl: options.finalUrl, title: options.title } : undefined,
    scope: options.scope,
    actions: options.actions?.map(summarizeEvidenceAction),
    networkPolicy: options.networkPolicy,
    artifacts,
    errors: options.errors ?? [],
    metadata: options.metadata ?? {}
  };

  if (options.output) {
    await mkdir(dirname(options.output), { recursive: true });
    await writeFile(options.output, JSON.stringify(manifest, null, 2), "utf8");
  }

  return manifest;
}

export function summarizeEvidenceAction(action: AgentAction, index: number): EvidenceActionSummary {
  const compact = (summary: EvidenceActionSummary): EvidenceActionSummary =>
    Object.fromEntries(Object.entries(summary).filter(([, value]) => value !== undefined)) as EvidenceActionSummary;

  switch (action.type) {
    case "navigate":
      return compact({ index, type: action.type, url: action.url, timeoutMs: action.timeoutMs });
    case "waitForUrl":
      return compact({ index, type: action.type, url: action.url, timeoutMs: action.timeoutMs });
    case "click":
    case "dblclick":
    case "hover":
    case "type":
    case "press":
    case "select":
    case "check":
    case "uncheck":
    case "submit":
    case "upload":
    case "download":
    case "waitForSelector":
      return compact({
        index,
        type: action.type,
        selector: "selector" in action ? action.selector : undefined,
        path: "path" in action ? action.path : undefined,
        timeoutMs: "timeoutMs" in action ? action.timeoutMs : undefined
      });
    case "screenshot":
      return compact({ index, type: action.type, path: action.path });
    case "extract":
      return compact({ index, type: action.type, selector: action.selector });
    case "wait":
      return { index, type: action.type };
    case "observe":
      return { index, type: action.type };
    default: {
      const exhaustive: never = action;
      return exhaustive;
    }
  }
}
