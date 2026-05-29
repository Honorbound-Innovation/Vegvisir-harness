use crate::domain;
use crate::ingest;
use crate::models::*;
use crate::source_meta;
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct CorpusMap {
    pub bundle_id: String,
    pub domain: Option<String>,
    pub source_count: usize,
    pub section_count: usize,
    pub skill_count: usize,
    pub candidate_count: usize,
    pub concepts: Vec<CorpusConcept>,
    pub source_clusters: BTreeMap<String, Vec<String>>,
    pub skill_clusters: BTreeMap<String, Vec<String>>,
    pub source_trust_summary: BTreeMap<String, usize>,
    pub domain_profile: Option<DomainProfileSummary>,
    pub missing_area_hints: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct DomainProfileSummary {
    pub name: String,
    pub preferred_agent_roles: Vec<String>,
    pub known_tools: Vec<String>,
    pub required_review_policy: String,
}

#[derive(Debug, Serialize)]
pub struct CorpusConcept {
    pub name: String,
    pub skill_ids: Vec<String>,
    pub section_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CorpusManifest {
    pub bundle_id: String,
    pub bundle_version: String,
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub source_count: usize,
    pub section_count: usize,
    pub skill_count: usize,
    pub source_hash: String,
    pub section_hash: String,
    pub skill_hash: String,
    pub sources: Vec<CorpusManifestSource>,
    pub change_hints: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CorpusChangeReport {
    pub old_bundle_id: String,
    pub old_bundle_version: String,
    pub new_bundle_id: String,
    pub new_bundle_version: String,
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub source_set_changed: bool,
    pub section_set_changed: bool,
    pub skill_set_changed: bool,
    pub added_sources: Vec<CorpusManifestSource>,
    pub removed_sources: Vec<CorpusManifestSource>,
    pub changed_sources: Vec<CorpusSourceChange>,
    pub unchanged_source_count: usize,
    pub review_required: bool,
    pub review_reasons: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CorpusLifecyclePlan {
    pub old_bundle_id: String,
    pub old_bundle_version: String,
    pub new_bundle_id: String,
    pub new_bundle_version: String,
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub plan_id: String,
    pub review_required: bool,
    pub recommended_version_bump: String,
    pub actions: Vec<CorpusLifecycleAction>,
    pub review_reasons: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CorpusLifecycleStatus {
    pub plan_id: String,
    pub bundle_id: String,
    pub bundle_version: String,
    pub matches_plan_target: bool,
    pub validation_valid: bool,
    pub readiness_ready: bool,
    pub action_count: usize,
    pub open_action_count: usize,
    pub critical_action_count: usize,
    pub high_action_count: usize,
    pub human_review_required: bool,
    pub lifecycle_ready: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CorpusLifecycleAction {
    pub action_id: String,
    pub action_type: String,
    pub target_type: String,
    pub target_id: String,
    pub title: String,
    pub reason: String,
    pub priority: String,
    pub requires_human_review: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CorpusSourceChange {
    pub source_id: String,
    pub title: String,
    pub old_hash: String,
    pub new_hash: String,
    pub old_version: Option<String>,
    pub new_version: Option<String>,
    pub old_section_count: usize,
    pub new_section_count: usize,
    pub old_skill_count: usize,
    pub new_skill_count: usize,
    pub changed_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusManifestSource {
    pub source_id: String,
    pub title: String,
    pub source_type: String,
    pub origin: String,
    pub version: Option<String>,
    pub hash: String,
    pub source_trust: String,
    pub section_count: usize,
    pub skill_count: usize,
    pub permission_status: String,
    pub secret_scan_status: String,
    pub export_policy: String,
}

pub fn build_corpus_manifest(bundle: &SkillBundle) -> CorpusManifest {
    let mut sections_by_source: BTreeMap<String, usize> = BTreeMap::new();
    for section in &bundle.sections {
        *sections_by_source
            .entry(section.source_id.clone())
            .or_insert(0) += 1;
    }
    let mut skills_by_source: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for skill in &bundle.skills {
        for source_id in skill
            .citations
            .iter()
            .map(|citation| citation.source_id.clone())
            .chain(skill.source_section_ids.iter().filter_map(|section_id| {
                bundle
                    .sections
                    .iter()
                    .find(|section| &section.section_id == section_id)
                    .map(|section| section.source_id.clone())
            }))
        {
            skills_by_source
                .entry(source_id)
                .or_default()
                .insert(skill.id.clone());
        }
    }
    let mut sources: Vec<_> = bundle
        .sources
        .iter()
        .map(|source| CorpusManifestSource {
            source_id: source.source_id.clone(),
            title: source.title.clone(),
            source_type: format!("{:?}", source.source_type),
            origin: source.origin.clone(),
            version: source.version.clone(),
            hash: source.hash.clone(),
            source_trust: format!("{:?}", source_meta::infer_source_trust(source)),
            section_count: sections_by_source
                .get(&source.source_id)
                .copied()
                .unwrap_or(0),
            skill_count: skills_by_source
                .get(&source.source_id)
                .map(|ids| ids.len())
                .unwrap_or(0),
            permission_status: format!("{:?}", source.permission_status),
            secret_scan_status: format!("{:?}", source.secret_scan_status),
            export_policy: format!("{:?}", source.export_policy),
        })
        .collect();
    sources.sort_by(|a, b| a.source_id.cmp(&b.source_id));

    let source_hash_input = sources
        .iter()
        .map(|source| {
            format!(
                "{}\t{}\t{}\t{}\t{}",
                source.source_id,
                source.origin,
                source.version.clone().unwrap_or_default(),
                source.hash,
                source.source_trust
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let section_hash_input = bundle
        .sections
        .iter()
        .map(|section| {
            format!(
                "{}\t{}\t{}\t{}\t{}",
                section.section_id,
                section.source_id,
                section.heading,
                section.line_start,
                section.line_end
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let skill_hash_input = bundle
        .skills
        .iter()
        .map(|skill| {
            format!(
                "{}\t{}\t{:?}\t{:?}\t{}",
                skill.id,
                skill.title,
                skill.status,
                skill.maturity,
                skill.metadata.len()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut change_hints = Vec::new();
    if sources.iter().any(|source| source.version.is_none()) {
        change_hints.push("Some sources have no detected version; version-aware recompiles may need manual review.".into());
    }
    if bundle
        .skills
        .iter()
        .any(|skill| skill.version_applicability.version_confidence == 0.0)
    {
        change_hints.push("Some skills lack version applicability; recompile with versioned sources or apply a Forge/version pass.".into());
    }
    if bundle.package.review_status != SkillStatus::Approved
        && bundle.package.review_status != SkillStatus::Published
    {
        change_hints.push(
            "Bundle is not approved/published; downstream agent packs should remain staged.".into(),
        );
    }

    CorpusManifest {
        bundle_id: bundle.package.bundle_id.clone(),
        bundle_version: bundle.package.version.clone(),
        generated_at: Utc::now(),
        source_count: bundle.sources.len(),
        section_count: bundle.sections.len(),
        skill_count: bundle.skills.len(),
        source_hash: ingest::stable_id("source-set", &source_hash_input),
        section_hash: ingest::stable_id("section-set", &section_hash_input),
        skill_hash: ingest::stable_id("skill-set", &skill_hash_input),
        sources,
        change_hints,
    }
}

pub fn write_corpus_manifest(bundle: &SkillBundle, out: &Path) -> Result<()> {
    fs::create_dir_all(out)?;
    let manifest = build_corpus_manifest(bundle);
    fs::write(
        out.join("corpus-manifest.yaml"),
        serde_yaml::to_string(&manifest)?,
    )?;
    fs::write(
        out.join("corpus-manifest.md"),
        corpus_manifest_markdown(&manifest),
    )?;
    Ok(())
}

fn corpus_manifest_markdown(manifest: &CorpusManifest) -> String {
    let mut out = format!(
        "# Corpus Manifest: {} {}\n\n",
        manifest.bundle_id, manifest.bundle_version
    );
    out.push_str(&format!(
        "Sources: {}  Sections: {}  Skills: {}\n\n",
        manifest.source_count, manifest.section_count, manifest.skill_count
    ));
    out.push_str(&format!("- Source set hash: `{}`\n", manifest.source_hash));
    out.push_str(&format!(
        "- Section set hash: `{}`\n",
        manifest.section_hash
    ));
    out.push_str(&format!("- Skill set hash: `{}`\n\n", manifest.skill_hash));
    out.push_str("## Sources\n\n");
    for source in &manifest.sources {
        out.push_str(&format!(
            "- `{}` {} ({}, trust: {}, version: {}, sections: {}, skills: {})\n",
            source.source_id,
            source.title,
            source.source_type,
            source.source_trust,
            source.version.clone().unwrap_or_else(|| "unknown".into()),
            source.section_count,
            source.skill_count
        ));
    }
    out.push_str("\n## Change Hints\n\n");
    for hint in &manifest.change_hints {
        out.push_str(&format!("- {}\n", hint));
    }
    out
}

pub fn compare_corpus_manifests(old: &CorpusManifest, new: &CorpusManifest) -> CorpusChangeReport {
    let old_sources: BTreeMap<_, _> = old
        .sources
        .iter()
        .map(|source| (source.source_id.clone(), source.clone()))
        .collect();
    let new_sources: BTreeMap<_, _> = new
        .sources
        .iter()
        .map(|source| (source.source_id.clone(), source.clone()))
        .collect();

    let mut added_sources = Vec::new();
    let mut removed_sources = Vec::new();
    let mut changed_sources = Vec::new();
    let mut unchanged_source_count = 0;

    for (source_id, new_source) in &new_sources {
        match old_sources.get(source_id) {
            None => added_sources.push(new_source.clone()),
            Some(old_source) => {
                let mut changed_fields = Vec::new();
                if old_source.hash != new_source.hash {
                    changed_fields.push("hash".into());
                }
                if old_source.version != new_source.version {
                    changed_fields.push("version".into());
                }
                if old_source.section_count != new_source.section_count {
                    changed_fields.push("section_count".into());
                }
                if old_source.skill_count != new_source.skill_count {
                    changed_fields.push("skill_count".into());
                }
                if old_source.permission_status != new_source.permission_status {
                    changed_fields.push("permission_status".into());
                }
                if old_source.secret_scan_status != new_source.secret_scan_status {
                    changed_fields.push("secret_scan_status".into());
                }
                if old_source.export_policy != new_source.export_policy {
                    changed_fields.push("export_policy".into());
                }
                if changed_fields.is_empty() {
                    unchanged_source_count += 1;
                } else {
                    changed_sources.push(CorpusSourceChange {
                        source_id: source_id.clone(),
                        title: new_source.title.clone(),
                        old_hash: old_source.hash.clone(),
                        new_hash: new_source.hash.clone(),
                        old_version: old_source.version.clone(),
                        new_version: new_source.version.clone(),
                        old_section_count: old_source.section_count,
                        new_section_count: new_source.section_count,
                        old_skill_count: old_source.skill_count,
                        new_skill_count: new_source.skill_count,
                        changed_fields,
                    });
                }
            }
        }
    }
    for (source_id, old_source) in &old_sources {
        if !new_sources.contains_key(source_id) {
            removed_sources.push(old_source.clone());
        }
    }

    let mut paired_added = BTreeSet::new();
    let mut paired_removed = BTreeSet::new();
    for (removed_index, old_source) in removed_sources.iter().enumerate() {
        if let Some((added_index, new_source)) =
            added_sources.iter().enumerate().find(|(idx, candidate)| {
                !paired_added.contains(idx)
                    && candidate.title == old_source.title
                    && candidate.source_type == old_source.source_type
            })
        {
            let mut changed_fields = vec!["source_id".into(), "origin".into()];
            if old_source.hash != new_source.hash {
                changed_fields.push("hash".into());
            }
            if old_source.version != new_source.version {
                changed_fields.push("version".into());
            }
            if old_source.section_count != new_source.section_count {
                changed_fields.push("section_count".into());
            }
            if old_source.skill_count != new_source.skill_count {
                changed_fields.push("skill_count".into());
            }
            if old_source.permission_status != new_source.permission_status {
                changed_fields.push("permission_status".into());
            }
            if old_source.secret_scan_status != new_source.secret_scan_status {
                changed_fields.push("secret_scan_status".into());
            }
            if old_source.export_policy != new_source.export_policy {
                changed_fields.push("export_policy".into());
            }
            changed_sources.push(CorpusSourceChange {
                source_id: new_source.source_id.clone(),
                title: new_source.title.clone(),
                old_hash: old_source.hash.clone(),
                new_hash: new_source.hash.clone(),
                old_version: old_source.version.clone(),
                new_version: new_source.version.clone(),
                old_section_count: old_source.section_count,
                new_section_count: new_source.section_count,
                old_skill_count: old_source.skill_count,
                new_skill_count: new_source.skill_count,
                changed_fields,
            });
            paired_added.insert(added_index);
            paired_removed.insert(removed_index);
        }
    }
    added_sources = added_sources
        .into_iter()
        .enumerate()
        .filter_map(|(idx, source)| (!paired_added.contains(&idx)).then_some(source))
        .collect();
    removed_sources = removed_sources
        .into_iter()
        .enumerate()
        .filter_map(|(idx, source)| (!paired_removed.contains(&idx)).then_some(source))
        .collect();

    added_sources.sort_by(|a, b| a.source_id.cmp(&b.source_id));
    removed_sources.sort_by(|a, b| a.source_id.cmp(&b.source_id));
    changed_sources.sort_by(|a, b| a.source_id.cmp(&b.source_id));

    let source_set_changed = old.source_hash != new.source_hash;
    let section_set_changed = old.section_hash != new.section_hash;
    let skill_set_changed = old.skill_hash != new.skill_hash;
    let mut review_reasons = Vec::new();
    if !added_sources.is_empty() {
        review_reasons.push(format!("{} source(s) added", added_sources.len()));
    }
    if !removed_sources.is_empty() {
        review_reasons.push(format!("{} source(s) removed", removed_sources.len()));
    }
    if !changed_sources.is_empty() {
        review_reasons.push(format!("{} source(s) changed", changed_sources.len()));
    }
    if section_set_changed {
        review_reasons.push("section set changed; affected skills should be revalidated".into());
    }
    if skill_set_changed {
        review_reasons.push("skill set changed; agent packs and routing should be reviewed".into());
    }
    for source in added_sources.iter().chain(
        changed_sources
            .iter()
            .filter_map(|change| new_sources.get(&change.source_id)),
    ) {
        if source.secret_scan_status.contains("Findings") {
            review_reasons.push(format!(
                "source {} has unresolved secret-scan findings",
                source.source_id
            ));
        }
        if source.permission_status.contains("Blocked") {
            review_reasons.push(format!(
                "source {} has blocked permission status",
                source.source_id
            ));
        }
    }

    CorpusChangeReport {
        old_bundle_id: old.bundle_id.clone(),
        old_bundle_version: old.bundle_version.clone(),
        new_bundle_id: new.bundle_id.clone(),
        new_bundle_version: new.bundle_version.clone(),
        generated_at: Utc::now(),
        source_set_changed,
        section_set_changed,
        skill_set_changed,
        added_sources,
        removed_sources,
        changed_sources,
        unchanged_source_count,
        review_required: !review_reasons.is_empty(),
        review_reasons,
    }
}

pub fn build_corpus_lifecycle_plan(report: &CorpusChangeReport) -> CorpusLifecyclePlan {
    let mut actions = Vec::new();

    for source in &report.added_sources {
        actions.push(CorpusLifecycleAction {
            action_id: ingest::stable_id(
                "corpus-action",
                &format!("add:{}:{}", report.new_bundle_id, source.source_id),
            ),
            action_type: "compile-new-source".into(),
            target_type: "source".into(),
            target_id: source.source_id.clone(),
            title: source.title.clone(),
            reason:
                "New source requires deterministic extraction, validation, and reviewer triage."
                    .into(),
            priority: if source.secret_scan_status.contains("Findings")
                || source.permission_status.contains("Blocked")
            {
                "critical".into()
            } else {
                "medium".into()
            },
            requires_human_review: true,
        });
    }

    for source in &report.removed_sources {
        actions.push(CorpusLifecycleAction {
            action_id: ingest::stable_id("corpus-action", &format!("remove:{}:{}", report.old_bundle_id, source.source_id)),
            action_type: "review-removed-source-impact".into(),
            target_type: "source".into(),
            target_id: source.source_id.clone(),
            title: source.title.clone(),
            reason: "Removed source may orphan citations, weaken evidence, or require skill deprecation.".into(),
            priority: "high".into(),
            requires_human_review: true,
        });
    }

    for change in &report.changed_sources {
        let priority = if change
            .changed_fields
            .iter()
            .any(|field| field == "permission_status" || field == "secret_scan_status")
        {
            "critical"
        } else if change
            .changed_fields
            .iter()
            .any(|field| field == "version" || field == "hash")
        {
            "high"
        } else {
            "medium"
        };
        actions.push(CorpusLifecycleAction {
            action_id: ingest::stable_id(
                "corpus-action",
                &format!(
                    "change:{}:{}:{}",
                    report.new_bundle_id,
                    change.source_id,
                    change.changed_fields.join(",")
                ),
            ),
            action_type: "revalidate-changed-source".into(),
            target_type: "source".into(),
            target_id: change.source_id.clone(),
            title: change.title.clone(),
            reason: format!(
                "Changed fields: {}. Re-run validation and review affected skills.",
                change.changed_fields.join(", ")
            ),
            priority: priority.into(),
            requires_human_review: true,
        });
    }

    if report.section_set_changed {
        actions.push(CorpusLifecycleAction {
            action_id: ingest::stable_id("corpus-action", &format!("sections:{}:{}", report.old_bundle_version, report.new_bundle_version)),
            action_type: "rebuild-section-index".into(),
            target_type: "bundle".into(),
            target_id: report.new_bundle_id.clone(),
            title: "Section index changed".into(),
            reason: "Section boundaries or source coverage changed; rerun citation and evidence validation.".into(),
            priority: "high".into(),
            requires_human_review: true,
        });
    }

    if report.skill_set_changed {
        actions.push(CorpusLifecycleAction {
            action_id: ingest::stable_id("corpus-action", &format!("skills:{}:{}", report.old_bundle_version, report.new_bundle_version)),
            action_type: "rebuild-skill-review".into(),
            target_type: "bundle".into(),
            target_id: report.new_bundle_id.clone(),
            title: "Skill set changed".into(),
            reason: "Skill inventory changed; rerun verifier review, readiness, and agent-pack generation.".into(),
            priority: "high".into(),
            requires_human_review: true,
        });
    }

    actions.sort_by(|a, b| a.action_id.cmp(&b.action_id));
    let recommended_version_bump = if !report.added_sources.is_empty()
        || !report.removed_sources.is_empty()
    {
        "minor"
    } else if report.source_set_changed || report.section_set_changed || report.skill_set_changed {
        "patch"
    } else {
        "none"
    };
    let action_hash_input = actions
        .iter()
        .map(|action| {
            format!(
                "{}:{}:{}:{}",
                action.action_id, action.action_type, action.target_type, action.target_id
            )
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        );

    CorpusLifecyclePlan {
        old_bundle_id: report.old_bundle_id.clone(),
        old_bundle_version: report.old_bundle_version.clone(),
        new_bundle_id: report.new_bundle_id.clone(),
        new_bundle_version: report.new_bundle_version.clone(),
        generated_at: Utc::now(),
        plan_id: ingest::stable_id(
            "corpus-plan",
            &format!(
                "{}:{}:{}:{}:{}",
                report.old_bundle_id,
                report.old_bundle_version,
                report.new_bundle_id,
                report.new_bundle_version,
                action_hash_input
            ),
        ),
        review_required: report.review_required
            || actions.iter().any(|action| action.requires_human_review),
        recommended_version_bump: recommended_version_bump.into(),
        actions,
        review_reasons: report.review_reasons.clone(),
    }
}

pub fn build_corpus_lifecycle_status(
    bundle: &SkillBundle,
    plan: &CorpusLifecyclePlan,
) -> CorpusLifecycleStatus {
    let validation = crate::registry::validate_bundle(bundle);
    let readiness = crate::registry::readiness_report(bundle);
    let matches_plan_target = bundle.package.bundle_id == plan.new_bundle_id
        && bundle.package.version == plan.new_bundle_version;
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    if !matches_plan_target {
        blockers.push(format!(
            "bundle {} {} does not match lifecycle plan target {} {}",
            bundle.package.bundle_id,
            bundle.package.version,
            plan.new_bundle_id,
            plan.new_bundle_version
        ));
    }
    if !validation.valid {
        blockers.push("bundle validation is failing".into());
        blockers.extend(validation.errors.clone());
    }
    if !readiness.ready {
        blockers.push("bundle is not publication-ready".into());
        blockers.extend(readiness.blockers.clone());
    }

    let human_review_required = plan.review_required
        || plan
            .actions
            .iter()
            .any(|action| action.requires_human_review);
    if human_review_required
        && !matches!(
            bundle.package.review_status,
            SkillStatus::Approved | SkillStatus::Published
        )
    {
        blockers.push(
            "lifecycle plan requires human review before publication or agent-pack default use"
                .into(),
        );
    }

    let critical_action_count = plan
        .actions
        .iter()
        .filter(|action| action.priority.eq_ignore_ascii_case("critical"))
        .count();
    let high_action_count = plan
        .actions
        .iter()
        .filter(|action| action.priority.eq_ignore_ascii_case("high"))
        .count();
    if critical_action_count > 0 {
        blockers.push(format!(
            "{} critical lifecycle action(s) require resolution",
            critical_action_count
        ));
    }
    if high_action_count > 0 {
        warnings.push(format!(
            "{} high-priority lifecycle action(s) should be reviewed",
            high_action_count
        ));
    }
    warnings.extend(readiness.warnings.clone());

    let lifecycle_ready = blockers.is_empty();
    CorpusLifecycleStatus {
        plan_id: plan.plan_id.clone(),
        bundle_id: bundle.package.bundle_id.clone(),
        bundle_version: bundle.package.version.clone(),
        matches_plan_target,
        validation_valid: validation.valid,
        readiness_ready: readiness.ready,
        action_count: plan.actions.len(),
        open_action_count: plan.actions.len(),
        critical_action_count,
        high_action_count,
        human_review_required,
        lifecycle_ready,
        blockers,
        warnings,
    }
}

pub fn write_corpus_status(bundle: &SkillBundle, plan_path: &Path, out: &Path) -> Result<()> {
    fs::create_dir_all(out)?;
    let plan: CorpusLifecyclePlan = serde_yaml::from_str(&fs::read_to_string(plan_path)?)?;
    let status = build_corpus_lifecycle_status(bundle, &plan);
    fs::write(
        out.join("corpus-status.yaml"),
        serde_yaml::to_string(&status)?,
    )?;
    fs::write(
        out.join("corpus-status.md"),
        corpus_status_markdown(&status),
    )?;
    Ok(())
}

fn corpus_status_markdown(status: &CorpusLifecycleStatus) -> String {
    let mut out = format!(
        "# Corpus Lifecycle Status: {} {}\n\n",
        status.bundle_id, status.bundle_version
    );
    out.push_str(&format!("- Plan ID: `{}`\n", status.plan_id));
    out.push_str(&format!(
        "- Matches plan target: {}\n",
        status.matches_plan_target
    ));
    out.push_str(&format!(
        "- Validation valid: {}\n",
        status.validation_valid
    ));
    out.push_str(&format!("- Readiness ready: {}\n", status.readiness_ready));
    out.push_str(&format!("- Lifecycle ready: {}\n", status.lifecycle_ready));
    out.push_str(&format!("- Action count: {}\n", status.action_count));
    out.push_str(&format!(
        "- Human review required: {}\n\n",
        status.human_review_required
    ));
    out.push_str("## Blockers\n\n");
    if status.blockers.is_empty() {
        out.push_str("- None\n");
    } else {
        for blocker in &status.blockers {
            out.push_str(&format!("- {}\n", blocker));
        }
    }
    out.push_str("\n## Warnings\n\n");
    if status.warnings.is_empty() {
        out.push_str("- None\n");
    } else {
        for warning in &status.warnings {
            out.push_str(&format!("- {}\n", warning));
        }
    }
    out
}

pub fn write_corpus_plan(diff_report: &Path, out: &Path) -> Result<()> {
    fs::create_dir_all(out)?;
    let report: CorpusChangeReport = serde_yaml::from_str(&fs::read_to_string(diff_report)?)?;
    let plan = build_corpus_lifecycle_plan(&report);
    fs::write(out.join("corpus-plan.yaml"), serde_yaml::to_string(&plan)?)?;
    fs::write(out.join("corpus-plan.md"), corpus_plan_markdown(&plan))?;
    Ok(())
}

fn corpus_plan_markdown(plan: &CorpusLifecyclePlan) -> String {
    let mut out = format!(
        "# Corpus Lifecycle Plan: {} {} → {} {}

",
        plan.old_bundle_id, plan.old_bundle_version, plan.new_bundle_id, plan.new_bundle_version
    );
    out.push_str(&format!(
        "- Plan ID: `{}`
",
        plan.plan_id
    ));
    out.push_str(&format!(
        "- Review required: {}
",
        plan.review_required
    ));
    out.push_str(&format!(
        "- Recommended version bump: {}
",
        plan.recommended_version_bump
    ));
    out.push_str(&format!(
        "- Action count: {}

",
        plan.actions.len()
    ));

    out.push_str(
        "## Review Reasons

",
    );
    if plan.review_reasons.is_empty() {
        out.push_str(
            "- No review-triggering changes detected.
",
        );
    } else {
        for reason in &plan.review_reasons {
            out.push_str(&format!(
                "- {}
",
                reason
            ));
        }
    }

    out.push_str(
        "
## Actions

",
    );
    if plan.actions.is_empty() {
        out.push_str(
            "- No lifecycle actions required.
",
        );
    } else {
        for action in &plan.actions {
            out.push_str(&format!(
                "- `{}` **{}** {} `{}` ({})
  - Priority: {}
  - Human review: {}
  - Reason: {}
",
                action.action_id,
                action.action_type,
                action.target_type,
                action.target_id,
                action.title,
                action.priority,
                action.requires_human_review,
                action.reason
            ));
        }
    }
    out
}

pub fn write_corpus_diff(old_manifest: &Path, new_manifest: &Path, out: &Path) -> Result<()> {
    fs::create_dir_all(out)?;
    let old: CorpusManifest = serde_yaml::from_str(&fs::read_to_string(old_manifest)?)?;
    let new: CorpusManifest = serde_yaml::from_str(&fs::read_to_string(new_manifest)?)?;
    let report = compare_corpus_manifests(&old, &new);
    fs::write(
        out.join("corpus-diff.yaml"),
        serde_yaml::to_string(&report)?,
    )?;
    fs::write(out.join("corpus-diff.md"), corpus_diff_markdown(&report))?;
    Ok(())
}

fn corpus_diff_markdown(report: &CorpusChangeReport) -> String {
    let mut out = format!(
        "# Corpus Diff: {} {} → {} {}\n\n",
        report.old_bundle_id,
        report.old_bundle_version,
        report.new_bundle_id,
        report.new_bundle_version
    );
    out.push_str(&format!(
        "- Source set changed: {}\n",
        report.source_set_changed
    ));
    out.push_str(&format!(
        "- Section set changed: {}\n",
        report.section_set_changed
    ));
    out.push_str(&format!(
        "- Skill set changed: {}\n",
        report.skill_set_changed
    ));
    out.push_str(&format!(
        "- Review required: {}\n\n",
        report.review_required
    ));
    out.push_str("## Review Reasons\n\n");
    if report.review_reasons.is_empty() {
        out.push_str("- No review-triggering changes detected.\n");
    } else {
        for reason in &report.review_reasons {
            out.push_str(&format!("- {}\n", reason));
        }
    }
    out.push_str("\n## Added Sources\n\n");
    for source in &report.added_sources {
        out.push_str(&format!(
            "- `{}` {} (version: {}, trust: {})\n",
            source.source_id,
            source.title,
            source.version.clone().unwrap_or_else(|| "unknown".into()),
            source.source_trust
        ));
    }
    out.push_str("\n## Removed Sources\n\n");
    for source in &report.removed_sources {
        out.push_str(&format!("- `{}` {}\n", source.source_id, source.title));
    }
    out.push_str("\n## Changed Sources\n\n");
    for change in &report.changed_sources {
        out.push_str(&format!(
            "- `{}` {} fields: {}\n",
            change.source_id,
            change.title,
            change.changed_fields.join(", ")
        ));
        if change.old_version != change.new_version {
            out.push_str(&format!(
                "  - version: {} → {}\n",
                change
                    .old_version
                    .clone()
                    .unwrap_or_else(|| "unknown".into()),
                change
                    .new_version
                    .clone()
                    .unwrap_or_else(|| "unknown".into())
            ));
        }
        if change.old_hash != change.new_hash {
            out.push_str("  - content hash changed\n");
        }
    }
    out
}

pub fn build_corpus_map(bundle: &SkillBundle) -> CorpusMap {
    let mut source_clusters: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for source in &bundle.sources {
        source_clusters
            .entry(format!("{:?}", source.source_type))
            .or_default()
            .push(source.source_id.clone());
    }

    let mut skill_clusters: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for skill in &bundle.skills {
        skill_clusters
            .entry(format!("{:?}", skill.skill_type))
            .or_default()
            .push(skill.id.clone());
    }

    let source_trust_summary = source_trust_summary(bundle);
    let domain_profile = bundle
        .package
        .compatibility
        .get("domain_profile")
        .and_then(|name| domain::get_profile(name))
        .map(|profile| DomainProfileSummary {
            name: profile.name,
            preferred_agent_roles: profile.preferred_agent_roles,
            known_tools: profile.known_tools,
            required_review_policy: profile.required_review_policy,
        });

    let mut missing_area_hints = Vec::new();
    if bundle.skills.iter().all(|s| s.tool_requirements.is_empty()) {
        missing_area_hints.push("No tool requirements detected; add CLI/API/tool references if operational agents are expected.".into());
    }
    if bundle
        .skills
        .iter()
        .all(|s| s.version_applicability.supported_versions.is_empty())
    {
        missing_area_hints.push(
            "No supported versions detected; add versioned sources or domain profile metadata."
                .into(),
        );
    }
    if bundle.skills.iter().all(|s| s.inference_records.is_empty()) {
        missing_area_hints
            .push("No inference records found; run forge/infer before agent packaging.".into());
    }

    CorpusMap {
        bundle_id: bundle.package.bundle_id.clone(),
        domain: bundle.package.domain.clone(),
        source_count: bundle.sources.len(),
        section_count: bundle.sections.len(),
        skill_count: bundle.skills.len(),
        candidate_count: bundle.capability_candidates.len(),
        concepts: bundle
            .graph
            .concepts
            .iter()
            .map(|c| CorpusConcept {
                name: c.concept.clone(),
                skill_ids: c.skill_ids.clone(),
                section_ids: c.source_section_ids.clone(),
            })
            .collect(),
        source_clusters,
        skill_clusters,
        source_trust_summary,
        domain_profile,
        missing_area_hints,
    }
}

fn source_trust_summary(bundle: &SkillBundle) -> BTreeMap<String, usize> {
    let mut summary = BTreeMap::new();
    for source in &bundle.sources {
        *summary
            .entry(format!("{:?}", source_meta::infer_source_trust(source)))
            .or_insert(0) += 1;
    }
    summary
}

pub fn write_corpus_map(bundle: &SkillBundle, out: &Path) -> Result<()> {
    fs::create_dir_all(out)?;
    let map = build_corpus_map(bundle);
    fs::write(out.join("corpus-map.yaml"), serde_yaml::to_string(&map)?)?;
    fs::write(out.join("corpus-map.md"), corpus_map_markdown(&map))?;
    Ok(())
}

fn corpus_map_markdown(map: &CorpusMap) -> String {
    let mut out = format!("# Corpus Map: {}\n\n", map.bundle_id);
    out.push_str(&format!(
        "Sources: {}  Sections: {}  Candidates: {}  Skills: {}\n\n",
        map.source_count, map.section_count, map.candidate_count, map.skill_count
    ));
    if let Some(profile) = &map.domain_profile {
        out.push_str("## Domain Profile\n\n");
        out.push_str(&format!("- Name: {}\n", profile.name));
        out.push_str(&format!(
            "- Preferred roles: {}\n",
            profile.preferred_agent_roles.join(", ")
        ));
        out.push_str(&format!(
            "- Known tools: {}\n",
            profile.known_tools.join(", ")
        ));
        out.push_str(&format!(
            "- Review policy: {}\n\n",
            profile.required_review_policy
        ));
    }
    out.push_str("## Source Trust Summary\n\n");
    for (trust, count) in &map.source_trust_summary {
        out.push_str(&format!("- {}: {} sources\n", trust, count));
    }
    out.push_str("\n## Skill Clusters\n\n");
    for (kind, ids) in &map.skill_clusters {
        out.push_str(&format!("- {}: {} skills\n", kind, ids.len()));
    }
    out.push_str("\n## Missing Area Hints\n\n");
    for hint in &map.missing_area_hints {
        out.push_str(&format!("- {}\n", hint));
    }
    out
}

pub fn bump_bundle_version(mut bundle: SkillBundle, new_version: Option<&str>) -> SkillBundle {
    let next = new_version
        .map(str::to_string)
        .unwrap_or_else(|| bump_patch(&bundle.package.version));
    bundle.package.version = next.clone();
    bundle.package.review_status = SkillStatus::NeedsReview;
    bundle.package.publish_status = PublishStatus::Unpublished;
    for skill in &mut bundle.skills {
        skill.status = SkillStatus::NeedsReview;
        skill.maturity = SkillMaturity::Level1StructuredCandidate;
        skill.confidence.human_review = 0.0;
        skill.confidence.runtime = 0.0;
        skill.metadata.remove("published_at");
        skill.metadata.remove("approved_by");
        skill
            .metadata
            .insert("staged_bundle_version".into(), next.clone());
        skill.metadata.insert(
            "version_bump_reason".into(),
            "staged for review after bundle change".into(),
        );
        skill.guardrails.push(
            "Version-bumped skill requires revalidation before publication or agent-pack default use.".into(),
        );
    }
    bundle.audit_events.push(AuditEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: "version_bump".into(),
        message: format!("bundle version staged as {}", next),
        created_at: chrono::Utc::now(),
        metadata: BTreeMap::new(),
    });
    bundle
}

fn bump_patch(version: &str) -> String {
    let mut parts: Vec<u64> = version
        .split('.')
        .map(|p| p.parse::<u64>().unwrap_or(0))
        .collect();
    while parts.len() < 3 {
        parts.push(0);
    }
    parts[2] += 1;
    format!("{}.{}.{}", parts[0], parts[1], parts[2])
}

pub fn domain_template(name: &str) -> DomainProfile {
    let mut profile = crate::domain::get_profile(name).unwrap_or_else(|| DomainProfile {
        name: name.into(),
        preferred_skill_types: vec![SkillType::Procedure, SkillType::Diagnostic],
        known_tools: vec![],
        risk_categories: vec!["mutation".into(), "version-specific".into()],
        common_task_types: vec!["diagnose".into(), "review".into(), "operate".into()],
        common_anti_patterns: vec![],
        preferred_agent_roles: vec!["Domain Specialist Agent".into()],
        source_trust_hierarchy: vec![SourceTrust::UnknownSource],
        terminology: vec![],
        required_review_policy:
            "Require review for operational, mutating, inferred, or low-confidence skills.".into(),
    });
    if profile.known_tools.is_empty() {
        profile.known_tools = vec!["replace-with-domain-tool".into()];
    }
    profile
}

pub fn candidate_from_section(section: &DocumentSection) -> CapabilityCandidate {
    let candidate_type = if !section.detected_api_operations.is_empty() {
        SkillType::ApiOperation
    } else if !section.detected_commands.is_empty() {
        SkillType::CliOperation
    } else if section.heading.to_lowercase().contains("diagnos") {
        SkillType::Diagnostic
    } else {
        SkillType::Procedure
    };
    CapabilityCandidate {
        candidate_id: ingest::stable_id("cand", &section.section_id),
        source_section_ids: vec![section.section_id.clone()],
        candidate_title: section.heading.clone(),
        candidate_type,
        detected_task: format!("Apply source section '{}'", section.heading),
        detected_inputs: vec!["User goal".into(), "Target environment/version".into()],
        detected_outputs: vec!["Source-grounded guidance".into()],
        detected_procedures: section.detected_normative_language.clone(),
        detected_warnings: section.detected_warnings.clone(),
        candidate_confidence: if section.detected_normative_language.is_empty() {
            0.45
        } else {
            0.65
        },
        evidence_strength: if section.detected_normative_language.is_empty() {
            0.45
        } else {
            0.7
        },
        extraction_type: EvidenceClass::DirectExtraction,
        related_candidates: vec![],
    }
}
