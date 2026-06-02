# Binary Intelligence Workbench — Full Suite Implementation Plan

## 1. Executive Summary

The **Binary Intelligence Workbench** is a Vegvisir-native reverse engineering, binary triage, and investigation platform that integrates the current tool ecosystem into one coherent suite:

- **Ghidra / Ghidra Headless MCP** for static binary analysis, decompilation, disassembly, xrefs, strings, symbols, imports, exports, call graphs, and structured extraction.
- **Solarium** for the human-facing investigative workbench UI.
- **Skiller** for reusable binary-analysis workflows, routing, validation, and procedural intelligence.
- **CMS-v2 / ECM** for durable project memory, case files, investigation continuity, and context exposure.
- **Vegvisir agents** for triage, function explanation, vulnerability analysis, firmware analysis, report generation, and workflow orchestration.
- **HBSE** for credential-safe integrations where external services, sandboxes, or authenticated feeds are eventually added.

The flagship user experience should be:

> A user drops in a binary or firmware image, Vegvisir analyzes it, builds a structured case file, surfaces findings in Solarium, routes deeper questions through Skiller workflows, coordinates specialized agents, and remembers the investigation over time.

The suite should begin as a CLI-first core pipeline and expand into a full visual, memory-backed, agentic reverse engineering environment.

---

## 2. Product Vision

### 2.1 Core Goal

Build a complete binary intelligence environment where static analysis artifacts, AI-assisted reasoning, repeatable skills, and persistent investigative memory are unified.

### 2.2 Target Outcomes

The system should be able to:

1. Ingest binaries, libraries, object files, archives, and firmware images.
2. Run deterministic Ghidra-backed extraction.
3. Normalize analysis artifacts into stable JSON schemas.
4. Produce Markdown and HTML/Solarium reports.
5. Identify suspicious capabilities and behaviors.
6. Explain functions, call paths, symbols, strings, and decompiled code.
7. Route specialized tasks through Skiller skills.
8. Store case-level memory without leaking secrets.
9. Support long-running investigations with notes, findings, evidence, and follow-up tasks.
10. Coordinate agents for deeper analysis.
11. Provide a polished Solarium workbench for exploration.
12. Eventually support firmware unpacking, binary diffing, patch analysis, vulnerability hunting, and controlled dynamic-analysis hooks.

---

## 3. Scope

### 3.1 In Scope

- Ghidra headless orchestration.
- Structured extraction from binaries.
- CLI commands for analysis and report generation.
- Case-file storage model.
- Solarium UI panels.
- Skiller bundle for binary analysis workflows.
- CMS-v2 memory integration.
- Agent workflows for analysis and reporting.
- Firmware analysis pipeline.
- Binary comparison / diff workflow.
- Defensive malware triage in controlled, user-authorized contexts.
- Documentation, tests, sample binaries, and developer ergonomics.

### 3.2 Out of Scope for Initial MVP

- Live detonation of malware on the host system.
- Unauthorized third-party target analysis.
- Exploit deployment or weaponization.
- Stealth, persistence, credential theft, or evasion tooling.
- Cloud service integrations requiring secrets unless routed through HBSE.
- Full replacement of Ghidra UI.

### 3.3 Security Boundary

This project is for **authorized defensive analysis, research, and engineering**. Any dynamic execution should eventually run only in explicit sandboxed environments with clear user approval. Credentials and API tokens must be referenced via HBSE secret refs, never plaintext in plans, logs, reports, or memory.

---

## 4. Proposed Repository Layout

```text
binary-intelligence-workbench/
  README.md
  IMPLEMENTATION_PLAN.md
  SECURITY.md
  ROADMAP.md
  docs/
    architecture.md
    case-file-schema.md
    ghidra-integration.md
    skiller-workflows.md
    solarium-ui.md
    cms-memory-model.md
    agent-workflows.md
    firmware-analysis.md
    binary-diffing.md
    threat-model.md
    developer-guide.md
  crates/
    biw-core/
      src/
        lib.rs
        models.rs
        casefile.rs
        errors.rs
        paths.rs
        artifact_store.rs
        hashing.rs
    biw-ghidra/
      src/
        lib.rs
        headless.rs
        scripts.rs
        parser.rs
        job.rs
        capabilities.rs
    biw-cli/
      src/
        main.rs
        commands/
          mod.rs
          triage.rs
          report.rs
          explain.rs
          diff.rs
          firmware.rs
          case.rs
          skill.rs
  scripts/
    ghidra/
      VegvisirExtractAll.java
      VegvisirExtractMetadata.java
      VegvisirExtractFunctions.java
      VegvisirExtractStrings.java
      VegvisirExtractImports.java
      VegvisirExtractExports.java
      VegvisirExtractXrefs.java
      VegvisirExtractCallGraph.java
      VegvisirDecompileSelected.java
      VegvisirFunctionSlice.java
  skiller-bundles/
    binary-analysis/
      bundle.toml
      skills/
        triage_unknown_binary.md
        explain_function.md
        suspicious_strings.md
        identify_crypto_usage.md
        identify_network_behavior.md
        identify_persistence_behavior.md
        identify_file_io_behavior.md
        vulnerability_hunt.md
        firmware_triage.md
        binary_diff_review.md
        generate_re_report.md
      evals/
        routing_cases.yaml
        output_contracts.yaml
  solarium/
    binary-workbench/
      package.json
      src/
        BinaryWorkbench.tsx
        api.ts
        types.ts
        panels/
          CaseOverviewPanel.tsx
          BinaryMetadataPanel.tsx
          FunctionExplorerPanel.tsx
          DecompiledFunctionPanel.tsx
          StringsPanel.tsx
          ImportsExportsPanel.tsx
          CallGraphPanel.tsx
          FindingsPanel.tsx
          NotesPanel.tsx
          SkillsPanel.tsx
          AgentTasksPanel.tsx
          MemoryPanel.tsx
  agents/
    binary-triage-agent.md
    function-analysis-agent.md
    vuln-hunting-agent.md
    firmware-analysis-agent.md
    report-writer-agent.md
  examples/
    sample-case/
      README.md
  tests/
    fixtures/
    golden/
    integration/
  .analysis/
    .gitkeep
```

