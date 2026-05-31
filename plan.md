# Vegvisir Contract-Driven Autonomy Plan

## Purpose

This plan records the intended evolution of Vegvisir autonomy mode from a flat Markdown checklist loop into a contract-driven implementation engine.

The goal is to make autonomy mode execute implementation plans deterministically and safely by compiling a human-authored Markdown plan into paired execution libraries:

- `.cll` — Contract Logic Library: implementation logic, rules, guardrails, success conditions, deliverables, validation requirements, and advancement rules.
- `.pll` — Prompt Logic Library: prompt slices associated with each `.cll` phase/section/subsection.

Autonomy mode keeps the standard Vegvisir system prompt as the model system prompt. All `.cll` and `.pll` slices are sent to the model in the user prompt as task-local instructions and never replace or mutate the system prompt.

## Authority and Prompt Hierarchy

Model request structure:

```text
SYSTEM:
  Standard Vegvisir system prompt

USER:
  Autonomy task packet containing:
    - current CLL contract slice
    - current PLL prompt slice
    - current phase/section/subsection identifier
    - objective for this exact unit
    - implementation rules/guardrails for this exact unit
    - expected deliverables
    - success conditions
    - validation/evidence requirements
    - required completion report format
```

Hierarchy:

```text
Vegvisir standard system prompt
  > user autonomy task packet
    > current CLL contract slice
    > current PLL prompt slice
    > model action/report
```

Important rule: `.cll` and `.pll` are task-local user-prompt content. They do not override the Vegvisir system prompt, user authority, tool policy, secret boundary, approval policy, or safety boundaries.

## Source, Compiled, and Runtime Files

Preferred run layout:

```text
.vegvisir/autonomy/<run-id>/
  implementation-plan.md
  implementation.cll
  implementation.pll
  compile-manifest.json
  state.json
  journal.jsonl
  evidence/
    <node-id>.completion.json
    <node-id>.validation.json
  prompts/
    <node-id>.user-prompt.md
```

Initial compatibility layout may also support:

```text
.vegvisir/autonomy/<session-id>-plan.md
.vegvisir/autonomy/<session-id>.cll
.vegvisir/autonomy/<session-id>.pll
.vegvisir/autonomy/<session-id>-compile-manifest.json
```

## Conceptual Flow

```text
implementation-plan.md
        ↓ compile
implementation.cll    implementation.pll
        ↓
autonomy controller selects current node
        ↓
USER prompt receives current CLL slice + PLL slice + runtime state
        ↓
model works one bounded unit
        ↓
model reports deliverables/evidence
        ↓
controller validates against CLL
        ↓
advance, retry, block, or stop
```

## Markdown Authoring Model

The Markdown plan is the human-readable source of truth for the initial plan.

It is broken down by heading hierarchy:

```text
# Implementation Plan
## Phase 1: Foundation
### Section 1.1: Parser
#### Subsection 1.1.1: Markdown AST
```

Checklist items attach to the nearest heading node:

```markdown
- [ ] Inspect current autonomy code
- [ ] Define plan AST structs
```

Known semantic blocks should be parsed or preserved for each node:

```markdown
Success conditions:
- Parser preserves phase/section/subsection ordering
- Checklist items attach to nearest node

Expected deliverables:
- AutonomyPlanAst struct
- Parser unit tests

Implementation rules:
- Keep contract generation deterministic
- Do not mutate unrelated user work

Guardrails:
- Do not request plaintext secrets
- Stop for destructive actions

Validation:
- cargo test autonomy_plan passes
```

## CLL Responsibilities

The `.cll` is the implementation logic. It contains authoritative contract material for the autonomous implementation.

It should include:

- run id
- objective
- source plan path
- phase/section/subsection hierarchy
- stable node IDs
- implementation rules
- guidelines
- guardrails
- allowed actions
- forbidden actions
- approval boundaries
- start conditions
- completion conditions
- success conditions
- expected deliverables
- validation requirements
- evidence requirements
- retry policy
- blocker policy
- dependencies
- stop conditions
- audit events

