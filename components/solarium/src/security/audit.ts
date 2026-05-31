import { mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { SolariumBrowser } from "../browser/engine.js";
import { ObservationRecorder } from "../agent/observations.js";
import { NetworkScopeGuard } from "./network.js";
import { assertUrlInScope, type ScopePolicy } from "./scope.js";
import type {
  AuditFinding,
  AuditOptions,
  AuditResult,
  AuditSeverity,
  CookieAuditFinding,
  HeaderAuditFinding,
  MixedContentAuditFinding
} from "../types.js";

const SECURITY_HEADERS = [
  "content-security-policy",
  "strict-transport-security",
  "x-content-type-options",
  "referrer-policy",
  "permissions-policy"
] as const;

/**
 * Run a passive, browser-observed security audit for an in-scope page.
 *
 * The audit intentionally does not fuzz, brute-force, exploit, or submit forms.
 * It loads the page once, observes browser/network evidence, and reports common
 * defensive configuration findings useful to site owners and authorized testers.
 */
export async function audit(options: AuditOptions): Promise<AuditResult> {
  assertUrlInScope(options.url, options.scope);

  const startedAt = new Date().toISOString();
  const findings: AuditFinding[] = [];
  const browser = await SolariumBrowser.launch(options);
  const networkGuard = new NetworkScopeGuard(options.scope);

  try {
    const page = await browser.newPage();
    await networkGuard.attach(page.raw());

    const recorder = new ObservationRecorder(page.raw());
    recorder.attach();

    const response = await page.raw().goto(options.url, {
      waitUntil: options.waitUntil ?? "domcontentloaded",
      timeout: options.timeoutMs ?? 30_000
    });

    assertUrlInScope(page.url(), options.scope);

    if (options.waitAfterNavigationMs && options.waitAfterNavigationMs > 0) {
      await page.wait(options.waitAfterNavigationMs);
    }

    const observation = await recorder.observe(options.observationOptions);
    const finalUrl = observation.url;
    const headers = normalizeHeaders(response?.headers() ?? {});
    const cookies = await page.raw().context().cookies(finalUrl);

    findings.push(...auditHeaders(finalUrl, headers));
    findings.push(...auditCookies(finalUrl, cookies));
    findings.push(...auditMixedContent(finalUrl, observation.network));
    findings.push(...auditForms(finalUrl, observation.forms));

    const result: AuditResult = {
      url: options.url,
      finalUrl,
      title: observation.title,
      startedAt,
      finishedAt: new Date().toISOString(),
      ok: true,
      findings: sortFindings(findings),
      summary: summarizeFindings(findings),
      observation: options.includeObservation ? observation : undefined,
      networkPolicy: networkGuard.summary()
    };

    if (options.outputPath) {
      await mkdir(dirname(options.outputPath), { recursive: true });
      await writeFile(options.outputPath, JSON.stringify(result, null, 2), "utf8");
    }

    return result;
  } catch (error) {
    return {
      url: options.url,
      startedAt,
      finishedAt: new Date().toISOString(),
      ok: false,
      findings: sortFindings(findings),
      summary: summarizeFindings(findings),
      networkPolicy: networkGuard.summary(),
      error: error instanceof Error ? error.message : String(error)
    };
  } finally {
    await browser.close();
  }
}

function auditHeaders(url: string, headers: Record<string, string>): HeaderAuditFinding[] {
  const findings: HeaderAuditFinding[] = [];
  const isHttps = new URL(url).protocol === "https:";

  for (const header of SECURITY_HEADERS) {
    if (!headers[header]) {
      findings.push({
        id: `missing-${header}`,
        category: "headers",
        severity: header === "content-security-policy" ? "medium" : "low",
        title: `Missing ${header} header`,
        description: `The response did not include the ${header} security header.`,
        recommendation: recommendationForHeader(header),
        evidence: { header, value: null }
      });
    }
  }

  const xFrameOptions = headers["x-frame-options"];
  const csp = headers["content-security-policy"];
  if (!xFrameOptions && !csp?.toLowerCase().includes("frame-ancestors")) {
    findings.push({
      id: "missing-clickjacking-control",
      category: "headers",
      severity: "medium",
      title: "Missing clickjacking protection",
      description: "The response did not include X-Frame-Options or a CSP frame-ancestors directive.",
      recommendation: "Add CSP frame-ancestors and/or X-Frame-Options for pages that should not be framed.",
      evidence: { header: "x-frame-options/content-security-policy", value: null }
    });
  }

  if (isHttps && headers["strict-transport-security"]) {
    const hsts = headers["strict-transport-security"].toLowerCase();
    if (!hsts.includes("max-age=") || /max-age\s*=\s*0\b/.test(hsts)) {
      findings.push({
        id: "weak-hsts",
        category: "headers",
        severity: "medium",
        title: "Weak HSTS policy",
        description: "Strict-Transport-Security is present but does not define a useful max-age.",
        recommendation: "Set a meaningful HSTS max-age after validating HTTPS readiness.",
        evidence: { header: "strict-transport-security", value: headers["strict-transport-security"] }
      });
    }
  }

  const xcto = headers["x-content-type-options"];
  if (xcto && xcto.toLowerCase() !== "nosniff") {
    findings.push({
      id: "weak-x-content-type-options",
      category: "headers",
      severity: "low",
      title: "Weak X-Content-Type-Options header",
      description: "X-Content-Type-Options is present but is not set to nosniff.",
      recommendation: "Set X-Content-Type-Options: nosniff.",
      evidence: { header: "x-content-type-options", value: xcto }
    });
  }

  return findings;
}

function auditCookies(url: string, cookies: Awaited<ReturnType<import("playwright").BrowserContext["cookies"]>>): CookieAuditFinding[] {
  const findings: CookieAuditFinding[] = [];
  const isHttps = new URL(url).protocol === "https:";

  for (const cookie of cookies) {
    if (isHttps && !cookie.secure) {
      findings.push({
        id: "cookie-missing-secure",
        category: "cookies",
        severity: "medium",
        title: "Cookie missing Secure flag",
        description: `Cookie ${cookie.name} was set without the Secure flag on an HTTPS page.`,
        recommendation: "Set the Secure flag for cookies that should only be sent over HTTPS.",
        evidence: { name: cookie.name, domain: cookie.domain, path: cookie.path, flag: "Secure" }
      });
    }

    if (!cookie.httpOnly) {
      findings.push({
        id: "cookie-missing-httponly",
        category: "cookies",
        severity: "low",
        title: "Cookie missing HttpOnly flag",
        description: `Cookie ${cookie.name} is accessible to client-side JavaScript.`,
        recommendation: "Set HttpOnly for session or sensitive cookies that JavaScript does not need to read.",
        evidence: { name: cookie.name, domain: cookie.domain, path: cookie.path, flag: "HttpOnly" }
      });
    }

    if (cookie.sameSite === "None" && !cookie.secure) {
      findings.push({
        id: "cookie-samesite-none-without-secure",
        category: "cookies",
        severity: "medium",
        title: "SameSite=None cookie without Secure",
        description: `Cookie ${cookie.name} uses SameSite=None without Secure.`,
        recommendation: "Use Secure with SameSite=None cookies, or choose Lax/Strict where appropriate.",
        evidence: { name: cookie.name, domain: cookie.domain, path: cookie.path, flag: "SameSite/Secure" }
      });
    }
  }

  return findings;
}

function auditMixedContent(url: string, network: AuditResult["observation"] extends infer _ ? import("../types.js").NetworkObservation[] : never): MixedContentAuditFinding[] {
  const pageUrl = new URL(url);
  if (pageUrl.protocol !== "https:") return [];

  return network
    .filter((event) => isHttpUrl(event.url))
    .map((event) => ({
      id: "mixed-content-request",
      category: "mixed-content" as const,
      severity: event.resourceType === "document" ? "high" as const : "medium" as const,
      title: "HTTPS page requested HTTP resource",
      description: "The page loaded or attempted to load an insecure HTTP resource from an HTTPS context.",
      recommendation: "Serve subresources over HTTPS and update hard-coded http:// URLs.",
      evidence: { url: event.url, resourceType: event.resourceType, status: event.status }
    }));
}

function auditForms(url: string, forms: import("../types.js").PageFormObservation[]): AuditFinding[] {
  const pageUrl = new URL(url);
  const findings: AuditFinding[] = [];

  for (const form of forms) {
    const action = form.action || url;
    const actionUrl = new URL(action, url);
    const hasPassword = form.fields.some((field) => field.type === "password");

    if (pageUrl.protocol === "https:" && actionUrl.protocol === "http:") {
      findings.push({
        id: "insecure-form-action",
        category: "forms",
        severity: hasPassword ? "high" : "medium",
        title: "HTTPS page has insecure HTTP form action",
        description: "A form on an HTTPS page submits to an HTTP endpoint.",
        recommendation: "Submit forms over HTTPS only.",
        evidence: { action: actionUrl.toString(), method: form.method, hasPassword }
      });
    }

    if (hasPassword && pageUrl.protocol !== "https:") {
      findings.push({
        id: "password-form-on-http-page",
        category: "forms",
        severity: "high",
        title: "Password field on non-HTTPS page",
        description: "A password field is present on a page that is not loaded over HTTPS.",
        recommendation: "Serve login and credential collection pages over HTTPS only.",
        evidence: { action: actionUrl.toString(), method: form.method }
      });
    }
  }

  return findings;
}

function normalizeHeaders(headers: Record<string, string>): Record<string, string> {
  return Object.fromEntries(Object.entries(headers).map(([key, value]) => [key.toLowerCase(), value]));
}

function recommendationForHeader(header: string): string {
  switch (header) {
    case "content-security-policy":
      return "Add a Content-Security-Policy tailored to the application to reduce XSS impact.";
    case "strict-transport-security":
      return "For HTTPS sites, add Strict-Transport-Security after validating HTTPS readiness.";
    case "x-content-type-options":
      return "Set X-Content-Type-Options: nosniff.";
    case "referrer-policy":
      return "Set a Referrer-Policy such as strict-origin-when-cross-origin or stricter.";
    case "permissions-policy":
      return "Set Permissions-Policy to limit browser features the site does not need.";
    default:
      return "Review and configure this security header.";
  }
}

function isHttpUrl(url: string): boolean {
  try {
    return new URL(url).protocol === "http:";
  } catch {
    return false;
  }
}

function summarizeFindings(findings: AuditFinding[]): Record<AuditSeverity, number> {
  return findings.reduce<Record<AuditSeverity, number>>(
    (summary, finding) => {
      summary[finding.severity] += 1;
      return summary;
    },
    { info: 0, low: 0, medium: 0, high: 0 }
  );
}

function sortFindings(findings: AuditFinding[]): AuditFinding[] {
  const rank: Record<AuditSeverity, number> = { high: 0, medium: 1, low: 2, info: 3 };
  return [...findings].sort((a, b) => rank[a.severity] - rank[b.severity] || a.id.localeCompare(b.id));
}