Notes:

- The actual implementation may live inside the main Vegvisir repository if that is architecturally preferable.
- This folder is the planning seed and can later become a project root, crate workspace, or module subtree.

---

## 5. System Architecture

### 5.1 High-Level Architecture

```text
              ┌────────────────────┐
              │      Solarium       │
              │  Workbench UI       │
              └─────────┬──────────┘
                        │
                        ▼
┌──────────────────────────────────────────────┐
│              Vegvisir BIW API                │
│ CLI commands, local API, job orchestration   │
└───────┬──────────────┬──────────────┬────────┘
        │              │              │
        ▼              ▼              ▼
┌────────────┐  ┌─────────────┐  ┌─────────────┐
│  Ghidra    │  │   Skiller   │  │ CMS-v2/ECM  │
│ Extraction │  │ Workflows   │  │ Case Memory │
└─────┬──────┘  └──────┬──────┘  └──────┬──────┘
      │                │                │
      ▼                ▼                ▼
┌──────────────────────────────────────────────┐
│              Case Artifact Store             │
│ JSON artifacts, reports, notes, findings     │
└──────────────────────────────────────────────┘
```

### 5.2 Main Components

#### BIW Core

Responsibilities:

- Case-file model.
- Artifact schemas.
- Stable paths and naming.
- Hashing and identity.
- Finding model.
- Report model.
- Error handling.

#### BIW Ghidra

Responsibilities:

- Locate/configure Ghidra headless or MCP bridge.
- Run extraction scripts.
- Manage temporary Ghidra projects.
- Parse generated JSON.
- Normalize Ghidra output into BIW schemas.
- Handle large outputs and partial failures.

#### BIW CLI

Responsibilities:

- User-facing command entrypoint.
- MVP orchestration.
- Report generation.
- Case inspection.
- Skill invocation.
- Agent workflow triggers.

#### Solarium Workbench

Responsibilities:

- Case browser.
- Artifact viewers.
- Function explorer.
- Call graph display.
- Finding review.
- Notes and memory panel.
- Skill execution UI.
- Agent task status.

#### Skiller Binary Analysis Bundle

Responsibilities:

- Define reusable workflows.
- Route binary-analysis questions.
- Enforce output contracts.
- Provide prompt/context templates.
- Support evaluation cases.

#### CMS-v2 / ECM Integration

Responsibilities:

- Store durable case summaries.
- Store non-secret investigation decisions.
- Recall prior related findings.
- Expose bounded context to models.
- Avoid memory pollution and secret leakage.

---

## 6. Data Model

### 6.1 Case File

Each analyzed artifact should create a stable case directory:

```text
.analysis/cases/<case_id>/
  case.json
  source/
    original-ref.json
  artifacts/
    metadata.json
    hashes.json
    strings.json
    imports.json
    exports.json
    functions.json
    callgraph.json
    xrefs.json
    segments.json
    decompile/
      <function_id>.json
  findings/
    findings.json
    suspicious-strings.json
    capability-map.json
  reports/
    summary.md
    full-report.md
  notes/
    notes.md
  memory/
    cms-links.json
  jobs/
    ghidra-triage.json
    skill-runs.json
    agent-runs.json
```

### 6.2 `case.json`

```json
{
  "schema_version": "biw.case.v1",
  "case_id": "sha256-prefix-or-ulid",
  "title": "sample.bin",
  "created_at": "2026-05-31T00:00:00Z",
  "updated_at": "2026-05-31T00:00:00Z",
  "source": {
    "path": "/workspace/samples/sample.bin",
    "filename": "sample.bin",
    "size_bytes": 123456,
    "sha256": "...",
    "mime": "application/x-executable",
    "format": "ELF",
    "architecture": "x86_64"
  },
  "status": "triaged",
  "tags": ["elf", "linux", "unknown"],
  "risk": {
    "level": "unknown",
    "score": null,
    "rationale": []
  }
}
```

