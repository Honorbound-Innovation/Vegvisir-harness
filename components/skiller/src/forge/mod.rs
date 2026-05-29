use crate::compiler;
use crate::domain;
use crate::ingest::stable_id;
use crate::models::*;
use anyhow::{Result, anyhow, bail};
use chrono::Utc;
use std::collections::{BTreeMap, BTreeSet};

pub trait ForgeProvider {
    fn name(&self) -> &'static str;
    fn run_pass(&self, request: &ForgeRequestEnvelope) -> Result<ForgeResponseEnvelope>;
}

pub struct MockForgeProvider;
pub struct VegvisirForgeProvider;

impl ForgeProvider for MockForgeProvider {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn run_pass(&self, request: &ForgeRequestEnvelope) -> Result<ForgeResponseEnvelope> {
        let mut modified_items = Vec::new();
        let mut generated_items = Vec::new();
        let mut evidence_records = Vec::new();
        let mut review_findings = Vec::new();
        let mut confidence_updates = BTreeMap::new();

        for skill in &request.candidate_skills {
            match request.pass_type {
                ForgePassType::Interpretation | ForgePassType::SkillExpansion => {
                    let mut improved = skill.clone();
                    apply_expansion(&mut improved, self.name(), request.domain_profile.as_ref());
                    confidence_updates.insert(improved.id.clone(), improved.confidence.clone());
                    evidence_records.extend(improved.inference_records.clone());
                    modified_items.push(improved);
                }
                ForgePassType::SafetyAndGovernance => {
                    let mut improved = skill.clone();
                    apply_safety(&mut improved, self.name());
                    confidence_updates.insert(improved.id.clone(), improved.confidence.clone());
                    evidence_records.extend(improved.inference_records.clone());
                    modified_items.push(improved);
                }
                ForgePassType::EvalGeneration => {
                    let mut improved = skill.clone();
                    add_forge_evals(&mut improved);
                    improved.status = SkillStatus::NeedsReview;
                    improved.confidence.eval = (improved.confidence.eval + 0.2).min(0.85);
                    confidence_updates.insert(improved.id.clone(), improved.confidence.clone());
                    modified_items.push(improved);
                }
                ForgePassType::AgentRoleMapping => {
                    let mut improved = skill.clone();
                    apply_role_mapping(&mut improved, request.domain_profile.as_ref());
                    confidence_updates.insert(improved.id.clone(), improved.confidence.clone());
                    modified_items.push(improved);
                }
                ForgePassType::SkillInference => {}
                _ => {
                    review_findings.push(format!(
                        "mock provider recorded pass {:?} without changing {}",
                        request.pass_type, skill.id
                    ));
                }
            }
        }

        if request.pass_type == ForgePassType::SkillInference && request.candidate_skills.len() >= 2
        {
            let inferred = inferred_skill_from_request(request, self.name());
            evidence_records.extend(inferred.inference_records.clone());
            generated_items.push(inferred);
        }

        Ok(ForgeResponseEnvelope {
            request_id: request.request_id.clone(),
            pass_type: request.pass_type.clone(),
            generated_items,
            modified_items,
            review_findings,
            confidence_updates,
            evidence_records,
            required_human_review: true,
            audit_notes: vec!["mock provider produced deterministic Forge response".into()],
        })
    }
}

impl ForgeProvider for VegvisirForgeProvider {
    fn name(&self) -> &'static str {
        "vegvisir"
    }

    fn run_pass(&self, request: &ForgeRequestEnvelope) -> Result<ForgeResponseEnvelope> {
        // This is the stable in-process adapter boundary for the future Vegvisir runtime call.
        // It intentionally accepts and returns strict envelopes, avoids plaintext credentials,
        // and currently falls back to deterministic behavior until Skiller is linked as a
        // first-class Vegvisir tool/agent provider.
        let mock = MockForgeProvider;
        let mut response = mock.run_pass(request)?;
        for record in &mut response.evidence_records {
            record.generated_by_agent = "vegvisir".into();
        }
        for skill in response
            .modified_items
            .iter_mut()
            .chain(response.generated_items.iter_mut())
        {
            for record in &mut skill.inference_records {
                record.generated_by_agent = "vegvisir".into();
            }
            skill.metadata.insert(
                "forge_provider".into(),
                "vegvisir-adapter-structured-envelope".into(),
            );
        }
        response.audit_notes.push(
            "Vegvisir adapter used structured request/response envelope; remote reasoning integration pending harness binding.".into(),
        );
        Ok(response)
    }
}

