import { readFile } from "node:fs/promises";
import type { AgentAction } from "../types.js";
import type { EventLogEvent } from "./events.js";

export interface ReplaySummary {
  path: string;
  eventCount: number;
  firstTimestamp?: string;
  lastTimestamp?: string;
  eventTypes: Record<string, number>;
  sessions: ReplaySessionSummary[];
  crawls: ReplayCrawlSummary[];
  loops: ReplayLoopSummary[];
  errors: ReplayErrorSummary[];
}

export interface ReplaySessionSummary {
  sessionId?: string;
  startedAt?: string;
  finishedAt?: string;
  ok?: boolean;
  startedSteps: number;
  finishedSteps: number;
  lastStartedStepIndex?: number;
  lastFinishedStepIndex?: number;
  lastSuccessfulStepIndex?: number;
  failedStepIndexes: number[];
}

export interface ReplayCrawlSummary {
  startUrl?: string;
  startedAt?: string;
  finishedAt?: string;
  ok?: boolean;
  startedPages: number;
  finishedPages: number;
  lastStartedPageIndex?: number;
  lastFinishedPageIndex?: number;
  failedPageIndexes: number[];
}

export interface ReplayLoopSummary {
  loopId?: string;
  url?: string;
  goal?: string;
  startedAt?: string;
  finishedAt?: string;
  ok?: boolean;
  startedIterations: number;
  finishedIterations: number;
  lastStartedIterationIndex?: number;
  lastFinishedIterationIndex?: number;
  failedIterationIndexes: number[];
  stopReason?: string;
}

export interface ReplayErrorSummary {
  timestamp: string;
  payload: Record<string, unknown>;
}

export interface SessionResumePlan {
  resumeFromStep: number;
  completedStepIndexes: number[];
  failedStepIndexes: number[];
  remainingActions: AgentAction[];
}

export async function readJsonlEvents(path: string): Promise<EventLogEvent[]> {
  const raw = await readFile(path, "utf8");
  const events: EventLogEvent[] = [];
  const lines = raw.split(/\r?\n/);

  for (const [lineIndex, line] of lines.entries()) {
    const trimmed = line.trim();
    if (!trimmed) continue;

    let parsed: unknown;
    try {
      parsed = JSON.parse(trimmed) as unknown;
    } catch (error) {
      throw new Error(`Invalid JSONL at ${path}:${lineIndex + 1}: ${error instanceof Error ? error.message : String(error)}`);
    }

    if (!isEventLogEvent(parsed)) {
      throw new Error(`Invalid event object at ${path}:${lineIndex + 1}`);
    }
    events.push(parsed);
  }

  return events;
}

export async function replayEvents(path: string): Promise<ReplaySummary> {
  const events = await readJsonlEvents(path);
  return summarizeEvents(path, events);
}