### 6.3 Function Model

```json
{
  "schema_version": "biw.functions.v1",
  "functions": [
    {
      "id": "func_00401000",
      "name": "main",
      "address": "0x00401000",
      "size_bytes": 320,
      "namespace": "global",
      "signature": "int main(int argc, char **argv)",
      "is_external": false,
      "is_thunk": false,
      "callers": [],
      "callees": ["func_00401200"],
      "string_refs": [],
      "import_refs": [],
      "tags": []
    }
  ]
}
```

### 6.4 Finding Model

```json
{
  "schema_version": "biw.findings.v1",
  "findings": [
    {
      "id": "finding_001",
      "title": "Potential network behavior",
      "severity": "medium",
      "confidence": "medium",
      "category": "network",
      "description": "The binary imports connect/send/recv and contains URL-like strings.",
      "evidence": [
        {
          "type": "import",
          "value": "connect",
          "artifact": "imports.json"
        }
      ],
      "recommended_next_steps": [
        "Inspect functions referencing connect/send/recv."
      ],
      "created_by": "binary.triage.unknown",
      "created_at": "2026-05-31T00:00:00Z"
    }
  ]
}
```

---

## 7. CLI Design

### 7.1 Initial Commands

```bash
biw triage <binary-path>
biw case list
biw case show <case-id>
biw report <case-id> --format markdown
biw explain function <case-id> <function-id>
biw skill run <case-id> binary.triage.unknown
biw diff <old-binary> <new-binary>
biw firmware triage <firmware-path>
```

If integrated directly into Vegvisir CLI:

```bash
vegvisir binary triage <binary-path>
vegvisir binary case list
vegvisir binary case show <case-id>
vegvisir binary report <case-id>
vegvisir binary explain function <case-id> <function-id>
vegvisir binary skill run <case-id> binary.triage.unknown
vegvisir binary diff <old-binary> <new-binary>
vegvisir firmware triage <firmware-path>
```

### 7.2 MVP Command

```bash
vegvisir binary triage ./samples/sample.bin \
  --out .analysis/cases \
  --decompile-top 20 \
  --summary
```

Expected output:

```text
Created case: .analysis/cases/sample-bin-3f42a91c
Extracted: metadata, hashes, segments, strings, imports, exports, functions, callgraph
Generated: findings/suspicious-strings.json, reports/summary.md
Next: vegvisir binary case show sample-bin-3f42a91c
```

---

## 8. Ghidra Integration Plan

### 8.1 Use Existing Assets

The workspace already contains `GhidraHeadlessMCP` with scripts such as:

- `VegvisirSummary.java`
- `VegvisirReport.java`
- `VegvisirListFunctions.java`
- `VegvisirListStrings.java`
- `VegvisirListImports.java`
- `VegvisirListExports.java`
- `VegvisirCallGraph.java`
- `VegvisirDecompile.java`
- `VegvisirXrefs.java`
- `VegvisirFunctionInfo.java`

The first implementation should reuse these before writing new scripts.

### 8.2 Ghidra Execution Modes

Support two modes:

1. **Headless local mode**
   - Directly invoke `analyzeHeadless` or `GhidraHeadlessMCP/bin/ghidra-headless`.
   - Best for deterministic CLI automation.

2. **MCP bridge mode**
   - Use the Ghidra MCP bridge for interactive function-level operations.
   - Best for live Solarium and agent workflows.

### 8.3 Extraction Stages

1. Identify binary and compute hashes.
2. Create temporary or persistent Ghidra project.
3. Import binary.
4. Run auto-analysis.
5. Extract metadata.
6. Extract segments.
7. Extract strings.
8. Extract imports and exports.
9. Extract functions.
10. Extract call graph.
11. Extract xrefs for suspicious symbols/strings.
12. Decompile selected functions.
13. Normalize all artifacts.
14. Generate findings and report.

### 8.4 Decompilation Strategy

Do not decompile every function by default for large binaries. Use prioritization:

1. Entry point.
2. `main` / startup-related functions.
3. Functions referencing suspicious strings.
4. Functions referencing network, crypto, process, registry, file, or memory APIs.
5. High fan-in/fan-out functions.
6. User-selected functions.

---

## 9. Heuristics and Capability Detection

### 9.1 Suspicious String Categories

- URLs and domains.
- IP addresses.
- File paths.
- Shell commands.
- Registry paths.
- User-agent strings.
- Encoded blobs.
- Base64-like strings.
- PowerShell / cmd / bash references.
- Persistence-related terms.
- Credential-related keywords.
- Crypto constants and algorithm names.
- Debug or anti-debug strings.

### 9.2 Import/API Categories

#### Network

- `socket`, `connect`, `send`, `recv`, `bind`, `listen`, `accept`
- `WinHttp*`, `WinInet*`, `InternetOpen`, `URLDownloadToFile`

#### Crypto

- OpenSSL symbols.
- Windows CryptoAPI / CNG.
- Libsodium.
- Common hash functions.

