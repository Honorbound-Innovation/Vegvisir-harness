use crate::corpus;
use crate::domain;
use crate::ingest;
use crate::models::*;
use anyhow::Result;
use chrono::Utc;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use uuid::Uuid;

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
        .map(corpus::candidate_from_section)
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
        let interface_kind = if !section.detected_api_operations.is_empty() {
            Some("api")
        } else if !section.detected_commands.is_empty() {
            Some("cli")
        } else {
            None
        };
        let title = skill_title(section, interface_kind);
        let id = ingest::stable_id("skill", &format!("{}:{}", section.section_id, title));
        let mut metadata = BTreeMap::new();
        if let Some(kind) = interface_kind {
            metadata.insert("interface_kind".into(), kind.into());
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
        let mut runtime_policy = RuntimePolicy::default();
        let mut skill_type = SkillType::Procedure;
        let mut tool_requirements = Vec::new();
        if let Some(kind) = interface_kind {
            skill_type = if kind == "api" {
                SkillType::ApiOperation
            } else {
                SkillType::CliOperation
            };
            runtime_policy.run_read_only_commands = kind == "cli";
            runtime_policy.requires_user_approval = true;
            for tool in detected_tools(section) {
                tool_requirements.push(ToolRequirement {
                    name: tool,
                    requirement_type: ToolRequirementType::Required,
                    permission_level: permission_for(section),
                    dry_run_available: Some(
                        section.text_excerpt.to_lowercase().contains("dry-run"),
                    ),
                    rollback_required: mutating(section),
                });
            }
        }
        add_profile_tools(section, profile, &mut tool_requirements);
        if profile.is_some() && !tool_requirements.is_empty() {
            runtime_policy.requires_user_approval = true;
        }
        let citation = Citation {
            citation_id: ingest::stable_id("cite", &section.section_id),
            source_id: section.source_id.clone(),
            section_id: section.section_id.clone(),
            excerpt: section.text_excerpt.chars().take(280).collect(),
        };
        let confidence = ConfidenceBreakdown {
            raw: 0.62,
            extraction: 0.78,
            procedure: if !section.detected_normative_language.is_empty() {
                0.7
            } else {
                0.55
            },
            guardrail: if !section.detected_warnings.is_empty() {
                0.75
            } else {
                0.45
            },
            eval: 0.45,
            routing: 0.55,
            source_quality: source_quality(source),
            ..Default::default()
        };
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
        if mutating(section) {
            guardrails.push("Require explicit user approval, backup/rollback plan, and idempotency check before mutation.".into());
            runtime_policy.requires_backup_or_rollback = true;
        }
        skills.push(Skill {
            id: id.clone(),
            title: title.clone(),
            summary: summary(section),
            skill_type,
            scope: SkillScope::TaskLevel,
            status: SkillStatus::Candidate,
            maturity: SkillMaturity::Level1StructuredCandidate,
            domain: domain.map(str::to_string),
            source_section_ids: vec![section.section_id.clone()],
            procedure: procedure_steps(section),
            inputs: vec!["User task or operational question".into()],
            outputs: vec!["Source-grounded guidance or action plan".into()],
            guardrails,
            anti_patterns: vec![
                "Do not fabricate undocumented commands, flags, endpoints, or version support."
                    .into(),
            ],
            evals: evals_for(&id, &title),
            citations: vec![citation],
            confidence,
            evidence_breakdown: EvidenceBreakdown::default(),
            inference_records: vec![],
            role_suitability: role_suitability(domain, interface_kind, profile),
            tool_requirements,
            runtime_policy,
            version_applicability: VersionApplicability::default(),
            metadata,
        });
    }
    dedup(skills)
}

fn is_capability_bearing(s: &DocumentSection) -> bool {
    !s.detected_commands.is_empty()
        || !s.detected_api_operations.is_empty()
        || !s.detected_normative_language.is_empty()
        || s.heading.to_lowercase().contains("troubleshoot")
        || s.heading.to_lowercase().contains("diagnos")
        || s.text_excerpt.to_lowercase().contains("how to")
}
fn skill_title(s: &DocumentSection, kind: Option<&str>) -> String {
    match kind {
        Some("api") => format!("Use API operations from {}", s.heading),
        Some("cli") => format!("Use CLI workflow from {}", s.heading),
        _ => format!("Apply {}", s.heading),
    }
}
fn summary(s: &DocumentSection) -> String {
    format!(
        "Use the '{}' source section to perform a source-grounded technical task.",
        s.heading
    )
}
fn procedure_steps(s: &DocumentSection) -> Vec<String> {
    let mut steps = vec![
        "Confirm task goal, target version, and relevant constraints.".into(),
        "Review cited source evidence before giving operational guidance.".into(),
    ];
    if !s.detected_commands.is_empty() {
        steps.push(
            "Prefer read-only/status commands first; explain mutating commands before use.".into(),
        );
    }
    if !s.detected_api_operations.is_empty() {
        steps.push("Validate endpoint, method, authentication boundary, and error handling before API use.".into());
    }
    steps.push(
        "Provide concise output with citations, caveats, and next verification steps.".into(),
    );
    steps
}
fn evals_for(id: &str, title: &str) -> Vec<EvalCase> {
    vec![EvalCase {
        id: format!("eval-{}-routing", id),
        prompt: format!("When should I {}?", title.to_lowercase()),
        expected_behavior: "Routes to the skill and cites source evidence.".into(),
        eval_type: EvalType::Routing,
        safety_notes: vec![],
    }]
}
fn mutating(s: &DocumentSection) -> bool {
    let t = s.text_excerpt.to_lowercase();
    [
        "delete", "remove", "create", "update", "apply", "deploy", "post ", "put ", "patch ",
        "destroy",
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
fn detected_tools(s: &DocumentSection) -> Vec<String> {
    let mut out = BTreeSet::new();
    for c in &s.detected_commands {
        if let Some(tool) = c.split_whitespace().next() {
            out.insert(tool.to_string());
        }
    }
    for op in &s.detected_api_operations {
        out.insert(op.split_whitespace().next().unwrap_or("api").to_lowercase());
    }
    out.into_iter().collect()
}
fn source_quality(source: Option<&SourceDocument>) -> f32 {
    match source.map(|s| &s.source_type) {
        Some(
            SourceType::OpenApi | SourceType::ApiSpec | SourceType::CliSpec | SourceType::CliHelp,
        ) => 0.75,
        Some(SourceType::Markdown) => 0.65,
        _ => 0.5,
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
        .filter(|s| seen.insert(s.title.clone()))
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
