import type { AgentAction, AgentSessionResult, AuditFinding, AuditResult, AuditSeverity, CrawlResult, LoopResult } from "../types.js";
import type { GraphqlAuditResult } from "../security/graphql-audit.js";
import type { OwaspAuditResult } from "../security/owasp-audit.js";

export interface HtmlReportOptions {
  title?: string;
  includeJsonAppendix?: boolean;
}


export function renderOwaspAuditHtmlReport(result: OwaspAuditResult, options: HtmlReportOptions = {}): string {
  const title = options.title ?? "Solarium OWASP Passive Audit Report";
  const body: string[] = [];
  body.push(`<h1>${h(title)}</h1>`);
  body.push(`<section class="card"><dl>${row("Standard", result.standard)}${row("Profile", result.profile)}${row("URL", result.url)}${result.finalUrl ? row("Final URL", result.finalUrl) : ""}${result.title ? row("Page title", result.title) : ""}${row("Status", result.ok ? "OK" : "Failed")}${row("Started", result.startedAt)}${row("Finished", result.finishedAt)}${result.error ? row("Error", result.error) : ""}</dl></section>`);

  body.push(`<h2>Checks</h2><ul>${result.checks.map((check) => `<li>${h(check)}</li>`).join("")}</ul>`);

  body.push(`<h2>Severity Summary</h2>`);
  body.push(`<table><thead><tr><th>Severity</th><th>Count</th></tr></thead><tbody>${(["high", "medium", "low", "info"] as AuditSeverity[]).map((severity) => `<tr><td><span class="badge ${severity}">${severity}</span></td><td>${result.summary[severity] ?? 0}</td></tr>`).join("")}</tbody></table>`);

  body.push(`<h2>OWASP Summary</h2>`);
  if (result.owaspSummary.length === 0) {
    body.push(`<p>No OWASP-mapped findings were reported.</p>`);
  } else {
    body.push(`<table><thead><tr><th>OWASP category</th><th>Findings</th><th>High</th><th>Medium</th><th>Low</th><th>Info</th></tr></thead><tbody>${result.owaspSummary.map((entry) => `<tr><td>${h(entry.category)}</td><td>${entry.count}</td><td>${entry.severities.high}</td><td>${entry.severities.medium}</td><td>${entry.severities.low}</td><td>${entry.severities.info}</td></tr>`).join("")}</tbody></table>`);
  }

  if (result.top10Summary?.length) {
    body.push(`<h2>OWASP Top 10 Scanner Coverage</h2>`);
    body.push(`<table><thead><tr><th>Category</th><th>Status</th><th>Findings</th><th>Highest severity</th><th>Automated checks</th></tr></thead><tbody>${result.top10Summary.map((entry) => `<tr><td>${h(`${entry.id} ${entry.name}`)}</td><td>${h(entry.status)}</td><td>${entry.findingCount}</td><td>${h(entry.highestSeverity ?? "none")}</td><td>${h(entry.automatedChecks.join(", "))}</td></tr>`).join("")}</tbody></table>`);
    const limitations = result.top10Summary.filter((entry) => entry.limitations);
    if (limitations.length) body.push(`<h3>Top 10 limitations</h3><ul>${limitations.map((entry) => `<li><strong>${h(`${entry.id} ${entry.name}`)}:</strong> ${h(entry.limitations ?? "")}</li>`).join("")}</ul>`);
  }

  appendNetworkPolicy(body, result.networkPolicy);

  body.push(`<h2>Findings</h2>`);
  body.push(result.findings.length ? result.findings.map((finding, index) => renderFinding(finding, index + 1)).join("\n") : `<p>No findings were reported by the current OWASP passive checks.</p>`);
  appendJsonAppendix(body, result, options);
  return page(title, body.join("\n"));
}

