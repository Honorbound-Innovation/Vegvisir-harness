import test from "node:test";
import assert from "node:assert/strict";

import { SolariumBrowser, ObservationRecorder, inspectPage } from "../dist/index.js";

async function withChromiumPage(html, callback) {
  let browser;
  try {
    browser = await SolariumBrowser.launch({ engine: "chromium", headless: true });
  } catch (error) {
    if (String(error?.message ?? error).includes("Executable doesn't exist")) {
      test.skip("Playwright Chromium browser is not installed");
      return;
    }
    throw error;
  }

  try {
    const page = await browser.newPage();
    await page.navigate(`data:text/html,${encodeURIComponent(html)}`);
    return await callback(page.raw());
  } finally {
    await browser.close();
  }
}

test("ObservationRecorder captures semantic metadata and redacts sensitive values", async () => {
  await withChromiumPage(
    `<!doctype html>
    <html>
      <body>
        <header>Site header</header>
        <main aria-label="Main content">
          <form id="login-form" aria-label="Login form">
            <label for="email">Email address</label>
            <input id="email" name="email" type="email" value="malice@example.test" placeholder="Email">
            <label for="password">Password</label>
            <input id="password" name="password" type="password" value="super-secret">
            <button data-testid="submit-login" type="submit">Sign in</button>
          </form>
          <table><caption>Users</caption><tr><th>Name</th></tr><tr><td>Malice</td></tr></table>
          <iframe title="Preview frame" src="about:blank"></iframe>
        </main>
      </body>
    </html>`,
    async (page) => {
      const recorder = new ObservationRecorder(page);
      const observation = await recorder.observe();

      assert.equal(observation.inputs.find((input) => input.id === "email")?.label, "Email address");
      const password = observation.inputs.find((input) => input.id === "password");
      assert.equal(password?.value, "[redacted]");
      assert.equal(password?.valueRedacted, true);

      const button = observation.buttons.find((candidate) => candidate.text === "Sign in");
      assert.equal(button?.role, "button");
      assert.equal(button?.accessibleName, "Sign in");
      assert.equal(button?.selectorHint, '[data-testid="submit-login"], [data-test="submit-login"], [data-cy="submit-login"]');
      assert.equal(button?.selectorHints?.[0]?.kind, "test-id");
      assert.equal(button?.selectorHints?.[0]?.confidence, "high");
      assert.ok(button?.boundingBox?.width > 0);

      assert.ok(observation.landmarks.some((landmark) => landmark.role === "main" || landmark.role === "banner"));
      assert.equal(observation.tables[0]?.caption, "Users");
      assert.deepEqual(observation.tables[0]?.headers, ["Name"]);
      assert.equal(observation.frames[0]?.title, "Preview frame");
    }
  );
});

test("inspectPage returns ranked selector hints while preserving CSS selector fallback", async () => {
  const html = `<!doctype html>
  <html>
    <body>
      <main>
        <form id="search-form" aria-label="Search form">
          <label for="query">Search query</label>
          <input id="query" name="q" placeholder="Search docs">
          <button data-testid="run-search" type="submit">Search</button>
        </form>
        <a id="docs-link" href="https://example.test/docs">Docs</a>
      </main>
    </body>
  </html>`;

  try {
    const result = await inspectPage({
      url: `data:text/html,${encodeURIComponent(html)}`,
      engine: "chromium",
      headless: true,
      includeObservation: true,
      maxCandidates: 10
    });

    const button = result.candidates.find((candidate) => candidate.kind === "button" && candidate.label === "Search");
    assert.equal(button?.selector, '[data-testid="run-search"], [data-test="run-search"], [data-cy="run-search"]');
    assert.equal(button?.selectorHints?.[0]?.kind, "test-id");
    assert.equal(button?.selectorHints?.[0]?.confidence, "high");
    assert.ok(button?.selectorHints?.some((hint) => hint.kind === "role" && hint.strategy === "playwright"));

    const input = result.candidates.find((candidate) => candidate.kind === "input" && candidate.label === "Search query");
    assert.equal(input?.selector, "#query");
    assert.ok(input?.selectorHints?.some((hint) => hint.kind === "label" && hint.confidence === "high"));
    assert.ok(input?.selectorHints?.some((hint) => hint.kind === "placeholder"));

    const link = result.candidates.find((candidate) => candidate.kind === "link" && candidate.label === "Docs");
    assert.equal(link?.selector, "#docs-link");
    assert.ok(link?.selectorHints?.some((hint) => hint.kind === "role"));
    assert.ok(result.observation?.landmarks.length);
  } catch (error) {
    if (String(error?.message ?? error).includes("Executable doesn't exist")) {
      test.skip("Playwright Chromium browser is not installed");
      return;
    }
    throw error;
  }
});
