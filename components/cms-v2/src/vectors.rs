use crate::core::{MemoryChunk, MemoryObject};
use crate::sqlite::{SqliteLedger, content_hash};
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const LEXICAL_VECTOR_PROVIDER: &str = "cms-lexical-v1";
pub const VECTOR_CHUNKING_VERSION: &str = "default-chunks-v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorHit {
    pub memory_id: String,
    pub chunk_id: String,
    pub score: f64,
}

pub trait EmbeddingService {
    fn provider_id(&self) -> String {
        LEXICAL_VECTOR_PROVIDER.to_string()
    }

    fn chunking_version(&self) -> String {
        VECTOR_CHUNKING_VERSION.to_string()
    }

    fn embed_text(&self, text: &str) -> anyhow::Result<Vec<f32>>;
}

pub struct DeterministicLexicalEmbedding;

impl EmbeddingService for DeterministicLexicalEmbedding {
    fn embed_text(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let weights = term_weights(text);
        Ok(weights.values().map(|value| *value as f32).collect())
    }
}

pub trait VectorIndex {
    fn upsert_memory(&self, memory: &MemoryObject) -> anyhow::Result<()>;
    fn delete_memory(&self, memory_id: &str) -> anyhow::Result<()>;
    fn semantic_search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<VectorHit>>;
}

pub struct SqliteVectorIndex<'a> {
    ledger: &'a SqliteLedger,
    provider_id: String,
    chunking_version: String,
}

impl<'a> SqliteVectorIndex<'a> {
    pub fn new(ledger: &'a SqliteLedger) -> Self {
        Self {
            ledger,
            provider_id: LEXICAL_VECTOR_PROVIDER.to_string(),
            chunking_version: VECTOR_CHUNKING_VERSION.to_string(),
        }
    }

    pub fn with_provider(
        ledger: &'a SqliteLedger,
        provider_id: impl Into<String>,
        chunking_version: impl Into<String>,
    ) -> Self {
        Self {
            ledger,
            provider_id: provider_id.into(),
            chunking_version: chunking_version.into(),
        }
    }

    pub fn with_embedding_service(
        ledger: &'a SqliteLedger,
        embedding: &dyn EmbeddingService,
    ) -> Self {
        Self::with_provider(
            ledger,
            embedding.provider_id(),
            embedding.chunking_version(),
        )
    }
}

impl VectorIndex for SqliteVectorIndex<'_> {
    fn upsert_memory(&self, memory: &MemoryObject) -> anyhow::Result<()> {
        self.delete_memory(&memory.id)?;
        for chunk in default_chunks(memory) {
            self.ledger.connection().execute(
                r#"
                INSERT INTO vector_chunks (chunk_id, memory_id, kind, ordinal, text)
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
                params![
                    chunk.id,
                    chunk.memory_id,
                    chunk.kind,
                    chunk.ordinal as i64,
                    chunk.text
                ],
            )?;

            for (term, weight) in term_weights(&chunk.text) {
                self.ledger.connection().execute(
                    r#"
                    INSERT INTO vector_terms (chunk_id, memory_id, term, weight)
                    VALUES (?1, ?2, ?3, ?4)
                    "#,
                    params![chunk.id, chunk.memory_id, term, weight],
                )?;
            }
        }

        self.ledger.connection().execute(
            r#"
            INSERT INTO index_state (memory_id, index_name, content_hash, indexed_at, status)
            VALUES (?1, 'sqlite-vector', ?2, ?3, 'indexed')
            ON CONFLICT(memory_id, index_name) DO UPDATE SET
                content_hash = excluded.content_hash,
                indexed_at = excluded.indexed_at,
                status = excluded.status
            "#,
            params![
                memory.id,
                vector_index_hash_with_config(memory, &self.provider_id, &self.chunking_version),
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    fn delete_memory(&self, memory_id: &str) -> anyhow::Result<()> {
        self.ledger.connection().execute(
            "DELETE FROM vector_terms WHERE memory_id = ?1",
            params![memory_id],
        )?;
        self.ledger.connection().execute(
            "DELETE FROM vector_chunks WHERE memory_id = ?1",
            params![memory_id],
        )?;
        Ok(())
    }

    fn semantic_search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<VectorHit>> {
        let query_terms = term_weights(query);
        if query_terms.is_empty() {
            return Ok(Vec::new());
        }

        let mut scores: BTreeMap<(String, String), f64> = BTreeMap::new();
        let mut stmt = self.ledger.connection().prepare(
            r#"
            SELECT vector_terms.chunk_id, vector_terms.memory_id, vector_terms.weight
            FROM vector_terms
            JOIN memories ON memories.id = vector_terms.memory_id
            WHERE vector_terms.term = ?1
              AND memories.status = 'active'
            "#,
        )?;

        for (term, query_weight) in query_terms {
            let rows = stmt.query_map(params![term], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })?;
            for row in rows {
                let (chunk_id, memory_id, weight) = row?;
                *scores.entry((memory_id, chunk_id)).or_default() += query_weight * weight;
            }
        }

        let mut hits = scores
            .into_iter()
            .map(|((memory_id, chunk_id), score)| VectorHit {
                memory_id,
                chunk_id,
                score,
            })
            .collect::<Vec<_>>();
        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.memory_id.cmp(&right.memory_id))
                .then_with(|| left.chunk_id.cmp(&right.chunk_id))
        });
        hits.truncate(limit);
        Ok(hits)
    }
}

