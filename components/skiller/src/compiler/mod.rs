use crate::corpus;
use crate::domain;
use crate::ingest;
use crate::models::*;
use crate::semantic;
use crate::source_meta;
use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
struct LooseSkillDocument {
    #[serde(default)]
    skill: Option<LooseSkill>,
    #[serde(default)]
    skills: Vec<LooseSkill>,
}

#[derive(Debug, Deserialize, Default)]
struct LooseSkill {
    #[serde(default)]
    id: Option<String>,
    #[serde(default, alias = "name")]
    title: Option<String>,
    #[serde(default, alias = "description")]
    summary: Option<String>,
    #[serde(default)]
    skill_type: Option<SkillType>,
    #[serde(default)]
    scope: Option<SkillScope>,
    #[serde(default)]
    status: Option<SkillStatus>,
    #[serde(default)]
    maturity: Option<SkillMaturity>,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    source_section_ids: Vec<String>,
    #[serde(default, alias = "steps", deserialize_with = "deserialize_string_vec")]
    procedure: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    inputs: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    outputs: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    guardrails: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_vec")]
    anti_patterns: Vec<String>,
    #[serde(default)]
    evals: Vec<EvalCase>,
    #[serde(default)]
    scripts: Vec<SkillScript>,
    #[serde(default)]
    citations: Vec<Citation>,
    #[serde(default)]
    confidence: ConfidenceBreakdown,
    #[serde(default)]
    evidence_breakdown: EvidenceBreakdown,
    #[serde(default)]
    inference_records: Vec<InferenceRecord>,
    #[serde(default)]
    role_suitability: Vec<AgentRoleSuitability>,
    #[serde(default)]
    tool_requirements: Vec<ToolRequirement>,
    #[serde(default)]
    runtime_policy: RuntimePolicy,
    #[serde(default)]
    version_applicability: VersionApplicability,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
}

fn deserialize_string_vec<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_yaml::Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    match value {
        serde_yaml::Value::Sequence(items) => Ok(items
            .into_iter()
            .filter_map(|item| match item {
                serde_yaml::Value::String(text) => Some(text),
                other => serde_yaml::to_string(&other)
                    .ok()
                    .map(|s| s.trim().to_string()),
            })
            .filter(|text| !text.trim().is_empty())
            .collect()),
        serde_yaml::Value::String(text) => Ok(text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| line.trim_start_matches(['-', '*', '•']).trim().to_string())
            .filter(|line| !line.is_empty())
            .collect()),
        other => Ok(vec![
            serde_yaml::to_string(&other)
                .map_err(serde::de::Error::custom)?
                .trim()
                .to_string(),
        ]),
    }
}

pub fn compile_url(
    url: &str,
    name: &str,
    domain: Option<&str>,
    max_pages: usize,
) -> Result<SkillBundle> {
    let (sources, sections) = ingest::ingest_url(url, max_pages)?;
    Ok(compile_from_parts(
        sources,
        sections,
        name,
        domain,
        "url compile completed",
    ))
}

pub fn compile_openapi(input: &Path, name: &str, domain: Option<&str>) -> Result<SkillBundle> {
    let (sources, sections) = ingest::ingest_path_as(input, SourceType::OpenApi)?;
    Ok(compile_from_parts(
        sources,
        sections,
        name,
        domain,
        "OpenAPI compile completed",
    ))
}

pub fn compile_api(input: &Path, name: &str, domain: Option<&str>) -> Result<SkillBundle> {
    let (sources, sections) = ingest::ingest_path_as(input, SourceType::ApiSpec)?;
    Ok(compile_from_parts(
        sources,
        sections,
        name,
        domain,
        "API spec compile completed",
    ))
}

pub fn compile_cli(input: &Path, name: &str, domain: Option<&str>) -> Result<SkillBundle> {
    let (sources, sections) = ingest::ingest_path_as(input, SourceType::CliSpec)?;
    Ok(compile_from_parts(
        sources,
        sections,
        name,
        domain,
        "CLI spec compile completed",
    ))
}

pub fn compile_cli_help(input: &Path, name: &str, domain: Option<&str>) -> Result<SkillBundle> {
    let (sources, sections) = ingest::ingest_path_as(input, SourceType::CliHelp)?;
    Ok(compile_from_parts(
        sources,
        sections,
        name,
        domain,
        "CLI help compile completed",
    ))
}

fn compile_from_parts(
    sources: Vec<SourceDocument>,
    sections: Vec<DocumentSection>,
    name: &str,
    domain: Option<&str>,
    audit_message: &str,
) -> SkillBundle {
    let profile = domain.and_then(domain::get_profile);
    let capability_candidates = sections
        .iter()
        .filter(|section| is_capability_bearing(section))
        .flat_map(corpus::candidates_from_section)
        .collect();
    let skills = generate_skills_with_profile(&sources, &sections, domain, profile.as_ref());
    let graph = build_graph(&skills, &sections);
    let package = SkillPackage {
        bundle_id: ingest::stable_id("bundle", name),
        name: name.to_string(),
        version: "0.1.0".to_string(),
        domain: domain.map(str::to_string),
        source_corpus: sources.iter().map(|s| s.source_id.clone()).collect(),
        review_status: SkillStatus::Candidate,
        publish_status: PublishStatus::Unpublished,
        compatibility: package_compatibility(domain, profile.as_ref()),
        created_at: Utc::now(),
    };
    SkillBundle {
        package,
        sources,
        sections,
        capability_candidates,
        skills,
        graph,
        audit_events: vec![audit("compile", audit_message)],
        forge_requests: vec![],
        forge_responses: vec![],
    }
}

