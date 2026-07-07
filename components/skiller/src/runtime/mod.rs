use crate::models::*;
use crate::semantic;
use anyhow::{Result, bail};

#[derive(Debug)]
pub struct RouteHit {
    pub skill_id: String,
    pub title: String,
    pub score: f32,
}
#[derive(Debug, Clone, Copy)]
pub enum LoadMode {
    Card,
    Body,
    Extended,
}

pub fn route(bundle: &SkillBundle, query: &str, limit: usize) -> Vec<RouteHit> {
    let q = query.to_lowercase();
    let mut hits: Vec<_> = bundle
        .skills
        .iter()
        .map(|s| {
            let mut score = overlap(
                &q,
                &format!("{} {} {}", s.title, s.summary, s.procedure.join(" ")).to_lowercase(),
            );
            if s.title.to_lowercase().contains(&q) {
                score += 1.0;
            }
            if matches!(s.skill_type, SkillType::CliOperation)
                && s.metadata
                    .get("target_command")
                    .and_then(|command| semantic::suspicious_cli_command_reason(command))
                    .is_some()
            {
                score *= 0.1;
            }
            if matches!(
                s.status,
                SkillStatus::Draft | SkillStatus::Candidate | SkillStatus::NeedsReview
            ) {
                score *= 0.85;
            }
            if semantic::looks_like_weak_title(&s.title) {
                score *= 0.8;
            }
            RouteHit {
                skill_id: s.id.clone(),
                title: s.title.clone(),
                score,
            }
        })
        .filter(|h| h.score > 0.0)
        .collect();
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    hits.truncate(limit);
    hits
}

pub fn load_skill(bundle: &SkillBundle, skill_id: &str, mode: LoadMode) -> Result<String> {
    let Some(s) = bundle.skills.iter().find(|s| s.id == skill_id) else {
        bail!("skill not found: {skill_id}")
    };
    let mut out = format!(
        "# {}\n\n{}\n\nStatus: {:?}\nMaturity: {:?}\n",
        s.title, s.summary, s.status, s.maturity
    );
    if matches!(mode, LoadMode::Body | LoadMode::Extended) {
        out.push_str("\n## Procedure\n");
        for step in &s.procedure {
            out.push_str(&format!("- {}\n", step));
        }
        out.push_str("\n## Guardrails\n");
        for g in &s.guardrails {
            out.push_str(&format!("- {}\n", g));
        }
        if !s.scripts.is_empty() {
            out.push_str("\n## Embedded Scripts\n");
            for script in &s.scripts {
                out.push_str(&format!(
                    "- `{}` ({:?}, {:?}): {} Entrypoint: `{}`. Permission: {:?}. Deterministic: {}. Approval required: {}.\n",
                    script.id,
                    script.script_type,
                    script.language,
                    script.description,
                    script.entrypoint,
                    script.permission_level,
                    script.deterministic,
                    script.requires_approval
                ));
            }
        }
    }
    if matches!(mode, LoadMode::Extended) {
        if !s.scripts.is_empty() {
            out.push_str("\n## Embedded Script Bodies\n");
            for script in &s.scripts {
                out.push_str(&format!(
                    "### {} (`{}`)\n\n- Type: {:?}\n- Language: {:?}\n- Entrypoint: `{}`\n- Inputs: {}\n- Outputs: {}\n- Source sections: {}\n\nGuardrails:\n{}\n\n```text\n{}\n```\n",
                    script.title,
                    script.id,
                    script.script_type,
                    script.language,
                    script.entrypoint,
                    script.inputs.join(", "),
                    script.outputs.join(", "),
                    script.source_section_ids.join(", "),
                    script
                        .guardrails
                        .iter()
                        .map(|guardrail| format!("- {guardrail}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    script.content
                ));
            }
        }
        out.push_str("\n## Citations\n");
        for c in &s.citations {
            out.push_str(&format!("- {}: {}\n", c.citation_id, c.excerpt));
        }
        out.push_str("\n## Runtime Policy\n");
        out.push_str(&serde_yaml::to_string(&s.runtime_policy)?);
    }
    Ok(out)
}

fn overlap(q: &str, text: &str) -> f32 {
    q.split_whitespace()
        .filter(|w| w.len() > 2 && text.contains(*w))
        .count() as f32
        / (q.split_whitespace().count().max(1) as f32)
}
