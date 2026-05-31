import { writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { mkdir } from "node:fs/promises";
import { SolariumBrowser } from "../browser/engine.js";
import type { BrowseResult, LaunchOptions, ObservationOptions } from "../types.js";
import { assertUrlInScope, type ScopePolicy } from "../security/scope.js";
import { attachScopedNetworkPolicy } from "../security/network-policy.js";
import { ObservationRecorder } from "./observations.js";

export interface BrowseOptions extends LaunchOptions {
  url: string;
  screenshotPath?: string;
  extractText?: boolean;
  observe?: boolean;
  observationPath?: string;
  observationOptions?: ObservationOptions;
  scope?: ScopePolicy;
}

export async function browse(options: BrowseOptions): Promise<BrowseResult> {
  assertUrlInScope(options.url, options.scope);

  const browser = await SolariumBrowser.launch(options);
  try {
    const page = await browser.newPage();
    const recorder = new ObservationRecorder(page.raw());
    recorder.attach();
    const networkPolicy = await attachScopedNetworkPolicy(page.raw(), {
      scope: options.scope,
      onBlockedRequest: (event) => recorder.recordNetworkEvent(event)
    });

    await page.navigate(options.url);

    const title = await page.title();
    let extractedText: string | undefined;

    if (options.extractText) {
      extractedText = (await page.extract({ format: "text" })).content;
    }

    if (options.screenshotPath) {
      await page.screenshot({ path: options.screenshotPath, fullPage: true });
    }

    const result: BrowseResult = {
      url: page.url(),
      title,
      screenshotPath: options.screenshotPath,
      extractedText,
      networkPolicy: networkPolicy.stats()
    };

    if (options.observe || options.observationPath) {
      result.observation = await recorder.observe(options.observationOptions);
      result.observationPath = options.observationPath;

      if (options.observationPath) {
        await mkdir(dirname(options.observationPath), { recursive: true });
        await writeFile(options.observationPath, JSON.stringify(result.observation, null, 2), "utf8");
      }
    }

    return result;
  } finally {
    await browser.close();
  }
}
