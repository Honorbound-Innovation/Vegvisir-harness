import test from "node:test";
import assert from "node:assert/strict";

import {
  hostMatches,
  checkUrlScope,
  validateScopePolicy,
  validateSolariumConfig,
  validateActions,
  summarizeAgentAction,
  renderSessionMarkdownReport,
  renderAuditMarkdownReport,
  renderAuditHtmlReport,
  renderGraphqlAuditMarkdownReport,
  renderGraphqlAuditHtmlReport,
  renderOwaspAuditMarkdownReport,
  renderOwaspAuditHtmlReport,
  owaspAudit
} from "../dist/index.js";

test("hostMatches supports exact hosts case-insensitively", () => {
  assert.equal(hostMatches("Example.COM", "example.com"), true);
  assert.equal(hostMatches("www.example.com", "example.com"), false);
});

test("hostMatches supports wildcard subdomains without matching apex", () => {
  assert.equal(hostMatches("app.example.com", "*.example.com"), true);
  assert.equal(hostMatches("deep.app.example.com", "*.example.com"), true);
  assert.equal(hostMatches("example.com", "*.example.com"), false);
});

test("checkUrlScope allows, denies, and blocks URLs deterministically", () => {
  const scope = {
    allowedHosts: ["example.com", "*.example.org"],
    blockedHosts: ["blocked.example.org"],
    authorizationNote: "Authorized test scope"
  };

  assert.equal(checkUrlScope("https://example.com/path", scope).allowed, true);
  assert.equal(checkUrlScope("https://app.example.org/path", scope).allowed, true);
  assert.equal(checkUrlScope("https://blocked.example.org/path", scope).allowed, false);
  assert.match(checkUrlScope("https://outside.test/path", scope).reason, /outside allowed scope/);
  assert.match(checkUrlScope("file:///tmp/x", scope).reason, /scheme is not allowed/);
});

test("validateScopePolicy rejects malformed scope policies", () => {
  assert.throws(() => validateScopePolicy({}), /allowedHosts and\/or blockedHosts/);
  assert.throws(() => validateScopePolicy({ allowedHosts: ["https://example.com"] }), /not a full URL/);
  assert.throws(() => validateScopePolicy({ allowedHosts: ["example.com"], maxRequestsPerMinute: 0 }), /positive number/);
  assert.deepEqual(validateScopePolicy({ allowedHosts: ["EXAMPLE.com"] }).allowedHosts, ["example.com"]);
});

test("validateActions accepts all current action primitives", () => {
  const actions = validateActions([
    { type: "navigate", url: "https://example.com" },
    { type: "click", selector: "button" },
    { type: "dblclick", selector: ".editable" },
    { type: "hover", selector: "nav" },
    { type: "type", selector: "input", text: "hello" },
    { type: "press", selector: "input", key: "Enter" },
    { type: "select", selector: "select", values: ["a", "b"] },
    { type: "check", selector: "#yes" },
    { type: "uncheck", selector: "#no" },
    { type: "submit", selector: "form" },
    { type: "upload", selector: "input[type=file]", files: ["a.txt"] },
    { type: "download", selector: "a.download", path: "downloads/file.txt", timeoutMs: 1000 },
    { type: "wait", ms: 1 },
    { type: "waitForSelector", selector: ".ready", state: "visible", timeoutMs: 1000 },
    { type: "waitForUrl", url: "**/dashboard", timeoutMs: 1000 },
    { type: "screenshot", path: "shot.png", fullPage: true },
    { type: "extract", selector: "main", format: "markdown" },
    { type: "observe" }
  ]);

  assert.equal(actions.length, 18);
});

test("validateActions rejects invalid actions with useful messages", () => {
  assert.throws(() => validateActions({}), /must be a JSON array/);
  assert.throws(() => validateActions([{ type: "hover" }]), /selector must be a non-empty string/);
  assert.throws(() => validateActions([{ type: "waitForSelector", selector: ".x", state: "gone" }]), /state must be/);
  assert.throws(() => validateActions([{ type: "waitForUrl", url: "**", timeoutMs: -1 }]), /non-negative number/);
  assert.throws(() => validateActions([{ type: "unknown" }]), /Unsupported action type/);
});