#### Process / Execution

- `CreateProcess`, `ShellExecute`, `system`, `popen`, `execve`, `fork`

#### Filesystem

- `CreateFile`, `ReadFile`, `WriteFile`, `DeleteFile`
- `open`, `read`, `write`, `unlink`

#### Memory / Injection Indicators

- `VirtualAlloc`, `VirtualProtect`, `WriteProcessMemory`, `CreateRemoteThread`
- `mmap`, `mprotect`, `ptrace`

#### Persistence

- Registry run keys.
- Startup folders.
- Service creation APIs.
- Cron/systemd artifacts.

### 9.3 Scoring Model

Initial scoring should be transparent and heuristic-based:

```text
risk_score = weighted_sum(capabilities, suspicious_strings, sensitive_imports, obfuscation_indicators)
```

Risk levels:

- `unknown`: insufficient evidence.
- `low`: ordinary utility/library behavior.
- `medium`: sensitive capabilities present but unclear intent.
- `high`: multiple suspicious indicators with coherent behavior.
- `critical`: confirmed dangerous behavior in authorized defensive context.

All risk labels must include evidence and confidence.

---

## 10. Skiller Bundle Plan

### 10.1 Bundle Name

`binary-analysis`

### 10.2 Initial Skills

#### `binary.triage.unknown`

Purpose:

- Analyze all extracted artifacts.
- Produce likely purpose, capability map, risk summary, suspicious evidence, and next steps.

Inputs:

- `case.json`
- `metadata.json`
- `strings.json`
- `imports.json`
- `exports.json`
- `functions.json`
- `callgraph.json`
- optional decompiled functions

Outputs:

- `findings.json`
- `summary.md`
- recommended follow-up tasks

#### `binary.explain_function`

Purpose:

- Explain a function using decompiled code, assembly metadata, callers, callees, strings, and imports.

Outputs:

- Plain-English explanation.
- Inputs/outputs and side effects.
- Security-relevant behavior.
- Questions and recommended next functions.

#### `binary.find_suspicious_strings`

Purpose:

- Classify strings and correlate them with xrefs/functions.

#### `binary.identify_crypto_usage`

Purpose:

- Identify crypto APIs, constants, algorithms, key-handling clues, and misuse indicators.

#### `binary.identify_network_behavior`

Purpose:

- Identify network stack usage, endpoints, protocols, request formats, and C2-like patterns.

#### `binary.identify_persistence_behavior`

Purpose:

- Identify persistence mechanisms in Windows/Linux/macOS binaries.

#### `binary.identify_file_io_behavior`

Purpose:

- Explain file read/write/delete behavior and sensitive path usage.

#### `binary.vulnerability_hunt`

Purpose:

- Hunt for memory safety, command injection, path traversal, unsafe parsing, integer overflow, and auth bypass patterns.

#### `binary.firmware_triage`

Purpose:

- Analyze unpacked firmware contents, embedded binaries, scripts, configs, and hardcoded indicators.

#### `binary.binary_diff_review`

Purpose:

- Compare two binaries and explain changed functions, imported capabilities, and patch implications.

#### `binary.generate_re_report`

Purpose:

- Generate a structured reverse-engineering report with evidence, scope, limitations, findings, and next steps.

### 10.3 Skill Quality Requirements

Each skill should define:

- Task purpose.
- Required inputs.
- Optional inputs.
- Output contract.
- Evidence rules.
- Safety boundary.
- Failure modes.
- Example invocation.
- Evaluation cases.

---

## 11. Agent Workflow Plan

### 11.1 Agents

#### Binary Triage Agent

Responsibilities:

- Review extracted artifacts.
- Identify likely purpose.
- Generate capability map.
- Create initial findings.

#### Function Analysis Agent

Responsibilities:

- Explain selected functions.
- Trace callers/callees.
- Identify side effects.
- Recommend follow-up functions.

#### Vulnerability Hunting Agent

Responsibilities:

- Look for risky parsing, memory handling, command execution, unsafe deserialization, and auth logic issues.

#### Firmware Analysis Agent

Responsibilities:

- Coordinate unpacking, file classification, embedded binary triage, config review, and report generation.

#### Report Writer Agent

Responsibilities:

- Convert artifacts and findings into concise or comprehensive reports.

### 11.2 Agent Board Integration

Longer tasks should be visible as agent tasks:

```text
case sample-bin-3f42a91c
  task 1: static extraction complete
  task 2: binary triage agent running
  task 3: function analysis queued for func_00401000
  task 4: report writer waiting on findings review
```

---

## 12. CMS-v2 / ECM Memory Plan

### 12.1 What to Store

Store durable, non-secret memory such as:

- Case summaries.
- Binary hashes and identity.
- User-approved findings.
- Analysis decisions.
- Important function explanations.
- Investigation notes.
- Relationships between cases.
- Follow-up tasks.

### 12.2 What Not to Store

Do not store:

