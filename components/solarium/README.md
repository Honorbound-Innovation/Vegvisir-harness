# Solarium

Solarium is a headless, agent-controlled web browser runtime intended for legitimate automation, research, and authorized web security testing.

## Vision

Solarium should let an AI agent safely and reliably:

- Browse the public web and interact with modern websites.
- Use search engines and gather research with traceable evidence.
- Fill forms, click controls, upload/download files where authorized, and inspect page state.
- Capture screenshots, DOM snapshots, network logs, console logs, storage state, and page artifacts.
- Run authorized web bug-hunting workflows against owned or permitted targets.
- Emulate common browser identities for compatibility testing and controlled security research.

## Security and acceptable-use boundary

Solarium is for defensive research, QA, automation, and authorized security testing. Browser identity/profile emulation is intended for compatibility, reproducibility, and anti-fingerprinting research in controlled/authorized contexts. The project should not implement credential theft, persistence, stealth malware behavior, unauthorized bypass of third-party protections, or workflows designed for abuse.

## Quick start

Install dependencies:

```bash
npm install
npm run install:browsers
```

Build:

```bash
npm run build
```

Browse a URL and capture a screenshot:

```bash
npm run dev -- browse https://example.com --screenshot .solarium/example.png --extract-text
```

Capture an agent-readable page observation:

```bash
npm run dev -- browse https://example.com \
  --observe \
  --observation .solarium/example.observation.json \
  --screenshot .solarium/example.png
```

Or after building:

```bash
node dist/cli/index.js browse https://example.com --screenshot .solarium/example.png --extract-text
```

## Browser profiles

Solarium ships with built-in browser emulation profiles and can load custom profile JSON files for reproducible compatibility testing and authorized research.

List built-in profiles:

```bash
npm run dev -- profiles
npm run dev -- profiles --json
```

Show a built-in profile:

```bash
npm run dev -- profiles:show chrome-stable
```

Validate and use a custom profile file:

```bash
npm run dev -- profiles:validate .solarium/profiles/research-desktop.json

npm run dev -- browse https://example.com \
  --profile-file .solarium/profiles/research-desktop.json \
  --observe
```

Profile files use the same shape as the `BrowserProfile` API:

```json
{
  "name": "research-desktop",
  "userAgent": "Mozilla/5.0 ... Chrome/131.0.0.0 Safari/537.36",
  "viewport": { "width": 1440, "height": 900 },
  "locale": "en-US",
  "timezoneId": "UTC",
  "colorScheme": "light",
  "deviceScaleFactor": 1,
  "isMobile": false,
  "hasTouch": false,
  "extraHTTPHeaders": {
    "Accept-Language": "en-US,en;q=0.9"
  }
}
```

Commands that launch a browser support `--profile-file`, including `browse`, `inspect`, `session`, `crawl`, `audit`, and `loop`. Config jobs can use `profilePath` to load a profile relative to the job file.

A schema is available for tooling:

```text
schemas/browser-profile.schema.json
```



## Uploads and downloads

Session actions now support authorized file upload/download workflows. Downloads are accepted in the browser context and can be saved either to an explicit action path or to the session evidence directory.

```json
[
  { "type": "navigate", "url": "https://example.com/upload" },
  { "type": "upload", "selector": "input[type=file]", "files": ["./fixtures/sample.txt"] },
  { "type": "click", "selector": "button[type=submit]" },
  { "type": "download", "selector": "a.export", "path": ".solarium/downloads/export.csv" }
]
```

Use `--downloads-dir` to control the browser-level downloads directory:

```bash
npm run dev -- session \
  --actions .solarium/actions.json \
  --scope .solarium/scope.json \
  --downloads-dir .solarium/downloads
```

`download` step results include the suggested filename, saved path, source URL, and any Playwright-reported failure. Treat downloaded files as untrusted artifacts unless they come from a fully trusted source.

## Browser storage state

Browser-launching commands support Playwright storage state files for legitimate authenticated QA/research workflows and reproducible multi-run browsing:

```bash
npm run dev -- session \
  --actions .solarium/actions.json \
  --scope .solarium/scope.json \
  --storage-state .solarium/state-before.json \
  --save-storage-state .solarium/state-after.json
```

