import type { Page, Request, Route } from "playwright";
import type { NetworkObservation } from "../types.js";
import { checkUrlScope, type ScopePolicy } from "./scope.js";

export interface NetworkPolicyOptions {
  scope?: ScopePolicy;
  onBlockedRequest?: (event: NetworkObservation) => void;
}

export interface NetworkPolicyStats {
  allowedRequests: number;
  blockedRequests: number;
  rateLimitedRequests: number;
}

export interface NetworkPolicyController {
  stats(): NetworkPolicyStats;
  detach(): Promise<void>;
}

const ROUTE_PATTERN = "**/*";
const PASSTHROUGH_PROTOCOLS = new Set(["about:", "data:", "blob:"]);

export async function attachScopedNetworkPolicy(
  page: Page,
  options: NetworkPolicyOptions = {}
): Promise<NetworkPolicyController> {
  const scope = options.scope;
  const requestTimestamps: number[] = [];
  const stats: NetworkPolicyStats = {
    allowedRequests: 0,
    blockedRequests: 0,
    rateLimitedRequests: 0
  };

  const handler = async (route: Route, request: Request): Promise<void> => {
    const url = request.url();
    const method = request.method();
    const resourceType = request.resourceType();

    try {
      const parsed = new URL(url);

      if (PASSTHROUGH_PROTOCOLS.has(parsed.protocol)) {
        stats.allowedRequests += 1;
        await route.continue();
        return;
      }

      if (!scope) {
        stats.allowedRequests += 1;
        await route.continue();
        return;
      }

      const decision = checkUrlScope(url, scope);
      if (!decision.allowed) {
        stats.blockedRequests += 1;
        options.onBlockedRequest?.({
          url,
          method,
          resourceType,
          ok: false,
          failureText: `blocked by Solarium scope policy: ${decision.reason}`
        });
        await route.abort("blockedbyclient");
        return;
      }

      if (isRateLimited(scope.maxRequestsPerMinute, requestTimestamps)) {
        stats.blockedRequests += 1;
        stats.rateLimitedRequests += 1;
        options.onBlockedRequest?.({
          url,
          method,
          resourceType,
          ok: false,
          failureText: `blocked by Solarium rate limit: maxRequestsPerMinute=${scope.maxRequestsPerMinute}`
        });
        await route.abort("blockedbyclient");
        return;
      }

      stats.allowedRequests += 1;
      await route.continue();
    } catch (error) {
      stats.blockedRequests += 1;
      options.onBlockedRequest?.({
        url,
        method,
        resourceType,
        ok: false,
        failureText: `blocked by Solarium network policy: ${error instanceof Error ? error.message : String(error)}`
      });
      await route.abort("blockedbyclient");
    }
  };

  await page.route(ROUTE_PATTERN, handler);

  return {
    stats: () => ({ ...stats }),
    detach: async () => {
      await page.unroute(ROUTE_PATTERN, handler);
    }
  };
}

function isRateLimited(maxRequestsPerMinute: number | undefined, timestamps: number[]): boolean {
  if (!maxRequestsPerMinute) return false;

  const now = Date.now();
  const windowStart = now - 60_000;

  while (timestamps.length > 0 && timestamps[0] < windowStart) {
    timestamps.shift();
  }

  if (timestamps.length >= maxRequestsPerMinute) {
    return true;
  }

  timestamps.push(now);
  return false;
}