- Plaintext credentials.
- Private keys.
- Tokens.
- Secret-bearing URLs.
- Sensitive customer data unless explicitly authorized and sanitized.
- Full large decompilation dumps by default.

### 12.3 Memory Record Examples

```text
Title: BIW case sample-bin-3f42a91c summary
Type: project
Content: Binary sample.bin sha256 ... was triaged on ... Key findings: network imports present; suspicious URL-like string; next step inspect func_00402010.
```

### 12.4 ECM Exposure Strategy

When answering questions, expose only bounded relevant artifacts:

- Case summary.
- Selected function metadata.
- Selected decompiled function.
- Relevant strings/imports/xrefs.
- Prior findings.

Avoid dumping entire artifact sets into model context.

---

## 13. Solarium Workbench Plan

### 13.1 UI Principles

- Evidence-first.
- Fast filtering and navigation.
- Case-oriented.
- Every AI summary should link back to artifacts.
- Skills and agents should be visible, not magic.
- Findings should be reviewable and editable.

### 13.2 Main Views

#### Case Browser

- List analyzed binaries.
- Show status, risk, tags, timestamps, hash prefix.
- Search/filter by tag, architecture, finding, hash.

#### Case Overview

- Summary.
- Risk level and rationale.
- Capabilities detected.
- Top suspicious strings.
- Top functions to inspect.
- Recent notes and memory.

#### Metadata Panel

- File type.
- Architecture.
- Compiler hints.
- Hashes.
- Sections/segments.
- Entry points.

#### Function Explorer

- Function table.
- Search by name/address/tag/import/string ref.
- Sort by size, callers, callees, suspicious score.
- Select function to view details.

#### Decompiled Function Panel

- Decompiled code.
- Assembly/disassembly toggle.
- Callers/callees.
- String refs.
- Import refs.
- Explain button.
- Add note button.
- Save finding button.

#### Strings Panel

- Searchable strings table.
- Classification labels.
- Xref links.
- Suspicious filter.

#### Imports/Exports Panel

- Group imports by capability.
- Link imports to referencing functions.

#### Call Graph Panel

- Graph view for selected function or whole binary subset.
- Expand callers/callees.
- Highlight suspicious nodes.

#### Findings Panel

- Finding list.
- Severity/confidence/category filters.
- Evidence links.
- Mark reviewed/accepted/rejected.
- Export to report.

#### Skills Panel

- Available binary-analysis skills.
- Required inputs/checklist.
- Run skill on case/function/selection.
- Show skill output and artifact writes.

#### Agent Tasks Panel

- Show running/completed agent tasks.
- Logs, status, outputs.
- Retry or continue.

#### Memory Panel

- Show CMS-linked memories.
- Save summary/finding/note to memory.
- Recall related prior cases.

---

## 14. Firmware Analysis Extension

### 14.1 Goals

Support firmware images as first-class case inputs.

### 14.2 Pipeline

1. Identify firmware format.
2. Extract filesystem using allowed tools where available.
3. Build file inventory.
4. Detect architecture and executable files.
5. Run binary triage on selected embedded binaries.
6. Scan configs/scripts for endpoints, credentials patterns, startup services, update logic.
7. Build firmware-level findings.
8. Generate firmware report.

### 14.3 Artifact Layout

```text
.analysis/cases/<firmware-case-id>/
  firmware/
    inventory.json
    filesystems.json
    extracted-root/
    embedded-binaries.json
    scripts.json
    configs.json
  child-cases/
    <binary-case-id>.json
  reports/
    firmware-report.md
```

### 14.4 Safety Notes

- Secret-like values discovered in firmware should be redacted in reports by default.
- User can inspect raw artifacts locally, but memory writes should store redacted summaries.

---

## 15. Binary Diffing Extension

### 15.1 Goals

Support patch diffing, regression analysis, and vulnerability remediation review.

### 15.2 Inputs

- Old binary.
- New binary.
- Optional symbols/debug info.
- Optional source commit notes.

### 15.3 Outputs

- Changed functions.
- Added/removed imports.
- Added/removed strings.
- Function similarity scores.
- New or removed capabilities.
- Patch-risk summary.
- Candidate vulnerability fix explanation.

### 15.4 MVP Approach

Start simple:

- Compare hashes, metadata, imports, exports, strings.
- Match functions by name/address where symbols exist.
- Compare decompiled text for matched functions.
- Later add fuzzy matching and graph similarity.

---

## 16. Reporting Plan

### 16.1 Report Types

#### Triage Summary

Short initial report:

- Binary identity.
- Analysis scope.
- Key metadata.
- Capabilities.
- Top findings.
- Recommended next steps.

#### Full Reverse Engineering Report

Comprehensive report:

- Executive summary.
- Methodology.
- File metadata.
- Static analysis findings.
- Function analysis.
- Capability analysis.
- Indicators.
- Vulnerability observations.
- Limitations.
- Evidence appendix.

#### Firmware Report

- Firmware identity.
- Extraction summary.
- Filesystem inventory.
- Embedded services.
- Embedded binaries.
- Config/script findings.
- Credential-pattern redaction summary.
- Update/security concerns.

