use crate::models::*;
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use uuid::Uuid;

pub fn proposals(bundle: &SkillBundle) -> Vec<AgentProfileProposal> {
    let mut roles = BTreeSet::new();
    for s in &bundle.skills {
        for r in &s.role_suitability {
            roles.insert(r.role.clone());
        }
    }
    if roles.is_empty() {
        roles.insert("Technical Documentation Agent".into());
    }
    roles
        .into_iter()
        .map(|role| AgentProfileProposal {
            agent_id: format!("agent-{}", Uuid::new_v4()),
            agent_name: role.clone(),
            agent_purpose: format!(
                "Use reviewed skills from {} for role-specific assistance.",
                bundle.package.name
            ),
            recommended_skills: bundle
                .skills
                .iter()
                .filter(|s| {
                    s.role_suitability.iter().any(|r| r.role == role) || bundle.skills.len() <= 10
                })
                .map(|s| s.id.clone())
                .collect(),
            required_tools: bundle
                .skills
                .iter()
                .flat_map(|s| s.tool_requirements.iter().map(|t| t.name.clone()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
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
            escalation_policy: "Escalate missing evidence, secrets, or destructive actions.".into(),
            example_tasks: vec!["Diagnose a source-grounded technical issue.".into()],
            evaluation_suite: vec![],
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
}
pub fn write_agent_pack(bundle: &SkillBundle, agent: &str, out: &Path) -> Result<()> {
    fs::create_dir_all(out)?;
    let pack = AgentPack {
        agent_name: agent,
        agent_version: "0.1.0",
        description: format!("Agent pack generated from {}", bundle.package.name),
        source_bundle_ids: vec![bundle.package.bundle_id.clone()],
        skill_ids: selected_skill_ids(bundle, agent),
        required_skills: bundle
            .skills
            .iter()
            .filter(|s| {
                s.maturity >= SkillMaturity::Level3Verified || s.status >= SkillStatus::Reviewed
            })
            .map(|s| s.id.clone())
            .collect(),
        optional_skills: bundle
            .skills
            .iter()
            .filter(|s| s.status == SkillStatus::Candidate || s.status == SkillStatus::NeedsReview)
            .map(|s| s.id.clone())
            .collect(),
        forbidden_skills: bundle
            .skills
            .iter()
            .filter(|s| {
                s.status == SkillStatus::Unsafe
                    || s.maturity < SkillMaturity::Level1StructuredCandidate
            })
            .map(|s| s.id.clone())
            .collect(),
        tool_permissions: bundle
            .skills
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
        evals: bundle
            .skills
            .iter()
            .flat_map(|s| s.evals.iter().cloned())
            .take(25)
            .collect(),
        example_prompts: vec![format!("Ask {} to solve a source-grounded task.", agent)],
        system_prompt_material:
            "Use Skiller skills as governed context; do not exceed runtime permissions.".into(),
    };
    fs::write(out.join("agent-pack.yaml"), serde_yaml::to_string(&pack)?)?;
    Ok(())
}

fn selected_skill_ids(bundle: &SkillBundle, agent: &str) -> Vec<String> {
    let agent_l = agent.to_lowercase();
    let selected: Vec<String> = bundle
        .skills
        .iter()
        .filter(|s| {
            s.role_suitability.iter().any(|r| {
                r.role.to_lowercase().contains(&agent_l) || agent_l.contains(&r.role.to_lowercase())
            }) || bundle.skills.len() <= 10
        })
        .map(|s| s.id.clone())
        .collect();
    if selected.is_empty() {
        bundle
            .skills
            .iter()
            .take(10)
            .map(|s| s.id.clone())
            .collect()
    } else {
        selected
    }
}
