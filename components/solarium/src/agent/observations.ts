import type { ConsoleMessage, Page, Request, Response } from "playwright";
import type {
  ConsoleLogObservation,
  NetworkObservation,
  ObservationOptions,
  PageFormObservation,
  PageInputObservation,
  PageObservation
} from "../types.js";

const DEFAULT_MAX_TEXT_CHARS = 20_000;
const DEFAULT_MAX_ELEMENTS = 100;
const DEFAULT_MAX_CONSOLE_EVENTS = 100;
const DEFAULT_MAX_NETWORK_EVENTS = 200;
const REDACTED_VALUE = "[redacted]";

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
    const redactInputValues = options.redactInputValues ?? true;
    const sensitiveSelectors = options.sensitiveSelectors ?? [];

    // Use a string expression instead of passing a function object here.
    // tsx/esbuild can wrap function literals with helper symbols such as
    // `__name`; those helpers do not exist in the browser page context and
    // cause Playwright page.evaluate failures. Keep this browser-side code
    // self-contained JavaScript.
    const domObservation = (await this.page.evaluate(`(() => {
      const maxTextChars = ${JSON.stringify(maxTextChars)};
      const maxElements = ${JSON.stringify(maxElements)};
      const redactInputValues = ${JSON.stringify(redactInputValues)};
      const sensitiveSelectors = ${JSON.stringify(sensitiveSelectors)};
      const redactedValue = ${JSON.stringify(REDACTED_VALUE)};
      const sensitiveNamePattern = /(password|passwd|token|secret|credential|api[_-]?key|auth|session|csrf|xsrf|otp|mfa)/i;
      const sensitiveAutocompleteValues = new Set(["current-password", "new-password", "one-time-code"]);
      const cleanText = (value) => (value ?? "").replace(/\s+/g, " ").trim();
      const cssEscape = (value) => CSS.escape(String(value));
      const quoteAttr = (value) => String(value).replace(/\\/g, "\\\\").replace(/"/g, "\\\"");
      const attrSelector = (name, value) => "[" + name + "=\"" + quoteAttr(value) + "\"]";
      const attrText = (input) => [
        input.getAttribute("name"),
        input.getAttribute("id"),
        input.getAttribute("autocomplete"),
        input.getAttribute("aria-label"),
        input.getAttribute("placeholder")
      ].filter(Boolean).join(" ");
      const matchesAnySensitiveSelector = (input) => sensitiveSelectors.some((selector) => {
        try {
          return input.matches(selector);
        } catch {
          return false;
        }
      });
      const shouldRedactInput = (input) => {
        if (!redactInputValues) return false;
        if (matchesAnySensitiveSelector(input)) return true;
        if (input instanceof HTMLInputElement) {
          const type = input.type.toLowerCase();
          if (["password", "hidden"].includes(type)) return true;
          if (sensitiveAutocompleteValues.has((input.autocomplete || "").toLowerCase())) return true;
        }
        return sensitiveNamePattern.test(attrText(input));
      };
      const labelTextFor = (element) => {
        if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement || element instanceof HTMLSelectElement) {
          if (element.labels?.length) {
            return cleanText(Array.from(element.labels).map((label) => label.innerText).join(" "));
          }
          const id = element.getAttribute("id");
          if (id) {
            const label = document.querySelector("label[for=\"" + quoteAttr(id) + "\"]");
            if (label) return cleanText(label.textContent);
          }
        }
        const wrappingLabel = element.closest("label");
        return wrappingLabel ? cleanText(wrappingLabel.textContent) : "";
      };
      const implicitRoleFor = (element) => {
        const tag = element.tagName.toLowerCase();
        if (tag === "a" && element.hasAttribute("href")) return "link";
        if (tag === "button") return "button";
        if (tag === "select") return "combobox";
        if (tag === "textarea") return "textbox";
        if (tag === "form") return element.getAttribute("aria-label") || element.getAttribute("aria-labelledby") ? "form" : null;
        if (tag === "main") return "main";
        if (tag === "nav") return "navigation";
        if (tag === "header") return "banner";
        if (tag === "footer") return "contentinfo";
        if (tag === "aside") return "complementary";
        if (tag === "section") return element.getAttribute("aria-label") || element.getAttribute("aria-labelledby") ? "region" : null;
        if (tag === "dialog") return "dialog";
        if (tag === "ul" || tag === "ol") return "list";
        if (tag === "table") return "table";
        if (tag === "input") {
          const type = (element.getAttribute("type") || "text").toLowerCase();
          if (["button", "submit", "reset"].includes(type)) return "button";
          if (type === "checkbox") return "checkbox";
          if (type === "radio") return "radio";
          if (type === "range") return "slider";
          if (type === "search") return "searchbox";
          if (["email", "password", "tel", "text", "url", "number"].includes(type)) return "textbox";
        }
        return null;
      };
      const accessibleNameFor = (element) => {
        const labelledBy = element.getAttribute("aria-labelledby");
        if (labelledBy) {
          const text = cleanText(labelledBy.split(/\s+/).map((id) => document.getElementById(id)?.textContent || "").join(" "));
          if (text) return text;
        }
        const aria = cleanText(element.getAttribute("aria-label"));
        if (aria) return aria;
        const label = labelTextFor(element);
        if (label) return label;
        if (element instanceof HTMLInputElement) {
          const value = cleanText(element.value);
          if (["button", "submit", "reset"].includes(element.type) && value) return value;
        }
        const alt = cleanText(element.getAttribute("alt"));
        if (alt) return alt;
        const placeholder = cleanText(element.getAttribute("placeholder"));
        if (placeholder) return placeholder;
        const title = cleanText(element.getAttribute("title"));
        if (title) return title;
        return cleanText(element.textContent).slice(0, 120) || null;
      };
      const boundingBoxFor = (element) => {
        const rect = element.getBoundingClientRect();
        if (!Number.isFinite(rect.width) || !Number.isFinite(rect.height)) return null;
        return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
      };
      const visibleFor = (element) => {
        const rect = element.getBoundingClientRect();
        const style = window.getComputedStyle(element);
        return rect.width > 0 && rect.height > 0 && style.visibility !== "hidden" && style.display !== "none" && Number(style.opacity || "1") > 0;
      };
      const uniqueSelector = (element) => {
        const id = element.getAttribute("id");
        if (id) return "#" + cssEscape(id);

        const testId = element.getAttribute("data-testid") || element.getAttribute("data-test") || element.getAttribute("data-cy");
        if (testId) return attrSelector("data-testid", testId) + ", " + attrSelector("data-test", testId) + ", " + attrSelector("data-cy", testId);

        const aria = element.getAttribute("aria-label");
        if (aria) return element.tagName.toLowerCase() + attrSelector("aria-label", aria);

        const name = element.getAttribute("name");
        if (name) return element.tagName.toLowerCase() + attrSelector("name", name);

        const parts = [];
        let current = element;
        while (current && current.nodeType === Node.ELEMENT_NODE && current !== document.body) {
          let part = current.tagName.toLowerCase();
          const currentId = current.getAttribute("id");
          if (currentId) {
            part += "#" + cssEscape(currentId);
            parts.unshift(part);
            break;
          }

          const parentElement = current.parentElement;
          if (parentElement) {
            const currentTag = current.tagName;
            const siblings = Array.from(parentElement.children).filter((child) => child instanceof Element && child.tagName === currentTag);
            if (siblings.length > 1) {
              part += ":nth-of-type(" + (siblings.indexOf(current) + 1) + ")";
            }
          }
          parts.unshift(part);
          current = parentElement;
        }

        return parts.join(" > ");
      };
      const selectorHintsFor = (element) => {
        const hints = [];
        const tag = element.tagName.toLowerCase();
        const role = element.getAttribute("role") || implicitRoleFor(element);
        const accessibleName = accessibleNameFor(element);
        const testId = element.getAttribute("data-testid") || element.getAttribute("data-test") || element.getAttribute("data-cy");
        if (testId) hints.push({ kind: "test-id", strategy: "css", value: attrSelector("data-testid", testId) + ", " + attrSelector("data-test", testId) + ", " + attrSelector("data-cy", testId), confidence: "high", reason: "Stable test id attribute." });
        const id = element.getAttribute("id");
        if (id) hints.push({ kind: "id", strategy: "css", value: "#" + cssEscape(id), confidence: "high", reason: "Element id attribute." });
        if (role && accessibleName) hints.push({ kind: "role", strategy: "playwright", value: "getByRole(" + JSON.stringify(role) + ", { name: " + JSON.stringify(accessibleName) + " })", confidence: "high", reason: "Accessible role and name." });
        const label = labelTextFor(element);
        if (label) hints.push({ kind: "label", strategy: "playwright", value: "getByLabel(" + JSON.stringify(label) + ")", confidence: "high", reason: "Associated label text." });
        const placeholder = element.getAttribute("placeholder");
        if (placeholder) hints.push({ kind: "placeholder", strategy: "playwright", value: "getByPlaceholder(" + JSON.stringify(placeholder) + ")", confidence: "medium", reason: "Placeholder text." });
        const name = element.getAttribute("name");
        if (name) hints.push({ kind: "name", strategy: "css", value: tag + attrSelector("name", name), confidence: "medium", reason: "Name attribute." });
        const text = cleanText(element.textContent);
        if (text && text.length <= 80 && ["a", "button"].includes(tag)) hints.push({ kind: "text", strategy: "playwright", value: "getByText(" + JSON.stringify(text) + ")", confidence: "medium", reason: "Visible text." });
        const aria = element.getAttribute("aria-label");
        if (aria) hints.push({ kind: "aria", strategy: "css", value: tag + attrSelector("aria-label", aria), confidence: "medium", reason: "ARIA label attribute." });
        hints.push({ kind: "css", strategy: "css", value: uniqueSelector(element), confidence: hints.length > 0 ? "medium" : "low", reason: hints.length > 0 ? "CSS fallback selector." : "Generated DOM path fallback." });
        const seen = new Set();
        return hints.filter((hint) => {
          const key = hint.strategy + ":" + hint.value;
          if (seen.has(key)) return false;
          seen.add(key);
          return true;
        }).slice(0, 6);
      };
      const metadataFor = (element) => {
        const selectorHints = selectorHintsFor(element);
        const cssSelectorHint = selectorHints.find((hint) => hint.strategy === "css")?.value;
        return {
          role: element.getAttribute("role") || implicitRoleFor(element),
          accessibleName: accessibleNameFor(element),
          selectorHint: cssSelectorHint,
          selectorHints,
          visible: visibleFor(element),
          boundingBox: boundingBoxFor(element)
        };
      };

      const visibleText = cleanText(document.body?.innerText ?? "").slice(0, maxTextChars);

      const links = Array.from(document.querySelectorAll("a[href]"))
        .slice(0, maxElements)
        .map((anchor) => ({
          ...metadataFor(anchor),
          text: cleanText(anchor.innerText || anchor.getAttribute("aria-label") || anchor.href),
          href: anchor.href,
          target: anchor.getAttribute("target"),
          rel: anchor.getAttribute("rel")
        }));

      const buttons = Array.from(
        document.querySelectorAll("button, input[type='button'], input[type='submit'], input[type='reset']")
      )
        .slice(0, maxElements)
        .map((button) => ({
          ...metadataFor(button),
          text: cleanText(
            button instanceof HTMLInputElement
              ? button.value || button.getAttribute("aria-label") || button.name
              : button.innerText || button.getAttribute("aria-label")
          ),
          type: button.getAttribute("type"),
          disabled: button.disabled
        }));

      const mapInput = (input) => {
        const canHaveValue = input instanceof HTMLInputElement || input instanceof HTMLTextAreaElement;
        const valueRedacted = canHaveValue && shouldRedactInput(input);
        return {
          ...metadataFor(input),
          name: input.getAttribute("name"),
          id: input.getAttribute("id"),
          type: input instanceof HTMLInputElement ? input.type : input.tagName.toLowerCase(),
          placeholder: canHaveValue ? input.placeholder : null,
          label: labelTextFor(input) || null,
          value: canHaveValue ? (valueRedacted ? redactedValue : input.value) : null,
          valueRedacted,
          required: input.required,
          disabled: input.disabled
        };
      };

      const inputs = Array.from(document.querySelectorAll("input, textarea, select"))
        .slice(0, maxElements)
        .map(mapInput);

      const forms = Array.from(document.querySelectorAll("form"))
        .slice(0, maxElements)
        .map((form) => ({
          ...metadataFor(form),
          action: form.action,
          method: form.method || "get",
          id: form.getAttribute("id"),
          name: form.getAttribute("name"),
          fields: Array.from(form.querySelectorAll("input, textarea, select"))
            .slice(0, maxElements)
            .map(mapInput)
        }));

      const landmarks = Array.from(document.querySelectorAll("main, nav, header, footer, aside, section[aria-label], section[aria-labelledby], [role='main'], [role='navigation'], [role='banner'], [role='contentinfo'], [role='complementary'], [role='region'], dialog, [role='dialog'], ul, ol"))
        .slice(0, maxElements)
        .map((element) => {
          const tag = element.tagName.toLowerCase();
          const role = element.getAttribute("role") || implicitRoleFor(element);
          return {
            ...metadataFor(element),
            kind: role === "dialog" || tag === "dialog" ? "dialog" : role === "list" || tag === "ul" || tag === "ol" ? "list" : "landmark",
            tagName: tag,
            text: cleanText(element.innerText || element.textContent).slice(0, 500)
          };
        });

      const tables = Array.from(document.querySelectorAll("table"))
        .slice(0, maxElements)
        .map((table) => ({
          ...metadataFor(table),
          caption: cleanText(table.querySelector("caption")?.textContent) || null,
          headers: Array.from(table.querySelectorAll("th")).slice(0, 50).map((cell) => cleanText(cell.textContent)),
          rowCount: table.querySelectorAll("tr").length,
          sampleRows: Array.from(table.querySelectorAll("tr")).slice(0, 5).map((row) =>
            Array.from(row.querySelectorAll("th,td")).slice(0, 10).map((cell) => cleanText(cell.textContent))
          )
        }));

      const frames = Array.from(document.querySelectorAll("iframe, frame"))
        .slice(0, maxElements)
        .map((frame) => ({
          ...metadataFor(frame),
          src: frame.getAttribute("src"),
          title: frame.getAttribute("title"),
          name: frame.getAttribute("name")
        }));

      return { visibleText, links, buttons, inputs, forms, landmarks, tables, frames };
    })()`)) as Pick<PageObservation, "visibleText" | "links" | "buttons" | "inputs" | "forms" | "landmarks" | "tables" | "frames">;

    return {
      observedAt: new Date().toISOString(),
      url: this.page.url(),
      title: await this.page.title(),
      visibleText: domObservation.visibleText,
      links: domObservation.links,
      buttons: domObservation.buttons,
      inputs: redactInputValues ? redactSensitiveObservedInputs(domObservation.inputs) : domObservation.inputs,
      forms: redactInputValues ? redactSensitiveObservedFormInputs(domObservation.forms) : domObservation.forms,
      landmarks: domObservation.landmarks,
      tables: domObservation.tables,
      frames: domObservation.frames,
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

export function redactSensitiveObservedInputs(inputs: PageInputObservation[]): PageInputObservation[] {
  return inputs.map((input) => {
    if (!isSensitiveObservedInput(input)) return input;
    return { ...input, value: REDACTED_VALUE, valueRedacted: true };
  });
}

export function redactSensitiveObservedFormInputs(forms: PageFormObservation[]): PageFormObservation[] {
  return forms.map((form) => ({
    ...form,
    fields: redactSensitiveObservedInputs(form.fields)
  }));
}

function isSensitiveObservedInput(input: PageInputObservation): boolean {
  if (input.valueRedacted) return true;
  const type = input.type?.toLowerCase();
  if (type === "password" || type === "hidden") return true;
  const combined = [input.name, input.id, input.placeholder, input.label, input.accessibleName].filter(Boolean).join(" ");
  return /(password|passwd|token|secret|credential|api[_-]?key|auth|session|csrf|xsrf|otp|mfa)/i.test(combined);
}

function requestKey(request: Request): string {
  return `${request.method()} ${request.url()}`;
}
