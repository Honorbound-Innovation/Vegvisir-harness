use crate::cms_api::{
    Claim, CmsApiError, CmsApiResult, CmsMemoryClient, CommitRequest, CommitResult, MemoryId,
    MemoryLink, MemoryObject, MemoryRetrievalResult, Metadata, RetrievalBundle, RetrievalMode,
    RetrievalRequest,
};
use crate::core::{self as cms_core, MemorySource};
use crate::graph::{GraphIndex, SqliteGraphIndex};
use crate::rag::{HybridRagOrchestrator, RagOrchestrator};
use crate::safety::contains_sensitive_content;
use crate::sqlite::SqliteLedger;
use crate::usrl::{ScopeResolution, memory_visible_to_scope};
use crate::vectors::{SqliteVectorIndex, VectorIndex};
use serde_json::Value;
use std::collections::BTreeMap;

pub struct LocalCmsMemoryClient<'a> {
    ledger: &'a mut SqliteLedger,
}

impl<'a> LocalCmsMemoryClient<'a> {
    pub fn new(ledger: &'a mut SqliteLedger) -> Self {
        Self { ledger }
    }
}

impl CmsMemoryClient for LocalCmsMemoryClient<'_> {
    fn retrieve(&self, request: RetrievalRequest) -> CmsApiResult<RetrievalBundle> {
        if request.limit == 0 {
            return Err(CmsApiError::InvalidRetrievalRequest(
                "limit must be greater than zero".to_string(),
            ));
        }

        let modes = if request.modes.is_empty() {
            vec![RetrievalMode::Hybrid]
        } else {
            request.modes.clone()
        };
        let orchestrator = HybridRagOrchestrator::new(self.ledger);
        let mut merged = BTreeMap::<String, MemoryRetrievalResult>::new();
        let mut trace = Metadata::new();
        trace.insert(
            "retrieval_trace_id".to_string(),
            Value::String(format!("trace_retrieval_{}", uuid::Uuid::now_v7().simple())),
        );
        trace.insert(
            "requested_modes".to_string(),
            serde_json::to_value(&modes).map_err(|err| CmsApiError::Backend(err.to_string()))?,
        );
        if let Some(correlation_id) = string_filter(&request.filters, "correlation_id") {
            trace.insert(
                "correlation_id".to_string(),
                Value::String(correlation_id.to_string()),
            );
        }

        for mode in modes {
            let bundle = orchestrator
                .retrieve(&request.query, mode.into(), request.limit)
                .map_err(|err| CmsApiError::Backend(err.to_string()))?;
            for result in bundle.results {
                if !request.memory_types.is_empty()
                    && !request
                        .memory_types
                        .iter()
                        .any(|memory_type| memory_type == &result.memory.memory_type)
                {
                    continue;
                }
                let memory_id = result.memory.id.clone();
                let reason = result.reasons.join("; ");
                let converted = MemoryRetrievalResult {
                    memory: convert_memory(result.memory),
                    score: result.score as f32,
                    source_mode: mode,
                    reason,
                };
                if !memory_visible_to_request(&converted.memory, &request) {
                    continue;
                }
                merged
                    .entry(memory_id)
                    .and_modify(|existing| {
                        if converted.score > existing.score {
                            *existing = converted.clone();
                        }
                    })
                    .or_insert(converted);
            }
        }

        let mut results = merged.into_values().collect::<Vec<_>>();
        results.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.memory.id.0.cmp(&right.memory.id.0))
        });
        results.truncate(request.limit);
        trace.insert(
            "scope_filtered_result_count".to_string(),
            Value::from(results.len() as u64),
        );
        if let Some(user_id) = string_filter(&request.filters, "user_id") {
            trace.insert(
                "scope_user_id".to_string(),
                Value::String(user_id.to_string()),
            );
        }
        if let Some(project_id) = request
            .project_id
            .as_ref()
            .map(|project_id| project_id.0.as_str())
            .or_else(|| string_filter(&request.filters, "project_id"))
        {
            trace.insert(
                "scope_project_id".to_string(),
                Value::String(project_id.to_string()),
            );
        }

        Ok(RetrievalBundle {
            query: request.query,
            results,
            contradictions: Vec::new(),
            trace,
        })
    }

    fn commit_memory(&mut self, request: CommitRequest) -> CmsApiResult<CommitResult> {
        if request.memory.title.trim().is_empty() {
            return Err(CmsApiError::InvalidCommitRequest(
                "memory title must not be empty".to_string(),
            ));
        }
        if memory_contains_sensitive_content(&request.memory) {
            return Err(CmsApiError::InvalidCommitRequest(
                "memory appears to contain sensitive secret-like content".to_string(),
            ));
        }
        let memory_id = request.memory.id.clone();
        let existed = self
            .ledger
            .memory_hash(&memory_id.0)
            .map_err(|err| CmsApiError::Backend(err.to_string()))?
            .is_some();
        let core_memory = convert_api_memory(request.memory);
        self.ledger
            .upsert_memory(&core_memory, None)
            .map_err(|err| CmsApiError::Backend(err.to_string()))?;
        SqliteGraphIndex::new(self.ledger)
            .upsert_memory(&core_memory)
            .map_err(|err| CmsApiError::Backend(err.to_string()))?;
        SqliteVectorIndex::new(self.ledger)
            .upsert_memory(&core_memory)
            .map_err(|err| CmsApiError::Backend(err.to_string()))?;

        let mut trace = Metadata::new();
        trace.insert("reason".to_string(), Value::String(request.reason));
        trace.insert("deduplicate".to_string(), Value::Bool(request.deduplicate));
        trace.insert(
            "update_existing".to_string(),
            Value::Bool(request.update_existing),
        );
        Ok(CommitResult {
            memory_id,
            created_new: !existed,
            updated_existing: existed,
            linked_memory_ids: core_memory
                .links
                .iter()
                .filter_map(|link| {
                    link.target_id
                        .strip_prefix("mem_")
                        .map(|_| MemoryId(link.target_id.clone()))
                })
                .collect(),
            trace,
        })
    }
}

