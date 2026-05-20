// @effect-diagnostics globalTimers:off nodeBuiltinImport:off
import {
  ProviderDriverKind,
  type ServerProvider,
  VegvisirSettings,
  type VegvisirSettings as VegvisirSettingsType,
} from "@t3tools/contracts";
import crypto from "node:crypto";
import { spawn } from "node:child_process";
import * as readline from "node:readline";
import * as Effect from "effect/Effect";
import * as DateTime from "effect/DateTime";
import * as Queue from "effect/Queue";
import * as Schema from "effect/Schema";
import * as Stream from "effect/Stream";

import { TextGenerationError } from "@t3tools/contracts";
import type { TextGenerationShape } from "../../textGeneration/TextGeneration.ts";
import { ProviderDriverError } from "../Errors.ts";
import { mergeProviderInstanceEnvironment } from "../ProviderInstanceEnvironment.ts";
import {
  defaultProviderContinuationIdentity,
  type ProviderDriver,
  type ProviderInstance,
} from "../ProviderDriver.ts";
import { makeManualOnlyProviderMaintenanceCapabilities } from "../providerMaintenance.ts";
import type { ServerProviderShape } from "../Services/ServerProvider.ts";
import { makeVegvisirAdapter } from "../Layers/VegvisirAdapter.ts";

const DRIVER_KIND = ProviderDriverKind.make("vegvisir");
const decodeVegvisirSettings = Schema.decodeSync(VegvisirSettings);

export type VegvisirDriverEnv = never;

interface VegvisirBridgeEvent {
  readonly type?: string;
  readonly id?: string;
  readonly payload?: unknown;
}

interface VegvisirBridgeProviderModel {
  readonly id?: string;
  readonly provider?: string;
  readonly model?: string;
  readonly display_name?: string;
}

function fallbackVegvisirModelOptions(settings: VegvisirSettingsType): ServerProvider["models"] {
  const configuredSlug =
    settings.defaultProvider && settings.defaultModel
      ? `${settings.defaultProvider}/${settings.defaultModel}`
      : settings.defaultModel || "openai-sso/gpt-5.5";
  const candidates = [
    {
      slug: configuredSlug,
      name:
        configuredSlug === settings.defaultModel
          ? `Vegvisir ${settings.defaultModel}`
          : `Vegvisir ${configuredSlug}`,
    },
    { slug: "openai-sso/gpt-5.5", name: "OpenAI SSO GPT-5.5" },
    { slug: "openai-hbse/gpt-5.5", name: "OpenAI HBSE GPT-5.5" },
    { slug: "openai-sso/gpt-5.4", name: "OpenAI SSO GPT-5.4" },
    { slug: "openai-hbse/gpt-5.4", name: "OpenAI HBSE GPT-5.4" },
    { slug: "openai-sso/gpt-5.4-mini", name: "OpenAI SSO GPT-5.4 Mini" },
    { slug: "openai-hbse/gpt-5.4-mini", name: "OpenAI HBSE GPT-5.4 Mini" },
    { slug: "xai-hbse/grok-4.3", name: "xAI HBSE Grok 4.3" },
    { slug: "xai-hbse/grok-4", name: "xAI HBSE Grok 4" },
    { slug: "xai/grok-4.3", name: "xAI Grok 4.3" },
    { slug: "xai/grok-4", name: "xAI Grok 4" },
  ];
  return uniqueModelOptions(candidates);
}

function uniqueModelOptions(
  candidates: ReadonlyArray<{ readonly slug: string; readonly name: string }>,
): ServerProvider["models"] {
  const seen = new Set<string>();
  return candidates.flatMap((model) => {
    if (seen.has(model.slug)) return [];
    seen.add(model.slug);
    return [{ ...model, isCustom: true, capabilities: null }];
  });
}

