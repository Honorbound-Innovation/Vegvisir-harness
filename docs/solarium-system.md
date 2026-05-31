# Solarium System

Solarium is Vegvisir’s first-party headless browser automation and evidence runtime. It lives under:

```text
components/solarium
```

It is a Node/TypeScript component built on Playwright. It is intended for legitimate browser automation, QA, research, evidence capture, and authorized web security testing.

## Role In The Vegvisir Ecosystem

Vegvisir owns the agent harness: model providers, tools, policy, memory, transcript, approvals, skills, and orchestration.

Solarium owns browser execution and web evidence collection:

- launch a controlled browser context
- navigate pages
- interact with controls
- capture screenshots
- extract text
- emit agent-readable observations
- collect DOM/network/console/storage evidence
- run scoped crawls and audits
- use browser profiles and auth-session references
- replay or run saved jobs
- produce skill seeds/manifests for reusable workflows

The intended integration is that Vegvisir can ask Solarium to perform browser work while preserving scope, authorization, evidence, and secret boundaries.

## Install And Build

From the Solarium component directory:

```bash
cd components/solarium
npm install
npm run install:browsers
npm run build
npm test
```

For development:

```bash
npm run dev -- --help
npm run dev -- browse https://example.com --observe --screenshot .solarium/example.png
```

After build:

```bash
node dist/cli/index.js browse https://example.com --observe
```

## Current Command Surface

The CLI currently defines these commands in `components/solarium/src/cli/index.ts`:

```text
browse
session
crawl
audit
owasp-audit
graphql-audit
inspect
plan
loop
profiles
profiles:show
profiles:validate
validate
run
replay
auth-session
scope-check
server
skill-seed
manifest
```

## Command Families

### Single-page browsing

`browse` opens a URL and can capture screenshots, extract text, and emit an observation JSON file.

Example:

```bash
npm run dev -- browse https://example.com \
  --observe \
  --observation .solarium/example.observation.json \
  --screenshot .solarium/example.png \
  --extract-text
```

Use this for simple research, visual checks, deployment checks, and evidence capture.

### Session actions

`session` executes an action list against a browser context. It supports workflows such as navigation, clicking, typing, upload/download operations, storage-state use, and evidence capture.

Example action file pattern:

```json
[
  { "type": "navigate", "url": "https://example.com/upload" },
  { "type": "upload", "selector": "input[type=file]", "files": ["./fixtures/sample.txt"] },
  { "type": "click", "selector": "button[type=submit]" },
  { "type": "download", "selector": "a.export", "path": ".solarium/downloads/export.csv" }
]
```

Downloaded files should be treated as untrusted unless the source is fully trusted.

### Crawl and audit

`crawl`, `audit`, `owasp-audit`, and `graphql-audit` are for scoped exploration and authorized testing.

Use a scope file for boundaries:

```json
{
  "allowedHosts": ["example.com", "*.example.com"],
  "blockedHosts": ["accounts.example.com"],
  "maxRequestsPerMinute": 60,
  "authorizationNote": "User owns or is authorized to test this environment."
}
```

Check scope first:

```bash
npm run dev -- scope-check https://example.com --scope .solarium/scope.json
```

Then run a scoped workflow:

```bash
npm run dev -- crawl https://example.com --scope .solarium/scope.json --observe
npm run dev -- audit https://example.com --scope .solarium/scope.json
npm run dev -- graphql-audit https://example.com/graphql --scope .solarium/scope.json
```

### Inspect, plan, and loop

- `inspect` captures deeper page state for debugging and analysis.
- `plan` prepares browser-work plans.
- `loop` supports iterative agent-style browser workflows with repeated observation and action.

These commands are useful when Vegvisir needs Solarium not just as a screenshot tool but as an evidence-producing browser runtime.

### Profiles

Solarium supports built-in and custom browser profiles for reproducible compatibility testing and controlled research.

```bash
npm run dev -- profiles
npm run dev -- profiles --json
npm run dev -- profiles:show chrome-stable
npm run dev -- profiles:validate .solarium/profiles/research-desktop.json
```

