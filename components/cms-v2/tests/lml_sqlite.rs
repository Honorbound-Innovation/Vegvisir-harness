use chrono::{TimeZone, Utc};
use cms_v2::archive::{
    ArchiveExportOptions, ArchiveRedactionPolicy, ArchiveScopeFilter, export_archive,
    export_archive_with_options, export_json_with_options, read_manifest, restore_archive,
    restore_json_export,
};
use cms_v2::cms_api::{
    CmsApiResult, CmsMemoryClient, CommitRequest, CommitResult, MemoryId as ApiMemoryId,
    MemoryObject as ApiMemoryObject, MemoryRetrievalResult, RetrievalBundle,
    RetrievalMode as ApiRetrievalMode, RetrievalRequest,
};
use cms_v2::cms_runtime::LocalCmsMemoryClient;
use cms_v2::core::{Claim, MemoryLink, MemoryObject};
use cms_v2::data_import::{
    ChatGptImportOptions, DocumentImportOptions, JsonlImportOptions, document_paths,
    import_chatgpt_export, import_document_file, import_document_tree,
    import_document_tree_with_report, import_jsonl_file, preview_chatgpt_export,
    preview_document_file, preview_document_tree, preview_jsonl_file, report_chatgpt_export,
    report_document_tree, report_jsonl_file,
};
use cms_v2::diagnostics::run_diagnostics;
use cms_v2::ecm::{
    ContextBudget, ContextFrame, ContextFrameType, ContextMode, ContextPriority, ContextRequest,
    ContextSession, EterniumContextManager, MemoryCandidateType, PreparedContext, SessionId,
    UserId,
};
use cms_v2::graph::{GraphIndex, SqliteGraphIndex};
use cms_v2::lml::{CURRENT_LML_SCHEMA_VERSION, LmlError, LmlParser, LmlParserOptions, LmlWriter};
use cms_v2::maintenance::{
    LmlMaintenanceEngine, MaintenanceEngine, MaintenanceRepairer, Reindexer,
};
use cms_v2::prompt_cache::{
    CacheScopeIdentity, PromptCacheEngine, PromptCachePrepareRequest, PromptCacheZone,
};
use cms_v2::provider_contracts::{
    EmbeddingAdapter, ModelAdapter, ModelAdapterRequest, ModelAdapterResponse,
    ProviderEndpointSpec, ProviderUsage,
};
use cms_v2::rag::{HybridRagOrchestrator, RagOrchestrator, RetrievalMode};
use cms_v2::safety::detect_sensitive_content;
use cms_v2::sqlite::{CURRENT_SCHEMA_VERSION, MemoryStatusFilter, SqliteLedger, content_hash};
use cms_v2::usrl::{
    MemoryVisibility, ScopeResolution, UsrlImportOptions, UsrlScopePolicy, UsrlValidationStatus,
    import_usrl_file, import_usrl_file_with_options, import_usrl_text,
    import_usrl_text_with_options, memory_visible_to_scope, summarize_usrl, usrl_paths,
    validate_usrl_file_with_authoritative_cli,
};
use cms_v2::vectors::{
    DeterministicLexicalEmbedding, EmbeddingService, SqliteVectorIndex, VectorIndex,
    vector_index_hash, vector_index_hash_with_config,
};
use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

fn fake_openai_key() -> String {
    ["sk", "-1234567890abcdef1234567890abcdef"].concat()
}

fn fake_openai_key_short() -> String {
    ["sk", "-1234567890abcdefghijklmnop"].concat()
}

fn fake_openai_password_value() -> String {
    ["sk", "-abcdefghijklmnopqrstuvwxyz"].concat()
}

fn fake_github_token() -> String {
    ["ghp", "_1234567890abcdef1234567890abcdef"].concat()
}

fn fake_github_token_short() -> String {
    ["ghp", "_1234567890abcdefghijklmnop"].concat()
}

const SAMPLE: &str = r#"
memory {
    id: "mem_test"
    type: "design-thought"
    title: "Hybrid memory"
    created: "2026-05-15"
    updated: "2026-05-16T12:00:00Z"
    confidence: 0.9
    source: "unit-test"

    summary: """
    SQLite is a ledger and LML is the human-facing memory artifact.
    """

    claims {
        claim {
            id: "claim_001"
            text: "Vector stores are projections."
            confidence: 0.95
        }
    }

    links {
        relates_to: "project:CMS"
        depends_on: "concept:LML"
    }

    retrieval {
        tags: ["CMS", "RAG"]
        embedding_priority: "high"
    }
}
"#;

struct HarnessModelAdapterFixture;

impl ModelAdapter for HarnessModelAdapterFixture {
    fn complete(&self, request: ModelAdapterRequest) -> anyhow::Result<ModelAdapterResponse> {
        Ok(ModelAdapterResponse {
            provider: request.endpoint.provider,
            model: request.endpoint.model,
            output_text: format!(
                "harness adapter received {} prompt chars",
                request.envelope.prompt.len()
            ),
            usage: Some(ProviderUsage {
                input_tokens: request.envelope.prompt.len().div_ceil(4),
                output_tokens: 7,
                cached_input_tokens: request.envelope.cache_hint.cacheable_prefix_tokens,
            }),
            raw_metadata: Default::default(),
        })
    }
}

struct HarnessEmbeddingAdapterFixture;

impl EmbeddingService for HarnessEmbeddingAdapterFixture {
    fn provider_id(&self) -> String {
        "harness-openai-text-embedding-3-large".to_string()
    }

    fn chunking_version(&self) -> String {
        "vegvisir-chunks-v1".to_string()
    }

    fn embed_text(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        Ok(vec![text.len() as f32, 1.0])
    }
}

impl EmbeddingAdapter for HarnessEmbeddingAdapterFixture {
    fn endpoint(&self) -> ProviderEndpointSpec {
        ProviderEndpointSpec::openai_embeddings("text-embedding-3-large")
    }
}

#[test]
fn parses_lml_memory() {
    let memory = LmlParser::parse_text(SAMPLE).unwrap();
    assert_eq!(memory.id, "mem_test");
    assert_eq!(memory.memory_type, "design-thought");
    assert_eq!(memory.claims.len(), 1);
    assert_eq!(memory.links.len(), 2);
    assert_eq!(memory.tags, vec!["CMS", "RAG"]);
    assert_eq!(memory.metadata["embedding_priority"], "high");
}

#[test]
fn lml_retrieval_metadata_carries_prompt_cache_hints() {
    let source = SAMPLE.replace(
        "        embedding_priority: \"high\"",
        "        embedding_priority: \"high\"\n        prompt_cache_policy: \"project_stable\"\n        prompt_zone: \"StableMemoryCapsule\"\n        prompt_cache_sensitivity: \"normal\"",
    );
    let memory = LmlParser::parse_text(&source).unwrap();
    assert_eq!(memory.metadata["prompt_cache_policy"], "project_stable");
    assert_eq!(memory.metadata["prompt_zone"], "StableMemoryCapsule");
    assert_eq!(memory.metadata["prompt_cache_sensitivity"], "normal");

    let round_trip = LmlWriter::to_text(&memory).unwrap();
    assert!(round_trip.contains("prompt_cache_policy: \"project_stable\""));
    assert!(round_trip.contains("prompt_zone: \"StableMemoryCapsule\""));
}

#[test]
fn lml_round_trips() {
    let memory = LmlParser::parse_text(SAMPLE).unwrap();
    let text = LmlWriter::to_text(&memory).unwrap();
    let parsed = LmlParser::parse_text(&text).unwrap();
    assert_eq!(parsed.id, memory.id);
    assert_eq!(parsed.claims, memory.claims);
    assert_eq!(parsed.links, memory.links);
    assert_eq!(parsed.tags, memory.tags);
}

#[test]
fn lml_writer_emits_current_schema_version() {
    let memory = LmlParser::parse_text(SAMPLE).unwrap();
    let text = LmlWriter::to_text(&memory).unwrap();

    assert!(text.contains(&format!("schema_version: {CURRENT_LML_SCHEMA_VERSION}")));
    assert!(LmlParser::parse_text_strict(&text).is_ok());
}

#[test]
fn lml_parser_accepts_legacy_unversioned_files() {
    let memory = LmlParser::parse_text(SAMPLE).unwrap();

    assert_eq!(memory.id, "mem_test");
    assert_eq!(memory.title, "Hybrid memory");
}

#[test]
fn lml_strict_parser_rejects_unknown_top_level_fields() {
    let source = SAMPLE.replace(
        "    title: \"Hybrid memory\"",
        "    title: \"Hybrid memory\"\n    unknown_future_field: \"not yet supported\"",
    );
    let err = LmlParser::parse_text_strict(&source).unwrap_err();

    assert!(matches!(err, LmlError::UnknownField { .. }));
}

#[test]
fn lml_permissive_parser_allows_unknown_top_level_fields() {
    let source = SAMPLE.replace(
        "    title: \"Hybrid memory\"",
        "    title: \"Hybrid memory\"\n    unknown_future_field: \"not yet supported\"",
    );
    let memory = LmlParser::parse_text(&source).unwrap();

    assert_eq!(memory.id, "mem_test");
}

#[test]
fn lml_parser_rejects_future_schema_versions_by_default() {
    let source = SAMPLE.replacen("memory {", "memory {\n    schema_version: 999", 1);
    let err = LmlParser::parse_text(&source).unwrap_err();

    assert!(matches!(err, LmlError::UnsupportedSchemaVersion { .. }));
}

#[test]
fn lml_parser_can_read_future_schema_in_compatibility_mode() {
    let source = SAMPLE.replacen("memory {", "memory {\n    schema_version: 999", 1);
    let memory =
        LmlParser::parse_text_with_options(&source, LmlParserOptions::future_compatible()).unwrap();

    assert_eq!(memory.id, "mem_test");
}

#[test]
fn sqlite_persists_memory_claims_links_and_searches() {
    let memory = LmlParser::parse_text(SAMPLE).unwrap();
    let mut ledger = SqliteLedger::open_memory().unwrap();
    ledger.upsert_memory(&memory, None).unwrap();

    let stored = ledger.get_memory("mem_test").unwrap().unwrap();
    assert_eq!(stored.title, "Hybrid memory");
    assert_eq!(stored.source.unwrap().reference, "unit-test");
    assert_eq!(stored.claims.len(), 1);
    assert_eq!(stored.links.len(), 2);

    let results = ledger.search_exact("ledger", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].memory.id, "mem_test");
}

#[test]
fn sqlite_records_schema_migrations_on_fresh_database() {
    let ledger = SqliteLedger::open_memory().unwrap();

    let migrations = ledger.applied_migrations().unwrap();
    assert_eq!(ledger.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    assert_eq!(
        ledger.stats().unwrap().schema_version,
        CURRENT_SCHEMA_VERSION
    );
    assert_eq!(
        migrations
            .iter()
            .map(|migration| migration.version)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(migrations[0].name, "initial-ledger-schema");
    assert_eq!(migrations[1].name, "memory-source-columns");
    assert_eq!(migrations[2].name, "prompt-cache-tables");
    assert!(
        migrations
            .iter()
            .all(|migration| migration.checksum.len() == 64)
    );
}

#[test]
fn sqlite_schema_migrations_are_idempotent_on_reopen() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("cms.sqlite3");

    let first = SqliteLedger::open(&db_path).unwrap();
    let first_migrations = first.applied_migrations().unwrap();
    drop(first);

    let second = SqliteLedger::open(&db_path).unwrap();
    let second_migrations = second.applied_migrations().unwrap();

    assert_eq!(second.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    assert_eq!(second_migrations, first_migrations);
}

#[test]
fn sqlite_backup_to_creates_restorable_database_copy() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("cms.sqlite3");
    let backup_path = tempdir.path().join("backups/cms-backup.sqlite3");
    {
        let mut ledger = SqliteLedger::open(&db_path).unwrap();
        let memory = LmlParser::parse_text(SAMPLE).unwrap();
        ledger.upsert_memory(&memory, None).unwrap();
        let report = ledger.backup_to(&backup_path).unwrap();
        assert_eq!(report.source_schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(report.source_active_memories, 1);
        assert!(report.backup_size_bytes > 0);
    }

    let backup = SqliteLedger::open(&backup_path).unwrap();
    assert_eq!(backup.stats().unwrap().active_memories, 1);
    assert_eq!(
        backup.get_memory("mem_test").unwrap().unwrap().title,
        "Hybrid memory"
    );
}

#[test]
fn sqlite_migrates_legacy_database_without_source_columns() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("legacy.sqlite3");
    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE memories (
                id TEXT PRIMARY KEY,
                type TEXT NOT NULL,
                title TEXT NOT NULL,
                summary TEXT,
                body TEXT,
                body_hash TEXT NOT NULL,
                lml_path TEXT,
                confidence REAL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                status TEXT NOT NULL
            );
            "#,
        )
        .unwrap();
    }

    let mut ledger = SqliteLedger::open(&db_path).unwrap();
    assert_eq!(ledger.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    assert_eq!(ledger.applied_migrations().unwrap().len(), 3);

    let memory = LmlParser::parse_text(SAMPLE).unwrap();
    ledger.upsert_memory(&memory, None).unwrap();
    let stored = ledger.get_memory("mem_test").unwrap().unwrap();
    assert_eq!(stored.source.unwrap().reference, "unit-test");
}

#[test]
fn sqlite_migrates_v2_database_with_existing_source_columns_to_prompt_cache_schema() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("legacy-v2.sqlite3");
    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                checksum TEXT NOT NULL,
                applied_at TEXT NOT NULL
            );

            INSERT INTO schema_migrations (version, name, checksum, applied_at)
            VALUES
                (1, 'initial-ledger-schema', 'legacy-v1-checksum', '2026-05-16T00:00:00Z'),
                (2, 'memory-source-columns', 'legacy-v2-checksum', '2026-05-16T00:01:00Z');

            CREATE TABLE memories (
                id TEXT PRIMARY KEY,
                type TEXT NOT NULL,
                title TEXT NOT NULL,
                summary TEXT,
                body TEXT,
                body_hash TEXT NOT NULL,
                lml_path TEXT,
                source_kind TEXT,
                source_reference TEXT,
                confidence REAL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                status TEXT NOT NULL
            );

            INSERT INTO memories (
                id, type, title, summary, body, body_hash, lml_path,
                source_kind, source_reference, confidence, created_at, updated_at, status
            )
            VALUES (
                'mem_legacy_v2_prompt_cache_upgrade',
                'design-thought',
                'Legacy V2 Prompt Cache Upgrade',
                'LegacyV2PromptCacheUpgrade should keep source metadata.',
                'LegacyV2PromptCacheUpgrade body remains searchable after prompt-cache migration.',
                'legacy-v2-body-hash',
                NULL,
                'legacy-import',
                'legacy-v2-fixture',
                0.8,
                '2026-05-16T00:00:00Z',
                '2026-05-16T00:00:00Z',
                'active'
            );
            "#,
        )
        .unwrap();
    }

    let ledger = SqliteLedger::open(&db_path).unwrap();
    let migrations = ledger.applied_migrations().unwrap();
    assert_eq!(ledger.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    assert_eq!(
        migrations
            .iter()
            .map(|migration| migration.version)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(migrations[2].name, "prompt-cache-tables");
    assert_eq!(ledger.prompt_cache_stats().unwrap().manifests, 0);

    let stored = ledger
        .get_memory("mem_legacy_v2_prompt_cache_upgrade")
        .unwrap()
        .unwrap();
    assert_eq!(stored.source.unwrap().kind, "legacy-import");
    assert_eq!(
        stored.summary,
        "LegacyV2PromptCacheUpgrade should keep source metadata."
    );
    assert_eq!(
        ledger
            .search_exact("LegacyV2PromptCacheUpgrade", 5)
            .unwrap()[0]
            .memory
            .id,
        "mem_legacy_v2_prompt_cache_upgrade"
    );

    let diagnostics = run_diagnostics(&ledger, tempdir.path(), 10).unwrap();
    assert_eq!(diagnostics.schema.current_version, CURRENT_SCHEMA_VERSION);
    assert!(diagnostics.schema.is_current);
    assert_eq!(diagnostics.prompt_cache.manifests, 0);
}

#[test]
fn cli_upgrade_user_acceptance_migrates_reindexes_and_retrieves_legacy_database() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("legacy.sqlite3");
    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE memories (
                id TEXT PRIMARY KEY,
                type TEXT NOT NULL,
                title TEXT NOT NULL,
                summary TEXT,
                body TEXT,
                body_hash TEXT NOT NULL,
                lml_path TEXT,
                confidence REAL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                status TEXT NOT NULL
            );

            INSERT INTO memories (
                id, type, title, summary, body, body_hash, lml_path,
                confidence, created_at, updated_at, status
            )
            VALUES (
                'mem_cli_legacy_upgrade',
                'design-thought',
                'Legacy Upgrade Memory',
                'LegacyUpgradeMemory should survive schema migration.',
                'LegacyUpgradeMemory body remains retrievable after migration and reindex.',
                'legacy-body-hash',
                NULL,
                0.8,
                '2026-05-16T00:00:00Z',
                '2026-05-16T00:00:00Z',
                'active'
            );
            "#,
        )
        .unwrap();
    }

    let status_output = assert_success(
        cms_cmd(&db)
            .arg("status")
            .arg(tempdir.path())
            .arg("--json")
            .output()
            .unwrap(),
    );
    let status: Value = serde_json::from_slice(&status_output.stdout).unwrap();
    assert_eq!(status["stats"]["schema_version"], CURRENT_SCHEMA_VERSION);
    assert_eq!(status["stats"]["active_memories"], 1);
    assert_eq!(status["stats"]["graph_indexed_memories"], 0);
    assert_eq!(status["stats"]["vector_indexed_memories"], 0);

    let get_output = assert_success(
        cms_cmd(&db)
            .args(["get", "mem_cli_legacy_upgrade", "--json"])
            .output()
            .unwrap(),
    );
    let memory: Value = serde_json::from_slice(&get_output.stdout).unwrap();
    assert_eq!(memory["title"], "Legacy Upgrade Memory");
    assert_eq!(
        memory["summary"],
        "LegacyUpgradeMemory should survive schema migration."
    );

    assert_success(cms_cmd(&db).arg("reindex").output().unwrap());

    let retrieve_output = assert_success(
        cms_cmd(&db)
            .args([
                "retrieve",
                "LegacyUpgradeMemory",
                "--mode",
                "hybrid",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let retrieval: Value = serde_json::from_slice(&retrieve_output.stdout).unwrap();
    assert!(
        retrieval["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["memory"]["id"] == "mem_cli_legacy_upgrade")
    );

    let diagnostics_output = assert_success(
        cms_cmd(&db)
            .arg("diagnostics")
            .arg(tempdir.path())
            .args(["--json"])
            .output()
            .unwrap(),
    );
    let diagnostics: Value = serde_json::from_slice(&diagnostics_output.stdout).unwrap();
    assert_eq!(diagnostics["health"]["status"], "healthy");
    assert_eq!(diagnostics["schema"]["is_current"], true);
    assert_eq!(diagnostics["schema"]["applied_migrations"], 3);
    assert_eq!(diagnostics["stats"]["active_memories"], 1);
    assert_eq!(diagnostics["stats"]["graph_indexed_memories"], 1);
    assert_eq!(diagnostics["stats"]["vector_indexed_memories"], 1);
}

#[test]
fn repository_memory_examples_parse() {
    for path in [
        "memories/cms_hybrid_architecture.lml",
        "memories/usrl_canonical_reference.lml",
    ] {
        LmlParser::parse_file(path).unwrap_or_else(|err| panic!("{path} failed: {err}"));
    }
}

#[test]
fn repository_memory_examples_share_one_ledger() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    for path in [
        "memories/cms_hybrid_architecture.lml",
        "memories/usrl_canonical_reference.lml",
    ] {
        let memory = LmlParser::parse_file(path).unwrap();
        ledger.upsert_memory(&memory, None).unwrap();
    }

    let results = ledger.search_exact("USRL", 10).unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn archive_export_restore_reconstructs_active_memory_and_indexes() {
    let mut source = SqliteLedger::open_memory().unwrap();
    for path in [
        "memories/cms_hybrid_architecture.lml",
        "memories/usrl_canonical_reference.lml",
    ] {
        let memory = LmlParser::parse_file(path).unwrap();
        source
            .upsert_memory(&memory, Some(std::path::Path::new(path)))
            .unwrap();
        SqliteGraphIndex::new(&source)
            .upsert_memory(&memory)
            .unwrap();
        SqliteVectorIndex::new(&source)
            .upsert_memory(&memory)
            .unwrap();
    }

    let tempdir = tempfile::tempdir().unwrap();
    let archive_dir = tempdir.path().join("cms-archive");
    let manifest = export_archive(&source, &archive_dir).unwrap();

    assert_eq!(manifest.archive_version, 1);
    assert_eq!(manifest.cms_schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(manifest.lml_schema_version, CURRENT_LML_SCHEMA_VERSION);
    assert_eq!(manifest.memory_count, 2);
    assert!(archive_dir.join("manifest.json").exists());
    assert!(
        archive_dir
            .join("memories/mem_2026_05_15_cms_hybrid_architecture.lml")
            .exists()
    );
    assert_eq!(read_manifest(&archive_dir).unwrap().memory_count, 2);

    let mut restored = SqliteLedger::open_memory().unwrap();
    let report = restore_archive(&mut restored, &archive_dir).unwrap();

    assert_eq!(report.restored_memories, 2);
    assert_eq!(
        source.all_memory_ids().unwrap(),
        restored.all_memory_ids().unwrap()
    );
    let restored_memory = restored
        .get_memory("mem_2026_05_15_cms_hybrid_architecture")
        .unwrap()
        .unwrap();
    assert_eq!(
        restored_memory.title,
        "CMS should use hybrid memory substrates"
    );
    assert_eq!(restored.stats().unwrap().graph_indexed_memories, 2);
    assert_eq!(restored.stats().unwrap().vector_indexed_memories, 2);
    assert!(
        HybridRagOrchestrator::new(&restored)
            .retrieve("hybrid memory substrates", RetrievalMode::Hybrid, 5)
            .unwrap()
            .results
            .iter()
            .any(|result| result.memory.id == "mem_2026_05_15_cms_hybrid_architecture")
    );
}

#[test]
fn archive_restore_is_idempotent_for_unchanged_memories() {
    let mut source = SqliteLedger::open_memory().unwrap();
    let memory = LmlParser::parse_file("memories/cms_hybrid_architecture.lml").unwrap();
    source.upsert_memory(&memory, None).unwrap();

    let tempdir = tempfile::tempdir().unwrap();
    let archive_dir = tempdir.path().join("cms-archive");
    export_archive(&source, &archive_dir).unwrap();

    let mut restored = SqliteLedger::open_memory().unwrap();
    restore_archive(&mut restored, &archive_dir).unwrap();
    restore_archive(&mut restored, &archive_dir).unwrap();

    assert_eq!(restored.stats().unwrap().active_memories, 1);
    assert_eq!(
        restored
            .memory_versions("mem_2026_05_15_cms_hybrid_architecture")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn archive_export_can_exclude_private_and_redact_sensitive_memory_content() {
    let mut source = SqliteLedger::open_memory().unwrap();
    let mut public = scoped_memory(
        "mem_public_export",
        "Public export memory",
        "public",
        None,
        None,
    );
    public.summary = "Public information".to_string();
    public.body = "No sensitive content here".to_string();

    let mut private = scoped_memory(
        "mem_private_export",
        "Private export memory",
        "private",
        Some("alice"),
        Some("cms"),
    );
    private.body = "Private content should not be exported".to_string();

    let mut sensitive = scoped_memory(
        "mem_sensitive_export",
        "Sensitive export memory",
        "public",
        None,
        None,
    );
    sensitive.summary = "Contains password marker".to_string();
    sensitive.body = format!("api_key = {}", fake_openai_key_short());
    sensitive.claims.push(Claim {
        id: "claim-sensitive".to_string(),
        text: format!("access_token {}", fake_github_token_short()),
        confidence: 1.0,
        source: None,
    });
    sensitive
        .metadata
        .insert("secret_key".to_string(), "do-not-export".to_string());

    for memory in [&public, &private, &sensitive] {
        source.upsert_memory(memory, None).unwrap();
    }

    let tempdir = tempfile::tempdir().unwrap();
    let archive_dir = tempdir.path().join("redacted-archive");
    let manifest = export_archive_with_options(
        &source,
        &archive_dir,
        ArchiveExportOptions {
            scope_filter: None,
            redaction_policy: ArchiveRedactionPolicy {
                exclude_private: true,
                redact_sensitive: true,
            },
        },
    )
    .unwrap();

    assert_eq!(manifest.memory_count, 2);
    assert_eq!(
        manifest.redaction_policy,
        Some(ArchiveRedactionPolicy {
            exclude_private: true,
            redact_sensitive: true
        })
    );
    assert!(!archive_dir.join("memories/mem_private_export.lml").exists());

    let sensitive_lml =
        std::fs::read_to_string(archive_dir.join("memories/mem_sensitive_export.lml")).unwrap();
    assert!(!sensitive_lml.contains(&fake_openai_key_short()));
    assert!(!sensitive_lml.contains(&fake_github_token_short()));
    assert!(!sensitive_lml.contains("do-not-export"));

    let exported_sensitive =
        LmlParser::parse_file(archive_dir.join("memories/mem_sensitive_export.lml")).unwrap();
    assert_eq!(
        exported_sensitive.body,
        "[redacted sensitive content]".to_string()
    );
    assert_eq!(
        exported_sensitive.claims[0].text,
        "[redacted sensitive content]".to_string()
    );
    assert_eq!(exported_sensitive.metadata["archive_redacted"], "true");
    assert_eq!(
        exported_sensitive.metadata["secret_key"],
        "[redacted sensitive content]"
    );
}

#[test]
fn json_export_can_scope_exclude_private_and_redact_sensitive_content() {
    let mut source = SqliteLedger::open_memory().unwrap();
    let mut public = scoped_memory(
        "mem_json_public_export",
        "JSON public export",
        "public",
        Some("alice"),
        Some("cms"),
    );
    public.body = "Public CMS export content".to_string();

    let mut private = scoped_memory(
        "mem_json_private_export",
        "JSON private export",
        "private",
        Some("alice"),
        Some("cms"),
    );
    private.body = "Private CMS export content".to_string();

    let mut sensitive = scoped_memory(
        "mem_json_sensitive_export",
        "JSON sensitive export",
        "public",
        Some("alice"),
        Some("cms"),
    );
    sensitive.body = format!("password = {}", fake_openai_password_value());

    for memory in [&public, &private, &sensitive] {
        source.upsert_memory(memory, None).unwrap();
    }

    let tempdir = tempfile::tempdir().unwrap();
    let output = tempdir.path().join("cms-export.json");
    let export = export_json_with_options(
        &source,
        &output,
        ArchiveExportOptions {
            scope_filter: Some(ArchiveScopeFilter {
                visibility: None,
                user_id: Some("alice".to_string()),
                project_id: Some("cms".to_string()),
            }),
            redaction_policy: ArchiveRedactionPolicy {
                exclude_private: true,
                redact_sensitive: true,
            },
        },
    )
    .unwrap();

    assert_eq!(export.memory_count, 2);
    assert_eq!(
        export.scope_filter.as_ref().unwrap().user_id.as_deref(),
        Some("alice")
    );
    assert!(
        export
            .memories
            .iter()
            .all(|memory| memory.id != "mem_json_private_export")
    );
    let sensitive = export
        .memories
        .iter()
        .find(|memory| memory.id == "mem_json_sensitive_export")
        .unwrap();
    assert_eq!(sensitive.body, "[redacted sensitive content]");
    assert_eq!(sensitive.metadata["archive_redacted"], "true");

    let exported_json: Value = serde_json::from_str(&fs::read_to_string(output).unwrap()).unwrap();
    assert_eq!(exported_json["memory_count"], 2);
    assert_eq!(exported_json["redaction_policy"]["exclude_private"], true);
    assert!(
        !serde_json::to_string(&exported_json)
            .unwrap()
            .contains(&fake_openai_password_value())
    );
}

#[test]
fn json_export_restore_reconstructs_memory_and_indexes() {
    let mut source = SqliteLedger::open_memory().unwrap();
    let memory = LmlParser::parse_file("memories/cms_hybrid_architecture.lml").unwrap();
    source.upsert_memory(&memory, None).unwrap();

    let tempdir = tempfile::tempdir().unwrap();
    let output = tempdir.path().join("cms-export.json");
    export_json_with_options(&source, &output, ArchiveExportOptions::default()).unwrap();

    let mut restored = SqliteLedger::open_memory().unwrap();
    let report = restore_json_export(&mut restored, &output).unwrap();

    assert_eq!(report.restored_memories, 1);
    assert_eq!(
        report.memory_ids,
        vec!["mem_2026_05_15_cms_hybrid_architecture".to_string()]
    );
    assert_eq!(restored.stats().unwrap().graph_indexed_memories, 1);
    assert_eq!(restored.stats().unwrap().vector_indexed_memories, 1);
    assert!(
        HybridRagOrchestrator::new(&restored)
            .retrieve("hybrid memory substrates", RetrievalMode::Hybrid, 5)
            .unwrap()
            .results
            .iter()
            .any(|result| result.memory.id == "mem_2026_05_15_cms_hybrid_architecture")
    );
}

#[test]
fn json_restore_is_idempotent_for_unchanged_memories() {
    let mut source = SqliteLedger::open_memory().unwrap();
    let memory = LmlParser::parse_file("memories/cms_hybrid_architecture.lml").unwrap();
    source.upsert_memory(&memory, None).unwrap();

    let tempdir = tempfile::tempdir().unwrap();
    let output = tempdir.path().join("cms-export.json");
    export_json_with_options(&source, &output, ArchiveExportOptions::default()).unwrap();

    let mut restored = SqliteLedger::open_memory().unwrap();
    restore_json_export(&mut restored, &output).unwrap();
    restore_json_export(&mut restored, &output).unwrap();

    assert_eq!(restored.stats().unwrap().active_memories, 1);
    assert_eq!(
        restored
            .memory_versions("mem_2026_05_15_cms_hybrid_architecture")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn maintenance_reports_unindexed_lml_files() {
    let ledger = SqliteLedger::open_memory().unwrap();
    let engine = LmlMaintenanceEngine::new(&ledger, "memories");

    let report = engine.run_full_check().unwrap();

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.id == "missing-ledger-record")
    );
}

#[test]
fn maintenance_reports_ledger_records_pointing_to_missing_lml_files() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("memory.lml");
    fs::write(&path, SAMPLE).unwrap();

    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memory = LmlParser::parse_file(&path).unwrap();
    ledger.upsert_memory(&memory, Some(&path)).unwrap();
    SqliteGraphIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();
    SqliteVectorIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();

    fs::remove_file(&path).unwrap();
    let report = LmlMaintenanceEngine::new(&ledger, tempdir.path())
        .run_full_check()
        .unwrap();

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.id == "missing-lml-file")
    );
}

#[test]
fn maintenance_is_clean_after_repository_examples_are_ingested() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    for path in [
        "memories/cms_hybrid_architecture.lml",
        "memories/usrl_canonical_reference.lml",
    ] {
        let memory = LmlParser::parse_file(path).unwrap();
        ledger.upsert_memory(&memory, None).unwrap();
        SqliteGraphIndex::new(&ledger)
            .upsert_memory(&memory)
            .unwrap();
        SqliteVectorIndex::new(&ledger)
            .upsert_memory(&memory)
            .unwrap();
    }
    let engine = LmlMaintenanceEngine::new(&ledger, "memories");

    let report = engine.run_full_check().unwrap();

    assert!(report.is_clean(), "{:#?}", report.issues);
}

#[test]
fn rag_retrieval_builds_context_from_ledger_results() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memory = LmlParser::parse_text(SAMPLE).unwrap();
    ledger.upsert_memory(&memory, None).unwrap();
    let rag = HybridRagOrchestrator::new(&ledger);

    let bundle = rag.retrieve("ledger", RetrievalMode::Hybrid, 5).unwrap();

    assert_eq!(bundle.query, "ledger");
    assert_eq!(bundle.results.len(), 1);
    assert!(bundle.context.contains("id: mem_test"));
    assert!(bundle.context.contains("claims:"));
    assert_eq!(bundle.trace["mode"], "Hybrid");
    assert_eq!(bundle.trace["result_count"], 1);
    assert_eq!(bundle.trace["selected_memory_ids"][0], "mem_test");
    assert_eq!(bundle.trace["results"][0]["memory_id"], "mem_test");
    assert!(
        bundle.trace["results"][0]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason.as_str().unwrap().contains("sqlite exact"))
    );
    assert!(
        bundle.trace["results"][0]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                .as_str()
                .unwrap()
                .starts_with("hybrid contribution mode=exact"))
    );
    assert_eq!(bundle.trace["results"][0]["contributing_modes"][0], "exact");
    assert_eq!(bundle.trace["mode_contribution_counts"]["exact"], 1);
}

