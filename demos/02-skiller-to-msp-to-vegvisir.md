# Demo 02 — Skiller → MSP → Vegvisir

## Goal

Show that Skiller-authored skills can be registered into MSP and then consumed
through MSP by Vegvisir.

## One-line pitch

> Skiller authors skills; MSP packages, indexes, verifies, and distributes them;
> Vegvisir consumes them as one host runtime.

## Script

```bash
demos/scripts/02-skiller-to-msp-to-vegvisir.sh
```

## Flow

The script performs:

```text
Skiller bundle
  -> msp-cli publish import-skiller
  -> msp-cli registry index/search/load/trust verify
  -> vegvisir msp search/load/verify-trust
```

## What to show

1. The Skiller sample bundle path under the MSP repo.
2. `publish import-skiller` output with the generated pack and skill id.
3. `registry index` showing one pack and one skill.
4. Search finding the skill by task.
5. Load reporting valid body hash.
6. Vegvisir MSP client loading/verifying the same registry.

## What this proves

- MSP is not just theoretical protocol text.
- Skiller-to-MSP publication is implemented.
- Vegvisir can consume MSP registry artifacts rather than private Skiller state.
- The skill supply-chain boundary is clean.

## Expected key skill id

```text
skill.software_engineering.rust.refactor-module.v1
```