Supported commands:

- `browse`
- `inspect`
- `session`
- `crawl`
- `audit`
- `graphql-audit`
- `owasp-audit`
- `loop`

Use `--storage-state <path>` to load cookies/localStorage/sessionStorage into the browser context, and `--save-storage-state <path>` to write the final context state when the browser closes. Storage state files can contain sensitive authenticated session material, so keep them out of source control and treat them as local artifacts.

Config jobs can use the same paths relative to the job file:

```json
{
  "mode": "loop",
  "url": "https://example.com",
  "scopePath": "scope.json",
  "storageState": "state-before.json",
  "saveStorageState": "state-after.json",
  "options": {
    "goal": "find account settings",
    "maxIterations": 3
  }
}
```



### Auth session profiles

Solarium supports auth-session profile files that reference Playwright storage-state JSON without embedding credentials in actions, logs, or workflow seeds.

Create a profile:

```bash
npm run dev -- auth-session   --create .solarium/auth/staging-admin.auth-session.json   --name staging-admin   --storage-state .solarium/auth/staging-admin.state.json   --description "Staging admin browser state"   --secret-ref hbse://project/staging-admin
```

Use it with browser commands that accept storage state:

```bash
npm run dev -- browse https://staging.example.com   --auth-session .solarium/auth/staging-admin.auth-session.json   --scope .solarium/scope.json   --observe
```

Auth-session profiles use `schemaVersion: "solarium.auth-session.v1"`. They may include secret reference identifiers, but must never contain plaintext passwords, tokens, cookies copied into chat, or secret-bearing URLs.

## Scope policies

Solarium supports JSON scope policy files for authorized testing boundaries.

Example `.solarium/scope.json`:

```json
{
  "allowedHosts": ["example.com", "*.example.com"],
  "blockedHosts": ["accounts.example.com"],
  "maxRequestsPerMinute": 60,
  "authorizationNote": "User owns or is authorized to test this environment."
}
```

Check a URL against a policy:

```bash
npm run dev -- scope-check https://example.com --scope .solarium/scope.json
```

Use a policy with a single browse run:

```bash
npm run dev -- browse https://example.com \
  --scope .solarium/scope.json \
  --observe
```

Use a policy with a multi-step session:

```bash
npm run dev -- session \
  --actions .solarium/actions.json \
  --scope .solarium/scope.json \
  --output .solarium/session-result.json \
  --report .solarium/session-report.md \
  --html-report .solarium/session-report.html
```

Scope enforcement now includes:

- pre-navigation URL checks
- post-action URL checks after navigation-producing actions
- crawler allowlist requirements
- browser-network request interception for HTTP/HTTPS requests when a scope is configured
- network policy summaries showing allowed, blocked, and rate-limited request counts

Non-HTTP browser-internal resources such as `data:`, `blob:`, and `about:` are allowed so pages can render normally.


## Agent page inspection

Use `inspect` when an agent needs actionable interaction candidates before deciding the next step. It opens a page, captures an observation, and returns clickable/fillable/navigation candidates with Playwright-friendly selector hints.

```bash
npm run dev -- inspect https://example.com \
  --scope .solarium/scope.json \
  --screenshot .solarium/inspect.png \
  --output .solarium/inspect-result.json
```

Useful options:

```bash
--include-observation
--max-candidates 100
--max-elements 100
--max-text-chars 20000
--wait-after-navigation-ms 1000
```

Candidate records include:

- `kind` — `link`, `button`, `input`, or `form`
- `action` — suggested action such as `navigate`, `click`, `fill`, or `submit`
- `selector` — CSS selector usable in session actions
- optional role/text selector hints
- `href`, input metadata, form metadata, confidence, and reason

This command is designed for agent planning; it does not submit forms or perform active testing.


### Generate session actions from inspection

`inspect` results can be converted into runnable session action files. This gives agents a simple inspect → plan → session loop.

Generate suggested actions directly while inspecting:

```bash
npm run dev -- inspect https://example.com \
  --scope .solarium/scope.json \
  --output .solarium/inspect-result.json \
  --actions-out .solarium/suggested-actions.json \
  --goal "find documentation"
```