export function summarizeEvents(path: string, events: EventLogEvent[]): ReplaySummary {
  const eventTypes: Record<string, number> = {};
  const sessionsById = new Map<string, ReplaySessionSummary>();
  const crawlsByKey = new Map<string, ReplayCrawlSummary>();
  const loopsById = new Map<string, ReplayLoopSummary>();
  const errors: ReplayErrorSummary[] = [];

  for (const event of events) {
    eventTypes[event.type] = (eventTypes[event.type] ?? 0) + 1;

    if (event.type.startsWith("session.")) {
      const sessionId = stringValue(event.payload.sessionId) ?? "default";
      const session = getSessionSummary(sessionsById, sessionId);
      if (event.type === "session.started") {
        session.sessionId = stringValue(event.payload.sessionId);
        session.startedAt = stringValue(event.payload.startedAt) ?? event.timestamp;
      } else if (event.type === "session.step.started") {
        const index = numberValue(event.payload.index);
        session.startedSteps += 1;
        if (index !== undefined) session.lastStartedStepIndex = index;
      } else if (event.type === "session.step.finished") {
        const index = numberValue(event.payload.index);
        session.finishedSteps += 1;
        if (index !== undefined) {
          session.lastFinishedStepIndex = index;
          if (event.payload.ok === true) session.lastSuccessfulStepIndex = index;
          if (event.payload.ok === false && !session.failedStepIndexes.includes(index)) {
            session.failedStepIndexes.push(index);
          }
        }
      } else if (event.type === "session.finished") {
        session.sessionId = stringValue(event.payload.sessionId) ?? session.sessionId;
        session.ok = booleanValue(event.payload.ok);
        session.startedAt = stringValue(event.payload.startedAt) ?? session.startedAt;
        session.finishedAt = stringValue(event.payload.finishedAt) ?? event.timestamp;
      }
    }

    if (event.type.startsWith("crawl.")) {
      const key = stringValue(event.payload.startUrl) ?? "default";
      const crawl = getCrawlSummary(crawlsByKey, key);
      if (event.type === "crawl.started") {
        crawl.startUrl = stringValue(event.payload.startUrl);
        crawl.startedAt = stringValue(event.payload.startedAt) ?? event.timestamp;
      } else if (event.type === "crawl.page.started") {
        const index = numberValue(event.payload.index);
        crawl.startedPages += 1;
        if (index !== undefined) crawl.lastStartedPageIndex = index;
      } else if (event.type === "crawl.page.finished") {
        const index = numberValue(event.payload.index);
        crawl.finishedPages += 1;
        if (index !== undefined) {
          crawl.lastFinishedPageIndex = index;
          if (event.payload.ok === false && !crawl.failedPageIndexes.includes(index)) {
            crawl.failedPageIndexes.push(index);
          }
        }
      } else if (event.type === "crawl.finished") {
        crawl.startUrl = stringValue(event.payload.startUrl) ?? crawl.startUrl;
        crawl.ok = booleanValue(event.payload.ok);
        crawl.startedAt = stringValue(event.payload.startedAt) ?? crawl.startedAt;
        crawl.finishedAt = stringValue(event.payload.finishedAt) ?? event.timestamp;
      }
    }

    if (event.type.startsWith("loop.")) {
      const loopId = stringValue(event.payload.loopId) ?? "default";
      const loop = getLoopSummary(loopsById, loopId);
      if (event.type === "loop.started") {
        loop.loopId = stringValue(event.payload.loopId);
        loop.url = stringValue(event.payload.url);
        loop.goal = stringValue(event.payload.goal);
        loop.startedAt = stringValue(event.payload.startedAt) ?? event.timestamp;
      } else if (event.type === "loop.iteration.started") {
        const index = numberValue(event.payload.index);
        loop.startedIterations += 1;
        if (index !== undefined) loop.lastStartedIterationIndex = index;
      } else if (event.type === "loop.iteration.finished") {
        const index = numberValue(event.payload.index);
        loop.finishedIterations += 1;
        if (index !== undefined) {
          loop.lastFinishedIterationIndex = index;
          if (event.payload.ok === false && !loop.failedIterationIndexes.includes(index)) {
            loop.failedIterationIndexes.push(index);
          }
        }
      } else if (event.type === "loop.finished") {
        loop.loopId = stringValue(event.payload.loopId) ?? loop.loopId;
        loop.ok = booleanValue(event.payload.ok);
        loop.stopReason = stringValue(event.payload.stopReason) ?? loop.stopReason;
        loop.startedAt = stringValue(event.payload.startedAt) ?? loop.startedAt;
        loop.finishedAt = stringValue(event.payload.finishedAt) ?? event.timestamp;
      }
    }

    if (event.type === "error") {
      errors.push({ timestamp: event.timestamp, payload: event.payload });
    }
  }

  return {
    path,
    eventCount: events.length,
    firstTimestamp: events[0]?.timestamp,
    lastTimestamp: events.at(-1)?.timestamp,
    eventTypes,
    sessions: [...sessionsById.values()],
    crawls: [...crawlsByKey.values()],
    loops: [...loopsById.values()],
    errors
  };
}

export async function createSessionResumePlan(actions: AgentAction[], eventsPath: string): Promise<SessionResumePlan> {
  const events = await readJsonlEvents(eventsPath);
  const completedStepIndexes: number[] = [];
  const failedStepIndexes: number[] = [];

  for (const event of events) {
    if (event.type !== "session.step.finished") continue;
    const index = numberValue(event.payload.index);
    if (index === undefined) continue;
    if (event.payload.ok === true && !completedStepIndexes.includes(index)) {
      completedStepIndexes.push(index);
    }
    if (event.payload.ok === false && !failedStepIndexes.includes(index)) {
      failedStepIndexes.push(index);
    }
  }

  completedStepIndexes.sort((a, b) => a - b);
  failedStepIndexes.sort((a, b) => a - b);

  let resumeFromStep = 0;
  for (const index of completedStepIndexes) {
    if (index === resumeFromStep) {
      resumeFromStep += 1;
    } else if (index > resumeFromStep) {
      break;
    }
  }

  return {
    resumeFromStep,
    completedStepIndexes,
    failedStepIndexes,
    remainingActions: actions.slice(resumeFromStep)
  };
}

function getSessionSummary(map: Map<string, ReplaySessionSummary>, key: string): ReplaySessionSummary {
  let summary = map.get(key);
  if (!summary) {
    summary = {
      sessionId: key === "default" ? undefined : key,
      startedSteps: 0,
      finishedSteps: 0,
      failedStepIndexes: []
    };
    map.set(key, summary);
  }
  return summary;
}

function getCrawlSummary(map: Map<string, ReplayCrawlSummary>, key: string): ReplayCrawlSummary {
  let summary = map.get(key);
  if (!summary) {
    summary = {
      startUrl: key === "default" ? undefined : key,
      startedPages: 0,
      finishedPages: 0,
      failedPageIndexes: []
    };
    map.set(key, summary);
  }
  return summary;
}

function getLoopSummary(map: Map<string, ReplayLoopSummary>, key: string): ReplayLoopSummary {
  let summary = map.get(key);
  if (!summary) {
    summary = {
      loopId: key === "default" ? undefined : key,
      startedIterations: 0,
      finishedIterations: 0,
      failedIterationIndexes: []
    };
    map.set(key, summary);
  }
  return summary;
}

function isEventLogEvent(value: unknown): value is EventLogEvent {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return typeof candidate.type === "string" && typeof candidate.timestamp === "string" && isRecord(candidate.payload);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function stringValue(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function numberValue(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function booleanValue(value: unknown): boolean | undefined {
  return typeof value === "boolean" ? value : undefined;
}
