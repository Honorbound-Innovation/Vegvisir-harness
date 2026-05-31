import type { Page } from "playwright";
import { SolariumBrowser } from "../browser/engine.js";
import { attachScopedNetworkPolicy } from "../security/network-policy.js";
import { assertUrlInScope } from "../security/scope.js";
import type { InspectCandidate, InspectOptions, InspectResult } from "../types.js";
import { ObservationRecorder } from "./observations.js";

const DEFAULT_MAX_CANDIDATES = 100;

interface RawCandidate {
  kind: InspectCandidate["kind"];
  label: string;
  selector: string;
  roleSelector?: string;
  textSelector?: string;
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

      const uniqueSelector = (element: Element): string => {
        const id = element.getAttribute("id");
        if (id) return `#${cssEscape(id)}`;

        const testId = element.getAttribute("data-testid") || element.getAttribute("data-test") || element.getAttribute("data-cy");
        if (testId) return `[data-testid=\"${cssEscape(testId)}\"], [data-test=\"${cssEscape(testId)}\"], [data-cy=\"${cssEscape(testId)}\"]`;

        const aria = element.getAttribute("aria-label");
        if (aria) return `${element.tagName.toLowerCase()}[aria-label=\"${cssEscape(aria)}\"]`;

        const name = element.getAttribute("name");
        if (name) return `${element.tagName.toLowerCase()}[name=\"${cssEscape(name)}\"]`;

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

      const formSelectorFor = (element: Element) => {
        const form = element.closest("form");
        if (!form) return undefined;
        return {
          selector: uniqueSelector(form),
          action: (form as HTMLFormElement).action,
          method: ((form as HTMLFormElement).method || "get").toLowerCase()
        };
      };

      const candidates: RawCandidate[] = [];

      for (const anchor of Array.from(document.querySelectorAll<HTMLAnchorElement>("a[href]"))) {
        const label = cleanText(anchor.innerText || anchor.getAttribute("aria-label") || anchor.href);
        candidates.push({
          kind: "link",
          label,
          selector: uniqueSelector(anchor),
          textSelector: label ? `text=${label}` : undefined,
          action: "navigate",
          href: anchor.href,
          disabled: false,
          confidence: anchor.getAttribute("id") || anchor.getAttribute("data-testid") ? "high" : "medium",
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
        candidates.push({
          kind: "button",
          label: label || button.tagName.toLowerCase(),
          selector: uniqueSelector(button),
          roleSelector: label ? `getByRole('button', { name: ${JSON.stringify(label)} })` : undefined,
          textSelector: label ? `text=${label}` : undefined,
          action: "click",
          inputType: button.getAttribute("type"),
          required: false,
          disabled: button.disabled,
          form: formSelectorFor(button),
          confidence: button.id || button.getAttribute("data-testid") || label ? "high" : "medium",
          reason: "Button-like control can be clicked."
        });
      }

      for (const input of Array.from(
        document.querySelectorAll<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>("input, textarea, select")
      )) {
        const type = input instanceof HTMLInputElement ? input.type : input.tagName.toLowerCase();
        const label = cleanText(
          input.getAttribute("aria-label") ||
            input.getAttribute("placeholder") ||
            input.getAttribute("name") ||
            input.getAttribute("id") ||
            type
        );
        candidates.push({
          kind: "input",
          label,
          selector: uniqueSelector(input),
          roleSelector: label ? `getByLabel(${JSON.stringify(label)})` : undefined,
          action: "fill",
          inputType: type,
          required: input.required,
          disabled: input.disabled,
          form: formSelectorFor(input),
          confidence: input.id || input.getAttribute("name") || input.getAttribute("aria-label") ? "high" : "medium",
          reason: "Input-like field can be filled or selected."
        });
      }

      for (const form of Array.from(document.querySelectorAll<HTMLFormElement>("form"))) {
        const label = cleanText(form.getAttribute("aria-label") || form.getAttribute("name") || form.getAttribute("id") || form.action || "form");
        candidates.push({
          kind: "form",
          label,
          selector: uniqueSelector(form),
          action: "submit",
          href: form.action,
          confidence: form.id || form.getAttribute("name") ? "high" : "medium",
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
