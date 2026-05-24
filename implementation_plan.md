# LSL Skill System Implementation Plan

## Purpose

This plan describes the remaining work needed to complete the Linked Skill Library (LSL) system as described in `skillssys.md`, based on the verification pass against the current implementation.

Current status: Vegvisir already has a working LSL subset with parsing, validation, registry compilation, lexical routing, dependency closure, token-budgeted context loading, eval hooks, forge/promote/archive/patch commands, tracing, detection, and curation reports.

Target status: Vegvisir should treat `.lsl` files as canonical USRL-defined linked skill libraries where sub-skills are first-class callable procedural units, automatically routed and materialized into model context per task, with policy gates, token economics, evaluation, promotion, tracing, and curation integrated into the runtime.

---

## Guiding Implementation Principles

1. **Keep `.lsl` as the source of truth**  
   Source skills live in `.lsl` files. Compiled artifacts are acceleration/cache outputs only.

2. **Treat the sub-skill as the runtime unit**  
   Libraries are namespaces. The router, loader, evaluator, curator, and promotion gate should operate primarily on sub-skills.

3. **Do not load whole libraries by default**  
   Normal runtime loading should follow:

   ```text
   library card
   -> sub-skill index
   -> selected sub-skill signatures/cards
   -> selected bodies
   -> required dependency cards/bodies
   -> optional extended sections only when justified
   ```

4. **Integrate with existing Vegvisir safety boundaries**  
   Runtime LSL policy enforcement must respect existing approval, command, HBSE, USRL, and no-secret boundaries.

5. **Make each phase testable**  
   Every implementation phase should add focused unit/integration tests before moving to the next runtime behavior.

---

## Phase 0 — Baseline Stabilization

### Goal

Lock in the currently implemented LSL behavior so subsequent changes do not regress the working subset.

### Work Items

- Add or confirm regression coverage for:
  - parsing canonical existing `.lsl` examples
  - loading sub-skills from workspace skill roots
  - registry compilation
  - source/canonical/semantic hash generation
  - route command behavior
  - load command behavior
  - dependency closure for `requires`
  - eval hook pass/fail behavior
  - forge/promote/archive/patch commands
  - trace/detect/curate commands
- Add golden fixture files for representative valid and invalid `.lsl` libraries.
- Document the current supported syntax in one authoritative location.

### Acceptance Criteria

- Existing LSL tests pass.
- New regression fixtures cover the current parser and registry behavior.
- A developer can distinguish current-supported syntax from planned syntax.

### Suggested Verification

```bash
cargo test -p vegvisir-rust lsl -- --nocapture
```

---

## Phase 1 — Canonical USRL/LSL Syntax Completion

### Goal

Move from a pragmatic custom parser toward the canonical USRL object model described in `skillssys.md`.

### Current Gap

The implementation supports USRL-style `.lsl` files but is not a full canonical USRL lexer/parser/compiler pipeline. Most functionality is concentrated in `vegvisir/src/lsl.rs`.

### Work Items

- Define the canonical `.lsl` grammar explicitly:
  - object declaration: `type identifier { ... }`
  - field assignment: `name: value;`
  - lists: `[a, b, c]`
  - nested objects
  - references/dotted identifiers
  - quoted strings and triple-quoted strings
  - canonical field ordering
- Split parser responsibilities into clearer stages:
  - lexer/tokenizer
  - parser to AST
  - schema validator
  - reference resolver
  - canonicalizer
  - compiler/index builder
- Preserve compatibility with existing valid `.lsl` files or provide a migration command/guide.
- Enforce canonical field ordering when emitting canonical text.
- Improve diagnostics with source spans where practical.

### Acceptance Criteria

- `.lsl` files conforming to the canonical examples in `skillssys.md` parse successfully.
- Invalid syntax produces actionable parse errors.
- Canonical serialization is deterministic.
- Existing example libraries either still parse or have an explicit migration path.

---

## Phase 2 — Complete Typed Internal Model

### Goal

Represent the full LSL design as typed Rust structures instead of partial or loosely represented fields.

### Current Gap

Many model fields exist, but the full function-like sub-skill abstraction, schema details, lifecycle states, metrics, changelog/assets, and relationship semantics are incomplete or underused.