#### Diff Report

- Compared binaries.
- Changed capabilities.
- Changed functions.
- Potential patch intent.
- Security implications.

### 16.2 Evidence Rules

Every nontrivial claim should reference at least one artifact:

- String value and offset.
- Import and referencing function.
- Function address/name.
- Decompiled snippet reference.
- Config file path.
- Case artifact path.

---

## 17. Testing Strategy

### 17.1 Unit Tests

- Case ID generation.
- Hashing.
- JSON schema serialization/deserialization.
- Finding scoring.
- Path handling.
- String classification.
- Import capability mapping.

### 17.2 Integration Tests

- Run triage on tiny known ELF/PE/Mach-O fixtures.
- Validate generated artifacts exist.
- Validate JSON schemas.
- Validate report generation.
- Validate skill routing for sample questions.

### 17.3 Golden Tests

Maintain golden expected outputs for stable sample binaries:

```text
tests/golden/tiny-elf/
  metadata.json
  imports.json
  functions.json
  summary.md
```

### 17.4 Solarium Tests

- Component render tests.
- Mock API responses.
- Case browser filtering.
- Function table sorting.
- Findings review workflow.

### 17.5 Skiller Evals

- Routing evals.
- Output contract evals.
- Safety-boundary evals.
- Regression cases for each skill.

---

## 18. Implementation Phases

## Phase 0 — Project Foundation

### Goals

- Create planning directory and implementation plan.
- Decide repo/module placement.
- Inventory existing Ghidra scripts and MCP capabilities.

### Deliverables

- `binary-intelligence-workbench/IMPLEMENTATION_PLAN.md`
- Initial `README.md`
- Architecture decision record for repo placement.

### Acceptance Criteria

- Project folder exists.
- Plan is reviewed.
- First implementation target is selected.

---

## Phase 1 — CLI-First Ghidra Triage MVP

### Goals

Build the core deterministic pipeline before UI or agents.

### Deliverables

- `vegvisir binary triage <path>` or standalone `biw triage <path>`.
- Case directory creation.
- Hash extraction.
- Ghidra execution wrapper.
- JSON artifacts:
  - `case.json`
  - `metadata.json`
  - `strings.json`
  - `imports.json`
  - `exports.json`
  - `functions.json`
  - `callgraph.json`
- Initial `summary.md` report.

### Tasks

1. Implement core case model.
2. Implement artifact store.
3. Implement Ghidra headless runner.
4. Wire existing Ghidra scripts.
5. Normalize script outputs.
6. Add string/import heuristics.
7. Generate summary report.
8. Add tiny fixture binary.
9. Add integration test.

### Acceptance Criteria

- User can run one command on a sample binary.
- A complete case directory is produced.
- Artifacts are valid JSON.
- Summary report contains file identity, metadata, top strings/imports/functions, and next steps.

---

## Phase 2 — Findings and Capability Mapping

### Goals

Transform raw extraction into useful triage conclusions.

### Deliverables

- `findings.json`
- `capability-map.json`
- Suspicious string classification.
- Import capability classification.
- Risk scoring v1.
- Report evidence links.

### Tasks

1. Implement string classifier.
2. Implement import/API classifier.
3. Add xref correlation for suspicious strings/imports.
4. Add finding generation.
5. Add severity/confidence model.
6. Update summary report.
7. Add tests for classifiers and findings.

### Acceptance Criteria

- System identifies network, crypto, process, filesystem, and persistence indicators.
- Findings include evidence references.
- Risk score is explainable and not opaque.

---

## Phase 3 — Function Explanation Workflow

### Goals

Allow targeted function-level analysis.

### Deliverables

- `vegvisir binary explain function <case-id> <function-id>`.
- Decompile selected function on demand.
- Function context package builder.
- Markdown explanation output.
- Optional finding creation from explanation.

### Tasks

1. Implement function lookup.
2. Implement selected decompile call.
3. Build function context with callers/callees, strings, imports, xrefs.
4. Add explanation prompt/workflow.
5. Persist explanation artifact.
6. Add UI-ready API response shape.

### Acceptance Criteria

- User can ask for a function explanation.
- Output references decompiled code, imports, strings, and graph context.
- Explanation can be saved to notes/findings.

---

## Phase 4 — Skiller Binary Analysis Bundle

### Goals

Move repeatable analysis reasoning into validated Skiller skills.

### Deliverables

- `skiller-bundles/binary-analysis` bundle.
- Initial 8–10 skills.
- Routing evals.
- Output contracts.
- Bundle validation passing.

### Tasks

1. Draft skill cards.
2. Define input/output contracts.
3. Compile bundle if using Skiller Forge flow.
4. Add eval cases.
5. Validate bundle.
6. Wire CLI skill invocation.
7. Use skills for triage summary and function explanation.

### Acceptance Criteria

- Skiller routes binary-analysis queries to appropriate skills.
- Skills produce structured, evidence-based outputs.
- Bundle validation/evals pass.

---

## Phase 5 — CMS-v2 Case Memory

