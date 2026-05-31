import { readFile, writeFile } from "node:fs/promises";
import { dirname, basename } from "node:path";
import { mkdir } from "node:fs/promises";
import type { AgentAction } from "../types.js";
import { validateActions } from "../config/validate.js";
import type { EvidenceRunManifest } from "../reporting/evidence.js";

export interface WorkflowSeedOptions {
  name: string;
  actions: AgentAction[];
  output?: string;
  description?: string;
  source?: string;
  evidence?: EvidenceRunManifest;
  scope?: unknown;
  inputs?: WorkflowSeedInput[];
  safetyNotes?: string[];
}

export interface WorkflowSeedInput {
  name: string;
  description: string;
  required?: boolean;
  secret?: boolean;
}

export interface WorkflowSeedFromFilesOptions {
  name?: string;
  actionsPath: string;
  evidencePath?: string;
  output?: string;
  description?: string;
  source?: string;
}

export async function createWorkflowSeedFromFiles(options: WorkflowSeedFromFilesOptions): Promise<string> {
  const actionsRaw = await readFile(options.actionsPath, "utf8");
  const actions = validateActions(JSON.parse(actionsRaw) as unknown);
  const evidence = options.evidencePath
    ? (JSON.parse(await readFile(options.evidencePath, "utf8")) as EvidenceRunManifest)
    : undefined;

  return createWorkflowSeedMarkdown({
    name: options.name ?? inferSeedName(options.actionsPath, evidence),
    actions,
    evidence,
    output: options.output,
    description: options.description,
    source: options.source ?? options.actionsPath,
    scope: evidence?.scope
  });
}

export async function createWorkflowSeedMarkdown(options: WorkflowSeedOptions): Promise<string> {
  const markdown = renderWorkflowSeedMarkdown(options);
  if (options.output) {
    await mkdir(dirname(options.output), { recursive: true });
    await writeFile(options.output, markdown, "utf8");
  }
  return markdown;
}

export function renderWorkflowSeedMarkdown(options: WorkflowSeedOptions): string {
  const lines: string[] = [];
  const title = options.name.trim();
  lines.push(`# ${title}`);
  lines.push("");
  lines.push("## Purpose");
  lines.push("");
  lines.push(options.description?.trim() || "Reusable Solarium browser workflow seed generated from an action trace.");
  lines.push("");

  lines.push("## Source");
  lines.push("");
  if (options.source) lines.push(`- **Source:** ${options.source}`);
  if (options.evidence) {
    lines.push(`- **Evidence schema:** ${options.evidence.schemaVersion}`);
    lines.push(`- **Run ID:** ${options.evidence.runId}`);
    lines.push(`- **Run kind:** ${options.evidence.kind}`);
    lines.push(`- **Status:** ${options.evidence.status}`);
    if (options.evidence.target?.url) lines.push(`- **Target URL:** ${options.evidence.target.url}`);
    lines.push(`- **Artifacts:** ${options.evidence.artifacts.summary.files} files, ${options.evidence.artifacts.summary.totalBytes} bytes`);
  }
  lines.push("");

  lines.push("## Inputs");
  lines.push("");
  const inputs = options.inputs ?? inferInputs(options.actions);
  if (inputs.length === 0) {
    lines.push("- None identified. Review before publishing as a skill.");
  } else {
    for (const input of inputs) {
      lines.push(`- \`${input.name}\`${input.required === false ? "" : " required"}${input.secret ? " secret-ref" : ""}: ${input.description}`);
    }
  }
  lines.push("");

  lines.push("## Scope and authorization");
  lines.push("");
  lines.push("- Run only against explicitly authorized targets.");
  lines.push("- Prefer passing a Solarium `scope` object with `allowedHosts` and an `authorizationNote`.");
  lines.push("- Do not place plaintext credentials in action JSON, workflow seeds, logs, or reports.");
  if (options.scope) {
    lines.push("");
    lines.push("```json");
    lines.push(JSON.stringify(options.scope, null, 2));
    lines.push("```");
  }
  lines.push("");

  lines.push("## Action sequence");
  lines.push("");
  lines.push("```json");
  lines.push(JSON.stringify(options.actions.map(redactActionForSeed), null, 2));
  lines.push("```");
  lines.push("");

  lines.push("## Validation checklist");
  lines.push("");
  lines.push("- Confirm selectors are stable enough for reuse.");
  lines.push("- Replace environment-specific URLs with parameters where appropriate.");
  lines.push("- Replace typed secret or account-specific values with secret refs or runtime inputs.");
  lines.push("- Add a final `waitForSelector`, `waitForUrl`, `extract`, `observe`, or `screenshot` action that proves success.");
  lines.push("- Run the workflow through `solarium session` with an explicit scope policy before promoting it to a Skiller skill.");
  lines.push("");

  lines.push("## Safety notes");
  lines.push("");
  for (const note of options.safetyNotes ?? defaultSafetyNotes(options.actions)) {
    lines.push(`- ${note}`);
  }
  lines.push("");

  lines.push("## Suggested Skiller framing");
  lines.push("");
  lines.push("- **Capability:** browser workflow execution with Solarium");
  lines.push("- **When to use:** user requests a repeatable, authorized web workflow matching the target and scope.");
  lines.push("- **Tools:** `solarium.scopeCheck`, `solarium.session`, `solarium.manifest`");
  lines.push("- **Evidence:** screenshots, event logs, action results, and `solarium.evidence.v1` manifest.");
  lines.push("");

  return lines.join("\n");
}

function redactActionForSeed(action: AgentAction): AgentAction | Record<string, unknown> {
  if (action.type === "type") {
    return { ...action, text: "${input.form_values}" };
  }
  return action;
}

function inferInputs(actions: AgentAction[]): WorkflowSeedInput[] {
  const inputs: WorkflowSeedInput[] = [];
  if (actions.some((action) => action.type === "navigate" || action.type === "waitForUrl")) {
    inputs.push({ name: "target_url", description: "Authorized base or target URL for the workflow." });
  }
  if (actions.some((action) => action.type === "type")) {
    inputs.push({ name: "form_values", description: "Non-secret form values required by typed actions. Secrets must be provided through a secret broker/ref.", required: false });
  }
  if (actions.some((action) => action.type === "upload")) {
    inputs.push({ name: "upload_files", description: "Local file paths to upload during the workflow." });
  }
  return inputs;
}

function defaultSafetyNotes(actions: AgentAction[]): string[] {
  const notes = [
    "Treat this as a draft seed until reviewed by a human operator.",
    "Keep all browsing inside the caller-provided Solarium scope policy."
  ];
  if (actions.some((action) => action.type === "type")) {
    notes.push("Typed actions may contain sensitive or environment-specific values; parameterize before reuse.");
  }
  if (actions.some((action) => action.type === "download" || action.type === "upload")) {
    notes.push("File transfer actions can expose local or remote data; verify paths and evidence handling.");
  }
  return notes;
}

function inferSeedName(actionsPath: string, evidence?: EvidenceRunManifest): string {
  if (evidence?.runId) return `Solarium workflow seed: ${evidence.runId}`;
  return `Solarium workflow seed: ${basename(actionsPath).replace(/\.[^.]+$/, "")}`;
}