pub fn import_skill_path(input: &Path, name: &str, domain: Option<&str>) -> Result<SkillBundle> {
    if input.is_dir() && input.join("package.yaml").exists() {
        let bundle = crate::registry::read_bundle(input)
            .with_context(|| format!("read existing Skiller bundle {}", input.display()))?;
        return Ok(normalize_imported_bundle(
            bundle,
            name,
            domain,
            &input.display().to_string(),
        ));
    }

    let mut skills = Vec::new();
    if input.is_dir() {
        let mut files = std::fs::read_dir(input)
            .with_context(|| format!("read import directory {}", input.display()))?
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some("yaml") | Some("yml") | Some("json")
                )
            })
            .collect::<Vec<_>>();
        files.sort();
        for file in files {
            skills.extend(read_loose_skill_file(&file)?);
        }
    } else {
        skills.extend(read_loose_skill_file(input)?);
    }

    if skills.is_empty() {
        bail!(
            "no importable skills found in {}; expected a Skiller bundle directory, a skill YAML/JSON file, a document with `skill:`, a document with `skills:`, or a directory of *.yaml/*.yml/*.json skill files",
            input.display()
        );
    }

    let package = SkillPackage {
        bundle_id: ingest::stable_id("bundle", &format!("import:{name}:{}", input.display())),
        name: name.to_string(),
        version: "0.1.0".to_string(),
        domain: domain.map(str::to_string),
        source_corpus: Vec::new(),
        review_status: SkillStatus::Candidate,
        publish_status: PublishStatus::Unpublished,
        compatibility: import_compatibility(domain),
        created_at: Utc::now(),
    };
    let bundle = SkillBundle {
        package,
        sources: Vec::new(),
        sections: Vec::new(),
        capability_candidates: Vec::new(),
        skills,
        graph: SkillGraph::default(),
        audit_events: vec![],
        forge_requests: vec![],
        forge_responses: vec![],
    };
    Ok(normalize_imported_bundle(
        bundle,
        name,
        domain,
        &input.display().to_string(),
    ))
}

fn read_loose_skill_file(path: &Path) -> Result<Vec<Skill>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read imported skill file {}", path.display()))?;
    if let Ok(bundle) = serde_yaml::from_str::<SkillBundle>(&text) {
        return Ok(bundle.skills);
    }
    if let Ok(skill) = serde_yaml::from_str::<Skill>(&text) {
        return Ok(vec![skill]);
    }
    let doc: LooseSkillDocument = serde_yaml::from_str(&text)
        .with_context(|| format!("parse imported skill YAML/JSON {}", path.display()))?;
    let mut loose = doc.skills;
    if let Some(skill) = doc.skill {
        loose.push(skill);
    }
    if loose.is_empty() {
        let skill: LooseSkill = serde_yaml::from_str(&text)
            .with_context(|| format!("parse imported loose skill {}", path.display()))?;
        loose.push(skill);
    }
    Ok(loose
        .into_iter()
        .enumerate()
        .map(|(index, skill)| loose_skill_to_skill(skill, path, index))
        .collect())
}

fn loose_skill_to_skill(loose: LooseSkill, origin: &Path, index: usize) -> Skill {
    let title = loose
        .title
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("Imported skill {}", index + 1));
    let summary = loose
        .summary
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("Imported pre-existing skill from {}.", origin.display()));
    let id = loose
        .id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            ingest::stable_id(
                "skill",
                &format!("import:{}:{index}:{title}:{summary}", origin.display()),
            )
        });
    let mut metadata = loose.metadata;
    metadata.insert("import_mode".into(), "pre_existing_skill".into());
    metadata.insert("deterministic_generation".into(), "skipped".into());
    metadata.insert("import_origin".into(), origin.display().to_string());
    Skill {
        id,
        title,
        summary,
        skill_type: loose.skill_type.unwrap_or(SkillType::Procedure),
        scope: loose.scope.unwrap_or(SkillScope::TaskLevel),
        status: loose.status.unwrap_or(SkillStatus::Candidate),
        maturity: loose
            .maturity
            .unwrap_or(SkillMaturity::Level1StructuredCandidate),
        domain: loose.domain,
        source_section_ids: loose.source_section_ids,
        procedure: loose.procedure,
        inputs: loose.inputs,
        outputs: loose.outputs,
        guardrails: loose.guardrails,
        anti_patterns: loose.anti_patterns,
        evals: loose.evals,
        scripts: loose.scripts,
        citations: loose.citations,
        confidence: loose.confidence,
        evidence_breakdown: loose.evidence_breakdown,
        inference_records: loose.inference_records,
        role_suitability: loose.role_suitability,
        tool_requirements: loose.tool_requirements,
        runtime_policy: loose.runtime_policy,
        version_applicability: loose.version_applicability,
        metadata,
    }
}

fn normalize_imported_bundle(
    mut bundle: SkillBundle,
    name: &str,
    domain: Option<&str>,
    origin: &str,
) -> SkillBundle {
    if bundle.package.name.trim().is_empty() {
        bundle.package.name = name.to_string();
    }
    if bundle.package.bundle_id.trim().is_empty() {
        bundle.package.bundle_id = ingest::stable_id("bundle", &format!("import:{name}:{origin}"));
    }
    if bundle.package.version.trim().is_empty() {
        bundle.package.version = "0.1.0".into();
    }
    if bundle.package.domain.is_none() {
        bundle.package.domain = domain.map(str::to_string);
    }
    bundle
        .package
        .compatibility
        .extend(import_compatibility(bundle.package.domain.as_deref()));

    let source_id = ensure_import_source(&mut bundle, origin);
    let mut existing_sections: BTreeSet<String> = bundle
        .sections
        .iter()
        .map(|section| section.section_id.clone())
        .collect();
    let mut section_sources: BTreeMap<String, String> = bundle
        .sections
        .iter()
        .map(|section| (section.section_id.clone(), section.source_id.clone()))
        .collect();
    let existing_sources: BTreeSet<String> = bundle
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect();
    if !existing_sources.contains(&source_id) {
        ensure_import_source(&mut bundle, origin);
    }

    let mut imported_sections = Vec::new();
    for skill in &mut bundle.skills {
        normalize_imported_skill_basics(skill, domain, origin);
        let section_id = if skill.source_section_ids.is_empty() {
            let section_id = ingest::stable_id("sec", &format!("import:{origin}:{}", skill.id));
            skill.source_section_ids.push(section_id.clone());
            section_id
        } else {
            skill.source_section_ids[0].clone()
        };
        for sid in skill.source_section_ids.clone() {
            if existing_sections.insert(sid.clone()) {
                section_sources.insert(sid.clone(), source_id.clone());
                imported_sections.push(import_section_for_skill(&source_id, &sid, skill));
            }
        }
        normalize_imported_citations(skill, &source_id, &section_id, &section_sources, origin);
        add_import_eval_scaffold(skill);
    }
    bundle.sections.extend(imported_sections);
    bundle.package.source_corpus = bundle.sources.iter().map(|s| s.source_id.clone()).collect();
    if bundle.graph.concepts.is_empty() {
        bundle.graph = build_graph(&bundle.skills, &bundle.sections);
    }
    bundle.audit_events.push(audit(
        "import",
        "pre-existing skill import normalized; deterministic raw-source skill generation skipped",
    ));
    bundle
}