#[test]
fn graph_projection_finds_memories_connected_by_shared_entities() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    for path in [
        "memories/cms_hybrid_architecture.lml",
        "memories/usrl_canonical_reference.lml",
    ] {
        let memory = LmlParser::parse_file(path).unwrap();
        ledger.upsert_memory(&memory, None).unwrap();
        SqliteGraphIndex::new(&ledger)
            .upsert_memory(&memory)
            .unwrap();
    }

    let hits = SqliteGraphIndex::new(&ledger)
        .related_memories("mem_2026_05_16_usrl_canonical_reference", 2)
        .unwrap();

    assert!(
        hits.iter()
            .any(|hit| hit.memory_id == "mem_2026_05_15_cms_hybrid_architecture")
    );
}

#[test]
fn hybrid_rag_expands_exact_results_through_graph() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    for path in [
        "memories/cms_hybrid_architecture.lml",
        "memories/usrl_canonical_reference.lml",
    ] {
        let memory = LmlParser::parse_file(path).unwrap();
        ledger.upsert_memory(&memory, None).unwrap();
        SqliteGraphIndex::new(&ledger)
            .upsert_memory(&memory)
            .unwrap();
    }
    let rag = HybridRagOrchestrator::new(&ledger);

    let bundle = rag.retrieve("JSRT", RetrievalMode::Hybrid, 10).unwrap();

    assert!(
        bundle
            .results
            .iter()
            .any(|result| result.memory.id == "mem_2026_05_16_usrl_canonical_reference")
    );
    assert!(
        bundle
            .results
            .iter()
            .any(|result| result.memory.id == "mem_2026_05_15_cms_hybrid_architecture")
    );
}

#[test]
fn vector_projection_finds_memory_chunks_by_token_overlap() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memory = LmlParser::parse_file("memories/usrl_canonical_reference.lml").unwrap();
    ledger.upsert_memory(&memory, None).unwrap();
    let vector = SqliteVectorIndex::new(&ledger);
    vector.upsert_memory(&memory).unwrap();

    let hits = vector
        .semantic_search("deterministic contract transport", 10)
        .unwrap();

    assert!(!hits.is_empty());
    assert_eq!(hits[0].memory_id, "mem_2026_05_16_usrl_canonical_reference");
}

#[test]
fn semantic_rag_uses_vector_projection() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    for path in [
        "memories/cms_hybrid_architecture.lml",
        "memories/usrl_canonical_reference.lml",
    ] {
        let memory = LmlParser::parse_file(path).unwrap();
        ledger.upsert_memory(&memory, None).unwrap();
        SqliteVectorIndex::new(&ledger)
            .upsert_memory(&memory)
            .unwrap();
    }
    let rag = HybridRagOrchestrator::new(&ledger);

    let bundle = rag
        .retrieve(
            "deterministic contract transport",
            RetrievalMode::Semantic,
            10,
        )
        .unwrap();

    assert!(
        bundle
            .results
            .iter()
            .any(|result| result.memory.id == "mem_2026_05_16_usrl_canonical_reference")
    );
}

#[test]
fn hybrid_rag_trace_records_exact_and_semantic_contributions() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    for path in [
        "memories/cms_hybrid_architecture.lml",
        "memories/usrl_canonical_reference.lml",
    ] {
        let memory = LmlParser::parse_file(path).unwrap();
        ledger.upsert_memory(&memory, None).unwrap();
        SqliteVectorIndex::new(&ledger)
            .upsert_memory(&memory)
            .unwrap();
    }
    let rag = HybridRagOrchestrator::new(&ledger);

    let bundle = rag
        .retrieve(
            "deterministic contract transport",
            RetrievalMode::Hybrid,
            10,
        )
        .unwrap();

    let usrl_result = bundle
        .trace
        .get("results")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|result| result["memory_id"] == "mem_2026_05_16_usrl_canonical_reference")
        .unwrap();
    let modes = usrl_result["contributing_modes"].as_array().unwrap();
    assert!(modes.iter().any(|mode| mode == "semantic"));
    assert!(
        usrl_result["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                .as_str()
                .unwrap()
                .starts_with("hybrid contribution mode=semantic"))
    );
    assert!(
        bundle.trace["mode_contribution_counts"]["semantic"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn contradiction_rag_prioritizes_explicit_conflict_links() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let mut baseline = MemoryObject::new("decision", "Cache TTL policy");
    baseline.id = "mem_cache_ttl_policy".to_string();
    baseline.summary = "Cache entries use a five minute TTL for API responses.".to_string();
    baseline.body = baseline.summary.clone();
    baseline.tags.push("cache".to_string());
    baseline.claims.push(Claim {
        id: "claim_cache_ttl".to_string(),
        text: "API cache entries expire after five minutes.".to_string(),
        confidence: 0.9,
        source: None,
    });

    let mut conflict = MemoryObject::new("decision", "Cache TTL override rejected");
    conflict.id = "mem_cache_ttl_conflict".to_string();
    conflict.summary =
        "Cache entries must not use a five minute TTL for regulated data.".to_string();
    conflict.body = conflict.summary.clone();
    conflict.tags.push("cache".to_string());
    conflict.claims.push(Claim {
        id: "claim_cache_ttl_conflict".to_string(),
        text: "Regulated API cache entries cannot expire after five minutes.".to_string(),
        confidence: 0.95,
        source: None,
    });
    conflict.links.push(MemoryLink {
        source_id: conflict.id.clone(),
        target_id: baseline.id.clone(),
        relation: "conflicts_with".to_string(),
        confidence: 0.97,
    });

    ledger.upsert_memory(&baseline, None).unwrap();
    ledger.upsert_memory(&conflict, None).unwrap();
    SqliteGraphIndex::new(&ledger)
        .upsert_memory(&baseline)
        .unwrap();
    SqliteGraphIndex::new(&ledger)
        .upsert_memory(&conflict)
        .unwrap();
    SqliteVectorIndex::new(&ledger)
        .upsert_memory(&baseline)
        .unwrap();
    SqliteVectorIndex::new(&ledger)
        .upsert_memory(&conflict)
        .unwrap();

    let bundle = HybridRagOrchestrator::new(&ledger)
        .retrieve("cache TTL", RetrievalMode::Contradiction, 5)
        .unwrap();

    assert_eq!(bundle.results.len(), 1);
    assert_eq!(bundle.results[0].memory.id, "mem_cache_ttl_conflict");
    assert!(
        bundle.results[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("explicit contradiction link"))
    );
    assert!(
        !bundle.results[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("fallback"))
    );
}

#[test]
fn recent_rag_returns_updated_memories_without_exact_query_match() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let mut older = MemoryObject::new("note", "Older operational note");
    older.id = "mem_recent_older".to_string();
    older.summary = "Background context.".to_string();
    older.updated_at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

    let mut newer = MemoryObject::new("note", "Newer operational note");
    newer.id = "mem_recent_newer".to_string();
    newer.summary = "Latest context.".to_string();
    newer.updated_at = Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap();

    ledger.upsert_memory(&older, None).unwrap();
    ledger.upsert_memory(&newer, None).unwrap();

    let bundle = HybridRagOrchestrator::new(&ledger)
        .retrieve("", RetrievalMode::Recent, 2)
        .unwrap();

    assert_eq!(bundle.results[0].memory.id, "mem_recent_newer");
    assert_eq!(bundle.results[1].memory.id, "mem_recent_older");
    assert!(
        bundle.results[0]
            .reasons
            .contains(&"recent ledger match".to_string())
    );
}

#[test]
fn project_rag_uses_project_links_and_graph_expansion() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let mut project = MemoryObject::new("project-note", "CMS shard plan");
    project.id = "mem_project_cms_plan".to_string();
    project.summary = "CMS project planning record.".to_string();
    project.tags.push("project".to_string());
    project
        .metadata
        .insert("project".to_string(), "CMS".to_string());
    project.links.push(MemoryLink {
        source_id: project.id.clone(),
        target_id: "mem_project_cms_decision".to_string(),
        relation: "depends_on".to_string(),
        confidence: 0.9,
    });

    let mut decision = MemoryObject::new("decision", "CMS storage decision");
    decision.id = "mem_project_cms_decision".to_string();
    decision.summary = "CMS uses SQLite as the operational ledger.".to_string();

    ledger.upsert_memory(&project, None).unwrap();
    ledger.upsert_memory(&decision, None).unwrap();
    SqliteGraphIndex::new(&ledger)
        .upsert_memory(&project)
        .unwrap();
    SqliteGraphIndex::new(&ledger)
        .upsert_memory(&decision)
        .unwrap();

    let bundle = HybridRagOrchestrator::new(&ledger)
        .retrieve("CMS", RetrievalMode::Project, 5)
        .unwrap();

    assert!(
        bundle
            .results
            .iter()
            .any(|result| result.memory.id == "mem_project_cms_plan")
    );
    assert!(
        bundle
            .results
            .iter()
            .any(|result| result.memory.id == "mem_project_cms_decision")
    );
    assert!(
        bundle
            .results
            .iter()
            .flat_map(|result| &result.reasons)
            .all(|reason| !reason.contains("fallback"))
    );
}

#[test]
fn decision_history_rag_returns_decisions_chronologically() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let mut old_decision = MemoryObject::new("decision", "CMS selects LML");
    old_decision.id = "mem_decision_old".to_string();
    old_decision.summary = "CMS records human-facing artifacts as LML.".to_string();
    old_decision.updated_at = Utc.with_ymd_and_hms(2026, 1, 5, 0, 0, 0).unwrap();
    old_decision.tags.push("decision".to_string());

    let mut new_decision = MemoryObject::new("decision", "CMS adds graph projection");
    new_decision.id = "mem_decision_new".to_string();
    new_decision.summary = "CMS adds a graph projection for relationship traversal.".to_string();
    new_decision.updated_at = Utc.with_ymd_and_hms(2026, 2, 5, 0, 0, 0).unwrap();
    new_decision.tags.push("decision".to_string());

    let mut note = MemoryObject::new("note", "CMS implementation note");
    note.id = "mem_decision_note".to_string();
    note.summary = "CMS note that is not a decision.".to_string();
    note.updated_at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

    ledger.upsert_memory(&new_decision, None).unwrap();
    ledger.upsert_memory(&note, None).unwrap();
    ledger.upsert_memory(&old_decision, None).unwrap();

    let bundle = HybridRagOrchestrator::new(&ledger)
        .retrieve("CMS", RetrievalMode::DecisionHistory, 5)
        .unwrap();

    let ids = bundle
        .results
        .iter()
        .map(|result| result.memory.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["mem_decision_old", "mem_decision_new"]);
    assert!(
        bundle
            .results
            .iter()
            .all(|result| result.reasons == vec!["decision-history ledger match"])
    );
}

#[test]
fn reindexer_rebuilds_graph_and_vector_projections_from_ledger() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    for path in [
        "memories/cms_hybrid_architecture.lml",
        "memories/usrl_canonical_reference.lml",
    ] {
        let memory = LmlParser::parse_file(path).unwrap();
        ledger.upsert_memory(&memory, None).unwrap();
    }

    let report = LmlMaintenanceEngine::new(&ledger, "memories")
        .run_full_check()
        .unwrap();
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.id == "missing-derived-index")
    );

    let count = Reindexer::new(&ledger).reindex_all().unwrap();
    assert_eq!(count, 2);

    let report = LmlMaintenanceEngine::new(&ledger, "memories")
        .run_full_check()
        .unwrap();
    assert!(report.is_clean(), "{:#?}", report.issues);

    let vector_hits = SqliteVectorIndex::new(&ledger)
        .semantic_search("deterministic contract transport", 10)
        .unwrap();
    assert!(!vector_hits.is_empty());

    let graph_hits = SqliteGraphIndex::new(&ledger)
        .related_memories("mem_2026_05_16_usrl_canonical_reference", 2)
        .unwrap();
    assert!(!graph_hits.is_empty());
}

#[test]
fn maintenance_detects_stale_vector_provider_version_hash() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("cms.sqlite3");
    let memory_path = tempdir.path().join("memory.lml");
    fs::write(&memory_path, SAMPLE).unwrap();
    let memory = LmlParser::parse_file(&memory_path).unwrap();
    let mut ledger = SqliteLedger::open(&db_path).unwrap();
    ledger.upsert_memory(&memory, Some(&memory_path)).unwrap();
    SqliteGraphIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();
    SqliteVectorIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();
    Connection::open(&db_path)
        .unwrap()
        .execute(
            "UPDATE index_state SET content_hash = ?1 WHERE memory_id = ?2 AND index_name = 'sqlite-vector'",
            ["legacy-vector-provider-hash", memory.id.as_str()],
        )
        .unwrap();

    let report = LmlMaintenanceEngine::new(&ledger, tempdir.path())
        .run_full_check()
        .unwrap();

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.id == "stale-derived-index"
                && issue.message.contains("sqlite-vector"))
    );
}

#[test]
fn maintenance_repairer_repairs_missing_derived_indexes_and_audits() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    for path in [
        "memories/cms_hybrid_architecture.lml",
        "memories/usrl_canonical_reference.lml",
    ] {
        let memory = LmlParser::parse_file(path).unwrap();
        ledger.upsert_memory(&memory, None).unwrap();
    }

    let before = LmlMaintenanceEngine::new(&ledger, "memories")
        .run_full_check()
        .unwrap();
    assert!(
        before
            .issues
            .iter()
            .any(|issue| issue.id == "missing-derived-index")
    );

    let repair = MaintenanceRepairer::new(&ledger, "memories")
        .repair_derived_indexes()
        .unwrap();

    assert_eq!(repair.skipped_issues.len(), 0);
    assert_eq!(repair.repaired_issues, 4);
    assert_eq!(
        repair.reindexed_memories,
        vec![
            "mem_2026_05_15_cms_hybrid_architecture".to_string(),
            "mem_2026_05_16_usrl_canonical_reference".to_string()
        ]
    );
    assert!(
        LmlMaintenanceEngine::new(&ledger, "memories")
            .run_full_check()
            .unwrap()
            .is_clean()
    );
    assert!(
        ledger
            .audit_events(10)
            .unwrap()
            .iter()
            .any(|event| event.event_type == "maintenance.repaired")
    );
}

#[test]
fn ledger_records_versions_only_when_memory_content_changes() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let mut memory = LmlParser::parse_text(SAMPLE).unwrap();

    ledger.upsert_memory(&memory, None).unwrap();
    ledger.upsert_memory(&memory, None).unwrap();
    assert_eq!(ledger.memory_versions("mem_test").unwrap().len(), 1);

    memory.summary.push_str("\nAdditional durable context.");
    ledger.upsert_memory(&memory, None).unwrap();

    let versions = ledger.memory_versions("mem_test").unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].version, 1);
    assert_eq!(versions[1].version, 2);
}

#[test]
fn ledger_versions_track_structural_memory_changes() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let mut memory = LmlParser::parse_text(SAMPLE).unwrap();

    ledger.upsert_memory(&memory, None).unwrap();

    memory.claims.push(Claim {
        id: "claim_002".to_string(),
        text: "Ledger fingerprints include claims.".to_string(),
        confidence: 0.91,
        source: Some("unit-test".to_string()),
    });
    ledger.upsert_memory(&memory, None).unwrap();

    memory.links.push(MemoryLink {
        source_id: memory.id.clone(),
        target_id: "concept:hash-fingerprint".to_string(),
        relation: "mentions".to_string(),
        confidence: 0.8,
    });
    memory.tags.push("fingerprint".to_string());
    memory
        .metadata
        .insert("hash_scope".to_string(), "structural".to_string());
    ledger.upsert_memory(&memory, None).unwrap();

    let versions = ledger.memory_versions("mem_test").unwrap();
    assert_eq!(versions.len(), 3);
    assert_eq!(versions[1].version, 2);
    assert_eq!(versions[2].version, 3);
}

#[test]
fn content_hash_is_stable_for_unordered_structural_fields() {
    let mut first = LmlParser::parse_text(SAMPLE).unwrap();
    first.tags = vec!["RAG".to_string(), "CMS".to_string()];
    first.metadata.insert("b".to_string(), "2".to_string());
    first.metadata.insert("a".to_string(), "1".to_string());
    first.claims.push(Claim {
        id: "claim_002".to_string(),
        text: "Additional claim.".to_string(),
        confidence: 0.8,
        source: None,
    });

    let mut second = first.clone();
    second.tags.reverse();
    second.claims.reverse();

    assert_eq!(content_hash(&first), content_hash(&second));
}

#[test]
fn vector_index_hash_includes_provider_and_chunking_version() {
    let memory = LmlParser::parse_text(SAMPLE).unwrap();
    assert_ne!(content_hash(&memory), vector_index_hash(&memory));
    assert_eq!(vector_index_hash(&memory).len(), 64);
    assert_ne!(
        vector_index_hash(&memory),
        vector_index_hash_with_config(&memory, "external-embedding-v1", "default-chunks-v1")
    );
    assert_ne!(
        vector_index_hash(&memory),
        vector_index_hash_with_config(&memory, "cms-lexical-v1", "sentence-chunks-v2")
    );

    let embedding = DeterministicLexicalEmbedding;
    assert_eq!(embedding.provider_id(), "cms-lexical-v1");
    assert_eq!(embedding.chunking_version(), "default-chunks-v1");
    let first = embedding
        .embed_text("deterministic contract transport")
        .unwrap();
    let second = embedding
        .embed_text("deterministic contract transport")
        .unwrap();
    assert_eq!(first, second);
    assert!(!first.is_empty());
}

