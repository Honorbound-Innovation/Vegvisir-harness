import type {
  AgentSessionResult,
  AuditFinding,
  AuditResult,
  AuditSeverity,
  CrawlResult,
  LoopResult
} from "../types.js";
import type { GraphqlAuditResult } from "../security/graphql-audit.js";
import type { OwaspAuditResult } from "../security/owasp-audit.js";

export interface MarkdownReportOptions {
  title?: string;
  includeJsonAppendix?: boolean;
}


export function renderOwaspAuditMarkdownReport(result: OwaspAuditResult, options: MarkdownReportOptions = {}): string {
  const lines: string[] = [];
  lines.push(`# ${escapeMarkdown(options.title ?? "Solarium OWASP Passive Audit Report")}`);
  lines.push("");
  lines.push(`- **Standard:** ${result.standard}`);
  lines.push(`- **Profile:** ${result.profile}`);
  lines.push(`- **URL:** ${result.url}`);
  if (result.finalUrl) lines.push(`- **Final URL:** ${result.finalUrl}`);
  if (result.title) lines.push(`- **Page title:** ${escapeMarkdown(result.title)}`);
  lines.push(`- **Status:** ${result.ok ? "OK" : "Failed"}`);
  lines.push(`- **Started:** ${result.startedAt}`);
  lines.push(`- **Finished:** ${result.finishedAt}`);
  if (result.error) lines.push(`- **Error:** ${escapeMarkdown(result.error)}`);
  lines.push("");

  lines.push("## Checks");
  lines.push("");
  for (const check of result.checks) lines.push(`- ${escapeMarkdown(check)}`);
  lines.push("");

  lines.push("## Severity Summary");
  lines.push("");
  lines.push("| Severity | Count |");
  lines.push("| --- | ---: |");
  for (const severity of ["high", "medium", "low", "info"] as AuditSeverity[]) {
    lines.push(`| ${severity} | ${result.summary[severity] ?? 0} |`);
  }
  lines.push("");

  lines.push("## OWASP Summary");
  lines.push("");
  if (result.owaspSummary.length === 0) {
    lines.push("No OWASP-mapped findings were reported.");
  } else {
    lines.push("| OWASP category | Findings | High | Medium | Low | Info |");
    lines.push("| --- | ---: | ---: | ---: | ---: | ---: |");
    for (const row of result.owaspSummary) {
      lines.push(`| ${escapeMarkdown(row.category)} | ${row.count} | ${row.severities.high} | ${row.severities.medium} | ${row.severities.low} | ${row.severities.info} |`);
    }
  }
  lines.push("");

  if (result.top10Summary?.length) {
    lines.push("## OWASP Top 10 Scanner Coverage");
    lines.push("");
    lines.push("| Category | Status | Findings | Highest severity | Automated checks |");
    lines.push("| --- | --- | ---: | --- | --- |");
    for (const row of result.top10Summary) {
      lines.push(`| ${escapeMarkdown(`${row.id} ${row.name}`)} | ${row.status} | ${row.findingCount} | ${row.highestSeverity ?? "none"} | ${row.automatedChecks.map(escapeMarkdown).join(", ")} |`);
    }
    lines.push("");
    const limitations = result.top10Summary.filter((row) => row.limitations);
    if (limitations.length) {
      lines.push("### Top 10 limitations");
      lines.push("");
      for (const row of limitations) lines.push(`- **${escapeMarkdown(`${row.id} ${row.name}`)}:** ${escapeMarkdown(row.limitations ?? "")}`);
      lines.push("");
    }
  }

  appendNetworkPolicy(lines, result.networkPolicy);

  lines.push("## Findings");
  lines.push("");
  if (result.findings.length === 0) {
    lines.push("No findings were reported by the current OWASP passive checks.");
    lines.push("");
  } else {
    result.findings.forEach((finding, index) => {
      lines.push(renderFindingMarkdown(finding, index + 1));
      lines.push("");
    });
  }

  appendJsonAppendix(lines, result, options);
  return lines.join("\n");
}