function parseBridgeProviderModels(
  payload: unknown,
  settings: VegvisirSettingsType,
): ServerProvider["models"] {
  if (!payload || typeof payload !== "object") {
    return fallbackVegvisirModelOptions(settings);
  }
  const providerModels = (payload as { provider_models?: unknown }).provider_models;
  if (!Array.isArray(providerModels)) {
    return fallbackVegvisirModelOptions(settings);
  }
  const candidates = providerModels.flatMap((entry): Array<{ slug: string; name: string }> => {
    if (!entry || typeof entry !== "object") return [];
    const model = entry as VegvisirBridgeProviderModel;
    const slug =
      typeof model.id === "string" && model.id.trim().length > 0
        ? model.id.trim()
        : typeof model.provider === "string" &&
            model.provider.trim().length > 0 &&
            typeof model.model === "string" &&
            model.model.trim().length > 0
          ? `${model.provider.trim()}/${model.model.trim()}`
          : null;
    if (!slug) return [];
    const name =
      typeof model.display_name === "string" && model.display_name.trim().length > 0
        ? `${slug.split("/")[0]} ${model.display_name.trim()}`
        : slug;
    return [{ slug, name }];
  });
  return uniqueModelOptions([...candidates, ...fallbackVegvisirModelOptions(settings)]);
}

function discoverVegvisirModels(
  settings: VegvisirSettingsType,
  refresh: boolean,
): Promise<ServerProvider["models"]> {
  return new Promise((resolve, reject) => {
    const binary = settings.binaryPath || "vegvisir";
    const requestId = crypto.randomUUID();
    const child = spawn(binary, ["app-server", "--workspace", process.cwd()], {
      stdio: ["pipe", "pipe", "pipe"],
      env: process.env,
    });
    const stderr: string[] = [];
    const lineReader = readline.createInterface({ input: child.stdout });
    let settled = false;
    const done = (result: ServerProvider["models"] | Error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      lineReader.close();
      child.kill();
      if (result instanceof Error) {
        reject(result);
      } else {
        resolve(result);
      }
    };
    const timer = setTimeout(
      () =>
        done(
          new Error(
            `Vegvisir model discovery timed out${stderr.length ? `: ${stderr.join("").slice(0, 500)}` : ""}`,
          ),
        ),
      refresh ? 120_000 : 15_000,
    );
    child.stderr.on("data", (chunk) => stderr.push(String(chunk)));
    child.on("error", (error) => done(error));
    child.on("exit", (code) => {
      if (!settled && code !== 0) {
        done(new Error(`Vegvisir model discovery exited with code ${code ?? "unknown"}`));
      }
    });
    lineReader.on("line", (line) => {
      let event: VegvisirBridgeEvent;
      try {
        event = JSON.parse(line) as VegvisirBridgeEvent;
      } catch {
        return;
      }
      if (event.type === "server.ready") {
        child.stdin.write(
          `${JSON.stringify({
            id: requestId,
            method: "models.list",
            params: { refresh },
          })}\n`,
        );
        return;
      }
      if (event.id === requestId && event.type === "models.list") {
        done(parseBridgeProviderModels(event.payload, settings));
      }
      if (event.id === requestId && event.type === "error") {
        done(new Error(JSON.stringify(event.payload)));
      }
    });
  });
}

function makeSnapshot(input: {
  readonly instanceId: ProviderInstance["instanceId"];
  readonly displayName: string | undefined;
  readonly accentColor: string | undefined;
  readonly enabled: boolean;
  readonly settings: VegvisirSettingsType;
  readonly models: ServerProvider["models"];
}): ServerProvider {
  return {
    instanceId: input.instanceId,
    driver: DRIVER_KIND,
    displayName: input.displayName ?? "Vegvisir",
    ...(input.accentColor ? { accentColor: input.accentColor } : {}),
    continuation: { groupKey: `${DRIVER_KIND}:instance:${input.instanceId}` },
    showInteractionModeToggle: true,
    enabled: input.enabled,
    installed: true,
    version: null,
    status: input.enabled ? "ready" : "disabled",
    auth: {
      status: "unknown",
      type: "HBSE / configured provider",
      label: "Vegvisir managed auth",
    },
    checkedAt: DateTime.formatIso(DateTime.nowUnsafe()),
    models: input.models,
    slashCommands: [
      { name: "/system", description: "Show or set the Vegvisir system prompt" },
      { name: "/tools", description: "Inspect Vegvisir tools and permission state" },
      { name: "/approvals", description: "Manage Vegvisir approval requests" },
      { name: "/memory", description: "Inspect CMS-v2 memory status" },
      { name: "/hbse", description: "Inspect HBSE status and provider onboarding" },
      { name: "/hbse onboard", description: "Show deterministic HBSE onboarding helper usage" },
      { name: "/provider", description: "Switch Vegvisir provider" },
      { name: "/model", description: "Switch Vegvisir model" },
      { name: "/diff", description: "Show the current workspace diff" },
    ],
    skills: [],
  };
}

