# analysis.hypothesis_evidence_control

## Purpose

Manage competing hypotheses during technical analysis so that hints, user steering, prior knowledge, model assumptions, and tool output are kept distinct from verified evidence.

This skill applies whenever an investigation depends on interpreting clues, selecting candidate keys, explaining ambiguous behavior, debugging a parser/decryptor, or choosing between multiple plausible analysis branches.

## General principle

User guidance, external context, and analyst intuition are valuable hypothesis sources. They are not automatically evidence. Conclusions should be promoted only after verification against artifacts, checksums, protocol behavior, runtime output, reproducible commands, or other concrete observations.

## Use when

- User-provided hints or external context suggest a solve path.
- Multiple plausible interpretations of a clue exist.
- A tool, parser, decryptor, emulator, or solver is being debugged while candidate inputs are uncertain.
- An investigation is spending repeated effort on one branch without verification progress.
- Evidence appears to contradict an assumption, hint, or earlier conclusion.
- The task spans multiple layers such as archive extraction, protocol analysis, cryptography, binary analysis, and reporting.

## Inputs

- Current task goal and authorized scope.
- Artifact paths and observed evidence.
- User hints or steering messages.
- Candidate hypotheses and assumptions.
- Tool outputs, hashes, counts, logs, traces, and verification checkpoints.
- Known blockers or failed attempts.

## Outputs

- Hypothesis ledger with status for each branch.
- Evidence checkpoint table.
- Verification plan for top hypotheses.
- Pivot criteria for abandoning or deprioritizing weak branches.
- Updated analysis plan grounded in artifact evidence.
- Explicit uncertainty and next action.

## Procedure

1. Restate the concrete question being solved.
2. Separate inputs into categories:
   - user authority/scope instruction
   - user hint or steering
   - artifact observation
   - tool output
   - analyst/model inference
   - external reference
3. Create a hypothesis ledger with at least:
   - hypothesis
   - source
   - evidence supporting it
   - evidence against it
   - verification method
   - current status: untested, supported, contradicted, verified, or abandoned
4. Identify hard checkpoints that can validate layers independently. Examples:
   - file hashes
   - packet counts
   - extracted object sizes
   - protocol stream lengths
   - parser roundtrip tests
   - known plaintext or message authentication checks
   - expected magic bytes
   - runtime success/failure output
5. Test hypotheses with the cheapest decisive checks first.
6. If a tool has been validated independently, stop repeatedly blaming the tool unless new evidence points back to it.
7. If candidate inputs repeatedly fail against a validated tool and verified artifact, re-open clue interpretation or upstream assumptions.
8. Preserve parallel branches when user steering and artifact wording differ.
9. Promote only verified findings to final conclusions.
10. Document failed branches briefly so they do not get rediscovered repeatedly.

## Evidence hierarchy

Prefer conclusions grounded in:

1. Reproducible artifact-derived evidence.
2. Hash/checksum/protocol/runtime verification.
3. Tool output corroborated by another method.
4. Source/documentation tied directly to the observed artifact.
5. User-provided hints as hypotheses.
6. Analyst intuition or pattern matching.

## Guardrails

- Do not treat a hint as a confirmed fact without verification.
- Do not discard user steering; convert it into an explicit hypothesis and test it.
- Do not keep modifying tools once independent tests show they work; revisit assumptions and inputs.
- Do not claim a result is solved until it is verified against artifact evidence.
- Do not store secret-like recovered material in durable memory.
- Maintain authorized scope and safety boundaries for any dynamic execution or external contact.

## Anti-patterns

- Anchoring on one plausible clue while ignoring narrower artifact wording.
- Re-debugging a validated parser/decryptor/emulator because candidate inputs fail.
- Collapsing user intent, user hint, and artifact fact into one category.
- Reporting a guessed value without checksum/runtime/protocol verification.
- Following external writeups blindly without reproducing the evidence path.

## Example generalized checkpoint table

| Layer | Checkpoint | Why it matters |
|---|---|---|
| Extraction | expected file count/packet count/hash | confirms the artifact was recovered intact |
| Protocol | stream direction/length/conversation | confirms the right data was selected |
| Tooling | roundtrip test or known fixture | separates tool bugs from wrong inputs |
| Candidate | key/password/parameter test | validates or rejects a hypothesis |
| Payload | magic bytes/hash/runtime output | confirms final recovery |

## Completion criteria

The skill is complete when the analyst can state:

- which hypotheses were considered,
- which evidence supported or rejected them,
- what was verified,
- what remains uncertain,
- and the next action or final conclusion.