export function renderGraphqlAuditMarkdownReport(result: GraphqlAuditResult, options: MarkdownReportOptions = {}): string {
  const lines: string[] = [];
  lines.push(`# ${escapeMarkdown(options.title ?? "Solarium GraphQL Audit Report")}`);
  lines.push("");
  lines.push(`- **URL:** ${result.url}`);
  if (result.endpoint) lines.push(`- **Endpoint:** ${result.endpoint}`);
  lines.push(`- **Status:** ${result.ok ? "OK" : "Completed with errors"}`);
  lines.push(`- **Started:** ${result.startedAt}`);
  lines.push(`- **Finished:** ${result.finishedAt}`);
  if (result.error) lines.push(`- **Error:** ${escapeMarkdown(result.error)}`);
  lines.push("");

  lines.push("## Summary");
  lines.push("");
  lines.push("| Severity | Count |");
  lines.push("| --- | ---: |");
  for (const severity of ["high", "medium", "low", "info"] as AuditSeverity[]) {
    lines.push(`| ${severity} | ${result.summary[severity] ?? 0} |`);
  }
  lines.push("");

  appendNetworkPolicy(lines, result.networkPolicy);

  lines.push("## Endpoint Probes");
  lines.push("");
  for (const probe of result.endpointProbes) {
    lines.push(`- ${probe.ok ? "OK" : "No"}: ${probe.url}${probe.status ? ` (${probe.status})` : ""}${probe.typename ? ` → ${probe.typename}` : ""}${probe.error ? ` — ${escapeMarkdown(probe.error)}` : ""}`);
  }
  lines.push("");

  if (result.inventory) {
    lines.push("## Schema Operation Inventory");
    lines.push("");
    lines.push(`- **Query fields:** ${result.inventory.queryFields.length ? result.inventory.queryFields.map((field) => `\`${field}\``).join(", ") : "none"}`);
    lines.push(`- **Mutation fields:** ${result.inventory.mutationFields.length ? result.inventory.mutationFields.map((field) => `\`${field}\``).join(", ") : "none"}`);
    lines.push(`- **Subscription fields:** ${result.inventory.subscriptionFields.length ? result.inventory.subscriptionFields.map((field) => `\`${field}\``).join(", ") : "none"}`);
    if (result.inventory.sensitiveFields.length) lines.push(`- **Sensitive-looking fields:** ${result.inventory.sensitiveFields.map((field) => `\`${field}\``).join(", ")}`);
    if (result.inventory.dangerousFields.length) lines.push(`- **Dangerous-looking operations:** ${result.inventory.dangerousFields.map((field) => `\`${field}\``).join(", ")}`);
    lines.push("");
  }

  lines.push("## Findings");
  lines.push("");
  if (result.findings.length === 0) {
    lines.push("No GraphQL findings were reported by the current checks.");
    lines.push("");
  } else {
    result.findings.forEach((finding, index) => {
      lines.push(renderFindingMarkdown(finding, index + 1));
      lines.push("");
    });
  }

  appendJsonAppendix(lines, result, options);
  return lines.join("\n");
}

export function renderAuditMarkdownReport(result: AuditResult, options: MarkdownReportOptions = {}): string {
  const lines: string[] = [];
  lines.push(`# ${escapeMarkdown(options.title ?? "Solarium Audit Report")}`);
  lines.push("");
  lines.push(`- **URL:** ${result.url}`);
  if (result.finalUrl) lines.push(`- **Final URL:** ${result.finalUrl}`);
  if (result.title) lines.push(`- **Page title:** ${escapeMarkdown(result.title)}`);
  lines.push(`- **Status:** ${result.ok ? "OK" : "Failed"}`);
  lines.push(`- **Started:** ${result.startedAt}`);
  lines.push(`- **Finished:** ${result.finishedAt}`);
  if (result.error) lines.push(`- **Error:** ${escapeMarkdown(result.error)}`);
  lines.push("");

  lines.push("## Summary");
  lines.push("");
  lines.push("| Severity | Count |");
  lines.push("| --- | ---: |");
  for (const severity of ["high", "medium", "low", "info"] as AuditSeverity[]) {
    lines.push(`| ${severity} | ${result.summary[severity] ?? 0} |`);
  }
  lines.push("");

  appendNetworkPolicy(lines, result.networkPolicy);

  lines.push("## Findings");
  lines.push("");
  if (result.findings.length === 0) {
    lines.push("No findings were reported by the current passive checks.");
    lines.push("");
  } else {
    result.findings.forEach((finding, index) => {
      lines.push(renderFindingMarkdown(finding, index + 1));
      lines.push("");
    });
  }

  if (result.observation) {
    lines.push("## Page Observation Snapshot");
    lines.push("");
    lines.push(`- **Links observed:** ${result.observation.links.length}`);
    lines.push(`- **Buttons observed:** ${result.observation.buttons.length}`);
    lines.push(`- **Inputs observed:** ${result.observation.inputs.length}`);
    lines.push(`- **Forms observed:** ${result.observation.forms.length}`);
    lines.push(`- **Console events observed:** ${result.observation.console.length}`);
    lines.push(`- **Network events observed:** ${result.observation.network.length}`);
    lines.push("");
  }

  appendJsonAppendix(lines, result, options);
  return lines.join("\n");
}

