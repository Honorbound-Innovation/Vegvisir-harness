// @effect-diagnostics-next-line nodeBuiltinImport:off - Vegvisir app-server is a local stdio JSONL process.
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import * as readline from "node:readline";
import crypto from "node:crypto";

import {
  EventId,
  ProviderDriverKind,
  type ProviderInstanceId,
  type ProviderRuntimeEvent,
  type ProviderSession,
  RuntimeRequestId,
  ThreadId,
  TurnId,
  type VegvisirSettings,
} from "@t3tools/contracts";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";
import * as Queue from "effect/Queue";
import * as Stream from "effect/Stream";

import {
  ProviderAdapterRequestError,
  ProviderAdapterSessionNotFoundError,
  ProviderAdapterValidationError,
} from "../Errors.ts";
import type { ProviderAdapterShape } from "../Services/ProviderAdapter.ts";

const PROVIDER = ProviderDriverKind.make("vegvisir");

interface VegvisirBridgeEvent {
  readonly type: string;
  readonly id?: string;
  readonly payload?: unknown;
}

interface VegvisirBridgeSessionSnapshot {
  readonly workspace?: string | undefined;
  readonly session_id?: string | undefined;
  readonly provider?: string | undefined;
  readonly model?: string | undefined;
  readonly status?: string | undefined;
}

interface PendingRequest {
  readonly resolve: (event: VegvisirBridgeEvent) => void;
  readonly reject: (error: Error) => void;
}

interface VegvisirSessionContext {
  session: ProviderSession;
  readonly process: ChildProcessWithoutNullStreams;
  readonly lineReader: readline.Interface;
  readonly pendingRequests: Map<string, PendingRequest>;
  readonly turns: Array<{ id: TurnId; items: Array<unknown> }>;
  activeTurnId: TurnId | undefined;
  stopped: boolean;
}

export type VegvisirAdapterError =
  | ProviderAdapterRequestError
  | ProviderAdapterSessionNotFoundError
  | ProviderAdapterValidationError;

export type VegvisirAdapterShape = ProviderAdapterShape<VegvisirAdapterError>;

const nowIso = Effect.map(DateTime.now, DateTime.formatIso);

const nonEmpty = (value: string | undefined): string | undefined => {
  const trimmed = value?.trim() ?? "";
  return trimmed.length > 0 ? trimmed : undefined;
};

const bridgeSnapshot = (event: VegvisirBridgeEvent): VegvisirBridgeSessionSnapshot => {
  const payload = event.payload;
  if (!payload || typeof payload !== "object") return {};
  const record = payload as Record<string, unknown>;
  const nestedSession = record.session;
  const snapshot =
    nestedSession && typeof nestedSession === "object"
      ? (nestedSession as Record<string, unknown>)
      : record;
  return {
    workspace: typeof snapshot.workspace === "string" ? snapshot.workspace : undefined,
    session_id: typeof snapshot.session_id === "string" ? snapshot.session_id : undefined,
    provider: typeof snapshot.provider === "string" ? snapshot.provider : undefined,
    model: typeof snapshot.model === "string" ? snapshot.model : undefined,
    status: typeof snapshot.status === "string" ? snapshot.status : undefined,
  };
};

const makeRuntimeEvent = (input: Omit<ProviderRuntimeEvent, "eventId" | "createdAt">) =>
  ({
    eventId: EventId.make(crypto.randomUUID()),
    createdAt: DateTime.formatIso(DateTime.nowUnsafe()),
    ...input,
  }) as ProviderRuntimeEvent;

