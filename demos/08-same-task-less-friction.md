# Demo 08 — Same Task, Less Friction

## Goal

Compare Vegvisir against another harness on the same small task using factual,
non-hostile measurements.

## One-line pitch

> Vegvisir's value is not only capability; it is low operational friction.

## Script

```bash
demos/scripts/08-same-task-less-friction.sh
```

## Fixture

The script creates a tiny Rust repo with a failing `slugify` test. The task is:

```text
Fix the failing slugify test by making the smallest robust change, run tests,
and summarize the diff.
```

## Metrics to record

- Setup steps before useful work starts.
- Pasted context required.
- Number of user interventions.
- Whether the harness discovers/runs the right test command.
- Whether it edits the right file only.
- Final test status.
- Quality of final diff summary.
- Whether memory/secret/tool boundaries remain clear.

## What this proves

This is not a pure feature checklist. It demonstrates the lived operational
claim: Vegvisir is low-friction for real engineering tasks.

## Recording note

Keep it fair. Do not dunk on another tool for failing an artificial setup. The
claim should be: "Here is why I personally use Vegvisir as my daily harness."