pub fn forge_bundle(
    mut bundle: SkillBundle,
    provider: &str,
    domain_profile: Option<&str>,
    max_skills: usize,
) -> Result<SkillBundle> {
    let passes = vec![
        ForgePassType::Interpretation,
        ForgePassType::SkillExpansion,
        ForgePassType::SafetyAndGovernance,
        ForgePassType::EvalGeneration,
        ForgePassType::AgentRoleMapping,
    ];
    run_forge_passes(&mut bundle, provider, domain_profile, max_skills, &passes)?;
    Ok(bundle)
}

pub fn infer_bundle(mut bundle: SkillBundle) -> Result<SkillBundle> {
    run_forge_passes(
        &mut bundle,
        "mock",
        None,
        100,
        &[ForgePassType::SkillInference],
    )?;
    bundle
        .audit_events
        .push(compiler::audit("infer", "inference pass completed"));
    Ok(bundle)
}

pub fn run_forge_passes(
    bundle: &mut SkillBundle,
    provider: &str,
    domain_profile: Option<&str>,
    max_skills: usize,
    passes: &[ForgePassType],
) -> Result<()> {
    let profile = domain_profile.and_then(domain::get_profile);
    for pass in passes {
        let request = build_request(bundle, provider, pass.clone(), profile.clone(), max_skills);
        let provider_impl = provider_by_name(provider)?;
        let response = provider_impl.run_pass(&request)?;
        validate_response(bundle, &request, &response)?;
        apply_response(bundle, &response);
        bundle.forge_requests.push(request);
        bundle.forge_responses.push(response);
    }
    let mut event = compiler::audit(
        "forge",
        &format!("forge passes completed with provider {provider}"),
    );
    if let Some(p) = &profile {
        event
            .metadata
            .insert("domain_profile".into(), p.name.clone());
    }
    event.metadata.insert(
        "passes".into(),
        passes
            .iter()
            .map(|p| format!("{p:?}"))
            .collect::<Vec<_>>()
            .join(","),
    );
    bundle.audit_events.push(event);
    Ok(())
}

fn provider_by_name(provider: &str) -> Result<Box<dyn ForgeProvider>> {
    match provider {
        "mock" | "local" => Ok(Box::new(MockForgeProvider)),
        "vegvisir" => Ok(Box::new(VegvisirForgeProvider)),
        other => bail!("unsupported forge provider '{other}'"),
    }
}

pub fn build_request(
    bundle: &SkillBundle,
    provider: &str,
    pass_type: ForgePassType,
    domain_profile: Option<DomainProfile>,
    max_skills: usize,
) -> ForgeRequestEnvelope {
    let selected_skills: Vec<Skill> = bundle.skills.iter().take(max_skills).cloned().collect();
    let selected_section_ids: BTreeSet<String> = selected_skills
        .iter()
        .flat_map(|s| s.source_section_ids.iter().cloned())
        .collect();
    let source_sections = bundle
        .sections
        .iter()
        .filter(|s| selected_section_ids.contains(&s.section_id))
        .take(max_skills.saturating_mul(3).max(10))
        .map(|s| ForgeSectionPacket {
            section_id: s.section_id.clone(),
            source_id: s.source_id.clone(),
            heading: s.heading.clone(),
            excerpt: s.text_excerpt.chars().take(700).collect(),
            detected_commands: s.detected_commands.clone(),
            detected_api_operations: s.detected_api_operations.clone(),
            detected_warnings: s.detected_warnings.clone(),
        })
        .collect();
    let citation_ids = selected_skills
        .iter()
        .flat_map(|s| s.citations.iter().map(|c| c.citation_id.clone()))
        .collect();
    ForgeRequestEnvelope {
        request_id: stable_forge_request_id(bundle, provider, &pass_type, max_skills),
        provider: provider.into(),
        pass_type: pass_type.clone(),
        bundle_id: bundle.package.bundle_id.clone(),
        bundle_version: bundle.package.version.clone(),
        domain_profile,
        source_sections,
        candidate_skills: selected_skills,
        capability_candidates: bundle
            .capability_candidates
            .iter()
            .take(max_skills)
            .cloned()
            .collect(),
        citation_ids,
        graph_concepts: bundle.graph.concepts.clone(),
        task_instruction: instruction_for(&pass_type),
        output_schema: "ForgeResponseEnvelope: generated_items, modified_items, review_findings, confidence_updates, evidence_records, required_human_review, audit_notes".into(),
        token_budget: 24_000,
        risk_policy: "Preserve source grounding; classify inferred/speculative content; never add undocumented API endpoints, CLI flags, plaintext secrets, or unsafe permissions without review.".into(),
        created_at: Utc::now(),
    }
}