export const makeVegvisirAdapter = Effect.fn("makeVegvisirAdapter")(function* (
  settings: VegvisirSettings,
  options: {
    readonly instanceId?: ProviderInstanceId;
    readonly environment?: NodeJS.ProcessEnv;
  } = {},
) {
  const runtimeEvents = yield* Queue.unbounded<ProviderRuntimeEvent>();
  const sessions = new Map<string, VegvisirSessionContext>();
  const instanceId = options.instanceId;
  const env = { ...process.env, ...(options.environment ?? {}) };

  const offer = (event: ProviderRuntimeEvent) =>
    Queue.offer(runtimeEvents, event).pipe(Effect.asVoid);

  const requireSession = (
    threadId: ThreadId,
  ): Effect.Effect<VegvisirSessionContext, ProviderAdapterSessionNotFoundError> => {
    const ctx = sessions.get(String(threadId));
    if (!ctx || ctx.stopped) {
      return Effect.fail(
        new ProviderAdapterSessionNotFoundError({
          provider: PROVIDER,
          threadId: String(threadId),
        }),
      );
    }
    return Effect.succeed(ctx);
  };

  const sendBridgeRequest = (
    ctx: VegvisirSessionContext,
    method: string,
    params: unknown,
  ): Promise<VegvisirBridgeEvent> =>
    new Promise((resolve, reject) => {
      const id = crypto.randomUUID();
      ctx.pendingRequests.set(id, { resolve, reject });
      ctx.process.stdin.write(`${JSON.stringify({ id, method, params })}\n`, (error) => {
        if (error) {
          ctx.pendingRequests.delete(id);
          reject(error);
        }
      });
    });

  const closeContext = (ctx: VegvisirSessionContext): void => {
    if (ctx.stopped) return;
    ctx.stopped = true;
    for (const pending of ctx.pendingRequests.values()) {
      pending.reject(new Error("Vegvisir session stopped."));
    }
    ctx.pendingRequests.clear();
    try {
      ctx.process.stdin.write(
        `${JSON.stringify({ id: crypto.randomUUID(), method: "shutdown", params: {} })}\n`,
      );
    } catch {
      // Process may already be gone.
    }
    ctx.lineReader.close();
    ctx.process.kill();
  };

  const handleBridgeEvent = (ctx: VegvisirSessionContext, event: VegvisirBridgeEvent): void => {
    if (event.type === "content.delta") {
      const payload = event.payload as Record<string, unknown> | undefined;
      const text = typeof payload?.text === "string" ? payload.text : "";
      if (text.length > 0) {
        Effect.runFork(
          offer(
            makeRuntimeEvent({
              type: "content.delta",
              provider: PROVIDER,
              ...(instanceId ? { providerInstanceId: instanceId } : {}),
              threadId: ctx.session.threadId,
              ...(ctx.activeTurnId ? { turnId: ctx.activeTurnId } : {}),
              payload: {
                streamKind: "assistant_text",
                delta: text,
              },
              raw: { source: "acp.vegvisir.extension", payload: event },
            }),
          ),
        );
      }
      return;
    }

    if (event.type === "approval.required") {
      const payload = event.payload as Record<string, unknown> | undefined;
      const approvals = Array.isArray(payload?.approvals) ? payload.approvals : [];
      for (const approval of approvals) {
        const record = approval as Record<string, unknown>;
        const requestId = typeof record.id === "string" ? record.id : crypto.randomUUID();
        Effect.runFork(
          offer(
            makeRuntimeEvent({
              type: "request.opened",
              provider: PROVIDER,
              ...(instanceId ? { providerInstanceId: instanceId } : {}),
              threadId: ctx.session.threadId,
              ...(ctx.activeTurnId ? { turnId: ctx.activeTurnId } : {}),
              requestId: RuntimeRequestId.make(requestId),
              payload: {
                requestType: "dynamic_tool_call",
                detail:
                  typeof record.reason === "string"
                    ? record.reason
                    : "Vegvisir requires approval for a tool call.",
                args: record,
              },
              raw: { source: "acp.vegvisir.extension", payload: event },
            }),
          ),
        );
      }
      return;
    }

    if (!event.id) return;
    const pending = ctx.pendingRequests.get(event.id);
    if (!pending) return;
    if (event.type === "error" || event.type === "turn.failed") {
      const payload = event.payload as Record<string, unknown> | undefined;
      pending.reject(
        new Error(String(payload?.message ?? payload?.error ?? "Vegvisir request failed.")),
      );
    } else {
      pending.resolve(event);
    }
    ctx.pendingRequests.delete(event.id);
  };

  const startSession: VegvisirAdapterShape["startSession"] = (input) =>
    Effect.gen(function* () {
      if (input.provider && input.provider !== PROVIDER) {
        return yield* new ProviderAdapterValidationError({
          provider: PROVIDER,
          operation: "startSession",
          issue: `Provider mismatch: expected ${PROVIDER}, got ${input.provider}`,
        });
      }
      if (sessions.has(String(input.threadId))) {
        return yield* requireSession(input.threadId).pipe(Effect.map((ctx) => ctx.session));
      }

      const cwd = input.cwd ?? process.cwd();
      const args = [
        ...(settings.dangerousBypass ? ["--dangerously-bypass-approvals-and-sandbox"] : []),
        ...(nonEmpty(settings.defaultProvider) ? ["--provider", settings.defaultProvider] : []),
        ...(nonEmpty(input.modelSelection?.model ?? settings.defaultModel)
          ? ["--model", input.modelSelection?.model ?? settings.defaultModel]
          : []),
        ...(nonEmpty(settings.defaultAgent) ? ["--agent", settings.defaultAgent] : []),
        "app-server",
        "--workspace",
        cwd,
      ];
      const child = spawn(settings.binaryPath, args, {
        cwd,
        env,
        stdio: ["pipe", "pipe", "pipe"],
      });
      child.stderr.resume();
      const lineReader = readline.createInterface({ input: child.stdout });
      const now = yield* nowIso;
      const session: ProviderSession = {
        provider: PROVIDER,
        ...(instanceId ? { providerInstanceId: instanceId } : {}),
        status: "connecting",
        runtimeMode: input.runtimeMode,
        cwd,
        model: input.modelSelection?.model ?? nonEmpty(settings.defaultModel),
        threadId: input.threadId,
        createdAt: now,
        updatedAt: now,
      };
      const ctx: VegvisirSessionContext = {
        session,
        process: child,
        lineReader,
        pendingRequests: new Map(),
        turns: [],
        activeTurnId: undefined,
        stopped: false,
      };
      sessions.set(String(input.threadId), ctx);

      lineReader.on("line", (line) => {
        try {
          handleBridgeEvent(ctx, JSON.parse(line) as VegvisirBridgeEvent);
        } catch {
          // Ignore malformed provider output; stderr still carries diagnostics.
        }
      });
      child.on("exit", () => {
        ctx.session = {
          ...ctx.session,
          status: "closed",
          updatedAt: DateTime.formatIso(DateTime.nowUnsafe()),
        };
        ctx.stopped = true;
      });
      child.on("error", (error) => {
        ctx.session = {
          ...ctx.session,
          status: "error",
          updatedAt: DateTime.formatIso(DateTime.nowUnsafe()),
        };
        for (const pending of ctx.pendingRequests.values()) {
          pending.reject(error);
        }
        ctx.pendingRequests.clear();
      });

      const started = yield* Effect.tryPromise({
        try: () => sendBridgeRequest(ctx, "initialize", {}),
        catch: (cause) =>
          new ProviderAdapterRequestError({
            provider: PROVIDER,
            method: "initialize",
            detail: cause instanceof Error ? cause.message : String(cause),
            cause,
          }),
      });
      const snapshot = bridgeSnapshot(started);
      ctx.session = {
        ...ctx.session,
        status: "ready",
        model: snapshot.model ?? ctx.session.model,
        resumeCursor: snapshot.session_id ? { sessionId: snapshot.session_id } : undefined,
        updatedAt: yield* nowIso,
      };
      yield* offer(
        makeRuntimeEvent({
          type: "session.started",
          provider: PROVIDER,
          ...(instanceId ? { providerInstanceId: instanceId } : {}),
          threadId: input.threadId,
          payload: { message: "Vegvisir session started" },
          raw: { source: "acp.vegvisir.extension", payload: started },
        }),
      );
      return ctx.session;
    });

  const sendTurn: VegvisirAdapterShape["sendTurn"] = (input) =>
    Effect.gen(function* () {
      const ctx = yield* requireSession(input.threadId);
      const prompt = input.input?.trim();
      if (!prompt) {
        return yield* new ProviderAdapterValidationError({
          provider: PROVIDER,
          operation: "sendTurn",
          issue: "Turn requires non-empty text input.",
        });
      }
      const turnId = TurnId.make(crypto.randomUUID());
      ctx.activeTurnId = turnId;
      ctx.session = {
        ...ctx.session,
        activeTurnId: turnId,
        status: "running",
        updatedAt: yield* nowIso,
      };
      yield* offer(
        makeRuntimeEvent({
          type: "turn.started",
          provider: PROVIDER,
          ...(instanceId ? { providerInstanceId: instanceId } : {}),
          threadId: input.threadId,
          turnId,
          payload: { model: ctx.session.model },
        }),
      );
      const completed = yield* Effect.tryPromise({
        try: () => sendBridgeRequest(ctx, "turn.send", { content: prompt }),
        catch: (cause) =>
          new ProviderAdapterRequestError({
            provider: PROVIDER,
            method: "turn.send",
            detail: cause instanceof Error ? cause.message : String(cause),
            cause,
          }),
      });
      ctx.turns.push({ id: turnId, items: [completed.payload] });
      ctx.session = {
        ...ctx.session,
        activeTurnId: turnId,
        status: "ready",
        updatedAt: yield* nowIso,
      };
      yield* offer(
        makeRuntimeEvent({
          type: "turn.completed",
          provider: PROVIDER,
          ...(instanceId ? { providerInstanceId: instanceId } : {}),
          threadId: input.threadId,
          turnId,
          payload: { state: "completed", stopReason: null },
          raw: { source: "acp.vegvisir.extension", payload: completed },
        }),
      );
      return { threadId: input.threadId, turnId, resumeCursor: ctx.session.resumeCursor };
    });

  return {
    provider: PROVIDER,
    capabilities: { sessionModelSwitch: "unsupported" },
    startSession,
    sendTurn,
    interruptTurn: (threadId) =>
      requireSession(threadId).pipe(
        Effect.tap((ctx) => Effect.sync(() => closeContext(ctx))),
        Effect.asVoid,
      ),
    respondToRequest: (threadId, requestId, decision) =>
      requireSession(threadId).pipe(
        Effect.flatMap((ctx) =>
          Effect.tryPromise({
            try: () =>
              sendBridgeRequest(
                ctx,
                decision === "acceptForSession"
                  ? "approvals.approveSession"
                  : decision === "accept"
                    ? "approvals.approveOnce"
                    : "approvals.deny",
                { id: requestId },
              ),
            catch: (cause) =>
              new ProviderAdapterRequestError({
                provider: PROVIDER,
                method: "approvals.respond",
                detail: cause instanceof Error ? cause.message : String(cause),
                cause,
              }),
          }),
        ),
        Effect.asVoid,
      ),
    respondToUserInput: () => Effect.void,
    stopSession: (threadId) =>
      requireSession(threadId).pipe(
        Effect.tap((ctx) => Effect.sync(() => closeContext(ctx))),
        Effect.tap(() => Effect.sync(() => sessions.delete(String(threadId)))),
        Effect.asVoid,
      ),
    listSessions: () => Effect.succeed([...sessions.values()].map((ctx) => ctx.session)),
    hasSession: (threadId) => Effect.succeed(sessions.has(String(threadId))),
    readThread: (threadId) =>
      requireSession(threadId).pipe(Effect.map((ctx) => ({ threadId, turns: ctx.turns }))),
    rollbackThread: (threadId) =>
      requireSession(threadId).pipe(Effect.map((ctx) => ({ threadId, turns: ctx.turns }))),
    stopAll: () =>
      Effect.sync(() => {
        for (const ctx of sessions.values()) closeContext(ctx);
        sessions.clear();
      }),
    get streamEvents() {
      return Stream.fromQueue(runtimeEvents);
    },
  } satisfies VegvisirAdapterShape;
});
