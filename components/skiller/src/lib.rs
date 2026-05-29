pub mod agents;
pub mod compiler;
pub mod corpus;
pub mod domain;
pub mod evidence;
pub mod forge;
pub mod ingest;
pub mod models;
pub mod registry;
pub mod review;
pub mod runtime;
pub mod security;
pub mod source_meta;
pub mod telemetry;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "skiller")]
#[command(about = "Compile technical sources into governed, agent-ready skills", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Compile local source files/directories into a deterministic skill bundle.
    Compile {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "skiller-bundle")]
        name: String,
        #[arg(long)]
        domain: Option<String>,
    },
    /// Compile repository docs, examples, tests, configs, and code comments into a skill bundle.
    CompileRepo {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "skiller-repo-bundle")]
        name: String,
        #[arg(long)]
        domain: Option<String>,
    },
    /// Compile a public URL or small same-host docs crawl into a skill bundle.
    CompileUrl {
        url: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "skiller-url-bundle")]
        name: String,
        #[arg(long)]
        domain: Option<String>,
        #[arg(long, default_value_t = 1)]
        max_pages: usize,
    },
    /// Compile an OpenAPI/Swagger specification into API operation skills.
    CompileOpenapi {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "skiller-openapi-bundle")]
        name: String,
        #[arg(long)]
        domain: Option<String>,
    },
    /// Compile a lightweight API specification into API operation skills.
    CompileApi {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "skiller-api-bundle")]
        name: String,
        #[arg(long)]
        domain: Option<String>,
    },
    /// Compile a lightweight CLI specification into CLI operation skills.
    CompileCli {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "skiller-cli-bundle")]
        name: String,
        #[arg(long)]
        domain: Option<String>,
    },
    /// Compile captured CLI help/manpage text into CLI operation skills.
    CompileCliHelp {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "skiller-cli-help-bundle")]
        name: String,
        #[arg(long)]
        domain: Option<String>,
    },
    /// Validate a skill bundle.
    Validate { bundle: PathBuf },
    /// List skills in a bundle.
    List { bundle: PathBuf },
    /// Route a task to matching skills.
    Route {
        bundle: PathBuf,
        query: String,
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// Materialize a skill card/body/extended context.
    Load {
        bundle: PathBuf,
        skill_id: String,
        #[arg(long, value_enum, default_value_t = LoadModeArg::Body)]
        mode: LoadModeArg,
    },
    /// Run deterministic structural evals for a bundle.
    Eval { bundle: PathBuf },
    /// Show available Forge providers and current integration status.
    ForgeProviderStatus {
        #[arg(long)]
        provider: Option<String>,
    },
    /// Preflight-check the configured Vegvisir Forge adapter command.
    ForgeAdapterPreflight,
    /// Send a synthetic strict-envelope request to the configured Vegvisir Forge adapter.
    ForgeAdapterSelfTest,
    /// Run a local Forge pass over a deterministic bundle.
    Forge {
        bundle: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "mock")]
        provider: String,
        #[arg(long)]
        domain_profile: Option<String>,
        #[arg(long, default_value_t = 100)]
        max_skills: usize,
    },
    /// Export a strict Forge request envelope for Vegvisir to process.
    ForgeRequest {
        bundle: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, value_enum, default_value_t = ForgePassArg::SkillExpansion)]
        pass: ForgePassArg,
        #[arg(long)]
        domain_profile: Option<String>,
        #[arg(long, default_value_t = 100)]
        max_skills: usize,
    },
    /// Export a complete Vegvisir handoff directory with request, response template, and prompt.
    ForgeHandoff {
        bundle: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, value_enum, default_value_t = ForgePassArg::SkillExpansion)]
        pass: ForgePassArg,
        #[arg(long)]
        domain_profile: Option<String>,
        #[arg(long, default_value_t = 100)]
        max_skills: usize,
    },
    /// Validate a Vegvisir Forge response envelope without applying it.
    ForgeValidate {
        bundle: PathBuf,
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        response: PathBuf,
        /// Write a machine-readable validation report YAML before returning.
        #[arg(long)]
        report: Option<PathBuf>,
    },
    /// Validate and apply a Vegvisir Forge response envelope.
    ForgeApply {
        bundle: PathBuf,
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        response: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Write a machine-readable apply report YAML after successful application.
        #[arg(long)]
        report: Option<PathBuf>,
    },
    /// Generate inferred skills from a bundle.
    Infer {
        bundle: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Create a critique report for a bundle.
    Critique {
        bundle: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Generate an evidence report.
    EvidenceReport { bundle: PathBuf },
    /// Run deterministic verifier-agent style review and write report artifacts.
    ReviewAgent {
        bundle: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "verifier")]
        agent: String,
    },
    /// Apply verifier review decisions to a staged bundle.
    ApplyReview {
        bundle: PathBuf,
        #[arg(long)]
        review: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Show built-in domain profiles.
    DomainProfiles,
    /// Propose specialist agents from a bundle.
    ProposeAgents {
        bundle: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Build an Agent Builder handoff package.
    BuildAgentPack {
        bundle: PathBuf,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        lifecycle_status: Option<PathBuf>,
        #[arg(long)]
        report: Option<PathBuf>,
    },
    /// Verify generated agent proposal index and proposal files.
    VerifyAgentProposals { path: PathBuf },
    /// Verify an Agent Builder handoff package manifest.
    VerifyAgentPack { path: PathBuf },
    /// Write a consolidated Agent Builder summary for proposals and packs.
    AgentBuilderSummary {
        #[arg(long)]
        proposals: Option<PathBuf>,
        #[arg(long = "pack")]
        packs: Vec<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Scan Agent Builder artifacts and write a consolidated artifact index.
    AgentArtifactIndex {
        root: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Publish a bundle to a filesystem registry.
    Publish {
        bundle: PathBuf,
        #[arg(long)]
        registry: PathBuf,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// List published bundles in a filesystem registry and refresh index.yaml.
    RegistryList { registry: PathBuf },
    /// Verify a bundle or published registry entry MANIFEST.sha256.
    VerifyManifest { path: PathBuf },
    /// Mark a published registry entry as deprecated.
    RegistryDeprecate {
        registry: PathBuf,
        bundle_id: String,
        version: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        replacement_version: Option<String>,
    },
    /// Mark a previous registry version as the active rollback target.
    RegistryRollback {
        registry: PathBuf,
        bundle_id: String,
        to_version: String,
        #[arg(long)]
        reason: String,
    },
    /// Assess registry publication readiness.
    Readiness { bundle: PathBuf },
    /// Generate improvement proposals from telemetry.
    ImproveFromTelemetry {
        bundle: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Build a corpus map report from an existing bundle.
    CorpusMap {
        bundle: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Build a deterministic corpus manifest/source inventory for change detection.
    CorpusManifest {
        bundle: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Compare two corpus manifests and write a reviewable change report.
    CorpusDiff {
        old_manifest: PathBuf,
        new_manifest: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Build a lifecycle/review plan from a corpus diff report.
    CorpusPlan {
        diff_report: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },

    /// Evaluate a bundle against a corpus lifecycle plan plus validation/readiness.
    CorpusStatus {
        bundle: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Print a domain profile template as YAML.
    DomainTemplate { name: String },
    /// Stage a bundle version bump and reset review status.
    BumpVersion {
        bundle: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        version: Option<String>,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum LoadModeArg {
    Card,
    Body,
    Extended,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum ForgePassArg {
    Interpretation,
    SkillExpansion,
    SkillInference,
    DeduplicationAndScope,
    SafetyAndGovernance,
    EvalGeneration,
    AgentRoleMapping,
    RegistryReadiness,
    Critique,
    VerifierReview,
}

impl From<ForgePassArg> for models::ForgePassType {
    fn from(value: ForgePassArg) -> Self {
        match value {
            ForgePassArg::Interpretation => models::ForgePassType::Interpretation,
            ForgePassArg::SkillExpansion => models::ForgePassType::SkillExpansion,
            ForgePassArg::SkillInference => models::ForgePassType::SkillInference,
            ForgePassArg::DeduplicationAndScope => models::ForgePassType::DeduplicationAndScope,
            ForgePassArg::SafetyAndGovernance => models::ForgePassType::SafetyAndGovernance,
            ForgePassArg::EvalGeneration => models::ForgePassType::EvalGeneration,
            ForgePassArg::AgentRoleMapping => models::ForgePassType::AgentRoleMapping,
            ForgePassArg::RegistryReadiness => models::ForgePassType::RegistryReadiness,
            ForgePassArg::Critique => models::ForgePassType::Critique,
            ForgePassArg::VerifierReview => models::ForgePassType::VerifierReview,
        }
    }
}

impl From<LoadModeArg> for runtime::LoadMode {
    fn from(value: LoadModeArg) -> Self {
        match value {
            LoadModeArg::Card => runtime::LoadMode::Card,
            LoadModeArg::Body => runtime::LoadMode::Body,
            LoadModeArg::Extended => runtime::LoadMode::Extended,
        }
    }
}

fn write_bundle(bundle: models::SkillBundle, out: PathBuf, label: &str) -> Result<()> {
    registry::write_bundle(&bundle, &out)?;
    println!(
        "wrote {label} bundle {} to {}",
        bundle.package.bundle_id,
        out.display()
    );
    Ok(())
}

pub fn run_cli() -> Result<()> {
    run_cli_from(std::env::args_os())
}

pub fn run_cli_from<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    match cli.command {
        Commands::Compile {
            input,
            out,
            name,
            domain,
        } => write_bundle(
            compiler::compile_path(&input, &name, domain.as_deref())?,
            out,
            "deterministic",
        )?,
        Commands::CompileRepo {
            input,
            out,
            name,
            domain,
        } => write_bundle(
            compiler::compile_repo(&input, &name, domain.as_deref())?,
            out,
            "repository",
        )?,
        Commands::CompileUrl {
            url,
            out,
            name,
            domain,
            max_pages,
        } => write_bundle(
            compiler::compile_url(&url, &name, domain.as_deref(), max_pages)?,
            out,
            "URL",
        )?,
        Commands::CompileOpenapi {
            input,
            out,
            name,
            domain,
        } => write_bundle(
            compiler::compile_openapi(&input, &name, domain.as_deref())?,
            out,
            "OpenAPI",
        )?,
        Commands::CompileApi {
            input,
            out,
            name,
            domain,
        } => write_bundle(
            compiler::compile_api(&input, &name, domain.as_deref())?,
            out,
            "API",
        )?,
        Commands::CompileCli {
            input,
            out,
            name,
            domain,
        } => write_bundle(
            compiler::compile_cli(&input, &name, domain.as_deref())?,
            out,
            "CLI",
        )?,
        Commands::CompileCliHelp {
            input,
            out,
            name,
            domain,
        } => write_bundle(
            compiler::compile_cli_help(&input, &name, domain.as_deref())?,
            out,
            "CLI help",
        )?,
        Commands::Validate { bundle } => {
            let report = registry::validate_bundle_path(&bundle)?;
            println!("{}", serde_yaml::to_string(&report)?);
            if !report.valid {
                std::process::exit(1);
            }
        }
        Commands::List { bundle } => {
            let bundle = registry::read_bundle(&bundle)?;
            for skill in bundle.skills {
                println!("{}\t{}\t{:?}", skill.id, skill.title, skill.status);
            }
        }
        Commands::Route {
            bundle,
            query,
            limit,
        } => {
            let bundle = registry::read_bundle(&bundle)?;
            for hit in runtime::route(&bundle, &query, limit) {
                println!("{:.3}\t{}\t{}", hit.score, hit.skill_id, hit.title);
            }
        }
        Commands::Load {
            bundle,
            skill_id,
            mode,
        } => {
            let bundle = registry::read_bundle(&bundle)?;
            println!("{}", runtime::load_skill(&bundle, &skill_id, mode.into())?);
        }
        Commands::Eval { bundle } => {
            let bundle = registry::read_bundle(&bundle)?;
            let report = registry::eval_bundle(&bundle);
            println!("{}", serde_yaml::to_string(&report)?);
            if !report.passed {
                std::process::exit(1);
            }
        }
        Commands::ForgeProviderStatus { provider } => {
            if let Some(provider) = provider {
                let status = forge::provider_status(&provider)?;
                println!("{}", serde_yaml::to_string(&status)?);
            } else {
                let catalog = forge::provider_catalog();
                println!("{}", serde_yaml::to_string(&catalog)?);
            }
        }
        Commands::ForgeAdapterPreflight => {
            let report = forge::vegvisir_adapter_preflight_report();
            println!("{}", serde_yaml::to_string(&report)?);
            if !report.valid {
                std::process::exit(1);
            }
        }
        Commands::ForgeAdapterSelfTest => {
            let report = forge::vegvisir_adapter_self_test_report();
            println!("{}", serde_yaml::to_string(&report)?);
            if !report.valid {
                std::process::exit(1);
            }
        }
        Commands::Forge {
            bundle,
            out,
            provider,
            domain_profile,
            max_skills,
        } => {
            let bundle = registry::read_bundle(&bundle)?;
            let forged =
                forge::forge_bundle(bundle, &provider, domain_profile.as_deref(), max_skills)?;
            registry::write_bundle(&forged, &out)?;
            println!("wrote forged bundle to {}", out.display());
        }
        Commands::ForgeRequest {
            bundle,
            out,
            pass,
            domain_profile,
            max_skills,
        } => {
            let bundle = registry::read_bundle(&bundle)?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let request = forge::build_vegvisir_handoff(
                &bundle,
                pass.into(),
                domain_profile.as_deref(),
                max_skills,
            );
            std::fs::write(&out, serde_yaml::to_string(&request)?)?;
            println!("wrote Vegvisir Forge request to {}", out.display());
        }
        Commands::ForgeHandoff {
            bundle,
            out,
            pass,
            domain_profile,
            max_skills,
        } => {
            let bundle = registry::read_bundle(&bundle)?;
            std::fs::create_dir_all(&out)?;
            let request = forge::build_vegvisir_handoff(
                &bundle,
                pass.into(),
                domain_profile.as_deref(),
                max_skills,
            );
            let response_template = forge::response_template_for(&request);
            std::fs::write(
                out.join("forge-request.yaml"),
                serde_yaml::to_string(&request)?,
            )?;
            std::fs::write(
                out.join("forge-response-template.yaml"),
                serde_yaml::to_string(&response_template)?,
            )?;
            std::fs::write(
                out.join("vegvisir-prompt.md"),
                forge::vegvisir_prompt_markdown(&request),
            )?;
            println!("wrote Vegvisir handoff directory to {}", out.display());
        }
        Commands::ForgeValidate {
            bundle,
            request,
            response,
            report,
        } => {
            let bundle = registry::read_bundle(&bundle)?;
            let request: models::ForgeRequestEnvelope =
                serde_yaml::from_str(&std::fs::read_to_string(&request)?)?;
            let response: models::ForgeResponseEnvelope =
                serde_yaml::from_str(&std::fs::read_to_string(&response)?)?;
            let validation_report = forge::validate_response_report(&bundle, &request, &response);
            if let Some(report_path) = report {
                if let Some(parent) = report_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&report_path, serde_yaml::to_string(&validation_report)?)?;
            }
            if validation_report.valid {
                println!("Forge response is valid for request {}", request.request_id);
            } else {
                anyhow::bail!(
                    "Forge response is invalid for request {}: {}",
                    request.request_id,
                    validation_report.errors.join("; ")
                );
            }
        }
        Commands::ForgeApply {
            bundle,
            request,
            response,
            out,
            report,
        } => {
            let bundle = registry::read_bundle(&bundle)?;
            let request: models::ForgeRequestEnvelope =
                serde_yaml::from_str(&std::fs::read_to_string(&request)?)?;
            let response: models::ForgeResponseEnvelope =
                serde_yaml::from_str(&std::fs::read_to_string(&response)?)?;
            let (forged, apply_report) =
                forge::apply_external_response_with_report(bundle, request, response)?;
            registry::write_bundle(&forged, &out)?;
            if let Some(report_path) = report {
                if let Some(parent) = report_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&report_path, serde_yaml::to_string(&apply_report)?)?;
            }
            println!("validated and applied Forge response to {}", out.display());
        }
        Commands::Infer { bundle, out } => {
            let bundle = registry::read_bundle(&bundle)?;
            let inferred = forge::infer_bundle(bundle)?;
            registry::write_bundle(&inferred, &out)?;
            println!("wrote inferred bundle to {}", out.display());
        }
        Commands::Critique { bundle, out } => {
            let bundle = registry::read_bundle(&bundle)?;
            let report = forge::critique_markdown(&bundle);
            std::fs::create_dir_all(out.parent().unwrap_or_else(|| std::path::Path::new(".")))?;
            std::fs::write(&out, report)?;
            println!("wrote critique report to {}", out.display());
        }
        Commands::EvidenceReport { bundle } => {
            let bundle = registry::read_bundle(&bundle)?;
            println!("{}", evidence::evidence_report_markdown(&bundle));
        }
        Commands::ReviewAgent { bundle, out, agent } => {
            let bundle = registry::read_bundle(&bundle)?;
            std::fs::create_dir_all(&out)?;
            let report = review::verifier_review(&bundle, &agent);
            std::fs::write(
                out.join("verifier-review.yaml"),
                serde_yaml::to_string(&report)?,
            )?;
            std::fs::write(
                out.join("verifier-review.md"),
                review::verifier_review_markdown(&report),
            )?;
            println!("wrote verifier review to {}", out.display());
        }
        Commands::ApplyReview {
            bundle,
            review,
            out,
        } => {
            let bundle = registry::read_bundle(&bundle)?;
            let report: models::VerifierReviewReport =
                serde_yaml::from_str(&std::fs::read_to_string(&review)?)?;
            let reviewed = review::apply_verifier_review(bundle, &report);
            registry::write_bundle(&reviewed, &out)?;
            println!("applied verifier review to {}", out.display());
        }
        Commands::DomainProfiles => {
            println!("{}", serde_yaml::to_string(&domain::builtin_profiles())?)
        }
        Commands::ProposeAgents { bundle, out } => {
            let bundle = registry::read_bundle(&bundle)?;
            agents::write_agent_proposals(&bundle, &out)?;
            println!("wrote agent proposals to {}", out.display());
        }
        Commands::BuildAgentPack {
            bundle,
            agent,
            out,
            lifecycle_status,
            report,
        } => {
            let bundle = registry::read_bundle(&bundle)?;
            let build_report = agents::write_agent_pack_with_report(
                &bundle,
                &agent,
                &out,
                lifecycle_status.as_deref(),
            )?;
            if let Some(report) = report {
                if let Some(parent) = report.parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent)?;
                    }
                }
                std::fs::write(&report, serde_yaml::to_string(&build_report)?)?;
            }
            println!("wrote agent pack to {}", out.display());
        }
        Commands::VerifyAgentProposals { path } => {
            let report = agents::verify_agent_proposals(&path)?;
            println!("{}", serde_yaml::to_string(&report)?);
            if !report.valid {
                std::process::exit(1);
            }
        }
        Commands::VerifyAgentPack { path } => {
            let report = agents::verify_agent_pack(&path)?;
            println!("{}", serde_yaml::to_string(&report)?);
            if !report.valid {
                std::process::exit(1);
            }
        }
        Commands::AgentBuilderSummary {
            proposals,
            packs,
            out,
        } => {
            let summary = agents::write_agent_builder_summary(proposals.as_deref(), &packs, &out)?;
            println!("{}", serde_yaml::to_string(&summary)?);
            if !summary.valid {
                std::process::exit(1);
            }
        }
        Commands::AgentArtifactIndex { root, out } => {
            let index = agents::write_agent_artifact_index(&root, &out)?;
            println!("{}", serde_yaml::to_string(&index)?);
            if !index.valid {
                std::process::exit(1);
            }
        }
        Commands::Publish {
            bundle,
            registry: reg,
            force,
        } => {
            let bundle = registry::read_bundle(&bundle)?;
            registry::publish_bundle(&bundle, &reg, force)?;
            println!(
                "published {} to {}",
                bundle.package.bundle_id,
                reg.display()
            );
        }
        Commands::RegistryList { registry: reg } => {
            let index = registry::write_registry_index(&reg)?;
            println!("{}", serde_yaml::to_string(&index)?);
        }
        Commands::VerifyManifest { path } => {
            let report = registry::verify_manifest(&path)?;
            println!("{}", serde_yaml::to_string(&report)?);
            if !report.valid {
                std::process::exit(1);
            }
        }
        Commands::RegistryDeprecate {
            registry: reg,
            bundle_id,
            version,
            reason,
            replacement_version,
        } => {
            let record = registry::deprecate_registry_entry(
                &reg,
                &bundle_id,
                &version,
                &reason,
                replacement_version.as_deref(),
            )?;
            println!("{}", serde_yaml::to_string(&record)?);
        }
        Commands::RegistryRollback {
            registry: reg,
            bundle_id,
            to_version,
            reason,
        } => {
            let record = registry::rollback_registry_entry(&reg, &bundle_id, &to_version, &reason)?;
            println!("{}", serde_yaml::to_string(&record)?);
        }
        Commands::Readiness { bundle } => {
            let bundle = registry::read_bundle(&bundle)?;
            println!(
                "{}",
                serde_yaml::to_string(&registry::readiness_report(&bundle))?
            );
        }
        Commands::ImproveFromTelemetry { bundle, out } => {
            let bundle = registry::read_bundle(&bundle)?;
            telemetry::write_improvement_proposals(&bundle, &out)?;
            println!("wrote improvement proposals to {}", out.display());
        }
        Commands::CorpusMap { bundle, out } => {
            let bundle = registry::read_bundle(&bundle)?;
            corpus::write_corpus_map(&bundle, &out)?;
            println!("wrote corpus map to {}", out.display());
        }
        Commands::CorpusManifest { bundle, out } => {
            let bundle = registry::read_bundle(&bundle)?;
            corpus::write_corpus_manifest(&bundle, &out)?;
            println!("wrote corpus manifest to {}", out.display());
        }
        Commands::CorpusDiff {
            old_manifest,
            new_manifest,
            out,
        } => {
            corpus::write_corpus_diff(&old_manifest, &new_manifest, &out)?;
            println!("wrote corpus diff to {}", out.display());
        }
        Commands::CorpusPlan { diff_report, out } => {
            corpus::write_corpus_plan(&diff_report, &out)?;
            println!("wrote corpus lifecycle plan to {}", out.display());
        }
        Commands::CorpusStatus { bundle, plan, out } => {
            let bundle = registry::read_bundle(&bundle)?;
            corpus::write_corpus_status(&bundle, &plan, &out)?;
            println!("wrote corpus lifecycle status to {}", out.display());
        }
        Commands::DomainTemplate { name } => {
            println!(
                "{}",
                serde_yaml::to_string(&corpus::domain_template(&name))?
            );
        }
        Commands::BumpVersion {
            bundle,
            out,
            version,
        } => {
            let bundle = registry::read_bundle(&bundle)?;
            let bumped = corpus::bump_bundle_version(bundle, version.as_deref());
            registry::write_bundle(&bumped, &out)?;
            println!("wrote version-staged bundle to {}", out.display());
        }
    }
    Ok(())
}
