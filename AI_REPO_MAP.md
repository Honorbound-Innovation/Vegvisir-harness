# AI Repo Map

Purpose: token-friendly repository index for AI agents. Use this before broad searches. It maps major directories, entry points, and important symbols to files.

Repo: `Vegvisir-harness`
Primary product: local-first agent harness with TUI/headless CLI, providers, tools, memory, governed skills, subagents, browser evidence, approvals, and verification.

## Fast Orientation

- `README.md` - product overview, install/use examples, major feature list.
- `Cargo.toml` - root Rust workspace. Members: `vegvisir`, `components/cms-v2`, `components/HBSE/hbse`, `components/skiller`, `components/msp-client`.
- `vegvisir/` - main Rust harness: TUI, CLI, bridge/app server, provider adapters, tools, approvals, memory integration, subagents.
- `components/cms-v2/` - Rust Continuum Memory System v2: scoped memory, retrieval, prompt/context prep, SQLite storage.
- `components/HBSE/hbse/` - Rust Hardware Bound Secrets Enclave: vault, brokers, providers, policy, audit, redaction.
- `components/skiller/` - Rust skill compiler/forge/runtime for source-grounded skill bundles and agent packs.
- `components/msp/` - Rust Model Skill Protocol reference implementation: core types, registry, publisher, CLI, JSON-RPC server.
- `components/msp-client/` - Rust Vegvisir-native MSP client wrapper used by the main harness.
- `components/solarium/` - TypeScript Playwright browser automation/evidence/security runtime.
- `components/usrl/` - TypeScript USRL parser, validator, resolver, runtime, JSRT, PLL/CLL linked contracts.
- `components/desktop/` - Tauri/Vite desktop app shell for Vegvisir.
- `components/ghidra-headless-mcp/` - Python MCP bridge plus Java Ghidra scripts for binary analysis.
- `components/binary-intelligence-workbench/` - Python BIW CLI/server/analysis helpers for binary triage.
- `docs/` - architecture, usage, security, operations, runtime, component docs.
- `companion_scripts/` - shell helpers for repo/workspace/run/memory/HBSE/skill/subagent inspection.
- `scripts/` - install and HBSE/provider onboarding helpers.
- `demos/` - scripted demos and reference host.
- `benchmarks/` - benchmark runner and task specs.

## Skip Or Deprioritize For AI Context

Usually skip these unless the task explicitly targets them:

- `**/node_modules/`, `**/target/`, `**/dist/`, `Cargo.lock`, package lockfiles.
- `.git/`, `.vegvisir/`, `tmp/`, `demos/artifacts/`.
- `components/binary-intelligence-workbench/samples/` and `analysis/` binary/case artifacts.
- `docs/assets/screenshots/` image files.
- `components/msp/examples/conformance/` fixtures unless testing MSP contract output.

## Build And Test Commands

- Rust workspace check: `cargo check --workspace`
- Rust workspace tests: `cargo test --workspace`
- Main harness only: `cargo test -p vegvisir-rust`
- CMS only: `cargo test -p cms-v2`
- HBSE only: `cargo test -p hbse`
- Skiller only: `cargo test -p skiller`
- MSP client only: `cargo test -p msp-client`
- Solarium: `cd components/solarium && npm run build && npm test`
- USRL: `cd components/usrl && npm run build && npm test`
- Desktop web/typecheck: `cd components/desktop && npm run check` or `npm run web:build`
- BIW: `cd components/binary-intelligence-workbench && pytest`
- Repo helper: `companion_scripts/vtest.sh`

## Root Files

- `install.sh`, `upgrade.sh`, `uninstall.sh` - system install lifecycle.
- `scripts/install-system-deps.sh` - native dependency bootstrap.
- `scripts/install-local.sh` - local install helper.
- `scripts/hbse-provider-onboard.sh` - HBSE provider onboarding.
- `components/COMPONENTS.md`, `components/components.toml` - component inventory/config.
- `LICENSE`, `NOTICE`, `THIRD_PARTY_NOTICES.md`, `licenses/` - legal/license notices.

## Main Harness: `vegvisir/`

Entry points:

- `vegvisir/src/main.rs` - CLI parser and dispatch. Symbols: `Cli`, `Command`, `main`, `run_skiller`, `run_msp`, `run_desktop`, desktop binary/source discovery helpers.
- `vegvisir/src/lib.rs` - library module exports; re-exports `Model`, `ScriptedModel`, `AgentHarness`, `AgentResult`, `AgentTask`.
- `vegvisir/src/bin/vegvisir-agent-admin.rs` - separate agent admin binary entry.
- `vegvisir/tests/port_smoke.rs` - port/server smoke test.
- `vegvisir/packaging/` - package/install/uninstall scripts for packaged harness.

Core runtime and model flow:

- `vegvisir/src/orchestrator.rs` - high-level agent loop. Symbols: `AgentTask`, `AgentResult`, `AgentHarness`, `record_plan_evidence`, `complete_plan`.
- `vegvisir/src/model.rs` - model abstraction. Symbols: `Model`, `ScriptedModel`.
- `vegvisir/src/provider.rs` - provider adapter types, auth policy, generated artifacts, tool-round limits. Symbols: `ProviderAdapter`, `ProviderResponse`, `TokenUsage`, `direct_provider_auth_allowed`, `configured_max_tool_rounds`.
- `vegvisir/src/model_discovery.rs` - provider model listing. Symbols: `discover_provider_models`, `discover_openai_compatible_models`, `discover_hbse_openai_compatible_models`, `discover_anthropic_models`, `discover_google_models`, `discover_ollama_models`, `discover_openai_sso_models`.
- `vegvisir/src/openai_sso.rs` - OpenAI SSO auth store/tokens. Symbols: `OpenAISsoTokens`, `OpenAISsoAuthStore`, `login`, `exchange_code`, `refresh`, `load_fresh_tokens`.
- `vegvisir/src/runtime.rs` - runtime plugin/config container. Symbols: `RuntimePlugin`, `RuntimeConfig`, `Runtime`.
- `vegvisir/src/state.rs` - run state/progress. Symbols: `RunState`, `ProgressItem`, `utc_now`.
- `vegvisir/src/types.rs` - generic message/decision/tool observation structs. Symbols: `Role`, `Message`, `AgentDecision`, `Observation`, `ToolCall`.
- `vegvisir/src/core.rs` - shared data model. Symbols: `ToolDefinition`, `SkillDefinition`, `CommandDefinition`, `Attachment`, `ChatMessage`, `AuditEvent`, `ProviderConfig`, `ModelInfo`, `AgentProfile`, `McpServerConfig`.

TUI, desktop bridge, and app server:

- `vegvisir/src/app.rs` - central TUI state model. Symbols: `TuiApplication`, `HeadlessObservedRun`, `StreamEvent`, `DiffOverlay`, render cache and overlay structs.
- `vegvisir/src/app/tui_loop.rs` - terminal lifecycle. Symbols: `run_tui`, `run_tui_with_dangerous_bypass`, `run_pending_editor_action`, `TerminalGuard`.
- `vegvisir/src/tui2.rs` - Ratatui drawing and markdown/code rendering. Symbols: `draw`, `draw_header`, `draw_chat`, `draw_tool_log_panel`, `render_markdown`, `render_code_block`, `next_thinking_trace_expiry_at`.
- `vegvisir/src/app/input.rs` - input editing/key handling for TUI.
- `vegvisir/src/app/shell.rs` - slash command shell routing. Symbols: `agent_selection_prefix`, `command_overlay_title`.
- `vegvisir/src/app/runtime.rs` - app runtime execution helpers: approval IDs, turn repair idle timeout, CMS writeback, command/tool argument conversion.
- `vegvisir/src/app/workspace_state.rs` - workspace session state.
- `vegvisir/src/app/util.rs` - TUI utility formatting/path helpers. Symbols: `workspace_project_id`, `format_subagent_events_body`, `markdown_details`.
- `vegvisir/src/bridge.rs` - JSONL desktop/app bridge request/response structs. Symbols: `BridgeRequest`, `InitializeParams`, `ThreadStartParams`, `TurnParams`, `CommandParams`, provider/session params.
- `vegvisir/src/compat_server.rs` - OpenAI-compatible local server. Symbols: `CompatServerOptions`, `ChatCompletionRequest`, `ResponsesRequest`, `run_openai_compat_server`, `handle_request`.

Slash command handlers:

- `vegvisir/src/app/commands/mod.rs` - command module wiring.
- `commands/agents.rs` - `/agent` and Skiller agent pack registration. Symbols: `agent_templates`, `register_skiller_agent_pack`, `natural_agent_id`.
- `commands/autonomy.rs` - autonomy level commands.
- `commands/autonomy_plan.rs` - autonomy plan compile/status/CLL/PLL. Symbols: `autonomy_plan_status_unchecked`, `evaluate_validation_adapter`, `render_cll`, `render_pll`.
- `commands/mcp_hbse.rs` - HBSE/MCP setup command text. Symbols: `hbse_model_provider_setup`, `hbse_service_setup`, `normalize_hbse_ref_segment`.
- `commands/memory.rs` - memory/context/archive/import commands. Symbols: `ContextUsageReport`, `memory_export_path`, `run_dirs_sorted`.
- `commands/misc.rs` - helper command execution/recovery utilities.
- `commands/permissions.rs` - permissions help.
- `commands/persona.rs` - persona commands.
- `commands/profile.rs` - user profile commands.
- `commands/providers.rs` - provider/model commands and auth hints.
- `commands/runs.rs` - run artifact listing/inspection.
- `commands/sessions.rs` - session load/list helpers.
- `commands/skills.rs` - Skiller/LSL commands.
- `commands/speech.rs` - speech/TTS commands.
- `commands/sudo.rs` - sudo command handling.
- `commands/summary.rs` - conversation/work summary generation.
- `commands/tasks.rs` - background task commands.
- `commands/tools.rs` - tool allow-list/autonomy help.

Tools, policy, approvals, sandboxing:

- `vegvisir/src/tools.rs` - concrete tool helpers for Skiller/MSP paths, forge response parsing, excerpts. Symbols: `parse_skiller_forge_pass`, `parse_skiller_forge_response`, `default_msp_registry_path`, `skiller_bundle_output_path`.
- `vegvisir/src/command_registry.rs` - slash command and tool inventory. Symbols: `CommandRegistry`, `ToolRegistry`, `CommandSpec`, `ToolSpec`, `validate_default_command_definitions`, `default_command_definitions`.
- `vegvisir/src/guardrails.rs` - approval ledger and permission policy. Symbols: `ApprovalRequest`, `ApprovalResolution`, `ApprovalLedger`, `PermissionPolicy`, `GuardrailEngine`, `default_allowed_commands`, `reject_unsafe_sudo_invocation`.
- `vegvisir/src/policy.rs` - runtime gate policy. Symbols: `RuntimePolicy`, `RuntimeGateRequest`, `RuntimeGateDecision`.
- `vegvisir/src/policy_explain.rs` - human-readable policy explanation. Symbols: `PolicyExplanation`, `explain_pending_approval`, `explain_tool_call`.
- `vegvisir/src/control_requests.rs` - pending approval/control request queue. Symbols: `ControlRequest`, `ControlResponse`, `PendingControlRequests`, `ControlResolveOutcome`.
- `vegvisir/src/command_sandbox.rs` - OS command sandbox command building. Symbols: `CommandSandboxMode`, `CommandSandboxConfig`, `build_sandboxed_command`, `network_policy_label`.
- `vegvisir/src/sandbox.rs` - filesystem hardening and openat2 workspace containment. Symbols: `WorkspaceSandbox`, `CommandSandboxStatus`, `workspace_relative_path`.
- `vegvisir/src/privilege.rs` - sudo/HBSE privilege refresh. Symbols: `SudoStatus`, `sudo_status`, `sudo_refresh_via_hbse_broker`, `sudo_refresh_with_tui_password`.

Memory, context, retrieval:

- `vegvisir/src/memory.rs` - Vegvisir CMS integration. Symbols: `VegvisirCms`, `VegvisirCmsConfig`, `ContextPrepareOptions`, `VegvisirMemorySummary`, `ChatGptImportSummary`, `default_vegvisir_data_root`.
- `vegvisir/src/context.rs` - context budget manager. Symbols: `ContextManager`, `ContextBudgetPolicy`, `ContextBudgetDecision`.
- `vegvisir/src/retrieval.rs` - simple in-memory retrieval. Symbols: `RetrievalDocument`, `InMemoryRetriever`.
- `vegvisir/src/checkpoints.rs` - run snapshots. Symbols: `RunSnapshot`, `CheckpointStore`.
- `vegvisir/src/run_artifacts.rs` - run artifact manifests, diffs, memory-use evidence. Symbols: `RunArtifactManager`, `RunManifest`, `capture_workspace_diff`, `parse_git_status_line`.

Subagents, tasks, agents:

- `vegvisir/src/subagents.rs` - bounded child-agent records/supervisor. Symbols: `SubAgentStatus`, `SubAgentWorkBudget`, `SubAgentTaskRecord`, `SubAgentSupervisor`, `run_child_worker`, `upsert_record`.
- `vegvisir/src/tasks.rs` - background task lifecycle. Symbols: `TaskKind`, `TaskState`, `TaskRecord`, `TaskManager`, `TaskRunner`, `TaskRunnerEvent`.
- `vegvisir/src/agent_admin/` - persistent custom agent registry/admin.
  - `cli.rs` - `run_agent_admin_cli`.
  - `models.rs` - `AgentTemplate`, `ValidationReport`, `RegisterReport`, `AgentMetrics`.
  - `registry.rs` - registry service plus admin TUI; many `handle_admin_tui_*` functions.
  - `validation.rs` - `validate_profile`, model/tool/skill/MCP allow-list validation.
  - `templates.rs` - `agent_template`, `agent_templates`, `profile_from_template`.
  - `display.rs`, `history.rs`, `metrics.rs`, `utils.rs`, `service.rs` - rendering/history/metrics/helpers.

Skills, LSL, personas, speech, verification:

- `vegvisir/src/lsl.rs` - Linked Skill Library structures and loading. Symbols: `LinkedSkillLibrary`, `LslRegistry`, `LslSubskill`, `LoadedSkillContext`.
- `vegvisir/src/persona.rs` - builtin and file-backed persona profiles. Symbols: `PersonaProfile`, `builtin_personas`, `persona_path`.
- `vegvisir/src/profile.rs` - user profile store/preferences. Symbols: `UserProfile`, `UserProfileStore`, preference structs.
- `vegvisir/src/planning.rs` - plan model. Symbols: `TaskStatus`, `PlanItem`, `Plan`.
- `vegvisir/src/prompts.rs` - prompt assembly. Symbol: `PromptAssembler`.
- `vegvisir/src/evals.rs` - built-in evals/golden cases. Symbols: `EvalResult`, `run_builtin_evals`, `run_eval_file`, `eval_memory_project_isolation`, `eval_secret_memory_rejection`.
- `vegvisir/src/verification.rs` - verification runner. Symbol: `VerificationRunner`.
- `vegvisir/src/speech.rs` - STT/TTS backends. Symbols: `SpeechTranscriptionResult`, `TextToSpeechResult`, `speech_backends`, `synthesize_text_to_speech_with_provider`.
- `vegvisir/src/hooks.rs` - extension hooks. Symbols: `Hook`, `HookManager`.
- `vegvisir/src/events/mod.rs` - event envelope and run/tool/approval event types.
- `vegvisir/src/telemetry.rs` - token counting. Symbols: `count_text_tokens`, `selected_usage_or_counted`.
- `vegvisir/src/environment.rs` - environment file parsing/loading.
- `vegvisir/src/setup.rs` - first-run/provider setup. Symbols: `SetupOptions`, `SetupSummary`, `run_setup`.
- `vegvisir/src/attachments.rs` - attachment token parsing. Symbols: `extract_attachments`, `attachment_for`.

