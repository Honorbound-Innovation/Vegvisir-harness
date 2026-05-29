use crate::models::*;
use chrono::Utc;

pub fn verifier_review(bundle: &SkillBundle, reviewer: &str) -> VerifierReviewReport {
    let findings = bundle
        .skills
        .iter()
        .map(|skill| review_skill(skill, reviewer))
        .collect::<Vec<_>>();
    let approved = findings
        .iter()
        .filter(|f| f.decision == ReviewDecision::Approved)
        .count();
    let needs_changes = findings
        .iter()
        .filter(|f| f.decision == ReviewDecision::NeedsChanges)
        .count();
    let unsafe_count = findings
        .iter()
        .filter(|f| f.decision == ReviewDecision::Unsafe)
        .count();
    VerifierReviewReport {
        report_id: stable_review_id(bundle, reviewer),
        bundle_id: bundle.package.bundle_id.clone(),
        reviewer: reviewer.into(),
        created_at: Utc::now(),
        summary: format!(
            "Verifier review completed: {approved} approved, {needs_changes} need changes, {unsafe_count} unsafe."
        ),
        findings,
    }
}

pub fn verifier_review_markdown(report: &VerifierReviewReport) -> String {
    let mut out = format!(
        "# Verifier Review\n\n- Report: {}\n- Bundle: {}\n- Reviewer: {}\n- Created: {}\n- Summary: {}\n\n",
        report.report_id, report.bundle_id, report.reviewer, report.created_at, report.summary
    );
    for finding in &report.findings {
        out.push_str(&format!(
            "## {}\n\n- Decision: {:?}\n- Rationale: {}\n",
            finding.skill_id, finding.decision, finding.rationale
        ));
        if !finding.blockers.is_empty() {
            out.push_str("- Blockers:\n");
            for blocker in &finding.blockers {
                out.push_str(&format!("  - {blocker}\n"));
            }
        }
        if !finding.warnings.is_empty() {
            out.push_str("- Warnings:\n");
            for warning in &finding.warnings {
                out.push_str(&format!("  - {warning}\n"));
            }
        }
        if !finding.required_changes.is_empty() {
            out.push_str("- Required changes:\n");
            for change in &finding.required_changes {
                out.push_str(&format!("  - {change}\n"));
            }
        }
        out.push('\n');
    }
    out
}

fn review_skill(skill: &Skill, reviewer: &str) -> SkillReviewFinding {
    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut required_changes: Vec<String> = Vec::new();

    if skill.citations.is_empty() {
        blockers.push("missing citations".into());
        required_changes.push("add at least one source citation".into());
    }
    if !probability_ok(skill.confidence.raw)
        || !probability_ok(skill.confidence.extraction)
        || !probability_ok(skill.confidence.inference)
        || !probability_ok(skill.confidence.procedure)
        || !probability_ok(skill.confidence.guardrail)
        || !probability_ok(skill.confidence.eval)
        || !probability_ok(skill.confidence.routing)
        || !probability_ok(skill.confidence.source_quality)
        || !probability_ok(skill.confidence.human_review)
        || !probability_ok(skill.confidence.runtime)
    {
        blockers.push("confidence score outside 0.0..=1.0".into());
        required_changes.push("normalize confidence breakdown before review".into());
    }
    let evidence_total = skill.evidence_breakdown.direct_extraction
        + skill.evidence_breakdown.supporting_inference
        + skill.evidence_breakdown.operational_synthesis
        + skill.evidence_breakdown.speculative_candidate
        + skill.evidence_breakdown.community_derived
        + skill.evidence_breakdown.internal_policy_derived;
    if !probability_ok(skill.evidence_breakdown.direct_extraction)
        || !probability_ok(skill.evidence_breakdown.supporting_inference)
        || !probability_ok(skill.evidence_breakdown.operational_synthesis)
        || !probability_ok(skill.evidence_breakdown.speculative_candidate)
        || !probability_ok(skill.evidence_breakdown.community_derived)
        || !probability_ok(skill.evidence_breakdown.internal_policy_derived)
        || evidence_total > 1.05
    {
        blockers.push("invalid evidence breakdown".into());
        required_changes.push("normalize evidence classes and keep total within 100%".into());
    }
    if skill.source_section_ids.is_empty() {
        blockers.push("missing source section references".into());
    }
    if skill.evidence_breakdown.speculative_candidate > 0.35 {
        blockers.push("too much speculative evidence for publication".into());
        required_changes.push("reclassify, strengthen evidence, or keep unpublished".into());
    } else if skill.evidence_breakdown.speculative_candidate > 0.0 {
        warnings.push("contains speculative evidence; keep review gate".into());
    }
    if skill.runtime_policy.modify_external_systems && !skill.runtime_policy.requires_user_approval
    {
        blockers.push("external mutation lacks approval requirement".into());
        required_changes.push("set requires_user_approval and add rollback guardrail".into());
    }
    if skill.runtime_policy.modify_files && !skill.runtime_policy.requires_backup_or_rollback {
        warnings.push("file mutation should require backup or rollback context".into());
    }
    for tool in &skill.tool_requirements {
        if matches!(
            tool.permission_level,
            PermissionLevel::Dangerous | PermissionLevel::ExternalMutation
        ) && !skill.runtime_policy.requires_user_approval
        {
            blockers.push(format!(
                "tool '{}' requires {:?} without user approval",
                tool.name, tool.permission_level
            ));
            required_changes.push("require user approval for dangerous/external tools".into());
        }
        if tool.rollback_required && !skill.runtime_policy.requires_backup_or_rollback {
            warnings.push(format!(
                "tool '{}' requires rollback but skill policy does not require backup/rollback",
                tool.name
            ));
        }
    }
    if skill
        .inference_records
        .iter()
        .any(|record| record.required_review)
    {
        warnings.push("contains inference records requiring review".into());
    }
    if skill.guardrails.len() < 2 {
        warnings.push("thin guardrails".into());
        required_changes.push("add domain, approval, and source-grounding guardrails".into());
    }
    if skill.evals.is_empty() {
        warnings.push("missing eval cases".into());
        required_changes.push("add routing and source-grounding eval cases".into());
    }

    let decision = if skill.status == SkillStatus::Unsafe
        || blockers.iter().any(|b| b.contains("mutation lacks"))
    {
        ReviewDecision::Unsafe
    } else if blockers.is_empty()
        && warnings.is_empty()
        && skill.status >= SkillStatus::Reviewed
        && skill.maturity >= SkillMaturity::Level3Verified
    {
        ReviewDecision::Approved
    } else {
        ReviewDecision::NeedsChanges
    };

    let rationale = match decision {
        ReviewDecision::Approved => "Skill has citations, adequate guardrails, mature status, and no verifier blockers.",
        ReviewDecision::NeedsChanges => "Skill is source-linked but needs review, maturity, evidence, eval, or guardrail improvements before publication.",
        ReviewDecision::Unsafe => "Skill violates a safety publication gate or is explicitly marked unsafe.",
        ReviewDecision::Duplicate => "Skill appears to duplicate another skill.",
        ReviewDecision::Archived => "Skill should be archived.",
    }
    .into();

    SkillReviewFinding {
        skill_id: skill.id.clone(),
        decision,
        reviewer: reviewer.into(),
        rationale,
        blockers,
        warnings,
        required_changes,
    }
}

