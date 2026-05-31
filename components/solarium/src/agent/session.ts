import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { SolariumBrowser, type SolariumPage } from "../browser/engine.js";
import { assertUrlInScope, type ScopePolicy } from "../security/scope.js";
import { attachScopedNetworkPolicy, type NetworkPolicyController } from "../security/network-policy.js";
import type {
  AgentAction,
  AgentSessionOptions,
  AgentSessionResult,
  AgentStepResult,
  ExtractOptions,
  ObservationOptions
} from "../types.js";
import { ObservationRecorder } from "./observations.js";
import type { EventLogger } from "../reporting/events.js";
import { summarizeAgentAction, summarizeAgentStep, summarizeNetworkPolicy } from "../reporting/events.js";

export interface RunActionsOptions extends AgentSessionOptions {
  actions: AgentAction[];
  actionOffset?: number;
  scope?: ScopePolicy;
  continueOnError?: boolean;
  eventLogger?: EventLogger;
}

export class AgentSession {
  private readonly sessionId: string;
  private readonly startedAt = new Date().toISOString();
  private readonly evidenceDir: string;
  private readonly observeAfterEachAction: boolean;
  private readonly observationOptions?: ObservationOptions;
  private readonly steps: AgentStepResult[] = [];
  private browser?: SolariumBrowser;
  private page?: SolariumPage;
  private recorder?: ObservationRecorder;
  private networkPolicy?: NetworkPolicyController;

  private constructor(private readonly options: AgentSessionOptions & { eventLogger?: EventLogger } = {}) {
    this.sessionId = options.sessionId ?? `session-${Date.now()}`;
    this.evidenceDir = options.evidenceDir ?? `.solarium/sessions/${this.sessionId}`;
    this.observeAfterEachAction = options.observeAfterEachAction ?? true;
    this.observationOptions = options.observationOptions;
  }

  static async launch(options: AgentSessionOptions & { scope?: ScopePolicy; eventLogger?: EventLogger } = {}): Promise<AgentSession> {
    const session = new AgentSession(options);
    await mkdir(session.evidenceDir, { recursive: true });
    await options.eventLogger?.emit("session.started", {
      sessionId: session.sessionId,
      startedAt: session.startedAt,
      evidenceDir: session.evidenceDir,
      engine: options.engine ?? "chromium",
      headless: options.headless ?? true
    });
    session.browser = await SolariumBrowser.launch(options);
    session.page = await session.browser.newPage();
    session.recorder = new ObservationRecorder(session.page.raw());
    session.recorder.attach();
    session.networkPolicy = await attachScopedNetworkPolicy(session.page.raw(), {
      scope: options.scope,
      onBlockedRequest: (event) => session.recorder?.recordNetworkEvent(event)
    });
    return session;
  }

  async run(
    actions: AgentAction[],
    options: { scope?: ScopePolicy; continueOnError?: boolean; actionOffset?: number } = {}
  ): Promise<AgentSessionResult> {
    const actionOffset = options.actionOffset ?? 0;
    for (const [relativeIndex, action] of actions.entries()) {
      const index = actionOffset + relativeIndex;
      await this.options.eventLogger?.emit("session.step.started", {
        sessionId: this.sessionId,
        index,
        action: summarizeAgentAction(action)
      });
      const step = await this.runAction(action, index, options.scope);
      this.steps.push(step);
      await this.options.eventLogger?.emit("session.step.finished", {
        sessionId: this.sessionId,
        ...summarizeAgentStep(step)
      });

      if (!step.ok && !options.continueOnError) {
        break;
      }
    }

    const result = this.result();
    await this.options.eventLogger?.emit("network.policy.summary", {
      sessionId: this.sessionId,
      networkPolicy: summarizeNetworkPolicy(result.networkPolicy)
    });
    await this.options.eventLogger?.emit("session.finished", {
      sessionId: this.sessionId,
      ok: result.ok,
      startedAt: result.startedAt,
      finishedAt: result.finishedAt,
      steps: result.steps.length
    });
    return result;
  }

  async close(): Promise<void> {
    await this.networkPolicy?.detach();
    await this.browser?.close();
    await this.options.eventLogger?.close();
  }

