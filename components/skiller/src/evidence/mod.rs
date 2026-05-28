use crate::models::*;
pub fn evidence_report_markdown(bundle: &SkillBundle) -> String {
    let mut out = format!("# Evidence Report: {}\n\n", bundle.package.name);
    for s in &bundle.skills {
        out.push_str(&format!("## {}\n\n- Direct extraction: {:.0}%\n- Supporting inference: {:.0}%\n- Operational synthesis: {:.0}%\n- Speculative: {:.0}%\n- Citations: {}\n\n", s.title, s.evidence_breakdown.direct_extraction*100.0, s.evidence_breakdown.supporting_inference*100.0, s.evidence_breakdown.operational_synthesis*100.0, s.evidence_breakdown.speculative_candidate*100.0, s.citations.len()));
    }
    out
}
