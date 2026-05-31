import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { browse } from "../agent/actions.js";
import { inspectPage } from "../agent/inspect.js";
import { runLoop } from "../agent/loop.js";
import { planActionsFromInspectResult } from "../agent/plan.js";
import { runActions } from "../agent/session.js";
import { createJsonlEventLogger } from "../reporting/events.js";
import { replayEvents } from "../reporting/replay.js";
import { renderAuditHtmlReport, renderCrawlHtmlReport, renderGraphqlAuditHtmlReport, renderOwaspAuditHtmlReport, renderLoopHtmlReport, renderSessionHtmlReport } from "../reporting/html.js";
import { renderAuditMarkdownReport, renderCrawlMarkdownReport, renderGraphqlAuditMarkdownReport, renderOwaspAuditMarkdownReport, renderLoopMarkdownReport, renderSessionMarkdownReport } from "../reporting/markdown.js";
import { audit } from "../security/audit.js";
import { graphqlAudit, type GraphqlAuditResult } from "../security/graphql-audit.js";
import { owaspAudit, type OwaspAuditResult } from "../security/owasp-audit.js";
import { crawl } from "../security/crawler.js";
import { readBrowserProfile } from "../browser/profile-store.js";
import { validateScopePolicy, type ScopePolicy } from "../security/scope.js";
import type {
  AgentAction,
  AuditResult,
  BrowserEngine,
  BrowserProfile,
  BrowserProfileName,
  BrowseResult,
  CrawlResult,
  InspectCandidateKind,
  InspectResult,
  LoopResult,
  ObservationOptions,
  AgentSessionResult
} from "../types.js";

export type SolariumJobMode = "browse" | "inspect" | "plan" | "session" | "crawl" | "audit" | "owasp-audit" | "graphql-audit" | "loop" | "replay";

export interface SolariumJob {
  mode: SolariumJobMode;
  url?: string;
  startUrl?: string;
  actions?: AgentAction[];
  actionsPath?: string;
  inspectResult?: InspectResult;
  inspectResultPath?: string;
  scope?: ScopePolicy;
  scopePath?: string;
  output?: string;
  report?: string;
  htmlReport?: string;
  reportIncludeJson?: boolean;
  events?: string;
  engine?: BrowserEngine;
  profile?: BrowserProfileName | BrowserProfile;
  profilePath?: string;
  storageState?: string;
  saveStorageState?: string;
  downloadsDir?: string;
  headed?: boolean;
  trace?: boolean;
  options?: Record<string, unknown>;
}

export interface RunJobOptions {
  jobPath?: string;
  baseDir?: string;
}

export interface RunJobResult {
  mode: SolariumJobMode;
  ok: boolean;
  outputPath?: string;
  reportPath?: string;
  htmlReportPath?: string;
  eventsPath?: string;
  result: unknown;
}

export async function readSolariumJob(path: string): Promise<SolariumJob> {
  const raw = await readFile(path, "utf8");
  return validateSolariumJob(JSON.parse(raw) as unknown);
}

export function validateSolariumJob(value: unknown): SolariumJob {
  if (!value || typeof value !== "object") {
    throw new Error("Solarium job must be a JSON object");
  }

  const job = value as SolariumJob;
  const modes = new Set<SolariumJobMode>(["browse", "inspect", "plan", "session", "crawl", "audit", "owasp-audit", "graphql-audit", "loop", "replay"]);
  if (!modes.has(job.mode)) {
    throw new Error(`Unsupported Solarium job mode: ${String(job.mode)}`);
  }

  if (job.options !== undefined && (!job.options || typeof job.options !== "object" || Array.isArray(job.options))) {
    throw new Error("Solarium job options must be an object when provided");
  }

  return job;
}

