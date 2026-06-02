# analysis.multi_stage_artifact_chain

Multi-stage artifact chain analysis

Use this skill when an investigation depends on a chain of dependent stages rather than one direct artifact. Maintain an evidence ledger that records every clue, password candidate, file, hash, transformation, rejected branch, and verified transition.

General workflow:
1. Create a case ledger with stage number, source artifact, action, output artifact, verification method, confidence, and next dependency.
2. Preserve original artifacts and work on copies.
3. Verify each layer before using its output: file magic, archive listing, packet count, checksum, parser roundtrip, expected plaintext, or runtime output.
4. Treat hints and recovered strings as candidate keys until they unlock an artifact or pass an independent check.
5. Test casing, punctuation, repetition, corrected challenge notes, and documented bug-workarounds systematically.
6. Record failed hypotheses without letting them erase artifact-backed branches.
7. Produce a dependency map that shows how the final answer follows from verified intermediate artifacts.

Outputs should include a chain map, evidence ledger, candidate/rejected key table, verification checkpoints, and a reproducible command summary.

## Guardrails

- Use copies and read-only inspection before extraction or mutation.
- Do not execute recovered payloads unless explicitly authorized and sandboxed.
- Do not treat clues as facts until verified by artifact behavior.
- Do not store flags, credentials, tokens, or secret-like answers in durable memory/examples.
- Track provenance for every recovered key, password, and file.