#[test]
fn sqlite_vector_index_can_stamp_custom_provider_identity() {
    let tempdir = tempfile::tempdir().unwrap();
    let memory_path = tempdir.path().join("memory.lml");
    fs::write(&memory_path, SAMPLE).unwrap();
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memory = LmlParser::parse_file(&memory_path).unwrap();
    ledger.upsert_memory(&memory, Some(&memory_path)).unwrap();

    SqliteVectorIndex::with_provider(&ledger, "external-embedding-v1", "paragraph-chunks-v2")
        .upsert_memory(&memory)
        .unwrap();
    let stored_hash = ledger
        .index_hash(&memory.id, "sqlite-vector")
        .unwrap()
        .unwrap();
    assert_eq!(
        stored_hash,
        vector_index_hash_with_config(&memory, "external-embedding-v1", "paragraph-chunks-v2")
    );
    assert_ne!(stored_hash, vector_index_hash(&memory));

    let hits =
        SqliteVectorIndex::with_provider(&ledger, "external-embedding-v1", "paragraph-chunks-v2")
            .semantic_search("hybrid memory substrate", 5)
            .unwrap();
    assert!(hits.iter().any(|hit| hit.memory_id == memory.id));

    let report = LmlMaintenanceEngine::new(&ledger, tempdir.path())
        .run_full_check()
        .unwrap();
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.id == "stale-derived-index"
                && issue.message.contains("sqlite-vector")),
        "{:#?}",
        report.issues
    );

    SqliteVectorIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();
    let stored_hash = ledger
        .index_hash(&memory.id, "sqlite-vector")
        .unwrap()
        .unwrap();
    assert_eq!(stored_hash, vector_index_hash(&memory));
}

#[test]
fn safety_detector_finds_and_redacts_secret_like_content() {
    let findings = detect_sensitive_content(&format!(
        "api_key = {} and token {}",
        fake_openai_key(),
        fake_github_token()
    ));

    assert!(
        findings
            .iter()
            .any(|finding| finding.kind == "openai-api-key")
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.kind == "github-token")
    );
    assert!(
        findings
            .iter()
            .filter(|finding| finding.kind.ends_with("token") || finding.kind.ends_with("api-key"))
            .all(|finding| finding.evidence.contains("..."))
    );
}

#[test]
fn maintenance_reports_stale_derived_index_after_structural_change() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("memory.lml");
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let mut memory = LmlParser::parse_text(SAMPLE).unwrap();

    fs::write(&path, LmlWriter::to_text(&memory).unwrap()).unwrap();
    ledger.upsert_memory(&memory, Some(&path)).unwrap();
    SqliteGraphIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();
    SqliteVectorIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();

    memory.claims.push(Claim {
        id: "claim_002".to_string(),
        text: "A new graph and vector relevant claim.".to_string(),
        confidence: 0.93,
        source: None,
    });
    fs::write(&path, LmlWriter::to_text(&memory).unwrap()).unwrap();
    ledger.upsert_memory(&memory, Some(&path)).unwrap();

    let report = LmlMaintenanceEngine::new(&ledger, tempdir.path())
        .run_full_check()
        .unwrap();

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.id == "stale-derived-index")
    );
}

#[test]
fn maintenance_reports_duplicate_memories_with_merge_suggestion() {
    let tempdir = tempfile::tempdir().unwrap();
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let mut canonical = LmlParser::parse_text(SAMPLE).unwrap();
    canonical.id = "mem_a_duplicate_canonical".to_string();
    canonical.title = "Canonical durable memory".to_string();
    canonical.links.clear();
    canonical.tags.clear();

    let mut duplicate = canonical.clone();
    duplicate.id = "mem_b_duplicate_candidate".to_string();
    duplicate.title = "Imported durable memory copy".to_string();

    ledger.upsert_memory(&canonical, None).unwrap();
    ledger.upsert_memory(&duplicate, None).unwrap();

    let report = LmlMaintenanceEngine::new(&ledger, tempdir.path())
        .run_full_check()
        .unwrap();

    let duplicate_issue = report
        .issues
        .iter()
        .find(|issue| issue.id == "duplicate-memory")
        .expect("expected duplicate-memory maintenance issue");
    assert_eq!(duplicate_issue.severity, "warning");
    assert!(
        duplicate_issue
            .message
            .contains("cms merge mem_b_duplicate_candidate mem_a_duplicate_canonical")
    );
}

#[test]
fn ledger_records_audit_events_for_mutation_and_retrieval() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memory = LmlParser::parse_text(SAMPLE).unwrap();

    ledger.upsert_memory(&memory, None).unwrap();
    let results = ledger.search_exact("ledger", 10).unwrap();
    ledger
        .log_retrieval(
            "ledger",
            &results
                .iter()
                .map(|result| result.memory.id.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap();

    let events = ledger.audit_events(10).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "memory.created")
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "retrieval.logged")
    );
}

#[test]
fn diagnostics_report_health_stats_migrations_and_audit() {
    let tempdir = tempfile::tempdir().unwrap();
    let lml_path = tempdir.path().join("memory.lml");
    fs::write(&lml_path, SAMPLE).unwrap();
    let memory = LmlParser::parse_file(&lml_path).unwrap();
    let mut ledger = SqliteLedger::open_memory().unwrap();
    ledger.upsert_memory(&memory, Some(&lml_path)).unwrap();
    SqliteGraphIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();
    SqliteVectorIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();
    ledger
        .log_retrieval("ledger", std::slice::from_ref(&memory.id))
        .unwrap();
    ledger
        .log_audit_event(
            Some(&memory.id),
            "trace.test",
            &format!(
                "diagnostic redaction should hide api_key = {}",
                fake_openai_key()
            ),
        )
        .unwrap();

    let report = run_diagnostics(&ledger, tempdir.path(), 5).unwrap();

    assert!(report.is_healthy());
    assert_eq!(report.health.maintenance_issues, 0);
    assert_eq!(report.health.schema_issues, 0);
    assert_eq!(report.stats.active_memories, 1);
    assert_eq!(report.stats.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(report.migrations.len(), 3);
    assert_eq!(report.schema.expected_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(report.schema.current_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(report.schema.applied_migrations, 3);
    assert!(report.schema.missing_versions.is_empty());
    assert!(report.schema.unexpected_versions.is_empty());
    assert!(report.schema.is_current);
    assert_eq!(report.prompt_cache.manifests, 0);
    assert_eq!(report.prompt_cache.capsules, 0);
    assert_eq!(report.prompt_cache.usage_records, 0);
    assert_eq!(report.prompt_cache.invalidations, 0);
    assert_eq!(report.observability.recent_audit_event_count, 3);
    assert_eq!(report.observability.retrieval_events, 1);
    assert_eq!(report.observability.redacted_audit_messages, 1);
    assert_eq!(
        report
            .observability
            .recent_audit_event_types
            .get("retrieval.logged"),
        Some(&1)
    );
    assert!(
        report
            .recent_audit_events
            .iter()
            .any(|event| event.message.contains("[REDACTED]"))
    );
    assert!(
        report
            .recent_audit_events
            .iter()
            .all(|event| !event.message.contains("sk-1234567890"))
    );
    assert!(
        report
            .recent_audit_events
            .iter()
            .any(|event| event.event_type == "retrieval.logged")
    );
}

#[test]
fn diagnostics_report_errors_on_schema_migration_drift() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("cms.sqlite3");
    let ledger = SqliteLedger::open(&db_path).unwrap();
    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "DELETE FROM schema_migrations WHERE version = ?1",
            [CURRENT_SCHEMA_VERSION - 1],
        )
        .unwrap();
    }

    let report = run_diagnostics(&ledger, tempdir.path(), 5).unwrap();

    assert_eq!(report.health.status, "error");
    assert_eq!(report.health.schema_issues, 1);
    assert_eq!(report.health.error_issues, 1);
    assert_eq!(report.schema.expected_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(report.schema.current_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(
        report.schema.missing_versions,
        vec![CURRENT_SCHEMA_VERSION - 1]
    );
    assert!(!report.schema.is_current);
}

#[test]
fn diagnostics_report_warns_on_maintenance_issues() {
    let tempdir = tempfile::tempdir().unwrap();
    let lml_path = tempdir.path().join("memory.lml");
    fs::write(&lml_path, SAMPLE).unwrap();
    let ledger = SqliteLedger::open_memory().unwrap();

    let report = run_diagnostics(&ledger, tempdir.path(), 5).unwrap();

    assert_eq!(report.health.status, "warning");
    assert_eq!(report.health.warning_issues, 1);
    assert_eq!(report.maintenance.issues[0].id, "missing-ledger-record");
}

#[test]
fn soft_delete_removes_memory_from_retrieval_but_keeps_audit_history() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memory = LmlParser::parse_text(SAMPLE).unwrap();
    ledger.upsert_memory(&memory, None).unwrap();
    SqliteGraphIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();
    SqliteVectorIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();

    assert!(ledger.soft_delete_memory("mem_test").unwrap());
    SqliteGraphIndex::new(&ledger)
        .delete_memory("mem_test")
        .unwrap();
    SqliteVectorIndex::new(&ledger)
        .delete_memory("mem_test")
        .unwrap();

    assert!(ledger.get_memory("mem_test").unwrap().is_none());
    assert!(ledger.search_exact("ledger", 10).unwrap().is_empty());
    assert!(
        SqliteVectorIndex::new(&ledger)
            .semantic_search("ledger", 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        HybridRagOrchestrator::new(&ledger)
            .retrieve("ledger", RetrievalMode::Hybrid, 10)
            .unwrap()
            .results
            .is_empty()
    );
    assert_eq!(ledger.memory_versions("mem_test").unwrap().len(), 1);
    assert!(
        ledger
            .audit_events(10)
            .unwrap()
            .iter()
            .any(|event| event.event_type == "memory.deleted")
    );
}

#[test]
fn restore_reactivates_deleted_memory_and_rebuilds_retrieval() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memory = LmlParser::parse_text(SAMPLE).unwrap();
    ledger.upsert_memory(&memory, None).unwrap();
    SqliteGraphIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();
    SqliteVectorIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();

    assert!(ledger.soft_delete_memory("mem_test").unwrap());
    SqliteGraphIndex::new(&ledger)
        .delete_memory("mem_test")
        .unwrap();
    SqliteVectorIndex::new(&ledger)
        .delete_memory("mem_test")
        .unwrap();

    assert!(ledger.restore_memory("mem_test").unwrap());
    assert!(Reindexer::new(&ledger).reindex_memory("mem_test").unwrap());

    assert!(ledger.get_memory("mem_test").unwrap().is_some());
    assert_eq!(ledger.search_exact("ledger", 10).unwrap().len(), 1);
    assert!(
        !SqliteVectorIndex::new(&ledger)
            .semantic_search("ledger", 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        !HybridRagOrchestrator::new(&ledger)
            .retrieve("ledger", RetrievalMode::Hybrid, 10)
            .unwrap()
            .results
            .is_empty()
    );
    assert!(
        ledger
            .audit_events(10)
            .unwrap()
            .iter()
            .any(|event| event.event_type == "memory.restored")
    );
}

#[test]
fn graph_traversal_ignores_deleted_neighbor_memories() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    for path in [
        "memories/cms_hybrid_architecture.lml",
        "memories/usrl_canonical_reference.lml",
    ] {
        let memory = LmlParser::parse_file(path).unwrap();
        ledger.upsert_memory(&memory, None).unwrap();
        SqliteGraphIndex::new(&ledger)
            .upsert_memory(&memory)
            .unwrap();
    }

    assert!(
        ledger
            .soft_delete_memory("mem_2026_05_15_cms_hybrid_architecture")
            .unwrap()
    );

    let hits = SqliteGraphIndex::new(&ledger)
        .related_memories("mem_2026_05_16_usrl_canonical_reference", 2)
        .unwrap();

    assert!(
        !hits
            .iter()
            .any(|hit| hit.memory_id == "mem_2026_05_15_cms_hybrid_architecture")
    );
}

#[test]
fn ledger_stats_report_lifecycle_and_projection_counts() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    for path in [
        "memories/cms_hybrid_architecture.lml",
        "memories/usrl_canonical_reference.lml",
    ] {
        let memory = LmlParser::parse_file(path).unwrap();
        ledger.upsert_memory(&memory, None).unwrap();
        SqliteGraphIndex::new(&ledger)
            .upsert_memory(&memory)
            .unwrap();
        SqliteVectorIndex::new(&ledger)
            .upsert_memory(&memory)
            .unwrap();
    }

    let stats = ledger.stats().unwrap();
    assert_eq!(stats.active_memories, 2);
    assert_eq!(stats.deleted_memories, 0);
    assert_eq!(stats.graph_indexed_memories, 2);
    assert_eq!(stats.vector_indexed_memories, 2);
    assert!(stats.graph_edges > 0);
    assert!(stats.vector_chunks > 0);

    ledger
        .soft_delete_memory("mem_2026_05_16_usrl_canonical_reference")
        .unwrap();
    let stats = ledger.stats().unwrap();
    assert_eq!(stats.active_memories, 1);
    assert_eq!(stats.deleted_memories, 1);
}

#[test]
fn lifecycle_transitions_archive_quarantine_supersede_and_restore() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let mut archived = LmlParser::parse_text(SAMPLE).unwrap();
    archived.id = "mem_archived".to_string();
    archived.title = "Archived memory".to_string();
    archived.links.clear();
    let mut quarantined = archived.clone();
    quarantined.id = "mem_quarantined".to_string();
    quarantined.title = "Quarantined memory".to_string();
    let mut superseded = archived.clone();
    superseded.id = "mem_superseded".to_string();
    superseded.title = "Superseded memory".to_string();
    let mut replacement = archived.clone();
    replacement.id = "mem_replacement".to_string();
    replacement.title = "Replacement memory".to_string();
    for memory in [&archived, &quarantined, &superseded, &replacement] {
        ledger.upsert_memory(memory, None).unwrap();
    }

    assert!(ledger.archive_memory("mem_archived").unwrap());
    assert!(ledger.quarantine_memory("mem_quarantined").unwrap());
    assert!(
        ledger
            .supersede_memory("mem_superseded", "mem_replacement")
            .unwrap()
    );

    let stats = ledger.stats().unwrap();
    assert_eq!(stats.active_memories, 1);
    assert_eq!(stats.archived_memories, 1);
    assert_eq!(stats.quarantined_memories, 1);
    assert_eq!(stats.superseded_memories, 1);
    assert!(
        ledger
            .search_exact("Archived memory", 10)
            .unwrap()
            .is_empty()
    );

    assert_eq!(
        ledger
            .list_memories(MemoryStatusFilter::Inactive, 10)
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        ledger
            .list_memories(MemoryStatusFilter::Archived, 10)
            .unwrap()[0]
            .status,
        "archived"
    );
    assert_eq!(
        ledger
            .list_memories(MemoryStatusFilter::Quarantined, 10)
            .unwrap()[0]
            .status,
        "quarantined"
    );
    assert_eq!(
        ledger
            .list_memories(MemoryStatusFilter::Superseded, 10)
            .unwrap()[0]
            .status,
        "superseded"
    );

    assert!(ledger.restore_memory("mem_archived").unwrap());
    assert_eq!(ledger.stats().unwrap().archived_memories, 0);
    assert_eq!(ledger.stats().unwrap().active_memories, 2);
    assert!(
        ledger
            .audit_events(20)
            .unwrap()
            .iter()
            .any(|event| event.event_type == "memory.superseded")
    );
}

#[test]
fn merge_memory_supersedes_duplicate_and_links_to_canonical() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let mut duplicate = LmlParser::parse_text(SAMPLE).unwrap();
    duplicate.id = "mem_duplicate".to_string();
    duplicate.title = "Duplicate memory".to_string();
    duplicate.links.clear();
    let mut canonical = duplicate.clone();
    canonical.id = "mem_canonical".to_string();
    canonical.title = "Canonical memory".to_string();
    ledger.upsert_memory(&duplicate, None).unwrap();
    ledger.upsert_memory(&canonical, None).unwrap();

    assert!(
        ledger
            .merge_memory("mem_duplicate", "mem_canonical")
            .unwrap()
    );
    assert_eq!(ledger.stats().unwrap().superseded_memories, 1);
    assert!(
        ledger
            .search_exact("Duplicate memory", 10)
            .unwrap()
            .is_empty()
    );
    let superseded = ledger
        .list_memories(MemoryStatusFilter::Superseded, 10)
        .unwrap();
    assert_eq!(superseded[0].id, "mem_duplicate");
    assert!(
        ledger
            .audit_events(10)
            .unwrap()
            .iter()
            .any(|event| event.event_type == "memory.merged")
    );

    assert!(ledger.restore_memory("mem_duplicate").unwrap());
    let restored = ledger.get_memory("mem_duplicate").unwrap().unwrap();
    assert!(
        restored
            .links
            .iter()
            .any(|link| link.relation == "merged_into" && link.target_id == "mem_canonical")
    );
}

#[test]
fn ledger_lists_memories_by_status() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    for path in [
        "memories/cms_hybrid_architecture.lml",
        "memories/usrl_canonical_reference.lml",
    ] {
        let memory = LmlParser::parse_file(path).unwrap();
        ledger
            .upsert_memory(&memory, Some(std::path::Path::new(path)))
            .unwrap();
    }

    ledger
        .soft_delete_memory("mem_2026_05_16_usrl_canonical_reference")
        .unwrap();

    let active = ledger
        .list_memories(MemoryStatusFilter::Active, 10)
        .unwrap();
    let deleted = ledger
        .list_memories(MemoryStatusFilter::Deleted, 10)
        .unwrap();
    let all = ledger.list_memories(MemoryStatusFilter::All, 10).unwrap();

    assert_eq!(active.len(), 1);
    assert_eq!(active[0].status, "active");
    assert_eq!(active[0].memory_type, "design-thought");
    assert!(active[0].lml_path.is_some());
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0].status, "deleted");
    assert_eq!(all.len(), 2);
}

fn usrl_validator_root() -> PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_dir
            .parent()
            .and_then(Path::parent)
            .expect("cms-v2 crate should be under components/cms-v2");
        workspace_root.join("components/usrl")
    })
    .clone()
}

fn usrl_fixture_path() -> PathBuf {
    usrl_validator_root().join("test-constitution.usrl")
}

#[test]
fn usrl_summarizer_extracts_canonical_declarations() {
    let source = r#"
contract TestConstitution {
  section TaintTracking {
    fact UserInput = untrusted("DELETE FROM users");
    rule SafeOperation {
      emit log("safe");
    }
    constraint WorkersCantDelegate {
      assert true;
    }
  }
}
"#;

    let summary = summarize_usrl(source);

    assert_eq!(summary.contracts, vec!["TestConstitution"]);
    assert_eq!(summary.sections, vec!["TaintTracking"]);
    assert_eq!(summary.facts, vec!["UserInput"]);
    assert_eq!(summary.rules, vec!["SafeOperation"]);
    assert_eq!(summary.constraints, vec!["WorkersCantDelegate"]);
}

#[test]
fn usrl_importer_creates_structured_memory() {
    let memory = import_usrl_file(usrl_fixture_path()).unwrap();

    assert_eq!(memory.memory_type, "usrl-source");
    assert!(memory.title.contains("TestConstitution"));
    assert!(memory.tags.contains(&"USRL".to_string()));
    assert!(
        memory
            .links
            .iter()
            .any(|link| link.target_id == "usrl-contract:TestConstitution")
    );
    assert!(
        memory
            .links
            .iter()
            .any(|link| link.target_id == "usrl-rule:SafeOperation")
    );
}

#[test]
fn usrl_authoritative_validation_bridge_marks_valid_fixture() {
    let path = usrl_fixture_path();
    let validator_root = usrl_validator_root();
    let validation = validate_usrl_file_with_authoritative_cli(&path, &validator_root).unwrap();

    assert_eq!(validation.status, UsrlValidationStatus::Valid);
    assert_eq!(validation.module_count, Some(1));
    assert!(validation.symbol_count.unwrap() >= 1);
    assert!(validation.reference_count.unwrap() >= 1);

    let memory = import_usrl_file_with_options(
        path,
        &UsrlImportOptions {
            validator_root: Some(validator_root),
            require_authoritative_validation: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(memory.metadata["usrl_validation_status"], "Valid");
    assert_eq!(memory.metadata["usrl_module_count"], "1");
    assert!(memory.metadata.contains_key("usrl_symbol_count"));
}

#[test]
fn usrl_authoritative_validation_bridge_rejects_invalid_when_required() {
    let tempdir = tempfile::tempdir().unwrap();
    let invalid_path = tempdir.path().join("invalid.usrl");
    fs::write(&invalid_path, "contract {").unwrap();

    let validator_root = usrl_validator_root();
    let validation =
        validate_usrl_file_with_authoritative_cli(&invalid_path, &validator_root).unwrap();
    assert_eq!(validation.status, UsrlValidationStatus::Invalid);
    assert!(!validation.issues.is_empty());

    let error = import_usrl_file_with_options(
        &invalid_path,
        &UsrlImportOptions {
            validator_root: Some(validator_root),
            require_authoritative_validation: true,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("authoritative USRL validation failed")
    );
}

#[test]
fn imported_usrl_memory_is_retrievable() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memory = import_usrl_text(
        r#"
contract AccessContract {
  section CapabilityEnforcement {
    rule AllowedAction {
      permit read_operation();
    }
  }
}
"#,
        "inline-test.usrl",
        "inline-test",
    );

    ledger.upsert_memory(&memory, None).unwrap();
    SqliteGraphIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();
    SqliteVectorIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();

    let bundle = HybridRagOrchestrator::new(&ledger)
        .retrieve("CapabilityEnforcement", RetrievalMode::Hybrid, 5)
        .unwrap();

    assert!(
        bundle
            .results
            .iter()
            .any(|result| result.memory.id == memory.id)
    );
}

#[test]
fn scoped_usrl_import_respects_cms_retrieval_filters() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memory = import_usrl_text_with_options(
        r#"
contract PrivateScopePolicy {
  section Personalization {
    rule UserOnlyPolicy {
      permit scoped_memory_access();
    }
  }
}
"#,
        "private-scope.usrl",
        "private-scope",
        &UsrlImportOptions {
            visibility: "private".to_string(),
            user_id: Some("alice".to_string()),
            project_id: Some("project-a".to_string()),
            ..Default::default()
        },
    );

    assert_eq!(memory.metadata["visibility"], "private");
    assert_eq!(memory.metadata["user_id"], "alice");
    assert_eq!(memory.metadata["project_id"], "project-a");

    ledger.upsert_memory(&memory, None).unwrap();
    SqliteGraphIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();
    SqliteVectorIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();

    let client = LocalCmsMemoryClient::new(&mut ledger);
    let mut alice_filters = cms_v2::cms_api::Metadata::new();
    alice_filters.insert("user_id".to_string(), Value::String("alice".to_string()));
    let alice_bundle = client
        .retrieve(RetrievalRequest {
            query: "PrivateScopePolicy".to_string(),
            project_id: Some(cms_v2::cms_api::ProjectId("project-a".to_string())),
            modes: vec![ApiRetrievalMode::Exact, ApiRetrievalMode::Semantic],
            memory_types: Vec::new(),
            limit: 5,
            graph_depth: 1,
            include_contradictions: false,
            filters: alice_filters,
        })
        .unwrap();
    assert!(
        alice_bundle
            .results
            .iter()
            .any(|result| result.memory.id.0 == memory.id)
    );

    let mut bob_filters = cms_v2::cms_api::Metadata::new();
    bob_filters.insert("user_id".to_string(), Value::String("bob".to_string()));
    let bob_bundle = client
        .retrieve(RetrievalRequest {
            query: "PrivateScopePolicy".to_string(),
            project_id: Some(cms_v2::cms_api::ProjectId("project-a".to_string())),
            modes: vec![ApiRetrievalMode::Exact, ApiRetrievalMode::Semantic],
            memory_types: Vec::new(),
            limit: 5,
            graph_depth: 1,
            include_contradictions: false,
            filters: bob_filters,
        })
        .unwrap();
    assert!(bob_bundle.results.is_empty());
}

#[test]
fn usrl_scope_resolution_generates_retrieval_filters_and_visibility_decisions() {
    let scope = ScopeResolution::for_user_project("alice", "project-a", MemoryVisibility::Private);
    let metadata = scope.to_metadata();
    assert_eq!(metadata["user_id"], "alice");
    assert_eq!(metadata["project_id"], "project-a");
    assert_eq!(metadata["visibility"], "private");

    let mut request = RetrievalRequest::new("scoped memory");
    scope.apply_to_retrieval_request(&mut request);
    assert_eq!(request.filters["user_id"], "alice");
    assert_eq!(request.project_id.unwrap().0, "project-a");

    let mut alice_private = cms_v2::cms_api::Metadata::new();
    alice_private.insert(
        "visibility".to_string(),
        Value::String("private".to_string()),
    );
    alice_private.insert("user_id".to_string(), Value::String("alice".to_string()));
    assert!(memory_visible_to_scope(&alice_private, &scope));

    let mut bob_private = cms_v2::cms_api::Metadata::new();
    bob_private.insert(
        "visibility".to_string(),
        Value::String("private".to_string()),
    );
    bob_private.insert("user_id".to_string(), Value::String("bob".to_string()));
    assert!(!memory_visible_to_scope(&bob_private, &scope));

    let no_memory = ScopeResolution::no_memory("alice", Some("project-a".to_string()));
    assert!(!memory_visible_to_scope(&alice_private, &no_memory));
}

#[test]
fn usrl_scope_policy_resolves_retrieval_and_writeback_decisions() {
    let policy = UsrlScopePolicy;
    let mut metadata = cms_v2::cms_api::Metadata::new();
    metadata.insert(
        "visibility".to_string(),
        Value::String("project".to_string()),
    );
    let retrieval = policy.retrieval_scope("alice", Some("project-a".to_string()), &metadata);
    assert_eq!(retrieval.user_id.as_deref(), Some("alice"));
    assert_eq!(retrieval.project_id.as_deref(), Some("project-a"));
    assert_eq!(retrieval.visibility, Some(MemoryVisibility::Project));
    assert!(!retrieval.no_memory);

    metadata.insert(
        "writeback_visibility".to_string(),
        Value::String("shared".to_string()),
    );
    let writeback = policy.writeback_decision("alice", Some("project-a".to_string()), &metadata);
    assert!(writeback.allowed);
    assert_eq!(writeback.scope.visibility, Some(MemoryVisibility::Shared));
    assert_eq!(writeback.metadata["visibility"], "shared");
    assert_eq!(writeback.metadata["user_id"], "alice");
    assert_eq!(writeback.metadata["project_id"], "project-a");
    assert_eq!(writeback.metadata["usrl_scope_policy"], "default-v1");

    metadata.insert("memory_mode".to_string(), Value::String("none".to_string()));
    let blocked = policy.writeback_decision("alice", Some("project-a".to_string()), &metadata);
    assert!(!blocked.allowed);
    assert!(blocked.scope.no_memory);
    assert!(blocked.metadata.is_empty());
}

#[test]
fn usrl_project_discovery_skips_generated_and_vendor_dirs() {
    let paths = usrl_paths(usrl_validator_root()).unwrap();

    assert!(
        paths
            .iter()
            .any(|path| path.ends_with("test-constitution.usrl"))
    );
    assert!(paths.iter().all(|path| {
        let path = path.to_string_lossy();
        !path.contains("/node_modules/") && !path.contains("/dist/")
    }));
}

#[test]
fn usrl_project_imports_into_ledger() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let mut count = 0usize;
    for path in usrl_paths(usrl_validator_root()).unwrap() {
        let memory = import_usrl_file(&path).unwrap();
        ledger.upsert_memory(&memory, Some(&path)).unwrap();
        SqliteGraphIndex::new(&ledger)
            .upsert_memory(&memory)
            .unwrap();
        SqliteVectorIndex::new(&ledger)
            .upsert_memory(&memory)
            .unwrap();
        count += 1;
    }

    assert!(count >= 1);
    let bundle = HybridRagOrchestrator::new(&ledger)
        .retrieve("DangerousOperation", RetrievalMode::Hybrid, 5)
        .unwrap();
    assert!(!bundle.results.is_empty());
}

