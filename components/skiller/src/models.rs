use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDocument {
    pub source_id: String,
    pub title: String,
    pub source_type: SourceType,
    pub origin: String,
    pub version: Option<String>,
    pub license: Option<String>,
    pub owner: Option<String>,
    pub visibility: Visibility,
    pub ingested_at: DateTime<Utc>,
    pub hash: String,
    pub retention_policy: RetentionPolicy,
    pub export_policy: ExportPolicy,
    pub secret_scan_status: ScanStatus,
    pub permission_status: PermissionStatus,
    pub citation_policy: CitationPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSection {
    pub section_id: String,
    pub source_id: String,
    pub heading: String,
    pub breadcrumbs: Vec<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub text_excerpt: String,
    pub code_blocks: Vec<String>,
    pub links: Vec<String>,
    pub detected_commands: Vec<String>,
    pub detected_api_operations: Vec<String>,
    pub detected_warnings: Vec<String>,
    pub detected_examples: Vec<String>,
    pub detected_normative_language: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub skill_type: SkillType,
    pub scope: SkillScope,
    pub status: SkillStatus,
    #[serde(default)]
    pub maturity: SkillMaturity,
    pub domain: Option<String>,
    #[serde(default)]
    pub source_section_ids: Vec<String>,
    #[serde(default)]
    pub procedure: Vec<String>,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub guardrails: Vec<String>,
    #[serde(default)]
    pub anti_patterns: Vec<String>,
    #[serde(default)]
    pub evals: Vec<EvalCase>,
    #[serde(default)]
    pub citations: Vec<Citation>,
    #[serde(default)]
    pub confidence: ConfidenceBreakdown,
    #[serde(default)]
    pub evidence_breakdown: EvidenceBreakdown,
    #[serde(default)]
    pub inference_records: Vec<InferenceRecord>,
    #[serde(default)]
    pub role_suitability: Vec<AgentRoleSuitability>,
    #[serde(default)]
    pub tool_requirements: Vec<ToolRequirement>,
    #[serde(default)]
    pub runtime_policy: RuntimePolicy,
    #[serde(default)]
    pub version_applicability: VersionApplicability,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillBundle {
    pub package: SkillPackage,
    pub sources: Vec<SourceDocument>,
    pub sections: Vec<DocumentSection>,
    pub capability_candidates: Vec<CapabilityCandidate>,
    pub skills: Vec<Skill>,
    pub graph: SkillGraph,
    pub audit_events: Vec<AuditEvent>,
    #[serde(default)]
    pub forge_requests: Vec<ForgeRequestEnvelope>,
    #[serde(default)]
    pub forge_responses: Vec<ForgeResponseEnvelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPackage {
    pub bundle_id: String,
    pub name: String,
    pub version: String,
    pub domain: Option<String>,
    pub source_corpus: Vec<String>,
    pub review_status: SkillStatus,
    pub publish_status: PublishStatus,
    pub compatibility: BTreeMap<String, String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillGraph {
    pub dependencies: Vec<SkillDependency>,
    pub related: Vec<RelatedSkill>,
    pub concepts: Vec<ConceptNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDependency {
    pub from_skill: String,
    pub to_skill: String,
    pub dependency_type: DependencyType,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedSkill {
    pub skill_a: String,
    pub skill_b: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptNode {
    pub concept: String,
    pub skill_ids: Vec<String>,
    pub source_section_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub citation_id: String,
    pub source_id: String,
    pub section_id: String,
    pub excerpt: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCase {
    pub id: String,
    pub prompt: String,
    pub expected_behavior: String,
    pub eval_type: EvalType,
    pub safety_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityCandidate {
    pub candidate_id: String,
    pub source_section_ids: Vec<String>,
    pub candidate_title: String,
    pub candidate_type: SkillType,
    pub detected_task: String,
    pub detected_inputs: Vec<String>,
    pub detected_outputs: Vec<String>,
    pub detected_procedures: Vec<String>,
    pub detected_warnings: Vec<String>,
    pub candidate_confidence: f32,
    pub evidence_strength: f32,
    pub extraction_type: EvidenceClass,
    pub related_candidates: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRecord {
    pub inference_id: String,
    pub candidate_ids_used: Vec<String>,
    pub source_refs_used: Vec<String>,
    pub reasoning_summary: String,
    pub inference_type: InferenceType,
    pub evidence_type: EvidenceClass,
    pub confidence: f32,
    pub unsupported_assumptions: Vec<String>,
    pub required_review: bool,
    pub risk_flags: Vec<String>,
    pub generated_by_agent: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceBreakdown {
    pub direct_extraction: f32,
    pub supporting_inference: f32,
    pub operational_synthesis: f32,
    pub speculative_candidate: f32,
    pub community_derived: f32,
    pub internal_policy_derived: f32,
}
impl Default for EvidenceBreakdown {
    fn default() -> Self {
        Self {
            direct_extraction: 1.0,
            supporting_inference: 0.0,
            operational_synthesis: 0.0,
            speculative_candidate: 0.0,
            community_derived: 0.0,
            internal_policy_derived: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceBreakdown {
    pub raw: f32,
    pub extraction: f32,
    pub inference: f32,
    pub procedure: f32,
    pub guardrail: f32,
    pub eval: f32,
    pub routing: f32,
    pub source_quality: f32,
    pub human_review: f32,
    pub runtime: f32,
}
impl Default for ConfidenceBreakdown {
    fn default() -> Self {
        Self {
            raw: 0.55,
            extraction: 0.7,
            inference: 0.1,
            procedure: 0.5,
            guardrail: 0.5,
            eval: 0.4,
            routing: 0.5,
            source_quality: 0.5,
            human_review: 0.0,
            runtime: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePolicy {
    pub conceptual_answer: bool,
    pub recommend_commands: bool,
    pub run_read_only_commands: bool,
    pub modify_files: bool,
    pub modify_external_systems: bool,
    pub requires_user_approval: bool,
    pub requires_backup_or_rollback: bool,
    pub handles_secrets: bool,
    pub handles_licensed_source: bool,
}
impl Default for RuntimePolicy {
    fn default() -> Self {
        Self {
            conceptual_answer: true,
            recommend_commands: true,
            run_read_only_commands: false,
            modify_files: false,
            modify_external_systems: false,
            requires_user_approval: true,
            requires_backup_or_rollback: false,
            handles_secrets: false,
            handles_licensed_source: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRequirement {
    pub name: String,
    pub requirement_type: ToolRequirementType,
    pub permission_level: PermissionLevel,
    pub dry_run_available: Option<bool>,
    pub rollback_required: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRoleSuitability {
    pub role: String,
    pub suitability: f32,
    pub rationale: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionApplicability {
    pub supported_versions: Vec<String>,
    pub unsupported_versions: Vec<String>,
    pub version_source_refs: Vec<String>,
    pub version_confidence: f32,
    pub migration_notes: Vec<String>,
    pub deprecated_flags: Vec<String>,
}
impl Default for VersionApplicability {
    fn default() -> Self {
        Self {
            supported_versions: vec![],
            unsupported_versions: vec![],
            version_source_refs: vec![],
            version_confidence: 0.0,
            migration_notes: vec![],
            deprecated_flags: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainProfile {
    pub name: String,
    pub preferred_skill_types: Vec<SkillType>,
    pub known_tools: Vec<String>,
    pub risk_categories: Vec<String>,
    pub common_task_types: Vec<String>,
    pub common_anti_patterns: Vec<String>,
    pub preferred_agent_roles: Vec<String>,
    pub source_trust_hierarchy: Vec<SourceTrust>,
    pub terminology: Vec<String>,
    pub required_review_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfileProposal {
    pub agent_id: String,
    pub agent_name: String,
    pub agent_purpose: String,
    pub recommended_skills: Vec<String>,
    #[serde(default)]
    pub selection_rationale: Vec<AgentSkillSelection>,
    #[serde(default)]
    pub proposal_readiness: AgentProposalReadinessStatus,
    pub required_tools: Vec<String>,
    pub allowed_actions: Vec<String>,
    pub disallowed_actions: Vec<String>,
    pub runtime_context_policy: String,
    pub review_policy: String,
    pub escalation_policy: String,
    pub example_tasks: Vec<String>,
    pub evaluation_suite: Vec<EvalCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentProposalReadinessStatus {
    pub ready_for_packaging: bool,
    pub ready_for_default_use_candidate: bool,
    pub selected_skill_count: usize,
    pub reviewed_skill_count: usize,
    pub optional_skill_count: usize,
    pub high_risk_skill_count: usize,
    pub eval_case_count: usize,
    pub routing_eval_count: usize,
    pub source_grounding_eval_count: usize,
    pub tool_use_planning_eval_count: usize,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSkillSelection {
    pub skill_id: String,
    pub title: String,
    pub score: i32,
    pub reasons: Vec<String>,
    pub status: Option<SkillStatus>,
    pub maturity: Option<SkillMaturity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillImprovementProposal {
    pub proposal_id: String,
    pub skill_id: String,
    pub trigger_source: String,
    pub problem_observed: String,
    pub suggested_change: String,
    pub evidence: Vec<String>,
    pub risk: RiskLevel,
    pub requires_recompile: bool,
    pub requires_review: bool,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeRequestEnvelope {
    pub request_id: String,
    pub provider: String,
    pub pass_type: ForgePassType,
    pub bundle_id: String,
    pub bundle_version: String,
    pub domain_profile: Option<DomainProfile>,
    pub source_sections: Vec<ForgeSectionPacket>,
    pub candidate_skills: Vec<Skill>,
    pub capability_candidates: Vec<CapabilityCandidate>,
    pub citation_ids: Vec<String>,
    #[serde(default)]
    pub source_context: Vec<ForgeSourceContext>,
    #[serde(default)]
    pub bundle_context: ForgeBundleContext,
    #[serde(default)]
    pub validation_constraints: Vec<String>,
    #[serde(default)]
    pub response_schema_guide: ForgeResponseSchemaGuide,
    #[serde(default)]
    pub prior_forge_summary: Vec<String>,
    pub graph_concepts: Vec<ConceptNode>,
    pub task_instruction: String,
    pub output_schema: String,
    pub token_budget: usize,
    pub risk_policy: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ForgeSourceContext {
    pub source_id: String,
    pub title: String,
    pub source_type: SourceType,
    pub origin: String,
    pub version: Option<String>,
    pub source_trust: String,
    pub export_policy: ExportPolicy,
    pub permission_status: PermissionStatus,
    pub secret_scan_status: ScanStatus,
    pub section_count: usize,
    pub selected_section_count: usize,
    pub skill_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ForgeBundleContext {
    pub bundle_name: String,
    pub domain: Option<String>,
    pub review_status: SkillStatus,
    pub publish_status: PublishStatus,
    pub compatibility: BTreeMap<String, String>,
    pub total_source_count: usize,
    pub total_section_count: usize,
    pub total_skill_count: usize,
    pub selected_skill_count: usize,
    pub high_risk_skill_count: usize,
    pub inference_record_count: usize,
    pub existing_forge_request_count: usize,
    pub existing_forge_response_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeSectionPacket {
    pub section_id: String,
    pub source_id: String,
    pub heading: String,
    pub excerpt: String,
    pub detected_commands: Vec<String>,
    pub detected_api_operations: Vec<String>,
    pub detected_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ForgeResponseSchemaGuide {
    pub envelope_type: String,
    pub required_fields: Vec<String>,
    pub field_guidance: Vec<ForgeResponseFieldGuide>,
    pub skill_output_rules: Vec<String>,
    pub evidence_record_rules: Vec<String>,
    pub confidence_update_rules: Vec<String>,
    pub forbidden_outputs: Vec<String>,
    pub minimal_valid_response: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ForgeResponseFieldGuide {
    pub field: String,
    pub required: bool,
    pub expected_type: String,
    pub guidance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeResponseEnvelope {
    pub request_id: String,
    pub pass_type: ForgePassType,
    pub generated_items: Vec<Skill>,
    pub modified_items: Vec<Skill>,
    pub review_findings: Vec<String>,
    pub confidence_updates: BTreeMap<String, ConfidenceBreakdown>,
    pub evidence_records: Vec<InferenceRecord>,
    pub required_human_review: bool,
    pub audit_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillReviewFinding {
    pub skill_id: String,
    pub decision: ReviewDecision,
    pub reviewer: String,
    pub rationale: String,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub required_changes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierReviewReport {
    pub report_id: String,
    pub bundle_id: String,
    pub reviewer: String,
    pub created_at: DateTime<Utc>,
    pub summary: String,
    pub findings: Vec<SkillReviewFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_id: String,
    pub event_type: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SourceType {
    Markdown,
    Html,
    Text,
    OpenApi,
    ApiSpec,
    CliSpec,
    CliHelp,
    Repository,
    Url,
    Pdf,
    Epub,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
    Internal,
    Restricted,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RetentionPolicy {
    ExcerptsOnly,
    FullTextAllowed,
    DeleteAfterCompile,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExportPolicy {
    PrivateOnly,
    OrganizationOnly,
    PublicAllowed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScanStatus {
    Clean,
    Findings(Vec<String>),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionStatus {
    Allowed,
    IndexOnly,
    Blocked(String),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CitationPolicy {
    PointersOnly,
    ShortExcerpts,
    QuotesAllowed,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillType {
    Concept,
    Procedure,
    Diagnostic,
    Review,
    ToolUse,
    ApiOperation,
    CliOperation,
    Safety,
    AgentRole,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillScope {
    Atomic,
    TaskLevel,
    WorkflowLevel,
    RoleLevel,
    DomainLevel,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillStatus {
    Draft,
    Candidate,
    NeedsReview,
    Reviewed,
    Approved,
    Published,
    Deprecated,
    Archived,
    Unsafe,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillMaturity {
    Level0RawCandidate,
    Level1StructuredCandidate,
    Level2ForgeEnhanced,
    Level3Verified,
    Level4HumanApproved,
    Level5RuntimeProven,
    Level6Certified,
}

impl Default for SkillMaturity {
    fn default() -> Self {
        Self::Level1StructuredCandidate
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PublishStatus {
    Unpublished,
    Staged,
    Published,
    Rejected,
}

impl Default for SourceType {
    fn default() -> Self {
        Self::Unknown
    }
}

impl Default for ExportPolicy {
    fn default() -> Self {
        Self::PrivateOnly
    }
}

impl Default for ScanStatus {
    fn default() -> Self {
        Self::Clean
    }
}

impl Default for PermissionStatus {
    fn default() -> Self {
        Self::Allowed
    }
}

impl Default for SkillStatus {
    fn default() -> Self {
        Self::Candidate
    }
}

impl Default for PublishStatus {
    fn default() -> Self {
        Self::Unpublished
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EvalType {
    Positive,
    Negative,
    EdgeCase,
    Safety,
    Routing,
    ToolUsePlanning,
    SourceGrounding,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EvidenceClass {
    DirectExtraction,
    SupportingInference,
    OperationalSynthesis,
    SpeculativeCandidate,
    CommunityDerived,
    InternalPolicyDerived,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InferenceType {
    Expansion,
    NewSkill,
    Merge,
    Split,
    Critique,
    AgentMapping,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DependencyType {
    PrerequisiteConcept,
    RequiredTool,
    RequiredSafetySkill,
    RequiredDomainContext,
    RequiredVersionContext,
    RequiredPolicy,
    RequiredDiagnosticStep,
    RequiredValidationStep,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolRequirementType {
    Required,
    Optional,
    ReadOnly,
    Mutating,
    Dangerous,
    ExternalDependency,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PermissionLevel {
    ConceptualOnly,
    RecommendOnly,
    ReadOnly,
    FileMutation,
    ExternalMutation,
    Dangerous,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SourceTrust {
    OfficialVendorDocumentation,
    OfficialApiSpecification,
    OfficialCliReference,
    OfficialStandardsDocument,
    ProjectMaintainerDocumentation,
    RepositoryTestsAndExamples,
    InternalCompanyDocumentation,
    CommunityGuide,
    ForumPost,
    GeneratedDocumentation,
    UnknownSource,
    ArchivedOrStaleSource,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReviewDecision {
    Approved,
    NeedsChanges,
    Unsafe,
    Duplicate,
    Archived,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ForgePassType {
    Interpretation,
    SkillExpansion,
    SkillInference,
    DeduplicationAndScope,
    SafetyAndGovernance,
    EvalGeneration,
    AgentRoleMapping,
    RegistryReadiness,
    Critique,
    VerifierReview,
}
