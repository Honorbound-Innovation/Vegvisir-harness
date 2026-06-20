use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Serialize)]
pub struct AgentTemplate {
    pub mode: String,
    pub display_name: String,
    pub description: String,
    pub system_prompt: String,
    pub enabled_tools: Vec<String>,
    pub enabled_skills: Vec<String>,
    pub usrl_contracts: Vec<String>,
    pub memory_policy: String,
}

#[derive(Serialize)]
pub struct DoctorReport {
    pub agents_root: PathBuf,
    pub profile_count: usize,
    pub invalid_files: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct ValidationIssue {
    pub severity: String,
    pub field: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct ValidationReport {
    pub id: String,
    pub status: String,
    pub errors: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationIssue>,
    pub recommendations: Vec<ValidationIssue>,
}

#[derive(Default, Serialize)]
pub struct RegisterReport {
    pub builtin_created: usize,
    pub skiller_created: usize,
    pub dry_run: bool,
    pub created_ids: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
pub struct AgentMetrics {
    #[serde(default)]
    pub tasks_completed: u64,
    #[serde(default)]
    pub tasks_failed: u64,
    #[serde(default)]
    pub tasks_cancelled: u64,
    #[serde(default)]
    pub verification_successes: u64,
    #[serde(default)]
    pub verification_failures: u64,
    #[serde(default)]
    pub scope_violations: u64,
    #[serde(default)]
    pub follow_up_fixes: u64,
    #[serde(default)]
    pub retries: u64,
    #[serde(default)]
    pub average_turnaround_ms: Option<u64>,
    #[serde(default)]
    pub capability_scores: BTreeMap<String, f64>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub last_evaluated: Option<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Serialize)]
pub struct MetricsReport {
    pub id: String,
    pub path: PathBuf,
    pub metrics: AgentMetrics,
    pub verification_success_rate: Option<f64>,
    pub task_success_rate: Option<f64>,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
pub struct AgentComparison {
    pub left_id: String,
    pub right_id: String,
    pub differences: Vec<FieldDifference>,
}

#[derive(Serialize)]
pub struct FieldDifference {
    pub field: String,
    pub left: Value,
    pub right: Value,
}

#[derive(Serialize, Deserialize)]
pub struct HistoryEvent {
    pub agent_id: String,
    pub action: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
    pub timestamp: String,
}

#[derive(Debug)]
pub enum SkillerAgentArtifact {
    Pack(PathBuf),
    ProposalIndex(PathBuf),
}

#[derive(Debug, Default, Deserialize)]
pub struct SkillerAgentPackOnDisk {
    #[serde(default)]
    pub agent_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub system_prompt_material: String,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub tool_permissions: Vec<String>,
    #[serde(default)]
    pub memory_policy: String,
    #[serde(default)]
    pub source_bundle_ids: Vec<String>,
    #[serde(default)]
    pub source_bundle_name: String,
    #[serde(default)]
    pub source_bundle_version: String,
}