### Goals

Make investigations durable and recallable.

### Deliverables

- Memory write workflow for case summaries.
- Memory write workflow for approved findings.
- Memory links in case directory.
- Recall related cases by hash, name, capability, architecture, or finding.

### Tasks

1. Define memory policy.
2. Add explicit save-to-memory operations.
3. Add redaction checks.
4. Store CMS memory IDs in `memory/cms-links.json`.
5. Add recall command.
6. Add Solarium memory panel backend.

### Acceptance Criteria

- Case summaries can be saved to CMS.
- Secret-like values are redacted or blocked.
- Related cases can be recalled without flooding context.

---

## Phase 6 — Solarium Workbench MVP

### Goals

Create the first usable visual workbench.

### Deliverables

- Case browser.
- Case overview panel.
- Metadata panel.
- Strings panel.
- Imports/exports panel.
- Function explorer.
- Findings panel.

### Tasks

1. Define local API between Vegvisir and Solarium.
2. Implement TypeScript types matching BIW schemas.
3. Build case list and case detail views.
4. Implement tables with search/filter/sort.
5. Add report preview.
6. Add finding review controls.
7. Add basic tests.

### Acceptance Criteria

- User can open a case visually.
- User can inspect metadata, strings, imports, functions, and findings.
- UI links evidence to artifacts.

---

## Phase 7 — Solarium Deep Analysis UX

### Goals

Make the UI a true investigative environment.

### Deliverables

- Decompiled function panel.
- Call graph panel.
- Skill execution panel.
- Agent tasks panel.
- Notes panel.
- Memory panel.

### Tasks

1. Implement on-demand function decompile API.
2. Add function explanation action.
3. Add graph visualization.
4. Add skill run forms.
5. Add agent task status integration.
6. Add note-taking and finding creation.
7. Add CMS save/recall controls.

### Acceptance Criteria

- User can select a function, decompile it, explain it, annotate it, and save findings.
- User can run a Skiller workflow from the UI.
- Agent task status is visible.

---

## Phase 8 — Agentic Workflows

### Goals

Enable long-running, specialized analysis tasks.

### Deliverables

- Binary triage agent profile.
- Function analysis agent profile.
- Vulnerability hunting agent profile.
- Report writer agent profile.
- Task board integration.

### Tasks

1. Define agent prompts and boundaries.
2. Create structured inputs for agents.
3. Add task spawning commands.
4. Persist agent outputs.
5. Add report synthesis workflow.
6. Add cancellation/retry behavior.

### Acceptance Criteria

- User can spawn bounded analysis tasks.
- Outputs are written to case artifacts.
- Findings are evidence-backed and reviewable.

---

## Phase 9 — Firmware Analysis

### Goals

Support firmware image triage.

### Deliverables

- Firmware case model.
- Extraction inventory.
- Embedded binary detection.
- Child binary cases.
- Firmware report.

### Tasks

1. Add firmware identification.
2. Integrate extraction tools where available.
3. Inventory files.
4. Detect executable candidates.
5. Run binary triage on selected binaries.
6. Scan configs/scripts.
7. Generate firmware-level findings.
8. Redact secret-like values in memory/report summaries.

### Acceptance Criteria

- User can triage a firmware image into an inventory and report.
- Embedded binaries can become child cases.
- Sensitive values are handled carefully.

---

## Phase 10 — Binary Diffing and Patch Analysis

### Goals

Support binary comparison workflows.

### Deliverables

- Diff command.
- Diff case model.
- Changed import/string/function reports.
- Patch analysis Skiller workflow.

### Tasks

1. Analyze old and new binaries.
2. Compare metadata/imports/exports/strings.
3. Match functions by symbols/address.
4. Compare decompiled output for matched functions.
5. Generate diff findings.
6. Add Solarium diff view.

### Acceptance Criteria

- User can compare two binaries.
- Report identifies meaningful changed capabilities and functions.
- Patch implications are summarized with evidence.

---

## Phase 11 — Hardening, Performance, and Scale

### Goals

Make the suite reliable on large binaries and real projects.

### Deliverables

- Job queue or bounded task runner.
- Artifact size budgeting.
- Caching.
- Partial-failure recovery.
- Progress reporting.
- Better schema migrations.

### Tasks

1. Add job state machine.
2. Add progress events.
3. Stream large Ghidra outputs.
4. Cap default decompile counts.
5. Add artifact compression where useful.
6. Add schema version migration utilities.
7. Add performance benchmarks.

### Acceptance Criteria

- Large binaries do not lock up the system silently.
- Failed stages can be retried.
- UI and CLI show progress and partial results.

---

## Phase 12 — Polish and Documentation

### Goals

Make the suite usable by others and maintainable by us.

### Deliverables

- Full docs.
- Example walkthroughs.
- Troubleshooting guide.
- Developer guide.
- Security guide.
- Demo cases.

### Tasks

1. Write quickstart.
2. Write architecture docs.
3. Write Ghidra setup guide.
4. Write Solarium usage guide.
5. Write Skiller authoring guide for binary workflows.
6. Add demo scripts.
7. Add release checklist.

