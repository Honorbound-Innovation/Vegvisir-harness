# Vegvisir scripts

This directory contains a growing set of small shell utilities for working with Vegvisir, the workspace, git, CMS-style memory artifacts, HBSE-safe secret boundaries, approvals, runs, and skill bundles.

## Conventions

- All scripts are Bash.
- They use `set -euo pipefail` for safety.
- HBSE-related helpers avoid printing secret values; they focus on names, refs, file paths, metadata, or redacted summaries.
- Most scripts accept `.` or a path argument and are intended to be run from the workspace root.

## Quick start

Examples:

```bash
./scripts/v.sh git-status
./scripts/v.sh repo-map
./scripts/v.sh repo-query dispatcher
./scripts/v.sh repo-symbol vsh
./scripts/v.sh run-latest
./scripts/v.sh skill-route <bundle> <query>
./scripts/v.sh hbse-search hbse
```

## Dispatcher

- `v.sh` — dispatcher for all `v*.sh` scripts

## Categories

### 1) General repo and workspace navigation

- `vrepo-root.sh` — print the git repo root
- `vgit-status.sh` — git status plus diff summary
- `vchanged-files.sh` — show changed files
- `vtracked-files.sh` — list tracked files
- `vuntracked-files.sh` — list untracked files
- `vignored-files.sh` — list ignored files
- `vbranch-info.sh` — current branch and upstream
- `vbranch-drift.sh` — ahead/behind counts
- `vbranches-recent.sh` — branches sorted by recent commit time
- `vremotes.sh` — show git remotes
- `vmerges.sh` — recent merge commits
- `vrepo-stats.sh` — quick repo stats
- `vcommit-info.sh` — latest commit details
- `vdiff-summary.sh` — concise diff summary for a path
- `vtop-files.sh` — largest files in the workspace
- `vfilesize.sh` — readable size summary of files
- `vtree.sh` — compact tree view of the workspace
- `vrecent.sh` — compact recent commit summary
- `vrecent-commits.sh` — recent commits with decoration
- `vsnapshot.sh` — quick workspace snapshot
- `vcontext-pack.sh` — gather files related to a pattern
- `vgrep.sh` — search with context
- `vnotes.sh` — find TODO/FIXME/BUG/HACK notes
- `ventrypoints.sh` — locate likely build/entry files
- `vmarkdown-files.sh` — list markdown files
- `vsource-files.sh` — list source files by common extensions
- `vshell-files.sh` — list shell scripts
- `vtest.sh` — run a sensible test command for the repo type
- `venv-snapshot.sh` — print key environment values
- `vrepo-map.sh` — build a compact repo map, query symbols, diff snapshots, and export indices

### 2) Vegvisir runs and run artifacts

- `vruns.sh` — list `.vegvisir/runs` newest first
- `vrun-latest.sh` — newest run directory
- `vrun-list-head.sh` — top N newest run dirs
- `vrun-latest-summary.sh` — one-line artifact presence summary for newest run
- `vrun-summary.sh` — summarize one run directory
- `vrun-inspect.sh` — show key run artifacts together
- `vrun-artifacts.sh` — list artifact files per run
- `vrun-files.sh` — list run artifact files
- `vrun-kinds.sh` — show artifact types present in each run
- `vrun-errors.sh` — grep errors/failures in run artifacts
- `vrun-timeline.sh` — timeline from a run’s file timestamps
- `vrun-provenance.sh` — provenance hints from run filenames/timestamps
- `vrun-memory.sh` — inspect run memory artifacts
- `vrun-subagents.sh` — inspect run subagent metadata
- `vrun-verification.sh` — inspect verification output
- `vrun-search.sh` — search across run artifacts
- `vrun-failure-cluster.sh` — cluster run failures by error signature

### 3) Subagents, memory, and workspace operations

