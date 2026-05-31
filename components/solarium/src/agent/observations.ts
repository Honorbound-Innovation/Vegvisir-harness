import type { ConsoleMessage, Page, Request, Response } from "playwright";
import type {
  ConsoleLogObservation,
  NetworkObservation,
  ObservationOptions,
  PageObservation
} from "../types.js";

const DEFAULT_MAX_TEXT_CHARS = 20_000;
const DEFAULT_MAX_ELEMENTS = 100;
const DEFAULT_MAX_CONSOLE_EVENTS = 100;
const DEFAULT_MAX_NETWORK_EVENTS = 200;

export class ObservationRecorder {
  private readonly consoleEvents: ConsoleLogObservation[] = [];
  private readonly networkEvents = new Map<string, NetworkObservation>();

  constructor(private readonly page: Page) {}

  attach(): void {
    this.page.on("console", (message) => this.recordConsole(message));
    this.page.on("request", (request) => this.recordRequest(request));
    this.page.on("response", (response) => this.recordResponse(response));
    this.page.on("requestfailed", (request) => this.recordRequestFailure(request));
  }

  recordNetworkEvent(event: NetworkObservation): void {
    this.networkEvents.set(`${event.method} ${event.url}`, event);
  }

  async observe(options: ObservationOptions = {}): Promise<PageObservation> {
    const maxTextChars = options.maxTextChars ?? DEFAULT_MAX_TEXT_CHARS;
    const maxElements = options.maxElements ?? DEFAULT_MAX_ELEMENTS;
    const maxConsoleEvents = options.maxConsoleEvents ?? DEFAULT_MAX_CONSOLE_EVENTS;
    const maxNetworkEvents = options.maxNetworkEvents ?? DEFAULT_MAX_NETWORK_EVENTS;

    // Use a string expression instead of passing a function object here.
    // tsx/esbuild can wrap function literals with helper symbols such as
    // `__name`; those helpers do not exist in the browser page context and
    // cause Playwright page.evaluate failures. Keep this browser-side code
    // self-contained JavaScript.
    const domObservation = (await this.page.evaluate(`(() => {
      const maxTextChars = ${JSON.stringify(maxTextChars)};
      const maxElements = ${JSON.stringify(maxElements)};
      const cleanText = (value) => (value ?? "").replace(/\s+/g, " ").trim();

      const visibleText = cleanText(document.body?.innerText ?? "").slice(0, maxTextChars);

      const links = Array.from(document.querySelectorAll("a[href]"))
        .slice(0, maxElements)
        .map((anchor) => ({
          text: cleanText(anchor.innerText || anchor.getAttribute("aria-label") || anchor.href),
          href: anchor.href,
          target: anchor.getAttribute("target"),
          rel: anchor.getAttribute("rel")
        }));

      const buttons = Array.from(
        document.querySelectorAll("button, input[type='button'], input[type='submit'], input[type='reset']")
      )
        .slice(0, maxElements)
        .map((button, index) => ({
          text: cleanText(
            button instanceof HTMLInputElement
              ? button.value || button.getAttribute("aria-label") || button.name
              : button.innerText || button.getAttribute("aria-label")
          ),
          type: button.getAttribute("type"),
          disabled: button.disabled,
          selectorHint: button.id ? "#" + CSS.escape(button.id) : "button-or-input:" + index
        }));

      const mapInput = (input) => ({
        name: input.getAttribute("name"),
        id: input.getAttribute("id"),
        type: input instanceof HTMLInputElement ? input.type : input.tagName.toLowerCase(),
        placeholder:
          input instanceof HTMLInputElement || input instanceof HTMLTextAreaElement
            ? input.placeholder
            : null,
        value: input instanceof HTMLInputElement || input instanceof HTMLTextAreaElement ? input.value : null,
        required: input.required,
        disabled: input.disabled
      });

      const inputs = Array.from(document.querySelectorAll("input, textarea, select"))
        .slice(0, maxElements)
        .map(mapInput);

      const forms = Array.from(document.querySelectorAll("form"))
        .slice(0, maxElements)
        .map((form) => ({
          action: form.action,
          method: form.method || "get",
          id: form.getAttribute("id"),
          name: form.getAttribute("name"),
          fields: Array.from(form.querySelectorAll("input, textarea, select"))
            .slice(0, maxElements)
            .map(mapInput)
        }));

      return { visibleText, links, buttons, inputs, forms };
    })()`)) as Pick<PageObservation, "visibleText" | "links" | "buttons" | "inputs" | "forms">;

    return {
      observedAt: new Date().toISOString(),
      url: this.page.url(),
      title: await this.page.title(),
      visibleText: domObservation.visibleText,
      links: domObservation.links,
      buttons: domObservation.buttons,
      inputs: domObservation.inputs,
      forms: domObservation.forms,
      console: this.consoleEvents.slice(-maxConsoleEvents),
      network: Array.from(this.networkEvents.values()).slice(-maxNetworkEvents)
    };
  }

  private recordConsole(message: ConsoleMessage): void {
    this.consoleEvents.push({
      type: message.type(),
      text: message.text(),
      location: message.location()
    });
  }

  private recordRequest(request: Request): void {
    this.networkEvents.set(requestKey(request), {
      url: request.url(),
      method: request.method(),
      resourceType: request.resourceType()
    });
  }

  private recordResponse(response: Response): void {
    const request = response.request();
    const existing = this.networkEvents.get(requestKey(request));
    this.networkEvents.set(requestKey(request), {
      url: request.url(),
      method: request.method(),
      resourceType: request.resourceType(),
      status: response.status(),
      ok: response.ok(),
      failureText: existing?.failureText
    });
  }

  private recordRequestFailure(request: Request): void {
    const existing = this.networkEvents.get(requestKey(request));
    this.networkEvents.set(requestKey(request), {
      url: request.url(),
      method: request.method(),
      resourceType: request.resourceType(),
      status: existing?.status,
      ok: false,
      failureText: request.failure()?.errorText ?? existing?.failureText ?? "request failed"
    });
  }
}

function requestKey(request: Request): string {
  return `${request.method()} ${request.url()}`;
}