export async function runJob(job: SolariumJob, options: RunJobOptions = {}): Promise<RunJobResult> {
  const baseDir = options.baseDir ?? (options.jobPath ? dirname(resolve(options.jobPath)) : process.cwd());
  const opt = job.options ?? {};
  const scope = await resolveScope(job, baseDir);
  const profile = await resolveProfileForJob(job, baseDir);
  const common = {
    engine: job.engine,
    profile,
    headless: !job.headed,
    trace: Boolean(job.trace),
    storageState: resolveOptionalPath(baseDir, job.storageState ?? optionalString(opt.storageState)),
    saveStorageState: resolveOptionalPath(baseDir, job.saveStorageState ?? optionalString(opt.saveStorageState)),
    downloadsDir: resolveOptionalPath(baseDir, job.downloadsDir ?? optionalString(opt.downloadsDir))
  };

  let result: unknown;

  switch (job.mode) {
    case "browse": {
      const url = requireUrl(job);
      result = await browse({
        ...common,
        url,
        scope,
        screenshotPath: optionalString(opt.screenshotPath) ?? optionalString(opt.screenshot),
        extractText: optionalBoolean(opt.extractText),
        observe: optionalBoolean(opt.observe),
        observationPath: resolveOptionalPath(baseDir, optionalString(opt.observationPath) ?? optionalString(opt.observation)),
        observationOptions: observationOptions(opt)
      });
      break;
    }

    case "inspect": {
      const url = requireUrl(job);
      const inspectResult = await inspectPage({
        ...common,
        url,
        scope,
        screenshotPath: resolveOptionalPath(baseDir, optionalString(opt.screenshotPath) ?? optionalString(opt.screenshot)),
        includeObservation: optionalBoolean(opt.includeObservation),
        maxCandidates: optionalNumber(opt.maxCandidates),
        waitAfterNavigationMs: optionalNumber(opt.waitAfterNavigationMs),
        observationOptions: observationOptions(opt)
      });

      const actionsOut = resolveOptionalPath(baseDir, optionalString(opt.actionsOut));
      if (actionsOut) {
        const plan = planActionsFromInspectResult(inspectResult, {
          goal: optionalString(opt.goal),
          maxActions: optionalNumber(opt.planMaxActions),
          fillValue: optionalString(opt.fillValue)
        });
        await writeTextFile(actionsOut, JSON.stringify(plan.actions, null, 2));
      }

      result = inspectResult;
      break;
    }

    case "plan": {
      const inspectResult = await resolveInspectResult(job, baseDir);
      const plan = planActionsFromInspectResult(inspectResult, {
        goal: optionalString(opt.goal),
        maxActions: optionalNumber(opt.maxActions),
        fillValue: optionalString(opt.fillValue),
        includeNavigate: optionalBooleanOrUndefined(opt.includeNavigate),
        includeObserve: optionalBooleanOrUndefined(opt.includeObserve),
        includeScreenshot: optionalBoolean(opt.includeScreenshot),
        screenshotPath: optionalString(opt.screenshotPath),
        preferKinds: parsePreferKinds(optionalString(opt.preferKind) ?? opt.preferKinds)
      });

      const actionsOut = resolveOptionalPath(baseDir, optionalString(opt.actionsOut));
      const planOut = resolveOptionalPath(baseDir, optionalString(opt.planOutput));
      if (actionsOut) await writeTextFile(actionsOut, JSON.stringify(plan.actions, null, 2));
      if (planOut) await writeTextFile(planOut, JSON.stringify(plan, null, 2));
      result = plan;
      break;
    }

    case "session": {
      const actions = await resolveActions(job, baseDir);
      result = await runActions({
        ...common,
        actions,
        scope,
        sessionId: optionalString(opt.sessionId),
        evidenceDir: resolveOptionalPath(baseDir, optionalString(opt.evidenceDir)),
        observeAfterEachAction: optionalBooleanOrUndefined(opt.observeAfterEachAction),
        continueOnError: optionalBoolean(opt.continueOnError),
        observationOptions: observationOptions(opt),
        eventLogger: createJsonlEventLogger(resolveOptionalPath(baseDir, job.events))
      });
      break;
    }

    case "crawl": {
      if (!scope) throw new Error("crawl jobs require scope or scopePath");
      result = await crawl({
        ...common,
        startUrl: job.startUrl ?? requireUrl(job),
        scope,
        maxPages: optionalNumber(opt.maxPages),
        maxDepth: optionalNumber(opt.maxDepth),
        evidenceDir: resolveOptionalPath(baseDir, optionalString(opt.evidenceDir)),
        screenshots: optionalBoolean(opt.screenshots),
        includeObservations: optionalBoolean(opt.includeObservations),
        waitAfterNavigationMs: optionalNumber(opt.waitAfterNavigationMs),
        observationOptions: observationOptions(opt),
        eventLogger: createJsonlEventLogger(resolveOptionalPath(baseDir, job.events))
      });
      break;
    }

    case "audit": {
      result = await audit({
        ...common,
        url: requireUrl(job),
        scope,
        includeObservation: optionalBoolean(opt.includeObservation),
        waitAfterNavigationMs: optionalNumber(opt.waitAfterNavigationMs),
        observationOptions: observationOptions(opt)
      });
      break;
    }

    case "owasp-audit": {
      result = await owaspAudit({
        ...common,
        url: requireUrl(job),
        scope,
        owaspProfile: parseOwaspProfile(optionalString(opt.owaspProfile ?? opt.profileName)),
        outputPath: resolveOptionalPath(baseDir, optionalString(opt.outputPath)),
        maxActiveRequests: optionalNumber(opt.maxActiveRequests),
        activeDelayMs: optionalNumber(opt.activeDelayMs),
        activeRequestTimeoutMs: optionalNumber(opt.activeRequestTimeoutMs),
        waitAfterNavigationMs: optionalNumber(opt.waitAfterNavigationMs),
        observationOptions: observationOptions(opt)
      });
      break;
    }

    case "graphql-audit": {
      if (!scope) throw new Error("graphql-audit jobs require scope or scopePath");
      result = await graphqlAudit({
        url: requireUrl(job),
        scope,
        endpoint: optionalString(opt.endpoint),
        outputPath: resolveOptionalPath(baseDir, optionalString(opt.outputPath)),
        timeoutMs: optionalNumber(opt.timeoutMs),
        maxEndpoints: optionalNumber(opt.maxEndpoints),
        includeIntrospectionSchema: optionalBoolean(opt.includeIntrospectionSchema),
        batchCheck: optionalBooleanOrUndefined(opt.batchCheck),
        safeDataProbes: optionalBoolean(opt.safeDataProbes)
      });
      break;
    }

    case "loop": {
      result = await runLoop({
        ...common,
        url: requireUrl(job),
        scope,
        loopId: optionalString(opt.loopId),
        goal: optionalString(opt.goal),
        maxIterations: optionalNumber(opt.maxIterations),
        actionsPerIteration: optionalNumber(opt.actionsPerIteration),
        maxCandidates: optionalNumber(opt.maxCandidates),
        stopAfterNoActions: optionalNumber(opt.stopAfterNoActions),
        stopWhenText: optionalString(opt.stopWhenText),
        stopWhenUrl: optionalString(opt.stopWhenUrl),
        stopWhenSelector: optionalString(opt.stopWhenSelector),
        continueOnError: optionalBoolean(opt.continueOnError),
        evidenceDir: resolveOptionalPath(baseDir, optionalString(opt.evidenceDir)),
        screenshotEachIteration: optionalBoolean(opt.screenshotEachIteration) || optionalBoolean(opt.screenshots),
        includeObservations: optionalBoolean(opt.includeObservations),
        fillValue: optionalString(opt.fillValue),
        waitAfterNavigationMs: optionalNumber(opt.waitAfterNavigationMs),
        waitAfterActionMs: optionalNumber(opt.waitAfterActionMs),
        observationOptions: observationOptions(opt),
        eventLogger: createJsonlEventLogger(resolveOptionalPath(baseDir, job.events))
      });
      break;
    }

    case "replay": {
      const eventsPath = resolveOptionalPath(baseDir, job.events ?? optionalString(opt.events));
      if (!eventsPath) throw new Error("replay jobs require events or options.events");
      result = await replayEvents(eventsPath);
      break;
    }
  }

  const outputPath = resolveOptionalPath(baseDir, job.output);
  if (outputPath) await writeTextFile(outputPath, JSON.stringify(result, null, 2));

  await maybeWriteReports(job, baseDir, result);

  return {
    mode: job.mode,
    ok: resultOk(result),
    outputPath,
    reportPath: resolveOptionalPath(baseDir, job.report),
    htmlReportPath: resolveOptionalPath(baseDir, job.htmlReport),
    eventsPath: resolveOptionalPath(baseDir, job.events),
    result
  };
}