const makeRefreshableServerProvider = (
  input: {
    readonly instanceId: ProviderInstance["instanceId"];
    readonly displayName: string | undefined;
    readonly accentColor: string | undefined;
    readonly enabled: boolean;
    readonly settings: VegvisirSettingsType;
    readonly initialModels: ServerProvider["models"];
  },
  maintenanceCapabilities: ServerProviderShape["maintenanceCapabilities"],
): Effect.Effect<ServerProviderShape> =>
  Effect.gen(function* () {
    const changes = yield* Queue.unbounded<ServerProvider>();
    let currentSnapshot = makeSnapshot({ ...input, models: input.initialModels });
    return {
      maintenanceCapabilities,
      getSnapshot: Effect.sync(() => currentSnapshot),
      refresh: Effect.gen(function* () {
        const models = yield* Effect.promise(() =>
          discoverVegvisirModels(input.settings, true).catch(() => currentSnapshot.models),
        );
        currentSnapshot = makeSnapshot({ ...input, models });
        yield* Queue.offer(changes, currentSnapshot);
        return currentSnapshot;
      }),
      streamChanges: Stream.fromQueue(changes),
    };
  });

const unavailableTextGeneration = (operation: string) =>
  Effect.fail(
    new TextGenerationError({
      operation,
      detail: "Vegvisir overlay text generation is not implemented yet.",
    }),
  );

const textGeneration: TextGenerationShape = {
  generateCommitMessage: () => unavailableTextGeneration("generateCommitMessage"),
  generatePrContent: () => unavailableTextGeneration("generatePrContent"),
  generateBranchName: () => unavailableTextGeneration("generateBranchName"),
  generateThreadTitle: () => unavailableTextGeneration("generateThreadTitle"),
};

export const VegvisirDriver: ProviderDriver<VegvisirSettingsType, VegvisirDriverEnv> = {
  driverKind: DRIVER_KIND,
  metadata: {
    displayName: "Vegvisir",
    supportsMultipleInstances: true,
  },
  configSchema: VegvisirSettings,
  defaultConfig: (): VegvisirSettingsType => decodeVegvisirSettings({}),
  create: ({ instanceId, displayName, accentColor, environment, enabled, config }) =>
    Effect.gen(function* () {
      const effectiveConfig = { ...config, enabled };
      const adapter = yield* makeVegvisirAdapter(effectiveConfig, {
        instanceId,
        environment: mergeProviderInstanceEnvironment(environment),
      });
      const initialModels = yield* Effect.promise(() =>
        discoverVegvisirModels(effectiveConfig, true).catch(() =>
          fallbackVegvisirModelOptions(effectiveConfig),
        ),
      );
      const maintenanceCapabilities = makeManualOnlyProviderMaintenanceCapabilities({
        provider: DRIVER_KIND,
        packageName: null,
      });
      const snapshot = yield* makeRefreshableServerProvider(
        { instanceId, displayName, accentColor, enabled, settings: effectiveConfig, initialModels },
        maintenanceCapabilities,
      );
      return {
        instanceId,
        driverKind: DRIVER_KIND,
        continuationIdentity: defaultProviderContinuationIdentity({
          driverKind: DRIVER_KIND,
          instanceId,
        }),
        displayName,
        accentColor,
        enabled,
        snapshot,
        adapter,
        textGeneration,
      } satisfies ProviderInstance;
    }).pipe(
      Effect.mapError(
        (cause: unknown) =>
          new ProviderDriverError({
            driver: DRIVER_KIND,
            instanceId,
            detail: `Failed to build Vegvisir provider: ${
              cause instanceof Error ? cause.message : String(cause)
            }`,
            cause,
          }),
      ),
    ),
};
