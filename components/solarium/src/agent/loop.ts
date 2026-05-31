import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { SolariumBrowser, type SolariumPage } from "../browser/engine.js";
import { planActionsFromInspectResult } from "./plan.js";
import { discoverCandidates } from "./inspect.js";
import { ObservationRecorder } from "./observations.js";
import { attachScopedNetworkPolicy, type NetworkPolicyController } from "../security/network-policy.js";
import { assertUrlInScope, type ScopePolicy } from "../security/scope.js";
import type {
  AgentAction,
  ExtractOptions,
  InspectResult,
  LoopIterationResult,
  LoopOptions,
  LoopResult,
  PageObservation
} from "../types.js";
import type { EventLogger } from "../reporting/events.js";
import { summarizeAgentAction, summarizeNetworkPolicy } from "../reporting/events.js";

const DEFAULT_MAX_ITERATIONS = 5;
const DEFAULT_ACTIONS_PER_ITERATION = 1;
const DEFAULT_MAX_CANDIDATES = 100;

export interface RunLoopOptions extends LoopOptions {
  scope?: ScopePolicy;
  eventLogger?: EventLogger;
}

export async function runLoop(options: RunLoopOptions): Promise<LoopResult> {
  assertUrlInScope(options.url, options.scope);

  const loopId = options.loopId ?? `loop-${Date.now()}`;
  const startedAt = new Date().toISOString();
  const evidenceDir = options.evidenceDir ?? `.solarium/loops/${loopId}`;
  const maxIterations = options.maxIterations ?? DEFAULT_MAX_ITERATIONS;
  const actionsPerIteration = options.actionsPerIteration ?? DEFAULT_ACTIONS_PER_ITERATION;
  const maxCandidates = options.maxCandidates ?? DEFAULT_MAX_CANDIDATES;
  const stopAfterNoActions = options.stopAfterNoActions ?? 1;
  const iterations: LoopIterationResult[] = [];

  await mkdir(evidenceDir, { recursive: true });
  await options.eventLogger?.emit("loop.started", {
    loopId,
    url: options.url,
    goal: options.goal,
    startedAt,
    maxIterations,
    actionsPerIteration,
    evidenceDir,
    stopConditions: {
      text: options.stopWhenText,
      url: options.stopWhenUrl,
      selector: options.stopWhenSelector
    }
  });

  const browser = await SolariumBrowser.launch(options);
  let networkPolicy: NetworkPolicyController | undefined;
  try {
    const page = await browser.newPage();
    const rawPage = page.raw();
    const recorder = new ObservationRecorder(rawPage);
    recorder.attach();
    networkPolicy = await attachScopedNetworkPolicy(rawPage, {
      scope: options.scope,
      onBlockedRequest: (event) => recorder.recordNetworkEvent(event)
    });

    await page.navigate(options.url, { waitUntil: options.waitUntil, timeoutMs: options.timeoutMs });
    if (options.waitAfterNavigationMs) await page.wait(options.waitAfterNavigationMs);

    let noActionStreak = 0;
    let stopReason: string | undefined;
    for (let index = 0; index < maxIterations; index += 1) {
      await options.eventLogger?.emit("loop.iteration.started", {
        loopId,
        index,
        url: page.url(),
        goal: options.goal
      });

      const iteration = await runLoopIteration({
        index,
        loopId,
        page,
        recorder,
        evidenceDir,
        scope: options.scope,
        goal: options.goal,
        fillValue: options.fillValue,
        maxCandidates,
        actionsPerIteration,
        screenshotEachIteration: options.screenshotEachIteration,
        includeObservation: options.includeObservations,
        observationOptions: options.observationOptions,
        waitAfterActionMs: options.waitAfterActionMs,
        networkPolicy,
        stopWhenText: options.stopWhenText,
        stopWhenUrl: options.stopWhenUrl,
        stopWhenSelector: options.stopWhenSelector
      });
      iterations.push(iteration);

      await options.eventLogger?.emit("loop.iteration.finished", {
        loopId,
        index,
        ok: iteration.ok,
        url: iteration.url,
        title: iteration.title,
        candidateCount: iteration.candidateCount,
        actions: iteration.actions.map(summarizeAgentAction),
        error: iteration.error,
        screenshotPath: iteration.screenshotPath
      });

      if (iteration.stopReason) {
        stopReason = iteration.stopReason;
        break;
      }
      if (!iteration.ok && !options.continueOnError) {
        stopReason = `iteration ${iteration.index} failed`;
        break;
      }
      if (iteration.actions.length === 0) {
        noActionStreak += 1;
        if (noActionStreak >= stopAfterNoActions) {
          stopReason = `no actions planned for ${noActionStreak} consecutive iteration(s)`;
          break;
        }
      } else {
        noActionStreak = 0;
      }
    }

    stopReason ??= iterations.length >= maxIterations ? `max iterations reached (${maxIterations})` : "loop completed";

    const result: LoopResult = {
      loopId,
      url: options.url,
      goal: options.goal,
      startedAt,
      finishedAt: new Date().toISOString(),
      ok: iterations.every((iteration) => iteration.ok),
      iterations,
      stopReason,
      networkPolicy: networkPolicy.stats()
    };

    await options.eventLogger?.emit("network.policy.summary", {
      loopId,
      networkPolicy: summarizeNetworkPolicy(result.networkPolicy)
    });
    await options.eventLogger?.emit("loop.finished", {
      loopId,
      ok: result.ok,
      iterations: result.iterations.length,
      stopReason: result.stopReason,
      startedAt: result.startedAt,
      finishedAt: result.finishedAt
    });

    return result;
  } finally {
    await networkPolicy?.detach();
    await browser.close();
    await options.eventLogger?.close();
  }
}

