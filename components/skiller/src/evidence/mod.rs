use crate::models::*;
use std::collections::{BTreeMap, BTreeSet};

pub fn evidence_report_markdown(bundle: &SkillBundle) -> String {
    let source_by_id = bundle
        .sources
        .iter()
        .map(|source| (source.source_id.as_str(), source))
        .collect::<BTreeMap<_, _>>();
    let section_source = bundle
        .sections
        .iter()
        .map(|section| (section.section_id.as_str(), section.source_id.as_str()))
        .collect::<BTreeMap<_, _>>();

    let mut out = format!("# Evidence Report: {}\n\n", bundle.package.name);
    out.push_str(&format!(
        "- Bundle ID: {}\n- Version: {}\n- Review status: {:?}\n- Publish status: {:?}\n- Skills: {}\n- Sources: {}\n\n",
        bundle.package.bundle_id,
        bundle.package.version,
        bundle.package.review_status,
        bundle.package.publish_status,
        bundle.skills.len(),
        bundle.sources.len()
    ));

    out.push_str("## Source Trust and Rights\n\n");
    let mut trust_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut right_warnings = Vec::new();
    for source in &bundle.sources {
        *trust_counts
            .entry(format!("{:?}", infer_source_trust(source)))
            .or_default() += 1;
        if !matches!(source.permission_status, PermissionStatus::Allowed) {
            right_warnings.push(format!(
                "{} permission status is {:?}",
                source.source_id, source.permission_status
            ));
        }
        if !matches!(source.secret_scan_status, ScanStatus::Clean) {
            right_warnings.push(format!(
                "{} has unresolved secret scan status {:?}",
                source.source_id, source.secret_scan_status
            ));
        }
        if !matches!(source.export_policy, ExportPolicy::PublicAllowed) {
            right_warnings.push(format!(
                "{} export policy is {:?}",
                source.source_id, source.export_policy
            ));
        }
    }
    if trust_counts.is_empty() {
        out.push_str("- No sources recorded.\n");
    } else {
        for (trust, count) in trust_counts {
            out.push_str(&format!("- {trust}: {count}\n"));
        }
    }
    if !right_warnings.is_empty() {
        out.push_str("\n### Source Rights Warnings\n\n");
        for warning in right_warnings {
            out.push_str(&format!("- {warning}\n"));
        }
    }
    out.push('\n');

    out.push_str("## Evidence Summary by Skill\n\n");
    for skill in &bundle.skills {
        out.push_str(&format!(
            "## {}\n\n- Skill ID: {}\n- Status: {:?}\n- Maturity: {:?}\n- Scope: {:?}\n- Type: {:?}\n- Direct extraction: {:.0}%\n- Supporting inference: {:.0}%\n- Operational synthesis: {:.0}%\n- Speculative: {:.0}%\n- Community-derived: {:.0}%\n- Internal-policy derived: {:.0}%\n- Raw confidence: {:.0}%\n- Routing confidence: {:.0}%\n- Human review confidence: {:.0}%\n- Citations: {}\n- Inference records: {}\n\n",
            skill.title,
            skill.id,
            skill.status,
            skill.maturity,
            skill.scope,
            skill.skill_type,
            skill.evidence_breakdown.direct_extraction * 100.0,
            skill.evidence_breakdown.supporting_inference * 100.0,
            skill.evidence_breakdown.operational_synthesis * 100.0,
            skill.evidence_breakdown.speculative_candidate * 100.0,
            skill.evidence_breakdown.community_derived * 100.0,
            skill.evidence_breakdown.internal_policy_derived * 100.0,
            skill.confidence.raw * 100.0,
            skill.confidence.routing * 100.0,
            skill.confidence.human_review * 100.0,
            skill.citations.len(),
            skill.inference_records.len()
        ));

        let warnings = skill_warnings(skill);
        if !warnings.is_empty() {
            out.push_str("### Evidence / Publication Warnings\n\n");
            for warning in warnings {
                out.push_str(&format!("- {warning}\n"));
            }
            out.push('\n');
        }

        if !skill.citations.is_empty() {
            out.push_str("### Citations\n\n");
            for citation in &skill.citations {
                let source_title = source_by_id
                    .get(citation.source_id.as_str())
                    .map(|source| source.title.as_str())
                    .unwrap_or("unknown source");
                let ownership = match section_source.get(citation.section_id.as_str()) {
                    Some(source_id) if *source_id == citation.source_id.as_str() => "ok",
                    Some(_) => "source/section mismatch",
                    None => "missing section",
                };
                out.push_str(&format!(
                    "- {}: source={} ({source_title}), section={}, ownership={}\n",
                    citation.citation_id, citation.source_id, citation.section_id, ownership
                ));
            }
            out.push('\n');
        }

        if !skill.inference_records.is_empty() {
            out.push_str("### Inference Records\n\n");
            for record in &skill.inference_records {
                out.push_str(&format!(
                    "- {}: {:?}, evidence={:?}, confidence={:.0}%, required_review={}, refs={}\n",
                    record.inference_id,
                    record.inference_type,
                    record.evidence_type,
                    record.confidence * 100.0,
                    record.required_review,
                    record.source_refs_used.join(", ")
                ));
                if !record.unsupported_assumptions.is_empty() {
                    out.push_str(&format!(
                        "  - Unsupported assumptions: {}\n",
                        record.unsupported_assumptions.join(" | ")
                    ));
                }
                if !record.risk_flags.is_empty() {
                    out.push_str(&format!(
                        "  - Risk flags: {}\n",
                        record.risk_flags.join(" | ")
                    ));
                }
            }
            out.push('\n');
        }

        if !skill.tool_requirements.is_empty() {
            out.push_str("### Tool Requirements\n\n");
            let mut seen = BTreeSet::new();
            for tool in &skill.tool_requirements {
                if seen.insert(tool.name.clone()) {
                    out.push_str(&format!(
                        "- {}: {:?}, permission={:?}, rollback_required={}\n",
                        tool.name,
                        tool.requirement_type,
                        tool.permission_level,
                        tool.rollback_required
                    ));
                }
            }
            out.push('\n');
        }
    }
    out
}

