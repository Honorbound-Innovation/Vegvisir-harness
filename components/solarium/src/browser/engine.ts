import { chromium, firefox, webkit, type Browser, type BrowserContext, type Page } from "playwright";
import { mkdir } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import type {
  BrowserEngine,
  ExtractOptions,
  ExtractResult,
  LaunchOptions,
  NavigateOptions,
  ScreenshotOptions
} from "../types.js";
import { resolveProfile } from "./profile.js";

export class SolariumPage {
  constructor(private readonly page: Page) {}

  raw(): Page {
    return this.page;
  }

  async navigate(url: string, options: NavigateOptions = {}): Promise<void> {
    await this.page.goto(url, {
      waitUntil: options.waitUntil ?? "domcontentloaded",
      timeout: options.timeoutMs ?? 30_000
    });
  }

  async click(selector: string): Promise<void> {
    await this.page.click(selector);
  }

  async dblclick(selector: string): Promise<void> {
    await this.page.dblclick(selector);
  }

  async hover(selector: string): Promise<void> {
    await this.page.hover(selector);
  }

  async type(selector: string, text: string): Promise<void> {
    await this.page.fill(selector, text);
  }

  async press(selector: string, key: string): Promise<void> {
    await this.page.press(selector, key);
  }

  async upload(selector: string, files: string | string[]): Promise<void> {
    await this.page.setInputFiles(selector, files);
  }

  async select(selector: string, values: string | string[]): Promise<string[]> {
    return this.page.selectOption(selector, values);
  }

  async check(selector: string): Promise<void> {
    await this.page.check(selector);
  }

  async uncheck(selector: string): Promise<void> {
    await this.page.uncheck(selector);
  }

  async submit(selector: string): Promise<void> {
    await this.page.locator(selector).first().evaluate((element) => {
      const target = element instanceof HTMLFormElement ? element : element.closest("form");
      if (!target) {
        throw new Error("Selector does not resolve to a form or an element inside a form");
      }
      target.requestSubmit();
    });
  }

  async wait(ms: number): Promise<void> {
    await this.page.waitForTimeout(ms);
  }

  async waitForSelector(
    selector: string,
    options: { state?: "attached" | "detached" | "visible" | "hidden"; timeoutMs?: number } = {}
  ): Promise<void> {
    await this.page.waitForSelector(selector, {
      state: options.state ?? "visible",
      timeout: options.timeoutMs ?? 30_000
    });
  }

  async waitForUrl(url: string, options: { timeoutMs?: number } = {}): Promise<void> {
    await this.page.waitForURL(url, { timeout: options.timeoutMs ?? 30_000 });
  }

  async title(): Promise<string> {
    return this.page.title();
  }

  url(): string {
    return this.page.url();
  }

  async screenshot(options: ScreenshotOptions = {}): Promise<Buffer> {
    return this.page.screenshot({
      path: options.path,
      fullPage: options.fullPage ?? true
    });
  }

  async extract(options: ExtractOptions = {}): Promise<ExtractResult> {
    const selector = options.selector ?? "body";
    const format = options.format ?? "text";
    const locator = this.page.locator(selector).first();

    let content: string;
    if (format === "html") {
      content = await locator.innerHTML();
    } else {
      content = await locator.innerText();
    }

    return {
      url: this.page.url(),
      title: await this.page.title(),
      content,
      format
    };
  }
}

export class SolariumBrowser {
  private constructor(
    private readonly browser: Browser,
    private readonly context: BrowserContext,
    private readonly artifactsDir: string,
    private readonly traceEnabled: boolean,
    private readonly saveStorageStatePath?: string
  ) {}

  static async launch(options: LaunchOptions = {}): Promise<SolariumBrowser> {
    const engine = options.engine ?? "chromium";
    const launcher = getLauncher(engine);
    const profile = resolveProfile(options.profile);
    const artifactsDir = options.artifactsDir ?? ".solarium/artifacts";

    await mkdir(artifactsDir, { recursive: true });
    const downloadsPath = options.downloadsDir ? resolve(options.downloadsDir) : join(artifactsDir, "downloads");
    await mkdir(downloadsPath, { recursive: true });

    const browser = await launcher.launch({
      headless: options.headless ?? true,
      downloadsPath
    });

    const context = await browser.newContext({
      userAgent: profile.userAgent,
      viewport: profile.viewport,
      locale: profile.locale,
      timezoneId: profile.timezoneId,
      extraHTTPHeaders: profile.extraHTTPHeaders,
      colorScheme: profile.colorScheme,
      deviceScaleFactor: profile.deviceScaleFactor,
      isMobile: profile.isMobile,
      hasTouch: profile.hasTouch,
      acceptDownloads: true,
      storageState: options.storageState
    });

    if (profile.permissions?.length) {
      await context.grantPermissions(profile.permissions);
    }

    if (options.trace) {
      await context.tracing.start({ screenshots: true, snapshots: true, sources: true });
    }

    return new SolariumBrowser(browser, context, artifactsDir, options.trace ?? false, options.saveStorageState);
  }

  async newPage(): Promise<SolariumPage> {
    return new SolariumPage(await this.context.newPage());
  }

  async close(): Promise<void> {
    if (this.saveStorageStatePath) {
      await mkdir(dirname(this.saveStorageStatePath), { recursive: true });
      await this.context.storageState({ path: this.saveStorageStatePath });
    }

    if (this.traceEnabled) {
      await this.context.tracing.stop({ path: join(this.artifactsDir, `trace-${Date.now()}.zip`) });
    }
    await this.context.close();
    await this.browser.close();
  }
}

function getLauncher(engine: BrowserEngine) {
  switch (engine) {
    case "chromium":
      return chromium;
    case "firefox":
      return firefox;
    case "webkit":
      return webkit;
    default:
      throw new Error(`Unsupported browser engine: ${engine satisfies never}`);
  }
}
