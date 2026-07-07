# Demo 06 — HBSE: No Plaintext Secrets in Chat

## Goal

Show the intended credential boundary: Vegvisir should not ask users to paste API
keys into chat or store secrets in memory; HBSE owns secret handling.

## One-line pitch

> Stop pasting API keys into AI chat. Use secret references and a brokered secret
> boundary instead.

## Script

```bash
demos/scripts/06-hbse-no-plaintext-secrets.sh
```

## What to show

1. HBSE CLI exists and exposes vault/secret/provider/broker/readiness surfaces.
2. Vegvisir setup/doctor surfaces know about provider/HBSE configuration.
3. The public demo uses only fake placeholders or secret references.
4. Secret-like memory write does not become durable project memory.

## What this proves

- Secrets are a separate subsystem, not chat text.
- Provider authentication can be routed through HBSE-backed config.
- Memory and credentials are intentionally separated.

## Safety note

Never record real secrets. Use placeholders or disposable local test credentials
only.
