# Troubleshooting

- Stuck turn: use `/turn-repair` or `/turn-repair force`.
- Missing provider auth: use `/provider diagnose <provider>` and `/auth <provider>`.
- Unknown context exposure: use `/context last` or `/context explain <message>`.
- Missing artifact: use `/runs show latest` and check `.vegvisir/runs` permissions.
- Subagent conflict: use `/subagents ownership` and assign non-overlapping scopes.