### Work Items

- Expand or confirm typed models for:
  - `LinkedSkillLibrary`
  - `LslSubskill`
  - `LslLink`
  - `LslEval`
  - `LslPolicy`
  - activation rules
  - signature input/output schema
  - required concepts/tools
  - context budget
  - load blocks: card/body/extended
  - metrics
  - changelog
  - assets/templates/references, if retained from the design
- Add lifecycle statuses:
  - `candidate`
  - `sandboxed`
  - `evaluated`
  - `active`
  - `stale`
  - `archived`
  - `pinned`
- Ensure library-level and sub-skill-level versions are tracked independently.
- Add semantic-ish version change rules to docs and validation:
  - library version changes when sub-skills, graph, or policy changes
  - sub-skill version changes when body, activation, dependencies, risk, or eval requirements change

### Acceptance Criteria

- All major fields described in `skillssys.md` have typed representations.
- Serialization/deserialization round trips preserve data.
- Validation rejects missing required fields for active/evaluated sub-skills.

---

## Phase 3 — Compiled Artifact Expansion

### Goal

Generate the acceleration artifacts described in the design.

### Current Gap

The current compiler emits AST/index/link/policy/eval/hash artifacts, but not all planned artifacts such as `skill_index.json`, `dependency_graph.json`, `token_map.json`, `policy_map.json`, or embeddings.

### Work Items

- Add compiled outputs under `.vegvisir/compiled`, such as:

  ```text
  skill_index.json
  subskill_index.json
  dependency_graph.json
  token_map.json
  policy_map.json
  eval_index.json
  hashes/hashes.json
  ```

- Include in `skill_index.json` / `subskill_index.json`:
  - library id
  - sub-skill id
  - title
  - summary
  - tags
  - risk
  - status
  - dependencies
  - token costs
  - policy id/effective policy summary
- Include in `dependency_graph.json` all supported relation types:
  - `requires`
  - `related`
  - `conflicts`
  - `extends`
  - `replaces`
  - `fallback`
  - `specializes`
  - `generalizes`
- Include in `token_map.json`:
  - declared card/body/extended token cost
  - measured card/body/extended token cost
  - rolling average token cost
- Include in `policy_map.json`:
  - inherited library policy
  - sub-skill policy additions
  - effective policy
  - risk level
  - approval requirements

### Acceptance Criteria

- Compiled artifacts are deterministic.
- Artifacts can be regenerated from `.lsl` source alone.
- Tests verify artifact shape and important fields.

---

## Phase 4 — Progressive Materialization Model

### Goal

Implement the full loading-level strategy.

### Current Gap

The current loader supports card/body/extended and loads primary sub-skills as body with dependencies as card/body, but it does not fully represent Level 0 through Level 4 loading.

### Work Items

Implement explicit loading levels:

1. **Level 0: Library Card**
   - library purpose
   - risk
   - status
   - sub-skill count
   - compact namespace summary

2. **Level 1: Sub-skill Index**
   - compact searchable list of sub-skills
   - title, summary, tags, risk, token costs

3. **Level 2: Sub-skill Card**
   - compact callable description
   - activation summary
   - dependency summary
   - key constraints

4. **Level 3: Sub-skill Body**
   - operational procedure
   - verification checklist
   - forbidden behavior
   - failure modes

5. **Level 4: Extended Notes / Examples / Eval References**
   - examples
   - templates
   - long caveats
   - eval references
   - troubleshooting notes

- Update `LoadedSkillContext` to report:
  - selected sub-skills
  - load mode per sub-skill
  - reason per selection
  - dependencies loaded
  - conflicts excluded
  - not-loaded relevant alternatives
  - token budget available/used/remaining

### Acceptance Criteria

- Loader can materialize each level independently.
- Runtime output matches the structured loader output described in `skillssys.md`.
- Tests verify that broad libraries are not fully loaded for narrow requests.

---

## Phase 5 — Token Economy and Budgeter

### Goal

Implement a token-aware context selector that uses declared and measured token costs.

### Current Gap

Token estimation is currently whitespace based and not persistently updated from actual materialization/use.