export function renderGraphqlAuditHtmlReport(result: GraphqlAuditResult, options: HtmlReportOptions = {}): string {
  const title = options.title ?? "Solarium GraphQL Audit Report";
  const body: string[] = [];
  body.push(`<h1>${h(title)}</h1>`);
  body.push(`<section class="card"><dl>${row("URL", result.url)}${result.endpoint ? row("Endpoint", result.endpoint) : ""}${row("Status", result.ok ? "OK" : "Completed with errors")}${row("Started", result.startedAt)}${row("Finished", result.finishedAt)}${result.error ? row("Error", result.error) : ""}</dl></section>`);
  body.push(`<h2>Summary</h2>`);
  body.push(`<table><thead><tr><th>Severity</th><th>Count</th></tr></thead><tbody>${(["high", "medium", "low", "info"] as AuditSeverity[]).map((severity) => `<tr><td><span class="badge ${severity}">${severity}</span></td><td>${result.summary[severity] ?? 0}</td></tr>`).join("")}</tbody></table>`);
  appendNetworkPolicy(body, result.networkPolicy);
  body.push(`<h2>Endpoint Probes</h2><ul>${result.endpointProbes.map((probe) => `<li>${probe.ok ? "OK" : "No"}: ${h(probe.url)}${probe.status ? ` (${probe.status})` : ""}${probe.typename ? ` → ${h(probe.typename)}` : ""}${probe.error ? ` — ${h(probe.error)}` : ""}</li>`).join("")}</ul>`);
  if (result.inventory) {
    body.push(`<h2>Schema Operation Inventory</h2><ul><li>Query fields: ${result.inventory.queryFields.map((field) => `<code>${h(field)}</code>`).join(", ") || "none"}</li><li>Mutation fields: ${result.inventory.mutationFields.map((field) => `<code>${h(field)}</code>`).join(", ") || "none"}</li><li>Subscription fields: ${result.inventory.subscriptionFields.map((field) => `<code>${h(field)}</code>`).join(", ") || "none"}</li>${result.inventory.sensitiveFields.length ? `<li>Sensitive-looking fields: ${result.inventory.sensitiveFields.map((field) => `<code>${h(field)}</code>`).join(", ")}</li>` : ""}${result.inventory.dangerousFields.length ? `<li>Dangerous-looking operations: ${result.inventory.dangerousFields.map((field) => `<code>${h(field)}</code>`).join(", ")}</li>` : ""}</ul>`);
  }
  body.push(`<h2>Findings</h2>`);
  body.push(result.findings.length ? result.findings.map((finding, index) => renderFinding(finding, index + 1)).join("\n") : `<p>No GraphQL findings were reported by the current checks.</p>`);
  appendJsonAppendix(body, result, options);
  return page(title, body.join("\n"));
}

export function renderAuditHtmlReport(result: AuditResult, options: HtmlReportOptions = {}): string {
  const title = options.title ?? "Solarium Audit Report";
  const body: string[] = [];
  body.push(`<h1>${h(title)}</h1>`);
  body.push(`<section class="card"><dl>${row("URL", result.url)}${result.finalUrl ? row("Final URL", result.finalUrl) : ""}${result.title ? row("Page title", result.title) : ""}${row("Status", result.ok ? "OK" : "Failed")}${row("Started", result.startedAt)}${row("Finished", result.finishedAt)}${result.error ? row("Error", result.error) : ""}</dl></section>`);

  body.push(`<h2>Summary</h2>`);
  body.push(`<table><thead><tr><th>Severity</th><th>Count</th></tr></thead><tbody>${(["high", "medium", "low", "info"] as AuditSeverity[]).map((severity) => `<tr><td><span class="badge ${severity}">${severity}</span></td><td>${result.summary[severity] ?? 0}</td></tr>`).join("")}</tbody></table>`);

  appendNetworkPolicy(body, result.networkPolicy);

  body.push(`<h2>Findings</h2>`);
  if (result.findings.length === 0) {
    body.push(`<p>No findings were reported by the current passive checks.</p>`);
  } else {
    body.push(result.findings.map((finding, index) => renderFinding(finding, index + 1)).join("\n"));
  }

  if (result.observation) {
    body.push(`<h2>Page Observation Snapshot</h2>`);
    body.push(`<ul><li>Links observed: ${result.observation.links.length}</li><li>Buttons observed: ${result.observation.buttons.length}</li><li>Inputs observed: ${result.observation.inputs.length}</li><li>Forms observed: ${result.observation.forms.length}</li><li>Console events observed: ${result.observation.console.length}</li><li>Network events observed: ${result.observation.network.length}</li></ul>`);
  }

  appendJsonAppendix(body, result, options);
  return page(title, body.join("\n"));
}