pub fn apply_verifier_review(
    mut bundle: SkillBundle,
    report: &VerifierReviewReport,
) -> SkillBundle {
    let mut approved = 0usize;
    let mut needs_changes = 0usize;
    let mut unsafe_count = 0usize;
    let mut archived = 0usize;

    for finding in &report.findings {
        if let Some(skill) = bundle.skills.iter_mut().find(|s| s.id == finding.skill_id) {
            skill
                .metadata
                .insert("last_verifier_review_id".into(), report.report_id.clone());
            skill
                .metadata
                .insert("last_verifier_reviewer".into(), report.reviewer.clone());
            skill.metadata.insert(
                "last_verifier_decision".into(),
                format!("{:?}", finding.decision),
            );

            match finding.decision {
                ReviewDecision::Approved => {
                    approved += 1;
                    skill.status = SkillStatus::Reviewed;
                    if skill.maturity < SkillMaturity::Level3Verified {
                        skill.maturity = SkillMaturity::Level3Verified;
                    }
                    skill.confidence.human_review = skill.confidence.human_review.max(0.35);
                    skill.confidence.raw = skill.confidence.raw.max(0.72);
                    push_unique(
                        &mut skill.guardrails,
                        "Verifier-reviewed: keep claims source-grounded and preserve runtime approval gates.",
                    );
                }
                ReviewDecision::NeedsChanges | ReviewDecision::Duplicate => {
                    needs_changes += 1;
                    skill.status = SkillStatus::NeedsReview;
                    skill.metadata.insert(
                        "review_required_changes".into(),
                        finding.required_changes.join(" | "),
                    );
                }
                ReviewDecision::Unsafe => {
                    unsafe_count += 1;
                    skill.status = SkillStatus::Unsafe;
                    skill.runtime_policy.requires_user_approval = true;
                    skill.runtime_policy.requires_backup_or_rollback = true;
                    push_unique(
                        &mut skill.guardrails,
                        "Verifier marked this skill unsafe; do not publish or use for tool execution until corrected and re-reviewed.",
                    );
                }
                ReviewDecision::Archived => {
                    archived += 1;
                    skill.status = SkillStatus::Archived;
                }
            }
        }
    }

    bundle.package.review_status = if unsafe_count > 0 {
        SkillStatus::Unsafe
    } else if needs_changes > 0 {
        SkillStatus::NeedsReview
    } else if approved > 0 {
        SkillStatus::Reviewed
    } else {
        bundle.package.review_status.clone()
    };

    let mut metadata = std::collections::BTreeMap::new();
    metadata.insert("report_id".into(), report.report_id.clone());
    metadata.insert("reviewer".into(), report.reviewer.clone());
    metadata.insert("approved".into(), approved.to_string());
    metadata.insert("needs_changes".into(), needs_changes.to_string());
    metadata.insert("unsafe".into(), unsafe_count.to_string());
    metadata.insert("archived".into(), archived.to_string());
    bundle.audit_events.push(AuditEvent {
        event_id: stable_review_audit_id(&bundle.package.bundle_id, &report.report_id),
        event_type: "apply-verifier-review".into(),
        message: "applied verifier review decisions to staged bundle".into(),
        created_at: Utc::now(),
        metadata,
    });

    bundle
}

fn push_unique(items: &mut Vec<String>, value: &str) {
    if items.iter().all(|item| item != value) {
        items.push(value.into());
    }
}

fn stable_review_id(bundle: &SkillBundle, reviewer: &str) -> String {
    let mut parts = vec![
        bundle.package.bundle_id.as_str(),
        bundle.package.version.as_str(),
        reviewer,
    ];
    let mut skill_ids = bundle
        .skills
        .iter()
        .map(|s| s.id.as_str())
        .collect::<Vec<_>>();
    skill_ids.sort_unstable();
    parts.extend(skill_ids);
    format!("review-{}", stable_hash(&parts.join("|")))
}

fn stable_review_audit_id(bundle_id: &str, report_id: &str) -> String {
    format!(
        "audit-{}",
        stable_hash(&format!("apply-review|{bundle_id}|{report_id}"))
    )
}

fn stable_hash(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn probability_ok(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}
