# Agent Context Protocol (ACP)

Vegvisir supports the documentation-first **Agent Context Protocol** convention. ACP is not a replacement for MCP and it is not a JSON-RPC transport: ACP gives an agent a durable project knowledge base through `AGENT.md`, an `agent/` directory, Markdown workflow documents, and `agent/progress.yaml`.

## Commands

```text
/acp help
/acp init [--force]
/acp status
/acp validate
/acp context
/acp list
/acp show-command <name>
/acp run <name> [arguments...]
```

`/acp init` creates the portable directory pattern without overwriting existing project documents. It creates:

```text
AGENT.md
agent/
  commands/
  design/
  specs/
  milestones/
  patterns/
  tasks/
  index/
  artifacts/
  progress.yaml
```

The generated files are intentionally concise. Projects may add the complete upstream ACP package or their own command and artifact documents without changing Vegvisir.

## External ACP command spelling

ACP command documents can also be invoked using the upstream-style spelling:

```text
@acp.status
@acp.validate
@acp.resume
```

Arguments after the invocation are passed to the model along with the matching `agent/commands/<name>.md` document. Vegvisir treats the Markdown document as workspace-authored context. It does not execute Markdown as shell code, and the document cannot override the harness system prompt, user authority, tool approval, sandbox, or HBSE secret boundary.

## Automatic model context

When the active workspace contains `AGENT.md` or `agent/`, Vegvisir automatically adds a bounded ACP context section to model turns. The section includes:

- a bounded excerpt of `AGENT.md`;
- project, milestone, task, blocker, and next-step information parsed from `agent/progress.yaml`;
- a deterministic index of ACP artifact paths and SHA-256 identifiers;
- available command-document paths.

The context is capped before it reaches the provider so a large documentation tree cannot consume the entire model budget. Full documents remain available through normal workspace tools or `/acp show-command`.

## Validation and progress

`/acp validate` requires `AGENT.md`, `agent/`, and `agent/progress.yaml`. Missing artifact subdirectories are reported as warnings so older ACP projects remain usable. Malformed progress YAML and oversized/invalid UTF-8 documents are reported as diagnostics rather than preventing Vegvisir from opening the workspace.

The progress reader accepts the common ACP shape:

- `project.name`, `project.status`, and `project.current_milestone`;
- `milestones` with `status` and optional nested `tasks`;
- top-level `tasks` as a sequence or milestone-keyed mapping;
- `progress.overall`;
- `current_blockers` and `next_steps`.

Vegvisir does not make ACP progress files authoritative over actual tool output or verification. They are durable project context; implementation claims still require normal tests, evidence, and run artifacts.

## Relationship to other Vegvisir systems

- **ACP** — project documentation, planning, workflow directives, and progress.
- **CMS-v2/ECM** — durable memory and active context preparation across sessions.
- **Goal mode** — unbounded implementation of a Markdown specification until its exit criteria are verified.
- **MCP** — external tool/service discovery and invocation.
- **Run artifacts** — provider, tool, context, approval, and verification evidence for a turn.

The automatic ACP context is project-scoped and is not written into global memory. Stable decisions may still be recorded explicitly with the existing memory commands.
