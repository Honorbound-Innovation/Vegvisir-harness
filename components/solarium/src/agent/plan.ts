import type { AgentAction, InspectCandidate, InspectResult } from "../types.js";

export interface PlanActionsOptions {
  goal?: string;
  includeNavigate?: boolean;
  includeObserve?: boolean;
  includeScreenshot?: boolean;
  screenshotPath?: string;
  maxActions?: number;
  fillValue?: string;
  preferKinds?: InspectCandidate["kind"][];
}

export interface ActionPlanResult {
  createdAt: string;
  sourceUrl: string;
  finalUrl: string;
  title: string;
  goal?: string;
  actions: AgentAction[];
  selectedCandidates: InspectCandidate[];
  notes: string[];
}

const DEFAULT_MAX_ACTIONS = 10;
const DEFAULT_FILL_VALUE = "TODO: provide value";

export function planActionsFromInspectResult(inspect: InspectResult, options: PlanActionsOptions = {}): ActionPlanResult {
  const notes: string[] = [];
  const actions: AgentAction[] = [];
  const selectedCandidates: InspectCandidate[] = [];
  const maxActions = options.maxActions ?? DEFAULT_MAX_ACTIONS;

  if (options.includeNavigate ?? true) {
    actions.push({ type: "navigate", url: inspect.finalUrl || inspect.url });
  }

  const candidates = rankCandidates(inspect.candidates, options);
  for (const candidate of candidates) {
    if (actions.length >= maxActions) break;

    const action = actionForCandidate(candidate, options);
    if (!action) continue;

    actions.push(action);
    selectedCandidates.push(candidate);
  }

  if (options.includeScreenshot) {
    actions.push({ type: "screenshot", path: options.screenshotPath, fullPage: true });
  }

  if (options.includeObserve ?? true) {
    actions.push({ type: "observe" });
  }

  if (selectedCandidates.some((candidate) => candidate.action === "fill")) {
    notes.push("Generated fill actions use a placeholder value unless --fill-value is provided.");
  }
  if (selectedCandidates.some((candidate) => candidate.action === "submit")) {
    notes.push("Generated submit candidates are represented as clicks on the form selector; review before running against real targets.");
  }
  if (selectedCandidates.length === 0) {
    notes.push("No actionable candidates matched the planning options.");
  }

  return {
    createdAt: new Date().toISOString(),
    sourceUrl: inspect.url,
    finalUrl: inspect.finalUrl,
    title: inspect.title,
    goal: options.goal,
    actions,
    selectedCandidates,
    notes
  };
}

function rankCandidates(candidates: InspectCandidate[], options: PlanActionsOptions): InspectCandidate[] {
  const preferKinds = new Set(options.preferKinds ?? []);
  const goal = normalize(options.goal);

  return [...candidates].sort((a, b) => scoreCandidate(b, goal, preferKinds) - scoreCandidate(a, goal, preferKinds));
}

function scoreCandidate(candidate: InspectCandidate, goal: string, preferKinds: Set<InspectCandidate["kind"]>): number {
  let score = 0;

  if (candidate.confidence === "high") score += 30;
  if (candidate.confidence === "medium") score += 15;
  if (preferKinds.has(candidate.kind)) score += 25;
  if (candidate.disabled) score -= 100;
  if (candidate.action === "fill") score += 8;
  if (candidate.action === "click") score += 6;
  if (candidate.action === "navigate") score += 4;

  const haystack = normalize([candidate.label, candidate.href, candidate.inputType, candidate.reason].filter(Boolean).join(" "));
  for (const token of goal.split(" ").filter((token) => token.length >= 3)) {
    if (haystack.includes(token)) score += 10;
  }

  if (/search|query|q\b/.test(haystack)) score += goal.includes("search") ? 30 : 6;
  if (/login|sign in|password/.test(haystack)) score -= 20;
  if (/delete|remove|logout|sign out/.test(haystack)) score -= 25;

  return score;
}

function actionForCandidate(candidate: InspectCandidate, options: PlanActionsOptions): AgentAction | undefined {
  if (candidate.disabled) return undefined;

  switch (candidate.action) {
    case "navigate":
      return candidate.href ? { type: "navigate", url: candidate.href } : { type: "click", selector: candidate.selector };
    case "click":
      return { type: "click", selector: candidate.selector };
    case "fill":
      return { type: "type", selector: candidate.selector, text: options.fillValue ?? DEFAULT_FILL_VALUE };
    case "submit":
      return { type: "click", selector: candidate.selector };
    default: {
      const exhaustive: never = candidate.action;
      throw new Error(`Unsupported candidate action: ${String(exhaustive)}`);
    }
  }
}

function normalize(value: string | undefined): string {
  return (value ?? "").toLowerCase().replace(/[^a-z0-9]+/g, " ").replace(/\s+/g, " ").trim();
}