pub fn default_chunks(memory: &MemoryObject) -> Vec<MemoryChunk> {
    let mut chunks = Vec::new();
    if !memory.summary.trim().is_empty() {
        chunks.push(MemoryChunk {
            id: format!("{}_summary", memory.id),
            memory_id: memory.id.clone(),
            text: memory.summary.clone(),
            kind: "summary".to_string(),
            ordinal: chunks.len(),
        });
    }
    for claim in &memory.claims {
        chunks.push(MemoryChunk {
            id: format!("{}_claim_{}", memory.id, claim.id),
            memory_id: memory.id.clone(),
            text: claim.text.clone(),
            kind: "claim".to_string(),
            ordinal: chunks.len(),
        });
    }
    if !memory.body.trim().is_empty() {
        chunks.push(MemoryChunk {
            id: format!("{}_body", memory.id),
            memory_id: memory.id.clone(),
            text: memory.body.clone(),
            kind: "body".to_string(),
            ordinal: chunks.len(),
        });
    }
    chunks
}

pub fn vector_index_hash(memory: &MemoryObject) -> String {
    vector_index_hash_with_config(memory, LEXICAL_VECTOR_PROVIDER, VECTOR_CHUNKING_VERSION)
}

pub fn vector_index_hash_with_config(
    memory: &MemoryObject,
    provider_id: &str,
    chunking_version: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content_hash(memory).as_bytes());
    hasher.update(b"\nprovider=");
    hasher.update(provider_id.as_bytes());
    hasher.update(b"\nchunking=");
    hasher.update(chunking_version.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn term_weights(text: &str) -> BTreeMap<String, f64> {
    let mut counts: BTreeMap<String, f64> = BTreeMap::new();
    for term in tokenize(text) {
        if is_stop_word(&term) {
            continue;
        }
        *counts.entry(term).or_default() += 1.0;
    }

    let norm = counts
        .values()
        .map(|weight| weight * weight)
        .sum::<f64>()
        .sqrt();
    if norm == 0.0 {
        return BTreeMap::new();
    }
    for weight in counts.values_mut() {
        *weight /= norm;
    }
    counts
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter_map(|token| {
            let token = token.trim().to_ascii_lowercase();
            (token.len() >= 3).then_some(token)
        })
        .collect()
}

fn is_stop_word(term: &str) -> bool {
    matches!(
        term,
        "and"
            | "are"
            | "for"
            | "from"
            | "has"
            | "into"
            | "not"
            | "the"
            | "that"
            | "this"
            | "with"
            | "use"
            | "uses"
            | "should"
    )
}