  private async runAction(action: AgentAction, index: number, scope?: ScopePolicy): Promise<AgentStepResult> {
    const page = this.requirePage();
    const recorder = this.requireRecorder();
    const startedAt = new Date().toISOString();

    const base: Omit<AgentStepResult, "finishedAt" | "ok" | "url"> = {
      index,
      action,
      startedAt
    };

    try {
      let screenshotPath: string | undefined;
      let extracted: AgentStepResult["extracted"];
      let download: AgentStepResult["download"];
      let observation: AgentStepResult["observation"];

      switch (action.type) {
        case "navigate": {
          assertUrlInScope(action.url, scope);
          await page.navigate(action.url, { waitUntil: action.waitUntil, timeoutMs: action.timeoutMs });
          break;
        }
        case "click": {
          await page.click(action.selector);
          assertUrlInScope(page.url(), scope);
          break;
        }
        case "dblclick": {
          await page.dblclick(action.selector);
          assertUrlInScope(page.url(), scope);
          break;
        }
        case "hover": {
          await page.hover(action.selector);
          break;
        }
        case "type": {
          await page.type(action.selector, action.text);
          break;
        }
        case "press": {
          await page.press(action.selector, action.key);
          assertUrlInScope(page.url(), scope);
          break;
        }
        case "upload": {
          await page.upload(action.selector, action.files);
          break;
        }
        case "select": {
          await page.select(action.selector, action.values);
          break;
        }
        case "check": {
          await page.check(action.selector);
          break;
        }
        case "uncheck": {
          await page.uncheck(action.selector);
          break;
        }
        case "submit": {
          await page.submit(action.selector);
          assertUrlInScope(page.url(), scope);
          break;
        }
        case "download": {
          const rawPage = page.raw();
          const [downloadArtifact] = await Promise.all([
            rawPage.waitForEvent("download", { timeout: action.timeoutMs ?? 30_000 }),
            rawPage.click(action.selector)
          ]);
          const suggestedFilename = downloadArtifact.suggestedFilename();
          const downloadPath = action.path ?? join(this.evidenceDir, "downloads", `${index}-${suggestedFilename}`);
          await mkdir(dirnameForFile(downloadPath), { recursive: true });
          await downloadArtifact.saveAs(downloadPath);
          download = {
            suggestedFilename,
            path: downloadPath,
            url: downloadArtifact.url(),
            failure: await downloadArtifact.failure()
          };
          break;
        }
        case "wait": {
          await page.wait(action.ms);
          break;
        }
        case "waitForSelector": {
          await page.waitForSelector(action.selector, { state: action.state, timeoutMs: action.timeoutMs });
          break;
        }
        case "waitForUrl": {
          assertUrlInScope(action.url, scope);
          await page.waitForUrl(action.url, { timeoutMs: action.timeoutMs });
          assertUrlInScope(page.url(), scope);
          break;
        }
        case "screenshot": {
          screenshotPath = action.path ?? join(this.evidenceDir, `step-${index}-screenshot.png`);
          await page.screenshot({ path: screenshotPath, fullPage: action.fullPage ?? true });
          break;
        }
        case "extract": {
          extracted = await page.extract({ selector: action.selector, format: action.format as ExtractOptions["format"] });
          break;
        }
        case "observe": {
          observation = await recorder.observe(this.observationOptions);
          break;
        }
        default: {
          const exhaustive: never = action;
          throw new Error(`Unsupported action: ${JSON.stringify(exhaustive)}`);
        }
      }

      if (this.observeAfterEachAction && action.type !== "observe") {
        observation = await recorder.observe(this.observationOptions);
      }

      return {
        ...base,
        finishedAt: new Date().toISOString(),
        ok: true,
        url: page.url(),
        title: await page.title(),
        screenshotPath,
        extracted,
        download,
        observation
      };
    } catch (error) {
      return {
        ...base,
        finishedAt: new Date().toISOString(),
        ok: false,
        url: page.url(),
        title: await safeTitle(page),
        error: error instanceof Error ? error.message : String(error)
      };
    }
  }

  private result(): AgentSessionResult {
    return {
      sessionId: this.sessionId,
      startedAt: this.startedAt,
      finishedAt: new Date().toISOString(),
      ok: this.steps.every((step) => step.ok),
      steps: this.steps,
      networkPolicy: this.networkPolicy?.stats()
    };
  }

  private requirePage(): SolariumPage {
    if (!this.page) {
      throw new Error("Agent session has not been launched");
    }
    return this.page;
  }

  private requireRecorder(): ObservationRecorder {
    if (!this.recorder) {
      throw new Error("Agent session has not been launched");
    }
    return this.recorder;
  }
}

export async function runActions(options: RunActionsOptions): Promise<AgentSessionResult> {
  const session = await AgentSession.launch(options);
  try {
    return await session.run(options.actions, {
      scope: options.scope,
      continueOnError: options.continueOnError,
      actionOffset: options.actionOffset
    });
  } finally {
    await session.close();
  }
}

async function safeTitle(page: SolariumPage): Promise<string | undefined> {
  try {
    return await page.title();
  } catch {
    return undefined;
  }
}

function dirnameForFile(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const index = normalized.lastIndexOf("/");
  return index >= 0 ? path.slice(0, index) : ".";
}