async function runLoopIteration(options: {
  index: number;
  loopId: string;
  page: SolariumPage;
  recorder: ObservationRecorder;
  evidenceDir: string;
  scope?: ScopePolicy;
  goal?: string;
  fillValue?: string;
  maxCandidates: number;
  actionsPerIteration: number;
  screenshotEachIteration?: boolean;
  includeObservation?: boolean;
  observationOptions?: LoopOptions["observationOptions"];
  waitAfterActionMs?: number;
  networkPolicy?: NetworkPolicyController;
  stopWhenText?: string;
  stopWhenUrl?: string;
  stopWhenSelector?: string;
}): Promise<LoopIterationResult> {
  const startedAt = new Date().toISOString();
  const page = options.page;

  try {
    assertUrlInScope(page.url(), options.scope);
    const observation = await options.recorder.observe(options.observationOptions);
    const candidates = await discoverCandidates(page.raw(), options.maxCandidates);
    const inspectResult: InspectResult = {
      url: page.url(),
      finalUrl: page.url(),
      title: await page.title(),
      inspectedAt: new Date().toISOString(),
      candidates,
      observation: options.includeObservation ? observation : undefined,
      networkPolicy: options.networkPolicy?.stats()
    };

    const beforeActionStopReason = await evaluateLoopStopConditions(page, observation, options);
    if (beforeActionStopReason) {
      return {
        index: options.index,
        startedAt,
        finishedAt: new Date().toISOString(),
        ok: true,
        url: page.url(),
        title: await page.title(),
        candidateCount: candidates.length,
        actions: [],
        selectedCandidates: [],
        notes: [`Stop condition met before action: ${beforeActionStopReason}`],
        observation: options.includeObservation ? observation : undefined,
        stopReason: beforeActionStopReason
      };
    }

    const plan = planActionsFromInspectResult(inspectResult, {
      goal: options.goal,
      fillValue: options.fillValue,
      includeNavigate: false,
      includeObserve: false,
      maxActions: options.actionsPerIteration
    });
    const actions = plan.actions.slice(0, options.actionsPerIteration);

    for (const action of actions) {
      await executeLoopAction(page, action, options.scope);
      if (options.waitAfterActionMs) await page.wait(options.waitAfterActionMs);
      assertUrlInScope(page.url(), options.scope);
    }

    let screenshotPath: string | undefined;
    if (options.screenshotEachIteration) {
      screenshotPath = join(options.evidenceDir, `iteration-${options.index}.png`);
      await page.screenshot({ path: screenshotPath, fullPage: true });
    }

    const finalObservationForStop = await options.recorder.observe(options.observationOptions);
    const afterActionStopReason = await evaluateLoopStopConditions(page, finalObservationForStop, options);
    const finalObservation: PageObservation | undefined = options.includeObservation
      ? finalObservationForStop
      : undefined;

    return {
      index: options.index,
      startedAt,
      finishedAt: new Date().toISOString(),
      ok: true,
      url: page.url(),
      title: await page.title(),
      candidateCount: candidates.length,
      actions,
      selectedCandidates: plan.selectedCandidates,
      notes: afterActionStopReason ? [...plan.notes, `Stop condition met after action: ${afterActionStopReason}`] : plan.notes,
      screenshotPath,
      observation: finalObservation,
      stopReason: afterActionStopReason
    };
  } catch (error) {
    return {
      index: options.index,
      startedAt,
      finishedAt: new Date().toISOString(),
      ok: false,
      url: page.url(),
      title: await safeTitle(page),
      candidateCount: 0,
      actions: [],
      selectedCandidates: [],
      notes: [],
      error: error instanceof Error ? error.message : String(error)
    };
  }
}

