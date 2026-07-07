use anyhow::Context;
use clap::{Parser, Subcommand};
use msp_core::{
    ExecutionReport, MspInfo, PackSearchQuery, RiskLevel, RuntimeCompatibilityQuery,
    SkillSearchQuery, SkillTrustPolicy,
};
use msp_publisher::{PublicationDeprecation, PublishOptions, publish_skiller_bundle};
use msp_registry::{LocalRegistry, hash_file};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "msp")]
#[command(about = "MSP v0.1 reference CLI", long_about = None)]
struct Cli {
    /// Registry root containing skill.manifest.json and pack.manifest.json files.
    #[arg(long, global = true, default_value = "examples/registry")]
    registry: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Show MSP server/reference implementation metadata.
    Info,
    /// Registry commands.
    Registry {
        #[command(subcommand)]
        command: RegistryCommands,
    },
    /// Skill commands.
    Skills {
        #[command(subcommand)]
        command: SkillCommands,
    },
    /// Pack commands.
    Packs {
        #[command(subcommand)]
        command: PackCommands,
    },
    /// Trust commands.
    Trust {
        #[command(subcommand)]
        command: TrustCommands,
    },
    /// Producer-side publication commands.
    Publish {
        #[command(subcommand)]
        command: PublishCommands,
    },
    /// Utility commands.
    Hash {
        /// File to hash with sha256.
        file: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum RegistryCommands {
    /// Index the local registry and print counts.
    Index,
    /// Search/discover skills in the local registry.
    Search {
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        domain: Option<String>,
        #[arg(long)]
        language: Option<String>,
        #[arg(long = "tool")]
        available_tools: Vec<String>,
        #[arg(long = "max-risk")]
        max_risk: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum SkillCommands {
    /// Discover skills. Alias of registry search.
    Discover {
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        domain: Option<String>,
        #[arg(long)]
        language: Option<String>,
        #[arg(long = "tool")]
        available_tools: Vec<String>,
        #[arg(long = "max-risk")]
        max_risk: Option<String>,
    },
    /// Print one skill manifest as JSON.
    Manifest { id: String },
    /// Load one skill and verify its body hash.
    Load { id: String },
    /// Resolve dependencies for one skill.
    ResolveDependencies { id: String },
    /// Verify an execution report against its skill verification contract.
    VerifyResult { report: PathBuf },
    /// Check whether a skill is compatible with declared runtime capabilities.
    CheckCompatibility {
        id: String,
        #[arg(long)]
        msp_version: Option<String>,
        #[arg(long = "manifest-version")]
        supported_manifest_versions: Vec<String>,
        #[arg(long)]
        runtime_name: Option<String>,
        #[arg(long)]
        runtime_version: Option<String>,
        #[arg(long = "format")]
        supported_formats: Vec<String>,
        #[arg(long = "runtime-capability")]
        runtime_capabilities: Vec<String>,
        #[arg(long = "model-capability")]
        model_capabilities: Vec<String>,
        #[arg(long = "tool")]
        available_tools: Vec<String>,
        #[arg(long = "tool-version", value_parser = parse_key_value)]
        tool_versions: Vec<(String, String)>,
        #[arg(long = "permission")]
        permissions: Vec<String>,
        #[arg(long)]
        context_window: Option<u64>,
        #[arg(long)]
        platform: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum PackCommands {
    /// Discover skill packs in the local registry.
    Discover {
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        issuer: Option<String>,
        #[arg(long = "max-risk")]
        max_risk: Option<String>,
    },
    /// Print one pack manifest as JSON.
    Manifest { id: String },
    /// Load one pack manifest as JSON.
    Load { id: String },
    /// Verify one pack manifest trust hash/signature.
    VerifyTrust { id: String },
    /// Evaluate one pack against a trust policy JSON file.
    EvaluateTrust {
        id: String,
        #[arg(
            long,
            default_value = "examples/policies/local-reference.trust-policy.json"
        )]
        policy: PathBuf,
    },
    /// Validate pack member manifest URIs, ids, versions, index entries, and optionally trust.
    ValidateMembers {
        id: String,
        #[arg(long)]
        policy: Option<PathBuf>,
    },
    /// Evaluate a pack and its dependency graph against a trust policy JSON file.
    EvaluateDependencies {
        id: String,
        #[arg(
            long,
            default_value = "examples/policies/local-reference.trust-policy.json"
        )]
        policy: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum TrustCommands {
    /// Verify the manifest trust hash for one skill.
    Verify { id: String },
    /// Verify the body artifact hash for one skill.
    VerifyBody { id: String },
    /// Evaluate one skill against a trust policy JSON file.
    Evaluate {
        id: String,
        #[arg(
            long,
            default_value = "examples/policies/local-reference.trust-policy.json"
        )]
        policy: PathBuf,
    },
    /// Evaluate a skill and its dependency graph against a trust policy JSON file.
    EvaluateDependencies {
        id: String,
        #[arg(
            long,
            default_value = "examples/policies/local-reference.trust-policy.json"
        )]
        policy: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum PublishCommands {
    /// Import a Skiller bundle and publish canonical MSP skill/pack artifacts.
    ImportSkiller {
        /// Path to a Skiller bundle directory containing package.yaml and skills/*.yaml.
        bundle: PathBuf,
        /// Issuer recorded in MSP trust metadata.
        #[arg(long)]
        issuer: String,
        /// Regenerate artifacts for existing ids if the generated bytes are identical.
        #[arg(long)]
        force: bool,
        /// Explicitly permit byte-changing replacement of an existing same-id/version publication.
        #[arg(long = "allow-mutable-version", requires = "force")]
        allow_mutable_version: bool,
        /// Path to a local Ed25519 signing seed file. Accepts raw 32-byte seed or hex text.
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        /// Mark generated skill and pack manifests as deprecated.
        #[arg(long)]
        deprecated: bool,
        /// Deprecation reason to embed when --deprecated is used.
        #[arg(long = "deprecation-reason", requires = "deprecated")]
        deprecation_reason: Option<String>,
        /// Replacement skill id to embed in generated skill manifests when --deprecated is used.
        #[arg(long = "replacement-skill", requires = "deprecated")]
        replacement_skill: Option<String>,
        /// Replacement pack id to embed in the generated pack manifest when --deprecated is used.
        #[arg(long = "replacement-pack", requires = "deprecated")]
        replacement_pack: Option<String>,
        /// Deprecation sunset timestamp as RFC3339 date-time.
        #[arg(long = "sunset-at", requires = "deprecated")]
        sunset_at: Option<String>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Info => print_json(&MspInfo::default()),
        Commands::Hash { file } => {
            let digest =
                hash_file(&file).with_context(|| format!("failed to hash {}", file.display()))?;
            println!("{digest}");
            Ok(())
        }
        Commands::Registry { command } => {
            let registry = LocalRegistry::open(&cli.registry)
                .with_context(|| format!("failed to open registry {}", cli.registry.display()))?;
            match command {
                RegistryCommands::Index => print_json(&serde_json::json!({
                    "root": registry.root(),
                    "skills": registry.skill_count(),
                    "packs": registry.pack_count()
                })),
                RegistryCommands::Search {
                    task,
                    category,
                    domain,
                    language,
                    available_tools,
                    max_risk,
                } => search(
                    registry,
                    task,
                    category,
                    domain,
                    language,
                    available_tools,
                    max_risk,
                ),
            }
        }
        Commands::Skills { command } => {
            let registry = LocalRegistry::open(&cli.registry)
                .with_context(|| format!("failed to open registry {}", cli.registry.display()))?;
            match command {
                SkillCommands::Discover {
                    task,
                    category,
                    domain,
                    language,
                    available_tools,
                    max_risk,
                } => search(
                    registry,
                    task,
                    category,
                    domain,
                    language,
                    available_tools,
                    max_risk,
                ),
                SkillCommands::Manifest { id } => print_json(&registry.get_manifest(&id)?),
                SkillCommands::Load { id } => print_json(&registry.load_skill(&id)?),
                SkillCommands::ResolveDependencies { id } => {
                    print_json(&registry.resolve_dependencies(&id)?)
                }
                SkillCommands::VerifyResult { report } => {
                    let report = ExecutionReport::from_path(&report).with_context(|| {
                        format!("failed to load execution report {}", report.display())
                    })?;
                    print_json(&registry.verify_execution_report(&report)?)
                }
                SkillCommands::CheckCompatibility {
                    id,
                    msp_version,
                    supported_manifest_versions,
                    runtime_name,
                    runtime_version,
                    supported_formats,
                    runtime_capabilities,
                    model_capabilities,
                    available_tools,
                    tool_versions,
                    permissions,
                    context_window,
                    platform,
                } => {
                    let query = RuntimeCompatibilityQuery {
                        msp_version,
                        supported_manifest_versions,
                        runtime_name,
                        runtime_version,
                        supported_formats,
                        runtime_capabilities,
                        model_capabilities,
                        available_tools,
                        tool_versions: BTreeMap::from_iter(tool_versions),
                        permissions,
                        context_window,
                        platform,
                    };
                    print_json(&registry.check_skill_compatibility(&id, &query)?)
                }
            }
        }
        Commands::Packs { command } => {
            let registry = LocalRegistry::open(&cli.registry)
                .with_context(|| format!("failed to open registry {}", cli.registry.display()))?;
            match command {
                PackCommands::Discover {
                    task,
                    category,
                    issuer,
                    max_risk,
                } => {
                    let query = PackSearchQuery {
                        task,
                        category,
                        issuer,
                        max_risk: max_risk.as_deref().map(parse_risk_level).transpose()?,
                    };
                    print_json(&registry.discover_packs(&query))
                }
                PackCommands::Manifest { id } | PackCommands::Load { id } => {
                    print_json(&registry.load_pack(&id)?)
                }
                PackCommands::VerifyTrust { id } => print_json(&registry.verify_pack_trust(&id)?),
                PackCommands::EvaluateTrust { id, policy } => {
                    let policy = SkillTrustPolicy::from_path(&policy)
                        .with_context(|| format!("failed to load policy {}", policy.display()))?;
                    print_json(&registry.evaluate_pack_trust_policy(&policy, &id)?)
                }
                PackCommands::ValidateMembers { id, policy } => {
                    let policy = policy
                        .as_ref()
                        .map(|path| {
                            SkillTrustPolicy::from_path(path).with_context(|| {
                                format!("failed to load policy {}", path.display())
                            })
                        })
                        .transpose()?;
                    print_json(&registry.validate_pack_members_with_policy(&id, policy.as_ref())?)
                }
                PackCommands::EvaluateDependencies { id, policy } => {
                    let policy = SkillTrustPolicy::from_path(&policy)
                        .with_context(|| format!("failed to load policy {}", policy.display()))?;
                    print_json(&registry.evaluate_pack_dependency_trust(&policy, &id)?)
                }
            }
        }
        Commands::Publish { command } => match command {
            PublishCommands::ImportSkiller {
                bundle,
                issuer,
                force,
                allow_mutable_version,
                signing_key,
                deprecated,
                deprecation_reason,
                replacement_skill,
                replacement_pack,
                sunset_at,
            } => {
                let report = publish_skiller_bundle(
                    &bundle,
                    PublishOptions {
                        registry: cli.registry,
                        issuer,
                        force,
                        allow_mutable_version,
                        signing_key,
                        deprecation: PublicationDeprecation {
                            deprecated,
                            reason: deprecation_reason,
                            skill_replacement: replacement_skill,
                            pack_replacement: replacement_pack,
                            sunset_at,
                        },
                    },
                )
                .with_context(|| format!("failed to import Skiller bundle {}", bundle.display()))?;
                print_json(&report)
            }
        },
        Commands::Trust { command } => {
            let registry = LocalRegistry::open(&cli.registry)
                .with_context(|| format!("failed to open registry {}", cli.registry.display()))?;
            match command {
                TrustCommands::Verify { id } => print_json(&registry.verify_trust(&id)?),
                TrustCommands::VerifyBody { id } => {
                    print_json(&registry.verify_body_artifact(&id)?)
                }
                TrustCommands::Evaluate { id, policy } => {
                    let policy = SkillTrustPolicy::from_path(&policy)
                        .with_context(|| format!("failed to load policy {}", policy.display()))?;
                    print_json(&registry.evaluate_trust_policy(&policy, &id)?)
                }
                TrustCommands::EvaluateDependencies { id, policy } => {
                    let policy = SkillTrustPolicy::from_path(&policy)
                        .with_context(|| format!("failed to load policy {}", policy.display()))?;
                    print_json(&registry.evaluate_dependency_trust(&policy, &id)?)
                }
            }
        }
    }
}

fn search(
    registry: LocalRegistry,
    task: Option<String>,
    category: Option<String>,
    domain: Option<String>,
    language: Option<String>,
    available_tools: Vec<String>,
    max_risk: Option<String>,
) -> anyhow::Result<()> {
    let query = SkillSearchQuery {
        task,
        category,
        domain,
        language,
        available_tools,
        max_risk: max_risk.as_deref().map(parse_risk_level).transpose()?,
    };
    print_json(&registry.search(&query))
}

fn parse_risk_level(value: &str) -> anyhow::Result<RiskLevel> {
    match value.to_ascii_lowercase().as_str() {
        "low" => Ok(RiskLevel::Low),
        "medium" => Ok(RiskLevel::Medium),
        "high" => Ok(RiskLevel::High),
        "critical" => Ok(RiskLevel::Critical),
        _ => anyhow::bail!("expected one of: low, medium, high, critical"),
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn parse_key_value(value: &str) -> Result<(String, String), String> {
    let (key, value) = value
        .split_once('=')
        .ok_or_else(|| "expected KEY=VALUE".to_string())?;
    if key.trim().is_empty() || value.trim().is_empty() {
        return Err("expected non-empty KEY=VALUE".to_string());
    }
    Ok((key.to_string(), value.to_string()))
}