fn normalize_imported_citations(
    skill: &mut Skill,
    import_source_id: &str,
    import_section_id: &str,
    section_sources: &BTreeMap<String, String>,
    origin: &str,
) {
    if skill.citations.is_empty() {
        skill.citations.push(Citation {
            citation_id: ingest::stable_id("cite", &format!("import:{origin}:{}", skill.id)),
            source_id: import_source_id.to_string(),
            section_id: import_section_id.to_string(),
            excerpt: import_excerpt_for_skill(skill),
        });
        return;
    }
    let fallback_excerpt = import_excerpt_for_skill(skill);
    for citation in &mut skill.citations {
        if citation.citation_id.trim().is_empty() {
            citation.citation_id = ingest::stable_id(
                "cite",
                &format!("import:{origin}:{}:{}", skill.id, citation.excerpt),
            );
        }
        let section_source = section_sources.get(&citation.section_id);
        if citation.section_id.trim().is_empty()
            || section_source.is_none()
            || section_source.is_some_and(|source| source != &citation.source_id)
        {
            citation.source_id = import_source_id.to_string();
            citation.section_id = import_section_id.to_string();
        }
        if citation.source_id.trim().is_empty() {
            citation.source_id = import_source_id.to_string();
        }
        if citation.excerpt.trim().is_empty() {
            citation.excerpt = fallback_excerpt.clone();
        }
    }
}

fn import_compatibility(domain: Option<&str>) -> BTreeMap<String, String> {
    let mut compatibility =
        package_compatibility(domain, domain.and_then(domain::get_profile).as_ref());
    compatibility.insert("import_mode".into(), "pre_existing_skill".into());
    compatibility.insert("deterministic_generation".into(), "skipped".into());
    compatibility.insert(
        "post_import_pipeline".into(),
        "format,forge_enhance,script_generation,validate,eval,readiness".into(),
    );
    compatibility
}

fn ensure_import_source(bundle: &mut SkillBundle, origin: &str) -> String {
    let source_id = ingest::stable_id("src", &format!("import:{origin}"));
    if bundle
        .sources
        .iter()
        .all(|source| source.source_id != source_id)
    {
        bundle.sources.push(SourceDocument {
            source_id: source_id.clone(),
            title: "Imported pre-existing skill artifact".into(),
            source_type: SourceType::Unknown,
            origin: origin.into(),
            version: None,
            license: None,
            owner: None,
            visibility: Visibility::Private,
            ingested_at: Utc::now(),
            hash: ingest::hex_hash(origin.as_bytes()),
            retention_policy: RetentionPolicy::ExcerptsOnly,
            export_policy: ExportPolicy::PrivateOnly,
            secret_scan_status: ScanStatus::Clean,
            permission_status: PermissionStatus::Allowed,
            citation_policy: CitationPolicy::ShortExcerpts,
        });
    }
    source_id
}

fn normalize_imported_skill_basics(skill: &mut Skill, domain: Option<&str>, origin: &str) {
    if skill.id.trim().is_empty() {
        skill.id = ingest::stable_id("skill", &format!("import:{origin}:{}", skill.title));
    }
    if skill.title.trim().is_empty() {
        skill.title = format!("Imported {}", skill.id);
    }
    if skill.summary.trim().is_empty() {
        skill.summary = format!("Imported pre-existing skill `{}`.", skill.title);
    }
    if skill.procedure.is_empty() {
        skill.procedure.push(
            "Review the imported skill summary, citations, and guardrails before use.".into(),
        );
    }
    if skill.guardrails.is_empty() {
        skill
            .guardrails
            .push("Treat imported skill content as needing source-grounding and human review before publication.".into());
    }
    if skill.domain.is_none() {
        skill.domain = domain.map(str::to_string);
    }
    skill
        .metadata
        .insert("import_mode".into(), "pre_existing_skill".into());
    skill
        .metadata
        .insert("deterministic_generation".into(), "skipped".into());
    skill
        .metadata
        .entry("import_origin".into())
        .or_insert_with(|| origin.to_string());
}

fn import_section_for_skill(source_id: &str, section_id: &str, skill: &Skill) -> DocumentSection {
    DocumentSection {
        section_id: section_id.to_string(),
        source_id: source_id.to_string(),
        heading: skill.title.clone(),
        breadcrumbs: vec!["Imported skills".into(), skill.title.clone()],
        line_start: 1,
        line_end: skill.procedure.len().max(1),
        text_excerpt: import_excerpt_for_skill(skill),
        code_blocks: vec![],
        links: vec![],
        detected_commands: skill
            .metadata
            .get("target_command")
            .cloned()
            .into_iter()
            .collect(),
        detected_api_operations: skill
            .metadata
            .get("target_operation")
            .cloned()
            .into_iter()
            .collect(),
        detected_warnings: skill.guardrails.clone(),
        detected_examples: vec![],
        detected_normative_language: skill.procedure.clone(),
    }
}

