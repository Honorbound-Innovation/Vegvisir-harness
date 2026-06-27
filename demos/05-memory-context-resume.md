# Demo 05 — Useful Memory and Active Context

## Goal

Show that Vegvisir can store useful non-secret project facts, recall them later,
and prepare active context without dumping all history into the model.

## One-line pitch

> Vegvisir remembers project facts without turning secrets or full chat history
> into model context.

## Script

```bash
demos/scripts/05-memory-context-resume.sh
```

## What to show

1. `vegvisir remember` storing a non-secret project fact.
2. `vegvisir recall` retrieving relevant memory.
3. `vegvisir context` preparing active context for a new message.
4. Secret-like memory attempt being blocked, rejected, or visibly handled by the
   current safety policy.

## What this proves

- CMS-v2 provides durable memory.
- ECM/context preparation exposes active context deliberately.
- Useful continuity does not require pasting the whole prior chat.
- Secret handling is separate from memory.

## Recording notes

Use only harmless demo memory. Do not use real credentials, tokens, private URLs,
or personal data.