Or generate actions from a saved inspect result:

```bash
npm run dev -- plan .solarium/inspect-result.json \
  --goal "search for project documentation" \
  --prefer-kind input,button,link \
  --output .solarium/suggested-actions.json \
  --plan-output .solarium/action-plan.json
```

Then run the generated action file:

```bash
npm run dev -- session \
  --actions .solarium/suggested-actions.json \
  --scope .solarium/scope.json \
  --events .solarium/planned-session.events.jsonl
```

Planning is intentionally conservative and reviewable: fill actions use a placeholder unless `--fill-value` is supplied, and generated actions are plain JSON that can be edited before execution.

## Multi-step agent sessions

Solarium can execute a JSON action list as a persistent browser session. This keeps one browser/page alive across multiple actions and records step-level results.

Create an action file:

```json
[
  { "type": "navigate", "url": "https://example.com" },
  { "type": "observe" },
  { "type": "screenshot", "path": ".solarium/example-session.png" },
  { "type": "extract", "selector": "body", "format": "text" }
]
```

Run it:

```bash
npm run dev -- session \
  --actions .solarium/actions.json \
  --output .solarium/session-result.json \
  --report .solarium/session-report.md \
  --html-report .solarium/session-report.html
```

Useful session options:

```bash
--headed
--engine chromium
--profile chrome-stable
--scope .solarium/scope.json
--evidence-dir .solarium/sessions/demo
--no-observe-after-each-action
--continue-on-error
--max-text-chars 10000
--max-elements 50
--events .solarium/session.events.jsonl
--resume-from .solarium/session.events.jsonl
--trace
```

Supported action types:

- `navigate` — `{ "type": "navigate", "url": "https://example.com" }`
- `click` — `{ "type": "click", "selector": "text=More" }`
- `dblclick` — `{ "type": "dblclick", "selector": ".editable-title" }`
- `hover` — `{ "type": "hover", "selector": "nav .products" }`
- `type` — `{ "type": "type", "selector": "input[name=q]", "text": "query" }`
- `press` — `{ "type": "press", "selector": "input[name=q]", "key": "Enter" }`
- `wait` — `{ "type": "wait", "ms": 1000 }`
- `waitForSelector` — `{ "type": "waitForSelector", "selector": ".results", "state": "visible", "timeoutMs": 10000 }`
- `waitForUrl` — `{ "type": "waitForUrl", "url": "**/dashboard", "timeoutMs": 10000 }`
- `screenshot` — `{ "type": "screenshot", "path": ".solarium/step.png" }`
- `extract` — `{ "type": "extract", "selector": "main", "format": "text" }`
- `observe` — `{ "type": "observe" }`

## Authorized crawl mode

The crawler inventories in-scope pages, links, and forms. It requires a scope policy with `allowedHosts`.

```bash
npm run dev -- crawl https://example.com \
  --scope .solarium/scope.json \
  --max-pages 10 \
  --max-depth 1 \
  --evidence-dir .solarium/crawl/example \
  --screenshots \
  --events .solarium/crawl.events.jsonl \
  --output .solarium/crawl-result.json \
  --report .solarium/crawl-report.md \
  --html-report .solarium/crawl-report.html
```

## Passive audit mode

Solarium can passively load an authorized page and report common defensive web security findings. This mode does not fuzz, brute-force, exploit, or submit forms; it only uses browser-observed evidence from a page load.

```bash
npm run dev -- audit https://example.com \
  --scope .solarium/scope.json \
  --output .solarium/audit-result.json \
  --report .solarium/audit-report.md \
  --html-report .solarium/audit-report.html
```

Add full observation evidence:

```bash
npm run dev -- audit https://example.com \
  --scope .solarium/scope.json \
  --include-observation \
  --wait-after-navigation-ms 1000
```

Current passive findings include:

- missing or weak security headers
- missing clickjacking controls
- missing cookie `Secure` / `HttpOnly` flags
- `SameSite=None` cookies without `Secure`
- HTTPS pages requesting HTTP subresources
- insecure form actions
- password forms on non-HTTPS pages

## OWASP audit and Top 10 scanner modes