#[test]
fn chatgpt_export_preview_counts_conversations_messages_and_memory_chunks() {
    let tempdir = tempfile::tempdir().unwrap();
    write_chatgpt_export(tempdir.path());
    let preview = preview_chatgpt_export(
        tempdir.path(),
        &ChatGptImportOptions {
            messages_per_memory: 2,
            max_chars_per_memory: 0,
        },
    )
    .unwrap();

    assert_eq!(preview.source_kind, "chatgpt-export");
    assert_eq!(preview.conversations, 2);
    assert_eq!(preview.messages, 4);
    assert_eq!(preview.memories, 3);
}

#[test]
fn chatgpt_export_imports_full_transcripts_as_chunked_memories() {
    let tempdir = tempfile::tempdir().unwrap();
    write_chatgpt_export(tempdir.path());
    let memories = import_chatgpt_export(
        tempdir.path().join("conversations.json"),
        &ChatGptImportOptions {
            messages_per_memory: 2,
            max_chars_per_memory: 0,
        },
    )
    .unwrap();

    assert_eq!(memories.len(), 3);
    assert!(
        memories
            .iter()
            .all(|memory| memory.id.starts_with("mem_chatgpt_"))
    );
    assert!(
        memories
            .iter()
            .all(|memory| memory.memory_type == "chatgpt-conversation")
    );
    assert!(
        memories
            .iter()
            .any(|memory| memory.body.contains("How should CMS import chats?"))
    );
    assert!(
        memories
            .iter()
            .any(|memory| memory.body.contains("Use chunked memory objects."))
    );
    assert!(
        memories
            .iter()
            .all(|memory| memory.metadata.contains_key("conversation_id"))
    );
}

#[test]
fn chatgpt_export_can_chunk_by_character_budget() {
    let tempdir = tempfile::tempdir().unwrap();
    write_chatgpt_export(tempdir.path());
    let options = ChatGptImportOptions {
        messages_per_memory: 10,
        max_chars_per_memory: 70,
    };

    let preview = preview_chatgpt_export(tempdir.path(), &options).unwrap();
    assert_eq!(preview.messages, 4);
    assert_eq!(preview.memories, 4);

    let memories = import_chatgpt_export(tempdir.path(), &options).unwrap();
    let cms_chunks = memories
        .iter()
        .filter(|memory| memory.metadata["conversation_id"] == "conv_cms_import")
        .collect::<Vec<_>>();
    assert_eq!(cms_chunks.len(), 3);
    assert!(cms_chunks.iter().all(|memory| {
        memory.metadata["chunk_count"] == "3" && memory.metadata["message_count"] == "1"
    }));
    assert!(
        memories
            .iter()
            .any(|memory| memory.body.contains("A separate conversation.")
                && memory.metadata["chunk_count"] == "1")
    );
}

#[test]
fn chatgpt_import_report_is_stable_and_idempotency_friendly() {
    let tempdir = tempfile::tempdir().unwrap();
    write_chatgpt_export(tempdir.path());
    let options = ChatGptImportOptions {
        messages_per_memory: 2,
        max_chars_per_memory: 0,
    };

    let first = report_chatgpt_export(tempdir.path(), &options).unwrap();
    let second = report_chatgpt_export(tempdir.path(), &options).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.importer_version, "chatgpt-export-v1");
    assert_eq!(first.generated_memories, 3);
    assert_eq!(first.unique_memories, 3);
    assert_eq!(first.duplicate_memories, 0);
    assert_eq!(first.memory_ids.len(), 3);
    assert_eq!(first.source_hashes.len(), 3);
    assert_eq!(first.import_batch_hash.len(), 64);
}

#[test]
fn chatgpt_reimport_of_unchanged_export_does_not_create_new_versions() {
    let tempdir = tempfile::tempdir().unwrap();
    write_chatgpt_export(tempdir.path());
    let options = ChatGptImportOptions {
        messages_per_memory: 2,
        max_chars_per_memory: 0,
    };
    let first = import_chatgpt_export(tempdir.path(), &options).unwrap();
    let second = import_chatgpt_export(tempdir.path(), &options).unwrap();
    let mut ledger = SqliteLedger::open_memory().unwrap();

    assert_eq!(
        first.iter().map(|memory| &memory.id).collect::<Vec<_>>(),
        second.iter().map(|memory| &memory.id).collect::<Vec<_>>()
    );

    for memory in first.iter().chain(second.iter()) {
        ledger
            .upsert_memory(
                memory,
                Some(tempdir.path().join("conversations.json").as_path()),
            )
            .unwrap();
    }

    assert_eq!(ledger.stats().unwrap().active_memories, first.len() as i64);
    for memory in &first {
        assert_eq!(ledger.memory_versions(&memory.id).unwrap().len(), 1);
    }
}

#[test]
fn imported_chatgpt_export_is_retrievable_after_ingest() {
    let tempdir = tempfile::tempdir().unwrap();
    write_chatgpt_export(tempdir.path());
    let memories = import_chatgpt_export(
        tempdir.path(),
        &ChatGptImportOptions {
            messages_per_memory: 10,
            max_chars_per_memory: 0,
        },
    )
    .unwrap();
    let mut ledger = SqliteLedger::open_memory().unwrap();
    for memory in &memories {
        ledger
            .upsert_memory(
                memory,
                Some(tempdir.path().join("conversations.json").as_path()),
            )
            .unwrap();
        SqliteGraphIndex::new(&ledger)
            .upsert_memory(memory)
            .unwrap();
        SqliteVectorIndex::new(&ledger)
            .upsert_memory(memory)
            .unwrap();
    }

    let bundle = HybridRagOrchestrator::new(&ledger)
        .retrieve("chunked memory objects", RetrievalMode::Hybrid, 5)
        .unwrap();

    assert!(
        bundle
            .results
            .iter()
            .any(|result| result.memory.memory_type == "chatgpt-conversation")
    );
}

#[test]
fn chatgpt_export_handles_non_text_parts_code_messages_and_null_nodes() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::write(
        tempdir.path().join("conversations.json"),
        r#"
[
  {
    "id": "conv_variant",
    "title": "Variant payloads",
    "create_time": null,
    "update_time": null,
    "mapping": {
      "root": { "message": null },
      "missing_content": {
        "message": {
          "id": "missing_content",
          "author": { "role": "assistant" },
          "create_time": 1770002001.0
        }
      },
      "tool_result": {
        "message": {
          "id": "tool_result",
          "author": { "role": "tool" },
          "create_time": 1770002002.0,
          "content": {
            "content_type": "text",
            "parts": ["tool output should not become memory text"]
          }
        }
      },
      "user_multimodal": {
        "message": {
          "id": "user_multimodal",
          "author": { "role": "user" },
          "create_time": 1770002003.0,
          "content": {
            "content_type": "multimodal_text",
            "parts": [
              "Remember the VEGVISIR variant fixture.",
              { "text": "Object text parts should be preserved." },
              { "image_url": "ignored" },
              42
            ]
          }
        }
      },
      "assistant_code": {
        "message": {
          "id": "assistant_code",
          "author": { "role": "assistant" },
          "create_time": 1770002004.0,
          "content": {
            "content_type": "code",
            "parts": ["fn variant_fixture() {}"]
          }
        }
      },
      "assistant_text_field": {
        "message": {
          "id": "assistant_text_field",
          "author": { "role": "assistant" },
          "create_time": 1770002005.0,
          "content": {
            "content_type": "text",
            "text": "Direct text fields should be imported."
          }
        }
      }
    }
  }
]
"#,
    )
    .unwrap();
    let options = ChatGptImportOptions {
        messages_per_memory: 10,
        max_chars_per_memory: 0,
    };

    let preview = preview_chatgpt_export(tempdir.path(), &options).unwrap();
    assert_eq!(preview.conversations, 1);
    assert_eq!(preview.messages, 3);
    assert_eq!(preview.memories, 1);

    let report = report_chatgpt_export(tempdir.path(), &options).unwrap();
    assert_eq!(report.generated_memories, 1);
    assert_eq!(report.unique_memories, 1);
    assert_eq!(report.duplicate_memories, 0);

    let memories = import_chatgpt_export(tempdir.path(), &options).unwrap();
    assert_eq!(memories.len(), 1);
    let memory = &memories[0];
    assert_eq!(memory.metadata["message_count"], "3");
    assert!(memory.body.contains("## user"));
    assert!(memory.body.contains("## assistant"));
    assert!(
        memory
            .body
            .contains("Remember the VEGVISIR variant fixture.")
    );
    assert!(
        memory
            .body
            .contains("Object text parts should be preserved.")
    );
    assert!(memory.body.contains("```\nfn variant_fixture() {}\n```"));
    assert!(
        memory
            .body
            .contains("Direct text fields should be imported.")
    );
    assert!(
        !memory
            .body
            .contains("tool output should not become memory text")
    );
    assert!(!memory.body.contains("ignored"));
}

#[test]
fn jsonl_import_reports_warnings_and_builds_structured_memories() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("records.jsonl");
    fs::write(
        &path,
        r#"{"title":"Preference","body":"Use SQLite backup before migration.","tags":["ops"],"metadata":{"visibility":"public","priority":2},"user_id":"alice","project_id":"cms"}
not json
{"title":"Missing Body"}
{"title":"Decision","content":"JSONL import should preserve provenance.","visibility":"private"}
"#,
    )
    .unwrap();
    let options = JsonlImportOptions::default();

    let preview = preview_jsonl_file(&path, &options).unwrap();
    assert_eq!(preview.memories, 2);
    assert_eq!(preview.messages, 2);

    let report = report_jsonl_file(&path, &options).unwrap();
    assert_eq!(report.importer_version, "jsonl-record-v1");
    assert_eq!(report.generated_memories, 2);
    assert_eq!(report.unique_memories, 2);
    assert_eq!(report.warnings.len(), 2);
    assert!(report.warnings[0].contains("line 2"));
    assert!(report.warnings[1].contains("line 3"));

    let memories = import_jsonl_file(&path, &options).unwrap();
    assert_eq!(memories.len(), 2);
    let preference = memories
        .iter()
        .find(|memory| memory.title == "Preference")
        .unwrap();
    assert_eq!(preference.memory_type, "jsonl-record");
    assert_eq!(preference.metadata["importer"], "jsonl-record-v1");
    assert_eq!(preference.metadata["user_id"], "alice");
    assert_eq!(preference.metadata["project_id"], "cms");
    assert_eq!(preference.metadata["visibility"], "public");
    assert_eq!(preference.metadata["priority"], "2");
    assert!(preference.tags.contains(&"jsonl-import".to_string()));
}

#[test]
fn imported_jsonl_records_are_retrievable_after_ingest() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("records.jsonl");
    fs::write(
        &path,
        r#"{"title":"Ops Decision","body":"Run SQLite backup before every release migration."}"#,
    )
    .unwrap();
    let memories = import_jsonl_file(&path, &JsonlImportOptions::default()).unwrap();
    let mut ledger = SqliteLedger::open_memory().unwrap();
    for memory in &memories {
        ledger.upsert_memory(memory, Some(&path)).unwrap();
        SqliteGraphIndex::new(&ledger)
            .upsert_memory(memory)
            .unwrap();
        SqliteVectorIndex::new(&ledger)
            .upsert_memory(memory)
            .unwrap();
    }

    let bundle = HybridRagOrchestrator::new(&ledger)
        .retrieve("release migration backup", RetrievalMode::Hybrid, 5)
        .unwrap();

    assert!(
        bundle
            .results
            .iter()
            .any(|result| result.memory.memory_type == "jsonl-record")
    );
}

#[test]
fn jsonl_reimport_of_unchanged_records_does_not_create_new_versions() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("records.jsonl");
    fs::write(
        &path,
        r#"{"title":"Stable Record","body":"JSONL reimport should be idempotent."}"#,
    )
    .unwrap();
    let options = JsonlImportOptions::default();
    let first = import_jsonl_file(&path, &options).unwrap();
    let second = import_jsonl_file(&path, &options).unwrap();
    let mut ledger = SqliteLedger::open_memory().unwrap();

    assert_eq!(
        first.iter().map(|memory| &memory.id).collect::<Vec<_>>(),
        second.iter().map(|memory| &memory.id).collect::<Vec<_>>()
    );
    for memory in first.iter().chain(second.iter()) {
        ledger.upsert_memory(memory, Some(&path)).unwrap();
    }

    assert_eq!(ledger.stats().unwrap().active_memories, 1);
    assert_eq!(ledger.memory_versions(&first[0].id).unwrap().len(), 1);
}

#[test]
fn large_jsonl_fixture_import_is_stable_retrievable_and_idempotent() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("large-records.jsonl");
    let mut source = String::new();
    for index in 0..64 {
        source.push_str(&format!(
            "{{\"title\":\"Large Fixture {index:03}\",\"body\":\"VEGVISIR_LARGE_FIXTURE_{index:03} should survive import and retrieval.\",\"tags\":[\"large-fixture\",\"vegvisir\"],\"metadata\":{{\"fixture_index\":\"{index:03}\"}}}}\n"
        ));
    }
    fs::write(&path, source).unwrap();
    let options = JsonlImportOptions::default();

    let preview = preview_jsonl_file(&path, &options).unwrap();
    assert_eq!(preview.memories, 64);
    assert_eq!(preview.messages, 64);

    let first_report = report_jsonl_file(&path, &options).unwrap();
    let second_report = report_jsonl_file(&path, &options).unwrap();
    assert_eq!(first_report, second_report);
    assert_eq!(first_report.generated_memories, 64);
    assert_eq!(first_report.unique_memories, 64);
    assert_eq!(first_report.duplicate_memories, 0);
    assert_eq!(first_report.memory_ids.len(), 64);
    assert_eq!(first_report.source_hashes.len(), 64);
    assert_eq!(first_report.import_batch_hash.len(), 64);

    let first = import_jsonl_file(&path, &options).unwrap();
    let second = import_jsonl_file(&path, &options).unwrap();
    assert_eq!(
        first.iter().map(|memory| &memory.id).collect::<Vec<_>>(),
        second.iter().map(|memory| &memory.id).collect::<Vec<_>>()
    );

    let mut ledger = SqliteLedger::open_memory().unwrap();
    for memory in first.iter().chain(second.iter()) {
        ledger.upsert_memory(memory, Some(&path)).unwrap();
        SqliteGraphIndex::new(&ledger)
            .upsert_memory(memory)
            .unwrap();
        SqliteVectorIndex::new(&ledger)
            .upsert_memory(memory)
            .unwrap();
    }
    assert_eq!(ledger.stats().unwrap().active_memories, 64);
    for memory in &first {
        assert_eq!(ledger.memory_versions(&memory.id).unwrap().len(), 1);
    }

    let bundle = HybridRagOrchestrator::new(&ledger)
        .retrieve("VEGVISIR_LARGE_FIXTURE_042", RetrievalMode::Hybrid, 5)
        .unwrap();
    assert!(bundle.results.iter().any(|result| {
        result.memory.title == "Large Fixture 042"
            && result.memory.body.contains("VEGVISIR_LARGE_FIXTURE_042")
    }));
}

#[test]
fn markdown_document_import_chunks_by_heading_and_preserves_code_blocks() {
    let tempdir = tempfile::tempdir().unwrap();
    let doc_path = tempdir.path().join("architecture.md");
    fs::write(
        &doc_path,
        r#"# CMS Import Notes

Introductory notes for the CMS import framework.

## Adapter

The markdown adapter preserves fenced code blocks.

```rust
fn import_document() {}
```

## Retrieval

Imported markdown should be available to RAG retrieval.
"#,
    )
    .unwrap();
    let options = DocumentImportOptions {
        max_chars_per_memory: 120,
        source_kind: "markdown".to_string(),
    };

    let preview = preview_document_file(&doc_path, &options).unwrap();
    assert_eq!(preview.source_kind, "markdown");
    assert_eq!(preview.memories, 3);

    let memories = import_document_file(&doc_path, &options).unwrap();
    assert_eq!(memories.len(), 3);
    assert!(memories.iter().all(|memory| {
        memory.memory_type == "markdown-document"
            && memory.metadata["importer"] == "generic-document-v1"
            && memory.metadata.contains_key("source_hash")
    }));
    assert!(memories.iter().any(|memory| {
        memory.body.contains("```rust") && memory.body.contains("fn import_document() {}")
    }));

    let repeat = import_document_file(&doc_path, &options).unwrap();
    assert_eq!(
        memories.iter().map(|memory| &memory.id).collect::<Vec<_>>(),
        repeat.iter().map(|memory| &memory.id).collect::<Vec<_>>()
    );
}

#[test]
fn document_directory_import_skips_generated_dirs_and_reads_frontmatter() {
    let tempdir = tempfile::tempdir().unwrap();
    let docs_dir = tempdir.path().join("docs");
    let node_modules = docs_dir.join("node_modules");
    fs::create_dir_all(&node_modules).unwrap();
    fs::write(
        docs_dir.join("guide.md"),
        r#"---
title: Scoped Import Guide
visibility: private
---
# Ignored Heading

Frontmatter should provide the title and metadata.
"#,
    )
    .unwrap();
    fs::write(
        docs_dir.join("module.rs"),
        "pub fn generic_document_directory_import() {}",
    )
    .unwrap();
    fs::write(node_modules.join("ignored.md"), "# Ignored").unwrap();

    let paths = document_paths(&docs_dir).unwrap();
    assert_eq!(paths.len(), 2);
    assert!(paths.iter().all(|path| {
        !path.to_string_lossy().contains(&format!(
            "{}node_modules{}",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        ))
    }));

    let options = DocumentImportOptions {
        max_chars_per_memory: 1_000,
        source_kind: "document".to_string(),
    };
    let preview = preview_document_tree(&docs_dir, &options).unwrap();
    assert_eq!(preview.memories, 2);

    let memories = import_document_tree(&docs_dir, &options).unwrap();
    assert_eq!(memories.len(), 2);
    let markdown = memories
        .iter()
        .find(|memory| memory.memory_type == "markdown-document")
        .unwrap();
    assert_eq!(markdown.title, "Scoped Import Guide");
    assert_eq!(
        markdown.metadata["frontmatter:title"],
        "Scoped Import Guide"
    );
    assert_eq!(markdown.metadata["frontmatter:visibility"], "private");
    assert!(markdown.body.contains("Frontmatter should provide"));
    assert!(!markdown.body.contains("visibility: private"));
    assert!(
        memories
            .iter()
            .any(|memory| memory.memory_type == "code-document")
    );

    let report = report_document_tree(&docs_dir, &options).unwrap();
    assert_eq!(report.importer_version, "generic-document-v1");
    assert_eq!(report.generated_memories, 2);
    assert_eq!(report.unique_memories, 2);
    assert_eq!(report.duplicate_memories, 0);
    assert_eq!(report.memory_ids.len(), 2);
    assert_eq!(report.source_hashes.len(), 2);
    assert_eq!(report.import_batch_hash.len(), 64);
}

#[test]
fn document_directory_report_skips_malformed_files_with_warning() {
    let tempdir = tempfile::tempdir().unwrap();
    let docs_dir = tempdir.path().join("docs");
    fs::create_dir_all(&docs_dir).unwrap();
    fs::write(docs_dir.join("good.md"), "# Good\n\nImport this document.").unwrap();
    fs::write(docs_dir.join("bad.txt"), [0xff, 0xfe, 0xfd]).unwrap();

    let options = DocumentImportOptions {
        max_chars_per_memory: 1_000,
        source_kind: "document".to_string(),
    };
    let (report, memories) = import_document_tree_with_report(&docs_dir, &options).unwrap();

    assert_eq!(memories.len(), 1);
    assert_eq!(report.generated_memories, 1);
    assert_eq!(report.unique_memories, 1);
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("bad.txt"));
    assert!(report.warnings[0].contains("skipped"));
}

#[test]
fn imported_markdown_document_is_retrievable_after_ingest() {
    let tempdir = tempfile::tempdir().unwrap();
    let doc_path = tempdir.path().join("retrieval.md");
    fs::write(
        &doc_path,
        "# Retrieval Fixture\n\nGeneric document imports should feed semantic RAG retrieval.",
    )
    .unwrap();
    let options = DocumentImportOptions {
        max_chars_per_memory: 1_000,
        source_kind: "markdown".to_string(),
    };
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memories = import_document_file(&doc_path, &options).unwrap();
    for memory in &memories {
        ledger.upsert_memory(memory, Some(&doc_path)).unwrap();
        SqliteGraphIndex::new(&ledger)
            .upsert_memory(memory)
            .unwrap();
        SqliteVectorIndex::new(&ledger)
            .upsert_memory(memory)
            .unwrap();
    }

    let bundle = HybridRagOrchestrator::new(&ledger)
        .retrieve("semantic RAG retrieval", RetrievalMode::Hybrid, 5)
        .unwrap();

    assert!(
        bundle
            .results
            .iter()
            .any(|result| result.memory.memory_type == "markdown-document")
    );
}

#[test]
fn cms_api_retrieves_through_stable_boundary() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memory = LmlParser::parse_file("memories/cms_hybrid_architecture.lml").unwrap();
    ledger.upsert_memory(&memory, None).unwrap();
    SqliteGraphIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();
    SqliteVectorIndex::new(&ledger)
        .upsert_memory(&memory)
        .unwrap();

    let client = LocalCmsMemoryClient::new(&mut ledger);
    let bundle = client
        .retrieve(RetrievalRequest {
            query: "hybrid memory substrates".to_string(),
            project_id: None,
            modes: vec![ApiRetrievalMode::Hybrid],
            memory_types: Vec::new(),
            limit: 5,
            graph_depth: 2,
            include_contradictions: false,
            filters: Default::default(),
        })
        .unwrap();

    assert_eq!(bundle.results.len(), 1);
    assert_eq!(
        bundle.results[0].memory.id.0,
        "mem_2026_05_15_cms_hybrid_architecture"
    );
    assert_eq!(bundle.results[0].source_mode, ApiRetrievalMode::Hybrid);
}

fn scoped_memory(
    id: &str,
    title: &str,
    visibility: &str,
    user_id: Option<&str>,
    project_id: Option<&str>,
) -> MemoryObject {
    let mut memory = MemoryObject::new("scope-test", title);
    memory.id = id.to_string();
    memory.summary = "scoped safety marker".to_string();
    memory.body = format!("{title} scoped safety marker");
    memory.tags = vec!["scope-test".to_string()];
    memory
        .metadata
        .insert("visibility".to_string(), visibility.to_string());
    if let Some(user_id) = user_id {
        memory
            .metadata
            .insert("user_id".to_string(), user_id.to_string());
    }
    if let Some(project_id) = project_id {
        memory
            .metadata
            .insert("project_id".to_string(), project_id.to_string());
    }
    memory
}

fn ingest_scope_memories(ledger: &mut SqliteLedger, memories: &[MemoryObject]) {
    for memory in memories {
        ledger.upsert_memory(memory, None).unwrap();
    }
}