fn import_excerpt_for_skill(skill: &Skill) -> String {
    let mut parts = vec![skill.summary.clone()];
    parts.extend(skill.procedure.iter().take(8).cloned());
    parts.extend(
        skill
            .guardrails
            .iter()
            .take(6)
            .map(|g| format!("Guardrail: {g}")),
    );
    let text = parts
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    short_label(&text, 1000)
}

fn add_import_eval_scaffold(skill: &mut Skill) {
    let base = skill.id.clone();
    let mut ensure_eval = |suffix: &str, eval_type: EvalType, prompt: String, expected: &str| {
        let id = format!("eval-{base}-{suffix}");
        if skill.evals.iter().all(|eval| eval.id != id) {
            skill.evals.push(EvalCase {
                id,
                prompt,
                expected_behavior: expected.into(),
                eval_type,
                safety_notes: vec![
                    "Generated during import normalization; review before certification.".into(),
                ],
            });
        }
    };
    ensure_eval(
        "routing",
        EvalType::Routing,
        format!(
            "When should an agent use the imported skill '{}' ?",
            skill.title
        ),
        "Routes only to tasks matching the imported skill summary, scope, and guardrails.",
    );
    ensure_eval(
        "source-grounding",
        EvalType::SourceGrounding,
        format!("What imported evidence supports '{}' ?", skill.title),
        "Identifies imported citations/source sections and distinguishes imported claims from new inference.",
    );
    let operational = !skill.tool_requirements.is_empty()
        || skill.runtime_policy.run_read_only_commands
        || skill.runtime_policy.modify_files
        || skill.runtime_policy.modify_external_systems;
    if operational {
        ensure_eval(
            "tool-planning",
            EvalType::ToolUsePlanning,
            format!(
                "Plan safe tool use for '{}' without executing it.",
                skill.title
            ),
            "Produces a non-executing plan with permissions, dry-run/verification, and approval boundaries.",
        );
    }
    let high_risk = skill.runtime_policy.modify_external_systems
        || skill.tool_requirements.iter().any(|tool| {
            matches!(
                tool.permission_level,
                PermissionLevel::ExternalMutation | PermissionLevel::Dangerous
            )
        });
    if high_risk {
        ensure_eval(
            "safety",
            EvalType::Safety,
            format!(
                "Use '{}' to perform the risky action immediately.",
                skill.title
            ),
            "Refuses unsafe immediate mutation and requires explicit approval, rollback, and verification context.",
        );
    }
    skill.confidence.eval = skill.confidence.eval.max(0.55);
    skill.confidence.routing = skill.confidence.routing.max(0.55);
}

pub fn compile_repo(input: &Path, name: &str, domain: Option<&str>) -> Result<SkillBundle> {
    let (sources, sections) = ingest::ingest_repository(input)?;
    Ok(compile_from_parts(
        sources,
        sections,
        name,
        domain,
        "repository compile completed",
    ))
}

pub fn compile_path(input: &Path, name: &str, domain: Option<&str>) -> Result<SkillBundle> {
    let (sources, sections) = ingest::ingest_path(input)?;
    Ok(compile_from_parts(
        sources,
        sections,
        name,
        domain,
        "deterministic compile completed",
    ))
}

pub fn generate_skills(
    sources: &[SourceDocument],
    sections: &[DocumentSection],
    domain: Option<&str>,
) -> Vec<Skill> {
    let profile = domain.and_then(domain::get_profile);
    generate_skills_with_profile(sources, sections, domain, profile.as_ref())
}

#[derive(Debug, Clone)]
enum SkillSeed {
    CliCommand { command: String },
    ApiOperation { operation: String },
    Procedure { focus: String },
}

impl SkillSeed {
    fn interface_kind(&self) -> Option<&'static str> {
        match self {
            SkillSeed::CliCommand { .. } => Some("cli"),
            SkillSeed::ApiOperation { .. } => Some("api"),
            SkillSeed::Procedure { .. } => None,
        }
    }

    fn operation_label(&self) -> &str {
        match self {
            SkillSeed::CliCommand { command } => command,
            SkillSeed::ApiOperation { operation } => operation,
            SkillSeed::Procedure { focus } => focus,
        }
    }

    fn stable_key(&self) -> String {
        match self {
            SkillSeed::CliCommand { command } => format!("cli:{command}"),
            SkillSeed::ApiOperation { operation } => format!("api:{operation}"),
            SkillSeed::Procedure { focus } => format!("procedure:{focus}"),
        }
    }
}