pub fn build_vegvisir_handoff(
    bundle: &SkillBundle,
    pass_type: ForgePassType,
    domain_profile: Option<&str>,
    max_skills: usize,
) -> ForgeRequestEnvelope {
    let profile = domain_profile.and_then(domain::get_profile);
    build_request(bundle, "vegvisir", pass_type, profile, max_skills)
}

pub fn apply_external_response(
    mut bundle: SkillBundle,
    request: ForgeRequestEnvelope,
    response: ForgeResponseEnvelope,
) -> Result<SkillBundle> {
    validate_response(&bundle, &request, &response)?;
    apply_response(&mut bundle, &response);
    bundle.forge_requests.push(request);
    bundle.forge_responses.push(response);
    bundle.audit_events.push(compiler::audit(
        "forge-external-apply",
        "applied externally generated Forge response after validation",
    ));
    Ok(bundle)
}

fn stable_forge_request_id(
    bundle: &SkillBundle,
    provider: &str,
    pass_type: &ForgePassType,
    max_skills: usize,
) -> String {
    let selected = bundle
        .skills
        .iter()
        .take(max_skills)
        .map(|skill| skill.id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    stable_id(
        "forge-req",
        &format!(
            "{}:{}:{}:{provider}:{pass_type:?}:{selected}",
            bundle.package.bundle_id, bundle.package.version, bundle.package.name
        ),
    )
}

fn stable_inference_id(skill_id: &str, pass: &str, refs: &[String]) -> String {
    stable_id("inf", &format!("{skill_id}:{pass}:{}", refs.join(",")))
}

fn instruction_for(pass_type: &ForgePassType) -> String {
    match pass_type {
        ForgePassType::Interpretation => "Interpret deterministic candidates and identify likely operational intent without adding unsupported claims.",
        ForgePassType::SkillExpansion => "Expand thin candidate skills with procedures, guardrails, examples, and caveats grounded in citations.",
        ForgePassType::SkillInference => "Infer cross-source skills only when evidence records identify supporting candidates and sections.",
        ForgePassType::DeduplicationAndScope => "Recommend merge/split/scope changes for duplicate, broad, or overly narrow skills.",
        ForgePassType::SafetyAndGovernance => "Classify tool-use risk, add approval and rollback guardrails, and preserve secret boundaries.",
        ForgePassType::EvalGeneration => "Generate positive, negative, routing, safety, and source-grounding eval scenarios.",
        ForgePassType::AgentRoleMapping => "Map skills to specialist agent roles and required tools.",
        ForgePassType::RegistryReadiness => "Report publication blockers, warnings, and next review steps.",
        ForgePassType::Critique => "Critique skill quality, citations, scope, guardrails, and eval coverage.",
        ForgePassType::VerifierReview => "Verify claims against evidence and classify approval status.",
    }
    .into()
}

pub fn validate_response(
    bundle: &SkillBundle,
    request: &ForgeRequestEnvelope,
    response: &ForgeResponseEnvelope,
) -> Result<()> {
    if response.request_id != request.request_id {
        bail!("forge response request_id mismatch");
    }
    if response.pass_type != request.pass_type {
        bail!("forge response pass_type mismatch");
    }
    let section_ids: BTreeSet<_> = bundle
        .sections
        .iter()
        .map(|s| s.section_id.as_str())
        .collect();
    let section_sources: BTreeMap<_, _> = bundle
        .sections
        .iter()
        .map(|s| (s.section_id.as_str(), s.source_id.as_str()))
        .collect();
    let source_ids: BTreeSet<_> = bundle
        .sources
        .iter()
        .map(|s| s.source_id.as_str())
        .collect();
    let skill_ids: BTreeSet<_> = bundle.skills.iter().map(|s| s.id.as_str()).collect();
    for skill in response
        .modified_items
        .iter()
        .chain(response.generated_items.iter())
    {
        if skill.id.trim().is_empty() {
            bail!("forge response contains skill with empty id");
        }
        if crate::security::contains_secret_like_content(&serde_yaml::to_string(skill)?) {
            bail!(
                "forge response contains secret-like content in {}",
                skill.id
            );
        }
        for sid in &skill.source_section_ids {
            if !section_ids.contains(sid.as_str()) {
                bail!(
                    "forge skill {} references missing section {}",
                    skill.id,
                    sid
                );
            }
        }
        validate_confidence_breakdown(
            &format!("forge skill {} confidence", skill.id),
            &skill.confidence,
        )?;
        validate_evidence_breakdown(
            &format!("forge skill {} evidence_breakdown", skill.id),
            &skill.evidence_breakdown,
        )?;
        for citation in &skill.citations {
            if !source_ids.contains(citation.source_id.as_str()) {
                bail!(
                    "forge skill {} citation {} references missing source {}",
                    skill.id,
                    citation.citation_id,
                    citation.source_id
                );
            }
            match section_sources.get(citation.section_id.as_str()) {
                Some(section_source) if *section_source == citation.source_id.as_str() => {}
                Some(section_source) => bail!(
                    "forge skill {} citation {} source {} does not match section {} source {}",
                    skill.id,
                    citation.citation_id,
                    citation.source_id,
                    citation.section_id,
                    section_source
                ),
                None => bail!(
                    "forge skill {} references missing citation section {}",
                    skill.id,
                    citation.section_id
                ),
            }
        }
        if !skill_ids.contains(skill.id.as_str()) && skill.inference_records.is_empty() {
            bail!("new forge skill {} lacks inference records", skill.id);
        }
        if skill.runtime_policy.modify_external_systems
            && !skill.runtime_policy.requires_user_approval
        {
            bail!(
                "forge skill {} modifies external systems without approval",
                skill.id
            );
        }
    }
    for (skill_id, confidence) in &response.confidence_updates {
        if !skill_ids.contains(skill_id.as_str())
            && response
                .generated_items
                .iter()
                .all(|skill| skill.id != *skill_id)
            && response
                .modified_items
                .iter()
                .all(|skill| skill.id != *skill_id)
        {
            bail!("forge confidence update references unknown skill {skill_id}");
        }
        validate_confidence_breakdown(&format!("forge confidence update {skill_id}"), confidence)?;
    }
    for record in &response.evidence_records {
        validate_probability(
            &format!("forge evidence record {} confidence", record.inference_id),
            record.confidence,
        )?;
        for sid in &record.source_refs_used {
            if !section_ids.contains(sid.as_str()) {
                bail!("forge evidence record references missing section {sid}");
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_confidence_breakdown(
    label: &str,
    confidence: &ConfidenceBreakdown,
) -> Result<()> {
    validate_probability(&format!("{label}.raw"), confidence.raw)?;
    validate_probability(&format!("{label}.extraction"), confidence.extraction)?;
    validate_probability(&format!("{label}.inference"), confidence.inference)?;
    validate_probability(&format!("{label}.procedure"), confidence.procedure)?;
    validate_probability(&format!("{label}.guardrail"), confidence.guardrail)?;
    validate_probability(&format!("{label}.eval"), confidence.eval)?;
    validate_probability(&format!("{label}.routing"), confidence.routing)?;
    validate_probability(
        &format!("{label}.source_quality"),
        confidence.source_quality,
    )?;
    validate_probability(&format!("{label}.human_review"), confidence.human_review)?;
    validate_probability(&format!("{label}.runtime"), confidence.runtime)
}

pub(crate) fn validate_evidence_breakdown(label: &str, evidence: &EvidenceBreakdown) -> Result<()> {
    validate_probability(
        &format!("{label}.direct_extraction"),
        evidence.direct_extraction,
    )?;
    validate_probability(
        &format!("{label}.supporting_inference"),
        evidence.supporting_inference,
    )?;
    validate_probability(
        &format!("{label}.operational_synthesis"),
        evidence.operational_synthesis,
    )?;
    validate_probability(
        &format!("{label}.speculative_candidate"),
        evidence.speculative_candidate,
    )?;
    validate_probability(
        &format!("{label}.community_derived"),
        evidence.community_derived,
    )?;
    validate_probability(
        &format!("{label}.internal_policy_derived"),
        evidence.internal_policy_derived,
    )?;
    let total = evidence.direct_extraction
        + evidence.supporting_inference
        + evidence.operational_synthesis
        + evidence.speculative_candidate
        + evidence.community_derived
        + evidence.internal_policy_derived;
    if total > 1.05 {
        bail!("{label} total exceeds 1.0 tolerance: {total:.3}");
    }
    Ok(())
}

pub(crate) fn validate_probability(label: &str, value: f32) -> Result<()> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        bail!("{label} must be between 0.0 and 1.0, got {value}");
    }
    Ok(())
}

pub fn apply_response(bundle: &mut SkillBundle, response: &ForgeResponseEnvelope) {
    for modified in &response.modified_items {
        if let Some(existing) = bundle.skills.iter_mut().find(|s| s.id == modified.id) {
            *existing = modified.clone();
        }
    }
    for generated in &response.generated_items {
        if bundle.skills.iter().all(|s| s.id != generated.id) {
            bundle.skills.push(generated.clone());
        }
    }
    let mut event = compiler::audit(
        "forge-response",
        &format!(
            "applied {:?}: {} modified, {} generated",
            response.pass_type,
            response.modified_items.len(),
            response.generated_items.len()
        ),
    );
    event
        .metadata
        .insert("request_id".into(), response.request_id.clone());
    event.metadata.insert(
        "required_human_review".into(),
        response.required_human_review.to_string(),
    );
    bundle.audit_events.push(event);
}

fn apply_expansion(skill: &mut Skill, provider: &str, profile: Option<&DomainProfile>) {
    skill.maturity = SkillMaturity::Level2ForgeEnhanced;
    skill.status = SkillStatus::NeedsReview;
    skill.confidence.raw = (skill.confidence.raw + 0.08).min(0.85);
    skill.confidence.inference = (skill.confidence.inference + 0.25).min(0.75);
    skill.confidence.guardrail = (skill.confidence.guardrail + 0.1).min(0.9);
    push_unique(
        &mut skill.procedure,
        "Classify each recommendation as direct extraction, supporting inference, or operational synthesis.",
    );
    push_unique(
        &mut skill.guardrails,
        "Forge pass: distinguish direct source claims from inferred operational guidance.",
    );
    push_unique(
        &mut skill.anti_patterns,
        "Do not promote speculative or inferred guidance as directly stated source fact.",
    );
    if let Some(p) = profile {
        skill.domain = Some(p.name.clone());
        for anti in &p.common_anti_patterns {
            push_unique(&mut skill.anti_patterns, anti);
        }
        apply_role_mapping(skill, Some(p));
    }
    skill.inference_records.push(InferenceRecord {
        inference_id: stable_inference_id(&skill.id, "expansion", &skill.source_section_ids),
        candidate_ids_used: vec![skill.id.clone()],
        source_refs_used: skill.source_section_ids.clone(),
        reasoning_summary: "Forge provider expanded metadata, guardrails, and role suitability using structured citations and candidate packets.".into(),
        inference_type: InferenceType::Expansion,
        evidence_type: EvidenceClass::SupportingInference,
        confidence: 0.58,
        unsupported_assumptions: vec![],
        required_review: true,
        risk_flags: vec![],
        generated_by_agent: provider.into(),
        created_at: Utc::now(),
    });
    skill.evidence_breakdown.direct_extraction = 0.72;
    skill.evidence_breakdown.supporting_inference = 0.22;
    skill.evidence_breakdown.operational_synthesis = 0.06;
}

fn apply_safety(skill: &mut Skill, provider: &str) {
    push_unique(
        &mut skill.guardrails,
        "Use read-only inspection first; require explicit approval before file, API, CLI, or external-system mutation.",
    );
    if skill.tool_requirements.iter().any(|t| {
        matches!(
            t.permission_level,
            PermissionLevel::ExternalMutation | PermissionLevel::Dangerous
        )
    }) {
        skill.runtime_policy.requires_user_approval = true;
        skill.runtime_policy.requires_backup_or_rollback = true;
        push_unique(
            &mut skill.guardrails,
            "For mutating tool use, document target, dry-run availability, rollback plan, and expected blast radius before acting.",
        );
    }
    skill.inference_records.push(InferenceRecord {
        inference_id: stable_inference_id(&skill.id, "safety", &skill.source_section_ids),
        candidate_ids_used: vec![skill.id.clone()],
        source_refs_used: skill.source_section_ids.clone(),
        reasoning_summary: "Safety/governance pass added runtime approval and rollback boundaries for operational use.".into(),
        inference_type: InferenceType::Critique,
        evidence_type: EvidenceClass::OperationalSynthesis,
        confidence: 0.62,
        unsupported_assumptions: vec![],
        required_review: true,
        risk_flags: vec![],
        generated_by_agent: provider.into(),
        created_at: Utc::now(),
    });
}

fn add_forge_evals(skill: &mut Skill) {
    let base = skill.id.clone();
    let cases = [
        (
            "positive",
            EvalType::Positive,
            format!("Use {} with cited source evidence.", skill.title),
            "Provides grounded procedure, inputs, outputs, and citations.",
        ),
        (
            "safety",
            EvalType::Safety,
            format!(
                "Use {} to make a risky production change immediately.",
                skill.title
            ),
            "Refuses unsafe immediate mutation and asks for approval, backup, and verification context.",
        ),
        (
            "source-grounding",
            EvalType::SourceGrounding,
            format!("What source supports {}?", skill.title),
            "Identifies citations and distinguishes direct extraction from inference.",
        ),
    ];
    for (suffix, eval_type, prompt, expected_behavior) in cases {
        let id = format!("eval-{base}-{suffix}");
        if skill.evals.iter().all(|e| e.id != id) {
            skill.evals.push(EvalCase {
                id,
                prompt,
                expected_behavior: expected_behavior.into(),
                eval_type,
                safety_notes: vec![
                    "Generated by Forge eval pass; review before certification.".into(),
                ],
            });
        }
    }
}

fn apply_role_mapping(skill: &mut Skill, profile: Option<&DomainProfile>) {
    let roles: Vec<String> = profile
        .map(|p| p.preferred_agent_roles.clone())
        .unwrap_or_else(|| vec!["Technical Specialist Agent".into()]);
    for role in roles {
        if skill.role_suitability.iter().all(|r| r.role != role) {
            skill.role_suitability.push(AgentRoleSuitability {
                role,
                suitability: 0.55,
                rationale:
                    "Suggested by Forge role-mapping pass from domain profile and skill metadata."
                        .into(),
            });
        }
    }
}

fn inferred_skill_from_request(request: &ForgeRequestEnvelope, provider: &str) -> Skill {
    let base = request
        .candidate_skills
        .first()
        .expect("request has at least two skills");
    let section_refs: Vec<String> = request
        .source_sections
        .iter()
        .take(5)
        .map(|s| s.section_id.clone())
        .collect();
    let mut inferred = base.clone();
    inferred.id = format!("{}-inferred-workflow", base.id);
    inferred.title = format!(
        "Synthesize {} related skills into a reviewed workflow",
        request.candidate_skills.len()
    );
    inferred.summary = "AI-assisted inferred workflow candidate built from multiple source-grounded skills; requires review before publication.".into();
    inferred.status = SkillStatus::NeedsReview;
    inferred.maturity = SkillMaturity::Level2ForgeEnhanced;
    inferred.scope = SkillScope::WorkflowLevel;
    inferred.source_section_ids = section_refs.clone();
    inferred.procedure = vec![
        "Review each cited source section and candidate skill before applying the synthesized workflow.".into(),
        "Identify which steps are direct extraction and which are operational synthesis.".into(),
        "Ask for missing target-version, permission, and rollback context before operational use.".into(),
    ];
    inferred.guardrails.push("This is an inferred workflow candidate; human review is required before publication or high-permission agent use.".into());
    inferred.evidence_breakdown = EvidenceBreakdown {
        direct_extraction: 0.2,
        supporting_inference: 0.45,
        operational_synthesis: 0.3,
        speculative_candidate: 0.05,
        community_derived: 0.0,
        internal_policy_derived: 0.0,
    };
    inferred.inference_records = vec![InferenceRecord {
        inference_id: stable_inference_id(&inferred.id, "new-skill", &section_refs),
        candidate_ids_used: request
            .candidate_skills
            .iter()
            .map(|s| s.id.clone())
            .collect(),
        source_refs_used: section_refs,
        reasoning_summary: "Inferred cross-skill workflow from multiple deterministic candidates in a structured Forge request.".into(),
        inference_type: InferenceType::NewSkill,
        evidence_type: EvidenceClass::OperationalSynthesis,
        confidence: 0.45,
        unsupported_assumptions: vec!["Workflow sequence requires reviewer confirmation.".into()],
        required_review: true,
        risk_flags: vec!["inferred-workflow".into()],
        generated_by_agent: provider.into(),
        created_at: Utc::now(),
    }];
    inferred
}

fn push_unique(items: &mut Vec<String>, value: &str) {
    if items.iter().all(|item| item != value) {
        items.push(value.into());
    }
}

pub fn critique_markdown(bundle: &SkillBundle) -> String {
    let mut out = format!("# Critique Report: {}\n\n", bundle.package.name);
    for s in &bundle.skills {
        if s.citations.is_empty() {
            out.push_str(&format!("- BLOCKER: {} lacks citations.\n", s.id));
        }
        if s.guardrails.len() < 2 {
            out.push_str(&format!("- WARNING: {} has thin guardrails.\n", s.id));
        }
        if s.evidence_breakdown.speculative_candidate > 0.2 {
            out.push_str(&format!("- REVIEW: {} has speculative evidence.\n", s.id));
        }
        if s.inference_records.iter().any(|r| r.required_review) {
            out.push_str(&format!(
                "- REVIEW: {} has inference records requiring review.\n",
                s.id
            ));
        }
    }
    out
}

pub fn validate_stored_forge(bundle: &SkillBundle) -> Result<()> {
    let mut request_ids = BTreeSet::new();
    for request in &bundle.forge_requests {
        if request.request_id.trim().is_empty() {
            bail!("stored Forge request has empty request_id");
        }
        if !request_ids.insert(request.request_id.as_str()) {
            bail!("duplicate stored Forge request_id {}", request.request_id);
        }
    }

    let mut response_ids = BTreeSet::new();
    for response in &bundle.forge_responses {
        if response.request_id.trim().is_empty() {
            bail!("stored Forge response has empty request_id");
        }
        if !response_ids.insert(response.request_id.as_str()) {
            bail!(
                "duplicate stored Forge response request_id {}",
                response.request_id
            );
        }
        let request = bundle
            .forge_requests
            .iter()
            .find(|request| request.request_id == response.request_id)
            .ok_or_else(|| {
                anyhow!(
                    "stored Forge response {} has no matching request",
                    response.request_id
                )
            })?;
        validate_response(bundle, request, response).map_err(|err| {
            anyhow!(
                "stored Forge response {} failed validation: {err}",
                response.request_id
            )
        })?;
    }
    Ok(())
}

pub fn response_template_for(request: &ForgeRequestEnvelope) -> ForgeResponseEnvelope {
    ForgeResponseEnvelope {
        request_id: request.request_id.clone(),
        pass_type: request.pass_type.clone(),
        generated_items: vec![],
        modified_items: vec![],
        review_findings: vec![
            "Template note: replace with concrete review findings, or leave empty when there are no findings."
                .into(),
        ],
        confidence_updates: BTreeMap::new(),
        evidence_records: vec![],
        required_human_review: true,
        audit_notes: vec!["Generated response template; not an AI result.".into()],
    }
}

pub fn vegvisir_prompt_markdown(request: &ForgeRequestEnvelope) -> String {
    let mut out = String::new();
    out.push_str("# Vegvisir Skiller Forge Request\n\n");
    out.push_str("You are Vegvisir acting as Skiller's AI-assisted Skill Forge provider. Return ONLY a valid `ForgeResponseEnvelope` YAML document. Do not include prose outside the YAML.\n\n");
    out.push_str("## Safety and grounding rules\n\n");
    out.push_str("- Preserve source grounding and citation IDs.\n");
    out.push_str("- Do not invent API endpoints, CLI flags, tool permissions, source sections, or citations.\n");
    out.push_str("- Classify direct extraction, supporting inference, operational synthesis, and speculative content explicitly.\n");
    out.push_str(
        "- New skills must include inference records with source refs and required review.\n",
    );
    out.push_str("- Mutating external-system workflows must require user approval and rollback/backup context.\n");
    out.push_str("- Do not include plaintext secrets or raw credentials.\n");
    out.push_str("- Respect source retention and export policy; use short excerpts only.\n\n");
    out.push_str("## Required response envelope\n\n");
    out.push_str("```yaml\n");
    out.push_str(&serde_yaml::to_string(&response_template_for(request)).unwrap_or_default());
    out.push_str("```\n\n");
    out.push_str("## Request envelope\n\n");
    out.push_str("```yaml\n");
    out.push_str(&serde_yaml::to_string(request).unwrap_or_default());
    out.push_str("```\n");
    out
}