test("validateSolariumConfig infers actions and scope kinds", () => {
  const actionsResult = validateSolariumConfig([{ type: "hover", selector: "nav" }]);
  assert.equal(actionsResult.ok, true);
  assert.equal(actionsResult.kind, "actions");

  const scopeResult = validateSolariumConfig({ allowedHosts: ["example.com"] });
  assert.equal(scopeResult.ok, true);
  assert.equal(scopeResult.kind, "scope");
});

test("summarizeAgentAction redacts typed text length and includes new waits", () => {
  assert.deepEqual(summarizeAgentAction({ type: "type", selector: "#password", text: "secret" }), {
    type: "type",
    selector: "#password",
    textLength: 6
  });
  assert.deepEqual(summarizeAgentAction({ type: "waitForUrl", url: "**/dashboard", timeoutMs: 1000 }), {
    type: "waitForUrl",
    url: "**/dashboard",
    timeoutMs: 1000
  });
});

test("renderSessionMarkdownReport describes new action types", () => {
  const report = renderSessionMarkdownReport({
    sessionId: "test-session",
    startedAt: "2025-01-01T00:00:00.000Z",
    finishedAt: "2025-01-01T00:00:01.000Z",
    ok: true,
    steps: [
      {
        index: 0,
        action: { type: "hover", selector: "nav" },
        startedAt: "2025-01-01T00:00:00.000Z",
        finishedAt: "2025-01-01T00:00:00.100Z",
        ok: true,
        url: "https://example.com",
        title: "Example"
      },
      {
        index: 1,
        action: { type: "waitForSelector", selector: ".ready", state: "visible", timeoutMs: 1000 },
        startedAt: "2025-01-01T00:00:00.100Z",
        finishedAt: "2025-01-01T00:00:00.200Z",
        ok: true,
        url: "https://example.com",
        title: "Example"
      }
    ]
  });

  assert.match(report, /Step 1: hover/);
  assert.match(report, /Step 2: waitForSelector/);
});


test("audit and GraphQL audit render Markdown and HTML reports", () => {
  const baseFinding = {
    id: "missing-content-security-policy",
    category: "headers",
    severity: "medium",
    title: "Missing CSP",
    description: "Content-Security-Policy is not present.",
    recommendation: "Add an application-specific CSP.",
    evidence: { header: "content-security-policy", value: null }
  };

  const auditResult = {
    url: "https://example.com",
    finalUrl: "https://example.com/",
    title: "Example",
    startedAt: "2025-01-01T00:00:00.000Z",
    finishedAt: "2025-01-01T00:00:01.000Z",
    ok: true,
    findings: [baseFinding],
    summary: { info: 0, low: 0, medium: 1, high: 0 },
    networkPolicy: { allowedRequests: 1, blockedRequests: 0, rateLimitedRequests: 0 }
  };

  const auditMarkdown = renderAuditMarkdownReport(auditResult);
  const auditHtml = renderAuditHtmlReport(auditResult);
  assert.match(auditMarkdown, /Solarium Audit Report/);
  assert.match(auditMarkdown, /Missing CSP/);
  assert.match(auditHtml, /<!doctype html>/);
  assert.match(auditHtml, /Missing CSP/);

  const graphqlResult = {
    url: "https://example.com",
    endpoint: "https://example.com/graphql",
    startedAt: "2025-01-01T00:00:00.000Z",
    finishedAt: "2025-01-01T00:00:01.000Z",
    ok: true,
    endpointCandidates: ["https://example.com/graphql"],
    endpointProbes: [{ url: "https://example.com/graphql", ok: true, status: 200, typename: "Query" }],
    findings: [{ ...baseFinding, id: "graphql-introspection-enabled", category: "graphql", title: "GraphQL introspection is enabled" }],
    summary: { info: 0, low: 0, medium: 1, high: 0 },
    inventory: {
      queryFields: ["viewer"],
      mutationFields: [],
      subscriptionFields: [],
      sensitiveFields: [],
      dangerousFields: []
    },
    networkPolicy: { allowedRequests: 0, blockedRequests: 0, rateLimitedRequests: 0 }
  };

  const gqlMarkdown = renderGraphqlAuditMarkdownReport(graphqlResult);
  const gqlHtml = renderGraphqlAuditHtmlReport(graphqlResult);
  assert.match(gqlMarkdown, /Solarium GraphQL Audit Report/);
  assert.match(gqlMarkdown, /Endpoint Probes/);
  assert.match(gqlHtml, /<!doctype html>/);
  assert.match(gqlHtml, /Schema Operation Inventory/);
});


