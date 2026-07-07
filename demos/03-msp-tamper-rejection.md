# Demo 03 — MSP Tamper Rejection

## Goal

Show that MSP catches post-publication skill body tampering.

## One-line pitch

> AI skills are supply-chain artifacts. MSP can detect when a skill body no
> longer matches its manifest/trust metadata.

## Script

```bash
demos/scripts/03-msp-tamper-rejection.sh
```

## Flow

1. Publish a clean Skiller sample bundle into a temporary MSP registry.
2. Verify trust passes.
3. Load the skill successfully.
4. Append a tampered line to `skill.md`.
5. Verify trust reports `passed: false` and `hash_passed: false`.
6. `skills load` refuses to materialize the tampered body.

## What this proves

- MSP body hashes are meaningful.
- Trust verification surfaces tampering.
- Normal load path rejects hash mismatches.
- Skills are treated as verifiable artifacts, not untrusted loose prompt files.

## Nuance

At the time this runbook was created, `msp-cli trust verify` reports failure in
JSON but may still exit `0`. `msp-cli skills load` exits nonzero on a body hash
mismatch. For shell enforcement, prefer `skills load` or add/use a strict trust
verification mode.