Solarium can run OWASP-mapped browser audits that build on the standard audit checks and group findings by OWASP Top 10/ASVS context. Passive profiles remain non-invasive: no fuzzing, brute force, exploitation, or form submission. Active-authorized profiles require explicit scoped authorization and remain bounded.

```bash
npm run dev -- owasp-audit https://example.com \
  --scope scope.json \
  --output .solarium/owasp-audit-result.json \
  --report .solarium/owasp-audit-report.md \
  --html-report .solarium/owasp-audit-report.html
```

Profiles:

- `passive` — default browser-observed OWASP mapping for headers, cookies, mixed content, forms, HTTPS usage, failed resources, third-party scripts, and source-map signals.
- `strict-headers` — reserved stricter profile for deployments that want stronger header expectations as the audit pack grows.
- `active-authorized` — adds a small fixed set of bounded, scoped metadata/sensitive-file/OPTIONS probes. Requires `scope.authorizationNote`.
- `top10-passive` — emits an OWASP Top 10 coverage summary and category-specific findings/review items without active probing.
- `top10-active-authorized` — combines Top 10 coverage reporting with the bounded active-authorized probe set. Requires `scope.authorizationNote`.

Top 10 profiles cover all OWASP Top 10 categories in the report. Some categories are automatically checked from browser/network evidence, while categories such as broken access control, insecure design, and SSRF are marked as manual/app-specific review items rather than falsely claimed as fully verified.

Example Top 10 passive scan:

```bash
npm run dev -- owasp-audit https://example.com \
  --scope scope.json \
  --profile top10-passive \
  --output .solarium/top10-result.json \
  --report .solarium/top10-report.md \
  --html-report .solarium/top10-report.html
```

Example bounded active-authorized Top 10 scan:

```bash
npm run dev -- owasp-audit https://example.com \
  --scope scope.json \
  --profile top10-active-authorized \
  --max-active-requests 10 \
  --active-delay-ms 250 \
  --output .solarium/top10-active-result.json
```

## GraphQL audit mode

Solarium can also run bounded, non-DoS GraphQL endpoint and schema checks for an authorized target. The GraphQL audit supports the same JSON, Markdown, and HTML report pattern as the passive browser audit.

```bash
npm run dev -- graphql-audit https://example.com \
  --scope .solarium/scope.json \
  --endpoint /graphql \
  --output .solarium/graphql-audit-result.json \
  --report .solarium/graphql-audit-report.md \
  --html-report .solarium/graphql-audit-report.html
```

The GraphQL audit probes a small set of candidate endpoints, sends minimal `__typename`/introspection/suggestion/batching checks, and can optionally run known safe read-only data exposure probes when matching schema fields exist:

```bash
npm run dev -- graphql-audit https://example.com \
  --scope .solarium/scope.json \
  --safe-data-probes
```

Current GraphQL findings include:

- endpoint detection
- introspection enabled/not confirmed
- sensitive-looking schema field names
- dangerous-looking operation names
- GET query support
- field suggestions
- batching support
- optional known read-only exposure probes

## Human-readable reports

`session`, `crawl`, `audit`, `owasp-audit`, and `graphql-audit` can write human-readable Markdown and HTML reports in addition to JSON results:

`owasp-audit --profile active-authorized` and `--profile top10-active-authorized` are intentionally bounded: it requires a scope policy with an `authorizationNote`, caps extra probes, delays between probes, and does not perform DoS, brute force, credential attacks, high-rate fuzzing, exploitation, or destructive form submission.

```bash
npm run dev -- audit https://example.com \
  --scope .solarium/scope.json \
  --output .solarium/audit-result.json \
  --report .solarium/audit-report.md \
  --html-report .solarium/audit-report.html
```

Add the full machine-readable result as an appendix when useful. The appendix applies to both Markdown and HTML report output:

```bash
--report-include-json
```

HTML reports are self-contained files with embedded CSS for easier sharing and archiving.

Report support currently includes:

- audit summary tables and finding details
- crawl page inventories, form summaries, and evidence links
- session step timelines, action outcomes, and observation counts
- network policy summaries when scope enforcement is active