## CMS v2: `components/cms-v2/`

Entry points:

- `components/cms-v2/src/lib.rs` - exports modules and core memory structs.
- `components/cms-v2/src/bin/cms.rs` - `cms` CLI. Symbols: `Cli`, `Command`, `ScopeCommand`, `PromptCacheCommand`, `main`.
- `components/cms-v2/tests/lml_sqlite.rs` - LML/SQLite tests.

Main files:

- `core.rs` - base memory types: `MemoryObject`, `Claim`, `MemoryLink`, `MemorySource`, `MemoryVersion`, `MemoryChunk`, `MemorySearchResult`, `RetrievalBundle`.
- `cms_api.rs` - typed API facade: `MemoryId`, `ProjectId`, `RetrievalRequest`, `CommitRequest`, `CmsMemoryClient`.
- `cms_runtime.rs` - local CMS client and visibility conversion. Symbols: `LocalCmsMemoryClient`, `memory_contains_sensitive_content`, `memory_visible_to_request`.
- `sqlite.rs` - SQLite schema/storage records and operations. Symbols include `MemoryVersionRecord`, `AuditEvent`, `MemoryLedgerRecord`, `MemoryListEntry`, `LedgerStats`, prompt-cache records.
- `lml.rs` - Lightweight Memory Language parse/write/validate. Symbols: `LmlParser`, `LmlWriter`, `LmlValidator`, `memory_from_map`.
- `ecm.rs` - context exposure model. Symbols: `UserId`, `SessionId`, `ContextFrame`, `ContextBudget`, `ContextSession`, `PreparedContext`, `TaskIntent`.
- `rag.rs` - hybrid retrieval orchestrator. Symbols: `RagOrchestrator`, `HybridRagOrchestrator`, `retrieval_trace`.
- `vectors.rs` - embedding/vector index. Symbols: `EmbeddingService`, `DeterministicLexicalEmbedding`, `VectorIndex`, `SqliteVectorIndex`.
- `graph.rs` - graph index. Symbols: `GraphIndex`, `SqliteGraphIndex`, `GraphHit`.
- `prompt_cache.rs` - prompt cache policy/capsules. Symbols: `PromptCachePolicy`, `PromptBlock`, `PromptCapsule`.
- `safety.rs` - sensitive content detection/redaction. Symbols: `detect_sensitive_content`, `contains_sensitive_content`, `redact_sensitive_text`.
- `data_import.rs` - ChatGPT/document/JSONL import models and options.
- `archive.rs` - memory archive export/restore data models and export functions.
- `usrl.rs` - USRL scope visibility/import/validation. Symbols: `UsrlScopePolicy`, `import_usrl_file`, `validate_usrl_file`.
- `maintenance.rs` - LML maintenance/reindex/repair. Symbols: `MaintenanceEngine`, `MaintenanceRepairer`, `Reindexer`.
- `diagnostics.rs` - health and observability summaries. Symbol: `run_diagnostics`.
- `provider_contracts.rs` - provider/adapter contracts. Symbols: `ProviderEndpointSpec`, `ModelAdapter`, `EmbeddingAdapter`.

## HBSE: `components/HBSE/hbse/`

Entry points:

- `src/main.rs` - `hbse` CLI. Symbols: `Cli`, `Command`, `VaultCommand`, `SecretCommand`, `AuditCommand`, `PolicyCommand`, `TicketCommand`, `ProviderCommand`.
- `src/bin/hbse-broker.rs` - broker binary entry.
- `src/lib.rs` - exports HBSE modules and `HBSE_VERSION`.
- `components/HBSE/install.sh`, `uninstall.sh`, `package-local.sh` - component lifecycle.

Main files:

- `vault.rs` - local vault operations. Symbols: `LocalVault`, `VaultError`, `secret_id_from_ref`.
- `store.rs` - SQLite vault storage and permissions. Symbols: `SQLiteVaultStore`, `SecretSummary`, `VaultHeader`.
- `records.rs` - secret record schema. Symbols: `SecretRecord`, `SecretStatus`, `SecretType`.
- `crypto.rs` - AES-GCM and AAD helpers. Symbols: `CryptoEngine`, `CryptoError`, `encrypt_aes_gcm`, `decrypt_aes_gcm`.
- `keys.rs` - key hierarchy and KDF. Symbols: `KeyHierarchy`, `counter_kdf_hmac_sha256`.
- `policy.rs` - access policy engine. Symbols: `AccessPolicy`, `AccessRequest`, `PolicyEngine`, `PolicyDecision`, `DeliveryMode`.
- `tickets.rs` - access tickets. Symbols: `SecretAccessTicket`, `TicketManager`.
- `broker_daemon.rs` - Unix socket/HTTP gateway broker. Symbols: `BrokerState`, `serve`, `serve_with_http_gateway`, `brokered_http_request`.
- `provider.rs` - passphrase provider. Symbols: `PassphraseProvider`, `PassphraseProviderBinding`.
- `provider_system.rs` - system fingerprint provider.
- `provider_tpm2.rs` - TPM2 tools provider.
- `provider_tpm2_esapi.rs` - TPM2 ESAPI provider.
- `provider_yubikey.rs` - YubiKey PIV provider.
- `provider_catalog.rs` - `local_provider_catalog`.
- `audit.rs` - audit chain. Symbols: `AuditEvent`, `AuditManager`, `verify_audit_chain`.
- `backup.rs`, `recovery.rs`, `rotation.rs`, `release.rs` - backup/restore, recovery package, rotation jobs, release signing/evidence.
- `dotenv.rs` - dotenv secret scanning. Symbols: `scan_dotenv`, `parse_dotenv`.
- `redaction.rs` - known-secret and pattern redaction. Symbols: `RedactionEngine`, `redaction_fingerprint`.
- `mfa.rs` - TOTP config/enrollment/verification.
- `systemd.rs` - broker systemd unit/socket generation/install.
- `serialization.rs` - canonical JSON/base64url/time helpers.

## Skiller: `components/skiller/`

Entry points:

- `src/main.rs` - binary entry calls library CLI.
- `src/lib.rs` - CLI command definitions and dispatch plus module exports.
- `tests/rust_compile.rs`, `tests/rust_workflow.rs`, `tests/rust_interface_compile.rs` - workflow/interface tests.

Main files:

- `compiler/mod.rs` - source/API/CLI/OpenAPI/import compilers. Symbols: `compile_url`, `compile_openapi`, `compile_api`, `compile_cli`, `compile_cli_help`, `compile_from_parts`, `import_skill_path`.
- `ingest/mod.rs` - source ingestion. Symbols: `ingest_url`, `ingest_repository`, `ingest_path`, `html_sections`, `markdown_like_heading_sections`.
- `models.rs` - skill bundle/domain/eval/runtime data model.
- `registry/mod.rs` - local registry/search/load/compat behavior.
- `runtime/mod.rs` - runtime loading/execution support.
- `forge/mod.rs` - Forge provider catalog, summaries, handoff/preflight/self-test. Symbols: `ForgeProviderCatalog`, `provider_catalog`, `summarize_forge_history`, `forge_summary_markdown`.
- `agents/mod.rs` - agent pack build/verification/selection reports. Symbols: `AgentPackBuildReport`, `AgentPackVerificationReport`, `AgentBuilderSummary`.
- `review.rs` - verifier review and apply review. Symbols: `verifier_review`, `verifier_review_markdown`, `apply_verifier_review`.
- `corpus.rs` - corpus manifest/lifecycle planning. Symbols: `build_corpus_manifest`, `write_corpus_manifest`.
- `semantic.rs` - semantic matching/routing helpers.
- `domain/mod.rs` - builtin domain profiles. Symbols: `builtin_profiles`, `profile`, `get_profile`.
- `evidence/mod.rs` - evidence report markdown and skill warnings.
- `security/mod.rs` - security policy/check helpers.
- `source_meta.rs` - source metadata.
- `telemetry/mod.rs` - telemetry reporting.

## MSP: `components/msp/` And `components/msp-client/`

Reference MSP:

- `components/msp/Cargo.toml` - MSP workspace.
- `components/msp/spec.md` - protocol spec.
- `components/msp/schemas/` - JSON schemas for manifests, skill packs, trust policy, verification/execution reports, protocol results.
- `components/msp/docs/` - threat model, registry, skiller publication, compatibility docs.

Crates:

- `crates/msp-core/src/lib.rs` - protocol type re-exports.
- `msp-core/src/manifest.rs` - skill/pack manifest model. Symbols: `SkillManifest`, `SkillPackManifest`, `RiskLevel`, `ReviewStatus`, dependency/version structs.
- `msp-core/src/protocol.rs` - protocol requests/results. Symbols: `MspInfo`, `core_methods`, `SkillSearchQuery`, `PackSearchQuery`, `RegistrySearchResult`.
- `msp-core/src/trust_policy.rs` - trust policy model. Symbols: `TrustPolicyRule`, `TrustAction`, `SignaturePolicy`, `RiskPolicy`, `DependencyPolicy`.
- `msp-core/src/verification.rs` - verification/execution report model.
- `msp-core/src/signature.rs` - Ed25519 signing/verification helpers.
- `msp-core/src/schema_validation.rs` - schema validation helpers.
- `msp-registry/src/lib.rs` - local registry/search/load/verify/dependency trust. Symbols: `LocalRegistry`, `RegistrySkill`, `RegistryPack`, `version_matches_requirement`.
- `msp-publisher/src/lib.rs` - Skiller bundle publication. Symbols: `PublishOptions`, `PublishReport`, `SkillerPackage`.
- `msp-cli/src/main.rs` - CLI. Symbols: `Cli`, `Commands`, `RegistryCommands`, `SkillCommands`, `PackCommands`, `TrustCommands`, `PublishCommands`.
- `msp-server/src/main.rs` - JSON-RPC stdio server. Symbols: `JsonRpcRequest`, `JsonRpcResponse`, `serve_stdio`, `handle_request`, `dispatch`.