#[test]
fn cms_api_scope_filter_allows_private_memory_only_for_matching_user() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memories = vec![
        scoped_memory("mem_scope_public", "Public memory", "public", None, None),
        scoped_memory(
            "mem_scope_alice_private",
            "Alice private memory",
            "private",
            Some("alice"),
            None,
        ),
        scoped_memory(
            "mem_scope_bob_private",
            "Bob private memory",
            "private",
            Some("bob"),
            None,
        ),
    ];
    ingest_scope_memories(&mut ledger, &memories);

    let client = LocalCmsMemoryClient::new(&mut ledger);
    let mut filters = cms_v2::cms_api::Metadata::new();
    filters.insert("user_id".to_string(), Value::String("alice".to_string()));
    let bundle = client
        .retrieve(RetrievalRequest {
            query: "scoped safety marker".to_string(),
            project_id: None,
            modes: vec![ApiRetrievalMode::Exact],
            memory_types: Vec::new(),
            limit: 10,
            graph_depth: 1,
            include_contradictions: false,
            filters,
        })
        .unwrap();
    let ids = bundle
        .results
        .iter()
        .map(|result| result.memory.id.0.as_str())
        .collect::<Vec<_>>();

    assert!(ids.contains(&"mem_scope_public"));
    assert!(ids.contains(&"mem_scope_alice_private"));
    assert!(!ids.contains(&"mem_scope_bob_private"));
    assert_eq!(bundle.trace["scope_user_id"], "alice");
    assert!(
        bundle.trace["retrieval_trace_id"]
            .as_str()
            .unwrap()
            .starts_with("trace_retrieval_")
    );
}

#[test]
fn cms_api_scope_filter_prevents_cross_project_memory_leakage() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memories = vec![
        scoped_memory(
            "mem_scope_project_a",
            "Project A memory",
            "project",
            None,
            Some("project-a"),
        ),
        scoped_memory(
            "mem_scope_project_b",
            "Project B memory",
            "project",
            None,
            Some("project-b"),
        ),
    ];
    ingest_scope_memories(&mut ledger, &memories);

    let client = LocalCmsMemoryClient::new(&mut ledger);
    let bundle = client
        .retrieve(RetrievalRequest {
            query: "scoped safety marker".to_string(),
            project_id: Some(cms_v2::cms_api::ProjectId("project-a".to_string())),
            modes: vec![ApiRetrievalMode::Exact],
            memory_types: Vec::new(),
            limit: 10,
            graph_depth: 1,
            include_contradictions: false,
            filters: Default::default(),
        })
        .unwrap();
    let ids = bundle
        .results
        .iter()
        .map(|result| result.memory.id.0.as_str())
        .collect::<Vec<_>>();

    assert_eq!(ids, vec!["mem_scope_project_a"]);
    assert_eq!(bundle.trace["scope_project_id"], "project-a");
}

#[test]
fn cms_api_public_scope_excludes_private_memory_even_for_owner() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memories = vec![
        scoped_memory("mem_scope_public", "Public memory", "public", None, None),
        scoped_memory(
            "mem_scope_private",
            "Private memory",
            "private",
            Some("alice"),
            None,
        ),
    ];
    ingest_scope_memories(&mut ledger, &memories);

    let client = LocalCmsMemoryClient::new(&mut ledger);
    let mut filters = cms_v2::cms_api::Metadata::new();
    filters.insert("user_id".to_string(), Value::String("alice".to_string()));
    filters.insert(
        "visibility".to_string(),
        Value::String("public".to_string()),
    );
    let bundle = client
        .retrieve(RetrievalRequest {
            query: "scoped safety marker".to_string(),
            project_id: None,
            modes: vec![ApiRetrievalMode::Exact],
            memory_types: Vec::new(),
            limit: 10,
            graph_depth: 1,
            include_contradictions: false,
            filters,
        })
        .unwrap();
    let ids = bundle
        .results
        .iter()
        .map(|result| result.memory.id.0.as_str())
        .collect::<Vec<_>>();

    assert_eq!(ids, vec!["mem_scope_public"]);
}

#[test]
fn cms_api_and_ecm_boundary_do_not_import_runtime_storage_modules() {
    let cms_api_source = fs::read_to_string("src/cms_api.rs").unwrap();
    for forbidden in [
        "crate::sqlite",
        "crate::graph",
        "crate::vectors",
        "crate::rag",
        "crate::lml",
        "SqliteLedger",
        "SqliteGraphIndex",
        "SqliteVectorIndex",
        "HybridRagOrchestrator",
    ] {
        assert!(
            !cms_api_source.contains(forbidden),
            "cms_api.rs must not import or name runtime storage detail: {forbidden}"
        );
    }

    let ecm_source = fs::read_to_string("src/ecm.rs").unwrap();
    for forbidden in [
        "crate::sqlite",
        "crate::graph",
        "crate::vectors",
        "crate::rag",
        "SqliteLedger",
        "SqliteGraphIndex",
        "SqliteVectorIndex",
        "HybridRagOrchestrator",
    ] {
        assert!(
            !ecm_source.contains(forbidden),
            "ecm.rs must stay behind CmsMemoryClient and not name runtime detail: {forbidden}"
        );
    }
}

#[test]
fn provider_contracts_keep_live_provider_details_in_harness_boundary() {
    let prepared = PreparedContext {
        session_id: SessionId("session-provider-contract".to_string()),
        frames: vec![ContextFrame::new(
            ContextFrameType::System,
            "Provider contract fixture",
            "Use the provider-neutral prompt envelope.",
            "test",
            ContextPriority::P0Mandatory,
        )],
        cache_hints: Vec::new(),
        packed_text: "Use the provider-neutral prompt envelope.".to_string(),
        token_estimate: 10,
        included_memory_ids: Vec::new(),
        excluded_memory_ids: Vec::new(),
        trace_id: "trace-provider-contract".to_string(),
        metadata: Default::default(),
    };
    let envelope = PromptCacheEngine::prepare_model_prompt(
        &prepared,
        PromptCachePrepareRequest::new("openai", "gpt-test")
            .with_scope_identity(CacheScopeIdentity::for_user_project("alice", "Vegvisir")),
    );
    let endpoint = ProviderEndpointSpec::openai_responses("gpt-test");
    assert_eq!(endpoint.credential_env.as_deref(), Some("OPENAI_API_KEY"));
    assert_eq!(
        endpoint.default_base_url.as_deref(),
        Some("https://api.openai.com/v1")
    );

    let adapter = HarnessModelAdapterFixture;
    let response = adapter
        .complete(ModelAdapterRequest {
            endpoint,
            envelope: envelope.model_request,
        })
        .unwrap();
    assert_eq!(response.provider, "openai");
    assert_eq!(response.model, "gpt-test");
    assert!(response.output_text.contains("harness adapter received"));
    assert!(response.usage.unwrap().input_tokens > 0);
}

#[test]
fn embedding_adapter_contract_allows_harness_owned_provider_implementation() {
    let adapter = HarnessEmbeddingAdapterFixture;
    let endpoint = adapter.endpoint();
    assert_eq!(endpoint.provider, "openai");
    assert_eq!(
        endpoint.capability,
        cms_v2::provider_contracts::ProviderCapability::Embedding
    );
    assert_eq!(endpoint.credential_env.as_deref(), Some("OPENAI_API_KEY"));
    assert_eq!(
        adapter.provider_id(),
        "harness-openai-text-embedding-3-large"
    );
    assert_eq!(adapter.chunking_version(), "vegvisir-chunks-v1");
    assert_eq!(adapter.embed_text("Vegvisir").unwrap(), vec![8.0, 1.0]);
}

struct MockCmsMemoryClient {
    retrieval_bundle: RetrievalBundle,
    retrieval_requests: Arc<Mutex<Vec<RetrievalRequest>>>,
    committed: Arc<Mutex<Vec<CommitRequest>>>,
}

impl MockCmsMemoryClient {
    fn new(retrieval_bundle: RetrievalBundle) -> Self {
        Self {
            retrieval_bundle,
            retrieval_requests: Arc::new(Mutex::new(Vec::new())),
            committed: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn retrieval_requests_handle(&self) -> Arc<Mutex<Vec<RetrievalRequest>>> {
        Arc::clone(&self.retrieval_requests)
    }

    fn committed_handle(&self) -> Arc<Mutex<Vec<CommitRequest>>> {
        Arc::clone(&self.committed)
    }
}

impl CmsMemoryClient for MockCmsMemoryClient {
    fn retrieve(&self, request: RetrievalRequest) -> CmsApiResult<RetrievalBundle> {
        self.retrieval_requests.lock().unwrap().push(request);
        Ok(self.retrieval_bundle.clone())
    }

    fn commit_memory(&mut self, request: CommitRequest) -> CmsApiResult<CommitResult> {
        let memory_id = request.memory.id.clone();
        self.committed.lock().unwrap().push(request);
        Ok(CommitResult {
            memory_id,
            created_new: true,
            updated_existing: false,
            linked_memory_ids: Vec::new(),
            trace: Default::default(),
        })
    }
}

fn api_memory(id: &str, title: &str, body: &str) -> ApiMemoryObject {
    let now = Utc::now();
    ApiMemoryObject {
        id: ApiMemoryId(id.to_string()),
        memory_type: "design-decision".to_string(),
        title: title.to_string(),
        summary: body.to_string(),
        body: body.to_string(),
        claims: Vec::new(),
        links: Vec::new(),
        tags: vec!["mock-memory".to_string()],
        confidence: 0.91,
        source: Some("mock-client".to_string()),
        created_at: now,
        updated_at: now,
        metadata: Default::default(),
    }
}

#[test]
fn public_api_request_helpers_round_trip_through_json() {
    let mut memory = ApiMemoryObject::new(
        "mem_public_api_helper",
        "technical-fact",
        "Public API helper",
        "Constructors keep embedded CMS usage compact and serializable.",
    );
    memory.source = Some("api-test".to_string());
    memory.tags = vec!["api".to_string(), "devex".to_string()];
    memory.metadata.insert(
        "visibility".to_string(),
        Value::String("public".to_string()),
    );

    let commit = CommitRequest::new(memory.clone(), "public API serialization test");
    let commit_json = serde_json::to_value(&commit).unwrap();
    let decoded_commit: CommitRequest = serde_json::from_value(commit_json).unwrap();
    assert_eq!(
        decoded_commit.memory.id.to_string(),
        "mem_public_api_helper"
    );
    assert_eq!(
        decoded_commit.reason,
        "public API serialization test".to_string()
    );
    assert!(decoded_commit.deduplicate);
    assert!(decoded_commit.update_existing);

    let retrieval = RetrievalRequest::new("embedded retrieval")
        .with_project(cms_v2::cms_api::ProjectId::new("CMS"));
    let retrieval_json = serde_json::to_value(&retrieval).unwrap();
    let decoded_retrieval: RetrievalRequest = serde_json::from_value(retrieval_json).unwrap();
    assert_eq!(decoded_retrieval.query, "embedded retrieval");
    assert_eq!(
        decoded_retrieval.project_id.unwrap().to_string(),
        "CMS".to_string()
    );
    assert_eq!(decoded_retrieval.modes, vec![ApiRetrievalMode::Hybrid]);
    assert_eq!(decoded_retrieval.limit, 10);

    let context_request =
        ContextRequest::new("alice", "continue the implementation", ContextMode::Coding)
            .with_project("CMS");
    let context_json = serde_json::to_value(&context_request).unwrap();
    assert_eq!(context_json["user_id"], "alice");
    assert_eq!(context_json["project_id"], "CMS");
    assert_eq!(context_json["mode"], "Coding");

    let cache_request = PromptCachePrepareRequest::new("openai", "gpt-test").with_scope_identity(
        CacheScopeIdentity::for_user_project("alice", "CMS").with_session("session-1"),
    );
    let cache_json = serde_json::to_value(&cache_request).unwrap();
    assert_eq!(cache_json["provider"], "openai");
    assert_eq!(cache_json["model"], "gpt-test");
    assert_eq!(cache_json["scope_identity"]["user_id"], "alice");
    assert_eq!(cache_json["scope_identity"]["project_id"], "CMS");
    assert_eq!(cache_json["scope_identity"]["session_id"], "session-1");
}

#[test]
fn ecm_prepares_context_with_mock_cms_client_only() {
    let memory = api_memory(
        "mem_mock_ecm_boundary",
        "ECM boundary",
        "ECM must depend on CmsMemoryClient and not on SQLite runtime internals.",
    );
    let mock_client = MockCmsMemoryClient::new(RetrievalBundle {
        query: "ECM boundary".to_string(),
        results: vec![MemoryRetrievalResult {
            memory: memory.clone(),
            score: 0.98,
            source_mode: ApiRetrievalMode::Hybrid,
            reason: "mock retrieval".to_string(),
        }],
        contradictions: Vec::new(),
        trace: Default::default(),
    });
    let ecm = EterniumContextManager::new(mock_client);

    let prepared = ecm
        .prepare_context(ContextRequest {
            user_id: UserId("test-user".to_string()),
            project_id: None,
            session: None,
            message: "Plan the ECM boundary".to_string(),
            mode: ContextMode::Architecture,
            budget: ContextBudget::default(),
            metadata: Default::default(),
        })
        .unwrap();

    assert!(
        prepared
            .packed_text
            .contains("ECM must depend on CmsMemoryClient")
    );
    assert_eq!(prepared.included_memory_ids, vec![memory.id]);
}

#[test]
fn ecm_applies_prompt_cache_hints_from_memory_metadata() {
    let mut memory = api_memory(
        "mem_prompt_cache_hints",
        "Volatile tool output",
        "This memory should be shown but not cached.",
    );
    memory.metadata.insert(
        "prompt_zone".to_string(),
        Value::String("DynamicRetrievedContext".to_string()),
    );
    memory.metadata.insert(
        "prompt_cache_policy".to_string(),
        Value::String("no_cache".to_string()),
    );
    memory.metadata.insert(
        "prompt_cache_sensitivity".to_string(),
        Value::String("sensitive".to_string()),
    );
    let mock_client = MockCmsMemoryClient::new(RetrievalBundle {
        query: "volatile".to_string(),
        results: vec![MemoryRetrievalResult {
            memory,
            score: 0.98,
            source_mode: ApiRetrievalMode::Hybrid,
            reason: "mock retrieval".to_string(),
        }],
        contradictions: Vec::new(),
        trace: Default::default(),
    });
    let ecm = EterniumContextManager::new(mock_client);

    let prepared = ecm
        .prepare_context(ContextRequest {
            user_id: UserId("test-user".to_string()),
            project_id: Some(cms_v2::cms_api::ProjectId("CMS".to_string())),
            session: None,
            message: "Use volatile context".to_string(),
            mode: ContextMode::Architecture,
            budget: ContextBudget::default(),
            metadata: Default::default(),
        })
        .unwrap();
    let hint = prepared
        .cache_hints
        .iter()
        .find(|hint| hint.source_memory_ids == vec!["mem_prompt_cache_hints"])
        .unwrap();
    assert_eq!(
        hint.preferred_zone,
        PromptCacheZone::DynamicRetrievedContext
    );
    assert_eq!(hint.cache_policy_hint.as_deref(), Some("no_cache"));

    let envelope = PromptCacheEngine::prepare_model_prompt(
        &prepared,
        PromptCachePrepareRequest {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
            scope_identity: CacheScopeIdentity {
                organization_id: None,
                user_id: Some("test-user".to_string()),
                project_id: Some("CMS".to_string()),
                session_id: Some(prepared.session_id.0.clone()),
                shared_scope_id: None,
            },
            ..Default::default()
        },
    );
    let hinted_block = envelope
        .blocks
        .iter()
        .find(|block| block.source_memory_ids == vec!["mem_prompt_cache_hints"])
        .unwrap();
    assert!(!hinted_block.cache_policy.allow_local_cache);
    assert!(!hinted_block.cache_policy.allow_provider_cache);
    assert!(
        envelope
            .cache_plan
            .dynamic_suffix_blocks
            .contains(&hinted_block.id)
    );
}

fn prepared_prompt_cache_context(source_version_hash: &str) -> PreparedContext {
    let mut system = ContextFrame::new(
        ContextFrameType::System,
        "System",
        "CMS owns memory. ECM owns context.",
        "eternium",
        ContextPriority::P0Mandatory,
    );
    system.id = "frame_system".to_string();

    let mut memory = ContextFrame::new(
        ContextFrameType::RetrievedMemory,
        "Architecture memory",
        "Stable rendered memory text.",
        "cms-api",
        ContextPriority::P1Critical,
    );
    memory.id = "frame_memory".to_string();
    memory.memory_ids = vec![ApiMemoryId("mem_prompt_cache_versioned".to_string())];

    let mut turn = ContextFrame::new(
        ContextFrameType::UserRequest,
        "Current request",
        "continue",
        "user",
        ContextPriority::P0Mandatory,
    );
    turn.id = "frame_turn".to_string();

    PreparedContext {
        session_id: SessionId("session-prompt-cache".to_string()),
        frames: vec![system.clone(), memory.clone(), turn.clone()],
        cache_hints: vec![
            cms_v2::prompt_cache::ContextCacheHint {
                frame_id: system.id.clone(),
                preferred_zone: PromptCacheZone::SystemKernel,
                stability: cms_v2::prompt_cache::CacheStability::Static,
                scope: cms_v2::prompt_cache::CacheScope::Global,
                sensitivity: cms_v2::prompt_cache::CacheSensitivity::Normal,
                cache_policy_hint: None,
                source_memory_ids: Vec::new(),
                source_version_hashes: Vec::new(),
            },
            cms_v2::prompt_cache::ContextCacheHint {
                frame_id: memory.id.clone(),
                preferred_zone: PromptCacheZone::StableMemoryCapsule,
                stability: cms_v2::prompt_cache::CacheStability::Versioned,
                scope: cms_v2::prompt_cache::CacheScope::Project,
                sensitivity: cms_v2::prompt_cache::CacheSensitivity::Normal,
                cache_policy_hint: None,
                source_memory_ids: vec!["mem_prompt_cache_versioned".to_string()],
                source_version_hashes: vec![source_version_hash.to_string()],
            },
            cms_v2::prompt_cache::ContextCacheHint {
                frame_id: turn.id.clone(),
                preferred_zone: PromptCacheZone::CurrentTurn,
                stability: cms_v2::prompt_cache::CacheStability::Dynamic,
                scope: cms_v2::prompt_cache::CacheScope::Turn,
                sensitivity: cms_v2::prompt_cache::CacheSensitivity::Normal,
                cache_policy_hint: None,
                source_memory_ids: Vec::new(),
                source_version_hashes: Vec::new(),
            },
        ],
        packed_text: String::new(),
        token_estimate: 0,
        included_memory_ids: vec![ApiMemoryId("mem_prompt_cache_versioned".to_string())],
        excluded_memory_ids: Vec::new(),
        trace_id: "trace_prompt_cache_versioned".to_string(),
        metadata: Default::default(),
    }
}

#[test]
fn prompt_cache_capsule_reuse_counts_treat_source_version_changes_as_misses() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let request = PromptCachePrepareRequest {
        provider: "openai".to_string(),
        model: "gpt-test".to_string(),
        scope_identity: CacheScopeIdentity {
            organization_id: None,
            user_id: Some("alice".to_string()),
            project_id: Some("CMS".to_string()),
            session_id: Some("session-prompt-cache".to_string()),
            shared_scope_id: None,
        },
        ..Default::default()
    };
    let first = PromptCacheEngine::prepare_model_prompt(
        &prepared_prompt_cache_context("source-version-1"),
        request.clone(),
    );
    let first_capsule_ids = first
        .capsules
        .iter()
        .map(|capsule| capsule.capsule_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        ledger
            .prompt_cache_capsule_reuse_counts(&first_capsule_ids)
            .unwrap(),
        (0, first_capsule_ids.len())
    );
    ledger.put_prompt_cache_envelope(&first).unwrap();

    let repeat = PromptCacheEngine::prepare_model_prompt(
        &prepared_prompt_cache_context("source-version-1"),
        request.clone(),
    );
    let repeat_capsule_ids = repeat
        .capsules
        .iter()
        .map(|capsule| capsule.capsule_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        ledger
            .prompt_cache_capsule_reuse_counts(&repeat_capsule_ids)
            .unwrap(),
        (repeat_capsule_ids.len(), 0)
    );

    let changed = PromptCacheEngine::prepare_model_prompt(
        &prepared_prompt_cache_context("source-version-2"),
        request,
    );
    let changed_capsule_ids = changed
        .capsules
        .iter()
        .map(|capsule| capsule.capsule_id.clone())
        .collect::<Vec<_>>();
    let (hits, misses) = ledger
        .prompt_cache_capsule_reuse_counts(&changed_capsule_ids)
        .unwrap();
    assert!(hits > 0);
    assert!(misses > 0);
    assert_ne!(first.manifest.manifest_id, changed.manifest.manifest_id);
}

#[test]
fn prompt_cache_source_invalidation_evicts_only_affected_capsules() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let request = PromptCachePrepareRequest {
        provider: "openai".to_string(),
        model: "gpt-test".to_string(),
        scope_identity: CacheScopeIdentity {
            organization_id: None,
            user_id: Some("alice".to_string()),
            project_id: Some("CMS".to_string()),
            session_id: Some("session-prompt-cache".to_string()),
            shared_scope_id: None,
        },
        ..Default::default()
    };
    let envelope = PromptCacheEngine::prepare_model_prompt(
        &prepared_prompt_cache_context("source-version-1"),
        request,
    );
    let memory_capsule_id = envelope
        .capsules
        .iter()
        .find(|capsule| {
            capsule
                .source_memory_ids
                .contains(&"mem_prompt_cache_versioned".to_string())
        })
        .unwrap()
        .capsule_id
        .clone();
    let system_capsule_id = envelope
        .capsules
        .iter()
        .find(|capsule| capsule.source_memory_ids.is_empty())
        .unwrap()
        .capsule_id
        .clone();
    ledger.put_prompt_cache_envelope(&envelope).unwrap();

    let invalidations = ledger
        .invalidate_prompt_cache_by_source("mem_prompt_cache_versioned", "source-memory-updated")
        .unwrap();

    assert_eq!(invalidations.len(), 1);
    assert_eq!(invalidations[0].manifest_id, envelope.manifest.manifest_id);
    assert_eq!(
        ledger
            .prompt_cache_capsule_reuse_counts(std::slice::from_ref(&memory_capsule_id))
            .unwrap(),
        (0, 1)
    );
    assert_eq!(
        ledger
            .prompt_cache_capsule_reuse_counts(std::slice::from_ref(&system_capsule_id))
            .unwrap(),
        (1, 0)
    );
    assert!(
        ledger
            .audit_events(10)
            .unwrap()
            .iter()
            .any(|event| event.event_type == "prompt_cache.capsules_evicted")
    );
}

#[test]
fn prompt_cache_scope_invalidation_evicts_matching_user_scope_only() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let alice_request = PromptCachePrepareRequest {
        provider: "openai".to_string(),
        model: "gpt-test".to_string(),
        scope_identity: CacheScopeIdentity {
            organization_id: None,
            user_id: Some("alice".to_string()),
            project_id: Some("CMS".to_string()),
            session_id: Some("session-alice".to_string()),
            shared_scope_id: None,
        },
        ..Default::default()
    };
    let bob_request = PromptCachePrepareRequest {
        provider: "openai".to_string(),
        model: "gpt-test".to_string(),
        scope_identity: CacheScopeIdentity {
            organization_id: None,
            user_id: Some("bob".to_string()),
            project_id: Some("CMS".to_string()),
            session_id: Some("session-bob".to_string()),
            shared_scope_id: None,
        },
        ..Default::default()
    };
    let alice = PromptCacheEngine::prepare_model_prompt(
        &prepared_prompt_cache_context("source-version-1"),
        alice_request,
    );
    let bob = PromptCacheEngine::prepare_model_prompt(
        &prepared_prompt_cache_context("source-version-1"),
        bob_request,
    );
    let alice_capsules = alice
        .capsules
        .iter()
        .map(|capsule| capsule.capsule_id.clone())
        .collect::<Vec<_>>();
    let bob_capsules = bob
        .capsules
        .iter()
        .map(|capsule| capsule.capsule_id.clone())
        .collect::<Vec<_>>();
    ledger.put_prompt_cache_envelope(&alice).unwrap();
    ledger.put_prompt_cache_envelope(&bob).unwrap();

    let report = ledger
        .invalidate_prompt_cache_by_scope_filter(
            Some("alice"),
            None,
            None,
            None,
            "scope-policy-changed",
        )
        .unwrap();

    assert_eq!(report.invalidations.len(), 1);
    assert_eq!(
        report.invalidations[0].manifest_id,
        alice.manifest.manifest_id
    );
    assert!(report.evicted_capsules > 0);
    let (alice_hits, alice_misses) = ledger
        .prompt_cache_capsule_reuse_counts(&alice_capsules)
        .unwrap();
    let (bob_hits, bob_misses) = ledger
        .prompt_cache_capsule_reuse_counts(&bob_capsules)
        .unwrap();
    assert!(alice_misses > 0);
    assert!(alice_hits < alice_capsules.len());
    assert_eq!(bob_hits, bob_capsules.len());
    assert_eq!(bob_misses, 0);
    assert!(
        ledger
            .audit_events(10)
            .unwrap()
            .iter()
            .any(|event| event.event_type == "prompt_cache.scope_capsules_evicted")
    );
}