### Acceptance Criteria

- A new developer can run the MVP from docs.
- A user can analyze a sample binary from docs.
- Known limitations are explicit.

---

## 19. Milestone Summary

| Milestone | Name | Primary Output |
|---|---|---|
| M0 | Foundation | Plan and project folder |
| M1 | CLI Triage MVP | Analyze binary into JSON + summary |
| M2 | Findings | Capability map + findings |
| M3 | Function Explain | On-demand decompile/explain |
| M4 | Skiller Bundle | Reusable analysis workflows |
| M5 | Memory | CMS-backed case continuity |
| M6 | Solarium MVP | Visual case browser and artifact panels |
| M7 | Deep UI | Function, graph, skills, agents, memory panels |
| M8 | Agents | Long-running analysis workflows |
| M9 | Firmware | Firmware triage pipeline |
| M10 | Diffing | Binary comparison and patch analysis |
| M11 | Hardening | Scale, reliability, performance |
| M12 | Docs/Polish | Usable documented suite |

---

## 20. Recommended First Implementation Sprint

### Sprint Goal

Implement the CLI-first Ghidra triage MVP using existing workspace Ghidra scripts.

### Sprint Tasks

1. Inspect existing `GhidraHeadlessMCP` scripts and output formats.
2. Decide whether the CLI lives as standalone `biw` or under Vegvisir CLI.
3. Implement case directory creation.
4. Implement file hashing and identity.
5. Wire one Ghidra headless invocation.
6. Extract strings/imports/functions/summary.
7. Normalize outputs to BIW schemas.
8. Generate `summary.md`.
9. Test against a tiny sample binary.
10. Document quickstart.

### Sprint Acceptance Criteria

Running:

```bash
vegvisir binary triage ./samples/hello
```

or:

```bash
biw triage ./samples/hello
```

creates:

```text
.analysis/cases/hello-<hash>/
  case.json
  artifacts/
    metadata.json
    strings.json
    imports.json
    exports.json
    functions.json
    callgraph.json
  findings/
    suspicious-strings.json
  reports/
    summary.md
```

---

## 21. Risks and Mitigations

### Risk: Ghidra output inconsistency

Mitigation:

- Normalize through BIW schemas.
- Add schema validation and golden tests.

### Risk: Large binary performance

Mitigation:

- Decompile selectively.
- Stream outputs.
- Add limits and progress events.

### Risk: AI hallucinated findings

Mitigation:

- Require evidence links.
- Separate generated findings from reviewed findings.
- Use structured output contracts.

### Risk: Memory pollution

Mitigation:

- Save only summaries and approved findings.
- Redact secrets.
- Link to local artifacts instead of storing bulk data.

### Risk: Scope creep

Mitigation:

- Build CLI-first MVP.
- Add UI only after artifacts stabilize.
- Add firmware/diffing after core workflows work.

### Risk: Unsafe malware handling

Mitigation:

- Default to static analysis.
- Explicit warnings and sandbox requirement for dynamic analysis.
- No stealth, persistence, credential theft, or unauthorized targeting features.

---

## 22. Open Design Questions

1. Should BIW be a separate repo/module or integrated into the main Vegvisir CLI?
2. Should Solarium communicate with Vegvisir through a local HTTP API, IPC, or direct plugin bridge?
3. What is the canonical case ID format: ULID, hash prefix, or slug plus hash?
4. How much of Ghidra MCP should be reused versus direct headless execution?
5. Should decompiled function artifacts be stored by default or only on demand?
6. What is the best schema validation mechanism in the current stack?
7. Should Skiller skills write artifacts directly or return outputs for Vegvisir to persist?
8. What sample binaries are safe and appropriate for tests?
9. How should agent tasks be represented in Solarium?
10. What redaction policy should apply to secret-like strings in firmware reports?

---

## 23. Definition of Done for the Full Suite

The Binary Intelligence Workbench is considered complete when:

1. A user can analyze a binary from CLI and Solarium.
2. Ghidra extraction produces stable, validated artifacts.
3. Findings are evidence-backed and reviewable.
4. Skiller workflows route and execute common analysis tasks.
5. CMS memory preserves case continuity safely.
6. Function-level decompile/explain workflows are usable.
7. Solarium provides a coherent case investigation UI.
8. Agentic workflows can perform bounded deep analysis tasks.
9. Firmware and binary-diff workflows work at least at MVP level.
10. Reports are exportable and useful.
11. Tests cover schemas, extraction, heuristics, skill routing, and UI basics.
12. Documentation is sufficient for setup, usage, extension, and troubleshooting.

---

## 24. Immediate Next Step

Start Phase 1:

> Build the CLI-first Ghidra triage MVP that creates a case directory, runs existing Ghidra extraction scripts, emits normalized JSON artifacts, and generates a Markdown summary.

This gives the whole suite a solid spine. Solarium, Skiller, memory, and agents should attach to that stable artifact model rather than being built first in isolation.