Vegvisir MSP client:

- `components/msp-client/src/lib.rs` - host-friendly MSP API. Symbols: `default_registry_root`, `ClientInfo`, `SearchRequest`, `SearchResponse`, `LoadMode`, `LoadedSkill`, `CompatibilityRequest`, `TrustEvaluationRequest`, `ImportSkillerBundleRequest`.
- `components/msp-client/src/cli.rs` - client CLI. Symbols: `run_cli`, `run_cli_from`.
- `components/msp-client/src/main.rs` - binary entry.
- `components/msp-client/tests/reference_registry.rs` - registry integration test.

## Solarium: `components/solarium/`

Purpose: TypeScript Playwright runtime for browser automation, evidence capture, scoped crawling, audits, and JSON-RPC tool serving.

Entry points:

- `src/index.ts` - public exports.
- `src/cli/index.ts` - CLI entry.
- `src/server/json-rpc.ts` - JSON-RPC server. Symbols: `runJsonRpcServer`, `handleJsonRpcRequest`.
- `package.json` - scripts: `build`, `check`, `test`, `dev`, `browse`, `install:browsers`.
- `tests/*.test.mjs` - Node test suite.

Main files:

- `browser/engine.ts` - `SolariumBrowser`, `SolariumPage` and Playwright lifecycle.
- `browser/profile.ts` - builtin browser profiles. Symbols: `builtInProfiles`, `resolveProfile`.
- `browser/profile-store.ts` - profile list/read/validate/summary.
- `browser/auth-session.ts` - auth session profile creation/validation/resolution.
- `agent/actions.ts` - `browse` and action primitives.
- `agent/session.ts` - `AgentSession`, `runActions`.
- `agent/observations.ts` - `ObservationRecorder`, redaction helpers.
- `agent/inspect.ts` - `inspectPage`.
- `agent/plan.ts` - `planActionsFromInspectResult`.
- `agent/loop.ts` - `runLoop`.
- `config/job.ts` - job file read/validate/run. Symbols: `readSolariumJob`, `runJob`, `validateSolariumJob`.
- `config/validate.ts` - config/action validation. Symbols: `validateSolariumConfig`, `validateSolariumFile`, `validateActions`.
- `security/scope.ts` - scope policy. Symbols: `assertUrlInScope`, `checkUrlScope`, `hostMatches`, `validateScopePolicy`.
- `security/network-policy.ts` - scoped network policy attachment.
- `security/network.ts` - `NetworkScopeGuard`.
- `security/crawler.ts` - `crawl`.
- `security/audit.ts` - `audit`.
- `security/owasp-audit.ts` - `owaspAudit`.
- `security/graphql-audit.ts` - `graphqlAudit`.
- `reporting/markdown.ts`, `html.ts` - audit/crawl/session/loop report renderers.
- `reporting/events.ts` - JSONL event logging/summaries.
- `reporting/replay.ts` - replay/summarize events and session resume plan.
- `reporting/artifacts.ts`, `evidence.ts` - artifact/evidence manifests.
- `client/json-rpc.ts` - JSON-RPC client and server launcher.
- `skills/workflow-seed.ts` - workflow seed generation.
- `types.ts` - common browser/action/observation types.

## USRL: `components/usrl/`

Purpose: TypeScript DSL parser/runtime for USRL policies and linked PLL/CLL contracts.

Entry points:

- `src/index.ts` - public exports.
- `src/cli.ts` - CLI entry.
- `package.json` - scripts: `build`, `test`.
- `tests/*.test.ts` - parser/resolver/runtime/JSRT/linked validation tests.

Main files:

- `ast.ts` - AST types: `TopDecl`, `Statement`, `Expr`, `TypeExpr`, `Program`.
- `lexer.ts` - `lex`, `Token`, `TokenStream`.
- `parser.ts` - recursive-descent parser; public `parseUsrl`.
- `validator.ts` - semantic validation. Symbols: `validateProgram`, `assertValidProgram`.
- `resolver.ts` - symbol/reference graph. Symbols: `resolveProgram`, `ResolutionResult`, `BoundGraph`.
- `project-resolver.ts` - multi-file project resolution. Symbol: `resolveProject`.
- `runtime.ts` - evaluator. Symbol: `evaluateProgram`.
- `linked.ts` - PLL/CLL parsing/validation. Symbols: `parsePll`, `parseCll`, `validatePll`, `validateCll`, `validatePair`.
- `jsrt.ts` - JSON session/runtime trace validation and state engine. Symbols: `validateJsrtFrames`, `parseJsrtDocument`, `JsrtSessionEngine`, `applyJsrtFrames`, `computeFrameChecksum`, `signFrameHmac`.
- `errors.ts` - `UsrlError`.
- `jsrt.schema.json`, `jsrt.profiles.json`, `jsrt.transitions.json`, `jsrt.errors.json` - JSRT schema/profile data.

## Desktop App: `components/desktop/`

Purpose: Tauri desktop UI around Vegvisir bridge/runtime.

Entry points:

- `src/main.ts` - browser UI state, rendering, event handlers, bridge method forms, canvas/workbench panels.
- `src-tauri/src/main.rs` - Tauri backend entry.
- `src-tauri/src/bridge.rs` - bridge process management and commands.
- `src-tauri/src/explorer.rs` - file explorer backend commands.
- `src-tauri/tauri.conf.json`, `vite.config.ts`, `tailwind.config.js` - app config.
- `package.json` - scripts: `dev`, `build`, `check`, `web:build`, `web:dev`.

Frontend files:

- `src/types.ts` - bridge/message/approval/file explorer/layout types.
- `src/panels.ts` - panel definitions; `panelDefinition`.
- `src/markdown.ts` - chat markdown/code rendering; `renderMessage`.
- `src/html.ts` - `escapeHtml`, `cssEscape`.
- `src/config.ts` - frontend config constants.
- `src/styles.css` - UI styles.
- `index.html` - web root.

Backend files:

- `src-tauri/src/main.rs` - app command registration.
- `src-tauri/src/bridge.rs` - starts/stops harness bridge, streams events.
- `src-tauri/src/explorer.rs` - list/read filesystem entries.
- `src-tauri/build.rs` - Tauri build script.

## Ghidra Headless MCP: `components/ghidra-headless-mcp/`

- `bridge_mcp_ghidra_headless.py` - Python MCP bridge to installed Ghidra/analyzeHeadless runtime.
- `bin/ghidra-headless` - wrapper.
- `examples/smoke-test.sh` - smoke test.
- `scripts/Vegvisir*.java` - Ghidra scripts:
  - `VegvisirSummary`, `VegvisirReport`, `VegvisirListFunctions`, `VegvisirFunctionInfo`
  - `VegvisirDecompile`, `VegvisirDisassemble`, `VegvisirCallGraph`, `VegvisirXrefs`
  - `VegvisirListImports`, `VegvisirListExports`, `VegvisirListSegments`, `VegvisirListStrings`, `VegvisirStringRefs`
  - `VegvisirSearchSymbols`, `VegvisirReadBytes`, `VegvisirListVariables`, `VegvisirBookmarks`, `VegvisirMutate`, `VegvisirJson`

## Binary Intelligence Workbench: `components/binary-intelligence-workbench/`

Purpose: Python CLI/server and helpers for binary triage, case indexing, Ghidra extraction, reporting, firmware/diff workflows.

Entry points:

- `pyproject.toml` - package metadata and `biw = biw.cli:main`.
- `biw/cli.py` - command implementations. Symbols: `basic_extract`, `triage`, `case_list`, `serve_cmd`, `report_cmd`, `skill_run_cmd`.
- `tests/test_cli.py` - CLI tests.

Main files:

- `biw/core.py` - shared utilities and `CasePaths`.
- `biw/ghidra.py` - Ghidra wrapper/status/extraction.
- `biw/heuristics.py` - string/import analysis and finding risk.
- `biw/index.py` - case index/detail generation.
- `biw/memory.py` - redacted memory summaries.
- `biw/diff.py` - binary diff reports.
- `biw/explain.py` - function explanation via Ghidra artifacts.
- `biw/firmware.py` - firmware/archive triage.
- `biw/report.py` - summary/full report generation.
- `biw/server.py` - HTTP server; `BIWRequestHandler`, `serve`.
- `biw/skill.py` - BIW skill bundle runner; `run_skill`, `list_skills`.
- `biw/agent.py` - simple agent task helpers.
- `docs/` - BIW architecture/quickstart/case schema/workflows.
- `skiller-bundles/` - binary/artifact skill bundles.

## Companion Scripts

All under `companion_scripts/`; most source common helpers from `companion_scripts/lib/common.sh`.

High-value groups:

- Repo/git state: `v.sh`, `vrepo-root.sh`, `vgit-status.sh`, `vbranch-info.sh`, `vbranch-drift.sh`, `vdiff-summary.sh`, `vchanged-files.sh`, `vtracked-files.sh`, `vuntracked-files.sh`, `vrecent-commits.sh`, `vremotes.sh`.
- Repo maps/stats/search: `vrepo-map.sh`, `vworkspace-map.sh`, `vtree.sh`, `vsource-files.sh`, `vtop-files.sh`, `vfilesize.sh`, `vrepo-stats.sh`, `vgrep.sh`, `vmarkdown-files.sh`, `vsecurity-files.sh`, `vworkflow-files.sh`.
- Runs/artifacts: `vruns.sh`, `vrun-latest.sh`, `vrun-inspect.sh`, `vrun-summary.sh`, `vrun-timeline.sh`, `vrun-files.sh`, `vrun-errors.sh`, `vrun-verification.sh`, `vrun-memory.sh`, `vrun-provenance.sh`, `vrun-search.sh`, `vrun-artifacts.sh`.
- Memory/CMS: `vmemories.sh`, `vmemory-search.sh`, `vcms-search.sh`, `vcms-recent.sh`, `vcms-run.sh`, `vcms-artifacts.sh`, `vcms-context-size.sh`, `vcms-query-summary.sh`, `vcms-thread-map.sh`, `vcms-source-audit.sh`.
- HBSE/secrets: `vhbse-env.sh`, `vhbse-files.sh`, `vhbse-search.sh`, `vhbse-secret-scan.sh`, `vhbse-secret-refs.sh`, `vhbse-redaction-check.sh`, `vhbse-status-files.sh`, `vhbse-manifest-files.sh`, `vhbse-path-allowlist.sh`, `vsecret-scan.sh`.
- Skills/subagents: `vskill-index.sh`, `vskill-list.sh`, `vskill-meta.sh`, `vskill-route.sh`, `vskill-deps.sh`, `vskill-bundles.sh`, `vskill-artifacts.sh`, `vskill-compat.sh`, `vskill-change-impact.sh`, `vsubagents.sh`, `vsubagents-summary.sh`, `vsubagent-show.sh`, `vsubagent-logs.sh`, `vrun-subagents.sh`.
- Approvals/shell/context: `vapprovals.sh`, `vapprovals-pending.sh`, `vapprovals-files.sh`, `vapprovals-stale.sh`, `vshell-logs.sh`, `vshell-tail.sh`, `vshell-files.sh`, `vshell-log-view.sh`, `vcontext-budget.sh`, `vcontext-pack.sh`.
- Diagnostics/repro: `vdoctor.sh`, `vtest.sh`, `vprecommit.sh`, `vrepro.sh`, `vtrace.sh`, `vsnapshot.sh`, `vworkspace-health.sh`, `vworkspace-hotspots.sh`.