async function executeLoopAction(page: SolariumPage, action: AgentAction, scope?: ScopePolicy): Promise<void> {
  switch (action.type) {
    case "navigate":
      assertUrlInScope(action.url, scope);
      await page.navigate(action.url, { waitUntil: action.waitUntil, timeoutMs: action.timeoutMs });
      return;
    case "click":
      await page.click(action.selector);
      return;
    case "dblclick":
      await page.dblclick(action.selector);
      return;
    case "hover":
      await page.hover(action.selector);
      return;
    case "type":
      await page.type(action.selector, action.text);
      return;
    case "press":
      await page.press(action.selector, action.key);
      return;
    case "wait":
      await page.wait(action.ms);
      return;
    case "waitForSelector":
      await page.waitForSelector(action.selector, { state: action.state, timeoutMs: action.timeoutMs });
      return;
    case "waitForUrl":
      assertUrlInScope(action.url, scope);
      await page.waitForUrl(action.url, { timeoutMs: action.timeoutMs });
      return;
    case "screenshot":
      await page.screenshot({ path: action.path, fullPage: action.fullPage ?? true });
      return;
    case "extract":
      await page.extract({ selector: action.selector, format: action.format as ExtractOptions["format"] });
      return;
    case "upload":
      await page.upload(action.selector, action.files);
      return;
    case "select":
      await page.select(action.selector, action.values);
      return;
    case "check":
      await page.check(action.selector);
      return;
    case "uncheck":
      await page.uncheck(action.selector);
      return;
    case "submit":
      await page.submit(action.selector);
      return;
    case "download":
      throw new Error("Loop actions do not support downloads yet; use a session action for downloads");
    case "observe":
      return;
    default: {
      const exhaustive: never = action;
      throw new Error(`Unsupported loop action: ${JSON.stringify(exhaustive)}`);
    }
  }
}

async function evaluateLoopStopConditions(
  page: SolariumPage,
  observation: PageObservation,
  options: { stopWhenText?: string; stopWhenUrl?: string; stopWhenSelector?: string }
): Promise<string | undefined> {
  if (options.stopWhenUrl) {
    const pattern = options.stopWhenUrl;
    if (matchesPattern(page.url(), pattern)) return `URL matched ${pattern}`;
  }

  if (options.stopWhenText) {
    const needle = options.stopWhenText.toLowerCase();
    if (observation.visibleText.toLowerCase().includes(needle)) return `text matched ${options.stopWhenText}`;
  }

  if (options.stopWhenSelector) {
    const count = await page.raw().locator(options.stopWhenSelector).count();
    if (count > 0) return `selector matched ${options.stopWhenSelector}`;
  }

  return undefined;
}

function matchesPattern(value: string, pattern: string): boolean {
  if (pattern.startsWith("/") && pattern.endsWith("/") && pattern.length > 2) {
    return new RegExp(pattern.slice(1, -1)).test(value);
  }
  if (pattern.includes("*")) {
    const escaped = pattern.replace(/[.+?^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*");
    return new RegExp(`^${escaped}$`).test(value);
  }
  return value.includes(pattern);
}

async function safeTitle(page: SolariumPage): Promise<string | undefined> {
  try {
    return await page.title();
  } catch {
    return undefined;
  }
}