## JSONL event timelines, replay, and resume

Long-running sessions and crawls can write a machine-readable event stream while they run. Each line is one JSON object with `type`, `timestamp`, and `payload` fields. This is useful for agents, dashboards, and resumable research workflows that need progress before the final result JSON is available.

### Session events

```bash
npm run dev -- session \
  --actions .solarium/actions.json \
  --scope .solarium/scope.json \
  --events .solarium/session.events.jsonl
```

Session event types include:

- `session.started`
- `session.step.started`
- `session.step.finished`
- `network.policy.summary`
- `session.finished`

### Crawl events

```bash
npm run dev -- crawl https://example.com \
  --scope .solarium/scope.json \
  --events .solarium/crawl.events.jsonl
```

Crawl event types include:

- `crawl.started`
- `crawl.page.started`
- `crawl.page.finished`
- `network.policy.summary`
- `crawl.finished`

Example JSONL line:

```json
{"type":"session.step.finished","timestamp":"2026-01-01T00:00:00.000Z","payload":{"index":0,"ok":true,"action":{"type":"navigate","url":"https://example.com"}}}
```

Summarize an event timeline without launching a browser:

```bash
npm run dev -- replay --events .solarium/session.events.jsonl
```

Write the replay summary to disk:

```bash
npm run dev -- replay \
  --events .solarium/session.events.jsonl \
  --output .solarium/session.replay.json
```

Resume a session by skipping the contiguous prefix of steps that were already marked successful in a prior session event log:

```bash
npm run dev -- session \
  --actions .solarium/actions.json \
  --resume-from .solarium/session.events.jsonl \
  --events .solarium/session-resumed.events.jsonl
```

The resume result includes a `resume` object showing completed steps, failed steps, the computed `resumeFromStep`, and the remaining actions that were executed.


## JSON-RPC / MCP-style server mode

Solarium can run as a newline-delimited stdio JSON-RPC 2.0 server for external agents and MCP-style clients:

```bash
npm run dev -- server
# or after building
node dist/cli/index.js server
```

Supported MCP-style methods include `initialize`, `ping`, `tools/list`, and `tools/call`. Direct JSON-RPC tool calls are also supported with method names like `solarium.browse`, `solarium.inspect`, and `solarium.session`.

Example request:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"solarium.browse","arguments":{"url":"https://example.com","observe":true,"headless":true}}}
```

See [`docs/json-rpc-server.md and `docs/vegvisir-adapter.md``](docs/json-rpc-server.md and `docs/vegvisir-adapter.md`) for the full tool list and protocol examples.

## Library usage

```ts
import { ObservationRecorder, SolariumBrowser } from "solarium";

const browser = await SolariumBrowser.launch({
  engine: "chromium",
  headless: true,
  profile: "chrome-stable",
  trace: true
});

try {
  const page = await browser.newPage();
  const recorder = new ObservationRecorder(page.raw());
  recorder.attach();

  await page.navigate("https://example.com");
  await page.screenshot({ path: ".solarium/example.png" });

  const extracted = await page.extract({ selector: "main", format: "text" });
  const observation = await recorder.observe();

  console.log({ extracted, observation });
} finally {
  await browser.close();
}
```

Session API:

```ts
import { runActions } from "solarium";

const result = await runActions({
  actions: [
    { type: "navigate", url: "https://example.com" },
    { type: "observe" },
    { type: "screenshot" }
  ],
  scope: {
    allowedHosts: ["example.com"],
    authorizationNote: "Authorized example run"
  },
  observeAfterEachAction: true,
  evidenceDir: ".solarium/sessions/example"
});

console.log(result.steps);
```


Inspect API:

```ts
import { inspectPage } from "solarium";

const plan = await inspectPage({
  url: "https://example.com",
  scope: {
    allowedHosts: ["example.com"],
    authorizationNote: "Authorized inspection"
  },
  maxCandidates: 50
});

console.log(plan.candidates);
```


Plan API:

```ts
import { inspectPage, planActionsFromInspectResult, runActions } from "solarium";

const inspect = await inspectPage({ url: "https://example.com", maxCandidates: 50 });
const plan = planActionsFromInspectResult(inspect, { goal: "find documentation" });
const result = await runActions({ actions: plan.actions });