The `.cll` should support hierarchical completion:

```text
subsection complete
  if checklist complete
  and success conditions satisfied
  and deliverables produced
  and validation/evidence accepted

section complete
  if required subsections complete
  and section-level success conditions satisfied

phase complete
  if required sections complete
  and phase-level deliverables satisfied

run complete
  if all required phases complete
```

## PLL Responsibilities

The `.pll` is the prompt logic library associated with the `.cll`.

It should contain prompt slices for each phase/section/subsection. For the current execution unit, Vegvisir sends only the current `.pll` slice plus the relevant `.cll` slice in the user prompt.

The `.pll` should define:

- prompt binding to the CLL contract and node ID
- task-specific instructions
- expected behavior for the current node
- required response/completion format
- blocker reporting protocol
- evidence reporting protocol
- how to update the human-readable plan/state

## Runtime State

Mutable progress should not be stored by mutating `.cll` or `.pll`.

Use runtime files:

- `state.json` — current node, completed nodes, blocked nodes, attempts, latest validation result.
- `journal.jsonl` — append-only audit log of autonomy events.
- `evidence/` — structured completion and validation packets.
- `prompts/` — optional debug copies of generated user prompts.

Reason: `.cll` and `.pll` should remain compiled artifacts. Runtime state tracks execution without letting the model silently change the contract to pass.

## Stable Node IDs

Every phase/section/subsection compiles to a stable node ID.

Examples:

```text
phase_01_foundation
phase_01_foundation.section_01_parser
phase_01_foundation.section_01_parser.subsection_01_markdown_ast
```

The same ID is used in:

- `.cll`
- `.pll`
- `state.json`
- evidence files
- validation files
- prompt debug files
- logs/journal

## Completion and Evidence Protocol

The model does not advance by saying “done.” It submits a structured completion report for the current node.

Example shape:

```json
{
  "node_id": "phase_01_foundation.section_01_parser",
  "status": "complete",
  "deliverables": [
    {
      "type": "file",
      "path": "vegvisir/src/app/commands/autonomy_plan.rs",
      "description": "Markdown plan parser implementation"
    }
  ],
  "success_conditions_satisfied": [
    {
      "condition": "Parser supports phase/section/subsection headings",
      "evidence": "Unit test parse_nested_plan_sections passed"
    }
  ],
  "verification": [
    {
      "command": "cargo test autonomy_plan",
      "result": "passed"
    }
  ],
  "risks_or_notes": [],
  "next_recommended_action": "advance"
}
```

The controller validates the completion report against the `.cll` node before advancing.

## Validation Adapters

The `.cll` should distinguish deterministically checkable requirements from model/human attestations.

Validation types may include:

- `file_exists`
- `command_passes`
- `diff_contains_symbol`
- `artifact_exists`
- `test_output_contains`
- `model_attestation`
- `human_approval`

The controller should not pretend all success conditions are machine-verifiable.

## Retry, Blocker, and Escalation Policy

Each node may define retry behavior:

```text
retry_policy:
  max_attempts: 3
  on_failure: retry_with_error_context
  on_repeated_failure: pause_and_report
```

Blocker categories:

- `missing_secret`
- `pending_approval`
- `failing_test`
- `ambiguous_requirement`
- `external_dependency`
- `command_not_allowed`
- `insufficient_context`
- `destructive_action_required`

The model should report blockers structurally. The controller decides whether to pause, request approval, retry, or continue elsewhere.

## Context Policy

The model receives only the current task packet, not the whole plan by default.

Possible policy:

```text
include_global_guardrails: true
include_parent_phase_summary: true
include_prior_node_summaries: compact
include_full_plan: false
include_relevant_files: on_demand
```

This protects context budget and keeps each model turn focused.

## Compilation Manifest

Every compile from Markdown to `.cll`/`.pll` should produce a manifest:

```json
{
  "source": "implementation-plan.md",
  "cll": "implementation.cll",
  "pll": "implementation.pll",
  "source_hash": "...",
  "cll_hash": "...",
  "pll_hash": "...",
  "compiled_at": "...",
  "compiler_version": "..."
}
```

The manifest lets Vegvisir detect stale compiled libraries when the Markdown source changes.

## Contract Immutability and Amendments

The model should not silently mutate `.cll` to make itself pass.

If the plan or contract needs to change, autonomy should produce an amendment proposal:

```text
amendments/amend-0003.proposed.md
```

Meaningful amendments should require deterministic safe handling or user approval.

## Commands to Add Over Time

Possible autonomy commands:

```text
/autonomy on
/autonomy off
/autonomy stop
/autonomy status
/autonomy max-steps <n>
/autonomy validate
/autonomy compile
/autonomy inspect
/autonomy resume <run-id>
/autonomy dry-run
```

## Implementation Phases

### Phase 1: Record Plan and Compile Libraries

- Add this `plan.md`.
- Parse Markdown headings and checklist items into a plan AST.
- Generate deterministic `.cll` and `.pll` files.
- Generate `compile-manifest.json` with hashes.
- Add tests for parser and compiler.

### Phase 2: Integrate with Current Autonomy Loop

- Current loop still requests a Markdown plan.
- When the plan exists, compile/update `.cll` and `.pll` if missing or stale.
- Continuation prompts reference generated `.cll`/`.pll` slices in the user prompt.
- Keep the standard system prompt unchanged.
- Retain existing Markdown checklist completion as a compatibility stop condition.

### Phase 3: Contract-Aware Advancement

- Track current node in runtime state.
- Select the first incomplete phase/section/subsection.
- Send only that node’s `.cll`/`.pll` slice to the model.
- Require structured completion/evidence packet.
- Advance only after validation.

### Phase 4: Rich Validation and Resume

- Add validation adapters.
- Add state/journal/evidence files.
- Add `/autonomy resume`.
- Add amendment flow.
- Add dry-run/validate commands.

## Initial Practical Target

The first implementation should be conservative:

1. Keep existing autonomy behavior working.
2. Add deterministic Markdown-to-CLL/PLL compilation.
3. Write compiled files next to the plan.
4. Include current task-local CLL/PLL content in the user prompt.
5. Preserve the standard system prompt.
6. Continue using the Markdown checklist as the initial completion gate until contract-aware advancement is implemented.

## Implementation Progress

### Completed: Initial Compiler and Integration

- Root `plan.md` records the contract-driven autonomy design.
- Markdown implementation plans compile into adjacent `.cll`, `.pll`, and compile manifest files.
- The autonomy controller keeps the standard Vegvisir system prompt unchanged.
- Generated CLL/PLL content is injected as task-local USER prompt content.
- Parser/compiler tests cover heading hierarchy, semantic lists, and library generation.

### Completed: Current-Node State and Sliced Prompts

- Compiled runs now also write a runtime status file next to the plan:
  - `<plan-stem>-state.json`
- Runtime `AutonomyState` tracks:
  - state path
  - current node ID
  - current node title
  - completed node count
  - total executable node count
- The controller selects the first incomplete executable node from the Markdown-derived plan status.
- Continuation prompts now include only the current node's CLL contract slice and PLL prompt slice instead of the whole generated library.
- Progress signatures include current-node and node-completion state, improving no-progress detection.

### Remaining Next Slice

The next implementation slice should add structured completion/evidence packets and deterministic validation adapters so the controller can advance based on evidence files, not just Markdown checklist state.


## Implementation Progress Update

- Added per-node JSON completion evidence packet scaffolding and validation.

### Completed: Evidence-Gated Node Advancement

- Node completion is now contract-aware instead of checklist-only.
- A node is considered complete only when:
  - it has checklist items,
  - all checklist items are checked, and
  - its current JSON completion evidence packet validates against the node requirements.