test("OWASP audit reports render mapped findings", () => {
  const result = {
    schemaVersion: "solarium.owasp-audit.v1",
    standard: "OWASP",
    profile: "passive",
    url: "https://example.com",
    finalUrl: "https://example.com/",
    title: "Example",
    startedAt: "2025-01-01T00:00:00.000Z",
    finishedAt: "2025-01-01T00:00:01.000Z",
    ok: true,
    checks: ["security-headers", "cookie-flags"],
    findings: [
      {
        id: "missing-content-security-policy",
        category: "headers",
        severity: "medium",
        title: "Missing content-security-policy header",
        description: "The response did not include CSP.",
        recommendation: "Add an application-specific CSP.",
        evidence: { header: "content-security-policy", value: null },
        standard: "OWASP",
        owasp: { top10: "A05:2021-Security Misconfiguration", asvs: ["V14 Configuration"] }
      }
    ],
    summary: { info: 0, low: 0, medium: 1, high: 0 },
    owaspSummary: [
      { category: "A05:2021-Security Misconfiguration", count: 1, severities: { info: 0, low: 0, medium: 1, high: 0 } }
    ],
    top10Summary: [
      { id: "A05:2021", name: "Security Misconfiguration", findingCount: 1, highestSeverity: "medium", status: "finding", automatedChecks: ["security headers"] }
    ],
    baseAudit: {
      url: "https://example.com",
      finalUrl: "https://example.com/",
      title: "Example",
      startedAt: "2025-01-01T00:00:00.000Z",
      finishedAt: "2025-01-01T00:00:01.000Z",
      ok: true,
      findings: [],
      summary: { info: 0, low: 0, medium: 0, high: 0 }
    },
    networkPolicy: { allowedRequests: 1, blockedRequests: 0, rateLimitedRequests: 0 }
  };

  const markdown = renderOwaspAuditMarkdownReport(result);
  const html = renderOwaspAuditHtmlReport(result);
  assert.match(markdown, /Solarium OWASP Passive Audit Report/);
  assert.match(markdown, /A05:2021-Security Misconfiguration/);
  assert.match(markdown, /OWASP Top 10 Scanner Coverage/);
  assert.match(html, /<!doctype html>/);
  assert.match(html, /OWASP Summary/);
});


test("OWASP active-authorized requires explicit authorized scope", async () => {
  await assert.rejects(
    () => owaspAudit({ url: "https://example.com", owaspProfile: "active-authorized" }),
    /requires a scope policy with allowedHosts/
  );

  await assert.rejects(
    () => owaspAudit({
      url: "https://example.com",
      owaspProfile: "active-authorized",
      scope: { allowedHosts: ["example.com"] }
    }),
    /requires scope\.authorizationNote/
  );
});

