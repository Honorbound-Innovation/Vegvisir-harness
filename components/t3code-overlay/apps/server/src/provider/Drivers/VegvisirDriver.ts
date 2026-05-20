import {
  ProviderDriverKind,
  type ServerProvider,
  VegvisirSettings,
  type VegvisirSettings as VegvisirSettingsType,
} from "@t3tools/contracts";
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

function makeSnapshot(input: {
  readonly instanceId: ProviderInstance["instanceId"];
  readonly displayName: string | undefined;
  readonly accentColor: string | undefined;
  readonly enabled: boolean;
  readonly settings: VegvisirSettingsType;
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
    models: [
      {
        slug: input.settings.defaultModel || "gpt-5.5",
        name: input.settings.defaultModel || "GPT-5.5",
        isCustom: true,
        capabilities: null,
      },
    ],
    slashCommands: [
      { name: "/system", description: "Show or set the Vegvisir system prompt" },
      { name: "/tools", description: "Inspect Vegvisir tools and permission state" },
      { name: "/approvals", description: "Manage Vegvisir approval requests" },
      { name: "/memory", description: "Inspect CMS-v2 memory status" },
      { name: "/diff", description: "Show the current workspace diff" },
    ],
    skills: [],
  };
}

const makeStaticServerProvider = (
  snapshot: ServerProvider,
  maintenanceCapabilities: ServerProviderShape["maintenanceCapabilities"],
): Effect.Effect<ServerProviderShape> =>
  Effect.gen(function* () {
    const changes = yield* Queue.unbounded<ServerProvider>();
    return {
      maintenanceCapabilities,
      getSnapshot: Effect.succeed(snapshot),
      refresh: Effect.succeed(snapshot),
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
      const maintenanceCapabilities = makeManualOnlyProviderMaintenanceCapabilities({
        provider: DRIVER_KIND,
        packageName: null,
      });
      const snapshot = yield* makeStaticServerProvider(
        makeSnapshot({ instanceId, displayName, accentColor, enabled, settings: effectiveConfig }),
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
