# Run Artifacts

Vegvisir records durable run bundles under `.vegvisir/runs/<run-id>/` for TUI and headless work. Bundles are redacted before write and are intended for audit, recovery, handoff, and debugging.

Core files include `manifest.json`, `request.json`, `context.md`, `context-sources.json`, `provider-events.jsonl`, `tool-events.jsonl`, `file-changes.json`, `diff.patch`, `memory-used.json`, `memory-written.json`, `approvals.json`, `subagents.json`, `result.md`, `verification.json`, and `failure.json` when relevant.

Use `/runs`, `/runs show latest`, `/runs diff latest`, and `/runs replay-plan latest` in the TUI.