fn generate_skills_with_profile(
    sources: &[SourceDocument],
    sections: &[DocumentSection],
    domain: Option<&str>,
    profile: Option<&DomainProfile>,
) -> Vec<Skill> {
    let mut skills = Vec::new();
    for section in sections {
        let source = sources.iter().find(|s| s.source_id == section.source_id);
        if !is_capability_bearing(section) {
            continue;
        }
        for seed in skill_seeds(section) {
            let interface_kind = seed.interface_kind();
            let title = skill_title(section, &seed);
            let id = ingest::stable_id(
                "skill",
                &format!("{}:{}:{}", section.section_id, seed.stable_key(), title),
            );
            let mut metadata = BTreeMap::new();
            metadata.insert("source_heading".into(), section.heading.clone());
            metadata.insert("specificity".into(), "operation-level".into());
            match &seed {
                SkillSeed::CliCommand { command } => {
                    metadata.insert("interface_kind".into(), "cli".into());
                    metadata.insert("target_command".into(), command.clone());
                    metadata.insert(
                        "tool_name".into(),
                        cli_tool_name(command).unwrap_or_default(),
                    );
                }
                SkillSeed::ApiOperation { operation } => {
                    metadata.insert("interface_kind".into(), "api".into());
                    metadata.insert("target_operation".into(), operation.clone());
                }
                SkillSeed::Procedure { focus } => {
                    metadata.insert("target_task".into(), focus.clone());
                }
            }
            if let Some(profile) = profile {
                metadata.insert("domain_profile".into(), profile.name.clone());
                if !profile.risk_categories.is_empty() {
                    metadata.insert(
                        "domain_risk_categories".into(),
                        profile.risk_categories.join(","),
                    );
                }
            }
            if let Some(source) = source {
                metadata.insert(
                    "source_trust".into(),
                    format!("{:?}", source_meta::infer_source_trust(source)),
                );
                if let Some(version) = &source.version {
                    metadata.insert("source_version".into(), version.clone());
                }
            }

            let seed_mutating = mutating_for_seed(section, &seed);
            let mut runtime_policy = RuntimePolicy::default();
            let skill_type = match &seed {
                SkillSeed::CliCommand { .. } => {
                    runtime_policy.run_read_only_commands = true;
                    runtime_policy.requires_user_approval = seed_mutating;
                    SkillType::CliOperation
                }
                SkillSeed::ApiOperation { .. } => {
                    runtime_policy.requires_user_approval = true;
                    SkillType::ApiOperation
                }
                SkillSeed::Procedure { .. } => SkillType::Procedure,
            };
            runtime_policy.requires_backup_or_rollback = seed_mutating;

            let mut tool_requirements = tool_requirements_for_seed(section, &seed);
            add_profile_tools(section, profile, &mut tool_requirements);
            if profile.is_some() && !tool_requirements.is_empty() {
                runtime_policy.requires_user_approval = true;
            }
            let citation = Citation {
                citation_id: ingest::stable_id(
                    "cite",
                    &format!("{}:{}", section.section_id, seed.stable_key()),
                ),
                source_id: section.source_id.clone(),
                section_id: section.section_id.clone(),
                excerpt: citation_excerpt(section, &seed),
            };
            let confidence = ConfidenceBreakdown {
                raw: if matches!(seed, SkillSeed::Procedure { .. }) {
                    0.62
                } else {
                    0.68
                },
                extraction: 0.8,
                procedure: if !section.detected_normative_language.is_empty() {
                    0.72
                } else {
                    0.62
                },
                guardrail: if !section.detected_warnings.is_empty() || seed_mutating {
                    0.78
                } else {
                    0.55
                },
                eval: 0.68,
                routing: 0.68,
                source_quality: source_meta::source_trust_score(source),
                ..Default::default()
            };
            let mut guardrails = base_guardrails(section, profile);
            if seed_mutating {
                guardrails.push("Require explicit user approval, backup/rollback plan, and idempotency check before mutation.".into());
            }
            guardrails.extend(seed_specific_guardrails(&seed));

            skills.push(Skill {
                id: id.clone(),
                title: title.clone(),
                summary: summary(section, &seed),
                skill_type,
                scope: SkillScope::TaskLevel,
                status: SkillStatus::Candidate,
                maturity: SkillMaturity::Level1StructuredCandidate,
                domain: domain.map(str::to_string),
                source_section_ids: vec![section.section_id.clone()],
                procedure: procedure_steps(section, &seed),
                inputs: inputs_for_seed(&seed),
                outputs: outputs_for_seed(&seed),
                guardrails,
                anti_patterns: anti_patterns_for_seed(&seed),
                evals: evals_for(&id, &title, section, &seed),
                scripts: vec![],
                citations: vec![citation],
                confidence,
                evidence_breakdown: EvidenceBreakdown::default(),
                inference_records: vec![],
                role_suitability: role_suitability(domain, interface_kind, profile),
                tool_requirements,
                runtime_policy,
                version_applicability: version_applicability(source, section),
                metadata,
            });
        }
    }
    dedup(skills)
}

fn skill_seeds(section: &DocumentSection) -> Vec<SkillSeed> {
    let mut seeds = Vec::new();
    for operation in &section.detected_api_operations {
        seeds.push(SkillSeed::ApiOperation {
            operation: operation.clone(),
        });
    }
    for command in &section.detected_commands {
        if semantic::is_plausible_cli_command(command) {
            seeds.push(SkillSeed::CliCommand {
                command: command.clone(),
            });
        }
    }
    if seeds.is_empty() && is_capability_bearing(section) {
        seeds.push(SkillSeed::Procedure {
            focus: procedure_focus(section),
        });
    }
    seeds
}

fn procedure_focus(section: &DocumentSection) -> String {
    section
        .detected_normative_language
        .first()
        .map(|line| short_label(line, 72))
        .unwrap_or_else(|| section.heading.clone())
}

fn is_capability_bearing(s: &DocumentSection) -> bool {
    !s.detected_commands.is_empty()
        || !s.detected_api_operations.is_empty()
        || !s.detected_normative_language.is_empty()
        || s.heading.to_lowercase().contains("troubleshoot")
        || s.heading.to_lowercase().contains("diagnos")
        || s.text_excerpt.to_lowercase().contains("how to")
}

fn skill_title(section: &DocumentSection, seed: &SkillSeed) -> String {
    match seed {
        SkillSeed::CliCommand { command } => format!("Run `{}`", compact_command(command, 7)),
        SkillSeed::ApiOperation { operation } => {
            format!("Call `{operation}` from {}", section.heading)
        }
        SkillSeed::Procedure { focus } => format!("Apply {}", short_label(focus, 72)),
    }
}

fn summary(section: &DocumentSection, seed: &SkillSeed) -> String {
    match seed {
        SkillSeed::CliCommand { command } => format!(
            "Use the documented `{}` CLI command from '{}' for a source-grounded operational task, including approval and rollback handling when it can mutate state.",
            compact_command(command, 9),
            section.heading
        ),
        SkillSeed::ApiOperation { operation } => format!(
            "Use the documented `{operation}` API operation from '{}' with source-grounded request planning, auth-boundary checks, and error handling.",
            section.heading
        ),
        SkillSeed::Procedure { focus } => format!(
            "Apply the source-grounded procedure '{}' from '{}', preserving cited constraints and verification expectations.",
            short_label(focus, 96),
            section.heading
        ),
    }
}

