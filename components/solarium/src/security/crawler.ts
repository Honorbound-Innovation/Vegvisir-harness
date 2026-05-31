import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { SolariumBrowser } from "../browser/engine.js";
import { assertUrlInScope, checkUrlScope, type ScopePolicy } from "./scope.js";
import { attachScopedNetworkPolicy } from "./network-policy.js";
import type {
  CrawlOptions,
  CrawlPageResult,
  CrawlResult,
  PageFormObservation,
  PageLinkObservation
} from "../types.js";
import { ObservationRecorder } from "../agent/observations.js";
import { summarizeCrawlPage, summarizeNetworkPolicy } from "../reporting/events.js";

const DEFAULT_MAX_PAGES = 10;
const DEFAULT_MAX_DEPTH = 1;
const DEFAULT_WAIT_AFTER_NAVIGATION_MS = 0;

export async function crawl(options: CrawlOptions): Promise<CrawlResult> {
  if (!options.scope?.allowedHosts?.length) {
    throw new Error("Crawler requires a scope policy with allowedHosts");
  }

  assertUrlInScope(options.startUrl, options.scope);

  const maxPages = options.maxPages ?? DEFAULT_MAX_PAGES;
  const maxDepth = options.maxDepth ?? DEFAULT_MAX_DEPTH;
  const waitAfterNavigationMs = options.waitAfterNavigationMs ?? DEFAULT_WAIT_AFTER_NAVIGATION_MS;
  const startedAt = new Date().toISOString();
  const discovered = new Set<string>();
  const visited = new Set<string>();
  const queue: Array<{ url: string; depth: number }> = [{ url: normalizeUrl(options.startUrl), depth: 0 }];
  const pages: CrawlPageResult[] = [];

  await options.eventLogger?.emit("crawl.started", {
    startUrl: options.startUrl,
    startedAt,
    maxPages,
    maxDepth
  });

  const browser = await SolariumBrowser.launch(options);
  try {
    const page = await browser.newPage();
    const recorder = new ObservationRecorder(page.raw());
    recorder.attach();
    const networkPolicy = await attachScopedNetworkPolicy(page.raw(), {
      scope: options.scope,
      onBlockedRequest: (event) => recorder.recordNetworkEvent(event)
    });

    while (queue.length > 0 && pages.length < maxPages) {
      const next = queue.shift();
      if (!next) break;

      const url = normalizeUrl(next.url);
      if (visited.has(url)) continue;
      visited.add(url);

      await options.eventLogger?.emit("crawl.page.started", {
        index: pages.length,
        url,
        depth: next.depth
      });

      const pageStartedAt = new Date().toISOString();
      const result: CrawlPageResult = {
        url,
        depth: next.depth,
        ok: false,
        discoveredLinks: [],
        forms: [],
        startedAt: pageStartedAt,
        finishedAt: pageStartedAt
      };

      try {
        assertUrlInScope(url, options.scope);
        await page.navigate(url, { waitUntil: options.waitUntil, timeoutMs: options.timeoutMs });

        if (waitAfterNavigationMs > 0) {
          await page.wait(waitAfterNavigationMs);
        }

        const observation = await recorder.observe(options.observationOptions);
        result.ok = true;
        result.finalUrl = observation.url;
        result.title = observation.title;
        result.discoveredLinks = inScopeLinks(observation.links, options.scope);
        result.forms = observation.forms;
        result.observation = options.includeObservations ? observation : undefined;
        result.networkPolicy = networkPolicy.stats();

        if (options.evidenceDir) {
          await mkdir(options.evidenceDir, { recursive: true });
          const baseName = `page-${pages.length}`;
          const observationPath = join(options.evidenceDir, `${baseName}.observation.json`);
          await writeFile(observationPath, JSON.stringify(observation, null, 2), "utf8");
          result.observationPath = observationPath;

          if (options.screenshots) {
            const screenshotPath = join(options.evidenceDir, `${baseName}.png`);
            await page.screenshot({ path: screenshotPath, fullPage: true });
            result.screenshotPath = screenshotPath;
          }
        }

        if (next.depth < maxDepth) {
          for (const link of result.discoveredLinks) {
            const normalized = normalizeUrl(link.href);
            if (!visited.has(normalized) && !discovered.has(normalized)) {
              discovered.add(normalized);
              queue.push({ url: normalized, depth: next.depth + 1 });
            }
          }
        }
      } catch (error) {
        result.ok = false;
        result.networkPolicy = networkPolicy.stats();
        result.error = error instanceof Error ? error.message : String(error);
      } finally {
        result.finishedAt = new Date().toISOString();
        pages.push(result);
        await options.eventLogger?.emit("crawl.page.finished", {
          index: pages.length - 1,
          ...summarizeCrawlPage(result)
        });
        await rateLimitDelay(options.scope);
      }
    }

    const finalResult = {
      startUrl: options.startUrl,
      startedAt,
      finishedAt: new Date().toISOString(),
      ok: pages.every((page) => page.ok),
      pages,
      pageCount: pages.length,
      maxPages,
      maxDepth,
      networkPolicy: networkPolicy.stats()
    } satisfies CrawlResult;

    await options.eventLogger?.emit("network.policy.summary", {
      crawl: true,
      networkPolicy: summarizeNetworkPolicy(finalResult.networkPolicy)
    });
    await options.eventLogger?.emit("crawl.finished", {
      startUrl: options.startUrl,
      ok: finalResult.ok,
      startedAt: finalResult.startedAt,
      finishedAt: finalResult.finishedAt,
      pageCount: finalResult.pageCount,
      maxPages,
      maxDepth
    });

    return finalResult;
  } finally {
    await browser.close();
    await options.eventLogger?.close();
  }
}

function inScopeLinks(links: PageLinkObservation[], scope: ScopePolicy): PageLinkObservation[] {
  const seen = new Set<string>();
  const scoped: PageLinkObservation[] = [];

  for (const link of links) {
    const normalized = normalizeUrl(link.href);
    if (seen.has(normalized)) continue;

    const decision = checkUrlScope(normalized, scope);
    if (decision.allowed) {
      seen.add(normalized);
      scoped.push({ ...link, href: normalized });
    }
  }

  return scoped;
}

function normalizeUrl(url: string): string {
  const parsed = new URL(url);
  parsed.hash = "";
  return parsed.toString();
}

async function rateLimitDelay(scope: ScopePolicy): Promise<void> {
  if (!scope.maxRequestsPerMinute) return;
  const delayMs = Math.ceil(60_000 / scope.maxRequestsPerMinute);
  await new Promise((resolve) => setTimeout(resolve, delayMs));
}

export type { CrawlOptions, CrawlPageResult, CrawlResult, PageFormObservation };
