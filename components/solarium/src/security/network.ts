import type { Page, Route } from "playwright";
import { checkUrlScope, type ScopePolicy } from "./scope.js";
import type { NetworkPolicySummary } from "../types.js";

/**
 * Enforces a ScopePolicy at the browser-network boundary.
 *
 * This is intentionally conservative for HTTP(S) requests: if a scope policy is
 * configured, every HTTP(S) request must pass the same host allow/block rules as
 * top-level navigations. Non-HTTP browser-internal URLs such as data:, blob:,
 * about:, and devtools resources are allowed so normal page rendering can still
 * work.
 */
export class NetworkScopeGuard {
  private allowedRequests = 0;
  private blockedRequests = 0;
  private rateLimitedRequests = 0;
  private lastNavigationRequestAt = 0;

  constructor(private readonly scope?: ScopePolicy) {}

  async attach(page: Page): Promise<void> {
    if (!this.scope) return;
    await page.route("**/*", (route) => this.handleRoute(route));
  }

  summary(): NetworkPolicySummary {
    return {
      allowedRequests: this.allowedRequests,
      blockedRequests: this.blockedRequests,
      rateLimitedRequests: this.rateLimitedRequests
    };
  }

  private async handleRoute(route: Route): Promise<void> {
    const request = route.request();
    const url = request.url();

    if (!isHttpUrl(url)) {
      this.allowedRequests += 1;
      await route.continue();
      return;
    }

    const decision = checkUrlScope(url, this.scope);
    if (!decision.allowed) {
      this.blockedRequests += 1;
      await route.abort("blockedbyclient");
      return;
    }

    if (request.isNavigationRequest()) {
      await this.applyNavigationRateLimit();
    }

    this.allowedRequests += 1;
    await route.continue();
  }

  private async applyNavigationRateLimit(): Promise<void> {
    if (!this.scope?.maxRequestsPerMinute) return;

    const minDelayMs = Math.ceil(60_000 / this.scope.maxRequestsPerMinute);
    const now = Date.now();
    const elapsed = now - this.lastNavigationRequestAt;

    if (this.lastNavigationRequestAt > 0 && elapsed < minDelayMs) {
      this.rateLimitedRequests += 1;
      await delay(minDelayMs - elapsed);
    }

    this.lastNavigationRequestAt = Date.now();
  }
}

function isHttpUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
