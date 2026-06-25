# Rust Module Refactor Skill

Purpose: guide safe refactoring of a Rust module without changing observable behavior.

Procedure:

1. Inspect the current module boundaries and public API.
2. Identify the smallest coherent refactor.
3. Preserve behavior and existing tests unless the user explicitly requests behavior changes.
4. Apply edits in small, reviewable steps.
5. Run formatting and focused tests.
6. Report files changed, commands run, and verification results.

Constraints:

- Do not silently change public behavior.
- Do not remove tests to make the build pass.
- Do not broaden permissions or bypass host policy.
- Prefer reversible, minimal changes.

Verification expectations:

- Rust formatting should pass.
- Relevant tests should pass or failures must be reported clearly.
- The execution report must include changed files and commands run.
