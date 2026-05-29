use crate::models::*;
use anyhow::Result;
use std::fs;
use std::path::Path;

pub fn write_improvement_proposals(bundle: &SkillBundle, out: &Path) -> Result<()> {
    fs::create_dir_all(out)?;
    let mut written = Vec::new();
    for skill in &bundle.skills {
        for proposal in proposals_for_skill(skill) {
            let filename = format!("{}.yaml", proposal.proposal_id);
            fs::write(out.join(&filename), serde_yaml::to_string(&proposal)?)?;
            written.push(filename);
        }
    }
    written.sort();
    fs::write(out.join("index.yaml"), serde_yaml::to_string(&written)?)?;
    Ok(())
}

fn proposals_for_skill(skill: &Skill) -> Vec<SkillImprovementProposal> {
    let mut proposals = Vec::new();

    if skill.confidence.routing < 0.6 {
        proposals.push(build_proposal(
            skill,
            "low-routing-confidence",
            "static-analysis:routing-confidence",
            format!(
                "Skill routing confidence is {:.2}, below the 0.60 review threshold.",
                skill.confidence.routing
            ),
            "Add stronger routing phrases, user-intent examples, negative routing evals, and source-grounded disambiguation notes.",
            RiskLevel::Low,
            false,
        ));
    }

    if skill.evals.is_empty() {
        proposals.push(build_proposal(
            skill,
            "missing-evals",
            "static-analysis:eval-coverage",
            "Skill has no eval cases, so runtime behavior and routing quality cannot be regression-tested.",
            "Add positive, negative, edge-case, safety, routing, and source-grounding evals tied to cited source sections.",
            RiskLevel::Medium,
            false,
        ));
    }

    if skill.evidence_breakdown.speculative_candidate > 0.25 {
        proposals.push(build_proposal(
            skill,
            "speculative-evidence",
            "static-analysis:evidence-breakdown",
            format!(
                "Skill evidence is {:.0}% speculative, above the 25% review threshold.",
                skill.evidence_breakdown.speculative_candidate * 100.0
            ),
            "Attach stronger direct citations, downgrade unsupported claims, or keep the skill staged until verifier/human review resolves speculation.",
            RiskLevel::High,
            false,
        ));
    }

    if has_mutating_or_external_permissions(skill) && !is_human_approved(skill) {
        proposals.push(build_proposal(
            skill,
            "operational-review-required",
            "static-analysis:runtime-policy",
            "Skill can mutate files, use dangerous tools, or affect external systems without human-approved maturity.",
            "Require human review, add approval/rollback guardrails, lower runtime permissions, or split conceptual guidance from operational execution.",
            RiskLevel::High,
            false,
        ));
    }

    if skill.confidence.runtime < 0.2
        && matches!(
            skill.status,
            SkillStatus::Reviewed | SkillStatus::Approved | SkillStatus::Published
        )
    {
        proposals.push(build_proposal(
            skill,
            "needs-runtime-telemetry",
            "static-analysis:runtime-confidence",
            "Skill is reviewable/published but has very low runtime confidence, indicating little or no successful usage telemetry.",
            "Collect route/use/failure telemetry and add runtime-proven examples before promoting the skill to higher maturity levels.",
            RiskLevel::Medium,
            false,
        ));
    }

    proposals
}

fn build_proposal(
    skill: &Skill,
    suffix: &str,
    trigger_source: &str,
    problem_observed: impl Into<String>,
    suggested_change: &str,
    risk: RiskLevel,
    requires_recompile: bool,
) -> SkillImprovementProposal {
    SkillImprovementProposal {
        proposal_id: format!("proposal-{}-{}", sanitize_id(&skill.id), suffix),
        skill_id: skill.id.clone(),
        trigger_source: trigger_source.into(),
        problem_observed: problem_observed.into(),
        suggested_change: suggested_change.into(),
        evidence: proposal_evidence(skill),
        risk,
        requires_recompile,
        requires_review: true,
        status: "open".into(),
    }
}

fn proposal_evidence(skill: &Skill) -> Vec<String> {
    let mut evidence: Vec<String> = skill.source_section_ids.clone();
    for citation in &skill.citations {
        evidence.push(citation.section_id.clone());
    }
    for record in &skill.inference_records {
        evidence.extend(record.source_refs_used.clone());
        evidence.extend(record.candidate_ids_used.clone());
    }
    evidence.sort();
    evidence.dedup();
    if evidence.is_empty() {
        evidence.push(format!("skill:{}", skill.id));
    }
    evidence
}

fn has_mutating_or_external_permissions(skill: &Skill) -> bool {
    skill.runtime_policy.modify_files
        || skill.runtime_policy.modify_external_systems
        || skill.tool_requirements.iter().any(|tool| {
            matches!(
                tool.permission_level,
                PermissionLevel::FileMutation
                    | PermissionLevel::ExternalMutation
                    | PermissionLevel::Dangerous
            ) || matches!(
                tool.requirement_type,
                ToolRequirementType::Mutating | ToolRequirementType::Dangerous
            )
        })
}

fn is_human_approved(skill: &Skill) -> bool {
    matches!(skill.status, SkillStatus::Approved | SkillStatus::Published)
        && skill.maturity >= SkillMaturity::Level4HumanApproved
        && skill.confidence.human_review >= 0.6
}

fn sanitize_id(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('-');
        }
    }
    let collapsed = out
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if collapsed.is_empty() {
        "skill".into()
    } else {
        collapsed
    }
}
