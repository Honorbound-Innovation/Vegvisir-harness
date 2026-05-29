use crate::ingest::stable_id;
use crate::models::*;
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Serialize)]
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

#[derive(Clone, Debug, Serialize)]
pub struct AgentPackLifecycleStatus {
    pub plan_id: String,
    pub lifecycle_ready: bool,
    pub human_review_required: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
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
            let recommended_skills: Vec<String> = proposal_eligible_skills(bundle)
                .into_iter()
                .filter(|s| {
                    s.role_suitability.iter().any(|r| r.role == role) || bundle.skills.len() <= 10
                })
                .map(|s| s.id.clone())
                .collect();
            let required_tools = proposal_eligible_skills(bundle)
                .into_iter()
                .filter(|s| recommended_skills.contains(&s.id))
                .flat_map(|s| s.tool_requirements.iter().map(|t| t.name.clone()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            AgentProfileProposal {
                agent_id: stable_agent_id(&bundle.package.bundle_id, &role),
                agent_name: role.clone(),
                agent_purpose: format!(
                    "Use reviewed skills from {} for role-specific assistance.",
                    bundle.package.name
                ),
                recommended_skills,
                required_tools,
                allowed_actions: vec![
                    "answer with citations".into(),
                    "plan read-only checks".into(),
                ],
                disallowed_actions: vec![
                    "mutate external systems without approval".into(),
                    "use undocumented commands".into(),
                ],
                runtime_context_policy: "Load smallest sufficient skills and citations.".into(),
                review_policy: "High-risk skills require human approval.".into(),
                escalation_policy: "Escalate missing evidence, secrets, or destructive actions."
                    .into(),
                example_tasks: vec!["Diagnose a source-grounded technical issue.".into()],
                evaluation_suite: vec![],
            }
        })
        .collect()
}

pub fn write_agent_proposals(bundle: &SkillBundle, out: &Path) -> Result<()> {
    fs::create_dir_all(out)?;
    for p in proposals(bundle) {
        fs::write(
            out.join(format!(
                "{}.yaml",
                p.agent_name.to_lowercase().replace(' ', "-")
            )),
            serde_yaml::to_string(&p)?,
        )?;
    }
    Ok(())
}

#[derive(Serialize)]
struct AgentPack<'a> {
    agent_name: &'a str,
    agent_version: &'a str,
    description: String,
    source_bundle_ids: Vec<String>,
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
    system_prompt_material: String,
    lifecycle_status: Option<AgentPackLifecycleStatus>,
    eval_status: AgentPackEvalStatus,
}
pub fn write_agent_pack(
    bundle: &SkillBundle,
    agent: &str,
    out: &Path,
    lifecycle_status_path: Option<&Path>,
) -> Result<()> {
    fs::create_dir_all(out)?;
    let lifecycle_status = lifecycle_status_path
        .map(read_agent_pack_lifecycle_status)
        .transpose()?;
    let selected_skill_ids = selected_skill_ids(bundle, agent);
    let selected_skill_id_set: BTreeSet<String> = selected_skill_ids.iter().cloned().collect();
    let selected_skills: Vec<&Skill> = bundle
        .skills
        .iter()
        .filter(|s| selected_skill_id_set.contains(&s.id) && !is_forbidden_skill(s))
        .collect();
    let eval_status = agent_pack_eval_status(&selected_skills);
    let pack = AgentPack {
        agent_name: agent,
        agent_version: "0.1.0",
        description: format!("Agent pack generated from {}", bundle.package.name),
        source_bundle_ids: vec![bundle.package.bundle_id.clone()],
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
        system_prompt_material:
            "Use Skiller skills as governed context; do not exceed runtime permissions.".into(),
        lifecycle_status,
        eval_status,
    };
    fs::write(out.join("agent-pack.yaml"), serde_yaml::to_string(&pack)?)?;
    Ok(())
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
    let agent_l = agent.to_lowercase();
    let eligible = proposal_eligible_skills(bundle);
    let selected: Vec<String> = eligible
        .iter()
        .filter(|s| {
            s.role_suitability.iter().any(|r| {
                r.role.to_lowercase().contains(&agent_l) || agent_l.contains(&r.role.to_lowercase())
            }) || eligible.len() <= 10
        })
        .map(|s| s.id.clone())
        .collect();
    if selected.is_empty() {
        eligible.iter().take(10).map(|s| s.id.clone()).collect()
    } else {
        selected
    }
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