A browser profile can define user agent, viewport, locale, timezone, color scheme, device scale factor, mobile/touch flags, and extra HTTP headers.

Profiles are for compatibility, reproducibility, and authorized research. They should not be used for unauthorized bypass or abuse.

### Auth sessions

Solarium supports auth-session profile files that refer to Playwright storage-state files without embedding credentials in actions, logs, or workflow seeds.

Example:

```bash
npm run dev -- auth-session \
  --create .solarium/auth/staging-admin.auth-session.json \
  --name staging-admin \
  --storage-state .solarium/auth/staging-admin.state.json \
  --description "Staging admin browser state" \
  --secret-ref hbse://project/staging-admin
```

Use with commands that support authenticated browsing:

```bash
npm run dev -- browse https://staging.example.com \
  --auth-session .solarium/auth/staging-admin.auth-session.json \
  --scope .solarium/scope.json \
  --observe
```

Storage-state files can contain sensitive session material. Keep them out of source control and out of chat.

### Validation, jobs, replay, server, skill seeds, manifests

- `validate` checks Solarium config/workflow files.
- `run` executes a saved job/config.
- `replay` replays captured or saved workflows where supported.
- `server` exposes a server/runtime interface.
- `skill-seed` emits reusable workflow seed material for Skiller/Vegvisir skill flows.
- `manifest` emits component/job metadata useful for packaging and audit.

## Evidence Model

Solarium is valuable because it produces evidence, not just browser side effects. Evidence may include:

- screenshots
- extracted text
- observation JSON
- DOM snapshots
- network logs
- console logs
- storage state
- downloads
- action results
- audit findings
- replay artifacts

Vegvisir should cite or summarize that evidence when reporting browser-driven findings. If a browser check was not run, say so rather than pretending.

## Security And Acceptable Use

Solarium is for:

- QA
- accessibility/compatibility testing
- deployment checks
- public-web research
- owned-site automation
- authorized security testing
- controlled browser identity/profile research

Solarium must not be used for:

- credential theft
- unauthorized access
- stealth malware behavior
- persistence
- evasion for real-world abuse
- bypassing third-party protections without authorization
- targeting third-party systems outside explicit scope

Scope policies and authorization notes are part of the expected workflow for audits and bug-hunting.

## Integration With HBSE

Solarium should not require plaintext credentials in chat or repository files. Auth workflows should use:

- HBSE secret refs
- auth-session metadata files that reference secret IDs
- local storage-state files treated as sensitive artifacts
- `.gitignore` rules preventing `.solarium/`, state files, downloads, and logs from being committed

## Integration With Skiller

Solarium can generate reusable workflow seeds. Those seeds can become Skiller source material or LSL/USRL skill material when a browser workflow becomes repeatable.

Example pattern:

1. Run a scoped Solarium browser workflow.
2. Preserve screenshots/observations/audit output.
3. Generate a skill seed.
4. Compile or author a governed skill using Skiller/LSL.
5. Add evals and guardrails.
6. Promote/publish only after review.

## Runtime Artifacts And Ignore Policy

Do not commit local browser artifacts unless they are deliberate fixtures/examples.

Ignore or keep local:

```text
components/solarium/node_modules/
components/solarium/dist/
components/solarium/.solarium/
.solarium/
*.trace.zip
storage-state*.json
downloads/
```

The root `.gitignore` already ignores broad runtime/cache patterns such as `node_modules`, `dist`, `.vegvisir`, databases, logs, temp files, and environment files. Add more specific ignores if new Solarium artifact paths become common.

## Tests And Checks

From `components/solarium`:

```bash
npm run build
npm test
npm run check
```

For full monorepo confidence after integration changes:

```bash
cargo check --workspace
```

## Source References

- `components/solarium/package.json` — scripts, package metadata, binary entry
- `components/solarium/src/cli/index.ts` — command definitions
- `components/solarium/README.md` — detailed command examples and security boundary
- `components/components.toml` — component manifest entry and source snapshot metadata
