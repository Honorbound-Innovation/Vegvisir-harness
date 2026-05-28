use crate::ingest;
use crate::models::*;
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
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
    pub missing_area_hints: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CorpusConcept {
    pub name: String,
    pub skill_ids: Vec<String>,
    pub section_ids: Vec<String>,
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
        missing_area_hints,
    }
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
    out.push_str("## Skill Clusters\n\n");
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
        skill
            .metadata
            .insert("staged_bundle_version".into(), next.clone());
        skill.metadata.insert(
            "version_bump_reason".into(),
            "staged for review after bundle change".into(),
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