export function renderCrawlHtmlReport(result: CrawlResult, options: HtmlReportOptions = {}): string {
  const title = options.title ?? "Solarium Crawl Report";
  const body: string[] = [];
  body.push(`<h1>${h(title)}</h1>`);
  body.push(`<section class="card"><dl>${row("Start URL", result.startUrl)}${row("Status", result.ok ? "OK" : "Completed with errors")}${row("Pages visited", String(result.pageCount))}${row("Max pages", String(result.maxPages))}${row("Max depth", String(result.maxDepth))}${row("Started", result.startedAt)}${row("Finished", result.finishedAt)}</dl></section>`);
  appendNetworkPolicy(body, result.networkPolicy);

  body.push(`<h2>Pages</h2>`);
  if (result.pages.length === 0) {
    body.push(`<p>No pages were visited.</p>`);
  } else {
    for (const crawlPage of result.pages) {
      body.push(`<section class="card"><h3>${h(crawlPage.title || crawlPage.finalUrl || crawlPage.url)}</h3><dl>${row("URL", crawlPage.url)}${crawlPage.finalUrl && crawlPage.finalUrl !== crawlPage.url ? row("Final URL", crawlPage.finalUrl) : ""}${row("Depth", String(crawlPage.depth))}${row("Status", crawlPage.ok ? "OK" : "Failed")}${row("Links discovered", String(crawlPage.discoveredLinks.length))}${row("Forms discovered", String(crawlPage.forms.length))}${crawlPage.observationPath ? row("Observation", crawlPage.observationPath) : ""}${crawlPage.screenshotPath ? row("Screenshot", crawlPage.screenshotPath) : ""}${crawlPage.error ? row("Error", crawlPage.error) : ""}</dl>${renderForms(crawlPage.forms)}</section>`);
    }
  }

  appendJsonAppendix(body, result, options);
  return page(title, body.join("\n"));
}

export function renderSessionHtmlReport(result: AgentSessionResult, options: HtmlReportOptions = {}): string {
  const title = options.title ?? "Solarium Session Report";
  const body: string[] = [];
  body.push(`<h1>${h(title)}</h1>`);
  body.push(`<section class="card"><dl>${row("Session ID", result.sessionId)}${row("Status", result.ok ? "OK" : "Completed with errors")}${row("Steps", String(result.steps.length))}${row("Started", result.startedAt)}${row("Finished", result.finishedAt)}</dl></section>`);
  appendNetworkPolicy(body, result.networkPolicy);

  body.push(`<h2>Steps</h2>`);
  for (const step of result.steps) {
    body.push(`<section class="card"><h3>Step ${step.index + 1}: ${h(step.action.type)}</h3><dl>${row("Status", step.ok ? "OK" : "Failed")}${row("URL", step.url)}${step.title ? row("Title", step.title) : ""}${row("Started", step.startedAt)}${row("Finished", step.finishedAt)}${step.screenshotPath ? row("Screenshot", step.screenshotPath) : ""}${step.extracted ? row("Extracted", `${step.extracted.format}, ${step.extracted.content.length} characters`) : ""}${step.observation ? row("Observed links", String(step.observation.links.length)) + row("Observed forms", String(step.observation.forms.length)) + row("Observed network events", String(step.observation.network.length)) : ""}${step.error ? row("Error", step.error) : ""}</dl></section>`);
  }

  appendJsonAppendix(body, result, options);
  return page(title, body.join("\n"));
}

export function renderLoopHtmlReport(result: LoopResult, options: HtmlReportOptions = {}): string {
  const title = options.title ?? "Solarium Loop Report";
  const body: string[] = [];
  body.push(`<h1>${h(title)}</h1>`);
  body.push(`<section class="card"><dl>${row("Loop ID", result.loopId)}${row("Start URL", result.url)}${result.goal ? row("Goal", result.goal) : ""}${row("Status", result.ok ? "OK" : "Completed with errors")}${row("Iterations", String(result.iterations.length))}${result.stopReason ? row("Stop reason", result.stopReason) : ""}${row("Started", result.startedAt)}${row("Finished", result.finishedAt)}</dl></section>`);
  appendNetworkPolicy(body, result.networkPolicy);

  body.push(`<h2>Iterations</h2>`);
  if (result.iterations.length === 0) {
    body.push(`<p>No loop iterations were executed.</p>`);
  } else {
    for (const iteration of result.iterations) {
      body.push(`<section class="card"><h3>Iteration ${iteration.index + 1}</h3><dl>${row("Status", iteration.ok ? "OK" : "Failed")}${row("URL", iteration.url)}${iteration.title ? row("Title", iteration.title) : ""}${row("Candidates discovered", String(iteration.candidateCount))}${row("Actions executed", String(iteration.actions.length))}${row("Selected candidates", String(iteration.selectedCandidates.length))}${row("Started", iteration.startedAt)}${row("Finished", iteration.finishedAt)}${iteration.stopReason ? row("Stop reason", iteration.stopReason) : ""}${iteration.screenshotPath ? row("Screenshot", iteration.screenshotPath) : ""}${iteration.observation ? row("Observed links", String(iteration.observation.links.length)) + row("Observed forms", String(iteration.observation.forms.length)) + row("Observed network events", String(iteration.observation.network.length)) : ""}${iteration.error ? row("Error", iteration.error) : ""}</dl>${renderActions(iteration.actions)}${renderNotes(iteration.notes)}</section>`);
    }
  }

  appendJsonAppendix(body, result, options);
  return page(title, body.join("\n"));
}

