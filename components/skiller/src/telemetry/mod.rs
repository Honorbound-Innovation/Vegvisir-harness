use crate::models::*;
use anyhow::Result;
use std::fs;
use std::path::Path;
use uuid::Uuid;

pub fn write_improvement_proposals(bundle: &SkillBundle, out: &Path) -> Result<()> {
    fs::create_dir_all(out)?;
    for skill in &bundle.skills {
        if skill.confidence.routing < 0.6 || skill.evals.is_empty() {
            let proposal = SkillImprovementProposal { proposal_id: format!("proposal-{}", Uuid::new_v4()), skill_id: skill.id.clone(), trigger_source: "static-analysis".into(), problem_observed: "Skill has low routing confidence or thin eval coverage.".into(), suggested_change: "Add stronger routing phrases, positive/negative evals, and source-grounded examples.".into(), evidence: skill.source_section_ids.clone(), risk: RiskLevel::Low, requires_recompile: false, requires_review: true, status: "open".into() };
            fs::write(
                out.join(format!("{}.yaml", proposal.proposal_id)),
                serde_yaml::to_string(&proposal)?,
            )?;
        }
    }
    Ok(())
}
