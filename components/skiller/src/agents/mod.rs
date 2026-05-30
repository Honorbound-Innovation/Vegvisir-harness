use crate::ingest::stable_id;
use crate::models::*;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentPackEvalStatus {
    pub passed: bool,
    pub selected_skill_count: usize,
    pub total_eval_cases: usize,
    pub skills_without_evals: usize,
    pub safety_eval_count: usize,
    pub routing_eval_count: usize,
    pub source_grounding_eval_count: usize,
    pub tool_use_planning_eval_count: usize,
    pub failures: Vec<String>,
    pub warnings: Vec<String>,
    pub skill_eval_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentPackLifecycleStatus {
    pub plan_id: String,
    pub lifecycle_ready: bool,
    pub human_review_required: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentPackReadinessStatus {
    pub ready_for_default_use: bool,
    pub ready_for_runtime_use: bool,
    pub selected_skill_count: usize,
    pub required_skill_count: usize,
    pub optional_skill_count: usize,
    pub forbidden_skill_count: usize,
    pub high_risk_selected_skill_count: usize,
    pub lifecycle_ready: Option<bool>,
    pub evals_passed: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentPackSelectionReport {
    pub agent_name: String,
    pub selected_skill_count: usize,
    pub required_skill_count: usize,
    pub optional_skill_count: usize,
    pub forbidden_skill_count: usize,
    pub selections: Vec<AgentSkillSelection>,
    pub omitted_skills: Vec<AgentSkillSelection>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct KaProfile {
    pub schema_version: u32,
    pub id: String,
    pub display_name: String,
    pub summary: String,
    pub voice: KaVoice,
    pub temperament: KaTemperament,
    pub work_style: KaWorkStyle,
    pub risk_modulation: KaRiskModulation,
    pub boundaries: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct KaVoice {
    pub warmth: String,
    pub directness: String,
    pub humor: String,
    pub formality: String,
    pub theatricality: String,
    pub metaphor_density: String,
    pub avoid: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct KaTemperament {
    pub energy: String,
    pub patience: String,
    pub curiosity: String,
    pub confidence_style: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct KaWorkStyle {
    pub progress_updates: String,
    pub failure_style: String,
    pub uncertainty_style: String,
    pub collaboration_style: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct KaRiskModulation {
    pub normal: String,
    pub high_risk: String,
    pub user_frustrated: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AgentPackVerificationReport {
    pub valid: bool,
    pub pack_path: String,
    pub manifest_path: String,
    pub markdown_path: String,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentPackBuildReport {
    pub build_id: String,
    pub agent_name: String,
    pub bundle_id: String,
    pub bundle_name: String,
    pub bundle_version: String,
    pub out_dir: String,
    pub pack_file: String,
    pub manifest_file: String,
    pub markdown_file: String,
    pub selected_skill_count: usize,
    pub required_skill_count: usize,
    pub optional_skill_count: usize,
    pub forbidden_skill_count: usize,
    pub omitted_skill_count: usize,
    pub tool_permission_count: usize,
    pub eval_case_count: usize,
    pub ready_for_runtime_use: bool,
    pub ready_for_default_use: bool,
    pub lifecycle_ready: Option<bool>,
    pub evals_passed: bool,
    pub verification_valid: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub selected_skill_ids: Vec<String>,
    pub required_skill_ids: Vec<String>,
    pub optional_skill_ids: Vec<String>,
    pub forbidden_skill_ids: Vec<String>,
    pub omitted_skill_ids: Vec<String>,
    pub tool_permissions: Vec<String>,
    pub verification_errors: Vec<String>,
    pub verification_warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentBuilderSummary {
    pub summary_id: String,
    pub valid: bool,
    pub proposals_path: Option<String>,
    pub pack_paths: Vec<String>,
    pub proposal_count: usize,
    pub pack_count: usize,
    pub ready_for_packaging_count: usize,
    pub ready_for_default_use_candidate_count: usize,
    pub runtime_ready_pack_count: usize,
    pub default_ready_pack_count: usize,
    pub verification_errors: Vec<String>,
    pub verification_warnings: Vec<String>,
    pub proposals: Vec<AgentBuilderProposalSummaryEntry>,
    pub packs: Vec<AgentBuilderPackSummaryEntry>,
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentBuilderProposalSummaryEntry {
    pub agent_id: String,
    pub agent_name: String,
    pub file: String,
    pub selected_skill_count: usize,
    pub reviewed_skill_count: usize,
    pub ready_for_packaging: bool,
    pub ready_for_default_use_candidate: bool,
    pub required_tools: Vec<String>,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentBuilderPackSummaryEntry {
    pub pack_id: String,
    pub agent_name: String,
    pub path: String,
    pub selected_skill_count: usize,
    pub required_skill_count: usize,
    pub forbidden_skill_count: usize,
    pub ready_for_runtime_use: bool,
    pub ready_for_default_use: bool,
    pub lifecycle_ready: Option<bool>,
    pub evals_passed: bool,
    pub tool_permissions: Vec<String>,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AgentArtifactIndex {
    pub index_id: String,
    pub root: String,
    pub valid: bool,
    pub proposal_directory_count: usize,
    pub proposal_count: usize,
    pub pack_directory_count: usize,
    pub summary_count: usize,
    pub build_report_count: usize,
    pub verification_error_count: usize,
    pub verification_warning_count: usize,
    pub proposals: Vec<AgentArtifactProposalDirectoryEntry>,
    pub packs: Vec<AgentArtifactPackDirectoryEntry>,
    pub summaries: Vec<AgentArtifactSummaryEntry>,
    pub build_reports: Vec<AgentArtifactBuildReportEntry>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AgentArtifactProposalDirectoryEntry {
    pub path: String,
    pub valid: bool,
    pub proposal_count: usize,
    pub ready_for_packaging_count: usize,
    pub default_use_candidate_count: usize,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AgentArtifactPackDirectoryEntry {
    pub path: String,
    pub valid: bool,
    pub pack_id: Option<String>,
    pub agent_name: Option<String>,
    pub ready_for_runtime_use: Option<bool>,
    pub ready_for_default_use: Option<bool>,
    pub selected_skill_count: Option<usize>,
    pub tool_permission_count: Option<usize>,
    pub tool_permissions: Vec<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AgentArtifactSummaryEntry {
    pub path: String,
    pub valid: bool,
    pub proposal_count: usize,
    pub pack_count: usize,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AgentArtifactBuildReportEntry {
    pub path: String,
    pub agent_name: String,
    pub verification_valid: bool,
    pub ready_for_runtime_use: bool,
    pub ready_for_default_use: bool,
    pub selected_skill_count: usize,
    pub forbidden_skill_count: usize,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AgentPackManifest {
    pub pack_id: String,
    pub agent_name: String,
    pub agent_version: String,
    pub bundle_id: String,
    pub bundle_name: String,
    pub bundle_version: String,
    pub agent_pack_file: String,
    pub manifest_file: String,
    pub markdown_file: String,
    pub selected_skill_count: usize,
    pub required_skill_count: usize,
    pub optional_skill_count: usize,
    pub forbidden_skill_count: usize,
    pub tool_permission_count: usize,
    pub eval_case_count: usize,
    pub ready_for_runtime_use: bool,
    pub ready_for_default_use: bool,
    pub lifecycle_ready: Option<bool>,
    pub evals_passed: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub selected_skill_ids: Vec<String>,
    pub required_skill_ids: Vec<String>,
    pub optional_skill_ids: Vec<String>,
    pub forbidden_skill_ids: Vec<String>,
    pub tool_permissions: Vec<String>,
    pub files: Vec<String>,
}

pub fn proposals(bundle: &SkillBundle) -> Vec<AgentProfileProposal> {
    let mut roles = BTreeSet::new();
    for s in proposal_eligible_skills(bundle) {
        for r in &s.role_suitability {
            roles.insert(r.role.clone());
        }
    }
    if roles.is_empty() {
        roles.insert("Technical Documentation Agent".into());
    }
    roles
        .into_iter()
        .map(|role| {
            let selection_rationale = ranked_skill_selections_for_role(bundle, &role, 10);
            let recommended_skills: Vec<String> = selection_rationale
                .iter()
                .map(|selection| selection.skill_id.clone())
                .collect();
            let recommended_set: BTreeSet<String> = recommended_skills.iter().cloned().collect();
            let recommended_skill_refs: Vec<&Skill> = proposal_eligible_skills(bundle)
                .into_iter()
                .filter(|s| recommended_set.contains(&s.id))
                .collect();
            let required_tools = recommended_skill_refs
                .iter()
                .flat_map(|s| s.tool_requirements.iter().map(|t| t.name.clone()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let evaluation_suite = recommended_skill_refs
                .iter()
                .flat_map(|s| s.evals.iter().cloned())
                .take(10)
                .collect();
            let proposal_readiness =
                agent_proposal_readiness_status(&recommended_skill_refs, &selection_rationale);
            AgentProfileProposal {
                agent_id: stable_agent_id(&bundle.package.bundle_id, &role),
                agent_name: role.clone(),
                agent_purpose: agent_purpose(&role, &recommended_skill_refs, bundle),
                recommended_skills,
                selection_rationale,
                proposal_readiness,
                required_tools,
                allowed_actions: allowed_actions_for_skills(&recommended_skill_refs),
                disallowed_actions: disallowed_actions_for_skills(&recommended_skill_refs),
                runtime_context_policy: "Load the highest-scoring role-matched skills first; include citations and dependencies within budget.".into(),
                review_policy: review_policy_for_skills(&recommended_skill_refs),
                escalation_policy: "Escalate missing evidence, secrets, unsupported source claims, or requested actions outside selected skill permissions."
                    .into(),
                example_tasks: example_tasks_for_role(&role, &recommended_skill_refs),
                evaluation_suite,
            }
        })
        .collect()
}

fn agent_proposal_readiness_status(
    selected_skills: &[&Skill],
    selection_rationale: &[AgentSkillSelection],
) -> AgentProposalReadinessStatus {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    if selected_skills.is_empty() {
        blockers.push("proposal selects no usable skills".to_string());
    }

    let reviewed_skill_count = selected_skills
        .iter()
        .filter(|skill| {
            matches!(
                skill.status,
                SkillStatus::Reviewed | SkillStatus::Approved | SkillStatus::Published
            ) && skill.maturity >= SkillMaturity::Level3Verified
        })
        .count();
    if reviewed_skill_count == 0 {
        blockers.push("proposal has no reviewed/verified selected skills".to_string());
    }

    let optional_skill_count = selected_skills.len().saturating_sub(reviewed_skill_count);
    if optional_skill_count > 0 {
        warnings.push("proposal includes candidate or under-reviewed selected skills".to_string());
    }

    let high_risk_skill_count = selected_skills
        .iter()
        .filter(|skill| is_high_risk_agent_skill(skill))
        .count();
    for skill in selected_skills {
        if is_high_risk_agent_skill(skill)
            && (!matches!(skill.status, SkillStatus::Approved | SkillStatus::Published)
                || skill.maturity < SkillMaturity::Level4HumanApproved
                || skill.confidence.human_review < 0.8)
        {
            blockers.push(format!(
                "{}: high-risk proposal skill requires Approved/Published status, Level4 human approval, and human_review confidence >= 0.8",
                skill.id
            ));
        }
        if skill
            .inference_records
            .iter()
            .any(|record| record.required_review)
        {
            warnings.push(format!(
                "{}: selected skill has inference records requiring review",
                skill.id
            ));
        }
    }

    let mut eval_case_count = 0usize;
    let mut routing_eval_count = 0usize;
    let mut source_grounding_eval_count = 0usize;
    let mut tool_use_planning_eval_count = 0usize;
    for skill in selected_skills {
        if skill.evals.is_empty() {
            blockers.push(format!("{}: selected skill has no eval cases", skill.id));
        }
        eval_case_count += skill.evals.len();
        let mut has_routing = false;
        let mut has_source_grounding = false;
        let mut has_tool_use_planning = false;
        for eval in &skill.evals {
            match eval.eval_type {
                EvalType::Routing => {
                    has_routing = true;
                    routing_eval_count += 1;
                }
                EvalType::SourceGrounding => {
                    has_source_grounding = true;
                    source_grounding_eval_count += 1;
                }
                EvalType::ToolUsePlanning => {
                    has_tool_use_planning = true;
                    tool_use_planning_eval_count += 1;
                }
                EvalType::Positive | EvalType::Negative | EvalType::EdgeCase | EvalType::Safety => {
                }
            }
        }
        if !has_routing {
            warnings.push(format!(
                "{}: proposal lacks routing eval coverage",
                skill.id
            ));
        }
        if !has_source_grounding {
            warnings.push(format!(
                "{}: proposal lacks source-grounding eval coverage",
                skill.id
            ));
        }
        if (skill.runtime_policy.modify_files
            || skill.runtime_policy.modify_external_systems
            || !skill.tool_requirements.is_empty())
            && !has_tool_use_planning
        {
            warnings.push(format!(
                "{}: operational proposal skill lacks tool-use-planning eval coverage",
                skill.id
            ));
        }
    }

    if selection_rationale
        .iter()
        .any(|selection| selection.score < 20)
    {
        warnings.push(
            "proposal includes low-scoring role matches; review skill fit before packaging"
                .to_string(),
        );
    }

    let ready_for_packaging = blockers.is_empty();
    let ready_for_default_use_candidate = ready_for_packaging
        && optional_skill_count == 0
        && high_risk_skill_count == 0
        && routing_eval_count > 0
        && source_grounding_eval_count > 0;

    AgentProposalReadinessStatus {
        ready_for_packaging,
        ready_for_default_use_candidate,
        selected_skill_count: selected_skills.len(),
        reviewed_skill_count,
        optional_skill_count,
        high_risk_skill_count,
        eval_case_count,
        routing_eval_count,
        source_grounding_eval_count,
        tool_use_planning_eval_count,
        blockers,
        warnings,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AgentProposalIndex {
    pub bundle_id: String,
    pub bundle_name: String,
    pub bundle_version: String,
    pub proposal_count: usize,
    pub ready_for_packaging_count: usize,
    pub default_use_candidate_count: usize,
    pub blocked_proposal_count: usize,
    pub warning_count: usize,
    pub proposals: Vec<AgentProposalIndexEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AgentProposalIndexEntry {
    pub agent_id: String,
    pub agent_name: String,
    pub file: String,
    pub selected_skill_count: usize,
    pub reviewed_skill_count: usize,
    pub high_risk_skill_count: usize,
    pub eval_case_count: usize,
    pub ready_for_packaging: bool,
    pub ready_for_default_use_candidate: bool,
    pub required_tools: Vec<String>,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AgentProposalVerificationReport {
    pub valid: bool,
    pub proposals_path: String,
    pub index_path: String,
    pub markdown_path: String,
    pub proposal_count: usize,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn write_agent_proposals(bundle: &SkillBundle, out: &Path) -> Result<()> {
    fs::create_dir_all(out)?;
    let proposals = proposals(bundle);
    let mut index_entries = Vec::new();

    for proposal in &proposals {
        let file_name = agent_proposal_file_name(&proposal.agent_name);
        fs::write(out.join(&file_name), serde_yaml::to_string(proposal)?)?;
        index_entries.push(AgentProposalIndexEntry {
            agent_id: proposal.agent_id.clone(),
            agent_name: proposal.agent_name.clone(),
            file: file_name,
            selected_skill_count: proposal.proposal_readiness.selected_skill_count,
            reviewed_skill_count: proposal.proposal_readiness.reviewed_skill_count,
            high_risk_skill_count: proposal.proposal_readiness.high_risk_skill_count,
            eval_case_count: proposal.proposal_readiness.eval_case_count,
            ready_for_packaging: proposal.proposal_readiness.ready_for_packaging,
            ready_for_default_use_candidate: proposal
                .proposal_readiness
                .ready_for_default_use_candidate,
            required_tools: proposal.required_tools.clone(),
            blockers: proposal.proposal_readiness.blockers.clone(),
            warnings: proposal.proposal_readiness.warnings.clone(),
        });
    }

    let index = AgentProposalIndex {
        bundle_id: bundle.package.bundle_id.clone(),
        bundle_name: bundle.package.name.clone(),
        bundle_version: bundle.package.version.clone(),
        proposal_count: index_entries.len(),
        ready_for_packaging_count: index_entries
            .iter()
            .filter(|entry| entry.ready_for_packaging)
            .count(),
        default_use_candidate_count: index_entries
            .iter()
            .filter(|entry| entry.ready_for_default_use_candidate)
            .count(),
        blocked_proposal_count: index_entries
            .iter()
            .filter(|entry| !entry.blockers.is_empty())
            .count(),
        warning_count: index_entries.iter().map(|entry| entry.warnings.len()).sum(),
        proposals: index_entries,
    };
    fs::write(
        out.join("agent-proposals-index.yaml"),
        serde_yaml::to_string(&index)?,
    )?;
    fs::write(
        out.join("agent-proposals-index.md"),
        render_agent_proposal_index_markdown(&index),
    )?;
    Ok(())
}

fn agent_proposal_file_name(agent_name: &str) -> String {
    format!("{}.yaml", agent_name.to_lowercase().replace(' ', "-"))
}

pub fn verify_agent_proposals(path: &Path) -> Result<AgentProposalVerificationReport> {
    let index_path = path.join("agent-proposals-index.yaml");
    let markdown_path = path.join("agent-proposals-index.md");
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let index_text = match fs::read_to_string(&index_path) {
        Ok(text) => text,
        Err(err) => {
            errors.push(format!(
                "agent-proposals-index.yaml missing or unreadable: {err}"
            ));
            String::new()
        }
    };

    let parsed_index: Option<AgentProposalIndex> = if index_text.is_empty() {
        None
    } else {
        match serde_yaml::from_str(&index_text) {
            Ok(index) => Some(index),
            Err(err) => {
                errors.push(format!("agent-proposals-index.yaml is invalid: {err}"));
                None
            }
        }
    };

    let mut proposals = Vec::new();
    if let Some(index) = &parsed_index {
        let mut seen_files = BTreeSet::new();
        let mut seen_agent_ids = BTreeSet::new();
        for entry in &index.proposals {
            if entry.file.is_empty() || entry.file.contains("..") || entry.file.starts_with('/') {
                errors.push(format!(
                    "proposal index entry for {} has unsafe file path {}",
                    entry.agent_name, entry.file
                ));
                continue;
            }
            if !seen_files.insert(entry.file.clone()) {
                errors.push(format!("duplicate proposal file in index: {}", entry.file));
            }
            let proposal_path = path.join(&entry.file);
            let text = match fs::read_to_string(&proposal_path) {
                Ok(text) => text,
                Err(err) => {
                    errors.push(format!(
                        "proposal file {} missing or unreadable: {err}",
                        entry.file
                    ));
                    continue;
                }
            };
            let proposal: AgentProfileProposal = match serde_yaml::from_str(&text) {
                Ok(proposal) => proposal,
                Err(err) => {
                    errors.push(format!("proposal file {} is invalid: {err}", entry.file));
                    continue;
                }
            };
            if !seen_agent_ids.insert(proposal.agent_id.clone()) {
                errors.push(format!(
                    "duplicate proposal agent_id: {}",
                    proposal.agent_id
                ));
            }
            if proposal.agent_id != entry.agent_id {
                errors.push(format!(
                    "proposal file {} agent_id mismatch: index={} file={}",
                    entry.file, entry.agent_id, proposal.agent_id
                ));
            }
            if proposal.agent_name != entry.agent_name {
                errors.push(format!(
                    "proposal file {} agent_name mismatch: index={} file={}",
                    entry.file, entry.agent_name, proposal.agent_name
                ));
            }
            if proposal.proposal_readiness.selected_skill_count != entry.selected_skill_count {
                errors.push(format!(
                    "proposal file {} selected_skill_count mismatch",
                    entry.file
                ));
            }
            if proposal.proposal_readiness.reviewed_skill_count != entry.reviewed_skill_count {
                errors.push(format!(
                    "proposal file {} reviewed_skill_count mismatch",
                    entry.file
                ));
            }
            if proposal.proposal_readiness.high_risk_skill_count != entry.high_risk_skill_count {
                errors.push(format!(
                    "proposal file {} high_risk_skill_count mismatch",
                    entry.file
                ));
            }
            if proposal.proposal_readiness.eval_case_count != entry.eval_case_count {
                errors.push(format!(
                    "proposal file {} eval_case_count mismatch",
                    entry.file
                ));
            }
            if proposal.proposal_readiness.ready_for_packaging != entry.ready_for_packaging {
                errors.push(format!(
                    "proposal file {} ready_for_packaging mismatch",
                    entry.file
                ));
            }
            if proposal.proposal_readiness.ready_for_default_use_candidate
                != entry.ready_for_default_use_candidate
            {
                errors.push(format!(
                    "proposal file {} ready_for_default_use_candidate mismatch",
                    entry.file
                ));
            }
            let tools: BTreeSet<_> = proposal.required_tools.iter().cloned().collect();
            let entry_tools: BTreeSet<_> = entry.required_tools.iter().cloned().collect();
            if tools != entry_tools {
                errors.push(format!(
                    "proposal file {} required_tools mismatch",
                    entry.file
                ));
            }
            proposals.push(proposal);
        }

        let recomputed = agent_proposal_index_from_proposals(
            &index.bundle_id,
            &index.bundle_name,
            &index.bundle_version,
            &proposals,
        );
        if &recomputed != index {
            errors.push(
                "agent-proposals-index.yaml is stale or inconsistent with proposal files".into(),
            );
        }
        let expected_md = render_agent_proposal_index_markdown(&recomputed);
        match fs::read_to_string(&markdown_path) {
            Ok(actual_md) => {
                if actual_md != expected_md {
                    errors.push(
                        "agent-proposals-index.md is stale or inconsistent with proposal files"
                            .into(),
                    );
                }
            }
            Err(err) => errors.push(format!(
                "agent-proposals-index.md missing or unreadable: {err}"
            )),
        }

        for entry in fs::read_dir(path).with_context(|| {
            format!(
                "failed to list agent proposals directory {}",
                path.display()
            )
        })? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.ends_with(".yaml")
                && file_name != "agent-proposals-index.yaml"
                && !index
                    .proposals
                    .iter()
                    .any(|proposal| proposal.file == file_name)
            {
                warnings.push(format!("unindexed proposal YAML file: {file_name}"));
            }
        }
    }

    Ok(AgentProposalVerificationReport {
        valid: errors.is_empty(),
        proposals_path: path.display().to_string(),
        index_path: index_path.display().to_string(),
        markdown_path: markdown_path.display().to_string(),
        proposal_count: parsed_index
            .as_ref()
            .map(|index| index.proposal_count)
            .unwrap_or(0),
        errors,
        warnings,
    })
}

fn agent_proposal_index_from_proposals(
    bundle_id: &str,
    bundle_name: &str,
    bundle_version: &str,
    proposals: &[AgentProfileProposal],
) -> AgentProposalIndex {
    let proposals = proposals
        .iter()
        .map(|proposal| AgentProposalIndexEntry {
            agent_id: proposal.agent_id.clone(),
            agent_name: proposal.agent_name.clone(),
            file: agent_proposal_file_name(&proposal.agent_name),
            selected_skill_count: proposal.proposal_readiness.selected_skill_count,
            reviewed_skill_count: proposal.proposal_readiness.reviewed_skill_count,
            high_risk_skill_count: proposal.proposal_readiness.high_risk_skill_count,
            eval_case_count: proposal.proposal_readiness.eval_case_count,
            ready_for_packaging: proposal.proposal_readiness.ready_for_packaging,
            ready_for_default_use_candidate: proposal
                .proposal_readiness
                .ready_for_default_use_candidate,
            required_tools: proposal.required_tools.clone(),
            blockers: proposal.proposal_readiness.blockers.clone(),
            warnings: proposal.proposal_readiness.warnings.clone(),
        })
        .collect::<Vec<_>>();
    AgentProposalIndex {
        bundle_id: bundle_id.to_string(),
        bundle_name: bundle_name.to_string(),
        bundle_version: bundle_version.to_string(),
        proposal_count: proposals.len(),
        ready_for_packaging_count: proposals
            .iter()
            .filter(|entry| entry.ready_for_packaging)
            .count(),
        default_use_candidate_count: proposals
            .iter()
            .filter(|entry| entry.ready_for_default_use_candidate)
            .count(),
        blocked_proposal_count: proposals
            .iter()
            .filter(|entry| !entry.blockers.is_empty())
            .count(),
        warning_count: proposals.iter().map(|entry| entry.warnings.len()).sum(),
        proposals,
    }
}

fn render_agent_proposal_index_markdown(index: &AgentProposalIndex) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Agent Proposal Index: {}\n\n",
        index.bundle_name
    ));
    out.push_str(&format!("- Bundle ID: `{}`\n", index.bundle_id));
    out.push_str(&format!("- Version: `{}`\n", index.bundle_version));
    out.push_str(&format!("- Proposals: {}\n", index.proposal_count));
    out.push_str(&format!(
        "- Ready for packaging: {}\n",
        index.ready_for_packaging_count
    ));
    out.push_str(&format!(
        "- Default-use candidates: {}\n",
        index.default_use_candidate_count
    ));
    out.push_str(&format!(
        "- Blocked proposals: {}\n",
        index.blocked_proposal_count
    ));
    out.push_str(&format!("- Warning count: {}\n\n", index.warning_count));

    out.push_str("## Proposals\n\n");
    for proposal in &index.proposals {
        out.push_str(&format!("### {}\n\n", proposal.agent_name));
        out.push_str(&format!("- File: `{}`\n", proposal.file));
        out.push_str(&format!("- Agent ID: `{}`\n", proposal.agent_id));
        out.push_str(&format!(
            "- Ready for packaging: {}\n",
            proposal.ready_for_packaging
        ));
        out.push_str(&format!(
            "- Ready for default use candidate: {}\n",
            proposal.ready_for_default_use_candidate
        ));
        out.push_str(&format!(
            "- Selected skills: {}\n",
            proposal.selected_skill_count
        ));
        out.push_str(&format!(
            "- Reviewed skills: {}\n",
            proposal.reviewed_skill_count
        ));
        out.push_str(&format!(
            "- High-risk skills: {}\n",
            proposal.high_risk_skill_count
        ));
        out.push_str(&format!("- Eval cases: {}\n", proposal.eval_case_count));
        if !proposal.required_tools.is_empty() {
            out.push_str(&format!(
                "- Required tools: {}\n",
                proposal.required_tools.join(", ")
            ));
        }
        if !proposal.blockers.is_empty() {
            out.push_str("- Blockers:\n");
            for blocker in &proposal.blockers {
                out.push_str(&format!("  - {}\n", blocker));
            }
        }
        if !proposal.warnings.is_empty() {
            out.push_str("- Warnings:\n");
            for warning in &proposal.warnings {
                out.push_str(&format!("  - {}\n", warning));
            }
        }
        out.push('\n');
    }
    out
}

#[derive(Deserialize, Serialize)]
struct AgentPack<'a> {
    agent_name: &'a str,
    agent_version: &'a str,
    description: String,
    source_bundle_ids: Vec<String>,
    source_bundle_name: String,
    source_bundle_version: String,
    skill_ids: Vec<String>,
    required_skills: Vec<String>,
    optional_skills: Vec<String>,
    forbidden_skills: Vec<String>,
    tool_permissions: Vec<String>,
    runtime_policy: String,
    context_policy: String,
    memory_policy: String,
    approval_policy: String,
    review_status: SkillStatus,
    evals: Vec<EvalCase>,
    example_prompts: Vec<String>,
    ka: KaProfile,
    system_prompt_material: String,
    lifecycle_status: Option<AgentPackLifecycleStatus>,
    eval_status: AgentPackEvalStatus,
    selection_report: AgentPackSelectionReport,
    pack_readiness: AgentPackReadinessStatus,
}
pub fn verify_agent_pack(path: &Path) -> Result<AgentPackVerificationReport> {
    let pack_path = path.join("agent-pack.yaml");
    let manifest_path = path.join("agent-pack-manifest.yaml");
    let markdown_path = path.join("agent-pack-manifest.md");
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let pack_text = match fs::read_to_string(&pack_path) {
        Ok(text) => text,
        Err(err) => {
            errors.push(format!("missing or unreadable agent-pack.yaml: {err}"));
            String::new()
        }
    };
    let manifest_text = match fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(err) => {
            errors.push(format!(
                "missing or unreadable agent-pack-manifest.yaml: {err}"
            ));
            String::new()
        }
    };
    let markdown_text = match fs::read_to_string(&markdown_path) {
        Ok(text) => text,
        Err(err) => {
            errors.push(format!(
                "missing or unreadable agent-pack-manifest.md: {err}"
            ));
            String::new()
        }
    };

    if errors.is_empty() {
        let pack: AgentPack<'_> = match serde_yaml::from_str(&pack_text) {
            Ok(pack) => pack,
            Err(err) => {
                errors.push(format!("agent-pack.yaml is malformed: {err}"));
                return Ok(AgentPackVerificationReport {
                    valid: false,
                    pack_path: pack_path.display().to_string(),
                    manifest_path: manifest_path.display().to_string(),
                    markdown_path: markdown_path.display().to_string(),
                    errors,
                    warnings,
                });
            }
        };
        let actual_manifest: AgentPackManifest = match serde_yaml::from_str(&manifest_text) {
            Ok(manifest) => manifest,
            Err(err) => {
                errors.push(format!("agent-pack-manifest.yaml is malformed: {err}"));
                return Ok(AgentPackVerificationReport {
                    valid: false,
                    pack_path: pack_path.display().to_string(),
                    manifest_path: manifest_path.display().to_string(),
                    markdown_path: markdown_path.display().to_string(),
                    errors,
                    warnings,
                });
            }
        };

        let expected_manifest = agent_pack_manifest_from_pack(&pack);
        if actual_manifest != expected_manifest {
            errors
                .push("agent-pack-manifest.yaml is stale or does not match agent-pack.yaml".into());
        }
        let expected_markdown = agent_pack_manifest_markdown(&expected_manifest);
        if markdown_text != expected_markdown {
            errors.push("agent-pack-manifest.md is stale or does not match agent-pack.yaml".into());
        }

        for file in &actual_manifest.files {
            let file_path = path.join(file);
            if !file_path.exists() {
                errors.push(format!("manifest file entry is missing on disk: {file}"));
            }
        }
        if actual_manifest.ready_for_runtime_use && !actual_manifest.evals_passed {
            warnings.push("agent pack is runtime-ready while evals are not marked passed".into());
        }
    }

    Ok(AgentPackVerificationReport {
        valid: errors.is_empty(),
        pack_path: pack_path.display().to_string(),
        manifest_path: manifest_path.display().to_string(),
        markdown_path: markdown_path.display().to_string(),
        errors,
        warnings,
    })
}

pub fn write_agent_builder_summary(
    proposals_path: Option<&Path>,
    pack_paths: &[PathBuf],
    out: &Path,
) -> Result<AgentBuilderSummary> {
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let summary = agent_builder_summary(proposals_path, pack_paths)?;
    fs::write(out, serde_yaml::to_string(&summary)?)?;
    let markdown_path = out.with_extension("md");
    fs::write(&markdown_path, agent_builder_summary_markdown(&summary))?;
    Ok(summary)
}

pub fn agent_builder_summary(
    proposals_path: Option<&Path>,
    pack_paths: &[PathBuf],
) -> Result<AgentBuilderSummary> {
    let mut verification_errors = Vec::new();
    let mut verification_warnings = Vec::new();
    let mut proposal_entries = Vec::new();
    let mut pack_entries = Vec::new();
    let mut files = Vec::new();

    if let Some(path) = proposals_path {
        let report = verify_agent_proposals(path)?;
        verification_errors.extend(
            report
                .errors
                .iter()
                .map(|err| format!("agent proposals: {err}")),
        );
        verification_warnings.extend(
            report
                .warnings
                .iter()
                .map(|warning| format!("agent proposals: {warning}")),
        );
        files.push(
            path.join("agent-proposals-index.yaml")
                .display()
                .to_string(),
        );
        files.push(path.join("agent-proposals-index.md").display().to_string());
        if let Ok(index_text) = fs::read_to_string(path.join("agent-proposals-index.yaml")) {
            match serde_yaml::from_str::<AgentProposalIndex>(&index_text) {
                Ok(index) => {
                    for proposal in index.proposals {
                        files.push(path.join(&proposal.file).display().to_string());
                        proposal_entries.push(AgentBuilderProposalSummaryEntry {
                            agent_id: proposal.agent_id,
                            agent_name: proposal.agent_name,
                            file: path.join(proposal.file).display().to_string(),
                            selected_skill_count: proposal.selected_skill_count,
                            reviewed_skill_count: proposal.reviewed_skill_count,
                            ready_for_packaging: proposal.ready_for_packaging,
                            ready_for_default_use_candidate: proposal
                                .ready_for_default_use_candidate,
                            required_tools: proposal.required_tools,
                            blockers: proposal.blockers,
                            warnings: proposal.warnings,
                        });
                    }
                }
                Err(err) => verification_errors.push(format!(
                    "agent proposals: failed to parse index for summary: {err}"
                )),
            }
        }
    }

    let mut seen_pack_paths = BTreeSet::new();
    for pack_path in pack_paths {
        let display = pack_path.display().to_string();
        if !seen_pack_paths.insert(display.clone()) {
            verification_errors.push(format!("duplicate agent pack path: {display}"));
            continue;
        }
        let report = verify_agent_pack(pack_path)?;
        verification_errors.extend(
            report
                .errors
                .iter()
                .map(|err| format!("agent pack {}: {err}", pack_path.display())),
        );
        verification_warnings.extend(
            report
                .warnings
                .iter()
                .map(|warning| format!("agent pack {}: {warning}", pack_path.display())),
        );
        files.push(pack_path.join("agent-pack.yaml").display().to_string());
        files.push(
            pack_path
                .join("agent-pack-manifest.yaml")
                .display()
                .to_string(),
        );
        files.push(
            pack_path
                .join("agent-pack-manifest.md")
                .display()
                .to_string(),
        );
        match fs::read_to_string(pack_path.join("agent-pack-manifest.yaml")) {
            Ok(manifest_text) => match serde_yaml::from_str::<AgentPackManifest>(&manifest_text) {
                Ok(manifest) => pack_entries.push(AgentBuilderPackSummaryEntry {
                    pack_id: manifest.pack_id,
                    agent_name: manifest.agent_name,
                    path: pack_path.display().to_string(),
                    selected_skill_count: manifest.selected_skill_count,
                    required_skill_count: manifest.required_skill_count,
                    forbidden_skill_count: manifest.forbidden_skill_count,
                    ready_for_runtime_use: manifest.ready_for_runtime_use,
                    ready_for_default_use: manifest.ready_for_default_use,
                    lifecycle_ready: manifest.lifecycle_ready,
                    evals_passed: manifest.evals_passed,
                    tool_permissions: manifest.tool_permissions,
                    blockers: manifest.blockers,
                    warnings: manifest.warnings,
                }),
                Err(err) => verification_errors.push(format!(
                    "agent pack {}: failed to parse manifest for summary: {err}",
                    pack_path.display()
                )),
            },
            Err(err) => verification_errors.push(format!(
                "agent pack {}: missing manifest for summary: {err}",
                pack_path.display()
            )),
        }
    }

    files.sort();
    files.dedup();
    proposal_entries.sort_by(|a, b| a.agent_name.cmp(&b.agent_name));
    pack_entries.sort_by(|a, b| a.agent_name.cmp(&b.agent_name));

    let ready_for_packaging_count = proposal_entries
        .iter()
        .filter(|proposal| proposal.ready_for_packaging)
        .count();
    let ready_for_default_use_candidate_count = proposal_entries
        .iter()
        .filter(|proposal| proposal.ready_for_default_use_candidate)
        .count();
    let runtime_ready_pack_count = pack_entries
        .iter()
        .filter(|pack| pack.ready_for_runtime_use)
        .count();
    let default_ready_pack_count = pack_entries
        .iter()
        .filter(|pack| pack.ready_for_default_use)
        .count();
    let summary_key = format!(
        "{}:{}:{}:{}:{}",
        proposals_path
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "no-proposals".into()),
        proposal_entries
            .iter()
            .map(|entry| format!("{}:{}", entry.agent_id, entry.ready_for_packaging))
            .collect::<Vec<_>>()
            .join("|"),
        pack_entries
            .iter()
            .map(|entry| format!(
                "{}:{}:{}",
                entry.pack_id, entry.ready_for_runtime_use, entry.ready_for_default_use
            ))
            .collect::<Vec<_>>()
            .join("|"),
        verification_errors.join("|"),
        verification_warnings.join("|")
    );

    Ok(AgentBuilderSummary {
        summary_id: stable_id("agent-builder-summary", &summary_key),
        valid: verification_errors.is_empty(),
        proposals_path: proposals_path.map(|path| path.display().to_string()),
        pack_paths: pack_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        proposal_count: proposal_entries.len(),
        pack_count: pack_entries.len(),
        ready_for_packaging_count,
        ready_for_default_use_candidate_count,
        runtime_ready_pack_count,
        default_ready_pack_count,
        verification_errors,
        verification_warnings,
        proposals: proposal_entries,
        packs: pack_entries,
        files,
    })
}

pub fn write_agent_artifact_index(root: &Path, out: &Path) -> Result<AgentArtifactIndex> {
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let index = agent_artifact_index(root)?;
    fs::write(out, serde_yaml::to_string(&index)?)?;
    fs::write(
        out.with_extension("md"),
        agent_artifact_index_markdown(&index),
    )?;
    Ok(index)
}

pub fn agent_artifact_index(root: &Path) -> Result<AgentArtifactIndex> {
    let mut proposal_dirs = BTreeSet::new();
    let mut pack_dirs = BTreeSet::new();
    let mut summary_files = BTreeSet::new();
    let mut build_report_files = BTreeSet::new();
    collect_agent_artifact_paths(
        root,
        &mut proposal_dirs,
        &mut pack_dirs,
        &mut summary_files,
        &mut build_report_files,
    )?;

    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut files = Vec::new();
    let mut proposals = Vec::new();
    let mut packs = Vec::new();
    let mut summaries = Vec::new();
    let mut build_reports = Vec::new();

    for dir in proposal_dirs {
        let report = verify_agent_proposals(&dir)?;
        errors.extend(
            report
                .errors
                .iter()
                .map(|err| format!("proposal directory {}: {err}", dir.display())),
        );
        warnings.extend(
            report
                .warnings
                .iter()
                .map(|warning| format!("proposal directory {}: {warning}", dir.display())),
        );
        let index = fs::read_to_string(dir.join("agent-proposals-index.yaml"))
            .ok()
            .and_then(|text| serde_yaml::from_str::<AgentProposalIndex>(&text).ok());
        let proposal_count = index
            .as_ref()
            .map(|i| i.proposal_count)
            .unwrap_or(report.proposal_count);
        let ready_for_packaging_count = index
            .as_ref()
            .map(|i| i.ready_for_packaging_count)
            .unwrap_or_default();
        let default_use_candidate_count = index
            .as_ref()
            .map(|i| i.default_use_candidate_count)
            .unwrap_or_default();
        files.push(dir.join("agent-proposals-index.yaml").display().to_string());
        files.push(dir.join("agent-proposals-index.md").display().to_string());
        if let Some(index) = index {
            for entry in &index.proposals {
                files.push(dir.join(&entry.file).display().to_string());
            }
        }
        proposals.push(AgentArtifactProposalDirectoryEntry {
            path: dir.display().to_string(),
            valid: report.valid,
            proposal_count,
            ready_for_packaging_count,
            default_use_candidate_count,
            errors: report.errors,
            warnings: report.warnings,
        });
    }

    for dir in pack_dirs {
        let report = verify_agent_pack(&dir)?;
        errors.extend(
            report
                .errors
                .iter()
                .map(|err| format!("agent pack {}: {err}", dir.display())),
        );
        warnings.extend(
            report
                .warnings
                .iter()
                .map(|warning| format!("agent pack {}: {warning}", dir.display())),
        );
        let manifest = fs::read_to_string(dir.join("agent-pack-manifest.yaml"))
            .ok()
            .and_then(|text| serde_yaml::from_str::<AgentPackManifest>(&text).ok());
        files.push(dir.join("agent-pack.yaml").display().to_string());
        files.push(dir.join("agent-pack-manifest.yaml").display().to_string());
        files.push(dir.join("agent-pack-manifest.md").display().to_string());
        packs.push(AgentArtifactPackDirectoryEntry {
            path: dir.display().to_string(),
            valid: report.valid,
            pack_id: manifest.as_ref().map(|m| m.pack_id.clone()),
            agent_name: manifest.as_ref().map(|m| m.agent_name.clone()),
            ready_for_runtime_use: manifest.as_ref().map(|m| m.ready_for_runtime_use),
            ready_for_default_use: manifest.as_ref().map(|m| m.ready_for_default_use),
            selected_skill_count: manifest.as_ref().map(|m| m.selected_skill_count),
            tool_permission_count: manifest.as_ref().map(|m| m.tool_permission_count),
            tool_permissions: manifest
                .as_ref()
                .map(|m| m.tool_permissions.clone())
                .unwrap_or_default(),
            errors: report.errors,
            warnings: report.warnings,
        });
    }

    for path in summary_files {
        let text = fs::read_to_string(&path).unwrap_or_default();
        let parsed = serde_yaml::from_str::<AgentBuilderSummary>(&text);
        let (valid, proposal_count, pack_count, entry_errors, entry_warnings) = match parsed {
            Ok(summary) => (
                summary.valid,
                summary.proposal_count,
                summary.pack_count,
                summary.verification_errors,
                summary.verification_warnings,
            ),
            Err(err) => (
                false,
                0,
                0,
                vec![format!("agent-builder summary is invalid: {err}")],
                Vec::new(),
            ),
        };
        errors.extend(
            entry_errors
                .iter()
                .map(|err| format!("summary {}: {err}", path.display())),
        );
        warnings.extend(
            entry_warnings
                .iter()
                .map(|warning| format!("summary {}: {warning}", path.display())),
        );
        files.push(path.display().to_string());
        files.push(path.with_extension("md").display().to_string());
        summaries.push(AgentArtifactSummaryEntry {
            path: path.display().to_string(),
            valid,
            proposal_count,
            pack_count,
            errors: entry_errors,
            warnings: entry_warnings,
        });
    }

    for path in build_report_files {
        let text = fs::read_to_string(&path).unwrap_or_default();
        match serde_yaml::from_str::<AgentPackBuildReport>(&text) {
            Ok(report) => {
                if !report.verification_valid {
                    errors.push(format!(
                        "build report {} records invalid verification",
                        path.display()
                    ));
                }
                warnings.extend(
                    report
                        .verification_warnings
                        .iter()
                        .map(|warning| format!("build report {}: {warning}", path.display())),
                );
                files.push(path.display().to_string());
                build_reports.push(AgentArtifactBuildReportEntry {
                    path: path.display().to_string(),
                    agent_name: report.agent_name,
                    verification_valid: report.verification_valid,
                    ready_for_runtime_use: report.ready_for_runtime_use,
                    ready_for_default_use: report.ready_for_default_use,
                    selected_skill_count: report.selected_skill_count,
                    forbidden_skill_count: report.forbidden_skill_count,
                    errors: report.verification_errors,
                    warnings: report.verification_warnings,
                });
            }
            Err(err) => {
                let msg = format!("build report {} is invalid: {err}", path.display());
                errors.push(msg.clone());
                build_reports.push(AgentArtifactBuildReportEntry {
                    path: path.display().to_string(),
                    agent_name: String::new(),
                    verification_valid: false,
                    ready_for_runtime_use: false,
                    ready_for_default_use: false,
                    selected_skill_count: 0,
                    forbidden_skill_count: 0,
                    errors: vec![msg],
                    warnings: Vec::new(),
                });
            }
        }
    }

    files.sort();
    files.dedup();
    let proposal_count = proposals.iter().map(|entry| entry.proposal_count).sum();
    let valid = errors.is_empty()
        && proposals.iter().all(|entry| entry.valid)
        && packs.iter().all(|entry| entry.valid)
        && summaries.iter().all(|entry| entry.valid)
        && build_reports.iter().all(|entry| entry.verification_valid);
    let index_key = format!(
        "{}:{proposal_count}:{}:{}:{}:{}",
        root.display(),
        packs.len(),
        summaries.len(),
        build_reports.len(),
        files.join("|")
    );

    Ok(AgentArtifactIndex {
        index_id: stable_id("agent-artifacts", &index_key),
        root: root.display().to_string(),
        valid,
        proposal_directory_count: proposals.len(),
        proposal_count,
        pack_directory_count: packs.len(),
        summary_count: summaries.len(),
        build_report_count: build_reports.len(),
        verification_error_count: errors.len(),
        verification_warning_count: warnings.len(),
        proposals,
        packs,
        summaries,
        build_reports,
        errors,
        warnings,
        files,
    })
}

fn collect_agent_artifact_paths(
    root: &Path,
    proposal_dirs: &mut BTreeSet<PathBuf>,
    pack_dirs: &mut BTreeSet<PathBuf>,
    summary_files: &mut BTreeSet<PathBuf>,
    build_report_files: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    if root.is_file() {
        classify_agent_artifact_file(
            root,
            proposal_dirs,
            pack_dirs,
            summary_files,
            build_report_files,
        );
        return Ok(());
    }
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_agent_artifact_paths(
                &path,
                proposal_dirs,
                pack_dirs,
                summary_files,
                build_report_files,
            )?;
        } else {
            classify_agent_artifact_file(
                &path,
                proposal_dirs,
                pack_dirs,
                summary_files,
                build_report_files,
            );
        }
    }
    Ok(())
}

fn classify_agent_artifact_file(
    path: &Path,
    proposal_dirs: &mut BTreeSet<PathBuf>,
    pack_dirs: &mut BTreeSet<PathBuf>,
    summary_files: &mut BTreeSet<PathBuf>,
    build_report_files: &mut BTreeSet<PathBuf>,
) {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    match name {
        "agent-proposals-index.yaml" => {
            if let Some(parent) = path.parent() {
                proposal_dirs.insert(parent.to_path_buf());
            }
        }
        "agent-pack.yaml" | "agent-pack-manifest.yaml" => {
            if let Some(parent) = path.parent() {
                pack_dirs.insert(parent.to_path_buf());
            }
        }
        "agent-builder-summary.yaml" => {
            summary_files.insert(path.to_path_buf());
        }
        name if name.ends_with("build-report.yaml") || name == "agent-pack-build-report.yaml" => {
            build_report_files.insert(path.to_path_buf());
        }
        _ => {}
    }
}

fn agent_artifact_index_markdown(index: &AgentArtifactIndex) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Agent Artifact Index\n\n"));
    out.push_str(&format!("- Index ID: `{}`\n", index.index_id));
    out.push_str(&format!("- Root: `{}`\n", index.root));
    out.push_str(&format!("- Valid: {}\n", index.valid));
    out.push_str(&format!(
        "- Proposal directories: {}\n- Proposals: {}\n- Agent packs: {}\n- Summaries: {}\n- Build reports: {}\n\n",
        index.proposal_directory_count,
        index.proposal_count,
        index.pack_directory_count,
        index.summary_count,
        index.build_report_count
    ));
    if !index.errors.is_empty() {
        out.push_str("## Verification Errors\n");
        for err in &index.errors {
            out.push_str(&format!("- {err}\n"));
        }
        out.push('\n');
    }
    if !index.warnings.is_empty() {
        out.push_str("## Verification Warnings\n");
        for warning in &index.warnings {
            out.push_str(&format!("- {warning}\n"));
        }
        out.push('\n');
    }
    out.push_str("## Proposal Directories\n");
    for proposal in &index.proposals {
        out.push_str(&format!(
            "- `{}`: valid={}, proposals={}, ready_for_packaging={}, default_candidates={}\n",
            proposal.path,
            proposal.valid,
            proposal.proposal_count,
            proposal.ready_for_packaging_count,
            proposal.default_use_candidate_count
        ));
    }
    out.push_str("\n## Agent Packs\n");
    for pack in &index.packs {
        out.push_str(&format!(
            "- `{}`: valid={}, agent={}, runtime_ready={:?}, default_ready={:?}, selected_skills={:?}, tools={}\n",
            pack.path,
            pack.valid,
            pack.agent_name.as_deref().unwrap_or("unknown"),
            pack.ready_for_runtime_use,
            pack.ready_for_default_use,
            pack.selected_skill_count,
            pack.tool_permissions.join(", ")
        ));
    }
    out.push_str("\n## Summaries\n");
    for summary in &index.summaries {
        out.push_str(&format!(
            "- `{}`: valid={}, proposals={}, packs={}\n",
            summary.path, summary.valid, summary.proposal_count, summary.pack_count
        ));
    }
    out.push_str("\n## Build Reports\n");
    for report in &index.build_reports {
        out.push_str(&format!(
            "- `{}`: agent={}, verification_valid={}, runtime_ready={}, default_ready={}\n",
            report.path,
            report.agent_name,
            report.verification_valid,
            report.ready_for_runtime_use,
            report.ready_for_default_use
        ));
    }
    out.push_str("\n## Files\n");
    for file in &index.files {
        out.push_str(&format!("- `{file}`\n"));
    }
    out
}

fn agent_builder_summary_markdown(summary: &AgentBuilderSummary) -> String {
    let mut out = String::new();
    out.push_str("# Agent Builder Summary\n\n");
    out.push_str("## Summary\n\n");
    out.push_str(&format!("- Summary ID: {}\n", summary.summary_id));
    out.push_str(&format!("- Valid: {}\n", summary.valid));
    out.push_str(&format!("- Proposal count: {}\n", summary.proposal_count));
    out.push_str(&format!("- Pack count: {}\n", summary.pack_count));
    out.push_str(&format!(
        "- Ready for packaging proposals: {}\n",
        summary.ready_for_packaging_count
    ));
    out.push_str(&format!(
        "- Default-use proposal candidates: {}\n",
        summary.ready_for_default_use_candidate_count
    ));
    out.push_str(&format!(
        "- Runtime-ready packs: {}\n",
        summary.runtime_ready_pack_count
    ));
    out.push_str(&format!(
        "- Default-ready packs: {}\n\n",
        summary.default_ready_pack_count
    ));

    if !summary.verification_errors.is_empty() {
        out.push_str("## Verification Errors\n\n");
        for error in &summary.verification_errors {
            out.push_str(&format!("- {}\n", error));
        }
        out.push('\n');
    }
    if !summary.verification_warnings.is_empty() {
        out.push_str("## Verification Warnings\n\n");
        for warning in &summary.verification_warnings {
            out.push_str(&format!("- {}\n", warning));
        }
        out.push('\n');
    }

    out.push_str("## Proposals\n\n");
    for proposal in &summary.proposals {
        out.push_str(&format!("### {}\n\n", proposal.agent_name));
        out.push_str(&format!("- Agent ID: {}\n", proposal.agent_id));
        out.push_str(&format!("- File: {}\n", proposal.file));
        out.push_str(&format!(
            "- Selected skills: {}\n",
            proposal.selected_skill_count
        ));
        out.push_str(&format!(
            "- Reviewed skills: {}\n",
            proposal.reviewed_skill_count
        ));
        out.push_str(&format!(
            "- Ready for packaging: {}\n",
            proposal.ready_for_packaging
        ));
        out.push_str(&format!(
            "- Default-use candidate: {}\n",
            proposal.ready_for_default_use_candidate
        ));
        if !proposal.required_tools.is_empty() {
            out.push_str("- Required tools:\n");
            for tool in &proposal.required_tools {
                out.push_str(&format!("  - {}\n", tool));
            }
        }
        out.push('\n');
    }

    out.push_str("## Agent Packs\n\n");
    for pack in &summary.packs {
        out.push_str(&format!("### {}\n\n", pack.agent_name));
        out.push_str(&format!("- Pack ID: {}\n", pack.pack_id));
        out.push_str(&format!("- Path: {}\n", pack.path));
        out.push_str(&format!(
            "- Selected skills: {}\n",
            pack.selected_skill_count
        ));
        out.push_str(&format!(
            "- Required skills: {}\n",
            pack.required_skill_count
        ));
        out.push_str(&format!(
            "- Forbidden skills: {}\n",
            pack.forbidden_skill_count
        ));
        out.push_str(&format!(
            "- Runtime ready: {}\n",
            pack.ready_for_runtime_use
        ));
        out.push_str(&format!(
            "- Default ready: {}\n",
            pack.ready_for_default_use
        ));
        if !pack.tool_permissions.is_empty() {
            out.push_str("- Tool permissions:\n");
            for tool in &pack.tool_permissions {
                out.push_str(&format!("  - {}\n", tool));
            }
        }
        out.push('\n');
    }

    out.push_str("## Files\n\n");
    for file in &summary.files {
        out.push_str(&format!("- {}\n", file));
    }
    out
}

pub fn write_agent_pack(
    bundle: &SkillBundle,
    agent: &str,
    out: &Path,
    lifecycle_status_path: Option<&Path>,
) -> Result<()> {
    write_agent_pack_with_report(bundle, agent, out, lifecycle_status_path).map(|_| ())
}

pub fn write_agent_pack_with_report(
    bundle: &SkillBundle,
    agent: &str,
    out: &Path,
    lifecycle_status_path: Option<&Path>,
) -> Result<AgentPackBuildReport> {
    fs::create_dir_all(out)?;
    let lifecycle_status = lifecycle_status_path
        .map(read_agent_pack_lifecycle_status)
        .transpose()?;
    let selected_skill_ids = selected_skill_ids(bundle, agent);
    let selection_report = agent_pack_selection_report(bundle, agent, &selected_skill_ids);
    let selected_skill_id_set: BTreeSet<String> = selected_skill_ids.iter().cloned().collect();
    let selected_skills: Vec<&Skill> = bundle
        .skills
        .iter()
        .filter(|s| selected_skill_id_set.contains(&s.id) && !is_forbidden_skill(s))
        .collect();
    let eval_status = agent_pack_eval_status(&selected_skills);
    let pack_readiness = agent_pack_readiness_status(
        &selected_skills,
        &selection_report,
        &eval_status,
        lifecycle_status.as_ref(),
    );
    let pack = AgentPack {
        agent_name: agent,
        agent_version: "0.1.0",
        description: format!("Agent pack generated from {}", bundle.package.name),
        source_bundle_ids: vec![bundle.package.bundle_id.clone()],
        source_bundle_name: bundle.package.name.clone(),
        source_bundle_version: bundle.package.version.clone(),
        skill_ids: selected_skill_ids,
        required_skills: selected_skills
            .iter()
            .filter(|s| {
                matches!(
                    s.status,
                    SkillStatus::Reviewed | SkillStatus::Approved | SkillStatus::Published
                ) && s.maturity >= SkillMaturity::Level3Verified
            })
            .map(|s| s.id.clone())
            .collect(),
        optional_skills: selected_skills
            .iter()
            .filter(|s| matches!(s.status, SkillStatus::Candidate | SkillStatus::NeedsReview))
            .map(|s| s.id.clone())
            .collect(),
        forbidden_skills: bundle
            .skills
            .iter()
            .filter(|s| is_forbidden_skill(s))
            .map(|s| s.id.clone())
            .collect(),
        tool_permissions: selected_skills
            .iter()
            .flat_map(|s| {
                s.tool_requirements
                    .iter()
                    .map(|t| format!("{}:{:?}", t.name, t.permission_level))
            })
            .collect(),
        runtime_policy: "Read-only first; approval before mutation; cite evidence.".into(),
        context_policy: "Load route-selected skills plus dependencies within budget.".into(),
        memory_policy: "Store non-secret durable improvements only after review.".into(),
        approval_policy: "Require user approval for file or external mutation.".into(),
        review_status: bundle.package.review_status.clone(),
        evals: selected_skills
            .iter()
            .flat_map(|s| s.evals.iter().cloned())
            .take(25)
            .collect(),
        example_prompts: vec![format!("Ask {} to solve a source-grounded task.", agent)],
        ka: default_ka_profile_for_agent(agent),
        system_prompt_material: agent_system_prompt_material(
            bundle,
            agent,
            &selected_skills,
            &selection_report,
            &eval_status,
            &pack_readiness,
            &default_ka_profile_for_agent(agent),
        ),
        lifecycle_status,
        eval_status,
        selection_report,
        pack_readiness,
    };
    fs::write(out.join("agent-pack.yaml"), serde_yaml::to_string(&pack)?)?;
    let manifest = agent_pack_manifest_from_pack(&pack);
    fs::write(
        out.join("agent-pack-manifest.yaml"),
        serde_yaml::to_string(&manifest)?,
    )?;
    fs::write(
        out.join("agent-pack-manifest.md"),
        agent_pack_manifest_markdown(&manifest),
    )?;
    let verification = verify_agent_pack(out)?;
    Ok(agent_pack_build_report_from_pack(
        &pack,
        &manifest,
        &verification,
        out,
    ))
}

fn default_ka_profile_for_agent(agent: &str) -> KaProfile {
    let agent_lower = agent.to_ascii_lowercase();
    if agent_lower.contains("goblin") || agent_lower.contains("chaotic") {
        chaotic_competent_ka_profile()
    } else if agent_lower.contains("practical") || agent_lower.contains("engineer") {
        practical_engineer_ka_profile()
    } else {
        vegvisir_default_ka_profile()
    }
}

fn vegvisir_default_ka_profile() -> KaProfile {
    KaProfile {
        schema_version: 1,
        id: "vegvisir_default".to_string(),
        display_name: "Vegvisir Default".to_string(),
        summary: "Capable, direct, evidence-seeking, transparent, and steady; a pragmatic working partner with a lightly human delivery style and a disciplined operational spine.".to_string(),
        voice: KaVoice {
            warmth: "medium".to_string(),
            directness: "high".to_string(),
            humor: "low-medium".to_string(),
            formality: "medium-low".to_string(),
            theatricality: "low".to_string(),
            metaphor_density: "low".to_string(),
            avoid: vec![
                "corporate_fluff".to_string(),
                "fake_certainty".to_string(),
                "performative_apologies".to_string(),
                "burying_failures_or_risk_under_style".to_string(),
                "over_narrating_when_work_is_simple".to_string(),
            ],
        },
        temperament: KaTemperament {
            energy: "steady_capable".to_string(),
            patience: "high".to_string(),
            curiosity: "high_when_evidence_is_missing".to_string(),
            confidence_style: "evidence_based_and_assumption_explicit".to_string(),
        },
        work_style: KaWorkStyle {
            progress_updates: "concise_material_updates".to_string(),
            failure_style: "plain_english_recovery_summary_with_next_step".to_string(),
            uncertainty_style: "state_uncertainty_then_inspect_or_propose_verification".to_string(),
            collaboration_style: "pragmatic_agentic_working_partner".to_string(),
        },
        risk_modulation: KaRiskModulation {
            normal: "direct, calm, lightly warm, and action-oriented".to_string(),
            high_risk: "maximum precision; minimize personality; surface risk, approval needs, and reversible steps".to_string(),
            user_frustrated: "short, accountable, specific, and recovery-focused; no jokes or theatrics".to_string(),
        },
        boundaries: default_ka_boundaries(),
    }
}

fn practical_engineer_ka_profile() -> KaProfile {
    KaProfile {
        schema_version: 1,
        id: "practical_engineer".to_string(),
        display_name: "Practical Engineer".to_string(),
        summary: "Direct, technically serious, calm, and evidence-oriented.".to_string(),
        voice: KaVoice {
            warmth: "medium".to_string(),
            directness: "high".to_string(),
            humor: "low".to_string(),
            formality: "medium-low".to_string(),
            theatricality: "low".to_string(),
            metaphor_density: "low".to_string(),
            avoid: vec![
                "corporate_fluff".to_string(),
                "fake_certainty".to_string(),
                "burying_errors_under_style".to_string(),
            ],
        },
        temperament: KaTemperament {
            energy: "steady".to_string(),
            patience: "high".to_string(),
            curiosity: "medium-high".to_string(),
            confidence_style: "evidence_based".to_string(),
        },
        work_style: KaWorkStyle {
            progress_updates: "concise".to_string(),
            failure_style: "direct_recovery_summary".to_string(),
            uncertainty_style: "state_assumption_then_verify".to_string(),
            collaboration_style: "capable_working_partner".to_string(),
        },
        risk_modulation: KaRiskModulation {
            normal: "direct and calm".to_string(),
            high_risk: "maximum precision; reduce personality to near-zero".to_string(),
            user_frustrated: "short, clear, accountable, no theatrics".to_string(),
        },
        boundaries: default_ka_boundaries(),
    }
}

fn chaotic_competent_ka_profile() -> KaProfile {
    KaProfile {
        schema_version: 1,
        id: "chaotic_competent".to_string(),
        display_name: "Chaotic but Competent".to_string(),
        summary:
            "Playful, animated, and occasionally dramatic, with a disciplined operational spine."
                .to_string(),
        voice: KaVoice {
            warmth: "medium".to_string(),
            directness: "high".to_string(),
            humor: "high".to_string(),
            formality: "low".to_string(),
            theatricality: "medium-high".to_string(),
            metaphor_density: "medium".to_string(),
            avoid: vec![
                "hiding_failures_behind_jokes".to_string(),
                "unclear_commands".to_string(),
                "performative_chaos_that_changes_behavior".to_string(),
            ],
        },
        temperament: KaTemperament {
            energy: "high".to_string(),
            patience: "medium".to_string(),
            curiosity: "high".to_string(),
            confidence_style: "evidence_based_even_when_playful".to_string(),
        },
        work_style: KaWorkStyle {
            progress_updates: "brief_but_colorful".to_string(),
            failure_style: "direct_recovery_summary_before_any_jokes".to_string(),
            uncertainty_style: "say_the_guess_then_go_verify".to_string(),
            collaboration_style: "chaotic_good_teammate".to_string(),
        },
        risk_modulation: KaRiskModulation {
            normal: "playful and animated while preserving exact details".to_string(),
            high_risk: "low theatricality; precision and safety first".to_string(),
            user_frustrated: "drop the bit; be direct and useful".to_string(),
        },
        boundaries: default_ka_boundaries(),
    }
}

fn default_ka_boundaries() -> Vec<String> {
    vec![
        "ka_affects_delivery_only".to_string(),
        "ka_must_not_override_usrl".to_string(),
        "ka_must_not_change_tool_permissions".to_string(),
        "ka_must_not_reduce_verification".to_string(),
        "clarity_over_character".to_string(),
        "never_hide_errors_or_risk".to_string(),
        "exact_commands_paths_errors_and_test_results_remain_precise".to_string(),
    ]
}

fn render_ka_prompt_section(ka: &KaProfile) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Communication ka: `{}` — {}.\n",
        ka.id, ka.display_name
    ));
    out.push_str(&format!("Summary: {}\n\n", ka.summary));
    out.push_str("Ka/persona controls delivery style only. It is lower priority than system/developer/runtime instructions, the embedded USRL contract, operating rules, selected skill policies, tool policy, approval policy, secrets policy, and user authority. If ka conflicts with clarity, safety, evidence, or policy, ignore the ka and follow the higher-priority rule.\n\n");
    out.push_str("Voice profile:\n");
    out.push_str(&format!("- Warmth: {}\n", ka.voice.warmth));
    out.push_str(&format!("- Directness: {}\n", ka.voice.directness));
    out.push_str(&format!("- Humor: {}\n", ka.voice.humor));
    out.push_str(&format!("- Formality: {}\n", ka.voice.formality));
    out.push_str(&format!("- Theatricality: {}\n", ka.voice.theatricality));
    out.push_str(&format!(
        "- Metaphor density: {}\n",
        ka.voice.metaphor_density
    ));
    if !ka.voice.avoid.is_empty() {
        out.push_str("- Avoid: ");
        out.push_str(&ka.voice.avoid.join(", "));
        out.push('\n');
    }
    out.push_str("\nTemperament and work style:\n");
    out.push_str(&format!("- Energy: {}\n", ka.temperament.energy));
    out.push_str(&format!("- Patience: {}\n", ka.temperament.patience));
    out.push_str(&format!("- Curiosity: {}\n", ka.temperament.curiosity));
    out.push_str(&format!(
        "- Confidence style: {}\n",
        ka.temperament.confidence_style
    ));
    out.push_str(&format!(
        "- Progress updates: {}\n",
        ka.work_style.progress_updates
    ));
    out.push_str(&format!(
        "- Failure style: {}\n",
        ka.work_style.failure_style
    ));
    out.push_str(&format!(
        "- Uncertainty style: {}\n",
        ka.work_style.uncertainty_style
    ));
    out.push_str(&format!(
        "- Collaboration style: {}\n",
        ka.work_style.collaboration_style
    ));
    out.push_str("\nRisk modulation:\n");
    out.push_str(&format!("- Normal work: {}\n", ka.risk_modulation.normal));
    out.push_str(&format!(
        "- High-risk/secrets/security/destructive/production work: {}\n",
        ka.risk_modulation.high_risk
    ));
    out.push_str(&format!(
        "- User frustration/confusion: {}\n",
        ka.risk_modulation.user_frustrated
    ));
    out.push_str("\nKa boundaries:\n");
    for boundary in &ka.boundaries {
        out.push_str(&format!("- {}\n", boundary));
    }
    out
}

fn agent_pack_build_report_from_pack(
    pack: &AgentPack<'_>,
    manifest: &AgentPackManifest,
    verification: &AgentPackVerificationReport,
    out: &Path,
) -> AgentPackBuildReport {
    let omitted_skill_ids = pack
        .selection_report
        .omitted_skills
        .iter()
        .map(|selection| selection.skill_id.clone())
        .collect();
    AgentPackBuildReport {
        build_id: stable_id(
            "agent-pack-build",
            &format!(
                "{}:{}:{}:{}",
                manifest.pack_id,
                manifest.bundle_version,
                manifest.ready_for_runtime_use,
                manifest.ready_for_default_use
            ),
        ),
        agent_name: pack.agent_name.to_string(),
        bundle_id: manifest.bundle_id.clone(),
        bundle_name: manifest.bundle_name.clone(),
        bundle_version: manifest.bundle_version.clone(),
        out_dir: out.display().to_string(),
        pack_file: manifest.agent_pack_file.clone(),
        manifest_file: manifest.manifest_file.clone(),
        markdown_file: manifest.markdown_file.clone(),
        selected_skill_count: manifest.selected_skill_count,
        required_skill_count: manifest.required_skill_count,
        optional_skill_count: manifest.optional_skill_count,
        forbidden_skill_count: manifest.forbidden_skill_count,
        omitted_skill_count: pack.selection_report.omitted_skills.len(),
        tool_permission_count: manifest.tool_permission_count,
        eval_case_count: manifest.eval_case_count,
        ready_for_runtime_use: manifest.ready_for_runtime_use,
        ready_for_default_use: manifest.ready_for_default_use,
        lifecycle_ready: manifest.lifecycle_ready,
        evals_passed: manifest.evals_passed,
        verification_valid: verification.valid,
        blockers: manifest.blockers.clone(),
        warnings: manifest.warnings.clone(),
        selected_skill_ids: manifest.selected_skill_ids.clone(),
        required_skill_ids: manifest.required_skill_ids.clone(),
        optional_skill_ids: manifest.optional_skill_ids.clone(),
        forbidden_skill_ids: manifest.forbidden_skill_ids.clone(),
        omitted_skill_ids,
        tool_permissions: manifest.tool_permissions.clone(),
        verification_errors: verification.errors.clone(),
        verification_warnings: verification.warnings.clone(),
    }
}

fn agent_pack_manifest_from_pack(pack: &AgentPack<'_>) -> AgentPackManifest {
    let bundle_id = pack.source_bundle_ids.first().cloned().unwrap_or_default();
    let pack_id = stable_id(
        "agent-pack",
        &format!(
            "{}:{}:{}:{}",
            bundle_id,
            pack.agent_version,
            pack.agent_name,
            pack.skill_ids.join(",")
        ),
    );
    AgentPackManifest {
        pack_id,
        agent_name: pack.agent_name.to_string(),
        agent_version: pack.agent_version.to_string(),
        bundle_id,
        bundle_name: pack.source_bundle_name.clone(),
        bundle_version: pack.source_bundle_version.clone(),
        agent_pack_file: "agent-pack.yaml".into(),
        manifest_file: "agent-pack-manifest.yaml".into(),
        markdown_file: "agent-pack-manifest.md".into(),
        selected_skill_count: pack.skill_ids.len(),
        required_skill_count: pack.required_skills.len(),
        optional_skill_count: pack.optional_skills.len(),
        forbidden_skill_count: pack.forbidden_skills.len(),
        tool_permission_count: pack.tool_permissions.len(),
        eval_case_count: pack.evals.len(),
        ready_for_runtime_use: pack.pack_readiness.ready_for_runtime_use,
        ready_for_default_use: pack.pack_readiness.ready_for_default_use,
        lifecycle_ready: pack.pack_readiness.lifecycle_ready,
        evals_passed: pack.eval_status.passed,
        blockers: pack.pack_readiness.blockers.clone(),
        warnings: pack.pack_readiness.warnings.clone(),
        selected_skill_ids: pack.skill_ids.clone(),
        required_skill_ids: pack.required_skills.clone(),
        optional_skill_ids: pack.optional_skills.clone(),
        forbidden_skill_ids: pack.forbidden_skills.clone(),
        tool_permissions: pack.tool_permissions.clone(),
        files: vec![
            "agent-pack.yaml".into(),
            "agent-pack-manifest.yaml".into(),
            "agent-pack-manifest.md".into(),
        ],
    }
}

fn agent_pack_manifest_markdown(manifest: &AgentPackManifest) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Agent Pack Manifest: {}\n\n",
        manifest.agent_name
    ));
    out.push_str("## Summary\n\n");
    out.push_str(&format!("- Pack ID: {}\n", manifest.pack_id));
    out.push_str(&format!(
        "- Bundle: {} ({})\n",
        manifest.bundle_name, manifest.bundle_id
    ));
    out.push_str(&format!("- Bundle version: {}\n", manifest.bundle_version));
    out.push_str(&format!(
        "- Selected skills: {}\n",
        manifest.selected_skill_count
    ));
    out.push_str(&format!(
        "- Required skills: {}\n",
        manifest.required_skill_count
    ));
    out.push_str(&format!(
        "- Optional skills: {}\n",
        manifest.optional_skill_count
    ));
    out.push_str(&format!(
        "- Forbidden skills: {}\n",
        manifest.forbidden_skill_count
    ));
    out.push_str(&format!(
        "- Tool permissions: {}\n",
        manifest.tool_permission_count
    ));
    out.push_str(&format!("- Eval cases: {}\n", manifest.eval_case_count));
    out.push_str(&format!(
        "- Runtime ready: {}\n",
        manifest.ready_for_runtime_use
    ));
    out.push_str(&format!(
        "- Default-use ready: {}\n",
        manifest.ready_for_default_use
    ));
    if let Some(lifecycle_ready) = manifest.lifecycle_ready {
        out.push_str(&format!("- Lifecycle ready: {}\n", lifecycle_ready));
    }
    out.push_str(&format!("- Evals passed: {}\n", manifest.evals_passed));

    out.push_str("\n## Skills\n\n");
    out.push_str("### Required\n");
    if manifest.required_skill_ids.is_empty() {
        out.push_str("- None\n");
    } else {
        for skill_id in &manifest.required_skill_ids {
            out.push_str(&format!("- {}\n", skill_id));
        }
    }
    out.push_str("\n### Optional\n");
    if manifest.optional_skill_ids.is_empty() {
        out.push_str("- None\n");
    } else {
        for skill_id in &manifest.optional_skill_ids {
            out.push_str(&format!("- {}\n", skill_id));
        }
    }
    out.push_str("\n### Forbidden\n");
    if manifest.forbidden_skill_ids.is_empty() {
        out.push_str("- None\n");
    } else {
        for skill_id in &manifest.forbidden_skill_ids {
            out.push_str(&format!("- {}\n", skill_id));
        }
    }

    out.push_str("\n## Tool Permissions\n\n");
    if manifest.tool_permissions.is_empty() {
        out.push_str("- None\n");
    } else {
        for tool in &manifest.tool_permissions {
            out.push_str(&format!("- {}\n", tool));
        }
    }

    out.push_str("\n## Blockers\n\n");
    if manifest.blockers.is_empty() {
        out.push_str("- None\n");
    } else {
        for blocker in &manifest.blockers {
            out.push_str(&format!("- {}\n", blocker));
        }
    }

    out.push_str("\n## Warnings\n\n");
    if manifest.warnings.is_empty() {
        out.push_str("- None\n");
    } else {
        for warning in &manifest.warnings {
            out.push_str(&format!("- {}\n", warning));
        }
    }

    out.push_str("\n## Files\n\n");
    for file in &manifest.files {
        out.push_str(&format!("- {}\n", file));
    }
    out
}

fn agent_system_prompt_material(
    bundle: &SkillBundle,
    agent: &str,
    selected_skills: &[&Skill],
    selection_report: &AgentPackSelectionReport,
    eval_status: &AgentPackEvalStatus,
    pack_readiness: &AgentPackReadinessStatus,
    ka: &KaProfile,
) -> String {
    let agent_id = stable_agent_id(&bundle.package.bundle_id, agent);
    let contract_name = usrl_identifier(&format!("{}AgentContract", agent));
    let subject = slugify(agent);
    let domain = bundle
        .package
        .domain
        .as_deref()
        .unwrap_or("general-technical");
    let selected_skill_ids = selected_skills
        .iter()
        .map(|skill| skill.id.clone())
        .collect::<Vec<_>>();
    let domain_list = selected_skills
        .iter()
        .filter_map(|skill| skill.domain.as_deref())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let tool_permissions = selected_skills
        .iter()
        .flat_map(|skill| {
            skill.tool_requirements.iter().map(|tool| {
                format!(
                    "{}:{:?}:{:?}",
                    tool.name, tool.permission_level, tool.requirement_type
                )
            })
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut permits = BTreeSet::from([
        "load_selected_skiller_skills".to_string(),
        "answer_with_source_citations".to_string(),
        "reason_from_selected_bundle_evidence".to_string(),
        "ask_clarifying_questions_when_scope_is_ambiguous".to_string(),
        "produce_actionable_plans".to_string(),
    ]);
    if selected_skills
        .iter()
        .any(|skill| skill.runtime_policy.recommend_commands)
    {
        permits.insert("recommend_documented_commands".to_string());
    }
    if selected_skills
        .iter()
        .any(|skill| skill.runtime_policy.run_read_only_commands)
    {
        permits.insert("run_read_only_commands_when_runtime_allows".to_string());
    }
    if selected_skills
        .iter()
        .any(|skill| skill.runtime_policy.modify_files)
    {
        permits.insert("modify_project_files_after_approval_when_required".to_string());
    }
    if selected_skills
        .iter()
        .any(|skill| skill.runtime_policy.modify_external_systems)
    {
        permits.insert("modify_external_systems_only_after_explicit_approval".to_string());
    }
    if selected_skills
        .iter()
        .any(|skill| skill.runtime_policy.handles_secrets)
    {
        permits.insert("use_secret_refs_without_exposing_plaintext".to_string());
    }

    let high_risk = selected_skills
        .iter()
        .any(|skill| is_high_risk_agent_skill(skill));
    let can_mutate_files = selected_skills
        .iter()
        .any(|skill| skill.runtime_policy.modify_files);
    let can_mutate_external = selected_skills
        .iter()
        .any(|skill| skill.runtime_policy.modify_external_systems);
    let can_handle_secrets = selected_skills
        .iter()
        .any(|skill| skill.runtime_policy.handles_secrets);

    let mut out = String::new();
    out.push_str(&format!("You are {agent}, a specialized Vegvisir agent created by Skiller from the `{}` skill bundle.\n\n", bundle.package.name));
    out.push_str("# Mission\n\n");
    out.push_str(&format!(
        "Your job is to use selected, source-grounded Skiller skills for `{domain}` work. You are not a generic chatbot: you are an operational Vegvisir agent with explicit runtime boundaries, evidence duties, memory/secrets discipline, approval gates, and verification habits.\n\n"
    ));
    out.push_str("# Operating rules\n\n");
    out.push_str("1. Treat the user as the authority. Follow the latest user instruction unless it conflicts with safety, credential, integrity, runtime, or contract boundaries.\n");
    out.push_str("2. Use Skiller skills as governed context. Prefer selected skills and their cited source sections over unsupported prior knowledge.\n");
    out.push_str("3. Be capable and direct. When the active runtime gives you tools and permissions, do the work rather than only describing it.\n");
    out.push_str("4. Preserve source grounding. Cite skill IDs, source sections, commands, APIs, or documents when making strong technical claims.\n");
    out.push_str("5. Respect the selected skill set. If a task needs an unselected, forbidden, unsafe, deprecated, or missing skill, say so and ask for routing/approval instead of improvising.\n");
    out.push_str("6. Preserve user work. Do not overwrite, delete, revert, or discard unrelated local changes without explicit instruction.\n");
    out.push_str("7. Keep secrets behind the runtime secret boundary. Never ask for, echo, store, or place plaintext credentials in memory or artifacts. Use secret refs when credentials are needed.\n");
    out.push_str("8. Use memory only for useful non-secret facts, decisions, project continuity, and reviewed improvements. Do not store secrets or secret-bearing URLs.\n");
    out.push_str("9. Use tools proactively when permitted and useful. Inspect files, run checks, query skill context, and verify claims instead of guessing.\n");
    out.push_str("10. Request approval when runtime policy, skill policy, or user risk requires it. Use dry-runs, backups, or rollback plans for consequential mutations.\n");
    out.push_str("11. For commands, prefer read-only diagnostics first. Explain mutating commands before execution unless the user has already authorized them and policy allows.\n");
    out.push_str("12. For external systems, stay scoped to user-authorized targets and avoid stealth, persistence, credential theft, destructive actions, or unauthorized third-party access.\n");
    out.push_str("13. Be honest about uncertainty, omitted context, failed tools, unavailable skills, and skipped verification.\n");
    out.push_str("14. Keep responses concise by default, but provide detailed plans, traces, diffs, or reports when task risk or complexity warrants it.\n");
    out.push_str("15. When modifying artifacts, match existing architecture, naming, style, dependency policy, and test strategy unless the user asks for redesign.\n\n");

    out.push_str("# Runtime posture\n\n");
    out.push_str("- Mode: specialized Vegvisir agent generated from a Skiller agent pack.\n");
    out.push_str("- Skill source: governed Skiller bundle, selected skill IDs, evals, guardrails, and citations.\n");
    out.push_str("- Default action style: orient, plan, execute with tools when allowed, verify, then report.\n");
    out.push_str("- Evidence posture: source-grounded; do not invent APIs, flags, versions, policies, or source claims.\n");
    out.push_str("- Memory posture: useful non-secret continuity only.\n");
    out.push_str("- Secrets posture: secret-ref-only; no plaintext secrets.\n");
    out.push_str(&format!("- File mutation posture: {}.\n", if can_mutate_files { "allowed only within runtime approvals and selected skill policy" } else { "not allowed by default; propose patches/plans unless explicitly rebound by runtime policy" }));
    out.push_str(&format!(
        "- External mutation posture: {}.\n",
        if can_mutate_external {
            "allowed only after explicit approval and skill-policy satisfaction"
        } else {
            "not allowed by default"
        }
    ));
    out.push_str(&format!("- High-risk posture: {}.\n\n", if high_risk { "selected skills include high-risk capabilities; keep actions opt-in, review-heavy, and approval-gated" } else { "normal defensive/engineering posture; still approval-gate consequential actions" }));

    out.push_str("# Selected skill context\n\n");
    out.push_str(&format!("- Bundle ID: `{}`\n", bundle.package.bundle_id));
    out.push_str(&format!("- Bundle version: `{}`\n", bundle.package.version));
    out.push_str(&format!("- Agent ID: `{agent_id}`\n"));
    out.push_str(&format!(
        "- Selected skill count: {}\n",
        selected_skills.len()
    ));
    out.push_str(&format!(
        "- Required skill count: {}\n",
        selection_report.required_skill_count
    ));
    out.push_str(&format!(
        "- Optional skill count: {}\n",
        selection_report.optional_skill_count
    ));
    out.push_str(&format!(
        "- Forbidden skill count: {}\n",
        selection_report.forbidden_skill_count
    ));
    if !domain_list.is_empty() {
        out.push_str(&format!("- Skill domains: {}\n", domain_list.join(", ")));
    }
    out.push_str("\n## Skills\n\n");
    for skill in selected_skills.iter().take(20) {
        out.push_str(&format!("- `{}` — {}\n", skill.id, skill.title));
        if !skill.summary.trim().is_empty() {
            out.push_str(&format!(
                "  - Summary: {}\n",
                compact_line(&skill.summary, 220)
            ));
        }
        if !skill.guardrails.is_empty() {
            out.push_str("  - Guardrails:\n");
            for guardrail in skill.guardrails.iter().take(3) {
                out.push_str(&format!("    - {}\n", compact_line(guardrail, 180)));
            }
        }
        if !skill.citations.is_empty() {
            let citations = skill
                .citations
                .iter()
                .take(3)
                .map(|citation| citation.section_id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("  - Citation sections: {citations}\n"));
        }
    }
    if selected_skills.len() > 20 {
        out.push_str(&format!(
            "- ... {} additional selected skills omitted from prompt summary; load on demand.\n",
            selected_skills.len() - 20
        ));
    }
    if !tool_permissions.is_empty() {
        out.push_str("\n## Tool permissions from selected skills\n\n");
        for permission in &tool_permissions {
            out.push_str(&format!("- `{permission}`\n"));
        }
    }
    out.push('\n');

    out.push_str("# Communication ka\n\n");
    out.push_str(&render_ka_prompt_section(ka));
    out.push_str("\n");

    out.push_str("# Verification and reporting\n\n");
    out.push_str("- Before strong claims: inspect relevant skill/source/tool evidence.\n");
    out.push_str(
        "- Before completing implementation tasks: run focused tests/checks when practical.\n",
    );
    out.push_str("- If verification is skipped: say why and provide the smallest next check.\n");
    out.push_str("- Report important file writes, commands, approvals, failures, retries, and test results.\n\n");

    out.push_str("# Embedded USRL contract\n\n");
    out.push_str("```usrl\n");
    out.push_str(&format!("contract {contract_name} {{\n"));
    out.push_str("  section Metadata {\n");
    out.push_str(&format!(
        "    fact ContractId = \"{}\";\n",
        escape_usrl_string(&format!("skiller_agent_contract_{subject}"))
    ));
    out.push_str(&format!(
        "    fact Title = \"{} Runtime Contract\";\n",
        escape_usrl_string(agent)
    ));
    out.push_str(&format!(
        "    fact Subject = \"{}\";\n",
        escape_usrl_string(&subject)
    ));
    out.push_str("    fact Owner = \"user\";\n");
    out.push_str("    fact Scope = [\n");
    out.push_str(&format!("      \"{}\",\n", escape_usrl_string(domain)));
    out.push_str("      \"skiller-generated-agent\",\n");
    out.push_str("      \"source-grounded-skill-use\",\n");
    out.push_str("      \"vegvisir-runtime\"\n");
    out.push_str("    ];\n");
    out.push_str("  }\n\n");
    out.push_str("  section RuntimeFacts {\n");
    out.push_str("    fact Runtime = \"Vegvisir harness\";\n");
    out.push_str("    fact SkillSystem = \"Skiller\";\n");
    out.push_str(&format!(
        "    fact BundleId = \"{}\";\n",
        escape_usrl_string(&bundle.package.bundle_id)
    ));
    out.push_str(&format!(
        "    fact BundleVersion = \"{}\";\n",
        escape_usrl_string(&bundle.package.version)
    ));
    out.push_str(&format!(
        "    fact AgentId = \"{}\";\n",
        escape_usrl_string(&agent_id)
    ));
    out.push_str(&format!(
        "    fact SelectedSkillCount = {};\n",
        selected_skills.len()
    ));
    out.push_str(&format!(
        "    fact EvalCases = {};\n",
        eval_status.total_eval_cases
    ));
    out.push_str(&format!(
        "    fact RuntimeReady = {};\n",
        pack_readiness.ready_for_runtime_use
    ));
    out.push_str(&format!(
        "    fact DefaultUseReady = {};\n",
        pack_readiness.ready_for_default_use
    ));
    out.push_str("    fact CredentialVisibility = \"secret-ref-only\";\n");
    out.push_str("    fact MemoryPolicy = \"non-secret-reviewed-continuity\";\n");
    out.push_str("  }\n\n");
    out.push_str("  section SelectedSkills {\n");
    out.push_str("    fact SkillIds = [\n");
    for skill_id in selected_skill_ids.iter().take(50) {
        out.push_str(&format!("      \"{}\",\n", escape_usrl_string(skill_id)));
    }
    out.push_str("    ];\n");
    out.push_str("  }\n\n");
    out.push_str("  section Permissions {\n");
    for permit in &permits {
        out.push_str(&format!("    permit \"{}\";\n", escape_usrl_string(permit)));
    }
    out.push_str("    permit \"request_runtime_approval_when_required\";\n");
    out.push_str("  }\n\n");
    out.push_str("  section Constraints {\n");
    out.push_str("    constraint UserAuthority { require \"follow_latest_user_instruction\"; }\n");
    out.push_str("    constraint SourceGrounding {\n");
    out.push_str("      require \"prefer_selected_skills_and_citations\";\n");
    out.push_str("      deny \"invent_unsupported_source_claims\";\n");
    out.push_str("      deny \"invent_undocumented_commands_apis_or_versions\";\n");
    out.push_str("    }\n");
    out.push_str("    constraint SkillBoundary {\n");
    out.push_str("      require \"stay_within_selected_skill_policy\";\n");
    out.push_str(
        "      deny \"use_forbidden_unsafe_deprecated_or_archived_skills_as_authority\";\n",
    );
    out.push_str("    }\n");
    out.push_str("    constraint NoPlaintextSecrets {\n");
    out.push_str("      deny \"request_plaintext_credentials\";\n");
    out.push_str("      deny \"echo_plaintext_credentials\";\n");
    out.push_str("      deny \"store_plaintext_credentials\";\n");
    out.push_str("    }\n");
    out.push_str("    constraint PreserveUserWork {\n");
    out.push_str("      deny \"revert_unrelated_user_changes_without_explicit_request\";\n");
    out.push_str("      deny \"delete_unrelated_user_files_without_explicit_request\";\n");
    out.push_str("    }\n");
    out.push_str("    constraint ApprovalForRisk { require \"approval_when_runtime_or_skill_policy_requires\"; }\n");
    out.push_str(
        "    constraint HonestEvidence { require \"state_uncertainty_when_not_verified\"; }\n",
    );
    if can_handle_secrets {
        out.push_str(
            "    constraint HbseCredentialBoundary { require \"secret_ref_for_credentials\"; }\n",
        );
    }
    if high_risk || can_mutate_external {
        out.push_str("    constraint AuthorizedScope {\n");
        out.push_str("      deny \"credential_theft\";\n");
        out.push_str("      deny \"malware_persistence\";\n");
        out.push_str("      deny \"stealth_evasion_for_real_world_abuse\";\n");
        out.push_str("      deny \"unauthorized_third_party_targeting\";\n");
        out.push_str("    }\n");
    }
    out.push_str("  }\n\n");
    out.push_str("  section Stages {\n");
    out.push_str("    stage Orient { fact Goal = \"understand user intent, selected skills, evidence, workspace, tools, and risks\"; }\n");
    out.push_str("    stage Plan { fact Goal = \"choose a bounded skill-grounded path and identify approvals or checks\"; }\n");
    out.push_str("    stage Execute { fact Goal = \"use permitted tools and skills to complete the task coherently\"; }\n");
    out.push_str("    stage Verify { fact Goal = \"run focused checks or explain skipped verification\"; }\n");
    out.push_str("    stage Report { fact Goal = \"summarize actions, evidence, verification, and remaining risks\"; }\n");
    out.push_str("  }\n\n");
    out.push_str("  section Triggers {\n");
    out.push_str("    trigger UserRequestsSkillTask { permit \"load_selected_skills\"; permit \"answer_or_act_with_citations\"; }\n");
    out.push_str("    trigger ToolUseNeeded { require \"selected_skill_or_runtime_permission\"; require \"approval_if_required\"; }\n");
    out.push_str(
        "    trigger SecretNeeded { require \"secret_ref\"; deny \"plaintext_secret_request\"; }\n",
    );
    out.push_str(
        "    trigger EvidenceMissing { require \"state_gap_and_request_context_or_review\"; }\n",
    );
    out.push_str("  }\n");
    out.push_str("}\n");
    out.push_str("```\n\n");

    if !pack_readiness.blockers.is_empty() || !pack_readiness.warnings.is_empty() {
        out.push_str("# Pack readiness notes\n\n");
        if !pack_readiness.blockers.is_empty() {
            out.push_str("## Blockers\n");
            for blocker in &pack_readiness.blockers {
                out.push_str(&format!("- {}\n", blocker));
            }
        }
        if !pack_readiness.warnings.is_empty() {
            out.push_str("## Warnings\n");
            for warning in &pack_readiness.warnings {
                out.push_str(&format!("- {}\n", warning));
            }
        }
        out.push('\n');
    }

    out.push_str("# Communication style\n\n");
    out.push_str("Pragmatic, direct, technically serious, evidence-seeking, and user-directed. Do not become timid or generic; use the selected skills and runtime tools to move the task forward.\n");
    out
}

fn usrl_identifier(value: &str) -> String {
    let mut out = String::new();
    let mut capitalize_next = true;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if out.is_empty() && ch.is_ascii_digit() {
                out.push('A');
            }
            if capitalize_next {
                out.push(ch.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                out.push(ch);
            }
        } else {
            capitalize_next = true;
        }
    }
    if out.is_empty() {
        "SkillerAgentContract".to_string()
    } else {
        out
    }
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "skiller-agent".to_string()
    } else {
        out
    }
}

fn escape_usrl_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn compact_line(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        compact
    } else {
        let mut truncated = compact
            .chars()
            .take(max_chars.saturating_sub(1))
            .collect::<String>();
        truncated.push('…');
        truncated
    }
}

fn agent_pack_readiness_status(
    selected_skills: &[&Skill],
    selection_report: &AgentPackSelectionReport,
    eval_status: &AgentPackEvalStatus,
    lifecycle_status: Option<&AgentPackLifecycleStatus>,
) -> AgentPackReadinessStatus {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    if selected_skills.is_empty() {
        blockers.push("agent pack selects no usable skills".to_string());
    }
    if selection_report.required_skill_count == 0 {
        blockers.push("agent pack has no reviewed/verified required skills".to_string());
    }
    if !eval_status.passed {
        blockers.extend(
            eval_status
                .failures
                .iter()
                .map(|failure| format!("eval readiness failure: {failure}")),
        );
    }
    warnings.extend(
        eval_status
            .warnings
            .iter()
            .map(|warning| format!("eval readiness warning: {warning}")),
    );

    let high_risk_selected_skill_count = selected_skills
        .iter()
        .filter(|skill| is_high_risk_agent_skill(skill))
        .count();
    for skill in selected_skills {
        if is_high_risk_agent_skill(skill) {
            if !matches!(skill.status, SkillStatus::Approved | SkillStatus::Published)
                || skill.maturity < SkillMaturity::Level4HumanApproved
                || skill.confidence.human_review < 0.8
            {
                blockers.push(format!(
                    "{}: high-risk selected skill requires Approved/Published status, Level4 human approval, and human_review confidence >= 0.8",
                    skill.id
                ));
            }
        }
        if skill
            .inference_records
            .iter()
            .any(|record| record.required_review)
        {
            warnings.push(format!(
                "{}: selected skill has inference records requiring review",
                skill.id
            ));
        }
    }

    if let Some(status) = lifecycle_status {
        if !status.lifecycle_ready {
            blockers.push(format!("lifecycle status {} is not ready", status.plan_id));
        }
        if status.human_review_required {
            blockers.push(format!(
                "lifecycle status {} requires human review",
                status.plan_id
            ));
        }
        blockers.extend(
            status
                .blockers
                .iter()
                .map(|blocker| format!("lifecycle blocker: {blocker}")),
        );
        warnings.extend(
            status
                .warnings
                .iter()
                .map(|warning| format!("lifecycle warning: {warning}")),
        );
    }

    let ready_for_runtime_use = blockers.is_empty();
    let ready_for_default_use = ready_for_runtime_use
        && selection_report.optional_skill_count == 0
        && high_risk_selected_skill_count == 0
        && eval_status.routing_eval_count > 0
        && eval_status.source_grounding_eval_count > 0;
    if ready_for_runtime_use && !ready_for_default_use {
        if selection_report.optional_skill_count > 0 {
            warnings.push("agent pack contains optional/unreviewed selected skills".to_string());
        }
        if high_risk_selected_skill_count > 0 {
            warnings.push(
                "agent pack contains high-risk selected skills; keep opt-in even when approved"
                    .to_string(),
            );
        }
        if eval_status.routing_eval_count == 0 {
            warnings.push("agent pack lacks routing eval coverage".to_string());
        }
        if eval_status.source_grounding_eval_count == 0 {
            warnings.push("agent pack lacks source-grounding eval coverage".to_string());
        }
    }

    AgentPackReadinessStatus {
        ready_for_default_use,
        ready_for_runtime_use,
        selected_skill_count: selected_skills.len(),
        required_skill_count: selection_report.required_skill_count,
        optional_skill_count: selection_report.optional_skill_count,
        forbidden_skill_count: selection_report.forbidden_skill_count,
        high_risk_selected_skill_count,
        lifecycle_ready: lifecycle_status.map(|status| status.lifecycle_ready),
        evals_passed: eval_status.passed,
        blockers,
        warnings,
    }
}

fn is_high_risk_agent_skill(skill: &Skill) -> bool {
    skill.runtime_policy.modify_external_systems
        || skill.tool_requirements.iter().any(|tool| {
            matches!(
                tool.permission_level,
                PermissionLevel::ExternalMutation | PermissionLevel::Dangerous
            ) || matches!(tool.requirement_type, ToolRequirementType::Dangerous)
        })
}

fn agent_pack_eval_status(selected_skills: &[&Skill]) -> AgentPackEvalStatus {
    let mut failures = Vec::new();
    let mut warnings = Vec::new();
    let mut skill_eval_counts = BTreeMap::new();
    let mut total_eval_cases = 0usize;
    let mut skills_without_evals = 0usize;
    let mut safety_eval_count = 0usize;
    let mut routing_eval_count = 0usize;
    let mut source_grounding_eval_count = 0usize;
    let mut tool_use_planning_eval_count = 0usize;

    for skill in selected_skills {
        skill_eval_counts.insert(skill.id.clone(), skill.evals.len());
        total_eval_cases += skill.evals.len();
        if skill.evals.is_empty() {
            skills_without_evals += 1;
            failures.push(format!("{}: skill has no eval cases", skill.id));
        }

        let mut has_safety = false;
        let mut has_routing = false;
        let mut has_source_grounding = false;
        let mut has_tool_use_planning = false;
        let mut seen_eval_ids = BTreeSet::new();
        for eval in &skill.evals {
            if !seen_eval_ids.insert(eval.id.as_str()) {
                failures.push(format!("{}: duplicate eval id {}", skill.id, eval.id));
            }
            if eval.prompt.trim().is_empty() {
                failures.push(format!("{}: eval {} has empty prompt", skill.id, eval.id));
            }
            if eval.expected_behavior.trim().is_empty() {
                failures.push(format!(
                    "{}: eval {} has empty expected_behavior",
                    skill.id, eval.id
                ));
            }
            match eval.eval_type {
                EvalType::Safety => {
                    has_safety = true;
                    safety_eval_count += 1;
                }
                EvalType::Routing => {
                    has_routing = true;
                    routing_eval_count += 1;
                }
                EvalType::SourceGrounding => {
                    has_source_grounding = true;
                    source_grounding_eval_count += 1;
                }
                EvalType::ToolUsePlanning => {
                    has_tool_use_planning = true;
                    tool_use_planning_eval_count += 1;
                }
                EvalType::Positive | EvalType::Negative | EvalType::EdgeCase => {}
            }
        }

        let has_tool_requirements = !skill.tool_requirements.is_empty();
        let high_risk_tool = skill.tool_requirements.iter().any(|tool| {
            matches!(
                tool.permission_level,
                PermissionLevel::ExternalMutation | PermissionLevel::Dangerous
            ) || matches!(tool.requirement_type, ToolRequirementType::Dangerous)
        });
        let operational = skill.runtime_policy.modify_files
            || skill.runtime_policy.modify_external_systems
            || has_tool_requirements;

        if !has_routing {
            warnings.push(format!("{}: missing routing eval", skill.id));
        }
        if !has_source_grounding {
            warnings.push(format!("{}: missing source-grounding eval", skill.id));
        }
        if operational && !has_tool_use_planning {
            warnings.push(format!(
                "{}: operational skill missing tool-use-planning eval",
                skill.id
            ));
        }
        if (high_risk_tool || skill.runtime_policy.modify_external_systems) && !has_safety {
            failures.push(format!("{}: high-risk skill missing safety eval", skill.id));
        }
        if matches!(skill.status, SkillStatus::Approved | SkillStatus::Published)
            && (!has_source_grounding || !has_routing)
        {
            failures.push(format!(
                "{}: approved/published skill must include routing and source-grounding evals",
                skill.id
            ));
        }
    }

    AgentPackEvalStatus {
        passed: failures.is_empty(),
        selected_skill_count: selected_skills.len(),
        total_eval_cases,
        skills_without_evals,
        safety_eval_count,
        routing_eval_count,
        source_grounding_eval_count,
        tool_use_planning_eval_count,
        failures,
        warnings,
        skill_eval_counts,
    }
}

fn read_agent_pack_lifecycle_status(path: &Path) -> Result<AgentPackLifecycleStatus> {
    let value: serde_yaml::Value = serde_yaml::from_str(
        &fs::read_to_string(path)
            .with_context(|| format!("failed to read lifecycle status {}", path.display()))?,
    )
    .with_context(|| format!("failed to parse lifecycle status {}", path.display()))?;
    Ok(AgentPackLifecycleStatus {
        plan_id: value
            .get("plan_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        lifecycle_ready: value
            .get("lifecycle_ready")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        human_review_required: value
            .get("human_review_required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        blockers: yaml_string_vec(value.get("blockers")),
        warnings: yaml_string_vec(value.get("warnings")),
    })
}

fn yaml_string_vec(value: Option<&serde_yaml::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_sequence())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn selected_skill_ids(bundle: &SkillBundle, agent: &str) -> Vec<String> {
    ranked_skill_selections_for_role(bundle, agent, 10)
        .into_iter()
        .map(|selection| selection.skill_id)
        .collect()
}

fn ranked_skill_selections_for_role(
    bundle: &SkillBundle,
    role: &str,
    limit: usize,
) -> Vec<AgentSkillSelection> {
    let eligible = proposal_eligible_skills(bundle);
    let mut scored: Vec<AgentSkillSelection> = eligible
        .iter()
        .filter_map(|skill| {
            let selection = role_match_selection(skill, role);
            if selection.score > 0 {
                Some(selection)
            } else {
                None
            }
        })
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.skill_id.cmp(&b.skill_id))
    });
    let mut selections: Vec<AgentSkillSelection> = scored.into_iter().take(limit).collect();
    if selections.is_empty() {
        selections = eligible
            .iter()
            .filter(|s| !matches!(s.scope, SkillScope::DomainLevel | SkillScope::RoleLevel))
            .take(limit)
            .map(|skill| AgentSkillSelection {
                skill_id: skill.id.clone(),
                title: skill.title.clone(),
                score: 1,
                reasons: vec!["fallback selection because no role/content match was found".into()],
                status: Some(skill.status.clone()),
                maturity: Some(skill.maturity.clone()),
            })
            .collect();
    }
    selections
}

fn agent_pack_selection_report(
    bundle: &SkillBundle,
    agent: &str,
    selected_skill_ids: &[String],
) -> AgentPackSelectionReport {
    let selected_set: BTreeSet<String> = selected_skill_ids.iter().cloned().collect();
    let selections = ranked_skill_selections_for_role(bundle, agent, 10);
    let required_skill_count = bundle
        .skills
        .iter()
        .filter(|s| {
            selected_set.contains(&s.id)
                && matches!(
                    s.status,
                    SkillStatus::Reviewed | SkillStatus::Approved | SkillStatus::Published
                )
                && s.maturity >= SkillMaturity::Level3Verified
                && !is_forbidden_skill(s)
        })
        .count();
    let optional_skill_count = bundle
        .skills
        .iter()
        .filter(|s| {
            selected_set.contains(&s.id)
                && matches!(s.status, SkillStatus::Candidate | SkillStatus::NeedsReview)
                && !is_forbidden_skill(s)
        })
        .count();
    let forbidden_skill_count = bundle
        .skills
        .iter()
        .filter(|s| is_forbidden_skill(s))
        .count();
    let omitted_skills = bundle
        .skills
        .iter()
        .filter(|s| !selected_set.contains(&s.id))
        .map(|skill| {
            if is_forbidden_skill(skill) {
                AgentSkillSelection {
                    skill_id: skill.id.clone(),
                    title: skill.title.clone(),
                    score: 0,
                    reasons: vec![format!(
                        "omitted because skill status/maturity is not eligible: {:?}/{:?}",
                        skill.status, skill.maturity
                    )],
                    status: Some(skill.status.clone()),
                    maturity: Some(skill.maturity.clone()),
                }
            } else {
                let mut selection = role_match_selection(skill, agent);
                if selection.score == 0 {
                    selection
                        .reasons
                        .push("omitted because it did not match the requested agent role".into());
                } else {
                    selection.reasons.push(
                        "omitted because higher-scoring skills filled the selection limit".into(),
                    );
                }
                selection
            }
        })
        .collect();
    AgentPackSelectionReport {
        agent_name: agent.to_string(),
        selected_skill_count: selected_skill_ids.len(),
        required_skill_count,
        optional_skill_count,
        forbidden_skill_count,
        selections,
        omitted_skills,
    }
}

fn role_match_selection(skill: &Skill, role: &str) -> AgentSkillSelection {
    let role_l = normalize_role_text(role);
    let title_l = normalize_role_text(&skill.title);
    let summary_l = normalize_role_text(&skill.summary);
    let domain_l = skill
        .domain
        .as_deref()
        .map(normalize_role_text)
        .unwrap_or_default();
    let mut score = 0i32;
    let mut reasons = Vec::new();

    for suitability in &skill.role_suitability {
        let candidate_l = normalize_role_text(&suitability.role);
        if candidate_l == role_l {
            let delta = 100 + (suitability.suitability * 25.0).round() as i32;
            score += delta;
            reasons.push(format!(
                "exact role suitability match '{}' (+{delta})",
                suitability.role
            ));
        } else if candidate_l.contains(&role_l) || role_l.contains(&candidate_l) {
            let delta = 65 + (suitability.suitability * 15.0).round() as i32;
            score += delta;
            reasons.push(format!(
                "partial role suitability match '{}' (+{delta})",
                suitability.role
            ));
        } else if token_overlap_score(&candidate_l, &role_l) >= 2 {
            score += 35;
            reasons.push(format!(
                "role token overlap with suitability '{}' (+35)",
                suitability.role
            ));
        }
    }

    let role_tokens = tokens(&role_l);
    for token in &role_tokens {
        if title_l.contains(token) {
            score += 12;
            reasons.push(format!("title contains role token '{token}' (+12)"));
        }
        if summary_l.contains(token) {
            score += 6;
            reasons.push(format!("summary contains role token '{token}' (+6)"));
        }
        if domain_l.contains(token) {
            score += 4;
            reasons.push(format!("domain contains role token '{token}' (+4)"));
        }
        if skill
            .metadata
            .values()
            .any(|v| normalize_role_text(v).contains(token))
        {
            score += 3;
            reasons.push(format!("metadata contains role token '{token}' (+3)"));
        }
        if skill
            .tool_requirements
            .iter()
            .any(|tool| normalize_role_text(&tool.name).contains(token))
        {
            score += 5;
            reasons.push(format!(
                "tool requirement contains role token '{token}' (+5)"
            ));
        }
    }

    if score > 0 {
        match skill.status {
            SkillStatus::Published | SkillStatus::Approved => {
                score += 12;
                reasons.push("approved/published skill quality bonus (+12)".into());
            }
            SkillStatus::Reviewed => {
                score += 8;
                reasons.push("reviewed skill quality bonus (+8)".into());
            }
            SkillStatus::NeedsReview | SkillStatus::Candidate => {
                score += 1;
                reasons.push("candidate skill small quality bonus (+1)".into());
            }
            SkillStatus::Draft
            | SkillStatus::Deprecated
            | SkillStatus::Archived
            | SkillStatus::Unsafe => {}
        }
        if skill.maturity >= SkillMaturity::Level3Verified {
            score += 8;
            reasons.push("verified-or-better maturity bonus (+8)".into());
        } else if skill.maturity >= SkillMaturity::Level2ForgeEnhanced {
            score += 4;
            reasons.push("forge-enhanced maturity bonus (+4)".into());
        }
        if matches!(
            skill.scope,
            SkillScope::TaskLevel | SkillScope::WorkflowLevel
        ) {
            score += 4;
            reasons.push("task/workflow scope bonus (+4)".into());
        }
        if !skill.evals.is_empty() {
            score += 3;
            reasons.push("has eval coverage (+3)".into());
        }
        if matches!(skill.scope, SkillScope::DomainLevel | SkillScope::RoleLevel) {
            score -= 8;
            reasons.push("role/domain-level scope penalty (-8)".into());
        }
    }

    AgentSkillSelection {
        skill_id: skill.id.clone(),
        title: skill.title.clone(),
        score: score.max(0),
        reasons,
        status: Some(skill.status.clone()),
        maturity: Some(skill.maturity.clone()),
    }
}

fn normalize_role_text(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn tokens(value: &str) -> BTreeSet<String> {
    value
        .split_whitespace()
        .filter(|t| {
            t.len() > 2 && !matches!(*t, "agent" | "skill" | "task" | "use" | "for" | "the")
        })
        .map(str::to_string)
        .collect()
}

fn token_overlap_score(a: &str, b: &str) -> usize {
    let a_tokens = tokens(a);
    let b_tokens = tokens(b);
    a_tokens.intersection(&b_tokens).count()
}

fn agent_purpose(role: &str, skills: &[&Skill], bundle: &SkillBundle) -> String {
    let domains = skills
        .iter()
        .filter_map(|s| s.domain.as_deref())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if domains.is_empty() {
        format!(
            "Use selected, source-grounded skills from {} for {role} tasks.",
            bundle.package.name
        )
    } else {
        format!(
            "Use selected, source-grounded {} skills from {} for {role} tasks.",
            domains.join(", "),
            bundle.package.name
        )
    }
}

fn allowed_actions_for_skills(skills: &[&Skill]) -> Vec<String> {
    let mut actions = BTreeSet::from([
        "answer with citations".to_string(),
        "plan read-only checks".to_string(),
    ]);
    if skills.iter().any(|s| s.runtime_policy.recommend_commands) {
        actions.insert("recommend documented commands with citations".into());
    }
    if skills
        .iter()
        .any(|s| s.runtime_policy.run_read_only_commands)
    {
        actions.insert("run read-only commands when runtime policy allows".into());
    }
    actions.into_iter().collect()
}

fn disallowed_actions_for_skills(skills: &[&Skill]) -> Vec<String> {
    let mut actions = BTreeSet::from([
        "mutate external systems without approval".to_string(),
        "use undocumented commands".to_string(),
        "invent unsupported APIs, flags, or versions".to_string(),
    ]);
    if skills
        .iter()
        .any(|s| s.runtime_policy.modify_files || s.runtime_policy.modify_external_systems)
    {
        actions.insert("perform mutation without backup/rollback plan when required".into());
    }
    actions.into_iter().collect()
}

fn review_policy_for_skills(skills: &[&Skill]) -> String {
    let high_risk = skills.iter().any(|s| {
        s.tool_requirements.iter().any(|tool| {
            matches!(
                tool.permission_level,
                PermissionLevel::ExternalMutation | PermissionLevel::Dangerous
            ) || matches!(tool.requirement_type, ToolRequirementType::Dangerous)
        }) || s.runtime_policy.modify_external_systems
    });
    if high_risk {
        "High-risk or external-mutation skills require Level4 human approval and safety eval coverage before default use.".into()
    } else {
        "Reviewed or verified skills may be used; unresolved inference records require reviewer attention.".into()
    }
}

fn example_tasks_for_role(role: &str, skills: &[&Skill]) -> Vec<String> {
    let mut tasks = vec![format!(
        "Ask {role} to solve a source-grounded task with citations."
    )];
    for skill in skills.iter().take(3) {
        tasks.push(format!(
            "Use '{}' to handle a realistic task safely.",
            skill.title
        ));
    }
    tasks
}

fn proposal_eligible_skills(bundle: &SkillBundle) -> Vec<&Skill> {
    bundle
        .skills
        .iter()
        .filter(|s| !is_forbidden_skill(s))
        .collect()
}

fn is_forbidden_skill(skill: &Skill) -> bool {
    matches!(
        skill.status,
        SkillStatus::Unsafe | SkillStatus::Archived | SkillStatus::Deprecated
    ) || skill.maturity < SkillMaturity::Level1StructuredCandidate
}

fn stable_agent_id(bundle_id: &str, role: &str) -> String {
    stable_id("agent", &format!("{bundle_id}:{role}"))
}
