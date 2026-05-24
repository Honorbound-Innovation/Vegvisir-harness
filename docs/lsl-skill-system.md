# Linked Skill Libraries

Vegvisir supports Linked Skill Libraries in `.lsl` files. An `.lsl` file is a structured library of callable sub-skills with routing metadata, dependency links, policy inheritance, eval hooks, and token-budgeted loading.

## Locations

Vegvisir loads `.lsl` files from the same skill roots as markdown and USRL skills:

- `.vegvisir/skills`
- `skills`
- the Vegvisir data-root `skills` directory

Each `subskill` becomes a first-class enabled skill with kind `lsl_subskill`.

## Commands

Use `/skills compile` to compile all `.lsl` files into `.vegvisir/compiled`.

Compiled artifacts include:

- `usrl_ast/*.ast.json`
- `index/libraries.json`
- `index/subskills.json`
- `index/links.json`
- `index/policies.json`
- `index/evals.json`
- `hashes/hashes.json`

Use `/skills status` to check whether compiled artifacts exist and whether they are fresh against current source hashes.

Use `/skills route <query>` to route a request to matching sub-skills.

Use `/skills load [--tokens N] <query-or-subskill>` to materialize the routed sub-skill context plus linked dependencies inside a token budget.

Use `/skills eval [target-or-eval]` to run local eval hooks. Eval hooks check expected text and forbidden text against materialized sub-skill context and produce a weighted score.

Use `/skills forge <library.subskill> | <title> | <summary> | <body> [| tags=a,b]` to create a new candidate sub-skill. The Skill Forge writes or updates `skills/<library>.lsl`, adds a candidate sub-skill, adds an index entry, attaches provenance, creates a basic eval hook, validates the result, and recompiles.

Use `/skills promote <library.subskill>` to promote a candidate to active. Promotion is blocked unless the sub-skill has eval hooks and all matching eval hooks pass.

Use `/skills archive <library.subskill>` to archive a sub-skill without deleting its source.

Use `/skills patch <library.subskill> | <operation> | <path> | <value>` for structured mutation. Supported operations include replacing `summary` or `load.card/body/extended`, and appending list items to `verification`, `failure_modes`, `activation.positive`, `activation.negative`, or `tags`.

Use `/skills trace` to inspect recent skill route/load traces.

Use `/skills detect` to detect repeated no-match traces that may deserve new skills.

Use `/skills curate` to report candidate, stale, archived, duplicate-summary, failing-eval, least-used, and missing-skill signals.

## Validation

The compiler validates:

- duplicate sub-skill IDs
- required titles, summaries, and load blocks
- status, risk, type, relation, and load-hint enums
- link source and target references
- eval target and eval references
- policy inheritance references
- policy weakening attempts where a child explicitly allows a forbidden parent capability
- eval scoring weights when present

## Skill Forge

The Skill Forge is the controlled creation path for new skills. It implements the growth loop described by the LSL design:

1. A reusable pattern is identified by a user or agent.
2. Vegvisir writes a candidate sub-skill in USRL-style `.lsl` syntax.
3. The candidate receives provenance in its metrics block.
4. The candidate receives at least one eval hook.
5. The library is parsed, validated, hashed, and compiled.
6. Promotion requires passing eval hooks.

The runtime records route/load traces into `.vegvisir/compiled/skill_traces.json`. The Curator and missing-skill detector use those traces to identify low-use skills and repeated no-match requests.

New skills are not trusted immediately. They start with `status: candidate` and must be promoted to `active`.

## Example

See `vegvisir/src/defaults/example_cryptography.lsl` for a complete library with:

- library metadata
- load policy
- inherited policy
- indexed sub-skills
- activation rules
- signatures
- dependency links
- eval hooks with scoring