async function maybeWriteReports(job: SolariumJob, baseDir: string, result: unknown): Promise<void> {
  const markdownPath = resolveOptionalPath(baseDir, job.report);
  const htmlPath = resolveOptionalPath(baseDir, job.htmlReport);
  if (!markdownPath && !htmlPath) return;
  const reportOptions = { includeJsonAppendix: Boolean(job.reportIncludeJson) };

  switch (job.mode) {
    case "session":
      if (markdownPath) await writeTextFile(markdownPath, renderSessionMarkdownReport(result as AgentSessionResult, reportOptions));
      if (htmlPath) await writeTextFile(htmlPath, renderSessionHtmlReport(result as AgentSessionResult, reportOptions));
      return;
    case "crawl":
      if (markdownPath) await writeTextFile(markdownPath, renderCrawlMarkdownReport(result as CrawlResult, reportOptions));
      if (htmlPath) await writeTextFile(htmlPath, renderCrawlHtmlReport(result as CrawlResult, reportOptions));
      return;
    case "audit":
      if (markdownPath) await writeTextFile(markdownPath, renderAuditMarkdownReport(result as AuditResult, reportOptions));
      if (htmlPath) await writeTextFile(htmlPath, renderAuditHtmlReport(result as AuditResult, reportOptions));
      return;
    case "owasp-audit":
      if (markdownPath) await writeTextFile(markdownPath, renderOwaspAuditMarkdownReport(result as OwaspAuditResult, reportOptions));
      if (htmlPath) await writeTextFile(htmlPath, renderOwaspAuditHtmlReport(result as OwaspAuditResult, reportOptions));
      return;
    case "graphql-audit":
      if (markdownPath) await writeTextFile(markdownPath, renderGraphqlAuditMarkdownReport(result as GraphqlAuditResult, reportOptions));
      if (htmlPath) await writeTextFile(htmlPath, renderGraphqlAuditHtmlReport(result as GraphqlAuditResult, reportOptions));
      return;
    case "loop":
      if (markdownPath) await writeTextFile(markdownPath, renderLoopMarkdownReport(result as LoopResult, reportOptions));
      if (htmlPath) await writeTextFile(htmlPath, renderLoopHtmlReport(result as LoopResult, reportOptions));
      return;
    default:
      throw new Error(`Reports are not supported for ${job.mode} jobs`);
  }
}