export function renderCrawlMarkdownReport(result: CrawlResult, options: MarkdownReportOptions = {}): string {
  const lines: string[] = [];
  lines.push(`# ${escapeMarkdown(options.title ?? "Solarium Crawl Report")}`);
  lines.push("");
  lines.push(`- **Start URL:** ${result.startUrl}`);
  lines.push(`- **Status:** ${result.ok ? "OK" : "Completed with errors"}`);
  lines.push(`- **Pages visited:** ${result.pageCount}`);
  lines.push(`- **Max pages:** ${result.maxPages}`);
  lines.push(`- **Max depth:** ${result.maxDepth}`);
  lines.push(`- **Started:** ${result.startedAt}`);
  lines.push(`- **Finished:** ${result.finishedAt}`);
  lines.push("");

  appendNetworkPolicy(lines, result.networkPolicy);

  lines.push("## Pages");
  lines.push("");
  if (result.pages.length === 0) {
    lines.push("No pages were visited.");
    lines.push("");
  } else {
    for (const page of result.pages) {
      lines.push(`### ${escapeMarkdown(page.title || page.finalUrl || page.url)}`);
      lines.push("");
      lines.push(`- **URL:** ${page.url}`);
      if (page.finalUrl && page.finalUrl !== page.url) lines.push(`- **Final URL:** ${page.finalUrl}`);
      lines.push(`- **Depth:** ${page.depth}`);
      lines.push(`- **Status:** ${page.ok ? "OK" : "Failed"}`);
      lines.push(`- **Links discovered:** ${page.discoveredLinks.length}`);
      lines.push(`- **Forms discovered:** ${page.forms.length}`);
      if (page.observationPath) lines.push(`- **Observation:** ${page.observationPath}`);
      if (page.screenshotPath) lines.push(`- **Screenshot:** ${page.screenshotPath}`);
      if (page.error) lines.push(`- **Error:** ${escapeMarkdown(page.error)}`);
      lines.push("");

      if (page.forms.length > 0) {
        lines.push("#### Forms");
        lines.push("");
        for (const form of page.forms) {
          lines.push(`- \`${form.method.toUpperCase()}\` ${form.action || "(current URL)"} (${form.fields.length} fields)`);
        }
        lines.push("");
      }
    }
  }

  appendJsonAppendix(lines, result, options);
  return lines.join("\n");
}