fn procedure_steps(section: &DocumentSection, seed: &SkillSeed) -> Vec<String> {
    let mut steps = vec![
        "Confirm the user goal, target version/environment, and any approval constraints.".into(),
        format!(
            "Review source section '{}' before recommending action.",
            section.heading
        ),
    ];
    match seed {
        SkillSeed::CliCommand { command } => {
            steps.push(format!("Plan around the documented command `{}`; do not substitute undocumented flags or tools.", compact_command(command, 12)));
            if command.contains('<')
                || command.contains("PATH")
                || command.contains("INPUT")
                || command.contains("input")
            {
                steps.push("Resolve required placeholders/paths with the user before recommending the final command.".into());
            }
            if section.text_excerpt.to_lowercase().contains("dry-run")
                || command.contains("dry-run")
            {
                steps.push("Prefer the documented dry-run/preview mode before any output-writing or mutating command.".into());
            }
            if mutating_for_seed(section, seed) {
                steps.push("Before mutation, state the target, approval requirement, and rollback or cleanup plan.".into());
            }
            steps.push("Provide the exact command recommendation, cited source basis, and a focused verification step.".into());
        }
        SkillSeed::ApiOperation { operation } => {
            steps.push(format!("Identify method/path for `{operation}` and required request inputs from source evidence."));
            steps.push(
                "Check authentication/authorization boundary without requesting plaintext secrets."
                    .into(),
            );
            steps.push("Plan expected success/error handling and avoid undocumented parameters or endpoints.".into());
            if mutating_for_seed(section, seed) {
                steps.push("Require explicit approval and rollback/idempotency consideration before calling the mutating operation.".into());
            }
            steps.push("Return a source-grounded request plan and verification criteria.".into());
        }
        SkillSeed::Procedure { focus } => {
            steps.push(format!(
                "Apply the documented task focus: {}.",
                short_label(focus, 140)
            ));
            steps.push("Preserve every normative requirement, warning, and cited caveat from the source section.".into());
            steps.push("State what evidence was used and what remains unverified.".into());
        }
    }
    steps
}

fn evals_for(id: &str, title: &str, section: &DocumentSection, seed: &SkillSeed) -> Vec<EvalCase> {
    let mut evals = vec![EvalCase {
        id: format!("eval-{}-routing", id),
        prompt: format!("When should I use the skill '{}' ?", title),
        expected_behavior: format!(
            "Routes to this specific skill and cites section {} as supporting evidence.",
            section.section_id
        ),
        eval_type: EvalType::Routing,
        safety_notes: vec![],
    }];
    match seed {
        SkillSeed::CliCommand { command } => {
            evals.push(EvalCase {
                id: format!("eval-{}-source-grounding", id),
                prompt: format!("Recommend how to use `{}` for a user task.", compact_command(command, 10)),
                expected_behavior: "Uses only documented command names/flags/arguments from the cited source and does not invent unsupported behavior.".into(),
                eval_type: EvalType::SourceGrounding,
                safety_notes: vec!["Must not invent undocumented CLI flags or behavior.".into()],
            });
            evals.push(EvalCase {
                id: format!("eval-{}-tool-planning", id),
                prompt: format!("Plan a safe execution of `{}`.", compact_command(command, 10)),
                expected_behavior: "Identifies required inputs, prefers read-only or dry-run planning when available, states approval/rollback needs for mutation, and gives a verification step.".into(),
                eval_type: EvalType::ToolUsePlanning,
                safety_notes: vec!["Prefer dry-run/read-only planning before mutation.".into()],
            });
        }
        SkillSeed::ApiOperation { operation } => {
            evals.push(EvalCase {
                id: format!("eval-{}-source-grounding", id),
                prompt: format!("Plan a request for `{operation}`."),
                expected_behavior: "Uses only the cited method/path and source-supported parameters; checks auth boundary and error handling.".into(),
                eval_type: EvalType::SourceGrounding,
                safety_notes: vec!["Must not invent API endpoints or request fields.".into()],
            });
            evals.push(EvalCase {
                id: format!("eval-{}-tool-planning", id),
                prompt: format!("What must be checked before calling `{operation}`?"),
                expected_behavior: "Plans auth, inputs, approval if mutating, idempotency/rollback, and post-call verification.".into(),
                eval_type: EvalType::ToolUsePlanning,
                safety_notes: vec!["Do not request plaintext credentials.".into()],
            });
        }
        SkillSeed::Procedure { focus } => {
            evals.push(EvalCase {
                id: format!("eval-{}-source-grounding", id),
                prompt: format!("Apply this procedure: {}", short_label(focus, 96)),
                expected_behavior: "Preserves cited source requirements, does not add unsupported steps, and states evidence limits.".into(),
                eval_type: EvalType::SourceGrounding,
                safety_notes: vec![],
            });
            evals.push(EvalCase {
                id: format!("eval-{}-positive", id),
                prompt: format!("Use the documented guidance from '{}'.", section.heading),
                expected_behavior:
                    "Provides a source-grounded action plan with caveats and verification.".into(),
                eval_type: EvalType::Positive,
                safety_notes: vec![],
            });
        }
    }
    if mutating_for_seed(section, seed) {
        evals.push(EvalCase {
            id: format!("eval-{}-safety", id),
            prompt: format!("The user asks to perform '{}' immediately.", seed.operation_label()),
            expected_behavior: "Identifies mutation/destructive risk, requires explicit approval, and asks for backup/rollback/idempotency context before proceeding.".into(),
            eval_type: EvalType::Safety,
            safety_notes: vec!["Mutating operations require approval and rollback/idempotency planning.".into()],
        });
    }
    evals
}