#[test]
fn ecm_retrieval_request_carries_user_project_and_visibility_scope() {
    let mock_client = MockCmsMemoryClient::new(RetrievalBundle::empty("scope"));
    let retrieval_requests = mock_client.retrieval_requests_handle();
    let ecm = EterniumContextManager::new(mock_client);
    let mut metadata = cms_v2::cms_api::Metadata::new();
    metadata.insert(
        "visibility".to_string(),
        Value::String("public".to_string()),
    );

    ecm.prepare_context(ContextRequest {
        user_id: UserId("alice".to_string()),
        project_id: Some(cms_v2::cms_api::ProjectId("project-a".to_string())),
        session: None,
        message: "Plan scoped architecture".to_string(),
        mode: ContextMode::Architecture,
        budget: ContextBudget::default(),
        metadata,
    })
    .unwrap();

    let requests = retrieval_requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].filters["user_id"], "alice");
    assert_eq!(requests[0].filters["project_id"], "project-a");
    assert_eq!(requests[0].filters["visibility"], "public");
    assert_eq!(
        requests[0]
            .project_id
            .as_ref()
            .map(|project_id| &project_id.0),
        Some(&"project-a".to_string())
    );
}

#[test]
fn ecm_writeback_commits_through_mock_cms_client_only() {
    let mock_client = MockCmsMemoryClient::new(RetrievalBundle::empty("unused"));
    let committed = mock_client.committed_handle();
    let session = ContextSession::new(UserId("test-user".to_string()), None);
    let mut ecm = EterniumContextManager::new(mock_client);

    let results = ecm
        .complete_turn(
            &session,
            "Capture this architecture decision.",
            "Decision: ECM writeback must commit through the CmsMemoryClient trait only.",
        )
        .unwrap();

    let committed = committed.lock().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(committed.len(), 1);
    assert_eq!(results[0].memory_id, committed[0].memory.id);
    assert_eq!(committed[0].reason, "ECM writeback evaluation");
    assert!(committed[0].deduplicate);
    assert!(committed[0].update_existing);
    assert_eq!(committed[0].memory.metadata["user_id"], "test-user");
    assert_eq!(committed[0].memory.metadata["visibility"], "private");
    assert!(
        committed[0].memory.metadata["writeback_trace_id"]
            .as_str()
            .unwrap()
            .starts_with("trace_writeback_")
    );
    assert_eq!(
        results[0].trace["writeback_candidate_type"],
        "ArchitectureChange"
    );
    assert_eq!(results[0].trace["writeback_session_id"], session.id.0);
}

#[test]
fn ecm_writeback_uses_usrl_scope_policy_for_destination_metadata() {
    let mock_client = MockCmsMemoryClient::new(RetrievalBundle::empty("unused"));
    let committed = mock_client.committed_handle();
    let mut session = ContextSession::new(
        UserId("alice".to_string()),
        Some(cms_v2::cms_api::ProjectId("project-a".to_string())),
    );
    session.metadata.insert(
        "writeback_visibility".to_string(),
        Value::String("project".to_string()),
    );
    let mut ecm = EterniumContextManager::new(mock_client);

    let results = ecm
        .complete_turn(
            &session,
            "Capture this architecture decision.",
            "Decision: project-scoped ECM writeback should preserve project destination metadata.",
        )
        .unwrap();

    let committed = committed.lock().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].memory.metadata["user_id"], "alice");
    assert_eq!(committed[0].memory.metadata["project_id"], "project-a");
    assert_eq!(committed[0].memory.metadata["visibility"], "project");
    assert_eq!(
        committed[0].memory.metadata["usrl_scope_decision"],
        "writeback_allowed"
    );
    assert_eq!(
        committed[0].memory.metadata["usrl_scope_policy"],
        "default-v1"
    );
}

#[test]
fn ecm_writeback_skips_duplicate_candidate_through_mock_cms_client() {
    let mut duplicate = api_memory(
        "mem_existing_writeback",
        "Existing architecture decision",
        "Existing durable writeback memory.",
    );
    duplicate.memory_type = "architecture-change".to_string();
    let mock_client = MockCmsMemoryClient::new(RetrievalBundle {
        query: "duplicate".to_string(),
        results: vec![MemoryRetrievalResult {
            memory: duplicate,
            score: 0.99,
            source_mode: ApiRetrievalMode::Exact,
            reason: "existing writeback match".to_string(),
        }],
        contradictions: Vec::new(),
        trace: Default::default(),
    });
    let committed = mock_client.committed_handle();
    let retrieval_requests = mock_client.retrieval_requests_handle();
    let session = ContextSession::new(UserId("test-user".to_string()), None);
    let mut ecm = EterniumContextManager::new(mock_client);

    let results = ecm
        .complete_turn(
            &session,
            "Capture this architecture decision.",
            "Decision: ECM writeback must commit through the CmsMemoryClient trait only.",
        )
        .unwrap();

    assert!(results.is_empty());
    assert!(committed.lock().unwrap().is_empty());
    let requests = retrieval_requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].memory_types, vec!["architecture-change"]);
    assert_eq!(requests[0].filters["user_id"], "test-user");
}

#[test]
fn ecm_prepares_context_packet_without_storage_dependencies() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    for path in [
        "memories/cms_hybrid_architecture.lml",
        "memories/usrl_canonical_reference.lml",
    ] {
        let memory = LmlParser::parse_file(path).unwrap();
        ledger.upsert_memory(&memory, None).unwrap();
        SqliteGraphIndex::new(&ledger)
            .upsert_memory(&memory)
            .unwrap();
        SqliteVectorIndex::new(&ledger)
            .upsert_memory(&memory)
            .unwrap();
    }

    let ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut ledger));
    let mut metadata = cms_v2::cms_api::Metadata::new();
    metadata.insert(
        "correlation_id".to_string(),
        Value::String("corr-context-001".to_string()),
    );
    let prepared = ecm
        .prepare_context(ContextRequest {
            user_id: UserId("test-user".to_string()),
            project_id: None,
            session: None,
            message: "Continue the CMS and Eternium architecture".to_string(),
            mode: ContextMode::Architecture,
            budget: ContextBudget {
                max_tokens: 4_000,
                reserved_for_response: 1_000,
                reserved_for_system: 500,
                reserved_for_tools: 500,
            },
            metadata,
        })
        .unwrap();

    assert!(prepared.packed_text.contains("Eternium boundary rule"));
    assert!(
        prepared
            .included_memory_ids
            .iter()
            .any(|id| id.0 == "mem_2026_05_15_cms_hybrid_architecture")
    );
    assert_eq!(prepared.metadata["intent"], "ArchitectureDesign");
    assert_eq!(prepared.metadata["retrieval_result_count"], 2);
    assert!(prepared.metadata["included_frame_count"].as_u64().unwrap() >= 3);
    assert_eq!(
        prepared.metadata["retrieval_trace"]["scope_filtered_result_count"],
        2
    );
    assert_eq!(prepared.metadata["context_trace_id"], prepared.trace_id);
    assert_eq!(prepared.metadata["correlation_id"], "corr-context-001");
    assert_eq!(
        prepared.metadata["retrieval_trace"]["correlation_id"],
        "corr-context-001"
    );
    assert!(
        prepared.metadata["retrieval_trace"]["retrieval_trace_id"]
            .as_str()
            .unwrap()
            .starts_with("trace_retrieval_")
    );
    assert!(
        prepared.metadata["retrieval_results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["memory_id"] == "mem_2026_05_15_cms_hybrid_architecture")
    );
    assert!(prepared.token_estimate <= 2_000);
}

#[test]
fn ecm_minimal_mode_does_not_retrieve_long_term_memory() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memory = LmlParser::parse_file("memories/cms_hybrid_architecture.lml").unwrap();
    ledger.upsert_memory(&memory, None).unwrap();

    let ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut ledger));
    let prepared = ecm
        .prepare_context(ContextRequest {
            user_id: UserId("test-user".to_string()),
            project_id: None,
            session: None,
            message: "Answer without memory".to_string(),
            mode: ContextMode::Minimal,
            budget: ContextBudget::default(),
            metadata: Default::default(),
        })
        .unwrap();

    assert!(prepared.included_memory_ids.is_empty());
    assert!(
        !prepared
            .packed_text
            .contains("CMS should use hybrid memory substrates")
    );
}

#[test]
fn ecm_no_memory_mode_skips_retrieval_and_writeback() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let memory = LmlParser::parse_file("memories/cms_hybrid_architecture.lml").unwrap();
    ledger.upsert_memory(&memory, None).unwrap();

    let mut metadata = cms_v2::cms_api::Metadata::new();
    metadata.insert("memory_mode".to_string(), Value::String("none".to_string()));

    let mut session = ContextSession::new(UserId("test-user".to_string()), None);
    session.metadata = metadata.clone();

    let mut ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut ledger));
    let prepared = ecm
        .prepare_context(ContextRequest {
            user_id: UserId("test-user".to_string()),
            project_id: None,
            session: Some(session.clone()),
            message: "Continue CMS architecture".to_string(),
            mode: ContextMode::Architecture,
            budget: ContextBudget::default(),
            metadata,
        })
        .unwrap();

    assert!(prepared.included_memory_ids.is_empty());
    assert_eq!(prepared.metadata["memory_mode"], "none");
    assert_eq!(prepared.metadata["retrieval_result_count"], 0);

    let results = ecm
        .complete_turn(
            &session,
            "Decision: no-memory mode must not persist this.",
            "Decision: no-memory mode suppresses writeback.",
        )
        .unwrap();
    assert!(results.is_empty());
    assert_eq!(ledger.stats().unwrap().active_memories, 1);
}

#[test]
fn ecm_writeback_evaluates_durable_candidates_without_committing() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let session = ContextSession::new(UserId("test-user".to_string()), None);
    let ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut ledger));

    let candidates = ecm.evaluate_writeback(
        &session,
        "We need to lock the ECM boundary.",
        "Decision: ECM must use cms-api and must not depend on SQLite internals.",
    );

    assert_eq!(candidates.len(), 1);
    assert!(matches!(
        candidates[0].candidate_type,
        MemoryCandidateType::ArchitectureChange | MemoryCandidateType::DesignDecision
    ));
    assert!(candidates[0].duplicate_check_required);
    assert_eq!(candidates[0].suggested_tags[0], "ecm-writeback");
}

#[test]
fn ecm_writeback_suppresses_secret_like_turns_before_commit() {
    let mock_client = MockCmsMemoryClient::new(RetrievalBundle::empty("unused"));
    let committed = mock_client.committed_handle();
    let session = ContextSession::new(UserId("test-user".to_string()), None);
    let mut ecm = EterniumContextManager::new(mock_client);

    let candidates = ecm.evaluate_writeback(
        &session,
        "Capture this architecture decision.",
        &format!(
            "Decision: never store this api_key = {}.",
            fake_openai_key()
        ),
    );
    assert_eq!(
        candidates[0].candidate_type,
        MemoryCandidateType::DoNotStore
    );
    assert!(
        candidates[0]
            .suggested_tags
            .contains(&"sensitive-content".to_string())
    );
    assert!(
        candidates[0]
            .persistence_reason
            .contains("sensitive finding")
    );

    let results = ecm
        .complete_turn(
            &session,
            "Capture this architecture decision.",
            &format!(
                "Decision: never store this api_key = {}.",
                fake_openai_key()
            ),
        )
        .unwrap();
    assert!(results.is_empty());
    assert!(committed.lock().unwrap().is_empty());
}

#[test]
fn ecm_complete_turn_commits_writeback_memory_through_cms_api() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let session = ContextSession::new(UserId("test-user".to_string()), None);
    let results = {
        let mut ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut ledger));
        ecm.complete_turn(
            &session,
            "Capture this architecture decision.",
            "Decision: ECM writeback stores durable design decisions through cms-api.",
        )
        .unwrap()
    };

    assert_eq!(results.len(), 1);
    assert!(results[0].created_new);
    assert!(results[0].memory_id.0.starts_with("mem_ecm_"));
    assert!(
        results[0].trace["writeback_trace_id"]
            .as_str()
            .unwrap()
            .starts_with("trace_writeback_")
    );
    assert_eq!(
        results[0].trace["writeback_candidate_type"],
        "ArchitectureChange"
    );
    assert_eq!(results[0].trace["writeback_session_id"], session.id.0);

    let stored = ledger
        .get_memory(&results[0].memory_id.0)
        .unwrap()
        .expect("committed memory should be retrievable");
    assert_eq!(stored.source.unwrap().reference, "ecm.writeback");
    assert!(stored.tags.contains(&"ecm-writeback".to_string()));
    assert!(
        HybridRagOrchestrator::new(&ledger)
            .retrieve("durable design decisions", RetrievalMode::Hybrid, 5)
            .unwrap()
            .results
            .iter()
            .any(|result| result.memory.id == stored.id)
    );
}

#[test]
fn cms_api_rejects_direct_commit_with_secret_like_content() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let mut client = LocalCmsMemoryClient::new(&mut ledger);
    let memory = api_memory(
        "mem_sensitive_commit",
        "Sensitive commit",
        &format!("This memory contains api_key = {}.", fake_openai_key()),
    );

    let err = client
        .commit_memory(CommitRequest {
            memory,
            reason: "safety test".to_string(),
            deduplicate: true,
            update_existing: true,
        })
        .unwrap_err();

    assert!(format!("{err}").contains("sensitive secret-like content"));
}

