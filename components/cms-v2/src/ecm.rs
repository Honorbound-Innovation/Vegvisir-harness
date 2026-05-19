use crate::cms_api::{
    Claim, CmsApiError, CmsMemoryClient, CommitRequest, CommitResult, MemoryId, MemoryLink,
    MemoryObject, Metadata, ProjectId, RetrievalBundle, RetrievalMode, RetrievalRequest,
};
use crate::prompt_cache::{
    CacheScope, CacheSensitivity, CacheStability, ContextCacheHint, PromptCacheZone,
};
use crate::safety::detect_sensitive_content;
use crate::usrl::UsrlScopePolicy;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub String);

impl UserId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl From<&str> for UserId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for UserId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl From<&str> for SessionId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for SessionId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContextFrameType {
    System,
    UserRequest,
    UserPreference,
    ProjectState,
    TaskState,
    RetrievedMemory,
    ToolResult,
    Decision,
    Constraint,
    Summary,
    Scratch,
    OutputContract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum ContextPriority {
    P0Mandatory,
    P1Critical,
    P2Useful,
    P3Optional,
    P4Background,
    P5Discardable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContextFrameState {
    Observed,
    Classified,
    Retrieved,
    Assembled,
    Used,
    Distilled,
    Committed,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContextMode {
    Minimal,
    Session,
    Project,
    DeepProject,
    Research,
    Coding,
    Debugging,
    Architecture,
    MemoryRecall,
    DecisionReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskIntent {
    GeneralChat,
    ArchitectureDesign,
    CodeGeneration,
    Debugging,
    Planning,
    MemoryRecall,
    DecisionReview,
    ProjectUpdate,
    Research,
    FileAnalysis,
    Summarization,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentAnalysis {
    pub primary_intent: TaskIntent,
    pub secondary_intents: Vec<TaskIntent>,
    pub needs_memory: bool,
    pub needs_graph_context: bool,
    pub needs_semantic_context: bool,
    pub needs_recent_context: bool,
    pub needs_project_context: bool,
    pub persistence_likelihood: f32,
    pub suggested_retrieval_modes: Vec<RetrievalMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextFrame {
    pub id: String,
    pub frame_type: ContextFrameType,
    pub title: String,
    pub content: String,
    pub source: String,
    pub priority: ContextPriority,
    pub confidence: f32,
    pub token_estimate: usize,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub memory_ids: Vec<MemoryId>,
    pub tags: Vec<String>,
    pub metadata: Metadata,
    pub state: ContextFrameState,
}

impl ContextFrame {
    pub fn new(
        frame_type: ContextFrameType,
        title: impl Into<String>,
        content: impl Into<String>,
        source: impl Into<String>,
        priority: ContextPriority,
    ) -> Self {
        let content = content.into();
        Self {
            id: format!("frame_{}", Uuid::now_v7().simple()),
            frame_type,
            title: title.into(),
            token_estimate: estimate_tokens(&content),
            content,
            source: source.into(),
            priority,
            confidence: 1.0,
            created_at: Utc::now(),
            expires_at: None,
            memory_ids: Vec::new(),
            tags: Vec::new(),
            metadata: Metadata::new(),
            state: ContextFrameState::Observed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudget {
    pub max_tokens: usize,
    pub reserved_for_response: usize,
    pub reserved_for_system: usize,
    pub reserved_for_tools: usize,
}

impl ContextBudget {
    pub fn available_for_context(&self) -> usize {
        self.max_tokens
            .saturating_sub(self.reserved_for_response)
            .saturating_sub(self.reserved_for_system)
            .saturating_sub(self.reserved_for_tools)
    }
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            max_tokens: 16_000,
            reserved_for_response: 4_000,
            reserved_for_system: 1_000,
            reserved_for_tools: 1_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSession {
    pub id: SessionId,
    pub user_id: UserId,
    pub project_id: Option<ProjectId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub frames: Vec<ContextFrame>,
    pub metadata: Metadata,
}

impl ContextSession {
    pub fn new(user_id: UserId, project_id: Option<ProjectId>) -> Self {
        let now = Utc::now();
        Self {
            id: SessionId(format!("session_{}", Uuid::now_v7().simple())),
            user_id,
            project_id,
            created_at: now,
            updated_at: now,
            frames: Vec::new(),
            metadata: Metadata::new(),
        }
    }

    pub fn add_frame(&mut self, frame: ContextFrame) {
        self.frames.push(frame);
        self.updated_at = Utc::now();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedContext {
    pub session_id: SessionId,
    pub frames: Vec<ContextFrame>,
    pub cache_hints: Vec<ContextCacheHint>,
    pub packed_text: String,
    pub token_estimate: usize,
    pub included_memory_ids: Vec<MemoryId>,
    pub excluded_memory_ids: Vec<MemoryId>,
    pub trace_id: String,
    pub metadata: Metadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRequest {
    pub user_id: UserId,
    pub project_id: Option<ProjectId>,
    pub session: Option<ContextSession>,
    pub message: String,
    pub mode: ContextMode,
    pub budget: ContextBudget,
    pub metadata: Metadata,
}

impl ContextRequest {
    pub fn new(user_id: impl Into<UserId>, message: impl Into<String>, mode: ContextMode) -> Self {
        Self {
            user_id: user_id.into(),
            project_id: None,
            session: None,
            message: message.into(),
            mode,
            budget: ContextBudget::default(),
            metadata: Metadata::new(),
        }
    }

    pub fn with_project(mut self, project_id: impl Into<ProjectId>) -> Self {
        self.project_id = Some(project_id.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryCandidateType {
    DoNotStore,
    SessionNote,
    TaskCheckpoint,
    ProjectUpdate,
    DesignDecision,
    UserPreference,
    TechnicalFact,
    ArchitectureChange,
    Contradiction,
    OpenQuestion,
    ResolvedIssue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCandidate {
    pub candidate_type: MemoryCandidateType,
    pub title: String,
    pub summary: String,
    pub content: String,
    pub confidence: f32,
    pub importance: f32,
    pub project_id: Option<ProjectId>,
    pub suggested_memory_type: String,
    pub suggested_links: Vec<MemoryLink>,
    pub suggested_tags: Vec<String>,
    pub related_memory_ids: Vec<MemoryId>,
    pub persistence_reason: String,
    pub duplicate_check_required: bool,
}

pub struct EterniumContextManager<C> {
    cms: C,
}

impl<C> EterniumContextManager<C>
where
    C: CmsMemoryClient,
{
    pub fn new(cms: C) -> Self {
        Self { cms }
    }

    pub fn prepare_context(&self, request: ContextRequest) -> Result<PreparedContext, CmsApiError> {
        let user_id = request.user_id.clone();
        let memory_disabled_by_request = no_memory_mode(&request.metadata);
        let mut session = request
            .session
            .unwrap_or_else(|| ContextSession::new(request.user_id, request.project_id.clone()));
        let memory_disabled = memory_disabled_by_request || no_memory_mode(&session.metadata);
        let user_frame = ContextFrame::new(
            ContextFrameType::UserRequest,
            "Current user request",
            request.message.clone(),
            "user",
            ContextPriority::P0Mandatory,
        );
        session.add_frame(user_frame);

        let intent = analyze_intent(&request.message, request.mode);
        let retrieval_bundle = if intent.needs_memory && !memory_disabled {
            self.cms.retrieve(build_retrieval_request(
                &request.message,
                &user_id,
                &request.project_id,
                &intent,
                &request.budget,
                &request.metadata,
            ))?
        } else {
            RetrievalBundle::empty(request.message.clone())
        };
        let mut prepared = assemble_context(
            session,
            request.message,
            intent,
            retrieval_bundle,
            request.budget,
        );
        if memory_disabled {
            prepared
                .metadata
                .insert("memory_mode".to_string(), Value::String("none".to_string()));
        }
        Ok(prepared)
    }

    pub fn evaluate_writeback(
        &self,
        session: &ContextSession,
        user_message: &str,
        assistant_response: &str,
    ) -> Vec<MemoryCandidate> {
        evaluate_writeback_candidates(session, user_message, assistant_response)
    }

    pub fn complete_turn(
        &mut self,
        session: &ContextSession,
        user_message: &str,
        assistant_response: &str,
    ) -> Result<Vec<CommitResult>, CmsApiError> {
        let scope_policy = UsrlScopePolicy;
        let writeback_decision = scope_policy.writeback_decision(
            session.user_id.0.clone(),
            session
                .project_id
                .as_ref()
                .map(|project_id| project_id.0.clone()),
            &session.metadata,
        );
        if !writeback_decision.allowed {
            return Ok(Vec::new());
        }
        let candidates = self.evaluate_writeback(session, user_message, assistant_response);
        let mut results = Vec::new();
        for (candidate_index, candidate) in candidates.into_iter().enumerate() {
            if candidate.candidate_type == MemoryCandidateType::DoNotStore {
                continue;
            }
            if candidate.duplicate_check_required
                && self.is_duplicate_writeback_candidate(session, &candidate)?
            {
                continue;
            }
            let candidate_type = format!("{:?}", candidate.candidate_type);
            let importance = candidate.importance;
            let mut memory =
                memory_candidate_to_memory_object(&session.id, &session.user_id, candidate);
            for (key, value) in writeback_decision.metadata.clone() {
                memory.metadata.insert(key, value);
            }
            let correlation_id = session.metadata.get("correlation_id").cloned();
            if let Some(correlation_id) = correlation_id.clone() {
                memory
                    .metadata
                    .insert("correlation_id".to_string(), correlation_id);
            }
            let writeback_trace_id = format!("trace_writeback_{}", Uuid::now_v7().simple());
            memory.metadata.insert(
                "writeback_trace_id".to_string(),
                Value::String(writeback_trace_id.clone()),
            );
            let mut result = self.cms.commit_memory(CommitRequest {
                memory,
                reason: "ECM writeback evaluation".to_string(),
                deduplicate: true,
                update_existing: true,
            })?;
            result.trace.insert(
                "writeback_trace_id".to_string(),
                Value::String(writeback_trace_id),
            );
            result.trace.insert(
                "writeback_candidate_index".to_string(),
                Value::from(candidate_index as u64),
            );
            result.trace.insert(
                "writeback_candidate_type".to_string(),
                Value::String(candidate_type),
            );
            result
                .trace
                .insert("writeback_importance".to_string(), Value::from(importance));
            result.trace.insert(
                "writeback_session_id".to_string(),
                Value::String(session.id.0.clone()),
            );
            if let Some(correlation_id) = correlation_id {
                result
                    .trace
                    .insert("correlation_id".to_string(), correlation_id);
            }
            results.push(result);
        }
        Ok(results)
    }

    fn is_duplicate_writeback_candidate(
        &self,
        session: &ContextSession,
        candidate: &MemoryCandidate,
    ) -> Result<bool, CmsApiError> {
        let scope_policy = UsrlScopePolicy;
        let mut filters = scope_policy
            .writeback_decision(
                session.user_id.0.clone(),
                session
                    .project_id
                    .as_ref()
                    .map(|project_id| project_id.0.clone()),
                &session.metadata,
            )
            .metadata;
        filters.remove("usrl_scope_policy");
        filters.remove("usrl_scope_decision");
        filters.insert(
            "memory_mode".to_string(),
            Value::String("writeback_duplicate_check".to_string()),
        );

        let bundle = self.cms.retrieve(RetrievalRequest {
            query: candidate.summary.clone(),
            project_id: session.project_id.clone(),
            modes: vec![RetrievalMode::Exact, RetrievalMode::Semantic],
            memory_types: vec![candidate.suggested_memory_type.clone()],
            limit: 5,
            graph_depth: 1,
            include_contradictions: false,
            filters,
        })?;

        Ok(bundle
            .results
            .iter()
            .any(|result| candidate_matches_existing_memory(candidate, result)))
    }
}

pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

fn analyze_intent(message: &str, mode: ContextMode) -> IntentAnalysis {
    let lower = message.to_ascii_lowercase();
    let primary_intent = if matches!(mode, ContextMode::Architecture) {
        TaskIntent::ArchitectureDesign
    } else if matches!(mode, ContextMode::DecisionReview) || lower.contains("decision") {
        TaskIntent::DecisionReview
    } else if lower.contains("debug") || lower.contains("error") || lower.contains("failing") {
        TaskIntent::Debugging
    } else if lower.contains("plan") || lower.contains("continue") {
        TaskIntent::Planning
    } else if lower.contains("remember") || lower.contains("recall") {
        TaskIntent::MemoryRecall
    } else {
        TaskIntent::GeneralChat
    };

    let needs_memory = !matches!(mode, ContextMode::Minimal);
    let needs_project_context = matches!(
        mode,
        ContextMode::Project
            | ContextMode::DeepProject
            | ContextMode::Architecture
            | ContextMode::Coding
            | ContextMode::Debugging
    ) || lower.contains("project")
        || lower.contains("cms")
        || lower.contains("eternium");
    let needs_graph_context = matches!(mode, ContextMode::DeepProject | ContextMode::Architecture)
        || lower.contains("relationship")
        || lower.contains("depends");
    let needs_semantic_context = !matches!(mode, ContextMode::Minimal | ContextMode::Session);
    let needs_recent_context = matches!(mode, ContextMode::Session | ContextMode::Project)
        || lower.contains("recent")
        || lower.contains("continue");

    let mut suggested_retrieval_modes = Vec::new();
    if needs_project_context {
        suggested_retrieval_modes.push(RetrievalMode::Project);
    }
    if matches!(
        primary_intent,
        TaskIntent::DecisionReview | TaskIntent::ArchitectureDesign
    ) {
        suggested_retrieval_modes.push(RetrievalMode::DecisionHistory);
    }
    if needs_recent_context {
        suggested_retrieval_modes.push(RetrievalMode::Recent);
    }
    if needs_semantic_context {
        suggested_retrieval_modes.push(RetrievalMode::Semantic);
    }
    if needs_graph_context {
        suggested_retrieval_modes.push(RetrievalMode::Graph);
    }
    suggested_retrieval_modes.push(RetrievalMode::Hybrid);
    suggested_retrieval_modes.sort_by_key(|mode| format!("{mode:?}"));
    suggested_retrieval_modes.dedup();

    IntentAnalysis {
        primary_intent,
        secondary_intents: Vec::new(),
        needs_memory,
        needs_graph_context,
        needs_semantic_context,
        needs_recent_context,
        needs_project_context,
        persistence_likelihood: if matches!(primary_intent, TaskIntent::GeneralChat) {
            0.2
        } else {
            0.7
        },
        suggested_retrieval_modes,
    }
}

fn build_retrieval_request(
    message: &str,
    user_id: &UserId,
    project_id: &Option<ProjectId>,
    intent: &IntentAnalysis,
    budget: &ContextBudget,
    metadata: &Metadata,
) -> RetrievalRequest {
    let scope_policy = UsrlScopePolicy;
    let scope = scope_policy.retrieval_scope(
        user_id.0.clone(),
        project_id.as_ref().map(|project_id| project_id.0.clone()),
        metadata,
    );
    let mut filters = scope.to_metadata();
    if let Some(correlation_id) = metadata.get("correlation_id").and_then(Value::as_str) {
        filters.insert(
            "correlation_id".to_string(),
            Value::String(correlation_id.to_string()),
        );
    }

    RetrievalRequest {
        query: message.to_string(),
        project_id: project_id.clone(),
        modes: intent.suggested_retrieval_modes.clone(),
        memory_types: Vec::new(),
        limit: (budget.available_for_context() / 600).clamp(4, 24),
        graph_depth: if intent.needs_graph_context { 2 } else { 1 },
        include_contradictions: matches!(
            intent.primary_intent,
            TaskIntent::DecisionReview | TaskIntent::ArchitectureDesign
        ),
        filters,
    }
}

fn no_memory_mode(metadata: &Metadata) -> bool {
    metadata
        .get("memory_mode")
        .or_else(|| metadata.get("memory"))
        .and_then(Value::as_str)
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "none" | "no-memory" | "no_memory" | "disabled" | "off"
            )
        })
        .unwrap_or(false)
}

fn assemble_context(
    session: ContextSession,
    user_message: String,
    intent: IntentAnalysis,
    retrieval_bundle: RetrievalBundle,
    budget: ContextBudget,
) -> PreparedContext {
    let retrieval_trace = retrieval_bundle.trace.clone();
    let retrieval_result_count = retrieval_bundle.results.len();
    let retrieval_result_trace = retrieval_bundle
        .results
        .iter()
        .map(retrieval_result_trace_value)
        .collect::<Vec<_>>();
    let mut frames = Vec::new();
    frames.push(ContextFrame::new(
        ContextFrameType::System,
        "Eternium boundary rule",
        "CMS owns long-term memory. ECM owns active context exposure. Response generation belongs to the model/provider adapter.",
        "eternium",
        ContextPriority::P0Mandatory,
    ));

    frames.push(ContextFrame::new(
        ContextFrameType::UserRequest,
        "Current user request",
        user_message,
        "user",
        ContextPriority::P0Mandatory,
    ));

    for result in retrieval_bundle.results {
        let mut frame = ContextFrame::new(
            if result.memory.memory_type.contains("decision") {
                ContextFrameType::Decision
            } else {
                ContextFrameType::RetrievedMemory
            },
            result.memory.title.clone(),
            memory_frame_content(&result),
            "cms-api",
            if matches!(
                result.source_mode,
                RetrievalMode::Project | RetrievalMode::DecisionHistory
            ) {
                ContextPriority::P1Critical
            } else {
                ContextPriority::P2Useful
            },
        );
        frame.confidence = result.memory.confidence;
        frame.memory_ids.push(result.memory.id.clone());
        frame.tags = result.memory.tags.clone();
        frame.metadata.insert(
            "source_mode".to_string(),
            Value::String(format!("{:?}", result.source_mode)),
        );
        frame.metadata.insert(
            "content_hash".to_string(),
            Value::String(api_memory_content_hash(&result.memory)),
        );
        for key in [
            "prompt_zone",
            "prompt_cache_policy",
            "prompt_cache_sensitivity",
        ] {
            if let Some(value) = result.memory.metadata.get(key) {
                frame.metadata.insert(key.to_string(), value.clone());
            }
        }
        frame
            .metadata
            .insert("score".to_string(), Value::from(result.score));
        frames.push(frame);
    }

    frames.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| right.confidence.total_cmp(&left.confidence))
            .then_with(|| left.title.cmp(&right.title))
    });

    let mut packed_text = String::new();
    let mut included_memory_ids = Vec::new();
    let mut excluded_memory_ids = Vec::new();
    let mut included_frames = Vec::new();
    let mut cache_hints = Vec::new();
    let mut token_estimate = 0usize;
    let available = budget.available_for_context();
    for frame in frames {
        if token_estimate + frame.token_estimate > available
            && !matches!(frame.priority, ContextPriority::P0Mandatory)
        {
            excluded_memory_ids.extend(frame.memory_ids.clone());
            continue;
        }

        packed_text.push_str(&format!(
            "\n[{:?}] {}\n{}\n",
            frame.frame_type, frame.title, frame.content
        ));
        token_estimate += frame.token_estimate;
        included_memory_ids.extend(frame.memory_ids.clone());
        cache_hints.push(context_cache_hint_for_frame(&frame, &session));
        included_frames.push(frame);
    }

    let trace_id = format!("trace_{}", Uuid::now_v7().simple());
    let mut metadata = Metadata::new();
    metadata.insert(
        "context_trace_id".to_string(),
        Value::String(trace_id.clone()),
    );
    if let Some(correlation_id) = retrieval_trace.get("correlation_id").cloned() {
        metadata.insert("correlation_id".to_string(), correlation_id);
    }
    metadata.insert(
        "intent".to_string(),
        Value::String(format!("{:?}", intent.primary_intent)),
    );
    metadata.insert(
        "session_frame_count".to_string(),
        Value::from(session.frames.len()),
    );
    metadata.insert(
        "retrieval_result_count".to_string(),
        Value::from(retrieval_result_count as u64),
    );
    metadata.insert(
        "included_frame_count".to_string(),
        Value::from(included_frames.len() as u64),
    );
    metadata.insert(
        "included_memory_count".to_string(),
        Value::from(included_memory_ids.len() as u64),
    );
    metadata.insert(
        "excluded_memory_count".to_string(),
        Value::from(excluded_memory_ids.len() as u64),
    );
    metadata.insert(
        "token_budget_available".to_string(),
        Value::from(available as u64),
    );
    metadata.insert(
        "token_estimate".to_string(),
        Value::from(token_estimate as u64),
    );
    metadata.insert(
        "retrieval_trace".to_string(),
        Value::Object(retrieval_trace.into_iter().collect()),
    );
    metadata.insert(
        "retrieval_results".to_string(),
        Value::Array(retrieval_result_trace),
    );

    PreparedContext {
        session_id: session.id,
        frames: included_frames,
        cache_hints,
        packed_text: packed_text.trim().to_string(),
        token_estimate,
        included_memory_ids,
        excluded_memory_ids,
        trace_id,
        metadata,
    }
}

fn api_memory_content_hash(memory: &MemoryObject) -> String {
    let mut hasher = Sha256::new();
    hash_api_field(&mut hasher, "id", &memory.id.0);
    hash_api_field(&mut hasher, "type", &memory.memory_type);
    hash_api_field(&mut hasher, "title", &memory.title);
    hash_api_field(&mut hasher, "summary", &memory.summary);
    hash_api_field(&mut hasher, "body", &memory.body);
    hash_api_field(&mut hasher, "confidence", &memory.confidence.to_string());

    let mut claims = memory.claims.clone();
    claims.sort_by(|left, right| (&left.id, &left.text).cmp(&(&right.id, &right.text)));
    for claim in claims {
        hash_api_field(&mut hasher, "claim.id", &claim.id);
        hash_api_field(&mut hasher, "claim.text", &claim.text);
        hash_api_field(
            &mut hasher,
            "claim.confidence",
            &claim.confidence.to_string(),
        );
        if let Some(source) = claim.source {
            hash_api_field(&mut hasher, "claim.source", &source);
        }
    }

    let mut links = memory.links.clone();
    links.sort_by(|left, right| {
        (
            &left.source_id.0,
            &left.target_id,
            &left.relation,
            left.confidence.to_bits(),
        )
            .cmp(&(
                &right.source_id.0,
                &right.target_id,
                &right.relation,
                right.confidence.to_bits(),
            ))
    });
    for link in links {
        hash_api_field(&mut hasher, "link.source", &link.source_id.0);
        hash_api_field(&mut hasher, "link.target", &link.target_id);
        hash_api_field(&mut hasher, "link.relation", &link.relation);
        hash_api_field(&mut hasher, "link.confidence", &link.confidence.to_string());
    }

    let mut tags = memory.tags.clone();
    tags.sort();
    for tag in tags {
        hash_api_field(&mut hasher, "tag", &tag);
    }
    for (key, value) in &memory.metadata {
        hash_api_field(&mut hasher, "metadata.key", key);
        hash_api_field(&mut hasher, "metadata.value", &value.to_string());
    }
    format!("{:x}", hasher.finalize())
}

fn hash_api_field(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update(name.as_bytes());
    hasher.update([0]);
    hasher.update(value.len().to_le_bytes());
    hasher.update(value.as_bytes());
    hasher.update([0xff]);
}

fn context_cache_hint_for_frame(
    frame: &ContextFrame,
    session: &ContextSession,
) -> ContextCacheHint {
    let mut preferred_zone = match frame.frame_type {
        ContextFrameType::System | ContextFrameType::OutputContract => {
            PromptCacheZone::SystemKernel
        }
        ContextFrameType::UserPreference => PromptCacheZone::UserScopeCapsule,
        ContextFrameType::ProjectState
        | ContextFrameType::Decision
        | ContextFrameType::Constraint => PromptCacheZone::ProjectCapsule,
        ContextFrameType::RetrievedMemory | ContextFrameType::Summary => {
            PromptCacheZone::StableMemoryCapsule
        }
        ContextFrameType::ToolResult => PromptCacheZone::ToolResultTail,
        ContextFrameType::UserRequest => PromptCacheZone::CurrentTurn,
        ContextFrameType::TaskState | ContextFrameType::Scratch => {
            PromptCacheZone::DynamicRetrievedContext
        }
    };
    if let Some(zone) = frame
        .metadata
        .get("prompt_zone")
        .and_then(Value::as_str)
        .and_then(prompt_cache_zone_from_hint)
    {
        preferred_zone = zone;
    }

    let mut stability = match preferred_zone {
        PromptCacheZone::ToolDefinitions
        | PromptCacheZone::SystemKernel
        | PromptCacheZone::EterniumRuntime => CacheStability::Static,
        PromptCacheZone::UserScopeCapsule
        | PromptCacheZone::ProjectCapsule
        | PromptCacheZone::StableMemoryCapsule => CacheStability::Versioned,
        PromptCacheZone::SessionCheckpoint => CacheStability::Session,
        PromptCacheZone::DynamicRetrievedContext
        | PromptCacheZone::CurrentTurn
        | PromptCacheZone::ToolResultTail => CacheStability::Dynamic,
    };

    let mut scope = match preferred_zone {
        PromptCacheZone::ToolDefinitions
        | PromptCacheZone::SystemKernel
        | PromptCacheZone::EterniumRuntime => CacheScope::Global,
        PromptCacheZone::UserScopeCapsule => CacheScope::User,
        PromptCacheZone::ProjectCapsule | PromptCacheZone::StableMemoryCapsule => {
            if session.project_id.is_some() {
                CacheScope::Project
            } else {
                CacheScope::User
            }
        }
        PromptCacheZone::SessionCheckpoint | PromptCacheZone::DynamicRetrievedContext => {
            CacheScope::Session
        }
        PromptCacheZone::CurrentTurn | PromptCacheZone::ToolResultTail => CacheScope::Turn,
    };
    if let Some(policy) = frame
        .metadata
        .get("prompt_cache_policy")
        .and_then(Value::as_str)
    {
        match policy {
            "project_stable" | "project-stable" => {
                preferred_zone = PromptCacheZone::StableMemoryCapsule;
                stability = CacheStability::Versioned;
                scope = if session.project_id.is_some() {
                    CacheScope::Project
                } else {
                    CacheScope::User
                };
            }
            "session" | "session_checkpoint" | "session-checkpoint" => {
                preferred_zone = PromptCacheZone::SessionCheckpoint;
                stability = CacheStability::Session;
                scope = CacheScope::Session;
            }
            "dynamic" | "dynamic_context" | "dynamic-context" => {
                preferred_zone = PromptCacheZone::DynamicRetrievedContext;
                stability = CacheStability::Dynamic;
                scope = CacheScope::Session;
            }
            "no_cache" | "no-cache" | "none" => {
                stability = CacheStability::Dynamic;
                scope = CacheScope::Turn;
            }
            _ => {}
        }
    }

    let sensitivity = if let Some(hint) = frame
        .metadata
        .get("prompt_cache_sensitivity")
        .and_then(Value::as_str)
    {
        prompt_cache_sensitivity_from_hint(hint)
    } else if detect_sensitive_content(&frame.content).is_empty() {
        CacheSensitivity::Normal
    } else {
        CacheSensitivity::Secret
    };

    let source_version_hashes = frame
        .metadata
        .get("content_hash")
        .and_then(Value::as_str)
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();

    ContextCacheHint {
        frame_id: frame.id.clone(),
        preferred_zone,
        stability,
        scope,
        sensitivity,
        cache_policy_hint: frame
            .metadata
            .get("prompt_cache_policy")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        source_memory_ids: frame.memory_ids.iter().map(|id| id.0.clone()).collect(),
        source_version_hashes,
    }
}

fn prompt_cache_zone_from_hint(value: &str) -> Option<PromptCacheZone> {
    match value {
        "ToolDefinitions" | "tool_definitions" | "tool-definitions" => {
            Some(PromptCacheZone::ToolDefinitions)
        }
        "SystemKernel" | "system_kernel" | "system-kernel" => Some(PromptCacheZone::SystemKernel),
        "EterniumRuntime" | "eternium_runtime" | "eternium-runtime" => {
            Some(PromptCacheZone::EterniumRuntime)
        }
        "UserScopeCapsule" | "user_scope_capsule" | "user-scope-capsule" => {
            Some(PromptCacheZone::UserScopeCapsule)
        }
        "ProjectCapsule" | "project_capsule" | "project-capsule" => {
            Some(PromptCacheZone::ProjectCapsule)
        }
        "StableMemoryCapsule" | "stable_memory_capsule" | "stable-memory-capsule" => {
            Some(PromptCacheZone::StableMemoryCapsule)
        }
        "SessionCheckpoint" | "session_checkpoint" | "session-checkpoint" => {
            Some(PromptCacheZone::SessionCheckpoint)
        }
        "DynamicRetrievedContext" | "dynamic_retrieved_context" | "dynamic-retrieved-context" => {
            Some(PromptCacheZone::DynamicRetrievedContext)
        }
        "CurrentTurn" | "current_turn" | "current-turn" => Some(PromptCacheZone::CurrentTurn),
        "ToolResultTail" | "tool_result_tail" | "tool-result-tail" => {
            Some(PromptCacheZone::ToolResultTail)
        }
        _ => None,
    }
}

fn prompt_cache_sensitivity_from_hint(value: &str) -> CacheSensitivity {
    match value {
        "public" | "Public" => CacheSensitivity::Public,
        "sensitive" | "Sensitive" => CacheSensitivity::Sensitive,
        "secret" | "Secret" => CacheSensitivity::Secret,
        _ => CacheSensitivity::Normal,
    }
}

fn retrieval_result_trace_value(result: &crate::cms_api::MemoryRetrievalResult) -> Value {
    let mut object = Map::new();
    object.insert(
        "memory_id".to_string(),
        Value::String(result.memory.id.0.clone()),
    );
    object.insert(
        "memory_type".to_string(),
        Value::String(result.memory.memory_type.clone()),
    );
    object.insert(
        "source_mode".to_string(),
        Value::String(format!("{:?}", result.source_mode)),
    );
    object.insert("score".to_string(), Value::from(result.score));
    object.insert("reason".to_string(), Value::String(result.reason.clone()));
    Value::Object(object)
}

fn candidate_matches_existing_memory(
    candidate: &MemoryCandidate,
    result: &crate::cms_api::MemoryRetrievalResult,
) -> bool {
    if result.memory.memory_type != candidate.suggested_memory_type {
        return false;
    }
    if result.score >= 0.95 {
        return true;
    }

    let candidate_summary = normalize_for_duplicate_check(&candidate.summary);
    let existing_summary = normalize_for_duplicate_check(&result.memory.summary);
    let existing_body = normalize_for_duplicate_check(&result.memory.body);

    !candidate_summary.is_empty()
        && (existing_summary.contains(&candidate_summary)
            || existing_body.contains(&candidate_summary)
            || candidate_summary.contains(&existing_summary))
}

fn normalize_for_duplicate_check(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn memory_frame_content(result: &crate::cms_api::MemoryRetrievalResult) -> String {
    let memory = &result.memory;
    let mut content = String::new();
    if !memory.summary.trim().is_empty() {
        content.push_str(memory.summary.trim());
    } else {
        content.push_str(memory.body.trim());
    }
    if !result.reason.trim().is_empty() {
        content.push_str("\nreason: ");
        content.push_str(&result.reason);
    }
    content
}

fn evaluate_writeback_candidates(
    session: &ContextSession,
    user_message: &str,
    assistant_response: &str,
) -> Vec<MemoryCandidate> {
    let combined = format!("{user_message}\n{assistant_response}");
    let sensitive_findings = detect_sensitive_content(&combined);
    if !sensitive_findings.is_empty() {
        return vec![MemoryCandidate {
            candidate_type: MemoryCandidateType::DoNotStore,
            title: "Sensitive turn suppressed".to_string(),
            summary: "Turn contained secret-like content and was not selected for writeback."
                .to_string(),
            content: "Sensitive content suppressed before persistence.".to_string(),
            confidence: 1.0,
            importance: 1.0,
            project_id: session.project_id.clone(),
            suggested_memory_type: "safety-event".to_string(),
            suggested_links: Vec::new(),
            suggested_tags: vec![
                "ecm-writeback".to_string(),
                "discarded".to_string(),
                "sensitive-content".to_string(),
            ],
            related_memory_ids: Vec::new(),
            persistence_reason: format!(
                "suppressed {} sensitive finding(s) before storage",
                sensitive_findings.len()
            ),
            duplicate_check_required: false,
        }];
    }
    let lower = combined.to_ascii_lowercase();
    let candidate_type = if lower.contains("do not remember")
        || lower.contains("don't remember")
        || lower.contains("temporary")
    {
        MemoryCandidateType::DoNotStore
    } else if lower.contains("prefer") || lower.contains("preference") {
        MemoryCandidateType::UserPreference
    } else if lower.contains("open question")
        || lower.contains("todo")
        || lower.contains("follow up")
    {
        MemoryCandidateType::OpenQuestion
    } else if lower.contains("resolved") || lower.contains("fixed") {
        MemoryCandidateType::ResolvedIssue
    } else if lower.contains("contradict") || lower.contains("conflict") {
        MemoryCandidateType::Contradiction
    } else if lower.contains("architecture")
        || lower.contains("boundary")
        || lower.contains("module")
    {
        MemoryCandidateType::ArchitectureChange
    } else if lower.contains("decided")
        || lower.contains("decision")
        || lower.contains("we will")
        || lower.contains("must")
        || lower.contains("should")
    {
        MemoryCandidateType::DesignDecision
    } else {
        MemoryCandidateType::DoNotStore
    };

    if candidate_type == MemoryCandidateType::DoNotStore {
        return vec![MemoryCandidate {
            candidate_type,
            title: "Do not store turn".to_string(),
            summary: "Turn did not meet ECM writeback persistence thresholds.".to_string(),
            content: combined,
            confidence: 0.4,
            importance: 0.1,
            project_id: session.project_id.clone(),
            suggested_memory_type: "session-note".to_string(),
            suggested_links: Vec::new(),
            suggested_tags: vec!["ecm-writeback".to_string(), "discarded".to_string()],
            related_memory_ids: Vec::new(),
            persistence_reason: "low durable value or explicit no-store signal".to_string(),
            duplicate_check_required: false,
        }];
    }

    let suggested_memory_type = match candidate_type {
        MemoryCandidateType::UserPreference => "user-preference",
        MemoryCandidateType::OpenQuestion => "open-question",
        MemoryCandidateType::ResolvedIssue => "resolved-issue",
        MemoryCandidateType::Contradiction => "contradiction",
        MemoryCandidateType::ArchitectureChange => "architecture-change",
        MemoryCandidateType::DesignDecision => "design-decision",
        MemoryCandidateType::TaskCheckpoint => "task-checkpoint",
        MemoryCandidateType::ProjectUpdate => "project-update",
        MemoryCandidateType::TechnicalFact => "technical-fact",
        MemoryCandidateType::SessionNote => "session-note",
        MemoryCandidateType::DoNotStore => "session-note",
    }
    .to_string();

    let related_memory_ids = session
        .frames
        .iter()
        .flat_map(|frame| frame.memory_ids.clone())
        .collect::<Vec<_>>();
    let title = writeback_title(candidate_type, user_message);
    vec![MemoryCandidate {
        candidate_type,
        title: title.clone(),
        summary: summarize_turn(user_message, assistant_response),
        content: format!("User:\n{user_message}\n\nAssistant:\n{assistant_response}"),
        confidence: 0.82,
        importance: match candidate_type {
            MemoryCandidateType::ArchitectureChange | MemoryCandidateType::DesignDecision => 0.9,
            MemoryCandidateType::UserPreference | MemoryCandidateType::Contradiction => 0.85,
            _ => 0.7,
        },
        project_id: session.project_id.clone(),
        suggested_memory_type,
        suggested_links: related_memory_ids
            .iter()
            .map(|memory_id| MemoryLink {
                source_id: MemoryId("pending".to_string()),
                target_id: memory_id.0.clone(),
                relation: "derived_from_context".to_string(),
                confidence: 0.75,
            })
            .collect(),
        suggested_tags: vec![
            "ecm-writeback".to_string(),
            format!("{:?}", candidate_type).to_ascii_lowercase(),
        ],
        related_memory_ids,
        persistence_reason: format!(
            "{candidate_type:?} is durable according to ECM writeback rules"
        ),
        duplicate_check_required: true,
    }]
}

fn memory_candidate_to_memory_object(
    session_id: &SessionId,
    user_id: &UserId,
    candidate: MemoryCandidate,
) -> MemoryObject {
    let now = Utc::now();
    let memory_id = MemoryId(format!("mem_ecm_{}", Uuid::now_v7().simple()));
    let links = candidate
        .suggested_links
        .into_iter()
        .map(|mut link| {
            link.source_id = memory_id.clone();
            link
        })
        .collect();
    let mut metadata = Metadata::new();
    metadata.insert(
        "candidate_type".to_string(),
        Value::String(format!("{:?}", candidate.candidate_type)),
    );
    metadata.insert(
        "importance".to_string(),
        Value::from(candidate.importance as f64),
    );
    metadata.insert(
        "persistence_reason".to_string(),
        Value::String(candidate.persistence_reason),
    );
    metadata.insert(
        "source_session_id".to_string(),
        Value::String(session_id.0.clone()),
    );
    metadata.insert("user_id".to_string(), Value::String(user_id.0.clone()));
    metadata.insert(
        "visibility".to_string(),
        Value::String("private".to_string()),
    );
    if let Some(project_id) = &candidate.project_id {
        metadata.insert(
            "project_id".to_string(),
            Value::String(project_id.0.clone()),
        );
    }

    MemoryObject {
        id: memory_id,
        memory_type: candidate.suggested_memory_type,
        title: candidate.title.clone(),
        summary: candidate.summary,
        body: candidate.content,
        claims: vec![Claim {
            id: "claim_001".to_string(),
            text: format!(
                "ECM selected this turn for durable memory as {:?}.",
                candidate.candidate_type
            ),
            confidence: candidate.confidence,
            source: Some("ecm.writeback".to_string()),
        }],
        links,
        tags: candidate.suggested_tags,
        confidence: candidate.confidence,
        source: Some("ecm.writeback".to_string()),
        created_at: now,
        updated_at: now,
        metadata,
    }
}

fn writeback_title(candidate_type: MemoryCandidateType, user_message: &str) -> String {
    let compact = user_message
        .split_whitespace()
        .take(8)
        .collect::<Vec<_>>()
        .join(" ");
    let compact = if compact.is_empty() {
        "turn".to_string()
    } else {
        compact
    };
    format!("ECM {:?}: {compact}", candidate_type)
}

fn summarize_turn(user_message: &str, assistant_response: &str) -> String {
    let user = user_message.trim();
    let assistant = assistant_response.trim();
    let assistant = assistant
        .split_whitespace()
        .take(32)
        .collect::<Vec<_>>()
        .join(" ");
    format!("User asked: {user}. Assistant outcome: {assistant}")
}
