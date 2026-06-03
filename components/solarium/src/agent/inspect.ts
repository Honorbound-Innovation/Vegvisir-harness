import type { Page } from "playwright";
import { SolariumBrowser } from "../browser/engine.js";
import { attachScopedNetworkPolicy } from "../security/network-policy.js";
import { assertUrlInScope } from "../security/scope.js";
import type { InspectCandidate, InspectOptions, InspectResult, SelectorHint } from "../types.js";
import { ObservationRecorder } from "./observations.js";

const DEFAULT_MAX_CANDIDATES = 100;

interface RawCandidate {
  kind: InspectCandidate["kind"];
  label: string;
  selector: string;
  roleSelector?: string;
  textSelector?: string;
  selectorHints?: SelectorHint[];
  action: InspectCandidate["action"];
  href?: string;
  inputType?: string | null;
  required?: boolean;
  disabled?: boolean;
  form?: {
    selector: string;
    action: string;
    method: string;
  };
  confidence: InspectCandidate["confidence"];
  reason: string;
}

export async function inspectPage(options: InspectOptions): Promise<InspectResult> {
  assertUrlInScope(options.url, options.scope);

  const browser = await SolariumBrowser.launch(options);
  try {
    const page = await browser.newPage();
    const rawPage = page.raw();
    const recorder = new ObservationRecorder(rawPage);
    recorder.attach();
    const networkPolicy = await attachScopedNetworkPolicy(rawPage, {
      scope: options.scope,
      onBlockedRequest: (event) => recorder.recordNetworkEvent(event)
    });

    await page.navigate(options.url, {
      waitUntil: options.waitUntil,
      timeoutMs: options.timeoutMs
    });

    if (options.waitAfterNavigationMs) {
      await page.wait(options.waitAfterNavigationMs);
    }

    if (options.screenshotPath) {
      await page.screenshot({ path: options.screenshotPath, fullPage: true });
    }

    const observation = await recorder.observe(options.observationOptions);
    const candidates = await discoverCandidates(rawPage, options.maxCandidates ?? DEFAULT_MAX_CANDIDATES);

    return {
      url: options.url,
      finalUrl: page.url(),
      title: await page.title(),
      inspectedAt: new Date().toISOString(),
      screenshotPath: options.screenshotPath,
      candidates,
      observation: options.includeObservation ? observation : undefined,
      networkPolicy: networkPolicy.stats()
    };
  } finally {
    await browser.close();
  }
}