fn memory_contains_sensitive_content(memory: &MemoryObject) -> bool {
    contains_sensitive_content(&memory.title)
        || contains_sensitive_content(&memory.summary)
        || contains_sensitive_content(&memory.body)
        || memory
            .claims
            .iter()
            .any(|claim| contains_sensitive_content(&claim.text))
}

fn memory_visible_to_request(memory: &MemoryObject, request: &RetrievalRequest) -> bool {
    let scope = ScopeResolution::from_retrieval_request(request);
    memory_visible_to_scope(&memory.metadata, &scope)
}

fn string_filter<'a>(metadata: &'a Metadata, key: &str) -> Option<&'a str> {
    metadata.get(key).and_then(Value::as_str)
}

impl From<RetrievalMode> for crate::rag::RetrievalMode {
    fn from(mode: RetrievalMode) -> Self {
        match mode {
            RetrievalMode::Exact => Self::Exact,
            RetrievalMode::Semantic => Self::Semantic,
            RetrievalMode::Graph => Self::Graph,
            RetrievalMode::Hybrid => Self::Hybrid,
            RetrievalMode::Recent => Self::Recent,
            RetrievalMode::Project => Self::Project,
            RetrievalMode::DecisionHistory => Self::DecisionHistory,
            RetrievalMode::Contradiction => Self::Contradiction,
        }
    }
}

fn convert_memory(memory: cms_core::MemoryObject) -> MemoryObject {
    let memory_id = MemoryId(memory.id.clone());
    MemoryObject {
        id: memory_id.clone(),
        memory_type: memory.memory_type,
        title: memory.title,
        summary: memory.summary,
        body: memory.body,
        claims: memory
            .claims
            .into_iter()
            .map(|claim| Claim {
                id: claim.id,
                text: claim.text,
                confidence: claim.confidence as f32,
                source: claim.source,
            })
            .collect(),
        links: memory
            .links
            .into_iter()
            .map(|link| MemoryLink {
                source_id: memory_id.clone(),
                target_id: link.target_id,
                relation: link.relation,
                confidence: link.confidence as f32,
            })
            .collect(),
        tags: memory.tags,
        confidence: memory.confidence as f32,
        source: memory.source.map(|source| source.reference),
        created_at: memory.created_at,
        updated_at: memory.updated_at,
        metadata: memory
            .metadata
            .into_iter()
            .map(|(key, value)| (key, Value::String(value)))
            .collect(),
    }
}

fn convert_api_memory(memory: MemoryObject) -> cms_core::MemoryObject {
    cms_core::MemoryObject {
        id: memory.id.0.clone(),
        memory_type: memory.memory_type,
        title: memory.title,
        summary: memory.summary,
        body: memory.body,
        claims: memory
            .claims
            .into_iter()
            .map(|claim| cms_core::Claim {
                id: claim.id,
                text: claim.text,
                confidence: claim.confidence as f64,
                source: claim.source,
            })
            .collect(),
        links: memory
            .links
            .into_iter()
            .map(|link| cms_core::MemoryLink {
                source_id: memory.id.0.clone(),
                target_id: link.target_id,
                relation: link.relation,
                confidence: link.confidence as f64,
            })
            .collect(),
        metadata: memory
            .metadata
            .into_iter()
            .map(|(key, value)| {
                let value = value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| value.to_string());
                (key, value)
            })
            .collect(),
        confidence: memory.confidence as f64,
        created_at: memory.created_at,
        updated_at: memory.updated_at,
        source: memory.source.map(|reference| MemorySource {
            kind: "cms-api".to_string(),
            reference,
        }),
        tags: memory.tags,
    }
}