test("OWASP active-authorized report rendering labels active profile", () => {
  const result = {
    schemaVersion: "solarium.owasp-audit.v1",
    standard: "OWASP",
    profile: "active-authorized",
    url: "https://example.com",
    finalUrl: "https://example.com/",
    title: "Example",
    startedAt: "2025-01-01T00:00:00.000Z",
    finishedAt: "2025-01-01T00:00:01.000Z",
    ok: true,
    checks: ["authorized-sensitive-file-exposure-probes"],
    findings: [
      {
        id: "owasp-active-authorized-run-summary",
        category: "active-probe",
        severity: "info",
        title: "Active-authorized probes completed",
        description: "Bounded fixed probe set completed.",
        recommendation: "Keep runs scoped.",
        evidence: { probesSent: 2, maxActiveRequests: 2 },
        standard: "OWASP",
        owasp: { top10: "A05:2021-Security Misconfiguration", asvs: ["V14 Configuration"] }
      }
    ],
    summary: { info: 1, low: 0, medium: 0, high: 0 },
    owaspSummary: [
      { category: "A05:2021-Security Misconfiguration", count: 1, severities: { info: 1, low: 0, medium: 0, high: 0 } }
    ],
    baseAudit: {
      url: "https://example.com",
      finalUrl: "https://example.com/",
      title: "Example",
      startedAt: "2025-01-01T00:00:00.000Z",
      finishedAt: "2025-01-01T00:00:01.000Z",
      ok: true,
      findings: [],
      summary: { info: 0, low: 0, medium: 0, high: 0 }
    },
    networkPolicy: { allowedRequests: 1, blockedRequests: 0, rateLimitedRequests: 0 }
  };

  const markdown = renderOwaspAuditMarkdownReport(result);
  const html = renderOwaspAuditHtmlReport(result);
  assert.match(markdown, /active-authorized/);
  assert.match(markdown, /Active-authorized probes completed/);
  assert.match(html, /active-authorized/);
});


test("OWASP Top 10 active-authorized profile requires explicit authorized scope", async () => {
  await assert.rejects(
    () => owaspAudit({ url: "https://example.com", owaspProfile: "top10-active-authorized" }),
    /requires a scope policy with allowedHosts/
  );
});

test("OWASP Top 10 report rendering includes category coverage", () => {
  const result = {
    schemaVersion: "solarium.owasp-audit.v1",
    standard: "OWASP",
    profile: "top10-passive",
    url: "https://example.com",
    finalUrl: "https://example.com/",
    title: "Example",
    startedAt: "2025-01-01T00:00:00.000Z",
    finishedAt: "2025-01-01T00:00:01.000Z",
    ok: true,
    checks: ["owasp-top10-category-coverage"],
    findings: [
      {
        id: "owasp-top10-broken-access-control-review",
        category: "access-control",
        severity: "info",
        title: "Broken access control requires role/object authorization review",
        description: "Manual review item.",
        recommendation: "Review authorization boundaries.",
        evidence: { automatedStatus: "manual-review-required" },
        standard: "OWASP",
        owasp: { top10: "A01:2021-Broken Access Control", asvs: ["Manual review"] }
      }
    ],
    summary: { info: 1, low: 0, medium: 0, high: 0 },
    owaspSummary: [
      { category: "A01:2021-Broken Access Control", count: 1, severities: { info: 1, low: 0, medium: 0, high: 0 } }
    ],
    top10Summary: [
      {
        id: "A01:2021",
        name: "Broken Access Control",
        findingCount: 1,
        highestSeverity: "info",
        status: "manual-review",
        automatedChecks: ["scope enforcement"],
        limitations: "Requires authenticated roles."
      }
    ],
    baseAudit: {
      url: "https://example.com",
      finalUrl: "https://example.com/",
      title: "Example",
      startedAt: "2025-01-01T00:00:00.000Z",
      finishedAt: "2025-01-01T00:00:01.000Z",
      ok: true,
      findings: [],
      summary: { info: 0, low: 0, medium: 0, high: 0 }
    },
    networkPolicy: { allowedRequests: 1, blockedRequests: 0, rateLimitedRequests: 0 }
  };

  const markdown = renderOwaspAuditMarkdownReport(result);
  const html = renderOwaspAuditHtmlReport(result);
  assert.match(markdown, /OWASP Top 10 Scanner Coverage/);
  assert.match(markdown, /A01:2021 Broken Access Control/);
  assert.match(markdown, /Top 10 limitations/);
  assert.match(html, /OWASP Top 10 Scanner Coverage/);
  assert.match(html, /manual-review/);
});