console.log(plan.notes, result.ok);
```

Audit API:

```ts
import { audit, owaspAudit, graphqlAudit, renderGraphqlAuditMarkdownReport } from "solarium";

const result = await audit({
  url: "https://example.com",
  scope: {
    allowedHosts: ["example.com"],
    authorizationNote: "Authorized passive audit"
  }
});

const gql = await graphqlAudit({
  url: "https://example.com",
  endpoint: "/graphql",
  scope: {
    allowedHosts: ["example.com"],
    authorizationNote: "Authorized GraphQL audit"
  }
});

console.log(result.summary, gql.summary);
console.log(renderGraphqlAuditMarkdownReport(gql));
```

Replay API:

```ts
import { replayEvents, createSessionResumePlan } from "solarium";

const summary = await replayEvents(".solarium/session.events.jsonl");
const resume = await createSessionResumePlan(actions, ".solarium/session.events.jsonl");

console.log(summary, resume.resumeFromStep);
```

## Structured observations

The observation layer returns an agent-oriented JSON object containing:

- current URL and page title
- visible text
- links
- buttons
- inputs
- forms and form fields
- console events
- network request/response summary

This is designed to help agents reason about page state without requiring raw screenshots or full DOM dumps for every step.

## Initial design goals

1. **Agent-native control API** — expose high-level actions like `navigate`, `click`, `type`, `select`, `submit`, `wait`, `extract`, `screenshot`, and `download`.
2. **Evidence-first browsing** — every action can produce trace logs, screenshots, DOM snapshots, and network artifacts.
3. **Browser profile emulation** — configurable user agent, viewport, locale, timezone, headers, client hints, permissions, and storage profiles.
4. **Security research tooling** — safe helpers for crawling, endpoint discovery, form inventory, request replay against authorized targets, and vulnerability evidence collection.
5. **Policy-aware operation** — scopes, allowlists/denylists, rate limits, robots/security notes, and explicit target authorization metadata.
6. **Pluggable engines** — start with Playwright/Chromium and design for Firefox/WebKit support.
7. **Headless-first, observable always** — headless by default, with optional headed/debug mode and full trace capture.

## Repository status

The initial TypeScript + Playwright scaffold is in place. See [`docs/architecture.md`](docs/architecture.md) for the current architecture.


## Autonomous inspect-plan-act loop

Solarium includes a bounded autonomous loop for agentic browsing. The loop keeps one browser page open and repeatedly:

1. observes/inspects the current page,
2. ranks selector candidates against an optional goal,
3. executes one or more generated actions,
4. records structured iteration results and optional JSONL events,
5. stops at `--max-iterations`, after repeated no-action iterations, on error, or at a scope boundary.

Example:

```bash
npm run dev -- loop https://example.com \
  --scope .solarium/scope.json \
  --goal "find documentation" \
  --max-iterations 5 \
  --actions-per-iteration 1 \
  --events .solarium/loop.events.jsonl \
  --output .solarium/loop-result.json \
  --report .solarium/loop-report.md \
  --html-report .solarium/loop-report.html