export async function discoverCandidates(page: Page, maxCandidates: number): Promise<InspectCandidate[]> {
  const rawCandidates = await page.evaluate(
    ({ maxCandidates }) => {
      const cleanText = (value: string | null | undefined): string =>
        (value ?? "").replace(/\s+/g, " ").trim();

      const cssEscape = (value: string): string => CSS.escape(value);
      const quoteAttr = (value: string): string => String(value).replace(/\\/g, "\\\\").replace(/"/g, "\\\"");
      const attrSelector = (name: string, value: string): string => `[${name}="${quoteAttr(value)}"]`;

      const labelTextFor = (element: Element): string => {
        if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement || element instanceof HTMLSelectElement) {
          if (element.labels?.length) {
            return cleanText(Array.from(element.labels).map((label) => label.innerText).join(" "));
          }
          const id = element.getAttribute("id");
          if (id) {
            const label = document.querySelector(`label[for="${quoteAttr(id)}"]`);
            if (label) return cleanText(label.textContent);
          }
        }
        const wrappingLabel = element.closest("label");
        return wrappingLabel ? cleanText(wrappingLabel.textContent) : "";
      };

      const implicitRoleFor = (element: Element): string | null => {
        const tag = element.tagName.toLowerCase();
        if (tag === "a" && element.hasAttribute("href")) return "link";
        if (tag === "button") return "button";
        if (tag === "select") return "combobox";
        if (tag === "textarea") return "textbox";
        if (tag === "form") return element.getAttribute("aria-label") || element.getAttribute("aria-labelledby") ? "form" : null;
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

      const accessibleNameFor = (element: Element): string | null => {
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

      const uniqueSelector = (element: Element): string => {
        const id = element.getAttribute("id");
        if (id) return `#${cssEscape(id)}`;

        const testId = element.getAttribute("data-testid") || element.getAttribute("data-test") || element.getAttribute("data-cy");
        if (testId) return `${attrSelector("data-testid", testId)}, ${attrSelector("data-test", testId)}, ${attrSelector("data-cy", testId)}`;

        const aria = element.getAttribute("aria-label");
        if (aria) return `${element.tagName.toLowerCase()}${attrSelector("aria-label", aria)}`;

        const name = element.getAttribute("name");
        if (name) return `${element.tagName.toLowerCase()}${attrSelector("name", name)}`;

        const parts: string[] = [];
        let current: Element | null = element;
        while (current && current.nodeType === Node.ELEMENT_NODE && current !== document.body) {
          let part = current.tagName.toLowerCase();
          const currentId = current.getAttribute("id");
          if (currentId) {
            part += `#${cssEscape(currentId)}`;
            parts.unshift(part);
            break;
          }

          const parentElement: Element | null = current.parentElement;
          if (parentElement) {
            const currentTag = current.tagName;
            const siblings = Array.from(parentElement.children).filter(
              (child): child is Element => child instanceof Element && child.tagName === currentTag
            );
            if (siblings.length > 1) {
              part += `:nth-of-type(${siblings.indexOf(current) + 1})`;
            }
          }
          parts.unshift(part);
          current = parentElement;
        }

        return parts.join(" > ");
      };

      const selectorHintsFor = (element: Element): SelectorHint[] => {
        const hints: SelectorHint[] = [];
        const tag = element.tagName.toLowerCase();
        const role = element.getAttribute("role") || implicitRoleFor(element);
        const accessibleName = accessibleNameFor(element);
        const testId = element.getAttribute("data-testid") || element.getAttribute("data-test") || element.getAttribute("data-cy");
        if (testId) {
          hints.push({
            kind: "test-id",
            strategy: "css",
            value: `${attrSelector("data-testid", testId)}, ${attrSelector("data-test", testId)}, ${attrSelector("data-cy", testId)}`,
            confidence: "high",
            reason: "Stable test id attribute."
          });
        }
        const id = element.getAttribute("id");
        if (id) {
          hints.push({ kind: "id", strategy: "css", value: `#${cssEscape(id)}`, confidence: "high", reason: "Element id attribute." });
        }
        if (role && accessibleName) {
          hints.push({
            kind: "role",
            strategy: "playwright",
            value: `getByRole(${JSON.stringify(role)}, { name: ${JSON.stringify(accessibleName)} })`,
            confidence: "high",
            reason: "Accessible role and name."
          });
        }
        const label = labelTextFor(element);
        if (label) {
          hints.push({
            kind: "label",
            strategy: "playwright",
            value: `getByLabel(${JSON.stringify(label)})`,
            confidence: "high",
            reason: "Associated label text."
          });
        }
        const placeholder = element.getAttribute("placeholder");
        if (placeholder) {
          hints.push({
            kind: "placeholder",
            strategy: "playwright",
            value: `getByPlaceholder(${JSON.stringify(placeholder)})`,
            confidence: "medium",
            reason: "Placeholder text."
          });
        }
        const name = element.getAttribute("name");
        if (name) {
          hints.push({ kind: "name", strategy: "css", value: `${tag}${attrSelector("name", name)}`, confidence: "medium", reason: "Name attribute." });
        }
        const text = cleanText(element.textContent);
        if (text && text.length <= 80 && ["a", "button"].includes(tag)) {
          hints.push({ kind: "text", strategy: "playwright", value: `getByText(${JSON.stringify(text)})`, confidence: "medium", reason: "Visible text." });
        }
        const aria = element.getAttribute("aria-label");
        if (aria) {
          hints.push({ kind: "aria", strategy: "css", value: `${tag}${attrSelector("aria-label", aria)}`, confidence: "medium", reason: "ARIA label attribute." });
        }
        hints.push({
          kind: "css",
          strategy: "css",
          value: uniqueSelector(element),
          confidence: hints.length > 0 ? "medium" : "low",
          reason: hints.length > 0 ? "CSS fallback selector." : "Generated DOM path fallback."
        });

        const seen = new Set<string>();
        return hints.filter((hint) => {
          const key = `${hint.strategy}:${hint.value}`;
          if (seen.has(key)) return false;
          seen.add(key);
          return true;
        }).slice(0, 6);
      };

      const cssSelectorFor = (element: Element): string => selectorHintsFor(element).find((hint) => hint.strategy === "css")?.value ?? uniqueSelector(element);

      const formSelectorFor = (element: Element) => {
        const form = element.closest("form");
        if (!form) return undefined;
        return {
          selector: cssSelectorFor(form),
          action: (form as HTMLFormElement).action,
          method: ((form as HTMLFormElement).method || "get").toLowerCase()
        };
      };

      const confidenceFor = (hints: SelectorHint[]): InspectCandidate["confidence"] => {
        if (hints.some((hint) => hint.confidence === "high")) return "high";
        if (hints.some((hint) => hint.confidence === "medium")) return "medium";
        return "low";
      };

      const candidates: RawCandidate[] = [];

      for (const anchor of Array.from(document.querySelectorAll<HTMLAnchorElement>("a[href]"))) {
        const label = cleanText(anchor.innerText || anchor.getAttribute("aria-label") || anchor.href);
        const selectorHints = selectorHintsFor(anchor);
        candidates.push({
          kind: "link",
          label,
          selector: cssSelectorFor(anchor),
          textSelector: label ? `text=${label}` : undefined,
          selectorHints,
          action: "navigate",
          href: anchor.href,
          disabled: false,
          confidence: confidenceFor(selectorHints),
          reason: "Anchor with href can be followed or clicked for navigation."
        });
      }

      for (const button of Array.from(
        document.querySelectorAll<HTMLButtonElement | HTMLInputElement>(
          "button, input[type='button'], input[type='submit'], input[type='reset']"
        )
      )) {
        const label = cleanText(
          button instanceof HTMLInputElement
            ? button.value || button.getAttribute("aria-label") || button.name
            : button.innerText || button.getAttribute("aria-label")
        );
        const selectorHints = selectorHintsFor(button);
        candidates.push({
          kind: "button",
          label: label || button.tagName.toLowerCase(),
          selector: cssSelectorFor(button),
          roleSelector: selectorHints.find((hint) => hint.kind === "role")?.value,
          textSelector: label ? `text=${label}` : undefined,
          selectorHints,
          action: "click",
          inputType: button.getAttribute("type"),
          required: false,
          disabled: button.disabled,
          form: formSelectorFor(button),
          confidence: confidenceFor(selectorHints),
          reason: "Button-like control can be clicked."
        });
      }

      for (const input of Array.from(
        document.querySelectorAll<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>("input, textarea, select")
      )) {
        const type = input instanceof HTMLInputElement ? input.type : input.tagName.toLowerCase();
        const label = cleanText(
          labelTextFor(input) ||
            input.getAttribute("aria-label") ||
            input.getAttribute("placeholder") ||
            input.getAttribute("name") ||
            input.getAttribute("id") ||
            type
        );
        const selectorHints = selectorHintsFor(input);
        candidates.push({
          kind: "input",
          label,
          selector: cssSelectorFor(input),
          roleSelector: selectorHints.find((hint) => hint.kind === "label" || hint.kind === "role")?.value,
          selectorHints,
          action: "fill",
          inputType: type,
          required: input.required,
          disabled: input.disabled,
          form: formSelectorFor(input),
          confidence: confidenceFor(selectorHints),
          reason: "Input-like field can be filled or selected."
        });
      }

      for (const form of Array.from(document.querySelectorAll<HTMLFormElement>("form"))) {
        const label = cleanText(form.getAttribute("aria-label") || form.getAttribute("name") || form.getAttribute("id") || form.action || "form");
        const selectorHints = selectorHintsFor(form);
        candidates.push({
          kind: "form",
          label,
          selector: cssSelectorFor(form),
          selectorHints,
          action: "submit",
          href: form.action,
          confidence: confidenceFor(selectorHints),
          reason: "Form container can be inspected and submitted after explicit agent/user intent."
        });
      }

      return candidates.slice(0, maxCandidates);
    },
    { maxCandidates }
  );

  return dedupeCandidates(rawCandidates);
}

function dedupeCandidates(candidates: RawCandidate[]): InspectCandidate[] {
  const seen = new Set<string>();
  const deduped: InspectCandidate[] = [];
  for (const candidate of candidates) {
    const key = `${candidate.kind}:${candidate.action}:${candidate.selector}:${candidate.href ?? ""}`;
    if (seen.has(key)) continue;
    seen.add(key);
    deduped.push(candidate);
  }
  return deduped;
}