export function renderSessionMarkdownReport(result: AgentSessionResult, options: MarkdownReportOptions = {}): string {
  const lines: string[] = [];
  lines.push(`# ${escapeMarkdown(options.title ?? "Solarium Session Report")}`);
  lines.push("");
  lines.push(`- **Session ID:** ${result.sessionId}`);
  lines.push(`- **Status:** ${result.ok ? "OK" : "Completed with errors"}`);
  lines.push(`- **Steps:** ${result.steps.length}`);
  lines.push(`- **Started:** ${result.startedAt}`);
  lines.push(`- **Finished:** ${result.finishedAt}`);
  lines.push("");

  appendNetworkPolicy(lines, result.networkPolicy);

  lines.push("## Steps");
  lines.push("");
  for (const step of result.steps) {
    lines.push(`### Step ${step.index + 1}: ${step.action.type}`);
    lines.push("");
    lines.push(`- **Status:** ${step.ok ? "OK" : "Failed"}`);
    lines.push(`- **URL:** ${step.url}`);
    if (step.title) lines.push(`- **Title:** ${escapeMarkdown(step.title)}`);
    lines.push(`- **Started:** ${step.startedAt}`);
    lines.push(`- **Finished:** ${step.finishedAt}`);
    if (step.screenshotPath) lines.push(`- **Screenshot:** ${step.screenshotPath}`);
    if (step.extracted) lines.push(`- **Extracted:** ${step.extracted.format}, ${step.extracted.content.length} characters`);
    if (step.download) lines.push(`- **Downloaded:** ${step.download.suggestedFilename} → ${step.download.path}`);
    if (step.observation) {
      lines.push(`- **Observed links:** ${step.observation.links.length}`);
      lines.push(`- **Observed forms:** ${step.observation.forms.length}`);
      lines.push(`- **Observed network events:** ${step.observation.network.length}`);
    }
    if (step.error) lines.push(`- **Error:** ${escapeMarkdown(step.error)}`);
    lines.push("");
  }

  appendJsonAppendix(lines, result, options);
  return lines.join("\n");
}

export function renderLoopMarkdownReport(result: LoopResult, options: MarkdownReportOptions = {}): string {
  const lines: string[] = [];
  lines.push(`# ${escapeMarkdown(options.title ?? "Solarium Loop Report")}`);
  lines.push("");
  lines.push(`- **Loop ID:** ${result.loopId}`);
  lines.push(`- **Start URL:** ${result.url}`);
  if (result.goal) lines.push(`- **Goal:** ${escapeMarkdown(result.goal)}`);
  lines.push(`- **Status:** ${result.ok ? "OK" : "Completed with errors"}`);
  lines.push(`- **Iterations:** ${result.iterations.length}`);
  if (result.stopReason) lines.push(`- **Stop reason:** ${escapeMarkdown(result.stopReason)}`);
  lines.push(`- **Started:** ${result.startedAt}`);
  lines.push(`- **Finished:** ${result.finishedAt}`);
  lines.push("");

  appendNetworkPolicy(lines, result.networkPolicy);

  lines.push("## Iterations");
  lines.push("");
  if (result.iterations.length === 0) {
    lines.push("No loop iterations were executed.");
    lines.push("");
  } else {
    for (const iteration of result.iterations) {
      lines.push(`### Iteration ${iteration.index + 1}`);
      lines.push("");
      lines.push(`- **Status:** ${iteration.ok ? "OK" : "Failed"}`);
      lines.push(`- **URL:** ${iteration.url}`);
      if (iteration.title) lines.push(`- **Title:** ${escapeMarkdown(iteration.title)}`);
      lines.push(`- **Candidates discovered:** ${iteration.candidateCount}`);
      lines.push(`- **Actions executed:** ${iteration.actions.length}`);
      lines.push(`- **Selected candidates:** ${iteration.selectedCandidates.length}`);
      lines.push(`- **Started:** ${iteration.startedAt}`);
      lines.push(`- **Finished:** ${iteration.finishedAt}`);
      if (iteration.stopReason) lines.push(`- **Stop reason:** ${escapeMarkdown(iteration.stopReason)}`);
      if (iteration.screenshotPath) lines.push(`- **Screenshot:** ${iteration.screenshotPath}`);
      if (iteration.observation) {
        lines.push(`- **Observed links:** ${iteration.observation.links.length}`);
        lines.push(`- **Observed forms:** ${iteration.observation.forms.length}`);
        lines.push(`- **Observed network events:** ${iteration.observation.network.length}`);
      }
      if (iteration.error) lines.push(`- **Error:** ${escapeMarkdown(iteration.error)}`);
      lines.push("");

      if (iteration.actions.length > 0) {
        lines.push("#### Actions");
        lines.push("");
        for (const [actionIndex, action] of iteration.actions.entries()) {
          lines.push(`- ${actionIndex + 1}. \`${action.type}\` ${escapeMarkdown(describeAction(action))}`);
        }
        lines.push("");
      }

      if (iteration.notes.length > 0) {
        lines.push("#### Planner notes");
        lines.push("");
        for (const note of iteration.notes) lines.push(`- ${escapeMarkdown(note)}`);
        lines.push("");
      }
    }
  }

  appendJsonAppendix(lines, result, options);
  return lines.join("\n");
}