```

Useful options:

```bash
--screenshots
--include-observations
--evidence-dir .solarium/loops/demo
--fill-value "search query"
--wait-after-action-ms 500
--stop-when-text "Documentation"
--stop-when-url "*/docs*"
--stop-when-selector "main article"
--report-include-json
--continue-on-error
```

Early-stop options make loop runs easier to bound for supervised research tasks:

- `--stop-when-text <text>` stops when observed visible text contains the value.
- `--stop-when-url <pattern>` stops when the current URL contains the value, matches a `*` wildcard pattern, or matches a `/regex/`.
- `--stop-when-selector <selector>` stops when the selector exists on the page.

Loop JSON, Markdown reports, HTML reports, and replay summaries include a `stopReason` when one is available.

Loop event timelines are also included in `solarium replay` summaries under the `loops` array, including started/finished iteration counts and failed iteration indexes.

The loop is intentionally bounded and scope-aware. It does not claim goal completion semantically yet; it provides an auditable inspect/plan/act substrate for a supervising agent.

## Config-file job runner

For repeatable agent workflows, Solarium can run a complete job from one JSON file:

```bash
npm run dev -- run .solarium/job.json
```

Supported job modes:

- `browse`
- `inspect`
- `plan`
- `session`
- `crawl`
- `audit`
- `graphql-audit`
- `owasp-audit`
- `loop`
- `replay`

Paths inside a job file are resolved relative to the job file location, so jobs can be moved around with their artifacts directory.

Example plan job:

```json
{
  "mode": "plan",
  "inspectResultPath": "inspect-result.json",
  "output": "plan-result.json",
  "options": {
    "actionsOut": "suggested-actions.json",
    "planOutput": "action-plan.json",
    "goal": "find documentation",
    "maxActions": 4,
    "fillValue": "solarium documentation",
    "preferKinds": ["input", "button", "link"]
  }
}
```

Example loop job:

```json
{
  "mode": "loop",
  "url": "https://example.com",
  "scopePath": "scope.json",
  "output": "loop-result.json",
  "report": "loop-report.md",
  "htmlReport": "loop-report.html",
  "events": "loop.events.jsonl",
  "engine": "chromium",
  "profile": "chrome-stable",
  "options": {
    "goal": "find documentation",
    "maxIterations": 5,
    "actionsPerIteration": 1,
    "stopWhenUrl": "*/docs*",
    "evidenceDir": "loop-evidence",
    "screenshots": true
  }
}
```

Library API:

```ts
import { readSolariumJob, runJob } from "solarium";

const job = await readSolariumJob(".solarium/job.json");
const result = await runJob(job, { jobPath: ".solarium/job.json" });

console.log(result.mode, result.ok, result.outputPath);
```


## Artifact manifests

Create a SHA-256 inventory of Solarium evidence, reports, downloads, traces, and JSON outputs:

```bash
npm run dev -- manifest .solarium/sessions/demo .solarium/audit-report.md \
  --output .solarium/artifact-manifest.json
```

The manifest is machine-readable JSON and includes relative/absolute paths, file size, timestamps, SHA-256 hash, and a best-effort artifact kind such as `screenshot`, `report`, `event-log`, `json-result`, `trace`, `download`, `storage-state`, `profile`, or `config`.

For very large artifacts, skip hashing above a size threshold:

```bash
npm run dev -- manifest .solarium --max-file-bytes 104857600
```

Include hidden files below selected roots when needed:

```bash
npm run dev -- manifest .solarium --include-hidden
```

Library API:

```ts
import { createArtifactManifest } from "solarium";

const manifest = await createArtifactManifest({
  roots: [".solarium/sessions/demo", ".solarium/audit-report.html"],
  output: ".solarium/artifact-manifest.json"
});

console.log(manifest.summary);
```

## Config validation and JSON Schemas

Solarium includes JSON Schema files for editor/tooling integration:

```text
schemas/solarium-job.schema.json
schemas/scope.schema.json
schemas/actions.schema.json
schemas/browser-profile.schema.json
```

Validate configs before launching browser work:

```bash
npm run dev -- validate .solarium/job.json --kind job
npm run dev -- validate .solarium/scope.json --kind scope
npm run dev -- validate .solarium/actions.json --kind actions
```

`--kind auto` is the default and infers the config type from the JSON shape:

```bash
npm run dev -- validate .solarium/job.json
```

Write validation results to disk:

```bash
npm run dev -- validate .solarium/job.json \
  --kind job \
  --output .solarium/job.validation.json
```

Validation output is machine-readable:

```json
{
  "ok": true,
  "kind": "job",
  "path": ".solarium/job.json",
  "issues": []
}
```

The runtime validator checks Solarium-specific requirements that are important for agent workflows, such as:

- `session` jobs requiring `actions` or `actionsPath`
- `plan` jobs requiring `inspectResult` or `inspectResultPath`
- `crawl` jobs requiring `scope` or `scopePath`
- valid action shapes for `navigate`, `click`, `dblclick`, `hover`, `type`, `press`, `wait`, `waitForSelector`, `waitForUrl`, `screenshot`, `extract`, and `observe`
- valid scope host patterns