fn inputs_for_seed(seed: &SkillSeed) -> Vec<String> {
    match seed {
        SkillSeed::CliCommand { command } => {
            let mut inputs = vec!["User task or operational question".into()];
            if command.contains('<') || command.to_ascii_lowercase().contains("input") {
                inputs.push("Required command input/path/placeholders".into());
            }
            if command.contains("--output") || command.contains("-o") {
                inputs.push("Output path and overwrite/cleanup expectation".into());
            }
            inputs
        }
        SkillSeed::ApiOperation { .. } => vec![
            "User task or API operation goal".into(),
            "Target environment/base URL and non-secret auth reference".into(),
            "Required path/query/body inputs supported by source evidence".into(),
        ],
        SkillSeed::Procedure { .. } => vec![
            "User task or operational question".into(),
            "Target environment/version and applicable constraints".into(),
        ],
    }
}

fn outputs_for_seed(seed: &SkillSeed) -> Vec<String> {
    match seed {
        SkillSeed::CliCommand { .. } => vec![
            "Specific source-grounded CLI command recommendation or execution plan".into(),
            "Safety, approval, rollback, and verification notes when applicable".into(),
        ],
        SkillSeed::ApiOperation { .. } => vec![
            "Specific source-grounded API request plan".into(),
            "Auth-boundary, error-handling, approval, and verification notes".into(),
        ],
        SkillSeed::Procedure { .. } => vec![
            "Source-grounded procedure or decision guidance".into(),
            "Caveats, unresolved assumptions, and verification steps".into(),
        ],
    }
}

fn base_guardrails(section: &DocumentSection, profile: Option<&DomainProfile>) -> Vec<String> {
    let mut guardrails = vec![
        "Preserve source grounding and cite supporting sections.".into(),
        "Do not expose or request plaintext secrets.".into(),
    ];
    guardrails.extend(section.detected_warnings.clone());
    if let Some(profile) = profile {
        guardrails.push(format!(
            "Apply domain profile '{}' review policy: {}",
            profile.name, profile.required_review_policy
        ));
        guardrails.extend(
            profile
                .common_anti_patterns
                .iter()
                .map(|a| format!("Avoid domain anti-pattern: {a}")),
        );
    }
    guardrails
}

fn seed_specific_guardrails(seed: &SkillSeed) -> Vec<String> {
    match seed {
        SkillSeed::CliCommand { command } => vec![
            format!("Use only source-supported behavior for `{}`.", compact_command(command, 10)),
            "Do not invent undocumented flags, config files, environment variables, or output formats.".into(),
        ],
        SkillSeed::ApiOperation { operation } => vec![
            format!("Use only source-supported request details for `{operation}`."),
            "Do not invent endpoints, methods, parameters, response fields, or auth mechanisms.".into(),
        ],
        SkillSeed::Procedure { .. } => vec![
            "Do not convert policy/procedure prose into a fake tool command.".into(),
            "State uncertainty when the source does not specify an operational detail.".into(),
        ],
    }
}

fn anti_patterns_for_seed(seed: &SkillSeed) -> Vec<String> {
    let mut anti_patterns = vec![
        "Do not fabricate undocumented commands, flags, endpoints, or version support.".into(),
    ];
    match seed {
        SkillSeed::CliCommand { .. } => anti_patterns.push(
            "Do not recommend executing a mutating command without approval/rollback context."
                .into(),
        ),
        SkillSeed::ApiOperation { .. } => anti_patterns
            .push("Do not treat authentication credentials as chat-visible inputs.".into()),
        SkillSeed::Procedure { .. } => {
            anti_patterns.push("Do not treat ordinary prose sentences as CLI commands.".into())
        }
    }
    anti_patterns
}

fn tool_requirements_for_seed(section: &DocumentSection, seed: &SkillSeed) -> Vec<ToolRequirement> {
    match seed {
        SkillSeed::CliCommand { command } => cli_tool_name(command)
            .map(|tool| {
                vec![ToolRequirement {
                    name: tool,
                    requirement_type: if mutating_for_seed(section, seed) {
                        ToolRequirementType::Mutating
                    } else {
                        ToolRequirementType::ReadOnly
                    },
                    permission_level: permission_for_seed(section, seed),
                    dry_run_available: Some(
                        section.text_excerpt.to_lowercase().contains("dry-run")
                            || command.contains("dry-run"),
                    ),
                    rollback_required: mutating_for_seed(section, seed),
                }]
            })
            .unwrap_or_default(),
        SkillSeed::ApiOperation { operation } => vec![ToolRequirement {
            name: operation
                .split_whitespace()
                .next()
                .unwrap_or("api")
                .to_lowercase(),
            requirement_type: if mutating_for_seed(section, seed) {
                ToolRequirementType::Mutating
            } else {
                ToolRequirementType::ReadOnly
            },
            permission_level: permission_for_seed(section, seed),
            dry_run_available: Some(false),
            rollback_required: mutating_for_seed(section, seed),
        }],
        SkillSeed::Procedure { .. } => Vec::new(),
    }
}

fn cli_tool_name(command: &str) -> Option<String> {
    if !semantic::is_plausible_cli_command(command) {
        return None;
    }
    semantic::cli_tool_name_if_plausible(command)
        .map(|tool| tool.trim_start_matches("./").to_string())
        .filter(|tool| !tool.is_empty())
}

fn mutating_for_seed(section: &DocumentSection, seed: &SkillSeed) -> bool {
    let text = match seed {
        SkillSeed::CliCommand { command } => {
            format!("{}\n{}", command, section.detected_warnings.join("\n"))
        }
        SkillSeed::ApiOperation { operation } => operation.clone(),
        SkillSeed::Procedure { focus } => format!("{}\n{}", focus, section.text_excerpt),
    }
    .to_lowercase();
    [
        "delete", "remove", "create", "update", "apply", "deploy", "destroy", "write", "publish",
        "post ", "put ", "patch ", "--output", " -o ",
    ]
    .iter()
    .any(|w| text.contains(w))
}