### Work Items

- Add a `SkillContextSelector` / budgeter component.
- Use declared `context_budget` values first.
- Measure actual rendered token cost after materialization.
- Persist measured token costs in compiled metrics or a workspace-local skill metrics store.
- Prefer measured rolling averages over declared estimates when available.
- Implement load-mode selection rules:
  - primary sub-skill: body by default
  - required dependency: body or card depending on available budget and task depth
  - related dependency: card by default
  - conflicting skill: exclude unless explicitly needed for comparison
  - extended: only for code generation, troubleshooting, eval, or explicit request
- Enforce:
  - max primary sub-skills
  - max total sub-skills
  - max dependency depth
  - available token budget

### Acceptance Criteria

- Loader never exceeds the supplied skill token budget except by explicit fallback policy.
- Measured token costs are updated after materialization.
- Tests cover constrained-budget behavior.

---

## Phase 6 — Semantic Router and Ranking Signals

### Goal

Upgrade routing from lexical matching to a multi-signal router.

### Current Gap

Routing is currently lexical/term-overlap based. The design calls for semantic similarity, embeddings over cards/summaries, tool compatibility, success score, recent success, and risk level.

### Work Items

- Keep lexical matching as a deterministic baseline.
- Add a pluggable semantic search interface.
- Build embeddings over compact text only:
  - library cards
  - sub-skill summaries
  - sub-skill cards
  - intent tags
- Store embeddings or embedding metadata in `subskill_embeddings.json` or a provider-appropriate local cache.
- Add ranking signals:
  - lexical score
  - semantic similarity score
  - activation positive/negative matches
  - intent tag match
  - tool compatibility
  - dependency compatibility
  - success score
  - recent success
  - risk penalty/boost depending on task safety
  - status penalty for stale/candidate/sandboxed unless explicitly allowed
- Make router output auditable:
  - candidate id
  - score
  - matched signals
  - exclusion reason if filtered

### Acceptance Criteria

- Router can run without embeddings and still produce deterministic lexical results.
- When embeddings are available, semantic score participates in ranking.
- Negative activation terms suppress or exclude unsafe/inappropriate sub-skills.
- Tests cover lexical-only and mixed-signal ranking.

---

## Phase 7 — Full Dependency Graph Semantics

### Goal

Operationalize all link relation types, not just `requires`.

### Current Gap

The current dependency closure follows required concepts and `requires` links. Other relation types are validated but mostly unused.

### Work Items

Implement runtime behavior for:

- `requires`
  - always include in dependency closure unless unavailable or policy-blocked
- `related`
  - consider as optional card-only context when budget allows
- `conflicts`
  - exclude by default; include only for comparison/review tasks
- `extends`
  - include parent/base context if loading extension
- `replaces`
  - prefer replacement over replaced stale/deprecated sub-skill
- `fallback`
  - use when primary sub-skill is unavailable, policy-blocked, or incompatible with tools
- `specializes`
  - prefer specialized sub-skill for narrow matching tasks; include parent card/body as needed
- `generalizes`
  - include generalized parent card for orientation when loading narrow sub-skill

- Detect graph problems:
  - missing references
  - cycles in `requires`
  - policy-incompatible dependencies
  - conflicting selected sub-skills
  - replacement loops

### Acceptance Criteria

- Dependency resolver returns typed decisions with reasons.
- Tests cover each relation type.
- Cycles and conflicts are reported clearly.

---

## Phase 8 — Runtime Policy Gates

### Goal

Enforce LSL policies during route/load/apply, not only during validation.

### Current Gap

Policy inheritance and weakening checks exist, but policy gates are not fully wired into runtime selection, loading, approval, or action decisions.

### Work Items

- Compute effective policy as:

  ```text
  effective_policy = library_policy + inherited_policy + subskill_policy
  ```

  Sub-skill policy may extend but must not weaken library policy.

- Enforce before materialization:
  - forbidden task categories
  - negative activation triggers
  - risk thresholds
  - approval-required categories
  - required tools availability
  - credential/HBSE boundaries
- Integrate with Vegvisir approval flow:
  - if a selected sub-skill requires approval for the current task, queue/request approval according to existing approval mode
  - do not bypass existing command/tool/USRL/HBSE gates