fn skill_warnings(skill: &Skill) -> Vec<String> {
    let mut warnings = Vec::new();
    if skill.citations.is_empty() {
        warnings.push("missing citations".into());
    }
    if skill.evidence_breakdown.speculative_candidate > 0.25 {
        warnings.push("speculative evidence exceeds 25% review threshold".into());
    }
    if skill
        .inference_records
        .iter()
        .any(|record| record.required_review)
    {
        warnings.push("contains inference records requiring review".into());
    }
    if skill.runtime_policy.modify_external_systems && !skill.runtime_policy.requires_user_approval
    {
        warnings.push("external mutation lacks user approval requirement".into());
    }
    if skill.runtime_policy.modify_files && !skill.runtime_policy.requires_backup_or_rollback {
        warnings.push("file mutation lacks backup/rollback requirement".into());
    }
    if matches!(
        skill.status,
        SkillStatus::Unsafe | SkillStatus::Archived | SkillStatus::Deprecated
    ) {
        warnings.push(format!(
            "skill status {:?} is not publishable",
            skill.status
        ));
    }
    warnings
}

fn infer_source_trust(source: &SourceDocument) -> SourceTrust {
    match source.source_type {
        SourceType::OpenApi | SourceType::ApiSpec => SourceTrust::OfficialApiSpecification,
        SourceType::CliHelp | SourceType::CliSpec => SourceTrust::OfficialCliReference,
        SourceType::Repository => SourceTrust::ProjectMaintainerDocumentation,
        SourceType::Unknown => SourceTrust::UnknownSource,
        _ => {
            let origin = source.origin.to_lowercase();
            if origin.contains("official") || origin.contains("docs.") {
                SourceTrust::OfficialVendorDocumentation
            } else if matches!(
                source.visibility,
                Visibility::Internal | Visibility::Restricted
            ) {
                SourceTrust::InternalCompanyDocumentation
            } else {
                SourceTrust::ProjectMaintainerDocumentation
            }
        }
    }
}
