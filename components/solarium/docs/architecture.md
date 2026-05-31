# Solarium architecture

Solarium is organized around a small set of composable layers.

## Layers

```text
CLI / MCP / JSON-RPC integrations
          |
Agent session/action API
          |
Observation and evidence capture
          |
Policy, security scope, and network guards
          |
Browser engine abstraction
          |
Playwright browser contexts
```

## Core concepts

### Browser engine

`SolariumBrowser` wraps Playwright's Chromium, Firefox, and WebKit launchers. It creates isolated browser contexts with consistent artifacts, tracing, downloads, and profile settings.

### Browser profile emulation

Profiles are controlled bundles of browser-facing settings:

- User-Agent
- viewport
- locale
- timezone
- HTTP language headers
- color scheme
- device scale factor
- mobile/touch flags
- optional permissions

The goal is reproducible compatibility testing and authorized security research, not abusive evasion.

### Agent actions and sessions

The action layer supports both one-shot `browse()` calls and persistent multi-step sessions.

Current actions:

- `navigate`
- `click`
- `dblclick`
- `hover`
- `type`
- `press`
- `wait`
- `waitForSelector`
- `waitForUrl`
- `screenshot`
- `extract`
- `observe`

`AgentSession` keeps a browser/page open across multiple actions and records step results. Sessions can automatically observe after each action so an external agent receives structured page state after every interaction.

### Observation layer

`ObservationRecorder` captures agent-readable page evidence:

- current URL and title
- visible page text
- links
- buttons
- inputs
- forms
- console events
- network events

This gives agents compact JSON evidence instead of requiring full DOM dumps or screenshots for routine reasoning.

### Scope policy

Security research workflows should use explicit scopes. The scope module supports:

- host allowlists
- host denylists
- wildcard subdomain patterns such as `*.example.com`
- max request rate metadata
- authorization notes

### Network scope guard

`NetworkScopeGuard` attaches to a Playwright page and enforces configured scope policy at the browser-network boundary. It checks every HTTP/HTTPS request against the same allow/block rules used for explicit navigations. Out-of-scope requests are aborted before they leave the browser.

The guard also returns a `NetworkPolicySummary` with:

- allowed request count
- blocked request count
- rate-limited request count

Non-HTTP browser-internal resources such as `data:`, `blob:`, and `about:` are permitted so normal rendering can continue.

### Crawler

The crawler is intentionally scope-first. It requires a policy with `allowedHosts`, visits in-scope pages up to configured page/depth limits, records observations, inventories forms, and optionally writes screenshots/evidence files.

## Near-term roadmap

1. Add stronger selector generation for observed elements.
2. Add MCP or JSON-RPC server mode for external agents.
3. Add tests around scope matching, action validation, network guard behavior, and crawler queueing.