function renderFinding(finding: AuditFinding, number: number): string {
  const standardRows = `${finding.standard ? row("Standard", finding.standard) : ""}${finding.owasp ? row("OWASP", finding.owasp.top10) : ""}${finding.owasp?.asvs?.length ? row("ASVS", finding.owasp.asvs.join(", ")) : ""}`;
  return `<article class="finding card ${h(finding.severity)}"><h3>${number}. ${h(finding.title)}</h3><dl>${row("ID", finding.id)}${row("Severity", finding.severity)}${row("Category", finding.category)}${standardRows}${row("Description", finding.description)}${row("Recommendation", finding.recommendation)}</dl>${finding.evidence ? `<h4>Evidence</h4><pre>${h(JSON.stringify(finding.evidence, null, 2))}</pre>` : ""}</article>`;
}

function renderForms(forms: { method: string; action: string; fields: unknown[] }[]): string {
  if (forms.length === 0) return "";
  return `<h4>Forms</h4><ul>${forms.map((form) => `<li><code>${h(form.method.toUpperCase())}</code> ${h(form.action || "(current URL)")} (${form.fields.length} fields)</li>`).join("")}</ul>`;
}

function renderActions(actions: AgentAction[]): string {
  if (actions.length === 0) return "";
  return `<h4>Actions</h4><ol>${actions.map((action) => `<li><code>${h(action.type)}</code> ${h(describeAction(action))}</li>`).join("")}</ol>`;
}

function renderNotes(notes: string[]): string {
  if (notes.length === 0) return "";
  return `<h4>Planner notes</h4><ul>${notes.map((note) => `<li>${h(note)}</li>`).join("")}</ul>`;
}

function appendNetworkPolicy(body: string[], summary: { allowedRequests: number; blockedRequests: number; rateLimitedRequests: number } | undefined): void {
  if (!summary) return;
  body.push(`<h2>Network Policy Summary</h2><ul><li>Allowed requests: ${summary.allowedRequests}</li><li>Blocked requests: ${summary.blockedRequests}</li><li>Rate-limited requests: ${summary.rateLimitedRequests}</li></ul>`);
}

function appendJsonAppendix(body: string[], result: unknown, options: HtmlReportOptions): void {
  if (!options.includeJsonAppendix) return;
  body.push(`<h2>JSON Appendix</h2><pre>${h(JSON.stringify(result, null, 2))}</pre>`);
}

function row(label: string, value: string): string {
  return `<dt>${h(label)}</dt><dd>${h(value)}</dd>`;
}

function describeAction(action: AgentAction): string {
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

function page(title: string, body: string): string {
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>${h(title)}</title>
  <style>
    :root { color-scheme: light dark; --bg: #0b1020; --card: #111a33; --text: #e8eefc; --muted: #a8b3cf; --line: #263552; --accent: #7cc7ff; }
    body { margin: 0; padding: 2rem; font-family: Inter, ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif; background: var(--bg); color: var(--text); line-height: 1.5; }
    h1, h2, h3 { line-height: 1.2; }
    a { color: var(--accent); }
    .card { background: var(--card); border: 1px solid var(--line); border-radius: 12px; padding: 1rem; margin: 1rem 0; box-shadow: 0 8px 24px rgba(0,0,0,.18); }
    dl { display: grid; grid-template-columns: minmax(10rem, 18rem) 1fr; gap: .45rem 1rem; }
    dt { color: var(--muted); font-weight: 700; }
    dd { margin: 0; overflow-wrap: anywhere; }
    table { border-collapse: collapse; width: 100%; margin: 1rem 0; }
    th, td { border: 1px solid var(--line); padding: .6rem; text-align: left; }
    th { background: rgba(255,255,255,.05); }
    pre { overflow: auto; padding: 1rem; border-radius: 10px; background: #050814; border: 1px solid var(--line); }
    code { background: rgba(255,255,255,.08); padding: .1rem .25rem; border-radius: 4px; }
    .badge { display: inline-block; padding: .15rem .5rem; border-radius: 999px; font-weight: 700; }
    .high { background: #6e1b1b; color: #ffdada; }
    .medium { background: #6e511b; color: #fff1c2; }
    .low { background: #1d4d28; color: #d5ffd9; }
    .info { background: #1d3f66; color: #d8ecff; }
  </style>
</head>
<body>
${body}
</body>
</html>`;
}

function h(value: string): string {
  return value.replace(/[&<>"]/g, (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[char] ?? char);
}
