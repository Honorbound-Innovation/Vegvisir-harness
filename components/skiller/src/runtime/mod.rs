use crate::models::*;
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
            RouteHit {
                skill_id: s.id.clone(),
                title: s.title.clone(),
                score,
            }
        })
        .filter(|h| h.score > 0.0)
        .collect();
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
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
    }
    if matches!(mode, LoadMode::Extended) {
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