- Runtime state now records per-node evidence metadata:
  - `checklist_complete`
  - `evidence_path`
  - `evidence_valid`
  - `evidence_errors`
  - final `complete` status
- The autonomy controller no longer stops just because all Markdown checklist items are checked.
- Final completion now requires all executable contract nodes to have checked checklists and validated evidence.
- Continuation prompts explain that checked Markdown items alone are insufficient; evidence packets are required for advancement/completion.
- Tests now verify that a checked node without valid evidence remains incomplete and does not advance until a valid completion packet exists.

### Remaining Next Slice

Add richer validation adapters and node-level lifecycle controls:

- deterministic validation adapter types such as `file_exists`, `command_passes`, and `path_changed`,
- explicit blocked/partial handling from evidence packets,
- retry counters per node,
- append-only journal events,
- `/autonomy validate` for compile/contract/evidence validation without execution.

### Completed: Validation Adapters and Audit Journal Groundwork

- Added deterministic validation adapter support for CLL `Validation:` requirements:
  - `file_exists: <path>` checks that a workspace-relative file exists.
  - `path_exists: <path>` checks that a workspace-relative path exists.
  - `deliverable_path: <path>` checks that the completion packet deliverables reference the required path.
  - `path_changed: <path>` currently aliases deliverable path evidence and can later become diff-aware.
  - `command_passes: <command>` checks that the completion packet verification list reports the exact command with a passed/success result.
- Unknown validation lines remain non-executed semantic requirements and still require verification evidence for completed packets.
- Evidence validation now records adapter results in node status/state output.
- Added `<plan-stem>-journal.jsonl` as append-only audit groundwork.
- Library compilation appends `plan_compiled` journal events.
- Runtime node-status transitions append `node_status` journal events when the current node or evidence validity changes.
- `/autonomy status` now exposes the journal path.
- Tests cover both passing and failing validation adapter behavior.

### Remaining Next Slice

Add richer lifecycle and operator controls:

- explicit `blocked` and `partial` node handling from evidence packets,
- retry counters / attempt limits per node,
- `/autonomy validate` to compile/check plan, contract, state, evidence, and adapters without running the model,
- `/autonomy resume <run-id-or-plan>` using `state.json` and `journal.jsonl`,
- optional diff-aware `path_changed` validation.

## Implementation progress update: lifecycle, validation commands, and diff-aware `path_changed`

Implemented the next autonomy slice after evidence-gated node completion:

- Added explicit evidence lifecycle visibility:
  - `complete` / `completed` packets can validate and advance nodes.
  - `blocked` packets are surfaced as blocker state and stop active autonomy with a blocked reason.
  - `partial` packets are visible as progress state but do not complete nodes.
- Added runtime attempt tracking:
  - `current_node_attempts` are derived from node status journal events.
  - `max_node_attempts` defaults to 3.
  - `/autonomy max-attempts <n>` configures the retry budget.
  - active autonomy stops on `max_node_attempts_exceeded` when the current node still lacks valid completion evidence.
- Added manual control commands:
  - `/autonomy validate [plan-path]` recompiles/validates the current or supplied plan and reports node/evidence status.
  - `/autonomy resume <plan-path>` recompiles the plan, reloads CLL/PLL/state/evidence paths, restores current node status, and marks autonomy active.
- Made `path_changed:<path>` truly diff-aware:
  - It now requires the completion packet deliverables to reference the exact path.
  - It also requires `git status --porcelain=v1 -- <path>` to report a working-tree/index change for that path.
  - This avoids treating a mere deliverable claim as proof of a changed path.
- Updated CLL/PLL slices and continuation prompts to explain blocked/partial status, retry/attempt state, and diff-aware `path_changed` requirements.
- Added tests for:
  - diff-aware `path_changed` failing before a git diff exists and passing after the path changes,
  - blocked and partial packets being surfaced without completing the node.
