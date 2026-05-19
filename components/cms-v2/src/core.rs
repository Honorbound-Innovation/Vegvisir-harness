use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryObject {
    pub id: String,
    pub memory_type: String,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub claims: Vec<Claim>,
    pub links: Vec<MemoryLink>,
    pub metadata: BTreeMap<String, String>,
    pub confidence: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub source: Option<MemorySource>,
    pub tags: Vec<String>,
}

impl MemoryObject {
    pub fn new(memory_type: impl Into<String>, title: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: format!("mem_{}", Uuid::now_v7().simple()),
            memory_type: memory_type.into(),
            title: title.into(),
            summary: String::new(),
            body: String::new(),
            claims: Vec::new(),
            links: Vec::new(),
            metadata: BTreeMap::new(),
            confidence: 1.0,
            created_at: now,
            updated_at: now,
            source: None,
            tags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub text: String,
    pub confidence: f64,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryLink {
    pub source_id: String,
    pub target_id: String,
    pub relation: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemorySource {
    pub kind: String,
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryVersion {
    pub memory_id: String,
    pub version: i64,
    pub content_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryChunk {
    pub id: String,
    pub memory_id: String,
    pub text: String,
    pub kind: String,
    pub ordinal: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemorySearchResult {
    pub memory: MemoryObject,
    pub score: f64,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalBundle {
    pub query: String,
    pub results: Vec<MemorySearchResult>,
    pub context: String,
    pub trace: BTreeMap<String, Value>,
}