#[test]
fn ecm_complete_turn_suppresses_duplicate_writeback_memory_through_cms_api() {
    let mut ledger = SqliteLedger::open_memory().unwrap();
    let session = ContextSession::new(UserId("test-user".to_string()), None);
    let user_message = "Capture this architecture decision.";
    let assistant_response =
        "Decision: ECM writeback stores durable design decisions through cms-api.";

    let first_results = {
        let mut ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut ledger));
        ecm.complete_turn(&session, user_message, assistant_response)
            .unwrap()
    };
    assert_eq!(first_results.len(), 1);

    let second_results = {
        let mut ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut ledger));
        ecm.complete_turn(&session, user_message, assistant_response)
            .unwrap()
    };

    assert!(second_results.is_empty());
    assert_eq!(ledger.stats().unwrap().active_memories, 1);
    assert_eq!(
        ledger
            .memory_versions(&first_results[0].memory_id.0)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn cli_fresh_user_archive_acceptance_smoke() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let restored_db = tempdir.path().join("restored.sqlite3");
    let archive_dir = tempdir.path().join("archive");
    let chatgpt_dir = tempdir.path().join("chatgpt-export");
    let chatgpt_output_dir = tempdir.path().join("chatgpt-lml-output");
    fs::create_dir_all(&chatgpt_dir).unwrap();
    write_chatgpt_export(&chatgpt_dir);

    assert_success(cms_cmd(&db).arg("init").output().unwrap());
    assert_success(
        cms_cmd(&db)
            .arg("ingest")
            .arg("memories/cms_hybrid_architecture.lml")
            .output()
            .unwrap(),
    );
    let chatgpt_output = assert_success(
        cms_cmd(&db)
            .args([
                "import-chatgpt",
                "--ingest",
                "--messages-per-memory",
                "2",
                "--output-dir",
                chatgpt_output_dir.to_str().unwrap(),
                "--json",
            ])
            .arg(&chatgpt_dir)
            .output()
            .unwrap(),
    );
    assert_lml_output_dir_parseable(&chatgpt_output_dir, 3, "chatgpt-conversation");
    let chatgpt_report: Value = serde_json::from_slice(&chatgpt_output.stdout).unwrap();
    assert_eq!(chatgpt_report["generated_memories"], 3);
    assert_eq!(chatgpt_report["unique_memories"], 3);
    assert_eq!(chatgpt_report["importer_version"], "chatgpt-export-v1");
    assert_eq!(
        chatgpt_report["import_batch_hash"].as_str().unwrap().len(),
        64
    );

    let retrieve_output = assert_success(
        cms_cmd(&db)
            .args(["retrieve", "chunked memory objects", "--json"])
            .output()
            .unwrap(),
    );
    let retrieval: Value = serde_json::from_slice(&retrieve_output.stdout).unwrap();
    assert!(
        retrieval["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| { result["memory"]["memory_type"] == "chatgpt-conversation" })
    );

    let prepared_output = assert_success(
        cms_cmd(&db)
            .args([
                "prepare-context",
                "Continue CMS architecture",
                "--user",
                "acceptance-user",
                "--project",
                "CMS",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let prepared: Value = serde_json::from_slice(&prepared_output.stdout).unwrap();
    assert!(prepared["trace_id"].as_str().unwrap().starts_with("trace_"));
    assert!(prepared["cache_hints"].as_array().unwrap().len() >= 2);

    let model_request_output = assert_success(
        cms_cmd(&db)
            .args([
                "prepare-model-request",
                "Continue CMS architecture",
                "--user",
                "acceptance-user",
                "--project",
                "CMS",
                "--provider",
                "openai",
                "--model",
                "gpt-test",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let model_request: Value = serde_json::from_slice(&model_request_output.stdout).unwrap();
    assert!(
        model_request["cached_prompt"]["manifest"]["prompt_cache_key"]
            .as_str()
            .unwrap()
            .starts_with("pck_")
    );
    assert!(
        model_request["cached_prompt"]["manifest"]["cacheable_prefix_tokens"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        model_request["prompt_cache_trace"]["breakpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|breakpoint| breakpoint["zone"] == "SystemKernel")
    );
    let manifest_id = model_request["cached_prompt"]["manifest"]["manifest_id"]
        .as_str()
        .unwrap();

    let prompt_cache_inspect_output = assert_success(
        cms_cmd(&db)
            .args(["prompt-cache", "inspect", manifest_id, "--json"])
            .output()
            .unwrap(),
    );
    let prompt_cache_manifest: Value =
        serde_json::from_slice(&prompt_cache_inspect_output.stdout).unwrap();
    assert_eq!(
        prompt_cache_manifest["manifest"]["manifest_id"],
        model_request["cached_prompt"]["manifest"]["manifest_id"]
    );

    let prompt_cache_usage_output = assert_success(
        cms_cmd(&db)
            .args(["prompt-cache", "usage", manifest_id, "--json"])
            .output()
            .unwrap(),
    );
    let prompt_cache_usage: Value =
        serde_json::from_slice(&prompt_cache_usage_output.stdout).unwrap();
    assert_eq!(
        prompt_cache_usage[0]["usage"]["manifest_id"],
        model_request["cached_prompt"]["manifest"]["manifest_id"]
    );
    assert!(
        prompt_cache_usage[0]["usage"]["local_capsule_misses"]
            .as_u64()
            .unwrap()
            > 0
    );

    let prompt_cache_capsules_output = assert_success(
        cms_cmd(&db)
            .args(["prompt-cache", "capsules", "--json"])
            .output()
            .unwrap(),
    );
    let prompt_cache_capsules: Value =
        serde_json::from_slice(&prompt_cache_capsules_output.stdout).unwrap();
    assert!(
        prompt_cache_capsules
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["capsule"]["capsule_type"] == "SystemKernel")
    );

    let second_model_request_output = assert_success(
        cms_cmd(&db)
            .args([
                "prepare-model-request",
                "Continue CMS architecture with a different current turn",
                "--user",
                "acceptance-user",
                "--project",
                "CMS",
                "--provider",
                "openai",
                "--model",
                "gpt-test",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let second_model_request: Value =
        serde_json::from_slice(&second_model_request_output.stdout).unwrap();
    assert!(
        second_model_request["prompt_cache_trace"]["local_capsule_hits"]
            .as_u64()
            .unwrap()
            > 0
    );

    let provider_usage_output = assert_success(
        cms_cmd(&db)
            .args([
                "prompt-cache",
                "record-usage",
                manifest_id,
                "--provider-cached-input-tokens",
                "64",
                "--provider-cache-read-tokens",
                "64",
                "--latency-ms",
                "123",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let provider_usage: Value = serde_json::from_slice(&provider_usage_output.stdout).unwrap();
    assert_eq!(provider_usage["usage"]["provider_cached_input_tokens"], 64);
    assert_eq!(provider_usage["usage"]["latency_ms"], 123);

    let prompt_cache_plan_output = assert_success(
        cms_cmd(&db)
            .args([
                "prompt-cache",
                "plan",
                "Plan CMS architecture without persistence",
                "--user",
                "acceptance-user",
                "--project",
                "CMS",
                "--provider",
                "openai",
                "--model",
                "gpt-test",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let prompt_cache_plan: Value =
        serde_json::from_slice(&prompt_cache_plan_output.stdout).unwrap();
    assert_eq!(prompt_cache_plan["persisted"], false);
    assert!(
        prompt_cache_plan["cached_prompt"]["manifest"]["prompt_cache_key"]
            .as_str()
            .unwrap()
            .starts_with("pck_")
    );

    let prompt_cache_invalidate_output = assert_success(
        cms_cmd(&db)
            .args([
                "prompt-cache",
                "invalidate",
                manifest_id,
                "--reason",
                "source-version-changed",
                "--changed-source",
                "mem_acceptance",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let prompt_cache_invalidation: Value =
        serde_json::from_slice(&prompt_cache_invalidate_output.stdout).unwrap();
    assert_eq!(
        prompt_cache_invalidation["reason"],
        "source-version-changed"
    );

    let prompt_cache_explain_output = assert_success(
        cms_cmd(&db)
            .args(["prompt-cache", "explain-miss", manifest_id, "--json"])
            .output()
            .unwrap(),
    );
    let prompt_cache_explanation: Value =
        serde_json::from_slice(&prompt_cache_explain_output.stdout).unwrap();
    assert_eq!(prompt_cache_explanation["cache_valid"], false);
    assert_eq!(prompt_cache_explanation["changed_source"], "mem_acceptance");

    let writeback_output = assert_success(
        cms_cmd(&db)
            .args([
                "complete-turn",
                "Capture this architecture decision.",
                "Decision: CLI acceptance writeback stores durable design decisions through cms-api.",
                "--user",
                "acceptance-user",
                "--project",
                "CMS",
                "--commit",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let writeback: Value = serde_json::from_slice(&writeback_output.stdout).unwrap();
    assert_eq!(writeback.as_array().unwrap().len(), 1);

    let export_output = assert_success(
        cms_cmd(&db)
            .arg("export-archive")
            .arg(&archive_dir)
            .arg("--json")
            .output()
            .unwrap(),
    );
    let manifest: Value = serde_json::from_slice(&export_output.stdout).unwrap();
    assert!(manifest["memory_count"].as_u64().unwrap() >= 5);

    let restore_output = assert_success(
        cms_cmd(&restored_db)
            .arg("restore-archive")
            .arg(&archive_dir)
            .arg("--json")
            .output()
            .unwrap(),
    );
    let restore_report: Value = serde_json::from_slice(&restore_output.stdout).unwrap();
    assert_eq!(
        restore_report["restored_memories"],
        manifest["memory_count"]
    );

    let restored_retrieval_output = assert_success(
        cms_cmd(&restored_db)
            .args(["retrieve", "durable design decisions", "--json"])
            .output()
            .unwrap(),
    );
    let restored_retrieval: Value =
        serde_json::from_slice(&restored_retrieval_output.stdout).unwrap();
    assert!(!restored_retrieval["results"].as_array().unwrap().is_empty());
}

#[test]
fn cli_lifecycle_json_smoke() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let memory_id = "mem_2026_05_15_cms_hybrid_architecture";

    assert_success(cms_cmd(&db).arg("init").output().unwrap());
    assert_success(
        cms_cmd(&db)
            .arg("ingest")
            .arg("memories/cms_hybrid_architecture.lml")
            .output()
            .unwrap(),
    );

    let list_output = assert_success(cms_cmd(&db).args(["list", "--json"]).output().unwrap());
    let list: Value = serde_json::from_slice(&list_output.stdout).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);

    let get_output = assert_success(
        cms_cmd(&db)
            .args(["get", memory_id, "--json"])
            .output()
            .unwrap(),
    );
    let memory: Value = serde_json::from_slice(&get_output.stdout).unwrap();
    assert_eq!(memory["id"], memory_id);

    let history_output = assert_success(
        cms_cmd(&db)
            .args(["history", memory_id, "--json"])
            .output()
            .unwrap(),
    );
    let history: Value = serde_json::from_slice(&history_output.stdout).unwrap();
    assert_eq!(history.as_array().unwrap().len(), 1);

    assert_success(cms_cmd(&db).args(["delete", memory_id]).output().unwrap());
    assert_success(cms_cmd(&db).args(["restore", memory_id]).output().unwrap());
    assert_success(cms_cmd(&db).args(["archive", memory_id]).output().unwrap());
    let archived_output = assert_success(
        cms_cmd(&db)
            .args(["list", "--status", "archived", "--json"])
            .output()
            .unwrap(),
    );
    let archived: Value = serde_json::from_slice(&archived_output.stdout).unwrap();
    assert_eq!(archived[0]["status"], "archived");
    assert_success(cms_cmd(&db).args(["restore", memory_id]).output().unwrap());
    let duplicate_path = tempdir.path().join("duplicate.lml");
    let duplicate_lml = fs::read_to_string("memories/cms_hybrid_architecture.lml")
        .unwrap()
        .replace(
            "mem_2026_05_15_cms_hybrid_architecture",
            "mem_cli_duplicate",
        )
        .replace(
            "CMS hybrid architecture",
            "CMS hybrid architecture duplicate",
        );
    fs::write(&duplicate_path, duplicate_lml).unwrap();
    assert_success(
        cms_cmd(&db)
            .arg("ingest")
            .arg(&duplicate_path)
            .output()
            .unwrap(),
    );
    fs::remove_file(&duplicate_path).unwrap();
    assert_success(
        cms_cmd(&db)
            .args(["merge", "mem_cli_duplicate", memory_id])
            .output()
            .unwrap(),
    );
    let superseded_output = assert_success(
        cms_cmd(&db)
            .args(["list", "--status", "superseded", "--json"])
            .output()
            .unwrap(),
    );
    let superseded: Value = serde_json::from_slice(&superseded_output.stdout).unwrap();
    assert_eq!(superseded[0]["id"], "mem_cli_duplicate");

    let status_output = assert_success(
        cms_cmd(&db)
            .args(["status", "memories", "--json"])
            .output()
            .unwrap(),
    );
    let status: Value = serde_json::from_slice(&status_output.stdout).unwrap();
    assert_eq!(status["stats"]["active_memories"], 1);

    let audit_output = assert_success(
        cms_cmd(&db)
            .args(["audit", "--limit", "10", "--json"])
            .output()
            .unwrap(),
    );
    let audit: Value = serde_json::from_slice(&audit_output.stdout).unwrap();
    assert!(
        audit
            .as_array()
            .unwrap()
            .iter()
            .any(|event| { event["event_type"] == "memory.restored" })
    );

    let diagnostics_output = assert_success(
        cms_cmd(&db)
            .arg("diagnostics")
            .arg(tempdir.path())
            .args(["--audit-limit", "10", "--json"])
            .output()
            .unwrap(),
    );
    let diagnostics: Value = serde_json::from_slice(&diagnostics_output.stdout).unwrap();
    assert_eq!(diagnostics["health"]["status"], "healthy");
    assert_eq!(diagnostics["health"]["schema_issues"], 0);
    assert_eq!(diagnostics["stats"]["active_memories"], 1);
    assert_eq!(diagnostics["migrations"].as_array().unwrap().len(), 3);
    assert_eq!(diagnostics["schema"]["is_current"], true);
    assert_eq!(diagnostics["prompt_cache"]["manifests"], 0);
    assert_eq!(diagnostics["prompt_cache"]["capsules"], 0);
    assert!(
        diagnostics["observability"]["recent_audit_event_count"]
            .as_u64()
            .unwrap()
            >= 1
    );
    assert!(
        diagnostics["observability"]["recent_audit_event_types"]["memory.restored"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn cli_core_utility_commands_smoke() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let memories_dir = tempdir.path().join("memories");
    fs::create_dir_all(&memories_dir).unwrap();
    let memory_path = memories_dir.join("cms_hybrid_architecture.lml");
    fs::copy("memories/cms_hybrid_architecture.lml", &memory_path).unwrap();
    let roundtrip_path = tempdir.path().join("roundtrip.lml");
    let memory_id = "mem_2026_05_15_cms_hybrid_architecture";

    assert_success(
        cms_cmd(&db)
            .arg("validate")
            .arg(&memory_path)
            .output()
            .unwrap(),
    );
    assert_success(
        cms_cmd(&db)
            .arg("round-trip")
            .arg(&memory_path)
            .arg(&roundtrip_path)
            .output()
            .unwrap(),
    );
    assert!(roundtrip_path.exists());

    assert_success(
        cms_cmd(&db)
            .arg("ingest-dir")
            .arg(&memories_dir)
            .output()
            .unwrap(),
    );

    let search_output = assert_success(
        cms_cmd(&db)
            .args(["search", "hybrid memory", "--json"])
            .output()
            .unwrap(),
    );
    let search: Value = serde_json::from_slice(&search_output.stdout).unwrap();
    assert!(!search.as_array().unwrap().is_empty());

    let semantic_output = assert_success(
        cms_cmd(&db)
            .args(["semantic-search", "memory substrates", "--json"])
            .output()
            .unwrap(),
    );
    let semantic: Value = serde_json::from_slice(&semantic_output.stdout).unwrap();
    assert!(semantic.as_array().is_some());

    let graph_output = assert_success(
        cms_cmd(&db)
            .args(["graph-related", memory_id, "--json"])
            .output()
            .unwrap(),
    );
    let graph: Value = serde_json::from_slice(&graph_output.stdout).unwrap();
    assert!(graph.as_array().is_some());

    assert_success(cms_cmd(&db).arg("reindex").output().unwrap());
    assert_success(
        cms_cmd(&db)
            .args(["reindex", "--id", memory_id])
            .output()
            .unwrap(),
    );
    assert_success(
        cms_cmd(&db)
            .args([
                "reindex",
                "--id",
                memory_id,
                "--vector-provider",
                "external-embedding-v1",
                "--vector-chunking",
                "paragraph-chunks-v2",
            ])
            .output()
            .unwrap(),
    );
    let ledger = SqliteLedger::open(&db).unwrap();
    let memory = ledger.get_memory(memory_id).unwrap().unwrap();
    assert_eq!(
        ledger
            .index_hash(memory_id, "sqlite-vector")
            .unwrap()
            .unwrap(),
        vector_index_hash_with_config(&memory, "external-embedding-v1", "paragraph-chunks-v2")
    );
}

#[test]
fn cli_quarantine_and_supersede_commands_smoke() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let source_lml = fs::read_to_string("memories/cms_hybrid_architecture.lml").unwrap();
    let quarantine_path = tempdir.path().join("quarantine.lml");
    let old_path = tempdir.path().join("old.lml");
    let new_path = tempdir.path().join("new.lml");
    fs::write(
        &quarantine_path,
        source_lml
            .replace(
                "mem_2026_05_15_cms_hybrid_architecture",
                "mem_cli_quarantine",
            )
            .replace("CMS hybrid architecture", "CLI quarantine memory"),
    )
    .unwrap();
    fs::write(
        &old_path,
        source_lml
            .replace(
                "mem_2026_05_15_cms_hybrid_architecture",
                "mem_cli_superseded_old",
            )
            .replace("CMS hybrid architecture", "CLI superseded old memory"),
    )
    .unwrap();
    fs::write(
        &new_path,
        source_lml
            .replace(
                "mem_2026_05_15_cms_hybrid_architecture",
                "mem_cli_superseded_new",
            )
            .replace("CMS hybrid architecture", "CLI superseded new memory"),
    )
    .unwrap();

    for path in [&quarantine_path, &old_path, &new_path] {
        assert_success(cms_cmd(&db).arg("ingest").arg(path).output().unwrap());
    }
    assert_success(
        cms_cmd(&db)
            .args(["quarantine", "mem_cli_quarantine"])
            .output()
            .unwrap(),
    );
    let quarantined_output = assert_success(
        cms_cmd(&db)
            .args(["list", "--status", "quarantined", "--json"])
            .output()
            .unwrap(),
    );
    let quarantined: Value = serde_json::from_slice(&quarantined_output.stdout).unwrap();
    assert_eq!(quarantined[0]["id"], "mem_cli_quarantine");

    assert_success(
        cms_cmd(&db)
            .args([
                "supersede",
                "mem_cli_superseded_old",
                "mem_cli_superseded_new",
            ])
            .output()
            .unwrap(),
    );
    let superseded_output = assert_success(
        cms_cmd(&db)
            .args(["list", "--status", "superseded", "--json"])
            .output()
            .unwrap(),
    );
    let superseded: Value = serde_json::from_slice(&superseded_output.stdout).unwrap();
    assert!(
        superseded
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["id"] == "mem_cli_superseded_old")
    );
}

#[test]
fn cli_failure_modes_return_nonzero_with_actionable_errors() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let invalid_lml = tempdir.path().join("invalid.lml");
    fs::write(&invalid_lml, "memory { id: }").unwrap();
    assert_success(cms_cmd(&db).arg("init").output().unwrap());

    let validate_failure = assert_failure(
        cms_cmd(&db)
            .arg("validate")
            .arg(&invalid_lml)
            .output()
            .unwrap(),
    );
    assert!(
        String::from_utf8_lossy(&validate_failure.stderr).contains("failed to parse")
            || String::from_utf8_lossy(&validate_failure.stderr).contains("expected")
    );

    let get_failure = assert_failure(
        cms_cmd(&db)
            .args(["get", "mem_missing", "--json"])
            .output()
            .unwrap(),
    );
    assert!(String::from_utf8_lossy(&get_failure.stderr).contains("memory not found"));

    let scope_failure = assert_failure(
        cms_cmd(&db)
            .args([
                "prompt-cache",
                "invalidate-scope",
                "--reason",
                "missing-scope",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    assert!(
        String::from_utf8_lossy(&scope_failure.stderr)
            .contains("at least one scope filter is required")
    );
}

#[test]
fn cli_repair_json_smoke_repairs_missing_derived_indexes() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let memories_dir = tempdir.path().join("memories");
    fs::create_dir_all(&memories_dir).unwrap();
    let memory_path = memories_dir.join("cms_hybrid_architecture.lml");
    fs::copy("memories/cms_hybrid_architecture.lml", &memory_path).unwrap();
    {
        let mut ledger = SqliteLedger::open(&db).unwrap();
        let memory = LmlParser::parse_file(&memory_path).unwrap();
        ledger.upsert_memory(&memory, Some(&memory_path)).unwrap();
    }

    let check_output = cms_cmd(&db)
        .arg("check")
        .arg(&memories_dir)
        .arg("--json")
        .output()
        .unwrap();
    assert!(!check_output.status.success());
    let check_stdout: Value = serde_json::from_slice(&check_output.stdout).unwrap();
    assert!(
        check_stdout["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["id"] == "missing-derived-index")
    );

    let repair_output = assert_success(
        cms_cmd(&db)
            .arg("repair")
            .args([
                "--vector-provider",
                "external-embedding-v1",
                "--vector-chunking",
                "paragraph-chunks-v2",
            ])
            .arg(&memories_dir)
            .arg("--json")
            .output()
            .unwrap(),
    );
    let repair: Value = serde_json::from_slice(&repair_output.stdout).unwrap();
    assert_eq!(repair["repaired_issues"], 2);
    assert_eq!(repair["reindexed_memories"].as_array().unwrap().len(), 1);
    let ledger = SqliteLedger::open(&db).unwrap();
    let memory = ledger
        .get_memory("mem_2026_05_15_cms_hybrid_architecture")
        .unwrap()
        .unwrap();
    assert_eq!(
        ledger
            .index_hash(&memory.id, "sqlite-vector")
            .unwrap()
            .unwrap(),
        vector_index_hash_with_config(&memory, "external-embedding-v1", "paragraph-chunks-v2")
    );
    drop(ledger);
    assert_success(
        cms_cmd(&db)
            .args(["reindex", "--id", "mem_2026_05_15_cms_hybrid_architecture"])
            .output()
            .unwrap(),
    );

    let diagnostics_output = assert_success(
        cms_cmd(&db)
            .arg("diagnostics")
            .arg(&memories_dir)
            .args(["--json"])
            .output()
            .unwrap(),
    );
    let diagnostics: Value = serde_json::from_slice(&diagnostics_output.stdout).unwrap();
    assert_eq!(diagnostics["observability"]["repair_events"], 2);

    assert_success(
        cms_cmd(&db)
            .arg("check")
            .arg(&memories_dir)
            .arg("--json")
            .output()
            .unwrap(),
    );
}

#[test]
fn cli_scope_commands_inspect_and_filter_scoped_usrl_imports() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let usrl_path = tempdir.path().join("private-scope.usrl");
    fs::write(
        &usrl_path,
        r#"
contract PrivateScopePolicy {
  section Personalization {
    rule UserOnlyPolicy {
      permit scoped_memory_access();
    }
  }
}
"#,
    )
    .unwrap();

    assert_success(cms_cmd(&db).arg("init").output().unwrap());
    let resolved_scope_output = assert_success(
        cms_cmd(&db)
            .args([
                "scope",
                "resolve",
                "--visibility",
                "private",
                "--user",
                "alice",
                "--project",
                "project-a",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let resolved_scope: Value = serde_json::from_slice(&resolved_scope_output.stdout).unwrap();
    assert_eq!(resolved_scope["scope"]["visibility"], "Private");
    assert_eq!(resolved_scope["retrieval_filters"]["visibility"], "private");
    assert_eq!(resolved_scope["retrieval_filters"]["user_id"], "alice");

    assert_success(
        cms_cmd(&db)
            .args([
                "import-usrl",
                "--ingest",
                "--visibility",
                "private",
                "--user",
                "alice",
                "--project",
                "project-a",
            ])
            .arg(&usrl_path)
            .output()
            .unwrap(),
    );

    let alice_scope_output = assert_success(
        cms_cmd(&db)
            .args([
                "scope",
                "list",
                "--visibility",
                "private",
                "--user",
                "alice",
                "--project",
                "project-a",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let alice_scope: Value = serde_json::from_slice(&alice_scope_output.stdout).unwrap();
    let memory_id = alice_scope[0]["id"].as_str().unwrap();
    assert_eq!(alice_scope[0]["visibility"], "private");
    assert_eq!(alice_scope[0]["user_id"], "alice");
    assert_eq!(alice_scope[0]["project_id"], "project-a");

    let inspect_output = assert_success(
        cms_cmd(&db)
            .args(["scope", "inspect", memory_id, "--json"])
            .output()
            .unwrap(),
    );
    let inspected: Value = serde_json::from_slice(&inspect_output.stdout).unwrap();
    assert_eq!(inspected["visibility"], "private");
    assert_eq!(inspected["user_id"], "alice");

    let bob_scope_output = assert_success(
        cms_cmd(&db)
            .args(["scope", "list", "--user", "bob", "--json"])
            .output()
            .unwrap(),
    );
    let bob_scope: Value = serde_json::from_slice(&bob_scope_output.stdout).unwrap();
    assert!(bob_scope.as_array().unwrap().is_empty());

    let alice_retrieve_output = assert_success(
        cms_cmd(&db)
            .args([
                "retrieve",
                "PrivateScopePolicy",
                "--mode",
                "exact",
                "--user",
                "alice",
                "--project",
                "project-a",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let alice_retrieval: Value = serde_json::from_slice(&alice_retrieve_output.stdout).unwrap();
    assert_eq!(
        alice_retrieval["results"][0]["memory"]["id"],
        serde_json::json!(memory_id)
    );

    let bob_retrieve_output = assert_success(
        cms_cmd(&db)
            .args([
                "retrieve",
                "PrivateScopePolicy",
                "--mode",
                "exact",
                "--user",
                "bob",
                "--project",
                "project-a",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let bob_retrieval: Value = serde_json::from_slice(&bob_retrieve_output.stdout).unwrap();
    assert!(bob_retrieval["results"].as_array().unwrap().is_empty());

    let archive_dir = tempdir.path().join("alice-private-archive");
    let archive_output = assert_success(
        cms_cmd(&db)
            .args([
                "export-archive",
                "--visibility",
                "private",
                "--user",
                "alice",
                "--project",
                "project-a",
                "--json",
            ])
            .arg(&archive_dir)
            .output()
            .unwrap(),
    );
    let manifest: Value = serde_json::from_slice(&archive_output.stdout).unwrap();
    assert_eq!(manifest["memory_count"], 1);
    assert_eq!(manifest["scope_filter"]["visibility"], "private");
    assert_eq!(manifest["scope_filter"]["user_id"], "alice");
    assert_eq!(manifest["memories"][0]["id"], serde_json::json!(memory_id));
}

#[test]
fn cli_scoped_user_acceptance_isolates_context_and_prompt_cache() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let jsonl_path = tempdir.path().join("scoped-memories.jsonl");
    fs::write(
        &jsonl_path,
        concat!(
            r#"{"title":"Alice Scoped Memory","body":"ALICE_ONLY_SCOPE_PAYLOAD belongs only to Alice.","visibility":"private","user_id":"alice","project_id":"CMS"}"#,
            "\n",
            r#"{"title":"Bob Scoped Memory","body":"BOB_ONLY_SCOPE_PAYLOAD belongs only to Bob.","visibility":"private","user_id":"bob","project_id":"CMS"}"#,
            "\n",
        ),
    )
    .unwrap();

    assert_success(cms_cmd(&db).arg("init").output().unwrap());
    let import_output = assert_success(
        cms_cmd(&db)
            .args(["import-jsonl", "--ingest", "--json"])
            .arg(&jsonl_path)
            .output()
            .unwrap(),
    );
    let import_report: Value = serde_json::from_slice(&import_output.stdout).unwrap();
    assert_eq!(import_report["generated_memories"], 2);
    assert_eq!(import_report["unique_memories"], 2);

    let alice_retrieve_output = assert_success(
        cms_cmd(&db)
            .args([
                "retrieve",
                "Alice scoped memory",
                "--mode",
                "exact",
                "--user",
                "alice",
                "--project",
                "CMS",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let alice_retrieval: Value = serde_json::from_slice(&alice_retrieve_output.stdout).unwrap();
    assert!(
        alice_retrieval["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["memory"]["body"]
                .as_str()
                .unwrap()
                .contains("ALICE_ONLY_SCOPE_PAYLOAD"))
    );

    let bob_retrieve_output = assert_success(
        cms_cmd(&db)
            .args([
                "retrieve",
                "Alice scoped memory",
                "--mode",
                "exact",
                "--user",
                "bob",
                "--project",
                "CMS",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let bob_retrieval: Value = serde_json::from_slice(&bob_retrieve_output.stdout).unwrap();
    assert!(
        bob_retrieval["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|result| !result["memory"]["body"]
                .as_str()
                .unwrap()
                .contains("ALICE_ONLY_SCOPE_PAYLOAD"))
    );

    let alice_context_output = assert_success(
        cms_cmd(&db)
            .args([
                "prepare-context",
                "Recall Alice scoped memory",
                "--user",
                "alice",
                "--project",
                "CMS",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let alice_context: Value = serde_json::from_slice(&alice_context_output.stdout).unwrap();
    let alice_packed = alice_context["packed_text"].as_str().unwrap();
    assert!(alice_packed.contains("[RetrievedMemory] Alice Scoped Memory"));
    assert!(!alice_packed.contains("[RetrievedMemory] Bob Scoped Memory"));
    assert_eq!(
        alice_context["metadata"]["retrieval_trace"]["scope_user_id"],
        "alice"
    );

    let bob_context_output = assert_success(
        cms_cmd(&db)
            .args([
                "prepare-context",
                "Recall Alice scoped memory",
                "--user",
                "bob",
                "--project",
                "CMS",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let bob_context: Value = serde_json::from_slice(&bob_context_output.stdout).unwrap();
    let bob_packed = bob_context["packed_text"].as_str().unwrap();
    assert!(!bob_packed.contains("[RetrievedMemory] Alice Scoped Memory"));
    assert!(!bob_packed.contains("ALICE_ONLY_SCOPE_PAYLOAD"));

    let alice_plan_output = assert_success(
        cms_cmd(&db)
            .args([
                "prompt-cache",
                "plan",
                "Recall Alice scoped memory",
                "--user",
                "alice",
                "--project",
                "CMS",
                "--provider",
                "openai",
                "--model",
                "scoped-test",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let alice_plan: Value = serde_json::from_slice(&alice_plan_output.stdout).unwrap();

    let bob_plan_output = assert_success(
        cms_cmd(&db)
            .args([
                "prompt-cache",
                "plan",
                "Recall Alice scoped memory",
                "--user",
                "bob",
                "--project",
                "CMS",
                "--provider",
                "openai",
                "--model",
                "scoped-test",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let bob_plan: Value = serde_json::from_slice(&bob_plan_output.stdout).unwrap();

    assert!(
        alice_plan["cached_prompt"]["manifest"]["prompt_cache_key"]
            .as_str()
            .unwrap()
            .starts_with("pck_")
    );
    assert_ne!(
        alice_plan["cached_prompt"]["manifest"]["prompt_cache_key"],
        bob_plan["cached_prompt"]["manifest"]["prompt_cache_key"]
    );
    assert_eq!(
        alice_plan["cached_prompt"]["manifest"]["scope_identity"]["user_id"],
        "alice"
    );
    assert_eq!(
        bob_plan["cached_prompt"]["manifest"]["scope_identity"]["user_id"],
        "bob"
    );
}

#[test]
fn cli_agentic_ai_runtime_simulation_smoke() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let jsonl_path = tempdir.path().join("vegvisir-agent-runtime.jsonl");
    let correlation_id = "vegvisir-agent-test-001";
    fs::write(
        &jsonl_path,
        concat!(
            r#"{"title":"Vegvisir Agent Routing Policy","body":"VEGVISIR_AGENT_ROUTING_POLICY requires planner, coder, and reviewer agents to share durable project memory through CMS before acting.","visibility":"shared","user_id":"codex-agent","project_id":"Vegvisir","tags":["HarnessOS","Vegvisir","agent-runtime"]}"#,
            "\n",
            r#"{"title":"Vegvisir Private Operator Note","body":"VEGVISIR_PRIVATE_OPERATOR_NOTE is visible only to the codex-agent operator session.","visibility":"private","user_id":"codex-agent","project_id":"Vegvisir","tags":["HarnessOS","Vegvisir","operator"]}"#,
            "\n",
        ),
    )
    .unwrap();

    assert_success(cms_cmd(&db).arg("init").output().unwrap());
    let import_output = assert_success(
        cms_cmd(&db)
            .args(["import-jsonl", "--ingest", "--json"])
            .arg(&jsonl_path)
            .output()
            .unwrap(),
    );
    let import_report: Value = serde_json::from_slice(&import_output.stdout).unwrap();
    assert_eq!(import_report["generated_memories"], 2);
    assert_eq!(import_report["unique_memories"], 2);

    let planner_request_output = assert_success(
        cms_cmd(&db)
            .args([
                "prepare-model-request",
                "Plan the Vegvisir agent runtime handoff",
                "--user",
                "codex-agent",
                "--project",
                "Vegvisir",
                "--correlation-id",
                correlation_id,
                "--provider",
                "openai",
                "--model",
                "agent-runtime-test",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let planner_request: Value = serde_json::from_slice(&planner_request_output.stdout).unwrap();
    assert!(
        planner_request["prepared_context"]["packed_text"]
            .as_str()
            .unwrap()
            .contains("Vegvisir Agent Routing Policy")
    );
    assert!(
        planner_request["cached_prompt"]["manifest"]["prompt_cache_key"]
            .as_str()
            .unwrap()
            .starts_with("pck_")
    );
    assert!(
        planner_request["prompt_cache_trace"]["local_capsule_misses"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        planner_request["prepared_context"]["metadata"]["correlation_id"],
        correlation_id
    );
    assert_eq!(
        planner_request["prepared_context"]["metadata"]["retrieval_trace"]["correlation_id"],
        correlation_id
    );
    let planner_manifest_id = planner_request["cached_prompt"]["manifest"]["manifest_id"]
        .as_str()
        .unwrap();

    let coder_request_output = assert_success(
        cms_cmd(&db)
            .args([
                "prepare-model-request",
                "Implement the Vegvisir agent runtime handoff plan",
                "--user",
                "codex-agent",
                "--project",
                "Vegvisir",
                "--correlation-id",
                correlation_id,
                "--provider",
                "openai",
                "--model",
                "agent-runtime-test",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let coder_request: Value = serde_json::from_slice(&coder_request_output.stdout).unwrap();
    assert_eq!(
        coder_request["prepared_context"]["metadata"]["correlation_id"],
        correlation_id
    );
    assert!(
        coder_request["prompt_cache_trace"]["local_capsule_hits"]
            .as_u64()
            .unwrap()
            > 0
    );

    let coder_writeback_output = assert_success(
        cms_cmd(&db)
            .args([
                "complete-turn",
                "Implement the Vegvisir agent runtime handoff plan.",
                "Decision: Vegvisir coder agents must record implementation handoffs in CMS before reviewer agents prepare context.",
                "--user",
                "codex-agent",
                "--project",
                "Vegvisir",
                "--correlation-id",
                correlation_id,
                "--commit",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let coder_writeback: Value = serde_json::from_slice(&coder_writeback_output.stdout).unwrap();
    assert_eq!(coder_writeback.as_array().unwrap().len(), 1);
    let coder_writeback_memory_id = coder_writeback[0]["memory_id"].as_str().unwrap();
    assert!(coder_writeback_memory_id.starts_with("mem_ecm_"));
    assert_eq!(
        coder_writeback[0]["trace"]["correlation_id"],
        correlation_id
    );

    let reviewer_request_output = assert_success(
        cms_cmd(&db)
            .args([
                "prepare-model-request",
                "Review the Vegvisir coder implementation handoff",
                "--user",
                "codex-agent",
                "--project",
                "Vegvisir",
                "--correlation-id",
                correlation_id,
                "--provider",
                "anthropic",
                "--model",
                "agent-runtime-test",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let reviewer_request: Value = serde_json::from_slice(&reviewer_request_output.stdout).unwrap();
    assert!(
        reviewer_request["prompt_cache_trace"]["breakpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|breakpoint| breakpoint["zone"] == "StableMemoryCapsule")
    );
    assert_eq!(
        reviewer_request["cached_prompt"]["manifest"]["scope_identity"]["project_id"],
        "Vegvisir"
    );
    assert_eq!(
        reviewer_request["prepared_context"]["metadata"]["correlation_id"],
        correlation_id
    );
    assert!(
        reviewer_request["prepared_context"]["metadata"]["retrieval_results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["memory_id"] == coder_writeback_memory_id)
    );

    let outsider_context_output = assert_success(
        cms_cmd(&db)
            .args([
                "prepare-context",
                "Review the Vegvisir private operator note",
                "--user",
                "external-agent",
                "--project",
                "Vegvisir",
                "--correlation-id",
                correlation_id,
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let outsider_context: Value = serde_json::from_slice(&outsider_context_output.stdout).unwrap();
    let outsider_packed = outsider_context["packed_text"].as_str().unwrap();
    assert!(!outsider_packed.contains("Vegvisir Private Operator Note"));
    assert!(!outsider_packed.contains("VEGVISIR_PRIVATE_OPERATOR_NOTE"));
    assert_eq!(
        outsider_context["metadata"]["correlation_id"],
        correlation_id
    );

    let writeback_output = assert_success(
        cms_cmd(&db)
            .args([
                "complete-turn",
                "Capture the Vegvisir runtime handoff decision.",
                "Decision: Vegvisir agent runtime must use CMS for durable shared memory, ECM for scoped context assembly, and prompt-cache manifests for provider handoff.",
                "--user",
                "codex-agent",
                "--project",
                "Vegvisir",
                "--correlation-id",
                correlation_id,
                "--commit",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let writeback: Value = serde_json::from_slice(&writeback_output.stdout).unwrap();
    assert_eq!(writeback.as_array().unwrap().len(), 1);
    let reviewer_writeback_memory_id = writeback[0]["memory_id"].as_str().unwrap();
    assert!(reviewer_writeback_memory_id.starts_with("mem_ecm_"));
    assert_eq!(writeback[0]["trace"]["correlation_id"], correlation_id);

    let retrieve_output = assert_success(
        cms_cmd(&db)
            .args([
                "retrieve",
                "durable shared memory scoped context prompt-cache manifests",
                "--mode",
                "hybrid",
                "--user",
                "codex-agent",
                "--project",
                "Vegvisir",
                "--correlation-id",
                correlation_id,
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let retrieval: Value = serde_json::from_slice(&retrieve_output.stdout).unwrap();
    assert!(
        retrieval["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["memory"]["id"] == coder_writeback_memory_id)
    );
    assert!(
        retrieval["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["memory"]["id"] == reviewer_writeback_memory_id)
    );
    assert_eq!(retrieval["trace"]["scope_project_id"], "Vegvisir");
    assert_eq!(retrieval["trace"]["scope_user_id"], "codex-agent");
    assert_eq!(retrieval["trace"]["correlation_id"], correlation_id);

    let usage_output = assert_success(
        cms_cmd(&db)
            .args([
                "prompt-cache",
                "record-usage",
                planner_manifest_id,
                "--provider-cached-input-tokens",
                "128",
                "--provider-cache-read-tokens",
                "96",
                "--latency-ms",
                "240",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let usage: Value = serde_json::from_slice(&usage_output.stdout).unwrap();
    assert_eq!(usage["usage"]["provider_cached_input_tokens"], 128);
    assert_eq!(usage["usage"]["provider_cache_read_tokens"], 96);

    let diagnostics_output = assert_success(
        cms_cmd(&db)
            .arg("diagnostics")
            .arg(tempdir.path())
            .args(["--audit-limit", "20", "--json"])
            .output()
            .unwrap(),
    );
    let diagnostics: Value = serde_json::from_slice(&diagnostics_output.stdout).unwrap();
    assert_eq!(diagnostics["health"]["status"], "healthy");
    assert!(diagnostics["prompt_cache"]["manifests"].as_u64().unwrap() >= 2);
    assert!(
        diagnostics["prompt_cache"]["usage_records"]
            .as_u64()
            .unwrap()
            >= 3
    );
    assert!(
        diagnostics["observability"]["retrieval_events"]
            .as_u64()
            .unwrap()
            >= 1
    );
    assert!(
        diagnostics["observability"]["prompt_cache_events"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn cli_import_usrl_authoritative_validation_json_smoke() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let usrl_path = tempdir.path().join("validated.usrl");
    fs::write(
        &usrl_path,
        r#"
contract ValidatedPolicy {
  section RuntimeBoundary {
    rule SafeExecution {
      permit checked_execution();
    }
  }
}
"#,
    )
    .unwrap();

    assert_success(cms_cmd(&db).arg("init").output().unwrap());
    let import_output = assert_success(
        cms_cmd(&db)
            .args([
                "import-usrl",
                "--ingest",
                "--validator-root",
                usrl_validator_root().to_str().unwrap(),
                "--require-validation",
                "--json",
            ])
            .arg(&usrl_path)
            .output()
            .unwrap(),
    );
    let imported: Value = serde_json::from_slice(&import_output.stdout).unwrap();
    assert_eq!(imported["validation"]["status"], "Valid");
    assert_eq!(imported["validation"]["module_count"], 1);
    assert_eq!(
        imported["memory"]["metadata"]["usrl_validation_status"],
        "Valid"
    );

    let retrieve_output = assert_success(
        cms_cmd(&db)
            .args(["retrieve", "ValidatedPolicy", "--mode", "exact", "--json"])
            .output()
            .unwrap(),
    );
    let retrieval: Value = serde_json::from_slice(&retrieve_output.stdout).unwrap();
    assert_eq!(
        retrieval["results"][0]["memory"]["metadata"]["usrl_validation_status"],
        "Valid"
    );
}

#[test]
fn cli_export_archive_redaction_flags_are_reflected_in_manifest_and_files() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let archive_dir = tempdir.path().join("redacted-cli-archive");

    let mut ledger = SqliteLedger::open(&db).unwrap();
    let mut private = scoped_memory(
        "mem_cli_private_export",
        "CLI private export",
        "private",
        Some("alice"),
        Some("cms"),
    );
    private.body = "Private body".to_string();
    let mut sensitive = scoped_memory(
        "mem_cli_sensitive_export",
        "CLI sensitive export",
        "public",
        None,
        None,
    );
    sensitive.body = format!("password = {}", fake_openai_password_value());
    ledger.upsert_memory(&private, None).unwrap();
    ledger.upsert_memory(&sensitive, None).unwrap();
    drop(ledger);

    let archive_output = assert_success(
        cms_cmd(&db)
            .args([
                "export-archive",
                "--exclude-private",
                "--redact-sensitive",
                "--json",
            ])
            .arg(&archive_dir)
            .output()
            .unwrap(),
    );
    let manifest: Value = serde_json::from_slice(&archive_output.stdout).unwrap();
    assert_eq!(manifest["memory_count"], 1);
    assert_eq!(manifest["redaction_policy"]["exclude_private"], true);
    assert_eq!(manifest["redaction_policy"]["redact_sensitive"], true);
    assert!(
        !archive_dir
            .join("memories/mem_cli_private_export.lml")
            .exists()
    );

    let exported_lml =
        std::fs::read_to_string(archive_dir.join("memories/mem_cli_sensitive_export.lml")).unwrap();
    assert!(exported_lml.contains("[redacted sensitive content]"));
    assert!(!exported_lml.contains(&fake_openai_password_value()));
}

#[test]
fn cli_export_json_scope_and_redaction_smoke() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let output_path = tempdir.path().join("cms-export.json");

    let mut ledger = SqliteLedger::open(&db).unwrap();
    let mut private = scoped_memory(
        "mem_cli_json_private_export",
        "CLI JSON private export",
        "private",
        Some("alice"),
        Some("cms"),
    );
    private.body = "Private JSON export body".to_string();
    let mut sensitive = scoped_memory(
        "mem_cli_json_sensitive_export",
        "CLI JSON sensitive export",
        "public",
        Some("alice"),
        Some("cms"),
    );
    sensitive.body = format!("secret_key = {}", fake_openai_password_value());
    ledger.upsert_memory(&private, None).unwrap();
    ledger.upsert_memory(&sensitive, None).unwrap();
    drop(ledger);

    let output = assert_success(
        cms_cmd(&db)
            .args([
                "export-json",
                "--user",
                "alice",
                "--project",
                "cms",
                "--exclude-private",
                "--redact-sensitive",
                "--json",
            ])
            .arg(&output_path)
            .output()
            .unwrap(),
    );
    let export: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(export["memory_count"], 1);
    assert_eq!(export["scope_filter"]["user_id"], "alice");
    assert_eq!(export["redaction_policy"]["redact_sensitive"], true);
    assert_eq!(export["memories"][0]["id"], "mem_cli_json_sensitive_export");
    assert_eq!(
        export["memories"][0]["body"],
        "[redacted sensitive content]"
    );
    let file_export: Value =
        serde_json::from_str(&fs::read_to_string(output_path).unwrap()).unwrap();
    assert_eq!(file_export["memory_count"], 1);
    assert!(
        !serde_json::to_string(&file_export)
            .unwrap()
            .contains(&fake_openai_password_value())
    );
}

#[test]
fn cli_restore_json_round_trip_smoke() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let restored_db = tempdir.path().join("restored.sqlite3");
    let output_path = tempdir.path().join("cms-export.json");
    let mut ledger = SqliteLedger::open(&db).unwrap();
    let memory = LmlParser::parse_file("memories/cms_hybrid_architecture.lml").unwrap();
    ledger.upsert_memory(&memory, None).unwrap();
    drop(ledger);

    assert_success(
        cms_cmd(&db)
            .arg("export-json")
            .arg(&output_path)
            .output()
            .unwrap(),
    );
    let restore_output = assert_success(
        cms_cmd(&restored_db)
            .args(["restore-json", "--json"])
            .arg(&output_path)
            .output()
            .unwrap(),
    );
    let report: Value = serde_json::from_slice(&restore_output.stdout).unwrap();
    assert_eq!(report["restored_memories"], 1);

    let retrieval_output = assert_success(
        cms_cmd(&restored_db)
            .args(["retrieve", "hybrid memory substrates", "--json"])
            .output()
            .unwrap(),
    );
    let retrieval: Value = serde_json::from_slice(&retrieval_output.stdout).unwrap();
    assert!(!retrieval["results"].as_array().unwrap().is_empty());
}

#[test]
fn cli_backup_db_json_smoke_creates_restorable_copy() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let backup = tempdir.path().join("backup/cms.sqlite3");
    let mut ledger = SqliteLedger::open(&db).unwrap();
    let memory = LmlParser::parse_text(SAMPLE).unwrap();
    ledger.upsert_memory(&memory, None).unwrap();
    drop(ledger);

    let output = assert_success(
        cms_cmd(&db)
            .args(["backup-db", "--json"])
            .arg(&backup)
            .output()
            .unwrap(),
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["source_schema_version"], CURRENT_SCHEMA_VERSION);
    assert_eq!(report["source_active_memories"], 1);
    assert!(report["backup_size_bytes"].as_u64().unwrap() > 0);

    let backup_ledger = SqliteLedger::open(&backup).unwrap();
    assert_eq!(backup_ledger.stats().unwrap().active_memories, 1);
}

#[test]
fn cli_prompt_cache_invalidate_scope_json_smoke() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let mut ledger = SqliteLedger::open(&db).unwrap();
    let request = PromptCachePrepareRequest {
        provider: "openai".to_string(),
        model: "gpt-test".to_string(),
        scope_identity: CacheScopeIdentity {
            organization_id: None,
            user_id: Some("alice".to_string()),
            project_id: Some("CMS".to_string()),
            session_id: Some("session-alice".to_string()),
            shared_scope_id: None,
        },
        ..Default::default()
    };
    let envelope = PromptCacheEngine::prepare_model_prompt(
        &prepared_prompt_cache_context("source-version-cli"),
        request,
    );
    ledger.put_prompt_cache_envelope(&envelope).unwrap();
    drop(ledger);

    let output = assert_success(
        cms_cmd(&db)
            .args([
                "prompt-cache",
                "invalidate-scope",
                "--user",
                "alice",
                "--reason",
                "scope-policy-changed",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["invalidations"].as_array().unwrap().len(), 1);
    assert!(report["evicted_capsules"].as_u64().unwrap() > 0);
}

#[test]
fn cli_import_jsonl_preview_output_and_ingest_smoke() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let jsonl_path = tempdir.path().join("records.jsonl");
    let output_dir = tempdir.path().join("jsonl-output");
    fs::write(
        &jsonl_path,
        r#"{"title":"JSONL Note","body":"JSONL import should produce retrievable memories.","tags":["jsonl"],"metadata":{"visibility":"public"}}
bad json
"#,
    )
    .unwrap();

    assert_success(cms_cmd(&db).arg("init").output().unwrap());
    let preview_output = assert_success(
        cms_cmd(&db)
            .args(["import-jsonl", "--preview", "--json"])
            .arg(&jsonl_path)
            .output()
            .unwrap(),
    );
    let preview: Value = serde_json::from_slice(&preview_output.stdout).unwrap();
    assert_eq!(preview["source_kind"], "jsonl-record");
    assert_eq!(preview["memories"], 1);

    assert_success(
        cms_cmd(&db)
            .args(["import-jsonl", "--output-dir"])
            .arg(&output_dir)
            .arg(&jsonl_path)
            .output()
            .unwrap(),
    );
    assert_lml_output_dir_parseable(&output_dir, 1, "jsonl-record");

    let import_output = assert_success(
        cms_cmd(&db)
            .args(["import-jsonl", "--ingest", "--json"])
            .arg(&jsonl_path)
            .output()
            .unwrap(),
    );
    let import_report: Value = serde_json::from_slice(&import_output.stdout).unwrap();
    assert_eq!(import_report["importer_version"], "jsonl-record-v1");
    assert_eq!(import_report["generated_memories"], 1);
    assert_eq!(import_report["warnings"].as_array().unwrap().len(), 1);

    let audit_output = assert_success(
        cms_cmd(&db)
            .args(["audit", "--limit", "5", "--json"])
            .output()
            .unwrap(),
    );
    let audit: Value = serde_json::from_slice(&audit_output.stdout).unwrap();
    assert!(audit.as_array().unwrap().iter().any(|event| {
        event["event_type"] == "import.completed"
            && event["message"]
                .as_str()
                .unwrap()
                .contains("importer=jsonl-record-v1")
    }));

    let diagnostics_output = assert_success(
        cms_cmd(&db)
            .arg("diagnostics")
            .arg(tempdir.path())
            .args(["--json"])
            .output()
            .unwrap(),
    );
    let diagnostics: Value = serde_json::from_slice(&diagnostics_output.stdout).unwrap();
    assert_eq!(diagnostics["observability"]["import_events"], 1);

    let retrieve_output = assert_success(
        cms_cmd(&db)
            .args(["retrieve", "retrievable memories", "--json"])
            .output()
            .unwrap(),
    );
    let retrieval: Value = serde_json::from_slice(&retrieve_output.stdout).unwrap();
    assert!(
        retrieval["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["memory"]["memory_type"] == "jsonl-record")
    );
}

#[test]
fn cli_import_jsonl_quarantines_secret_like_content() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let jsonl_path = tempdir.path().join("secret.jsonl");
    fs::write(
        &jsonl_path,
        format!(
            r#"{{"title":"Secret JSONL","body":"api_key = {}"}}"#,
            fake_openai_key()
        ),
    )
    .unwrap();

    assert_success(cms_cmd(&db).arg("init").output().unwrap());
    let import_output = assert_success(
        cms_cmd(&db)
            .args(["import-jsonl", "--ingest", "--json"])
            .arg(&jsonl_path)
            .output()
            .unwrap(),
    );
    let import_report: Value = serde_json::from_slice(&import_output.stdout).unwrap();
    assert_eq!(import_report["generated_memories"], 1);
    assert_eq!(
        import_report["quarantined_memory_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        import_report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("quarantined"))
    );

    let status_output = assert_success(
        cms_cmd(&db)
            .args(["status", "memories", "--json"])
            .output()
            .unwrap(),
    );
    let status: Value = serde_json::from_slice(&status_output.stdout).unwrap();
    assert_eq!(status["stats"]["active_memories"], 0);
    assert_eq!(status["stats"]["quarantined_memories"], 1);
}

#[test]
fn cli_import_doc_preview_output_and_ingest_smoke() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let doc_path = tempdir.path().join("notes.md");
    let output_dir = tempdir.path().join("lml-output");
    fs::write(
        &doc_path,
        "# Import Notes\n\nMarkdown document import should preserve source provenance.",
    )
    .unwrap();

    assert_success(cms_cmd(&db).arg("init").output().unwrap());
    let preview_output = assert_success(
        cms_cmd(&db)
            .args(["import-doc", "--preview", "--json"])
            .arg(&doc_path)
            .output()
            .unwrap(),
    );
    let preview: Value = serde_json::from_slice(&preview_output.stdout).unwrap();
    assert_eq!(preview["source_kind"], "markdown");
    assert_eq!(preview["memories"], 1);

    assert_success(
        cms_cmd(&db)
            .args(["import-doc", "--output-dir"])
            .arg(&output_dir)
            .arg(&doc_path)
            .output()
            .unwrap(),
    );
    assert_lml_output_dir_parseable(&output_dir, 1, "markdown-document");

    let import_output = assert_success(
        cms_cmd(&db)
            .args(["import-doc", "--ingest", "--json"])
            .arg(&doc_path)
            .output()
            .unwrap(),
    );
    let import_report: Value = serde_json::from_slice(&import_output.stdout).unwrap();
    assert_eq!(import_report["importer_version"], "generic-document-v1");
    assert_eq!(import_report["generated_memories"], 1);
    assert_eq!(import_report["unique_memories"], 1);
    assert_eq!(import_report["duplicate_memories"], 0);
    assert_eq!(import_report["memory_ids"].as_array().unwrap().len(), 1);
    assert_eq!(import_report["source_hashes"].as_array().unwrap().len(), 1);
    assert_eq!(
        import_report["import_batch_hash"].as_str().unwrap().len(),
        64
    );
    let retrieve_output = assert_success(
        cms_cmd(&db)
            .args(["retrieve", "source provenance", "--json"])
            .output()
            .unwrap(),
    );
    let retrieval: Value = serde_json::from_slice(&retrieve_output.stdout).unwrap();
    assert!(
        retrieval["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["memory"]["memory_type"] == "markdown-document")
    );
}

#[test]
fn cli_import_doc_dir_preview_output_and_ingest_smoke() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let docs_dir = tempdir.path().join("docs");
    let output_dir = tempdir.path().join("doc-lml-output");
    fs::create_dir_all(docs_dir.join("target")).unwrap();
    fs::write(
        docs_dir.join("guide.md"),
        "# Directory Import\n\nDirectory import should ingest markdown files.",
    )
    .unwrap();
    fs::write(
        docs_dir.join("lib.rs"),
        "pub fn directory_import_code_fixture() {}",
    )
    .unwrap();
    fs::write(docs_dir.join("bad.txt"), [0xff, 0xfe, 0xfd]).unwrap();
    fs::write(docs_dir.join("target").join("ignored.md"), "# Ignored").unwrap();

    assert_success(cms_cmd(&db).arg("init").output().unwrap());
    let preview_output = assert_success(
        cms_cmd(&db)
            .args(["import-doc-dir", "--preview", "--json"])
            .arg(&docs_dir)
            .output()
            .unwrap(),
    );
    let preview: Value = serde_json::from_slice(&preview_output.stdout).unwrap();
    assert_eq!(preview["memories"], 2);

    assert_success(
        cms_cmd(&db)
            .args(["import-doc-dir", "--output-dir"])
            .arg(&output_dir)
            .arg(&docs_dir)
            .output()
            .unwrap(),
    );
    assert_lml_output_dir_parseable(&output_dir, 2, "markdown-document");
    let output_memories = parse_lml_output_dir(&output_dir);
    assert!(
        output_memories
            .iter()
            .any(|memory| memory.memory_type == "code-document")
    );

    let import_output = assert_success(
        cms_cmd(&db)
            .args(["import-doc-dir", "--ingest", "--json"])
            .arg(&docs_dir)
            .output()
            .unwrap(),
    );
    let import_report: Value = serde_json::from_slice(&import_output.stdout).unwrap();
    assert_eq!(import_report["importer_version"], "generic-document-v1");
    assert_eq!(import_report["generated_memories"], 2);
    assert_eq!(import_report["unique_memories"], 2);
    assert_eq!(import_report["duplicate_memories"], 0);
    assert_eq!(import_report["memory_ids"].as_array().unwrap().len(), 2);
    assert_eq!(import_report["source_hashes"].as_array().unwrap().len(), 2);
    assert_eq!(import_report["warnings"].as_array().unwrap().len(), 1);
    assert!(
        import_report["warnings"][0]
            .as_str()
            .unwrap()
            .contains("bad.txt")
    );
    let retrieve_output = assert_success(
        cms_cmd(&db)
            .args(["retrieve", "directory import", "--json"])
            .output()
            .unwrap(),
    );
    let retrieval: Value = serde_json::from_slice(&retrieve_output.stdout).unwrap();
    assert!(
        retrieval["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["memory"]["memory_type"] == "markdown-document")
    );
}

#[test]
fn cli_import_doc_quarantines_secret_like_content() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("cms.sqlite3");
    let doc_path = tempdir.path().join("secret.md");
    fs::write(
        &doc_path,
        format!("# Secret Fixture\n\napi_key = {}", fake_openai_key()),
    )
    .unwrap();

    assert_success(cms_cmd(&db).arg("init").output().unwrap());
    let import_output = assert_success(
        cms_cmd(&db)
            .args(["import-doc", "--ingest", "--json"])
            .arg(&doc_path)
            .output()
            .unwrap(),
    );
    let import_report: Value = serde_json::from_slice(&import_output.stdout).unwrap();
    assert_eq!(import_report["generated_memories"], 1);
    assert_eq!(
        import_report["quarantined_memory_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        import_report["warnings"][0]
            .as_str()
            .unwrap()
            .contains("quarantined")
    );

    let status_output = assert_success(
        cms_cmd(&db)
            .args(["status", tempdir.path().to_str().unwrap(), "--json"])
            .output()
            .unwrap(),
    );
    let status: Value = serde_json::from_slice(&status_output.stdout).unwrap();
    assert_eq!(status["stats"]["active_memories"], 0);
    assert_eq!(status["stats"]["quarantined_memories"], 1);

    let retrieve_output = assert_success(
        cms_cmd(&db)
            .args(["retrieve", "Secret Fixture", "--json"])
            .output()
            .unwrap(),
    );
    let retrieval: Value = serde_json::from_slice(&retrieve_output.stdout).unwrap();
    assert!(retrieval["results"].as_array().unwrap().is_empty());

    let quarantined_output = assert_success(
        cms_cmd(&db)
            .args(["list", "--status", "quarantined", "--json"])
            .output()
            .unwrap(),
    );
    let quarantined: Value = serde_json::from_slice(&quarantined_output.stdout).unwrap();
    assert_eq!(
        quarantined[0]["id"],
        import_report["quarantined_memory_ids"][0]
    );
}

fn cms_bin() -> PathBuf {
    option_env!("CARGO_BIN_EXE_cms")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/cms"))
}

fn cms_cmd(db: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new(cms_bin());
    command.arg("--db").arg(db);
    command
}

fn assert_success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "command failed\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn assert_failure(output: Output) -> Output {
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn assert_lml_output_dir_parseable(output_dir: &Path, expected_count: usize, expected_type: &str) {
    let memories = parse_lml_output_dir(output_dir);
    assert_eq!(memories.len(), expected_count);
    assert!(
        memories
            .iter()
            .all(|memory| !memory.id.is_empty() && !memory.title.is_empty())
    );
    assert!(
        memories
            .iter()
            .any(|memory| memory.memory_type == expected_type),
        "expected at least one {expected_type} memory in {}",
        output_dir.display()
    );
}

fn parse_lml_output_dir(output_dir: &Path) -> Vec<MemoryObject> {
    let mut paths = fs::read_dir(output_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("lml"))
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .iter()
        .map(|path| LmlParser::parse_file(path).unwrap())
        .collect()
}

fn write_chatgpt_export(dir: &std::path::Path) {
    fs::write(
        dir.join("conversations.json"),
        r#"
[
  {
    "id": "conv_cms_import",
    "title": "CMS import planning",
    "create_time": 1770000000.0,
    "update_time": 1770000200.0,
    "mapping": {
      "root": {
        "message": null
      },
      "msg_1": {
        "message": {
          "id": "msg_1",
          "author": { "role": "user" },
          "create_time": 1770000001.0,
          "content": {
            "content_type": "text",
            "parts": ["How should CMS import chats?"]
          }
        }
      },
      "msg_2": {
        "message": {
          "id": "msg_2",
          "author": { "role": "assistant" },
          "create_time": 1770000002.0,
          "content": {
            "content_type": "text",
            "parts": ["Use chunked memory objects."]
          }
        }
      },
      "msg_3": {
        "message": {
          "id": "msg_3",
          "author": { "role": "user" },
          "create_time": 1770000003.0,
          "content": {
            "content_type": "text",
            "parts": ["Preserve the full transcript."]
          }
        }
      }
    }
  },
  {
    "id": "conv_other",
    "title": "Other topic",
    "create_time": 1770001000.0,
    "update_time": 1770001000.0,
    "mapping": {
      "msg_4": {
        "message": {
          "id": "msg_4",
          "author": { "role": "user" },
          "create_time": 1770001000.0,
          "content": {
            "content_type": "text",
            "parts": ["A separate conversation."]
          }
        }
      }
    }
  }
]
"#,
    )
    .unwrap();
}
