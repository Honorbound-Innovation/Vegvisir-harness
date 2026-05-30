use crate::compiler;
use crate::domain;
use crate::ingest::stable_id;
use crate::models::*;
use crate::source_meta;
use anyhow::{Result, anyhow, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::process::Child;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForgeBundleSummary {
    pub summary_id: String,
    pub bundle_id: String,
    pub bundle_version: String,
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_provenance: Option<ForgeProviderProvenance>,
    pub pass_count: usize,
    pub passes: Vec<ForgePassSummary>,
    pub generated_skill_count: usize,
    pub modified_skill_count: usize,
    pub review_finding_count: usize,
    pub required_human_review: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub readiness_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForgePassSummary {
    pub request_id: String,
    pub pass_type: ForgePassType,
    pub generated_skill_count: usize,
    pub modified_skill_count: usize,
    pub review_finding_count: usize,
    pub required_human_review: bool,
    pub audit_note_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForgeProviderProvenance {
    pub provider: String,
    pub provider_display_name: String,
    pub deterministic: bool,
    pub live_reasoning: bool,
    pub adapter_mode: String,
    pub adapter_command_configured: bool,
    pub adapter_preflight_ok: Option<bool>,
    pub adapter_timeout_secs: Option<u64>,
    pub structured_envelope: bool,
    pub secret_safe: bool,
    pub caveats: Vec<String>,
}

pub fn summarize_forge_history(bundle: &SkillBundle) -> ForgeBundleSummary {
    let provider = bundle
        .forge_requests
        .first()
        .map(|request| request.provider.clone());
    let mut generated_skill_count = 0usize;
    let mut modified_skill_count = 0usize;
    let mut review_finding_count = 0usize;
    let mut required_human_review = false;
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    let mut readiness_notes = Vec::new();
    let mut passes = Vec::new();
    let provider_provenance = bundle
        .forge_requests
        .first()
        .and_then(|request| request.provider_provenance.clone())
        .or_else(|| provider_provenance_for(provider.as_deref()));

    for response in &bundle.forge_responses {
        generated_skill_count += response.generated_items.len();
        modified_skill_count += response.modified_items.len();
        review_finding_count += response.review_findings.len();
        required_human_review |= response.required_human_review;
        for finding in &response.review_findings {
            let lower = finding.to_ascii_lowercase();
            if lower.contains("block")
                || lower.contains("unsafe")
                || lower.contains("requires review")
                || lower.contains("approval")
            {
                push_unique_string(&mut blockers, finding.clone());
            } else if lower.contains("warning")
                || lower.contains("missing")
                || lower.contains("speculative")
            {
                push_unique_string(&mut warnings, finding.clone());
            } else if matches!(response.pass_type, ForgePassType::RegistryReadiness) {
                push_unique_string(&mut readiness_notes, finding.clone());
            }
        }
        passes.push(ForgePassSummary {
            request_id: response.request_id.clone(),
            pass_type: response.pass_type.clone(),
            generated_skill_count: response.generated_items.len(),
            modified_skill_count: response.modified_items.len(),
            review_finding_count: response.review_findings.len(),
            required_human_review: response.required_human_review,
            audit_note_count: response.audit_notes.len(),
        });
    }

    ForgeBundleSummary {
        summary_id: stable_id(
            "forge-summary",
            &format!(
                "{}:{}:{}:{}:{}",
                bundle.package.bundle_id,
                bundle.package.version,
                provider.clone().unwrap_or_default(),
                bundle
                    .forge_requests
                    .iter()
                    .map(|request| request.request_id.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                bundle
                    .forge_responses
                    .iter()
                    .map(|response| response.request_id.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ),
        bundle_id: bundle.package.bundle_id.clone(),
        bundle_version: bundle.package.version.clone(),
        provider,
        provider_provenance,
        pass_count: passes.len(),
        passes,
        generated_skill_count,
        modified_skill_count,
        review_finding_count,
        required_human_review,
        blockers,
        warnings,
        readiness_notes,
    }
}

pub fn forge_summary_markdown(summary: &ForgeBundleSummary) -> String {
    let mut out = String::new();
    out.push_str("# Forge Summary\n\n");
    out.push_str(&format!("- Summary ID: `{}`\n", summary.summary_id));
    out.push_str(&format!("- Bundle: `{}`\n", summary.bundle_id));
    out.push_str(&format!("- Version: `{}`\n", summary.bundle_version));
    out.push_str(&format!(
        "- Provider: `{}`\n",
        summary.provider.as_deref().unwrap_or("unknown")
    ));
    if let Some(provenance) = &summary.provider_provenance {
        out.push_str(&format!("- Provider mode: `{}`\n", provenance.adapter_mode));
        out.push_str(&format!(
            "- Deterministic provider: {}\n",
            provenance.deterministic
        ));
        out.push_str(&format!(
            "- Live reasoning: {}\n",
            provenance.live_reasoning
        ));
        out.push_str(&format!(
            "- Adapter configured: {}\n",
            provenance.adapter_command_configured
        ));
        if let Some(ok) = provenance.adapter_preflight_ok {
            out.push_str(&format!("- Adapter preflight OK: {}\n", ok));
        }
    }
    out.push_str(&format!("- Passes: {}\n", summary.pass_count));
    out.push_str(&format!(
        "- Generated skills: {}\n",
        summary.generated_skill_count
    ));
    out.push_str(&format!(
        "- Modified skills: {}\n",
        summary.modified_skill_count
    ));
    out.push_str(&format!(
        "- Review findings: {}\n",
        summary.review_finding_count
    ));
    out.push_str(&format!(
        "- Human review required: {}\n\n",
        summary.required_human_review
    ));

    out.push_str("## Passes\n\n");
    for pass in &summary.passes {
        out.push_str(&format!(
            "- `{:?}` (`{}`): {} modified, {} generated, {} findings, human_review={}\n",
            pass.pass_type,
            pass.request_id,
            pass.modified_skill_count,
            pass.generated_skill_count,
            pass.review_finding_count,
            pass.required_human_review
        ));
    }

    out.push_str("\n## Blockers\n\n");
    if summary.blockers.is_empty() {
        out.push_str("- None recorded.\n");
    } else {
        for blocker in &summary.blockers {
            out.push_str(&format!("- {blocker}\n"));
        }
    }

    out.push_str("\n## Warnings\n\n");
    if summary.warnings.is_empty() {
        out.push_str("- None recorded.\n");
    } else {
        for warning in &summary.warnings {
            out.push_str(&format!("- {warning}\n"));
        }
    }

    out.push_str("\n## Registry Readiness Notes\n\n");
    if summary.readiness_notes.is_empty() {
        out.push_str("- None recorded.\n");
    } else {
        for note in &summary.readiness_notes {
            out.push_str(&format!("- {note}\n"));
        }
    }
    out
}

pub fn validate_forge_summary_artifacts(
    root: &std::path::Path,
    bundle: &SkillBundle,
) -> Result<()> {
    let has_history = !bundle.forge_requests.is_empty() || !bundle.forge_responses.is_empty();
    let yaml_path = root.join("forge_summary.yaml");
    let md_path = root.join("forge_summary.md");

    if !has_history {
        if yaml_path.exists() || md_path.exists() {
            bail!("Forge summary artifacts exist but bundle has no stored Forge history");
        }
        return Ok(());
    }

    if !yaml_path.exists() {
        bail!("missing forge_summary.yaml for bundle with stored Forge history");
    }
    if !md_path.exists() {
        bail!("missing forge_summary.md for bundle with stored Forge history");
    }

    let stored: ForgeBundleSummary = serde_yaml::from_str(
        &std::fs::read_to_string(&yaml_path)
            .map_err(|err| anyhow!("read {}: {err}", yaml_path.display()))?,
    )
    .map_err(|err| anyhow!("parse {}: {err}", yaml_path.display()))?;
    let expected = summarize_forge_history(bundle);
    if stored != expected {
        bail!("forge_summary.yaml is stale or does not match stored Forge history");
    }

    let stored_md = std::fs::read_to_string(&md_path)
        .map_err(|err| anyhow!("read {}: {err}", md_path.display()))?;
    let expected_md = forge_summary_markdown(&expected);
    if stored_md != expected_md {
        bail!("forge_summary.md is stale or does not match stored Forge history");
    }

    Ok(())
}

fn push_unique_string(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ForgeProviderCatalog {
    pub default_provider: String,
    pub providers: Vec<ForgeProviderStatus>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ForgeProviderStatus {
    pub name: String,
    pub display_name: String,
    pub available: bool,
    pub deterministic: bool,
    pub live_reasoning: bool,
    pub structured_envelope: bool,
    pub secret_safe: bool,
    pub supported_passes: Vec<ForgePassType>,
    pub adapter_command_configured: bool,
    pub adapter_command_env: Option<String>,
    pub adapter_command: Option<String>,
    pub adapter_mode: String,
    pub adapter_timeout_secs: Option<u64>,
    pub adapter_timeout_env: Option<String>,
    pub adapter_preflight_ok: Option<bool>,
    pub adapter_preflight_errors: Vec<String>,
    pub required_setup: Vec<String>,
    pub caveats: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForgeAdapterPreflightReport {
    pub provider: String,
    pub valid: bool,
    pub adapter_command_env: String,
    pub adapter_command_configured: bool,
    pub adapter_command: Option<String>,
    pub adapter_timeout_env: String,
    pub adapter_timeout_secs: u64,
    pub checks: Vec<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForgeAdapterSelfTestReport {
    pub provider: String,
    pub valid: bool,
    pub request_id: String,
    pub pass_type: ForgePassType,
    pub adapter_command_env: String,
    pub adapter_command_configured: bool,
    pub adapter_command: Option<String>,
    pub adapter_timeout_env: String,
    pub adapter_timeout_secs: u64,
    pub preflight_valid: bool,
    pub response_parsed: bool,
    pub response_valid: bool,
    pub validation_report: Option<ForgeValidationReport>,
    pub audit_notes: Vec<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn provider_catalog() -> ForgeProviderCatalog {
    ForgeProviderCatalog {
        default_provider: "mock".into(),
        providers: vec![mock_provider_status(), vegvisir_provider_status()],
    }
}

pub fn provider_status(name: &str) -> Result<ForgeProviderStatus> {
    provider_catalog()
        .providers
        .into_iter()
        .find(|provider| provider.name == name)
        .ok_or_else(|| anyhow!("unsupported forge provider '{name}'"))
}

fn provider_provenance_for(name: Option<&str>) -> Option<ForgeProviderProvenance> {
    let name = name?;
    let status = provider_status(name).ok()?;
    Some(ForgeProviderProvenance {
        provider: status.name,
        provider_display_name: status.display_name,
        deterministic: status.deterministic,
        live_reasoning: status.live_reasoning,
        adapter_mode: status.adapter_mode,
        adapter_command_configured: status.adapter_command_configured,
        adapter_preflight_ok: status.adapter_preflight_ok,
        adapter_timeout_secs: status.adapter_timeout_secs,
        structured_envelope: status.structured_envelope,
        secret_safe: status.secret_safe,
        caveats: status.caveats,
    })
}

fn all_forge_passes() -> Vec<ForgePassType> {
    vec![
        ForgePassType::Interpretation,
        ForgePassType::SkillExpansion,
        ForgePassType::SkillInference,
        ForgePassType::DeduplicationAndScope,
        ForgePassType::SafetyAndGovernance,
        ForgePassType::EvalGeneration,
        ForgePassType::Critique,
        ForgePassType::VerifierReview,
        ForgePassType::AgentRoleMapping,
        ForgePassType::RegistryReadiness,
    ]
}

fn mock_provider_status() -> ForgeProviderStatus {
    ForgeProviderStatus {
        name: "mock".into(),
        display_name: "Deterministic local Forge provider".into(),
        available: true,
        deterministic: true,
        live_reasoning: false,
        structured_envelope: true,
        secret_safe: true,
        supported_passes: all_forge_passes(),
        adapter_command_configured: false,
        adapter_command_env: None,
        adapter_command: None,
        adapter_mode: "in-process deterministic".into(),
        adapter_timeout_secs: None,
        adapter_timeout_env: None,
        adapter_preflight_ok: None,
        adapter_preflight_errors: vec![],
        required_setup: vec![],
        caveats: vec![
            "Uses deterministic rule-based refinement; does not call an AI model.".into(),
            "Useful for tests, dry runs, GUI integration, and local governance validation.".into(),
        ],
    }
}

pub const VEGVISIR_FORGE_ADAPTER_COMMAND_ENV: &str = "SKILLER_VEGVISIR_FORGE_ADAPTER";
pub const VEGVISIR_FORGE_ADAPTER_TIMEOUT_ENV: &str = "SKILLER_VEGVISIR_FORGE_ADAPTER_TIMEOUT_SECS";
const DEFAULT_VEGVISIR_FORGE_ADAPTER_TIMEOUT_SECS: u64 = 120;
const MAX_VEGVISIR_FORGE_ADAPTER_TIMEOUT_SECS: u64 = 900;

fn vegvisir_adapter_command_configured() -> bool {
    std::env::var_os(VEGVISIR_FORGE_ADAPTER_COMMAND_ENV).is_some()
}

fn vegvisir_adapter_command() -> Option<String> {
    std::env::var(VEGVISIR_FORGE_ADAPTER_COMMAND_ENV)
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
}

pub fn vegvisir_adapter_preflight_report() -> ForgeAdapterPreflightReport {
    let configured = vegvisir_adapter_command_configured();
    let command = vegvisir_adapter_command();
    let timeout_secs = vegvisir_adapter_timeout_secs();
    let mut checks = vec![
        "adapter command environment inspected".to_string(),
        "adapter timeout parsed and bounded".to_string(),
    ];
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    match (&configured, &command) {
        (false, _) => {
            warnings.push(format!(
                "{VEGVISIR_FORGE_ADAPTER_COMMAND_ENV} is not set; vegvisir provider will use deterministic fallback"
            ));
        }
        (true, None) => {
            errors.push(format!(
                "{VEGVISIR_FORGE_ADAPTER_COMMAND_ENV} is set but empty"
            ));
        }
        (true, Some(command)) => {
            let path = Path::new(command);
            if !path.exists() {
                errors.push(format!("adapter command does not exist: {command}"));
            } else {
                checks.push("adapter command exists".to_string());
                match std::fs::metadata(path) {
                    Ok(metadata) => {
                        if !metadata.is_file() {
                            errors.push(format!("adapter command is not a file: {command}"));
                        } else {
                            checks.push("adapter command is a file".to_string());
                        }
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            if metadata.permissions().mode() & 0o111 == 0 {
                                errors
                                    .push(format!("adapter command is not executable: {command}"));
                            } else {
                                checks.push("adapter command is executable".to_string());
                            }
                        }
                    }
                    Err(err) => errors.push(format!(
                        "failed to inspect adapter command metadata for {command}: {err}"
                    )),
                }
            }
        }
    }

    ForgeAdapterPreflightReport {
        provider: "vegvisir".to_string(),
        valid: errors.is_empty(),
        adapter_command_env: VEGVISIR_FORGE_ADAPTER_COMMAND_ENV.to_string(),
        adapter_command_configured: configured,
        adapter_command: command,
        adapter_timeout_env: VEGVISIR_FORGE_ADAPTER_TIMEOUT_ENV.to_string(),
        adapter_timeout_secs: timeout_secs,
        checks,
        errors,
        warnings,
    }
}

fn vegvisir_provider_status() -> ForgeProviderStatus {
    let preflight = vegvisir_adapter_preflight_report();
    let adapter_command_configured = preflight.adapter_command_configured;
    let adapter_preflight_ok = preflight.valid;
    let mut required_setup = vec![
        "Keep provider credentials behind Vegvisir/HBSE; do not place secrets in Forge requests."
            .into(),
    ];
    let mut caveats = vec![
        "Skiller sends and accepts strict ForgeRequestEnvelope/ForgeResponseEnvelope YAML; malformed output is rejected.".into(),
    ];

    if adapter_command_configured {
        if adapter_preflight_ok {
            caveats.push(
                "External Vegvisir Forge adapter command is configured and passed preflight; Skiller will treat provider output as live external reasoning and validate it strictly.".into(),
            );
        } else {
            required_setup.push(
                "Fix the configured Vegvisir Forge adapter command; preflight currently fails."
                    .into(),
            );
            caveats.push(
                "External Vegvisir Forge adapter command is configured but failed preflight; Forge execution will fail until fixed.".into(),
            );
        }
    } else {
        required_setup.push(
            format!(
                "Set {VEGVISIR_FORGE_ADAPTER_COMMAND_ENV} to a Vegvisir-managed adapter command for live reasoning."
            ),
        );
        caveats.push(
            "No external adapter command is configured; provider uses deterministic local refinement while preserving Vegvisir-generated-agent metadata.".into(),
        );
    }

    ForgeProviderStatus {
        name: "vegvisir".into(),
        display_name: "Vegvisir structured-envelope Forge adapter".into(),
        available: !adapter_command_configured || adapter_preflight_ok,
        deterministic: !adapter_command_configured,
        live_reasoning: adapter_command_configured && adapter_preflight_ok,
        structured_envelope: true,
        secret_safe: true,
        supported_passes: all_forge_passes(),
        adapter_command_configured,
        adapter_command_env: Some(VEGVISIR_FORGE_ADAPTER_COMMAND_ENV.into()),
        adapter_command: preflight.adapter_command.clone(),
        adapter_mode: if adapter_command_configured {
            if adapter_preflight_ok {
                "external strict-envelope command".into()
            } else {
                "external strict-envelope command (preflight failed)".into()
            }
        } else {
            "deterministic fallback".into()
        },
        adapter_timeout_secs: Some(preflight.adapter_timeout_secs),
        adapter_timeout_env: Some(VEGVISIR_FORGE_ADAPTER_TIMEOUT_ENV.into()),
        adapter_preflight_ok: Some(adapter_preflight_ok),
        adapter_preflight_errors: preflight.errors.clone(),
        required_setup,
        caveats,
    }
}

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
                ForgePassType::Critique => {
                    review_findings.extend(critique_findings_for_skill(skill));
                }
                ForgePassType::VerifierReview => {
                    review_findings.extend(verifier_findings_for_skill(skill));
                }
                ForgePassType::RegistryReadiness => {
                    review_findings.extend(registry_readiness_findings_for_skill(skill));
                }
                ForgePassType::SkillInference => {}
                ForgePassType::DeduplicationAndScope => {
                    review_findings.push(format!(
                        "scope review: {} has scope {:?} and should remain bounded to a reusable task/workflow",
                        skill.id, skill.scope
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
        if vegvisir_adapter_command_configured() {
            return run_vegvisir_adapter_command(request);
        }

        // This is the stable in-process adapter boundary for Vegvisir runtime integration.
        // It intentionally accepts and returns strict envelopes, avoids plaintext credentials,
        // and falls back to deterministic behavior unless an explicit Vegvisir-managed adapter
        // command is configured through SKILLER_VEGVISIR_FORGE_ADAPTER.
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
            "Vegvisir adapter used structured request/response envelope with deterministic fallback; configure SKILLER_VEGVISIR_FORGE_ADAPTER for live external reasoning.".into(),
        );
        Ok(response)
    }
}

fn spawn_adapter_command(command: &str) -> std::io::Result<Child> {
    let mut cmd = std::process::Command::new(command);
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    cmd.spawn()
}

fn terminate_adapter_process_group(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}

fn vegvisir_adapter_timeout_secs() -> u64 {
    std::env::var(VEGVISIR_FORGE_ADAPTER_TIMEOUT_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .map(|value| value.min(MAX_VEGVISIR_FORGE_ADAPTER_TIMEOUT_SECS))
        .unwrap_or(DEFAULT_VEGVISIR_FORGE_ADAPTER_TIMEOUT_SECS)
}

fn truncate_diagnostic(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .take(2000)
        .collect::<String>()
}

fn wait_for_adapter_output(
    mut child: std::process::Child,
    command: &str,
    timeout: Duration,
) -> Result<std::process::Output> {
    let started = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|err| anyhow!("failed waiting for Vegvisir Forge adapter '{command}': {err}"))?
            .is_some()
        {
            return child.wait_with_output().map_err(|err| {
                anyhow!("failed collecting Vegvisir Forge adapter '{command}' output: {err}")
            });
        }

        if started.elapsed() >= timeout {
            terminate_adapter_process_group(&mut child);
            let output = child.wait_with_output().ok();
            let stderr = output
                .as_ref()
                .map(|output| truncate_diagnostic(&output.stderr))
                .unwrap_or_default();
            bail!(
                "Vegvisir Forge adapter '{command}' timed out after {} seconds{}",
                timeout.as_secs(),
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {stderr}")
                }
            );
        }

        std::thread::sleep(Duration::from_millis(25));
    }
}

fn minimal_adapter_self_test_bundle_and_request() -> (SkillBundle, ForgeRequestEnvelope) {
    let now = Utc::now();
    let source = SourceDocument {
        source_id: "self-test-source".into(),
        title: "Vegvisir Forge Adapter Self-Test Source".into(),
        source_type: SourceType::Markdown,
        origin: "skiller://forge-adapter-self-test".into(),
        version: Some("self-test".into()),
        license: None,
        owner: Some("skiller".into()),
        visibility: Visibility::Private,
        ingested_at: now,
        hash: "self-test-source-hash".into(),
        retention_policy: RetentionPolicy::ExcerptsOnly,
        export_policy: ExportPolicy::PrivateOnly,
        secret_scan_status: ScanStatus::Clean,
        permission_status: PermissionStatus::Allowed,
        citation_policy: CitationPolicy::ShortExcerpts,
    };
    let section = DocumentSection {
        section_id: "self-test-section".into(),
        source_id: source.source_id.clone(),
        heading: "Adapter self-test".into(),
        breadcrumbs: vec!["Adapter self-test".into()],
        line_start: 1,
        line_end: 1,
        text_excerpt:
            "Use this synthetic section only to prove strict Forge envelope compatibility.".into(),
        code_blocks: vec![],
        links: vec![],
        detected_commands: vec![],
        detected_api_operations: vec![],
        detected_warnings: vec![],
        detected_examples: vec!["Return an empty but valid ForgeResponseEnvelope.".into()],
        detected_normative_language: vec!["must return matching request_id and pass_type".into()],
    };
    let candidate = CapabilityCandidate {
        candidate_id: "self-test-candidate".into(),
        source_section_ids: vec![section.section_id.clone()],
        candidate_title: "Validate strict Forge adapter envelope".into(),
        candidate_type: SkillType::Review,
        detected_task:
            "Return a syntactically valid ForgeResponseEnvelope without mutating skills.".into(),
        detected_inputs: vec!["ForgeRequestEnvelope".into()],
        detected_outputs: vec!["ForgeResponseEnvelope".into()],
        detected_procedures: vec![
            "Read request_id and pass_type from the request and echo them in the response.".into(),
        ],
        detected_warnings: vec!["Do not include secrets or raw external content.".into()],
        candidate_confidence: 1.0,
        evidence_strength: 1.0,
        extraction_type: EvidenceClass::DirectExtraction,
        related_candidates: vec![],
    };
    let bundle = SkillBundle {
        package: SkillPackage {
            bundle_id: "self-test-bundle".into(),
            name: "forge-adapter-self-test".into(),
            version: "0.0.0-self-test".into(),
            domain: Some("skiller-forge-adapter".into()),
            source_corpus: vec![source.origin.clone()],
            review_status: SkillStatus::Draft,
            publish_status: PublishStatus::Unpublished,
            compatibility: BTreeMap::new(),
            created_at: now,
        },
        sources: vec![source],
        sections: vec![section],
        capability_candidates: vec![candidate],
        skills: vec![],
        graph: SkillGraph::default(),
        audit_events: vec![],
        forge_requests: vec![],
        forge_responses: vec![],
    };
    let request = ForgeRequestEnvelope {
        request_id: stable_id("forge-adapter-self-test", "vegvisir:strict-envelope"),
        provider: "vegvisir".into(),
        provider_provenance: provider_provenance_for(Some("vegvisir")),
        pass_type: ForgePassType::Interpretation,
        bundle_id: bundle.package.bundle_id.clone(),
        bundle_version: bundle.package.version.clone(),
        domain_profile: None,
        source_sections: bundle.sections.iter().map(|section| ForgeSectionPacket {
            section_id: section.section_id.clone(),
            source_id: section.source_id.clone(),
            heading: section.heading.clone(),
            excerpt: section.text_excerpt.clone(),
            detected_commands: section.detected_commands.clone(),
            detected_api_operations: section.detected_api_operations.clone(),
            detected_warnings: section.detected_warnings.clone(),
        }).collect(),
        candidate_skills: vec![],
        capability_candidates: bundle.capability_candidates.clone(),
        citation_ids: vec![],
        source_context: vec![ForgeSourceContext {
            source_id: "self-test-source".into(),
            title: "Vegvisir Forge Adapter Self-Test Source".into(),
            source_type: SourceType::Markdown,
            origin: "skiller://forge-adapter-self-test".into(),
            version: Some("self-test".into()),
            source_trust: "InternalCompanyDocumentation".into(),
            export_policy: ExportPolicy::PrivateOnly,
            permission_status: PermissionStatus::Allowed,
            secret_scan_status: ScanStatus::Clean,
            section_count: 1,
            selected_section_count: 1,
            skill_count: 0,
        }],
        bundle_context: ForgeBundleContext {
            bundle_name: bundle.package.name.clone(),
            domain: bundle.package.domain.clone(),
            review_status: bundle.package.review_status.clone(),
            publish_status: bundle.package.publish_status.clone(),
            compatibility: BTreeMap::new(),
            total_source_count: 1,
            total_section_count: 1,
            total_skill_count: 0,
            selected_skill_count: 0,
            high_risk_skill_count: 0,
            inference_record_count: 0,
            existing_forge_request_count: 0,
            existing_forge_response_count: 0,
        },
        validation_constraints: validation_constraints_for(&ForgePassType::Interpretation),
        response_schema_guide: response_schema_guide_for(&ForgePassType::Interpretation),
        prior_forge_summary: vec![],
        graph_concepts: vec![],
        task_instruction: "Self-test the configured Vegvisir Forge adapter by returning a valid empty ForgeResponseEnvelope with matching request_id and pass_type. Do not generate or modify skills.".into(),
        output_schema: "ForgeResponseEnvelope YAML".into(),
        token_budget: 1024,
        risk_policy: "Self-test only: no secrets, no tool execution, no skill mutation, no registry publication.".into(),
        created_at: now,
    };
    (bundle, request)
}

pub fn vegvisir_adapter_self_test_report() -> ForgeAdapterSelfTestReport {
    let preflight = vegvisir_adapter_preflight_report();
    let (bundle, request) = minimal_adapter_self_test_bundle_and_request();
    let mut report = ForgeAdapterSelfTestReport {
        provider: "vegvisir".into(),
        valid: false,
        request_id: request.request_id.clone(),
        pass_type: request.pass_type.clone(),
        adapter_command_env: VEGVISIR_FORGE_ADAPTER_COMMAND_ENV.into(),
        adapter_command_configured: preflight.adapter_command_configured,
        adapter_command: preflight.adapter_command.clone(),
        adapter_timeout_env: VEGVISIR_FORGE_ADAPTER_TIMEOUT_ENV.into(),
        adapter_timeout_secs: preflight.adapter_timeout_secs,
        preflight_valid: preflight.valid,
        response_parsed: false,
        response_valid: false,
        validation_report: None,
        audit_notes: vec![],
        errors: vec![],
        warnings: preflight.warnings.clone(),
    };

    if !preflight.valid {
        report.errors.extend(preflight.errors.clone());
        if !preflight.adapter_command_configured {
            report.errors.push(format!(
                "{VEGVISIR_FORGE_ADAPTER_COMMAND_ENV} must be configured for adapter self-test"
            ));
        }
        return report;
    }
    if !preflight.adapter_command_configured {
        report.errors.push(format!(
            "{VEGVISIR_FORGE_ADAPTER_COMMAND_ENV} must be configured for adapter self-test"
        ));
        return report;
    }

    match run_vegvisir_adapter_command(&request) {
        Ok(response) => {
            report.response_parsed = true;
            report.audit_notes = response.audit_notes.clone();
            let validation = validate_response_report(&bundle, &request, &response);
            report.response_valid = validation.valid;
            if validation.valid {
                report.valid = true;
            } else {
                report.errors.extend(validation.errors.clone());
            }
            report.validation_report = Some(validation);
        }
        Err(err) => {
            report.errors.push(err.to_string());
        }
    }

    report
}

fn run_vegvisir_adapter_command(request: &ForgeRequestEnvelope) -> Result<ForgeResponseEnvelope> {
    let command = std::env::var(VEGVISIR_FORGE_ADAPTER_COMMAND_ENV)
        .map_err(|err| anyhow!("{VEGVISIR_FORGE_ADAPTER_COMMAND_ENV} is not configured: {err}"))?;
    if command.trim().is_empty() {
        bail!("{VEGVISIR_FORGE_ADAPTER_COMMAND_ENV} is empty");
    }
    let preflight = vegvisir_adapter_preflight_report();
    if !preflight.valid {
        bail!(
            "Vegvisir Forge adapter preflight failed: {}",
            preflight.errors.join("; ")
        );
    }

    let timeout_secs = vegvisir_adapter_timeout_secs();
    let request_yaml = serde_yaml::to_string(request)?;
    let mut child = spawn_adapter_command(&command)
        .map_err(|err| anyhow!("failed to start Vegvisir Forge adapter '{command}': {err}"))?;

    {
        let mut stdin = child.stdin.take().ok_or_else(|| {
            anyhow!("failed to open stdin for Vegvisir Forge adapter '{command}'")
        })?;
        stdin.write_all(request_yaml.as_bytes()).map_err(|err| {
            anyhow!("failed to write Forge request to Vegvisir Forge adapter '{command}': {err}")
        })?;
    }

    let output = wait_for_adapter_output(child, &command, Duration::from_secs(timeout_secs))?;
    if !output.status.success() {
        let stderr = truncate_diagnostic(&output.stderr);
        let stdout = truncate_diagnostic(&output.stdout);
        bail!(
            "Vegvisir Forge adapter '{command}' failed with status {}: stderr={stderr}; stdout={stdout}",
            output.status
        );
    }

    let response: ForgeResponseEnvelope = serde_yaml::from_slice(&output.stdout).map_err(|err| {
        let stdout = truncate_diagnostic(&output.stdout);
        let stderr = truncate_diagnostic(&output.stderr);
        anyhow!("Vegvisir Forge adapter '{command}' returned invalid ForgeResponseEnvelope YAML: {err}; stdout={stdout}; stderr={stderr}")
    })?;
    Ok(response)
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
        ForgePassType::SkillInference,
        ForgePassType::SafetyAndGovernance,
        ForgePassType::EvalGeneration,
        ForgePassType::Critique,
        ForgePassType::VerifierReview,
        ForgePassType::AgentRoleMapping,
        ForgePassType::RegistryReadiness,
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
    let source_sections: Vec<ForgeSectionPacket> = bundle
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
    let selected_source_ids: BTreeSet<String> = source_sections
        .iter()
        .map(|s| s.source_id.clone())
        .collect();
    let source_context = bundle
        .sources
        .iter()
        .filter(|src| selected_source_ids.contains(&src.source_id))
        .map(|src| {
            let section_count = bundle
                .sections
                .iter()
                .filter(|section| section.source_id == src.source_id)
                .count();
            let selected_section_count = source_sections
                .iter()
                .filter(|section| section.source_id == src.source_id)
                .count();
            let skill_count = selected_skills
                .iter()
                .filter(|skill| {
                    skill.source_section_ids.iter().any(|section_id| {
                        bundle.sections.iter().any(|section| {
                            section.section_id == *section_id && section.source_id == src.source_id
                        })
                    })
                })
                .count();
            ForgeSourceContext {
                source_id: src.source_id.clone(),
                title: src.title.clone(),
                source_type: src.source_type.clone(),
                origin: src.origin.clone(),
                version: src.version.clone(),
                source_trust: format!("{:?}", source_meta::infer_source_trust(src)),
                export_policy: src.export_policy.clone(),
                permission_status: src.permission_status.clone(),
                secret_scan_status: src.secret_scan_status.clone(),
                section_count,
                selected_section_count,
                skill_count,
            }
        })
        .collect();
    let high_risk_skill_count = selected_skills
        .iter()
        .filter(|skill| {
            skill.runtime_policy.modify_external_systems
                || skill.runtime_policy.handles_secrets
                || (skill.runtime_policy.modify_files
                    && !skill.runtime_policy.requires_backup_or_rollback)
        })
        .count();
    let inference_record_count = selected_skills
        .iter()
        .map(|skill| skill.inference_records.len())
        .sum();
    let prior_forge_summary = summarize_forge_history(bundle)
        .passes
        .into_iter()
        .map(|pass| {
            format!(
                "{:?}: generated={}, modified={}, findings={}, human_review={}",
                pass.pass_type,
                pass.generated_skill_count,
                pass.modified_skill_count,
                pass.review_finding_count,
                pass.required_human_review
            )
        })
        .collect();
    let bundle_context = ForgeBundleContext {
        bundle_name: bundle.package.name.clone(),
        domain: bundle.package.domain.clone(),
        review_status: bundle.package.review_status.clone(),
        publish_status: bundle.package.publish_status.clone(),
        compatibility: bundle.package.compatibility.clone(),
        total_source_count: bundle.sources.len(),
        total_section_count: bundle.sections.len(),
        total_skill_count: bundle.skills.len(),
        selected_skill_count: selected_skills.len(),
        high_risk_skill_count,
        inference_record_count,
        existing_forge_request_count: bundle.forge_requests.len(),
        existing_forge_response_count: bundle.forge_responses.len(),
    };
    let validation_constraints = validation_constraints_for(&pass_type);
    let response_schema_guide = response_schema_guide_for(&pass_type);
    ForgeRequestEnvelope {
        request_id: stable_forge_request_id(bundle, provider, &pass_type, max_skills),
        provider: provider.into(),
        provider_provenance: provider_provenance_for(Some(provider)),
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
        source_context,
        bundle_context,
        validation_constraints,
        response_schema_guide,
        prior_forge_summary,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForgeApplyReport {
    pub apply_id: String,
    pub request_id: String,
    pub pass_type: ForgePassType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_provenance: Option<ForgeProviderProvenance>,
    pub valid: bool,
    pub before_skill_count: usize,
    pub after_skill_count: usize,
    pub generated_skill_count: usize,
    pub modified_skill_count: usize,
    pub evidence_record_count: usize,
    pub review_finding_count: usize,
    pub confidence_update_count: usize,
    pub required_human_review: bool,
    pub applied_skill_ids: Vec<String>,
    pub generated_skill_ids: Vec<String>,
    pub modified_skill_ids: Vec<String>,
    pub review_findings: Vec<String>,
    pub validation_errors: Vec<String>,
}

pub fn apply_external_response(
    bundle: SkillBundle,
    request: ForgeRequestEnvelope,
    response: ForgeResponseEnvelope,
) -> Result<SkillBundle> {
    let (bundle, _) = apply_external_response_with_report(bundle, request, response)?;
    Ok(bundle)
}

pub fn apply_external_response_with_report(
    mut bundle: SkillBundle,
    request: ForgeRequestEnvelope,
    response: ForgeResponseEnvelope,
) -> Result<(SkillBundle, ForgeApplyReport)> {
    let validation_report = validate_response_report(&bundle, &request, &response);
    let request_provider = request.provider.clone();
    let before_skill_count = bundle.skills.len();
    let generated_skill_ids = response
        .generated_items
        .iter()
        .map(|skill| skill.id.clone())
        .collect::<Vec<_>>();
    let modified_skill_ids = response
        .modified_items
        .iter()
        .map(|skill| skill.id.clone())
        .collect::<Vec<_>>();
    let mut applied_skill_ids = modified_skill_ids.clone();
    for id in &generated_skill_ids {
        if !applied_skill_ids.iter().any(|existing| existing == id) {
            applied_skill_ids.push(id.clone());
        }
    }

    if !validation_report.valid {
        let report = ForgeApplyReport {
            apply_id: stable_id(
                "forge-apply",
                &format!(
                    "{}:{}:{}:invalid",
                    bundle.package.bundle_id, bundle.package.version, response.request_id
                ),
            ),
            request_id: response.request_id.clone(),
            pass_type: response.pass_type.clone(),
            provider_provenance: provider_provenance_for(Some(&request_provider)),
            valid: false,
            before_skill_count,
            after_skill_count: before_skill_count,
            generated_skill_count: response.generated_items.len(),
            modified_skill_count: response.modified_items.len(),
            evidence_record_count: response.evidence_records.len(),
            review_finding_count: response.review_findings.len(),
            confidence_update_count: response.confidence_updates.len(),
            required_human_review: response.required_human_review,
            applied_skill_ids,
            generated_skill_ids,
            modified_skill_ids,
            review_findings: response.review_findings.clone(),
            validation_errors: validation_report.errors,
        };
        bail!(
            "Forge response is invalid for request {}: {}",
            report.request_id,
            report.validation_errors.join("; ")
        );
    }

    apply_response(&mut bundle, &response);
    let after_skill_count = bundle.skills.len();
    bundle.forge_requests.push(request);
    bundle.forge_responses.push(response.clone());
    bundle.audit_events.push(compiler::audit(
        "forge-external-apply",
        "applied externally generated Forge response after validation",
    ));
    let report = ForgeApplyReport {
        apply_id: stable_id(
            "forge-apply",
            &format!(
                "{}:{}:{}:{}:{}",
                bundle.package.bundle_id,
                bundle.package.version,
                response.request_id,
                generated_skill_ids.join(","),
                modified_skill_ids.join(",")
            ),
        ),
        request_id: response.request_id.clone(),
        pass_type: response.pass_type.clone(),
        provider_provenance: provider_provenance_for(Some(&request_provider)),
        valid: true,
        before_skill_count,
        after_skill_count,
        generated_skill_count: response.generated_items.len(),
        modified_skill_count: response.modified_items.len(),
        evidence_record_count: response.evidence_records.len(),
        review_finding_count: response.review_findings.len(),
        confidence_update_count: response.confidence_updates.len(),
        required_human_review: response.required_human_review,
        applied_skill_ids,
        generated_skill_ids,
        modified_skill_ids,
        review_findings: response.review_findings.clone(),
        validation_errors: vec![],
    };
    Ok((bundle, report))
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

fn response_schema_guide_for(pass_type: &ForgePassType) -> ForgeResponseSchemaGuide {
    let mut minimal = BTreeMap::new();
    minimal.insert("request_id".into(), "must equal request.request_id".into());
    minimal.insert("pass_type".into(), format!("must equal {:?}", pass_type));
    minimal.insert(
        "generated_items".into(),
        "[] when no new skills are proposed".into(),
    );
    minimal.insert(
        "modified_items".into(),
        "[] when no existing skills are modified".into(),
    );
    minimal.insert(
        "review_findings".into(),
        "[] when there are no findings".into(),
    );
    minimal.insert(
        "confidence_updates".into(),
        "{} when no confidence changes are proposed".into(),
    );
    minimal.insert(
        "evidence_records".into(),
        "[] unless generated/modified items include inferred claims".into(),
    );
    minimal.insert(
        "required_human_review".into(),
        "true for inferred, risky, speculative, or operational changes".into(),
    );
    minimal.insert(
        "audit_notes".into(),
        "short non-secret notes explaining what the provider did".into(),
    );

    let mut skill_output_rules = vec![
        "Use existing skill IDs only in modified_items; use new stable IDs only for generated_items.".into(),
        "Every generated or modified skill must retain source_section_ids and citations that refer to request source_sections/citation_ids.".into(),
        "Do not set skills to Approved or Published; Forge output is staged for review.".into(),
        "Generated operational or inferred skills should use NeedsReview and Level2ForgeEnhanced at most.".into(),
        "Runtime permissions must be least-privilege and must require user approval for mutation.".into(),
    ];
    if matches!(pass_type, ForgePassType::RegistryReadiness) {
        skill_output_rules.push(
            "Prefer review_findings over skill mutation for registry readiness judgments.".into(),
        );
    }

    ForgeResponseSchemaGuide {
        envelope_type: "ForgeResponseEnvelope".into(),
        required_fields: vec![
            "request_id".into(),
            "pass_type".into(),
            "generated_items".into(),
            "modified_items".into(),
            "review_findings".into(),
            "confidence_updates".into(),
            "evidence_records".into(),
            "required_human_review".into(),
            "audit_notes".into(),
        ],
        field_guidance: vec![
            ForgeResponseFieldGuide { field: "request_id".into(), required: true, expected_type: "string".into(), guidance: "Must exactly match the request_id from the ForgeRequestEnvelope.".into() },
            ForgeResponseFieldGuide { field: "pass_type".into(), required: true, expected_type: "ForgePassType".into(), guidance: "Must exactly match the requested pass_type.".into() },
            ForgeResponseFieldGuide { field: "generated_items".into(), required: true, expected_type: "list<Skill>".into(), guidance: "New skills proposed by the pass. Include inference_records and required review for any inferred/synthesized skill.".into() },
            ForgeResponseFieldGuide { field: "modified_items".into(), required: true, expected_type: "list<Skill>".into(), guidance: "Full replacement Skill objects for existing skill IDs that should be updated.".into() },
            ForgeResponseFieldGuide { field: "review_findings".into(), required: true, expected_type: "list<string>".into(), guidance: "Concise blockers, warnings, readiness notes, critique, or verifier findings.".into() },
            ForgeResponseFieldGuide { field: "confidence_updates".into(), required: true, expected_type: "map<skill_id, ConfidenceBreakdown>".into(), guidance: "Only reference existing or generated skill IDs; all values must be finite 0.0..=1.0.".into() },
            ForgeResponseFieldGuide { field: "evidence_records".into(), required: true, expected_type: "list<InferenceRecord>".into(), guidance: "Evidence records for generated skills or new inferred claims; source_refs_used must refer to request sections.".into() },
            ForgeResponseFieldGuide { field: "required_human_review".into(), required: true, expected_type: "bool".into(), guidance: "Set true for inferred, speculative, mutating, high-risk, or governance-impacting outputs.".into() },
            ForgeResponseFieldGuide { field: "audit_notes".into(), required: true, expected_type: "list<string>".into(), guidance: "Short non-secret notes about pass decisions; do not include chain-of-thought or raw private source text.".into() },
        ],
        skill_output_rules,
        evidence_record_rules: vec![
            "Use evidence_type to distinguish DirectExtraction, SupportingInference, OperationalSynthesis, SpeculativeCandidate, CommunityDerived, or InternalPolicyDerived.".into(),
            "Set required_review=true for inferred, speculative, synthesized, high-risk, or weakly supported claims.".into(),
            "source_refs_used must reference section IDs present in source_sections.".into(),
            "unsupported_assumptions and risk_flags should be explicit when evidence is not direct extraction.".into(),
        ],
        confidence_update_rules: vec![
            "All confidence values must be finite numbers from 0.0 through 1.0.".into(),
            "Do not increase human_review confidence; only human/verifier review workflows should do that.".into(),
            "Low source trust, speculative evidence, missing evals, or missing guardrails should reduce confidence.".into(),
        ],
        forbidden_outputs: vec![
            "Plaintext secrets, credentials, private keys, tokens, or secret-bearing URLs.".into(),
            "Invented source IDs, section IDs, citation IDs, API endpoints, CLI flags, tool names, or version applicability.".into(),
            "Direct publication/approval status changes.".into(),
            "Large raw source excerpts beyond citation policy.".into(),
            "Mutation permissions without explicit approval and rollback/backup policy.".into(),
        ],
        minimal_valid_response: minimal,
    }
}

fn validation_constraints_for(pass_type: &ForgePassType) -> Vec<String> {
    let mut constraints = vec![
        "Return a ForgeResponseEnvelope with matching request_id and pass_type.".to_string(),
        "Every generated or modified skill must reference existing source sections through citations or source_section_ids.".to_string(),
        "Do not mark inferred/speculative content as direct extraction without evidence records.".to_string(),
        "Do not include plaintext secrets or secret-like values in generated text.".to_string(),
        "Do not invent API endpoints, CLI flags, tools, versions, or source claims without cited evidence.".to_string(),
        "External mutation permissions require explicit user approval, rollback guidance, and human review.".to_string(),
        "Confidence and evidence scores must be finite values in 0.0..=1.0.".to_string(),
    ];
    match pass_type {
        ForgePassType::SkillInference => constraints.push(
            "New inferred skills must include inference_records with source_refs, confidence, unsupported_assumptions, risk_flags, and required_review=true when evidence is not direct.".to_string(),
        ),
        ForgePassType::SafetyAndGovernance => constraints.push(
            "Classify tool requirements as read-only, mutating, dangerous, or external and add guardrails for operational workflows.".to_string(),
        ),
        ForgePassType::EvalGeneration => constraints.push(
            "Generated evals should cover routing, source grounding, safety, and tool-use planning for operational or high-risk skills.".to_string(),
        ),
        ForgePassType::RegistryReadiness => constraints.push(
            "Report blockers for unsafe, deprecated, archived, unreviewed, speculative, or source-rights-restricted skills.".to_string(),
        ),
        _ => {}
    }
    constraints
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForgeValidationReport {
    pub request_id: String,
    pub pass_type: ForgePassType,
    pub valid: bool,
    pub error_count: usize,
    pub errors: Vec<String>,
}

pub fn validate_response(
    bundle: &SkillBundle,
    request: &ForgeRequestEnvelope,
    response: &ForgeResponseEnvelope,
) -> Result<()> {
    let report = validate_response_report(bundle, request, response);
    if report.valid {
        Ok(())
    } else {
        bail!(report.errors.join("; "))
    }
}

pub fn validate_response_report(
    bundle: &SkillBundle,
    request: &ForgeRequestEnvelope,
    response: &ForgeResponseEnvelope,
) -> ForgeValidationReport {
    let mut errors = Vec::new();
    collect_response_validation_errors(bundle, request, response, &mut errors);
    ForgeValidationReport {
        request_id: response.request_id.clone(),
        pass_type: response.pass_type.clone(),
        valid: errors.is_empty(),
        error_count: errors.len(),
        errors,
    }
}

fn collect_response_validation_errors(
    bundle: &SkillBundle,
    request: &ForgeRequestEnvelope,
    response: &ForgeResponseEnvelope,
    errors: &mut Vec<String>,
) {
    if response.request_id != request.request_id {
        errors.push("forge response request_id mismatch".into());
    }
    if response.pass_type != request.pass_type {
        errors.push("forge response pass_type mismatch".into());
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
            errors.push("forge response contains skill with empty id".into());
        }
        match serde_yaml::to_string(skill) {
            Ok(skill_yaml) if crate::security::contains_secret_like_content(&skill_yaml) => {
                errors.push(format!(
                    "forge response contains secret-like content in {}",
                    skill.id
                ));
            }
            Ok(_) => {}
            Err(err) => errors.push(format!(
                "forge response skill {} could not be serialized for secret scan: {err}",
                skill.id
            )),
        }
        for sid in &skill.source_section_ids {
            if !section_ids.contains(sid.as_str()) {
                errors.push(format!(
                    "forge skill {} references missing section {}",
                    skill.id, sid
                ));
            }
        }
        collect_result_error(
            errors,
            validate_confidence_breakdown(
                &format!("forge skill {} confidence", skill.id),
                &skill.confidence,
            ),
        );
        collect_result_error(
            errors,
            validate_evidence_breakdown(
                &format!("forge skill {} evidence_breakdown", skill.id),
                &skill.evidence_breakdown,
            ),
        );
        for citation in &skill.citations {
            if !source_ids.contains(citation.source_id.as_str()) {
                errors.push(format!(
                    "forge skill {} citation {} references missing source {}",
                    skill.id, citation.citation_id, citation.source_id
                ));
            }
            match section_sources.get(citation.section_id.as_str()) {
                Some(section_source) if *section_source == citation.source_id.as_str() => {}
                Some(section_source) => errors.push(format!(
                    "forge skill {} citation {} source {} does not match section {} source {}",
                    skill.id,
                    citation.citation_id,
                    citation.source_id,
                    citation.section_id,
                    section_source
                )),
                None => errors.push(format!(
                    "forge skill {} references missing citation section {}",
                    skill.id, citation.section_id
                )),
            }
        }
        if !skill_ids.contains(skill.id.as_str()) && skill.inference_records.is_empty() {
            errors.push(format!(
                "new forge skill {} lacks inference records",
                skill.id
            ));
        }
        if skill.runtime_policy.modify_external_systems
            && !skill.runtime_policy.requires_user_approval
        {
            errors.push(format!(
                "forge skill {} modifies external systems without approval",
                skill.id
            ));
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
            errors.push(format!(
                "forge confidence update references unknown skill {skill_id}"
            ));
        }
        collect_result_error(
            errors,
            validate_confidence_breakdown(
                &format!("forge confidence update {skill_id}"),
                confidence,
            ),
        );
    }
    for record in &response.evidence_records {
        collect_result_error(
            errors,
            validate_probability(
                &format!("forge evidence record {} confidence", record.inference_id),
                record.confidence,
            ),
        );
        for sid in &record.source_refs_used {
            if !section_ids.contains(sid.as_str()) {
                errors.push(format!(
                    "forge evidence record references missing section {sid}"
                ));
            }
        }
    }
}

fn collect_result_error(errors: &mut Vec<String>, result: Result<()>) {
    if let Err(err) = result {
        errors.push(err.to_string());
    }
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

fn critique_findings_for_skill(skill: &Skill) -> Vec<String> {
    let mut findings = Vec::new();
    if skill.citations.is_empty() {
        findings.push(format!("BLOCKER: {} lacks citations", skill.id));
    }
    if skill.guardrails.len() < 2 {
        findings.push(format!("WARNING: {} has thin guardrails", skill.id));
    }
    if skill.evals.is_empty() {
        findings.push(format!("WARNING: {} lacks eval coverage", skill.id));
    }
    if skill.evidence_breakdown.speculative_candidate > 0.2 {
        findings.push(format!(
            "REVIEW: {} has speculative evidence above review threshold",
            skill.id
        ));
    }
    if findings.is_empty() {
        findings.push(format!(
            "OK: {} has citations, guardrails, and reviewable structure",
            skill.id
        ));
    }
    findings
}

fn verifier_findings_for_skill(skill: &Skill) -> Vec<String> {
    let mut findings = Vec::new();
    for citation in &skill.citations {
        findings.push(format!(
            "VERIFY: {} cites section {} from source {}",
            skill.id, citation.section_id, citation.source_id
        ));
    }
    for record in &skill.inference_records {
        if record.required_review {
            findings.push(format!(
                "REVIEW: {} inference {} requires human review",
                skill.id, record.inference_id
            ));
        }
    }
    if skill.runtime_policy.modify_external_systems && !skill.runtime_policy.requires_user_approval
    {
        findings.push(format!(
            "BLOCKER: {} modifies external systems without approval",
            skill.id
        ));
    }
    if findings.is_empty() {
        findings.push(format!(
            "VERIFY: {} has no inferred or mutating claims requiring escalation",
            skill.id
        ));
    }
    findings
}

fn registry_readiness_findings_for_skill(skill: &Skill) -> Vec<String> {
    let mut findings = Vec::new();
    match skill.status {
        SkillStatus::Reviewed | SkillStatus::Approved | SkillStatus::Published => {
            findings.push(format!(
                "READY-CANDIDATE: {} status is {:?}",
                skill.id, skill.status
            ));
        }
        SkillStatus::Unsafe | SkillStatus::Archived | SkillStatus::Deprecated => {
            findings.push(format!(
                "BLOCKER: {} status {:?} is not publishable",
                skill.id, skill.status
            ));
        }
        _ => {
            findings.push(format!(
                "REVIEW: {} status {:?} requires review before publication",
                skill.id, skill.status
            ));
        }
    }
    if skill.inference_records.iter().any(|r| r.required_review) {
        findings.push(format!(
            "REVIEW: {} has inference records that require human review",
            skill.id
        ));
    }
    findings
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
    out.push_str("## Context fields to use\n\n");
    out.push_str("- `bundle_context` summarizes review/publish status, compatibility, prior Forge history counts, and selected skill counts.\n");
    out.push_str("- `source_context` summarizes source trust, version, rights, scan status, and selected sections without exposing full raw documents.\n");
    out.push_str("- `validation_constraints` are hard requirements enforced by Skiller before any response can be applied.\n");
    out.push_str("- `prior_forge_summary` summarizes prior Forge passes; avoid duplicating prior recommendations unless still unresolved.\n\n");
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
    out.push_str("## Response schema guide\n\n");
    out.push_str("Use `response_schema_guide` from the request as the authoritative field guide. Empty lists/maps are valid when a pass has no changes, but required fields must remain present.\n\n");
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