- `vsubagents.sh` — list subagent tasks
- `vsubagents-summary.sh` — summarize subagent state
- `vsubagent-show.sh` — show one subagent task
- `vsubagent-logs.sh` — list local subagent-related logs
- `vshell-logs.sh` — list shell task logs
- `vshell-tail.sh` — tail the latest shell task log
- `vshell-log-view.sh` — view one shell task log
- `vmemories.sh` — recent CMS memories
- `vmemory-search.sh` — search CMS memories
- `vworkspace-map.sh` — workspace + scripts + run overview
- `vworkspace-meta.sh` — concise workspace metadata block
- `vworkspace-health.sh` — workspace hygiene summary
- `vworkspace-hotspots.sh` — find run/memory/approval hotspots
- `vartifact-sizes.sh` — size summary of Vegvisir artifacts

### 4) CMS-focused helpers

- `vcms-artifacts.sh` — locate CMS-style artifacts
- `vcms-context-size.sh` — estimate CMS artifact footprint
- `vcms-query-summary.sh` — condensed view of CMS-related queries
- `vcms-recent.sh` — recent CMS items with redaction
- `vcms-run.sh` — inspect CMS artifacts in a run dir
- `vcms-search.sh` — search CMS memories with redaction
- `vcms-source-audit.sh` — audit CMS source artifact age/staleness
- `vcms-thread-map.sh` — timestamp-only map of CMS memory threads

### 5) HBSE and secret-safety helpers

- `vhbse-secret-refs.sh` — find secret-ref mentions without expanding values
- `vhbse-env.sh` — list sensitive env var names only, redacted
- `vhbse-env-groups.sh` — grouped sensitive env var names
- `vhbse-files.sh` — list HBSE/secret-related files by name
- `vhbse-manifest-files.sh` — list likely HBSE manifest/ref files
- `vhbse-path-allowlist.sh` — show likely HBSE-safe path areas
- `vhbse-redaction-check.sh` — find redaction-sensitive terms for logs/docs
- `vhbse-ref-count.sh` — count HBSE/secret-ref mentions
- `vhbse-search.sh` — search for HBSE references in the workspace
- `vhbse-secret-scan.sh` — detect likely secret-bearing files by name/pattern
- `vhbse-status-files.sh` — show git-status files likely related to HBSE/secrets
- `vhbse-workspace-meta.sh` — workspace metadata plus HBSE marker count
- `vhbse-changed.sh` — changed files that mention HBSE/approval keywords

### 6) Approvals, policy, and security workflow

- `vapprovals.sh` — inspect run approvals
- `vapprovals-files.sh` — list approval-related files
- `vapprovals-pending.sh` — find pending approval markers
- `vapprovals-stale.sh` — show approval artifacts by age
- `vapprovals-linked-runs.sh` — link runs to approvals files
- `vsecurity-files.sh` — locate approval/auth/credential workflow files

### 7) Skills, bundles, routing, and compatibility

- `vskill-artifacts.sh` — list skill-related artifacts
- `vskill-bundles.sh` — show local skill bundles
- `vskill-change-impact.sh` — list potentially affected skill artifacts
- `vskill-compat.sh` — check bundle/path compatibility basics
- `vskill-deps.sh` — show dependency/requires hints in skills
- `vskill-index.sh` — list compiled skill/index artifacts
- `vskill-list.sh` — surface skills in a bundle
- `vskill-meta.sh` — show skill/bundle metadata safely
- `vskill-route.sh` — route a query against a bundle

## Operational notes

- Prefer the most specific script for the job:
  - `vrun-*` for run-level diagnostics
  - `vcms-*` for memory/context artifacts
  - `vhbse-*` for safe secret-boundary discovery
  - `vapprovals-*` for approvals and policy artifacts
  - `vskill-*` for skill/bundle inspection and routing
  - `vworkspace-*` for general workspace state
- The HBSE helpers are intentionally conservative and should not be used to print or exfiltrate secret values.
- If you add new scripts, keep the naming style consistent: lowercase, `v` prefix, hyphen-separated purpose.

## Maintenance tip

If you want a quick smoke check after editing scripts:

```bash
bash -n scripts/*.sh
```

If you want a functional pass, run the most relevant scripts for the area you changed.