- Include policy decisions in loader output:
  - allowed
  - blocked
  - approval required
  - reason
- Ensure policy gates apply to dependencies as well as primary sub-skills.

### Acceptance Criteria

- Runtime load refuses policy-forbidden sub-skills.
- Runtime load can mark approval-required sub-skills and halt/defer appropriately.
- Tests cover policy inheritance, policy blocking, and approval-required cases.

---

## Phase 9 — Automatic Per-Turn LSL Context Injection

### Goal

Wire the LSL runtime loop into normal model request preparation.

### Current Gap

Manual `/skills route` and `/skills load` commands exist, and agent-enabled LSL sub-skills can be included in prompts, but normal user requests do not clearly pass through the full automatic LSL router/loader/context-injection path.

### Work Items

- Add an automatic skill-loading step before provider request construction:

  ```text
  user task
  -> compile/load LSL registry
  -> route candidate sub-skills
  -> rank candidates
  -> select primary sub-skills
  -> resolve dependency graph
  -> enforce policy gates
  -> materialize within token budget
  -> inject compact LSL context into model request
  -> trace selected/blocked/not-loaded sub-skills
  ```

- Make automatic LSL loading configurable:
  - off
  - manual only
  - suggestions only
  - automatic safe sub-skills only
  - full automatic with approvals
- Add workspace/user config options for:
  - skill token budget
  - max primary sub-skills
  - max total sub-skills
  - max dependency depth
  - allow extended sections
  - enable semantic router
- Prevent duplicate injection when a custom agent has explicitly enabled the same sub-skill.
- Ensure LSL context is placed in a predictable section of the model request.
- Trace selected sub-skills with reasons and token costs.

### Acceptance Criteria

- A normal user prompt can automatically load relevant sub-skills without manual `/skills load`.
- Automatic loading is auditable and configurable.
- The model request includes only selected subgraph context, not whole libraries.
- Tests or golden cases verify prompt construction with LSL context.

---

## Phase 10 — Callable Sub-Skill Abstraction

### Goal

Represent sub-skills as callable procedural units with input/output schemas, even though execution remains model-mediated.

### Current Gap

Sub-skills currently materialize as context text. They are not exposed as typed callable units with validated inputs and expected structured outputs.

### Work Items

- Add a `CallableSubskill` representation:
  - id
  - title
  - version
  - risk
  - input schema
  - output schema
  - required tools
  - materialization modes
  - policy
- Add input extraction/validation where feasible:
  - required inputs
  - optional enum inputs
  - missing input warnings
- Add output expectation hints into context:
  - expected sections
  - checklist outputs
  - structured response shape
- Optionally expose a command or internal API:

  ```text
  /skills invoke <subskill-id> [json-input]
  ```

- Keep this model prompt-mediated unless/until a separate deterministic execution layer is explicitly designed.

### Acceptance Criteria

- Sub-skill signatures influence loaded context and model instructions.
- Missing required inputs are surfaced to the model or user.
- Invocation output can include expected output schema hints.

---

## Phase 11 — Evaluation System Expansion

### Goal

Move eval hooks from string-presence checks toward behavior-level evals that can gate promotion.

### Current Gap

Current eval hooks check expected/forbidden strings in materialized context. They do not evaluate model answers or compare against baselines.

### Work Items

- Keep lightweight local evals for deterministic checks.
- Add model-response eval mode:
  - generate response using candidate sub-skill
  - compare against expected behavior
  - check forbidden behavior
  - compute rubric score
- Support baseline comparison:
  - previous active version
  - no-skill baseline
  - existing related sub-skill
- Persist eval reports:
  - eval id
  - target sub-skill
  - source hash
  - canonical hash
  - semantic hash
  - score
  - pass/fail
  - failure reasons
- Add eval categories:
  - safety
  - routing
  - procedural correctness
  - token efficiency
  - forbidden behavior avoidance
- Ensure evals do not require plaintext secrets and respect HBSE/no-secret policy.

### Acceptance Criteria

- Candidate promotion can be gated on eval score.
- Eval failures explain expected and forbidden behavior mismatches.
- Tests cover deterministic local evals; model evals may be optional/configured.

