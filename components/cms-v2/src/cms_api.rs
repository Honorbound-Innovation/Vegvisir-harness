use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use thiserror::Error;

pub type Metadata = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryId(pub String);

impl MemoryId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl From<&str> for MemoryId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for MemoryId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for MemoryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectId(pub String);

impl ProjectId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl From<&str> for ProjectId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ProjectId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub text: String,
    pub confidence: f32,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryLink {
    pub source_id: MemoryId,
    pub target_id: String,
    pub relation: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryObject {
    pub id: MemoryId,
    pub memory_type: String,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub claims: Vec<Claim>,
    pub links: Vec<MemoryLink>,
    pub tags: Vec<String>,
    pub confidence: f32,
    pub source: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: Metadata,
}

impl MemoryObject {
    pub fn new(
        id: impl Into<MemoryId>,
        memory_type: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        let title = title.into();
        let body = body.into();
        Self {
            id: id.into(),
            memory_type: memory_type.into(),
            summary: title.clone(),
            title,
            body,
            claims: Vec::new(),
            links: Vec::new(),
            tags: Vec::new(),
            confidence: 1.0,
            source: None,
            created_at: now,
            updated_at: now,
            metadata: Metadata::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RetrievalMode {
    Exact,
    Semantic,
    Graph,
    Hybrid,
    Recent,
    Project,
    DecisionHistory,
    Contradiction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalRequest {
    pub query: String,
    pub project_id: Option<ProjectId>,
    pub modes: Vec<RetrievalMode>,
    pub memory_types: Vec<String>,
    pub limit: usize,
    pub graph_depth: usize,
    pub include_contradictions: bool,
    pub filters: Metadata,
}

impl RetrievalRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            project_id: None,
            modes: vec![RetrievalMode::Hybrid],
            memory_types: Vec::new(),
            limit: 10,
            graph_depth: 1,
            include_contradictions: false,
            filters: Metadata::new(),
        }
    }

    pub fn with_project(mut self, project_id: impl Into<ProjectId>) -> Self {
        self.project_id = Some(project_id.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRetrievalResult {
    pub memory: MemoryObject,
    pub score: f32,
    pub source_mode: RetrievalMode,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConflict {
    pub left_memory_id: MemoryId,
    pub right_memory_id: MemoryId,
    pub description: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalBundle {
    pub query: String,
    pub results: Vec<MemoryRetrievalResult>,
    pub contradictions: Vec<MemoryConflict>,
    pub trace: Metadata,
}

impl RetrievalBundle {
    pub fn empty(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            results: Vec::new(),
            contradictions: Vec::new(),
            trace: Metadata::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitRequest {
    pub memory: MemoryObject,
    pub reason: String,
    pub deduplicate: bool,
    pub update_existing: bool,
}

impl CommitRequest {
    pub fn new(memory: MemoryObject, reason: impl Into<String>) -> Self {
        Self {
            memory,
            reason: reason.into(),
            deduplicate: true,
            update_existing: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitResult {
    pub memory_id: MemoryId,
    pub created_new: bool,
    pub updated_existing: bool,
    pub linked_memory_ids: Vec<MemoryId>,
    pub trace: Metadata,
}

#[derive(Debug, Error)]
pub enum CmsApiError {
    #[error("invalid retrieval request: {0}")]
    InvalidRetrievalRequest(String),
    #[error("invalid commit request: {0}")]
    InvalidCommitRequest(String),
    #[error("backend error: {0}")]
    Backend(String),
}

pub type CmsApiResult<T> = Result<T, CmsApiError>;

pub trait CmsMemoryClient {
    fn retrieve(&self, request: RetrievalRequest) -> CmsApiResult<RetrievalBundle>;
    fn commit_memory(&mut self, request: CommitRequest) -> CmsApiResult<CommitResult>;
}