function renderFindingMarkdown(finding: AuditFinding, number: number): string {
  const lines: string[] = [];
  lines.push(`### ${number}. ${escapeMarkdown(finding.title)}`);
  lines.push("");
  lines.push(`- **ID:** \`${finding.id}\``);
  lines.push(`- **Severity:** ${finding.severity}`);
  lines.push(`- **Category:** ${finding.category}`);
  if (finding.standard) lines.push(`- **Standard:** ${escapeMarkdown(finding.standard)}`);
  if (finding.owasp) {
    lines.push(`- **OWASP:** ${escapeMarkdown(finding.owasp.top10)}`);
    if (finding.owasp.asvs?.length) lines.push(`- **ASVS:** ${finding.owasp.asvs.map(escapeMarkdown).join(", ")}`);
  }
  lines.push(`- **Description:** ${escapeMarkdown(finding.description)}`);
  lines.push(`- **Recommendation:** ${escapeMarkdown(finding.recommendation)}`);
  if (finding.evidence) {
    lines.push("");
    lines.push("**Evidence**");
    lines.push("");
    lines.push("```json");
    lines.push(JSON.stringify(finding.evidence, null, 2));
    lines.push("```");
  }
  return lines.join("\n");
}

function appendNetworkPolicy(lines: string[], summary: { allowedRequests: number; blockedRequests: number; rateLimitedRequests: number } | undefined): void {
  if (!summary) return;
  lines.push("## Network Policy Summary");
  lines.push("");
  lines.push(`- **Allowed requests:** ${summary.allowedRequests}`);
  lines.push(`- **Blocked requests:** ${summary.blockedRequests}`);
  lines.push(`- **Rate-limited requests:** ${summary.rateLimitedRequests}`);
  lines.push("");
}

function appendJsonAppendix(lines: string[], result: unknown, options: MarkdownReportOptions): void {
  if (!options.includeJsonAppendix) return;
  lines.push("## JSON Appendix");
  lines.push("");
  lines.push("```json");
  lines.push(JSON.stringify(result, null, 2));
  lines.push("```");
  lines.push("");
}

function describeAction(action: LoopResult["iterations"][number]["actions"][number]): string {
  switch (action.type) {
    case "navigate":
      return action.url;
    case "click":
    case "dblclick":
    case "hover":
      return action.selector;
    case "type":
      return `${action.selector} (${action.text.length} characters)`;
    case "press":
      return `${action.selector} ${action.key}`;
    case "select":
      return `${action.selector} ← ${Array.isArray(action.values) ? action.values.join(", ") : action.values}`;
    case "check":
      return `${action.selector} checked`;
    case "uncheck":
      return `${action.selector} unchecked`;
    case "submit":
      return `${action.selector} submitted`;
    case "wait":
      return `${action.ms}ms`;
    case "waitForSelector":
      return `${action.selector} (${action.state ?? "visible"}, timeout ${action.timeoutMs ?? 30_000}ms)`;
    case "waitForUrl":
      return `${action.url} (timeout ${action.timeoutMs ?? 30_000}ms)`;
    case "screenshot":
      return action.path ?? "(default path)";
    case "extract":
      return `${action.selector ?? "page"} as ${action.format ?? "text"}`;
    case "upload":
      return `${action.selector} ← ${Array.isArray(action.files) ? action.files.join(", ") : action.files}`;
    case "download":
      return `${action.selector} → ${action.path ?? "default downloads directory"}`;
    case "observe":
      return "observe current page";
    default: {
      const exhaustive: never = action;
      return JSON.stringify(exhaustive);
    }
  }
}

function escapeMarkdown(value: string): string {
  return value.replace(/[<>]/g, (match) => (match === "<" ? "&lt;" : "&gt;"));
}