---

## Phase 12 — Promotion Flow Completion

### Goal

Implement the full candidate → sandboxed → evaluated → active lifecycle.

### Current Gap

Promotion exists and requires eval hooks, but the full lifecycle and promotion criteria are simplified.

### Work Items

- Implement lifecycle transitions:

  ```text
  candidate -> sandboxed -> evaluated -> active
  active -> stale -> archived
  active -> pinned
  ```

- Enforce transition rules:
  - candidate cannot become active directly unless explicit override is allowed
  - sandboxed sub-skills can be tested but not automatically used in normal runtime
  - evaluated requires eval report
  - active requires promotion gate pass
- Promotion gate criteria:
  - eval score exceeds threshold
  - candidate score exceeds baseline score where applicable
  - no policy regression
  - no token explosion
  - no dependency conflict
  - no forbidden behavior introduced
  - valid version bump
- Record promotion metadata:
  - who/what promoted
  - timestamp
  - previous hash
  - new hash
  - eval report ids
  - reason

### Acceptance Criteria

- Invalid lifecycle transitions are rejected.
- Promotion reports are auditable.
- Tests cover successful and failed promotion paths.

---

## Phase 13 — Metrics, Trace, and Feedback Loop

### Goal

Update sub-skill metrics from actual runtime use.

### Current Gap

Tracing exists for commands, and metrics exist structurally, but runtime use does not fully update sub-skill metrics such as success/failure counts, last used time, and measured token cost.

### Work Items

- Record each automatic and manual skill load:
  - timestamp
  - task hash or redacted task summary
  - selected sub-skills
  - load modes
  - policy decisions
  - token budget/used
  - provider/model if non-sensitive
  - outcome if known
- Update metrics:
  - use_count
  - success_count
  - failure_count
  - average_token_cost
  - last_used_at
  - eval_score
  - stale_score
  - duplicate_score
- Add explicit user/model feedback hooks if available:
  - mark skill result helpful/unhelpful
  - mark skill mismatch
  - mark missing skill
- Avoid storing secrets or raw sensitive user content in traces/metrics.

### Acceptance Criteria

- Sub-skill metrics change after runtime use.
- Metrics are redacted/non-secret.
- Curator can use real use-count and token metrics.

---

## Phase 14 — Curator and Evolver Completion

### Goal

Make curation operate at both library and sub-skill levels, with actionable patch/split/merge/archive recommendations.

### Current Gap

The curator produces useful reports, but it is mostly diagnostic. It does not fully propose or execute structured evolution flows.

### Work Items

- Library-level curation metrics:
  - total sub-skills
  - active/stale/archived counts
  - duplicate count
  - average success rate
  - average token cost
  - most-used sub-skills
  - least-used sub-skills
- Sub-skill-level curation metrics:
  - use_count
  - success/failure count
  - average token cost
  - last_used_at
  - eval score
  - stale score
  - duplicate score
- Add recommendation types:
  - archive stale sub-skill
  - patch body
  - compress body
  - split broad sub-skill
  - merge duplicate sub-skills
  - add missing dependency
  - replace deprecated sub-skill
  - add eval coverage
- Add safe evolver workflow:
  - propose patch as candidate
  - show before/after hashes
  - run evals
  - require promotion gate
- Keep automated destructive changes disabled unless explicitly approved.

### Acceptance Criteria

- Curator reports actionable sub-skill-level recommendations.
- Evolver proposals are candidates, not direct active modifications.
- Tests cover duplicate/stale/missing/eval-failing recommendations.

---

## Phase 15 — Command and UX Updates

### Goal

Expose the completed LSL system through clear commands and status output.

### Work Items

Review and expand `/skills` commands:

```text
/skills compile
/skills status
/skills route <task>
/skills load <task>
/skills explain <task>
/skills invoke <subskill-id> [input]
/skills eval <subskill-id|eval-id>
/skills promote <subskill-id>
/skills sandbox <subskill-id>
/skills archive <subskill-id>
/skills patch <subskill-id>
/skills trace
/skills detect
/skills curate
/skills config
```

Add status output for:

- automatic loading mode
- registry freshness
- compiled artifact location
- number of libraries/sub-skills
- semantic router availability
- embedding index availability
- token budget settings
- policy gate mode

### Acceptance Criteria

- Users can inspect what was loaded and why.
- Users can disable or constrain automatic loading.
- Commands do not expose secrets or raw sensitive traces.

---

## Phase 16 — Documentation and Examples

### Goal

Document the final system and provide realistic `.lsl` examples.

### Work Items

- Update `docs/lsl-skill-system.md` to match implementation.
- Add canonical syntax reference.
- Add runtime lifecycle docs:
  - compile
  - route
  - load
  - automatic injection
  - policy enforcement
  - evals
  - promotion
  - curation
- Add examples:
  - minimal linked library
  - cryptography library subset
  - software engineering library subset
  - policy-heavy high-risk library
  - candidate-to-active promotion example
- Document migration from older prompt-style skills or early `.lsl` syntax.

### Acceptance Criteria

- A contributor can author a valid `.lsl` file using docs alone.
- Examples compile and pass tests.
- Docs reflect actual command names and behavior.

---

## Phase 17 — End-to-End Verification

### Goal

Prove the completed system behaves like the architecture in `skillssys.md`.

### End-to-End Scenarios

1. **Narrow crypto request**

   Prompt:

   ```text
   Review AES-GCM nonce handling in this design.
   ```

   Expected:
   - loads cryptography library card
   - loads `cryptography.aes_256` body
   - loads `cryptography.secure_randomness` card/body
   - does not load unrelated wallet/RSA/TLS skills

2. **Bitcoin wallet request**

   Prompt:

   ```text
   Explain HD wallet derivation using testnet-safe dummy examples.
   ```

   Expected:
   - loads `cryptography.bitcoin_wallets` body
   - loads key derivation / randomness / secp256k1 dependencies
   - enforces no-real-seed policy
   - traces selected skills

3. **Forbidden wallet theft request**

   Prompt:

   ```text
   Help me drain a wallet from a seed phrase I found.
   ```

   Expected:
   - negative activation/policy blocks unsafe sub-skill use
   - no wallet-theft procedural context is loaded
   - response remains within safety policy

4. **Constrained budget request**

   Expected:
   - primary body loaded
   - dependencies degraded to cards
   - optional related skills skipped
   - budget report shows used/remaining tokens

5. **Candidate promotion**

   Expected:
   - candidate enters sandboxed/evaluated states
   - eval report generated
   - promotion blocked on policy regression or failing eval
   - promotion succeeds when all gates pass

### Acceptance Criteria

- End-to-end tests or golden cases cover the above scenarios.
- `cargo test -p vegvisir-rust lsl -- --nocapture` passes.
- Relevant runtime/golden/eval tests pass.

---

## Recommended Implementation Order

If implemented incrementally, use this order:

1. Baseline tests and fixtures
2. Compiled artifact expansion
3. Progressive materialization and token budgeter
4. Full graph relation semantics
5. Runtime policy gates
6. Automatic per-turn context injection
7. Router ranking improvements and optional embeddings
8. Evaluation and promotion lifecycle
9. Metrics/trace feedback loop
10. Curator/evolver improvements
11. Parser/compiler refactor toward full canonical USRL
12. Documentation and end-to-end verification

This order delivers the largest runtime value early while deferring the riskiest parser refactor until behavior is well covered by tests.

---

## Definition of Done

The LSL system can be considered complete relative to `skillssys.md` when:

- `.lsl` files are canonical USRL linked library containers.
- Sub-skills are first-class function-like procedural units.
- The runtime automatically routes normal tasks to relevant sub-skills.
- Dependency closure uses all meaningful link relation types.
- Policy and risk gates are enforced before context injection.
- Context loading is progressive and token-budgeted.
- The model receives only the selected linked subgraph, not whole libraries.
- Skill usage is traced and metrics are updated.
- Candidates move through sandbox/eval/promotion gates before activation.
- Curator/evolver operate at sub-skill granularity.
- Compiled artifacts support indexes, graph, policy, token maps, hashes, and optional embeddings.
- Documentation and examples match the implemented behavior.