## Docs Map

- Start here: `docs/README.md`, `docs/quickstart.md`, `docs/system-overview.md`, `docs/architecture.md`, `docs/runtime-architecture.md`.
- Main harness use: `docs/vegvisir-usage.md`, `docs/development.md`, `docs/install-upgrade.md`, `docs/troubleshooting.md`, `docs/desktop-app.md`.
- Runtime controls: `docs/command-sandboxing-and-approvals.md`, `docs/autonomy-levels.md`, `docs/privileged-command-workflow.md`, `docs/security-and-operations.md`, `docs/security-model.md`, `docs/run-artifacts.md`, `docs/evals-and-verification.md`.
- Providers/HBSE/MCP: `docs/provider-setup.md`, `docs/hbse-setup.md`, `docs/hbse-usage.md`, `docs/mcp-services.md`.
- Memory/CMS: `docs/memory-model.md`, `docs/cms-v2-usage.md`.
- Skills/Skiller/USRL/LSL/subagents: `docs/skills.md`, `docs/skiller-system.md`, `docs/usrl-usage.md`, `docs/usrl-language-reference.md`, `docs/lsl-skill-system.md`, `docs/subagents.md`, `docs/subagent-delegation.md`, `docs/subagent-plan-implementation.md`.
- Solarium/browser: `docs/solarium-system.md`, `docs/overlay-integration.md`.
- Admin: `docs/agent-admin.md`, `docs/release-checklist.md`, `docs/new-runtime-features.md`.

## Demo And Benchmark Map

- `demos/README.md` - demo overview.
- `demos/RUN_ALL_SAFE_SMOKES.sh` - safe smoke runner.
- `demos/scripts/*.sh` - named walkthroughs:
  - `01-vegvisir-fixes-itself.sh`, `02-skiller-to-msp-to-vegvisir.sh`, `03-msp-tamper-rejection.sh`
  - `04-bounded-subagents-review.sh`, `05-memory-context-resume.sh`, `06-hbse-no-plaintext-secrets.sh`
  - `07-five-minute-repo-takeover.sh`, `08-same-task-less-friction.sh`, `09-usrl-policy-bound-workflow.sh`, `10-msp-reference-host-adapter.sh`
- `demos/reference-host/msp_reference_host.py` - Python reference MSP host adapter.
- `benchmarks/runner.py` - benchmark task runner.
- `benchmarks/tasks/*.json` - benchmark task specs.

## Common Search Recipes

- Find Rust symbol: `rg -n "fn NAME|struct NAME|enum NAME|trait NAME" vegvisir/src components -g '*.rs'`
- Find TypeScript symbol: `rg -n "function NAME|class NAME|interface NAME|type NAME|const NAME" components/solarium/src components/usrl/src components/desktop/src -g '*.ts'`
- Find CLI command definitions: `rg -n "enum Command|Subcommand|Commands|Command::|match .*command" vegvisir/src components -g '*.rs'`
- Find slash command handler: `rg -n "handle_.*command|/COMMAND|COMMAND usage|commands/COMMAND" vegvisir/src/app vegvisir/src/app/commands`
- Find provider logic: `rg -n "ProviderAdapter|discover_.*models|openai|anthropic|google|ollama|hbse" vegvisir/src`
- Find memory/CMS logic: `rg -n "VegvisirCms|CmsMemoryClient|MemoryObject|RetrievalRequest|ContextFrame" vegvisir/src components/cms-v2/src`
- Find approval/sandbox logic: `rg -n "Approval|PermissionPolicy|Sandbox|openat2|dangerous|allow" vegvisir/src`
- Find Skiller/MSP logic: `rg -n "skiller|SkillBundle|msp|LocalRegistry|SkillManifest|Forge" vegvisir/src components/skiller/src components/msp*`

## Maintenance Notes

- Keep this file high-signal and hand-edited. Do not paste full generated symbol dumps.
- If adding a new component, update: Fast Orientation, Build/Test, component section, docs/scripts if relevant.
- If adding a new major public module, add one bullet with responsibility and 3-8 important symbols.
- If generated files or fixtures grow, add them to "Skip Or Deprioritize".