async function resolveProfileForJob(job: SolariumJob, baseDir: string): Promise<BrowserProfileName | BrowserProfile | undefined> {
  if (job.profilePath) return readBrowserProfile(resolve(baseDir, job.profilePath));
  return job.profile;
}

async function resolveScope(job: SolariumJob, baseDir: string): Promise<ScopePolicy | undefined> {
  if (job.scope) return validateScopePolicy(job.scope);
  if (!job.scopePath) return undefined;
  const raw = await readFile(resolve(baseDir, job.scopePath), "utf8");
  return validateScopePolicy(JSON.parse(raw) as unknown);
}

async function resolveActions(job: SolariumJob, baseDir: string): Promise<AgentAction[]> {
  if (job.actions) return job.actions;
  if (!job.actionsPath) throw new Error(`${job.mode} jobs require actions or actionsPath`);
  const raw = await readFile(resolve(baseDir, job.actionsPath), "utf8");
  const parsed = JSON.parse(raw) as unknown;
  if (!Array.isArray(parsed)) throw new Error("actionsPath must point to a JSON AgentAction[] array");
  return parsed as AgentAction[];
}

async function resolveInspectResult(job: SolariumJob, baseDir: string): Promise<InspectResult> {
  if (job.inspectResult) return job.inspectResult;
  if (!job.inspectResultPath) throw new Error("plan jobs require inspectResult or inspectResultPath");
  const raw = await readFile(resolve(baseDir, job.inspectResultPath), "utf8");
  const parsed = JSON.parse(raw) as unknown;
  if (!parsed || typeof parsed !== "object" || !Array.isArray((parsed as InspectResult).candidates)) {
    throw new Error("inspectResultPath must point to a solarium inspect result JSON object");
  }
  return parsed as InspectResult;
}

function requireUrl(job: SolariumJob): string {
  if (typeof job.url === "string" && job.url) return job.url;
  throw new Error(`${job.mode} jobs require url`);
}

function observationOptions(value: Record<string, unknown>): ObservationOptions {
  return {
    maxTextChars: optionalNumber(value.maxTextChars),
    maxElements: optionalNumber(value.maxElements),
    maxConsoleEvents: optionalNumber(value.maxConsoleEvents),
    maxNetworkEvents: optionalNumber(value.maxNetworkEvents)
  };
}

function parsePreferKinds(value: unknown): InspectCandidateKind[] | undefined {
  if (!value) return undefined;
  const raw = Array.isArray(value) ? value : String(value).split(",");
  const allowed = new Set<InspectCandidateKind>(["link", "button", "input", "form", "navigation"]);
  const kinds = raw.map((item) => String(item).trim()).filter(Boolean) as InspectCandidateKind[];
  for (const kind of kinds) {
    if (!allowed.has(kind)) throw new Error(`Unsupported preferred candidate kind: ${kind}`);
  }
  return kinds;
}

function optionalString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function optionalNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function optionalBoolean(value: unknown): boolean {
  return value === true;
}

function optionalBooleanOrUndefined(value: unknown): boolean | undefined {
  return typeof value === "boolean" ? value : undefined;
}

function resolveOptionalPath(baseDir: string, path?: string): string | undefined {
  return path ? resolve(baseDir, path) : undefined;
}

function resultOk(result: unknown): boolean {
  if (result && typeof result === "object" && "ok" in result) {
    return Boolean((result as { ok?: unknown }).ok);
  }
  return true;
}

async function writeTextFile(path: string, content: string): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, content, "utf8");
}

function parseOwaspProfile(value?: string): "passive" | "strict-headers" | "active-authorized" | undefined {
  if (value === undefined) return undefined;
  if (value !== "passive" && value !== "strict-headers" && value !== "active-authorized") throw new Error(`Unsupported OWASP audit profile: ${value}`);
  return value;
}