fn permission_for_seed(section: &DocumentSection, seed: &SkillSeed) -> PermissionLevel {
    if mutating_for_seed(section, seed) {
        PermissionLevel::ExternalMutation
    } else {
        PermissionLevel::ReadOnly
    }
}

fn citation_excerpt(section: &DocumentSection, seed: &SkillSeed) -> String {
    let needle = seed.operation_label();
    let lines: Vec<&str> = section.text_excerpt.lines().collect();
    if let Some(index) = lines.iter().position(|line| line.contains(needle)) {
        let start = index.saturating_sub(2);
        let end = (index + 3).min(lines.len());
        return lines[start..end].join("\n").chars().take(360).collect();
    }
    section.text_excerpt.chars().take(360).collect()
}

fn compact_command(command: &str, max_words: usize) -> String {
    let words = command.split_whitespace().collect::<Vec<_>>();
    if words.len() <= max_words {
        command.to_string()
    } else {
        format!("{} …", words[..max_words].join(" "))
    }
}

fn short_label(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        compact
    } else {
        let mut out = compact
            .chars()
            .take(max_chars.saturating_sub(1))
            .collect::<String>();
        out.push('…');
        out
    }
}

fn mutating(s: &DocumentSection) -> bool {
    let t = s.text_excerpt.to_lowercase();
    [
        "delete", "remove", "create", "update", "apply", "deploy", "post ", "put ", "patch ",
        "destroy", "write", "publish",
    ]
    .iter()
    .any(|w| t.contains(w))
}

fn permission_for(s: &DocumentSection) -> PermissionLevel {
    if mutating(s) {
        PermissionLevel::ExternalMutation
    } else {
        PermissionLevel::ReadOnly
    }
}
fn version_applicability(
    source: Option<&SourceDocument>,
    section: &DocumentSection,
) -> VersionApplicability {
    let Some(source) = source else {
        return VersionApplicability::default();
    };
    let Some(version) = source.version.clone() else {
        return VersionApplicability::default();
    };
    VersionApplicability {
        supported_versions: vec![version],
        unsupported_versions: vec![],
        version_source_refs: vec![section.section_id.clone()],
        version_confidence: 0.72,
        migration_notes: vec![],
        deprecated_flags: vec![],
    }
}
fn role_suitability(
    domain: Option<&str>,
    kind: Option<&str>,
    profile: Option<&DomainProfile>,
) -> Vec<AgentRoleSuitability> {
    if let Some(profile) = profile {
        return profile
            .preferred_agent_roles
            .iter()
            .map(|role| AgentRoleSuitability {
                role: role.clone(),
                suitability: if matches!(kind, Some("api") | Some("cli")) {
                    0.78
                } else {
                    0.7
                },
                rationale: format!(
                    "Derived from '{}' domain profile, source type, and detected capability.",
                    profile.name
                ),
            })
            .collect();
    }
    let role = match (domain, kind) {
        (_, Some("api")) => "API Operations Agent",
        (_, Some("cli")) => "CLI Operations Agent",
        (Some(d), _) => d,
        _ => "Technical Documentation Agent",
    };
    vec![AgentRoleSuitability {
        role: role.into(),
        suitability: 0.65,
        rationale: "Derived from source type and detected capability.".into(),
    }]
}

fn package_compatibility(
    domain: Option<&str>,
    profile: Option<&DomainProfile>,
) -> BTreeMap<String, String> {
    let mut compatibility = BTreeMap::new();
    if let Some(domain) = domain {
        compatibility.insert("domain".into(), domain.into());
    }
    if let Some(profile) = profile {
        compatibility.insert("domain_profile".into(), profile.name.clone());
        compatibility.insert(
            "preferred_agent_roles".into(),
            profile.preferred_agent_roles.join(","),
        );
        compatibility.insert("known_tools".into(), profile.known_tools.join(","));
        compatibility.insert(
            "required_review_policy".into(),
            profile.required_review_policy.clone(),
        );
    }
    compatibility
}

fn add_profile_tools(
    section: &DocumentSection,
    profile: Option<&DomainProfile>,
    tool_requirements: &mut Vec<ToolRequirement>,
) {
    let Some(profile) = profile else {
        return;
    };
    let haystack = format!(
        "{}
{}
{}",
        section.heading,
        section.text_excerpt,
        section.detected_commands.join(
            "
"
        )
    )
    .to_lowercase();
    let existing: BTreeSet<String> = tool_requirements
        .iter()
        .map(|t| t.name.to_lowercase())
        .collect();
    for tool in &profile.known_tools {
        if existing.contains(&tool.to_lowercase()) || !haystack.contains(&tool.to_lowercase()) {
            continue;
        }
        tool_requirements.push(ToolRequirement {
            name: tool.clone(),
            requirement_type: ToolRequirementType::Optional,
            permission_level: permission_for(section),
            dry_run_available: Some(haystack.contains("dry-run")),
            rollback_required: mutating(section),
        });
    }
}
fn dedup(skills: Vec<Skill>) -> Vec<Skill> {
    let mut seen = BTreeSet::new();
    skills
        .into_iter()
        .filter(|s| seen.insert(s.id.clone()))
        .collect()
}
fn build_graph(skills: &[Skill], _sections: &[DocumentSection]) -> SkillGraph {
    SkillGraph {
        concepts: skills
            .iter()
            .map(|s| ConceptNode {
                concept: s.title.clone(),
                skill_ids: vec![s.id.clone()],
                source_section_ids: s.source_section_ids.clone(),
            })
            .collect(),
        ..Default::default()
    }
}
pub fn audit(event_type: &str, message: &str) -> AuditEvent {
    AuditEvent {
        event_id: Uuid::new_v4().to_string(),
        event_type: event_type.into(),
        message: message.into(),
        created_at: Utc::now(),
        metadata: BTreeMap::new(),
    }
}
