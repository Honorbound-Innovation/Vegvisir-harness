use crate::core::{Claim, MemoryLink, MemoryObject, MemorySearchResult, MemorySource};
use crate::prompt_cache::{
    CacheScope, CacheScopeIdentity, CachedPromptEnvelope, PromptCacheManifest, PromptCacheUsage,
    PromptCapsule, PromptCapsuleType,
};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub const CURRENT_SCHEMA_VERSION: i64 = 3;
const INITIAL_SCHEMA_MIGRATION: i64 = 1;
const SOURCE_COLUMNS_MIGRATION: i64 = 2;
const PROMPT_CACHE_TABLES_MIGRATION: i64 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryVersionRecord {
    pub memory_id: String,
    pub version: i64,
    pub content_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: String,
    pub memory_id: Option<String>,
    pub event_type: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryLedgerRecord {
    pub id: String,
    pub lml_path: Option<String>,
    pub body_hash: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryListEntry {
    pub id: String,
    pub memory_type: String,
    pub title: String,
    pub lml_path: Option<String>,
    pub body_hash: String,
    pub status: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryScopeEntry {
    pub id: String,
    pub memory_type: String,
    pub title: String,
    pub status: String,
    pub visibility: String,
    pub user_id: Option<String>,
    pub project_id: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerStats {
    pub schema_version: i64,
    pub active_memories: i64,
    pub deleted_memories: i64,
    pub archived_memories: i64,
    pub quarantined_memories: i64,
    pub superseded_memories: i64,
    pub claims: i64,
    pub links: i64,
    pub tags: i64,
    pub versions: i64,
    pub retrieval_logs: i64,
    pub audit_events: i64,
    pub graph_nodes: i64,
    pub graph_edges: i64,
    pub vector_chunks: i64,
    pub vector_terms: i64,
    pub graph_indexed_memories: i64,
    pub vector_indexed_memories: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaMigrationRecord {
    pub version: i64,
    pub name: String,
    pub checksum: String,
    pub applied_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptCacheManifestRecord {
    pub manifest: PromptCacheManifest,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptCacheUsageRecord {
    pub id: String,
    pub usage: PromptCacheUsage,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptCacheInvalidationRecord {
    pub id: String,
    pub manifest_id: String,
    pub reason: String,
    pub changed_source: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptCacheScopeInvalidationReport {
    pub invalidations: Vec<PromptCacheInvalidationRecord>,
    pub evicted_capsules: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptCapsuleRecord {
    pub capsule: PromptCapsule,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub use_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptCacheStats {
    pub manifests: i64,
    pub blocks: i64,
    pub capsules: i64,
    pub usage_records: i64,
    pub invalidations: i64,
}

pub struct SqliteLedger {
    conn: Connection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SqliteBackupReport {
    pub output_path: String,
    pub source_schema_version: i64,
    pub source_active_memories: i64,
    pub backup_size_bytes: u64,
}

impl SqliteLedger {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        let ledger = Self { conn };
        ledger.migrate()?;
        Ok(ledger)
    }

    pub fn open_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let ledger = Self { conn };
        ledger.migrate()?;
        Ok(ledger)
    }

    pub fn migrate(&self) -> anyhow::Result<()> {
        self.conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        self.ensure_schema_migration_table()?;

        let initial_schema = r#"
            CREATE TABLE IF NOT EXISTS memories (
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

            CREATE TABLE IF NOT EXISTS claims (
                id TEXT NOT NULL,
                memory_id TEXT NOT NULL,
                text TEXT NOT NULL,
                confidence REAL,
                source TEXT,
                PRIMARY KEY(memory_id, id),
                FOREIGN KEY(memory_id) REFERENCES memories(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS memory_links (
                id TEXT PRIMARY KEY,
                source_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                relation TEXT NOT NULL,
                confidence REAL,
                FOREIGN KEY(source_id) REFERENCES memories(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS memory_tags (
                memory_id TEXT NOT NULL,
                tag TEXT NOT NULL,
                PRIMARY KEY(memory_id, tag),
                FOREIGN KEY(memory_id) REFERENCES memories(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS memory_metadata (
                memory_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                PRIMARY KEY(memory_id, key),
                FOREIGN KEY(memory_id) REFERENCES memories(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS index_state (
                memory_id TEXT NOT NULL,
                index_name TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                indexed_at TEXT NOT NULL,
                status TEXT NOT NULL,
                PRIMARY KEY(memory_id, index_name)
            );

            CREATE TABLE IF NOT EXISTS retrieval_logs (
                id TEXT PRIMARY KEY,
                query TEXT NOT NULL,
                selected_memory_ids TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS memory_versions (
                memory_id TEXT NOT NULL,
                version INTEGER NOT NULL,
                content_hash TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY(memory_id, version),
                FOREIGN KEY(memory_id) REFERENCES memories(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS audit_events (
                id TEXT PRIMARY KEY,
                memory_id TEXT,
                event_type TEXT NOT NULL,
                message TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS graph_nodes (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                label TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS graph_edges (
                id TEXT PRIMARY KEY,
                source_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                relation TEXT NOT NULL,
                confidence REAL NOT NULL,
                memory_id TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS vector_chunks (
                chunk_id TEXT PRIMARY KEY,
                memory_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                ordinal INTEGER NOT NULL,
                text TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS vector_terms (
                chunk_id TEXT NOT NULL,
                memory_id TEXT NOT NULL,
                term TEXT NOT NULL,
                weight REAL NOT NULL,
                PRIMARY KEY(chunk_id, term),
                FOREIGN KEY(chunk_id) REFERENCES vector_chunks(chunk_id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_vector_terms_term ON vector_terms(term);
            CREATE INDEX IF NOT EXISTS idx_vector_terms_memory ON vector_terms(memory_id);
            "#;
        if !self.has_migration(INITIAL_SCHEMA_MIGRATION)? {
            self.conn.execute_batch(initial_schema)?;
            self.record_migration(
                INITIAL_SCHEMA_MIGRATION,
                "initial-ledger-schema",
                initial_schema,
            )?;
        } else {
            self.conn.execute_batch(initial_schema)?;
        }

        let source_columns = "ALTER TABLE memories ADD COLUMN source_kind TEXT; ALTER TABLE memories ADD COLUMN source_reference TEXT;";
        if !self.has_migration(SOURCE_COLUMNS_MIGRATION)? {
            self.ensure_column("memories", "source_kind", "TEXT")?;
            self.ensure_column("memories", "source_reference", "TEXT")?;
            self.record_migration(
                SOURCE_COLUMNS_MIGRATION,
                "memory-source-columns",
                source_columns,
            )?;
        } else {
            self.ensure_column("memories", "source_kind", "TEXT")?;
            self.ensure_column("memories", "source_reference", "TEXT")?;
        }

        let prompt_cache_tables = r#"
            CREATE TABLE IF NOT EXISTS prompt_cache_manifests (
                manifest_id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                prompt_cache_key TEXT NOT NULL,
                cacheable_prefix_hash TEXT NOT NULL,
                cacheable_prefix_tokens INTEGER NOT NULL,
                total_prompt_tokens INTEGER NOT NULL,
                renderer_version TEXT NOT NULL,
                tokenizer_version TEXT NOT NULL,
                scope_identity_json TEXT NOT NULL,
                block_hashes_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS prompt_cache_blocks (
                block_id TEXT NOT NULL,
                manifest_id TEXT NOT NULL,
                zone TEXT NOT NULL,
                kind TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                token_estimate INTEGER NOT NULL,
                stability TEXT NOT NULL,
                scope TEXT NOT NULL,
                sensitivity TEXT NOT NULL,
                source_memory_ids_json TEXT NOT NULL,
                source_version_hashes_json TEXT NOT NULL,
                cache_policy_json TEXT NOT NULL,
                provider_annotations_json TEXT NOT NULL,
                PRIMARY KEY(manifest_id, block_id),
                FOREIGN KEY(manifest_id) REFERENCES prompt_cache_manifests(manifest_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS prompt_cache_capsules (
                capsule_id TEXT PRIMARY KEY,
                capsule_type TEXT NOT NULL,
                scope TEXT NOT NULL,
                scope_identity_json TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                token_estimate INTEGER NOT NULL,
                source_memory_ids_json TEXT NOT NULL,
                source_version_hashes_json TEXT NOT NULL,
                block_ids_json TEXT NOT NULL,
                renderer_version TEXT NOT NULL,
                created_at TEXT NOT NULL,
                last_used_at TEXT NOT NULL,
                use_count INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS prompt_cache_usage (
                id TEXT PRIMARY KEY,
                manifest_id TEXT NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                total_input_tokens INTEGER NOT NULL,
                provider_cached_input_tokens INTEGER NOT NULL,
                provider_cache_write_tokens INTEGER NOT NULL,
                provider_cache_read_tokens INTEGER NOT NULL,
                local_capsule_hits INTEGER NOT NULL,
                local_capsule_misses INTEGER NOT NULL,
                latency_ms INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY(manifest_id) REFERENCES prompt_cache_manifests(manifest_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS prompt_cache_invalidations (
                id TEXT PRIMARY KEY,
                manifest_id TEXT NOT NULL,
                reason TEXT NOT NULL,
                changed_source TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY(manifest_id) REFERENCES prompt_cache_manifests(manifest_id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_prompt_cache_manifests_provider_model
                ON prompt_cache_manifests(provider, model);
            CREATE INDEX IF NOT EXISTS idx_prompt_cache_usage_manifest
                ON prompt_cache_usage(manifest_id);
            CREATE INDEX IF NOT EXISTS idx_prompt_cache_invalidations_manifest
                ON prompt_cache_invalidations(manifest_id);
            CREATE INDEX IF NOT EXISTS idx_prompt_cache_capsules_type_scope
                ON prompt_cache_capsules(capsule_type, scope);
            "#;
        if !self.has_migration(PROMPT_CACHE_TABLES_MIGRATION)? {
            self.conn.execute_batch(prompt_cache_tables)?;
            self.record_migration(
                PROMPT_CACHE_TABLES_MIGRATION,
                "prompt-cache-tables",
                prompt_cache_tables,
            )?;
        } else {
            self.conn.execute_batch(prompt_cache_tables)?;
        }
        Ok(())
    }

    pub fn upsert_memory(
        &mut self,
        memory: &MemoryObject,
        lml_path: Option<&Path>,
    ) -> anyhow::Result<()> {
        let tx = self.conn.transaction()?;
        let body_hash = content_hash(memory);
        let previous_hash = tx
            .query_row(
                "SELECT body_hash FROM memories WHERE id = ?1",
                params![memory.id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let event_type = match previous_hash.as_deref() {
            None => "memory.created",
            Some(previous) if previous != body_hash => "memory.updated",
            Some(_) => "memory.refreshed",
        };

        tx.execute(
            r#"
            INSERT INTO memories (
                id, type, title, summary, body, body_hash, lml_path,
                source_kind, source_reference, confidence, created_at, updated_at, status
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'active')
            ON CONFLICT(id) DO UPDATE SET
                type = excluded.type,
                title = excluded.title,
                summary = excluded.summary,
                body = excluded.body,
                body_hash = excluded.body_hash,
                lml_path = excluded.lml_path,
                source_kind = excluded.source_kind,
                source_reference = excluded.source_reference,
                confidence = excluded.confidence,
                updated_at = excluded.updated_at,
                status = 'active'
            "#,
            params![
                memory.id,
                memory.memory_type,
                memory.title,
                memory.summary,
                memory.body,
                body_hash,
                lml_path.map(|path| path.to_string_lossy().to_string()),
                memory.source.as_ref().map(|source| source.kind.as_str()),
                memory
                    .source
                    .as_ref()
                    .map(|source| source.reference.as_str()),
                memory.confidence,
                memory.created_at.to_rfc3339(),
                memory.updated_at.to_rfc3339(),
            ],
        )?;

        tx.execute(
            "DELETE FROM claims WHERE memory_id = ?1",
            params![memory.id],
        )?;
        for claim in &memory.claims {
            tx.execute(
                "INSERT INTO claims (id, memory_id, text, confidence, source) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![claim.id, memory.id, claim.text, claim.confidence, claim.source],
            )?;
        }

        tx.execute(
            "DELETE FROM memory_links WHERE source_id = ?1",
            params![memory.id],
        )?;
        for link in &memory.links {
            tx.execute(
                "INSERT INTO memory_links (id, source_id, target_id, relation, confidence) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    format!("{}:{}:{}", link.source_id, link.relation, link.target_id),
                    link.source_id,
                    link.target_id,
                    link.relation,
                    link.confidence
                ],
            )?;
        }

        tx.execute(
            "DELETE FROM memory_tags WHERE memory_id = ?1",
            params![memory.id],
        )?;
        for tag in &memory.tags {
            tx.execute(
                "INSERT OR IGNORE INTO memory_tags (memory_id, tag) VALUES (?1, ?2)",
                params![memory.id, tag],
            )?;
        }

        tx.execute(
            "DELETE FROM memory_metadata WHERE memory_id = ?1",
            params![memory.id],
        )?;
        for (key, value) in &memory.metadata {
            tx.execute(
                "INSERT INTO memory_metadata (memory_id, key, value) VALUES (?1, ?2, ?3)",
                params![memory.id, key, value],
            )?;
        }

        tx.execute(
            r#"
            INSERT INTO index_state (memory_id, index_name, content_hash, indexed_at, status)
            VALUES (?1, 'sqlite-ledger', ?2, ?3, 'indexed')
            ON CONFLICT(memory_id, index_name) DO UPDATE SET
                content_hash = excluded.content_hash,
                indexed_at = excluded.indexed_at,
                status = excluded.status
            "#,
            params![memory.id, body_hash, Utc::now().to_rfc3339()],
        )?;
        if previous_hash.as_deref() != Some(body_hash.as_str()) {
            let next_version = tx.query_row(
                "SELECT COALESCE(MAX(version), 0) + 1 FROM memory_versions WHERE memory_id = ?1",
                params![memory.id],
                |row| row.get::<_, i64>(0),
            )?;
            tx.execute(
                r#"
                INSERT INTO memory_versions (memory_id, version, content_hash, created_at)
                VALUES (?1, ?2, ?3, ?4)
                "#,
                params![memory.id, next_version, body_hash, Utc::now().to_rfc3339()],
            )?;
        }
        tx.execute(
            r#"
            INSERT INTO audit_events (id, memory_id, event_type, message, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                format!("aud_{}", uuid::Uuid::now_v7().simple()),
                memory.id,
                event_type,
                format!("{} {}", event_type, memory.title),
                Utc::now().to_rfc3339(),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_memory(&self, id: &str) -> anyhow::Result<Option<MemoryObject>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, type, title, summary, body, source_kind, source_reference, confidence, created_at, updated_at FROM memories WHERE id = ?1 AND status = 'active'",
        )?;
        let Some(mut memory) = stmt.query_row(params![id], row_to_memory).optional()? else {
            return Ok(None);
        };
        memory.claims = self.claims_for(id)?;
        memory.links = self.links_for(id)?;
        memory.tags = self.tags_for(id)?;
        memory.metadata = self.metadata_for(id)?;
        Ok(Some(memory))
    }

    pub fn soft_delete_memory(&self, id: &str) -> anyhow::Result<bool> {
        self.transition_memory_status(id, "deleted", "memory.deleted", "deleted")
    }

    pub fn restore_memory(&self, id: &str) -> anyhow::Result<bool> {
        let affected = self.conn.execute(
            r#"
            UPDATE memories
            SET status = 'active', updated_at = ?2
            WHERE id = ?1 AND status != 'active'
            "#,
            params![id, Utc::now().to_rfc3339()],
        )?;
        if affected == 0 {
            return Ok(false);
        }

        self.conn.execute(
            r#"
            INSERT INTO audit_events (id, memory_id, event_type, message, created_at)
            VALUES (?1, ?2, 'memory.restored', ?3, ?4)
            "#,
            params![
                format!("aud_{}", uuid::Uuid::now_v7().simple()),
                id,
                format!("memory.restored {id}"),
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(true)
    }

    pub fn archive_memory(&self, id: &str) -> anyhow::Result<bool> {
        self.transition_memory_status(id, "archived", "memory.archived", "archived")
    }

    pub fn quarantine_memory(&self, id: &str) -> anyhow::Result<bool> {
        self.transition_memory_status(id, "quarantined", "memory.quarantined", "quarantined")
    }

    pub fn supersede_memory(&self, id: &str, replacement_id: &str) -> anyhow::Result<bool> {
        if !self.active_memory_exists(replacement_id)? {
            anyhow::bail!("replacement memory is not active: {replacement_id}");
        }
        let transitioned = self.transition_memory_status(
            id,
            "superseded",
            "memory.superseded",
            &format!("superseded by {replacement_id}"),
        )?;
        if transitioned {
            self.insert_lifecycle_link(id, replacement_id, "superseded_by", 1.0)?;
        }
        Ok(transitioned)
    }

    pub fn merge_memory(&self, duplicate_id: &str, canonical_id: &str) -> anyhow::Result<bool> {
        if duplicate_id == canonical_id {
            anyhow::bail!("cannot merge a memory into itself: {duplicate_id}");
        }
        if !self.active_memory_exists(canonical_id)? {
            anyhow::bail!("canonical memory is not active: {canonical_id}");
        }
        let transitioned = self.transition_memory_status(
            duplicate_id,
            "superseded",
            "memory.merged",
            &format!("merged into {canonical_id}"),
        )?;
        if transitioned {
            self.insert_lifecycle_link(duplicate_id, canonical_id, "merged_into", 1.0)?;
        }
        Ok(transitioned)
    }

    fn active_memory_exists(&self, id: &str) -> anyhow::Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM memories WHERE id = ?1 AND status = 'active')",
                params![id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(Into::into)
    }

    fn insert_lifecycle_link(
        &self,
        source_id: &str,
        target_id: &str,
        relation: &str,
        confidence: f64,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO memory_links (id, source_id, target_id, relation, confidence)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                format!("{source_id}:{relation}:{target_id}"),
                source_id,
                target_id,
                relation,
                confidence,
            ],
        )?;
        Ok(())
    }

    fn transition_memory_status(
        &self,
        id: &str,
        status: &str,
        event_type: &str,
        message: &str,
    ) -> anyhow::Result<bool> {
        let affected = self.conn.execute(
            r#"
            UPDATE memories
            SET status = ?2, updated_at = ?3
            WHERE id = ?1 AND status = 'active'
            "#,
            params![id, status, Utc::now().to_rfc3339()],
        )?;
        if affected == 0 {
            return Ok(false);
        }

        self.conn.execute(
            "UPDATE index_state SET status = ?2, indexed_at = ?3 WHERE memory_id = ?1",
            params![id, status, Utc::now().to_rfc3339()],
        )?;
        self.conn.execute(
            r#"
            INSERT INTO audit_events (id, memory_id, event_type, message, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                format!("aud_{}", uuid::Uuid::now_v7().simple()),
                id,
                event_type,
                format!("{event_type} {id}: {message}"),
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(true)
    }

    pub fn all_memory_ids(&self) -> anyhow::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM memories WHERE status = 'active' ORDER BY id")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn active_memory_records(&self) -> anyhow::Result<Vec<MemoryLedgerRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, lml_path, body_hash, status FROM memories WHERE status = 'active' ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(MemoryLedgerRecord {
                id: row.get(0)?,
                lml_path: row.get(1)?,
                body_hash: row.get(2)?,
                status: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_memories(
        &self,
        status: MemoryStatusFilter,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryListEntry>> {
        let status_clause = match status {
            MemoryStatusFilter::Active => "WHERE status = 'active'",
            MemoryStatusFilter::Deleted => "WHERE status = 'deleted'",
            MemoryStatusFilter::Archived => "WHERE status = 'archived'",
            MemoryStatusFilter::Quarantined => "WHERE status = 'quarantined'",
            MemoryStatusFilter::Superseded => "WHERE status = 'superseded'",
            MemoryStatusFilter::Inactive => "WHERE status != 'active'",
            MemoryStatusFilter::All => "",
        };
        let sql = format!(
            r#"
            SELECT id, type, title, lml_path, body_hash, status, updated_at
            FROM memories
            {status_clause}
            ORDER BY updated_at DESC, id
            LIMIT ?1
            "#
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let updated_at: String = row.get(6)?;
            Ok(MemoryListEntry {
                id: row.get(0)?,
                memory_type: row.get(1)?,
                title: row.get(2)?,
                lml_path: row.get(3)?,
                body_hash: row.get(4)?,
                status: row.get(5)?,
                updated_at: parse_rfc3339_for_sql(6, &updated_at)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_memory_scope(&self, id: &str) -> anyhow::Result<Option<MemoryScopeEntry>> {
        self.memory_scope_query(
            r#"
            WHERE memories.id = ?1
            LIMIT 1
            "#,
            params![id],
        )
        .map(|mut entries| entries.pop())
    }

    pub fn list_memories_by_scope(
        &self,
        status: MemoryStatusFilter,
        visibility: Option<&str>,
        user_id: Option<&str>,
        project_id: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryScopeEntry>> {
        let status_clause = match status {
            MemoryStatusFilter::Active => "WHERE memories.status = 'active'",
            MemoryStatusFilter::Deleted => "WHERE memories.status = 'deleted'",
            MemoryStatusFilter::Archived => "WHERE memories.status = 'archived'",
            MemoryStatusFilter::Quarantined => "WHERE memories.status = 'quarantined'",
            MemoryStatusFilter::Superseded => "WHERE memories.status = 'superseded'",
            MemoryStatusFilter::Inactive => "WHERE memories.status != 'active'",
            MemoryStatusFilter::All => "",
        };
        let filter_prefix = if status_clause.is_empty() {
            "WHERE"
        } else {
            "AND"
        };
        let where_clause = format!(
            r#"
            {status_clause}
            {filter_prefix} (?2 IS NULL OR COALESCE(visibility.value, 'public') = ?2)
              AND (?3 IS NULL OR user_scope.value = ?3)
              AND (?4 IS NULL OR project_scope.value = ?4)
            ORDER BY memories.updated_at DESC, memories.id
            LIMIT ?1
            "#
        );
        self.memory_scope_query(
            &where_clause,
            params![limit as i64, visibility, user_id, project_id],
        )
    }

    pub fn stats(&self) -> anyhow::Result<LedgerStats> {
        Ok(LedgerStats {
            schema_version: self.schema_version()?,
            active_memories: self.count_where("memories", "status = 'active'")?,
            deleted_memories: self.count_where("memories", "status = 'deleted'")?,
            archived_memories: self.count_where("memories", "status = 'archived'")?,
            quarantined_memories: self.count_where("memories", "status = 'quarantined'")?,
            superseded_memories: self.count_where("memories", "status = 'superseded'")?,
            claims: self.count_table("claims")?,
            links: self.count_table("memory_links")?,
            tags: self.count_table("memory_tags")?,
            versions: self.count_table("memory_versions")?,
            retrieval_logs: self.count_table("retrieval_logs")?,
            audit_events: self.count_table("audit_events")?,
            graph_nodes: self.count_table("graph_nodes")?,
            graph_edges: self.count_table("graph_edges")?,
            vector_chunks: self.count_table("vector_chunks")?,
            vector_terms: self.count_table("vector_terms")?,
            graph_indexed_memories: self.count_where(
                "index_state",
                "index_name = 'sqlite-graph' AND status = 'indexed'",
            )?,
            vector_indexed_memories: self.count_where(
                "index_state",
                "index_name = 'sqlite-vector' AND status = 'indexed'",
            )?,
        })
    }

    pub fn schema_version(&self) -> anyhow::Result<i64> {
        self.conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn applied_migrations(&self) -> anyhow::Result<Vec<SchemaMigrationRecord>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT version, name, checksum, applied_at
            FROM schema_migrations
            ORDER BY version
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            let applied_at: String = row.get(3)?;
            Ok(SchemaMigrationRecord {
                version: row.get(0)?,
                name: row.get(1)?,
                checksum: row.get(2)?,
                applied_at: parse_rfc3339_for_sql(3, &applied_at)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn backup_to(&self, output_path: impl AsRef<Path>) -> anyhow::Result<SqliteBackupReport> {
        let output_path = output_path.as_ref();
        if let Some(parent) = output_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        if output_path.exists() {
            fs::remove_file(output_path)?;
        }
        let stats = self.stats()?;
        self.conn.execute(
            "VACUUM main INTO ?1",
            params![output_path.to_string_lossy()],
        )?;
        let backup_size_bytes = fs::metadata(output_path)?.len();
        self.log_audit_event(
            None,
            "sqlite.backup_created",
            &format!("created SQLite backup {}", output_path.display()),
        )?;
        Ok(SqliteBackupReport {
            output_path: output_path.to_string_lossy().to_string(),
            source_schema_version: stats.schema_version,
            source_active_memories: stats.active_memories,
            backup_size_bytes,
        })
    }

    pub fn memory_exists(&self, id: &str) -> anyhow::Result<bool> {
        let exists = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM memories WHERE id = ?1 AND status = 'active')",
            params![id],
            |row| row.get::<_, bool>(0),
        )?;
        Ok(exists)
    }

    pub fn memory_hash(&self, id: &str) -> anyhow::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT body_hash FROM memories WHERE id = ?1 AND status = 'active'",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn index_hash(&self, memory_id: &str, index_name: &str) -> anyhow::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT content_hash FROM index_state WHERE memory_id = ?1 AND index_name = ?2",
                params![memory_id, index_name],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn search_exact(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemorySearchResult>> {
        let pattern = format!("%{query}%");
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, type, title, summary, body, source_kind, source_reference, confidence, created_at, updated_at
            FROM memories
            WHERE status = 'active'
              AND (
                title LIKE ?1 OR summary LIKE ?1 OR body LIKE ?1
               OR EXISTS (
                    SELECT 1 FROM claims
                    WHERE claims.memory_id = memories.id
                      AND claims.text LIKE ?1
               )
               OR EXISTS (
                    SELECT 1 FROM memory_tags
                    WHERE memory_tags.memory_id = memories.id
                      AND memory_tags.tag LIKE ?1
               )
               OR EXISTS (
                    SELECT 1 FROM memory_metadata
                    WHERE memory_metadata.memory_id = memories.id
                      AND (memory_metadata.key LIKE ?1 OR memory_metadata.value LIKE ?1)
               )
               OR EXISTS (
                    SELECT 1 FROM memory_links
                    WHERE memory_links.source_id = memories.id
                      AND (memory_links.relation LIKE ?1 OR memory_links.target_id LIKE ?1)
               )
              )
            ORDER BY updated_at DESC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![pattern, limit as i64], row_to_memory)?;
        let mut results = Vec::new();
        for row in rows {
            let mut memory = row?;
            memory.claims = self.claims_for(&memory.id)?;
            memory.links = self.links_for(&memory.id)?;
            memory.tags = self.tags_for(&memory.id)?;
            memory.metadata = self.metadata_for(&memory.id)?;
            results.push(MemorySearchResult {
                memory,
                score: 1.0,
                reasons: vec!["sqlite exact text match".to_string()],
            });
        }
        Ok(results)
    }

    pub fn recent_memories(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemorySearchResult>> {
        let pattern = query_pattern(query);
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, type, title, summary, body, source_kind, source_reference, confidence, created_at, updated_at
            FROM memories
            WHERE status = 'active'
              AND (
                ?1 = '%%'
                OR title LIKE ?1 OR summary LIKE ?1 OR body LIKE ?1
                OR type LIKE ?1 OR lml_path LIKE ?1
                OR EXISTS (
                    SELECT 1 FROM claims
                    WHERE claims.memory_id = memories.id
                      AND claims.text LIKE ?1
                )
                OR EXISTS (
                    SELECT 1 FROM memory_tags
                    WHERE memory_tags.memory_id = memories.id
                      AND memory_tags.tag LIKE ?1
                )
                OR EXISTS (
                    SELECT 1 FROM memory_metadata
                    WHERE memory_metadata.memory_id = memories.id
                      AND (memory_metadata.key LIKE ?1 OR memory_metadata.value LIKE ?1)
                )
              )
            ORDER BY updated_at DESC, id
            LIMIT ?2
            "#,
        )?;
        self.search_results_from_stmt(
            &mut stmt,
            params![pattern, limit as i64],
            "recent ledger match",
            0.85,
        )
    }

    pub fn project_memories(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemorySearchResult>> {
        let pattern = query_pattern(query);
        let project_ref = format!("%project:{}%", query.trim());
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, type, title, summary, body, source_kind, source_reference, confidence, created_at, updated_at
            FROM memories
            WHERE status = 'active'
              AND (
                title LIKE ?1 OR summary LIKE ?1 OR body LIKE ?1 OR type LIKE ?1 OR lml_path LIKE ?1
                OR source_reference LIKE ?1
                OR EXISTS (
                    SELECT 1 FROM claims
                    WHERE claims.memory_id = memories.id
                      AND claims.text LIKE ?1
                )
                OR EXISTS (
                    SELECT 1 FROM memory_tags
                    WHERE memory_tags.memory_id = memories.id
                      AND (memory_tags.tag LIKE ?1 OR lower(memory_tags.tag) = 'project')
                )
                OR EXISTS (
                    SELECT 1 FROM memory_metadata
                    WHERE memory_metadata.memory_id = memories.id
                      AND (
                        memory_metadata.key LIKE '%project%'
                        OR memory_metadata.value LIKE ?1
                      )
                )
                OR EXISTS (
                    SELECT 1 FROM memory_links
                    WHERE memory_links.source_id = memories.id
                      AND (
                        memory_links.target_id LIKE ?1
                        OR memory_links.target_id LIKE ?2
                        OR memory_links.relation LIKE '%project%'
                      )
                )
              )
            ORDER BY updated_at DESC, id
            LIMIT ?3
            "#,
        )?;
        self.search_results_from_stmt(
            &mut stmt,
            params![pattern, project_ref, limit as i64],
            "project-scoped ledger match",
            1.05,
        )
    }

    pub fn decision_history(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemorySearchResult>> {
        let pattern = query_pattern(query);
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, type, title, summary, body, source_kind, source_reference, confidence, created_at, updated_at
            FROM memories
            WHERE status = 'active'
              AND (
                lower(type) LIKE '%decision%'
                OR lower(type) = 'adr'
                OR lower(title) LIKE '%decision%'
                OR EXISTS (
                    SELECT 1 FROM memory_tags
                    WHERE memory_tags.memory_id = memories.id
                      AND lower(memory_tags.tag) IN ('decision', 'adr', 'decision-history')
                )
                OR EXISTS (
                    SELECT 1 FROM memory_metadata
                    WHERE memory_metadata.memory_id = memories.id
                      AND (
                        lower(memory_metadata.key) LIKE '%decision%'
                        OR lower(memory_metadata.value) LIKE '%decision%'
                        OR lower(memory_metadata.value) = 'adr'
                      )
                )
              )
              AND (
                ?1 = '%%'
                OR title LIKE ?1 OR summary LIKE ?1 OR body LIKE ?1
                OR EXISTS (
                    SELECT 1 FROM claims
                    WHERE claims.memory_id = memories.id
                      AND claims.text LIKE ?1
                )
                OR EXISTS (
                    SELECT 1 FROM memory_tags
                    WHERE memory_tags.memory_id = memories.id
                      AND memory_tags.tag LIKE ?1
                )
                OR EXISTS (
                    SELECT 1 FROM memory_metadata
                    WHERE memory_metadata.memory_id = memories.id
                      AND (memory_metadata.key LIKE ?1 OR memory_metadata.value LIKE ?1)
                )
              )
            ORDER BY updated_at ASC, id
            LIMIT ?2
            "#,
        )?;
        self.search_results_from_stmt(
            &mut stmt,
            params![pattern, limit as i64],
            "decision-history ledger match",
            0.95,
        )
    }

    pub fn log_retrieval(&self, query: &str, selected_memory_ids: &[String]) -> anyhow::Result<()> {
        let retrieval_id = format!("ret_{}", uuid::Uuid::now_v7().simple());
        self.conn.execute(
            "INSERT INTO retrieval_logs (id, query, selected_memory_ids, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                retrieval_id,
                query,
                serde_json::to_string(selected_memory_ids)?,
                Utc::now().to_rfc3339()
            ],
        )?;
        self.conn.execute(
            r#"
            INSERT INTO audit_events (id, memory_id, event_type, message, created_at)
            VALUES (?1, NULL, 'retrieval.logged', ?2, ?3)
            "#,
            params![
                format!("aud_{}", uuid::Uuid::now_v7().simple()),
                format!(
                    "retrieved {} memories for query: {query}",
                    selected_memory_ids.len()
                ),
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn memory_versions(&self, memory_id: &str) -> anyhow::Result<Vec<MemoryVersionRecord>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT memory_id, version, content_hash, created_at
            FROM memory_versions
            WHERE memory_id = ?1
            ORDER BY version
            "#,
        )?;
        let rows = stmt.query_map(params![memory_id], |row| {
            let created_at: String = row.get(3)?;
            Ok(MemoryVersionRecord {
                memory_id: row.get(0)?,
                version: row.get(1)?,
                content_hash: row.get(2)?,
                created_at: parse_rfc3339_for_sql(3, &created_at)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn audit_events(&self, limit: usize) -> anyhow::Result<Vec<AuditEvent>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, memory_id, event_type, message, created_at
            FROM audit_events
            ORDER BY created_at DESC, id DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let created_at: String = row.get(4)?;
            Ok(AuditEvent {
                id: row.get(0)?,
                memory_id: row.get(1)?,
                event_type: row.get(2)?,
                message: row.get(3)?,
                created_at: parse_rfc3339_for_sql(4, &created_at)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn put_prompt_cache_envelope(
        &mut self,
        envelope: &CachedPromptEnvelope,
    ) -> anyhow::Result<()> {
        let tx = self.conn.transaction()?;
        let manifest = &envelope.manifest;
        tx.execute(
            r#"
            INSERT OR REPLACE INTO prompt_cache_manifests (
                manifest_id, provider, model, prompt_cache_key, cacheable_prefix_hash,
                cacheable_prefix_tokens, total_prompt_tokens, renderer_version,
                tokenizer_version, scope_identity_json, block_hashes_json, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
            params![
                manifest.manifest_id,
                manifest.provider,
                manifest.model,
                manifest.prompt_cache_key,
                manifest.cacheable_prefix_hash,
                manifest.cacheable_prefix_tokens as i64,
                manifest.total_prompt_tokens as i64,
                manifest.renderer_version,
                manifest.tokenizer_version,
                serde_json::to_string(&manifest.scope_identity)?,
                serde_json::to_string(&manifest.block_hashes)?,
                Utc::now().to_rfc3339(),
            ],
        )?;
        tx.execute(
            "DELETE FROM prompt_cache_blocks WHERE manifest_id = ?1",
            params![manifest.manifest_id],
        )?;
        for block in &envelope.blocks {
            tx.execute(
                r#"
                INSERT INTO prompt_cache_blocks (
                    block_id, manifest_id, zone, kind, content_hash, token_estimate,
                    stability, scope, sensitivity, source_memory_ids_json,
                    source_version_hashes_json, cache_policy_json, provider_annotations_json
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                "#,
                params![
                    block.id,
                    manifest.manifest_id,
                    format!("{:?}", block.zone),
                    format!("{:?}", block.kind),
                    block.content_hash,
                    block.token_estimate as i64,
                    format!("{:?}", block.stability),
                    format!("{:?}", block.scope),
                    format!("{:?}", block.sensitivity),
                    serde_json::to_string(&block.source_memory_ids)?,
                    serde_json::to_string(&block.source_version_hashes)?,
                    serde_json::to_string(&block.cache_policy)?,
                    serde_json::to_string(&block.provider_annotations)?,
                ],
            )?;
        }
        for capsule in &envelope.capsules {
            tx.execute(
                r#"
                INSERT INTO prompt_cache_capsules (
                    capsule_id, capsule_type, scope, scope_identity_json, content_hash,
                    token_estimate, source_memory_ids_json, source_version_hashes_json,
                    block_ids_json, renderer_version, created_at, last_used_at, use_count
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, 1)
                ON CONFLICT(capsule_id) DO UPDATE SET
                    last_used_at = excluded.last_used_at,
                    use_count = prompt_cache_capsules.use_count + 1
                "#,
                params![
                    capsule.capsule_id,
                    format!("{:?}", capsule.capsule_type),
                    format!("{:?}", capsule.scope),
                    serde_json::to_string(&capsule.scope_identity)?,
                    capsule.content_hash,
                    capsule.token_estimate as i64,
                    serde_json::to_string(&capsule.source_memory_ids)?,
                    serde_json::to_string(&capsule.source_version_hashes)?,
                    serde_json::to_string(&capsule.block_ids)?,
                    capsule.renderer_version,
                    Utc::now().to_rfc3339(),
                ],
            )?;
        }
        tx.execute(
            r#"
            INSERT INTO audit_events (id, memory_id, event_type, message, created_at)
            VALUES (?1, NULL, 'prompt_cache.manifest_stored', ?2, ?3)
            "#,
            params![
                format!("aud_{}", uuid::Uuid::now_v7().simple()),
                format!(
                    "stored prompt cache manifest {} for {}/{}",
                    manifest.manifest_id, manifest.provider, manifest.model
                ),
                Utc::now().to_rfc3339(),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn prompt_cache_manifests(
        &self,
        limit: usize,
    ) -> anyhow::Result<Vec<PromptCacheManifestRecord>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT manifest_id, provider, model, prompt_cache_key, cacheable_prefix_hash,
                   cacheable_prefix_tokens, total_prompt_tokens, renderer_version,
                   tokenizer_version, scope_identity_json, block_hashes_json, created_at
            FROM prompt_cache_manifests
            ORDER BY created_at DESC, manifest_id
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], row_to_prompt_cache_manifest_record)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn prompt_cache_capsule_reuse_counts(
        &self,
        capsule_ids: &[String],
    ) -> anyhow::Result<(usize, usize)> {
        let mut hits = 0usize;
        for capsule_id in capsule_ids {
            let exists = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM prompt_cache_capsules WHERE capsule_id = ?1)",
                params![capsule_id],
                |row| row.get::<_, bool>(0),
            )?;
            if exists {
                hits += 1;
            }
        }
        Ok((hits, capsule_ids.len().saturating_sub(hits)))
    }

    pub fn get_prompt_cache_manifest(
        &self,
        manifest_id: &str,
    ) -> anyhow::Result<Option<PromptCacheManifestRecord>> {
        self.conn
            .query_row(
                r#"
                SELECT manifest_id, provider, model, prompt_cache_key, cacheable_prefix_hash,
                       cacheable_prefix_tokens, total_prompt_tokens, renderer_version,
                       tokenizer_version, scope_identity_json, block_hashes_json, created_at
                FROM prompt_cache_manifests
                WHERE manifest_id = ?1
                "#,
                params![manifest_id],
                row_to_prompt_cache_manifest_record,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn record_prompt_cache_usage(&self, usage: &PromptCacheUsage) -> anyhow::Result<String> {
        let id = format!("pcu_{}", uuid::Uuid::now_v7().simple());
        self.conn.execute(
            r#"
            INSERT INTO prompt_cache_usage (
                id, manifest_id, provider, model, total_input_tokens,
                provider_cached_input_tokens, provider_cache_write_tokens,
                provider_cache_read_tokens, local_capsule_hits, local_capsule_misses,
                latency_ms, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
            params![
                id,
                usage.manifest_id,
                usage.provider,
                usage.model,
                usage.total_input_tokens as i64,
                usage.provider_cached_input_tokens as i64,
                usage.provider_cache_write_tokens as i64,
                usage.provider_cache_read_tokens as i64,
                usage.local_capsule_hits as i64,
                usage.local_capsule_misses as i64,
                usage.latency_ms as i64,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(id)
    }

    pub fn prompt_cache_usage(
        &self,
        manifest_id: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<PromptCacheUsageRecord>> {
        if let Some(manifest_id) = manifest_id {
            let mut stmt = self.conn.prepare(
                r#"
                SELECT id, manifest_id, provider, model, total_input_tokens,
                       provider_cached_input_tokens, provider_cache_write_tokens,
                       provider_cache_read_tokens, local_capsule_hits, local_capsule_misses,
                       latency_ms, created_at
                FROM prompt_cache_usage
                WHERE manifest_id = ?1
                ORDER BY created_at DESC, id
                LIMIT ?2
                "#,
            )?;
            let rows = stmt.query_map(params![manifest_id, limit as i64], |row| {
                row_to_prompt_cache_usage_record(row)
            })?;
            return rows.collect::<Result<Vec<_>, _>>().map_err(Into::into);
        }

        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, manifest_id, provider, model, total_input_tokens,
                   provider_cached_input_tokens, provider_cache_write_tokens,
                   provider_cache_read_tokens, local_capsule_hits, local_capsule_misses,
                   latency_ms, created_at
            FROM prompt_cache_usage
            ORDER BY created_at DESC, id
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], row_to_prompt_cache_usage_record)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn invalidate_prompt_cache_manifest(
        &self,
        manifest_id: &str,
        reason: &str,
        changed_source: Option<&str>,
    ) -> anyhow::Result<PromptCacheInvalidationRecord> {
        if self.get_prompt_cache_manifest(manifest_id)?.is_none() {
            anyhow::bail!("unknown prompt cache manifest: {manifest_id}");
        }
        let record = PromptCacheInvalidationRecord {
            id: format!("pci_{}", uuid::Uuid::now_v7().simple()),
            manifest_id: manifest_id.to_string(),
            reason: reason.to_string(),
            changed_source: changed_source.map(ToOwned::to_owned),
            created_at: Utc::now(),
        };
        self.conn.execute(
            r#"
            INSERT INTO prompt_cache_invalidations (
                id, manifest_id, reason, changed_source, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                record.id,
                record.manifest_id,
                record.reason,
                record.changed_source,
                record.created_at.to_rfc3339(),
            ],
        )?;
        self.log_audit_event(
            None,
            "prompt_cache.invalidated",
            &format!("invalidated prompt cache manifest {manifest_id}: {reason}"),
        )?;
        Ok(record)
    }

    pub fn invalidate_prompt_cache_by_source(
        &self,
        source_memory_id: &str,
        reason: &str,
    ) -> anyhow::Result<Vec<PromptCacheInvalidationRecord>> {
        let pattern = format!("%\"{source_memory_id}\"%");
        let mut stmt = self.conn.prepare(
            r#"
            SELECT DISTINCT manifest_id
            FROM prompt_cache_blocks
            WHERE source_memory_ids_json LIKE ?1
            ORDER BY manifest_id
            "#,
        )?;
        let manifest_ids = stmt
            .query_map(params![pattern], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        let mut invalidations = Vec::new();
        for manifest_id in manifest_ids {
            invalidations.push(self.invalidate_prompt_cache_manifest(
                &manifest_id,
                reason,
                Some(source_memory_id),
            )?);
        }
        let evicted_capsules = self.conn.execute(
            "DELETE FROM prompt_cache_capsules WHERE source_memory_ids_json LIKE ?1",
            params![pattern],
        )?;
        if evicted_capsules > 0 {
            self.log_audit_event(
                None,
                "prompt_cache.capsules_evicted",
                &format!(
                    "evicted {evicted_capsules} prompt cache capsule(s) for source {source_memory_id}: {reason}"
                ),
            )?;
        }
        Ok(invalidations)
    }

    pub fn invalidate_prompt_cache_by_scope_filter(
        &self,
        user_id: Option<&str>,
        project_id: Option<&str>,
        session_id: Option<&str>,
        shared_scope_id: Option<&str>,
        reason: &str,
    ) -> anyhow::Result<PromptCacheScopeInvalidationReport> {
        if user_id.is_none()
            && project_id.is_none()
            && session_id.is_none()
            && shared_scope_id.is_none()
        {
            anyhow::bail!("at least one scope filter is required");
        }

        let mut stmt = self.conn.prepare(
            r#"
            SELECT manifest_id, scope_identity_json
            FROM prompt_cache_manifests
            ORDER BY manifest_id
            "#,
        )?;
        let manifest_rows = stmt
            .query_map([], |row| {
                let scope_identity_json: String = row.get(1)?;
                let scope_identity = serde_json::from_str::<CacheScopeIdentity>(
                    &scope_identity_json,
                )
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
                Ok((row.get::<_, String>(0)?, scope_identity))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        let manifest_ids = manifest_rows
            .into_iter()
            .filter(|(_, scope_identity)| {
                scope_identity_matches_filter(
                    scope_identity,
                    user_id,
                    project_id,
                    session_id,
                    shared_scope_id,
                )
            })
            .map(|(manifest_id, _)| manifest_id)
            .collect::<Vec<_>>();

        let mut capsule_stmt = self.conn.prepare(
            r#"
            SELECT capsule_id, scope_identity_json
            FROM prompt_cache_capsules
            ORDER BY capsule_id
            "#,
        )?;
        let capsule_rows = capsule_stmt
            .query_map([], |row| {
                let scope_identity_json: String = row.get(1)?;
                let scope_identity = serde_json::from_str::<CacheScopeIdentity>(
                    &scope_identity_json,
                )
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
                Ok((row.get::<_, String>(0)?, scope_identity))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(capsule_stmt);

        let capsule_ids = capsule_rows
            .into_iter()
            .filter(|(_, scope_identity)| {
                scope_identity_matches_filter(
                    scope_identity,
                    user_id,
                    project_id,
                    session_id,
                    shared_scope_id,
                )
            })
            .map(|(capsule_id, _)| capsule_id)
            .collect::<Vec<_>>();

        let mut invalidations = Vec::new();
        for manifest_id in manifest_ids {
            invalidations.push(self.invalidate_prompt_cache_manifest(
                &manifest_id,
                reason,
                None,
            )?);
        }
        for capsule_id in &capsule_ids {
            self.conn.execute(
                "DELETE FROM prompt_cache_capsules WHERE capsule_id = ?1",
                params![capsule_id],
            )?;
        }
        if !capsule_ids.is_empty() {
            self.log_audit_event(
                None,
                "prompt_cache.scope_capsules_evicted",
                &format!(
                    "evicted {} prompt cache capsule(s) for scope filter: {reason}",
                    capsule_ids.len()
                ),
            )?;
        }

        Ok(PromptCacheScopeInvalidationReport {
            invalidations,
            evicted_capsules: capsule_ids.len(),
        })
    }

    pub fn prompt_cache_invalidations(
        &self,
        manifest_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<PromptCacheInvalidationRecord>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, manifest_id, reason, changed_source, created_at
            FROM prompt_cache_invalidations
            WHERE manifest_id = ?1
            ORDER BY created_at DESC, id
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![manifest_id, limit as i64], |row| {
            let created_at: String = row.get(4)?;
            Ok(PromptCacheInvalidationRecord {
                id: row.get(0)?,
                manifest_id: row.get(1)?,
                reason: row.get(2)?,
                changed_source: row.get(3)?,
                created_at: parse_rfc3339_for_sql(4, &created_at)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn prompt_cache_capsules(&self, limit: usize) -> anyhow::Result<Vec<PromptCapsuleRecord>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT capsule_id, capsule_type, scope, scope_identity_json, content_hash,
                   token_estimate, source_memory_ids_json, source_version_hashes_json,
                   block_ids_json, renderer_version, created_at, last_used_at, use_count
            FROM prompt_cache_capsules
            ORDER BY last_used_at DESC, capsule_id
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], row_to_prompt_capsule_record)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn prompt_cache_stats(&self) -> anyhow::Result<PromptCacheStats> {
        Ok(PromptCacheStats {
            manifests: self.count_table("prompt_cache_manifests")?,
            blocks: self.count_table("prompt_cache_blocks")?,
            capsules: self.count_table("prompt_cache_capsules")?,
            usage_records: self.count_table("prompt_cache_usage")?,
            invalidations: self.count_table("prompt_cache_invalidations")?,
        })
    }

    pub fn log_audit_event(
        &self,
        memory_id: Option<&str>,
        event_type: &str,
        message: &str,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            r#"
            INSERT INTO audit_events (id, memory_id, event_type, message, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                format!("aud_{}", uuid::Uuid::now_v7().simple()),
                memory_id,
                event_type,
                message,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn clear_derived_indexes(&self) -> anyhow::Result<()> {
        self.conn.execute("DELETE FROM graph_edges", [])?;
        self.conn.execute(
            "DELETE FROM graph_nodes WHERE id NOT IN (SELECT id FROM memories WHERE status = 'active')",
            [],
        )?;
        self.conn.execute("DELETE FROM vector_terms", [])?;
        self.conn.execute("DELETE FROM vector_chunks", [])?;
        self.conn.execute(
            "DELETE FROM index_state WHERE index_name IN ('sqlite-graph', 'sqlite-vector')",
            [],
        )?;
        Ok(())
    }

    fn claims_for(&self, memory_id: &str) -> anyhow::Result<Vec<Claim>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, text, confidence, source FROM claims WHERE memory_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![memory_id], |row| {
            Ok(Claim {
                id: row.get(0)?,
                text: row.get(1)?,
                confidence: row.get(2)?,
                source: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn links_for(&self, memory_id: &str) -> anyhow::Result<Vec<MemoryLink>> {
        let mut stmt = self.conn.prepare(
            "SELECT source_id, target_id, relation, confidence FROM memory_links WHERE source_id = ?1 ORDER BY relation, target_id",
        )?;
        let rows = stmt.query_map(params![memory_id], |row| {
            Ok(MemoryLink {
                source_id: row.get(0)?,
                target_id: row.get(1)?,
                relation: row.get(2)?,
                confidence: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn tags_for(&self, memory_id: &str) -> anyhow::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag FROM memory_tags WHERE memory_id = ?1 ORDER BY tag")?;
        let rows = stmt.query_map(params![memory_id], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn metadata_for(
        &self,
        memory_id: &str,
    ) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM memory_metadata WHERE memory_id = ?1 ORDER BY key")?;
        let rows = stmt.query_map(params![memory_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<_, _>>().map_err(Into::into)
    }

    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    fn count_table(&self, table: &str) -> anyhow::Result<i64> {
        self.conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .map_err(Into::into)
    }

    fn count_where(&self, table: &str, where_clause: &str) -> anyhow::Result<i64> {
        self.conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE {where_clause}"),
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    fn ensure_column(&self, table: &str, column: &str, column_type: &str) -> anyhow::Result<()> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for existing in rows {
            if existing? == column {
                return Ok(());
            }
        }

        self.conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {column_type}"),
            [],
        )?;
        Ok(())
    }

    fn ensure_schema_migration_table(&self) -> anyhow::Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                checksum TEXT NOT NULL,
                applied_at TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    fn has_migration(&self, version: i64) -> anyhow::Result<bool> {
        let exists = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            params![version],
            |row| row.get::<_, bool>(0),
        )?;
        Ok(exists)
    }

    fn record_migration(&self, version: i64, name: &str, sql: &str) -> anyhow::Result<()> {
        self.conn.execute(
            r#"
            INSERT OR IGNORE INTO schema_migrations (version, name, checksum, applied_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            params![
                version,
                name,
                migration_checksum(version, name, sql),
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    fn search_results_from_stmt<P>(
        &self,
        stmt: &mut rusqlite::Statement<'_>,
        params: P,
        reason: &str,
        score: f64,
    ) -> anyhow::Result<Vec<MemorySearchResult>>
    where
        P: rusqlite::Params,
    {
        let rows = stmt.query_map(params, row_to_memory)?;
        let mut results = Vec::new();
        for row in rows {
            let mut memory = row?;
            memory.claims = self.claims_for(&memory.id)?;
            memory.links = self.links_for(&memory.id)?;
            memory.tags = self.tags_for(&memory.id)?;
            memory.metadata = self.metadata_for(&memory.id)?;
            results.push(MemorySearchResult {
                memory,
                score,
                reasons: vec![reason.to_string()],
            });
        }
        Ok(results)
    }

    fn memory_scope_query<P>(
        &self,
        where_clause: &str,
        params: P,
    ) -> anyhow::Result<Vec<MemoryScopeEntry>>
    where
        P: rusqlite::Params,
    {
        let sql = format!(
            r#"
            SELECT memories.id, memories.type, memories.title, memories.status,
                   COALESCE(visibility.value, 'public') AS visibility,
                   user_scope.value AS user_id,
                   project_scope.value AS project_id,
                   memories.updated_at
            FROM memories
            LEFT JOIN memory_metadata visibility
              ON visibility.memory_id = memories.id AND visibility.key = 'visibility'
            LEFT JOIN memory_metadata user_scope
              ON user_scope.memory_id = memories.id AND user_scope.key = 'user_id'
            LEFT JOIN memory_metadata project_scope
              ON project_scope.memory_id = memories.id AND project_scope.key = 'project_id'
            {where_clause}
            "#
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params, |row| {
            let updated_at: String = row.get(7)?;
            Ok(MemoryScopeEntry {
                id: row.get(0)?,
                memory_type: row.get(1)?,
                title: row.get(2)?,
                status: row.get(3)?,
                visibility: row.get(4)?,
                user_id: row.get(5)?,
                project_id: row.get(6)?,
                updated_at: parse_rfc3339_for_sql(7, &updated_at)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryStatusFilter {
    Active,
    Deleted,
    Archived,
    Quarantined,
    Superseded,
    Inactive,
    All,
}

impl MemoryStatusFilter {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "active" => Ok(Self::Active),
            "deleted" => Ok(Self::Deleted),
            "archived" => Ok(Self::Archived),
            "quarantined" => Ok(Self::Quarantined),
            "superseded" => Ok(Self::Superseded),
            "inactive" => Ok(Self::Inactive),
            "all" => Ok(Self::All),
            _ => anyhow::bail!("unknown memory status filter: {value}"),
        }
    }
}

fn query_pattern(query: &str) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        "%%".to_string()
    } else {
        format!("%{trimmed}%")
    }
}

fn row_to_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryObject> {
    let source_kind: Option<String> = row.get(5)?;
    let source_reference: Option<String> = row.get(6)?;
    let created_at: String = row.get(8)?;
    let updated_at: String = row.get(9)?;
    Ok(MemoryObject {
        id: row.get(0)?,
        memory_type: row.get(1)?,
        title: row.get(2)?,
        summary: row.get(3)?,
        body: row.get(4)?,
        claims: Vec::new(),
        links: Vec::new(),
        metadata: Default::default(),
        confidence: row.get(7)?,
        created_at: parse_rfc3339(&created_at).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(err))
        })?,
        updated_at: parse_rfc3339(&updated_at).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(err))
        })?,
        source: source_kind
            .zip(source_reference)
            .map(|(kind, reference)| MemorySource { kind, reference }),
        tags: Vec::new(),
    })
}

fn row_to_prompt_cache_manifest_record(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<PromptCacheManifestRecord> {
    let scope_identity_json: String = row.get(9)?;
    let block_hashes_json: String = row.get(10)?;
    let created_at: String = row.get(11)?;
    Ok(PromptCacheManifestRecord {
        manifest: PromptCacheManifest {
            manifest_id: row.get(0)?,
            provider: row.get(1)?,
            model: row.get(2)?,
            prompt_cache_key: row.get(3)?,
            cacheable_prefix_hash: row.get(4)?,
            cacheable_prefix_tokens: row.get::<_, i64>(5)? as usize,
            total_prompt_tokens: row.get::<_, i64>(6)? as usize,
            renderer_version: row.get(7)?,
            tokenizer_version: row.get(8)?,
            scope_identity: serde_json::from_str(&scope_identity_json).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    9,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?,
            block_hashes: serde_json::from_str(&block_hashes_json).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    10,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?,
        },
        created_at: parse_rfc3339_for_sql(11, &created_at)?,
    })
}

fn row_to_prompt_cache_usage_record(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<PromptCacheUsageRecord> {
    let created_at: String = row.get(11)?;
    Ok(PromptCacheUsageRecord {
        id: row.get(0)?,
        usage: PromptCacheUsage {
            manifest_id: row.get(1)?,
            provider: row.get(2)?,
            model: row.get(3)?,
            total_input_tokens: row.get::<_, i64>(4)? as usize,
            provider_cached_input_tokens: row.get::<_, i64>(5)? as usize,
            provider_cache_write_tokens: row.get::<_, i64>(6)? as usize,
            provider_cache_read_tokens: row.get::<_, i64>(7)? as usize,
            local_capsule_hits: row.get::<_, i64>(8)? as usize,
            local_capsule_misses: row.get::<_, i64>(9)? as usize,
            latency_ms: row.get::<_, i64>(10)? as usize,
        },
        created_at: parse_rfc3339_for_sql(11, &created_at)?,
    })
}

fn row_to_prompt_capsule_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<PromptCapsuleRecord> {
    let capsule_type: String = row.get(1)?;
    let scope: String = row.get(2)?;
    let scope_identity_json: String = row.get(3)?;
    let source_memory_ids_json: String = row.get(6)?;
    let source_version_hashes_json: String = row.get(7)?;
    let block_ids_json: String = row.get(8)?;
    let created_at: String = row.get(10)?;
    let last_used_at: String = row.get(11)?;
    Ok(PromptCapsuleRecord {
        capsule: PromptCapsule {
            capsule_id: row.get(0)?,
            capsule_type: parse_capsule_type_for_sql(1, &capsule_type)?,
            scope: parse_cache_scope_for_sql(2, &scope)?,
            scope_identity: serde_json::from_str::<CacheScopeIdentity>(&scope_identity_json)
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
            content_hash: row.get(4)?,
            token_estimate: row.get::<_, i64>(5)? as usize,
            source_memory_ids: serde_json::from_str(&source_memory_ids_json).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    6,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?,
            source_version_hashes: serde_json::from_str(&source_version_hashes_json).map_err(
                |err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        7,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                },
            )?,
            block_ids: serde_json::from_str(&block_ids_json).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    8,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?,
            renderer_version: row.get(9)?,
        },
        created_at: parse_rfc3339_for_sql(10, &created_at)?,
        last_used_at: parse_rfc3339_for_sql(11, &last_used_at)?,
        use_count: row.get(12)?,
    })
}

fn scope_identity_matches_filter(
    scope_identity: &CacheScopeIdentity,
    user_id: Option<&str>,
    project_id: Option<&str>,
    session_id: Option<&str>,
    shared_scope_id: Option<&str>,
) -> bool {
    user_id.is_none_or(|value| scope_identity.user_id.as_deref() == Some(value))
        && project_id.is_none_or(|value| scope_identity.project_id.as_deref() == Some(value))
        && session_id.is_none_or(|value| scope_identity.session_id.as_deref() == Some(value))
        && shared_scope_id
            .is_none_or(|value| scope_identity.shared_scope_id.as_deref() == Some(value))
}

fn parse_capsule_type_for_sql(column: usize, value: &str) -> rusqlite::Result<PromptCapsuleType> {
    match value {
        "ToolDefinitions" => Ok(PromptCapsuleType::ToolDefinitions),
        "SystemKernel" => Ok(PromptCapsuleType::SystemKernel),
        "UserScope" => Ok(PromptCapsuleType::UserScope),
        "Project" => Ok(PromptCapsuleType::Project),
        "StableMemory" => Ok(PromptCapsuleType::StableMemory),
        "Session" => Ok(PromptCapsuleType::Session),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            format!("unknown prompt capsule type: {value}").into(),
        )),
    }
}

fn parse_cache_scope_for_sql(column: usize, value: &str) -> rusqlite::Result<CacheScope> {
    match value {
        "Global" => Ok(CacheScope::Global),
        "User" => Ok(CacheScope::User),
        "Project" => Ok(CacheScope::Project),
        "Session" => Ok(CacheScope::Session),
        "Turn" => Ok(CacheScope::Turn),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            format!("unknown cache scope: {value}").into(),
        )),
    }
}

fn parse_rfc3339(value: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc))
}

fn parse_rfc3339_for_sql(column: usize, value: &str) -> rusqlite::Result<DateTime<Utc>> {
    parse_rfc3339(value).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })
}

fn migration_checksum(version: i64, name: &str, sql: &str) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, "version", &version.to_string());
    hash_field(&mut hasher, "name", name);
    hash_field(&mut hasher, "sql", sql);
    format!("{:x}", hasher.finalize())
}

pub fn content_hash(memory: &MemoryObject) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, "id", &memory.id);
    hash_field(&mut hasher, "type", &memory.memory_type);
    hash_field(&mut hasher, "title", &memory.title);
    hash_field(&mut hasher, "summary", &memory.summary);
    hash_field(&mut hasher, "body", &memory.body);
    hash_field(&mut hasher, "confidence", &memory.confidence.to_string());

    if let Some(source) = &memory.source {
        hash_field(&mut hasher, "source.kind", &source.kind);
        hash_field(&mut hasher, "source.reference", &source.reference);
    }

    let mut claims = memory.claims.clone();
    claims.sort_by(|left, right| {
        (&left.id, &left.text, &left.source).cmp(&(&right.id, &right.text, &right.source))
    });
    for claim in claims {
        hash_field(&mut hasher, "claim.id", &claim.id);
        hash_field(&mut hasher, "claim.text", &claim.text);
        hash_field(
            &mut hasher,
            "claim.confidence",
            &claim.confidence.to_string(),
        );
        if let Some(source) = claim.source {
            hash_field(&mut hasher, "claim.source", &source);
        }
    }

    let mut links = memory.links.clone();
    links.sort_by(|left, right| {
        (
            &left.source_id,
            &left.target_id,
            &left.relation,
            left.confidence.to_bits(),
        )
            .cmp(&(
                &right.source_id,
                &right.target_id,
                &right.relation,
                right.confidence.to_bits(),
            ))
    });
    for link in links {
        hash_field(&mut hasher, "link.source", &link.source_id);
        hash_field(&mut hasher, "link.target", &link.target_id);
        hash_field(&mut hasher, "link.relation", &link.relation);
        hash_field(&mut hasher, "link.confidence", &link.confidence.to_string());
    }

    let mut tags = memory.tags.clone();
    tags.sort();
    for tag in tags {
        hash_field(&mut hasher, "tag", &tag);
    }

    for (key, value) in &memory.metadata {
        hash_field(&mut hasher, "metadata.key", key);
        hash_field(&mut hasher, "metadata.value", value);
    }
    format!("{:x}", hasher.finalize())
}

fn hash_field(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update(name.as_bytes());
    hasher.update([0]);
    hasher.update(value.len().to_le_bytes());
    hasher.update(value.as_bytes());
    hasher.update([0xff]);
}
