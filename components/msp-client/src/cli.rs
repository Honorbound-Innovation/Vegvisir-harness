use std::path::PathBuf;

use crate::msp_core::{RuntimeCompatibilityQuery, SkillTrustPolicy};
use crate::msp_publisher::PublicationDeprecation;
use crate::{
    CompatibilityRequest, ImportSkillerBundleRequest, LoadMode, MspClient, PackSearchRequest,
    SearchRequest, TrustEvaluationRequest, parse_risk_level,
};
use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "msp-client")]
#[command(about = "Native Vegvisir MSP client component", long_about = None)]
struct Cli {
    /// MSP local registry root. Defaults to Vegvisir's user-global MSP registry.
    #[arg(long, global = true)]
    registry: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Print component and MSP protocol information.
    Info,
    /// Print local registry counts.
    Summary,
    /// Search local MSP skills.
    Search(SearchArgs),
    /// Import a Skiller bundle into the local MSP registry.
    ImportSkiller(ImportSkillerArgs),
    /// Load skill context from a local MSP registry.
    Load {
        id: String,
        #[arg(long, default_value = "body")]
        mode: LoadModeArg,
    },
    /// Print a skill manifest as JSON.
    Manifest { id: String },
    /// Verify a skill body hash/signature trust envelope.
    VerifyTrust { id: String },
    /// Evaluate a skill against a trust policy JSON file.
    EvaluateTrust {
        id: String,
        #[arg(long)]
        policy: PathBuf,
        #[arg(long)]
        dependencies: bool,
    },
    /// Check skill compatibility with a Vegvisir-like runtime query.
    CheckCompatibility(CompatibilityArgs),
    /// Resolve skill dependencies.
    ResolveDependencies { id: String },
    /// Discover MSP skill packs.
    DiscoverPacks(PackSearchArgs),
    /// Print a pack manifest as JSON.
    PackManifest { id: String },
}

#[derive(Args, Debug)]
struct ImportSkillerArgs {
    /// Skiller bundle directory containing package.yaml and skills/*.yaml.
    bundle: PathBuf,
    /// Trust issuer to stamp on generated MSP artifacts.
    #[arg(long)]
    issuer: String,
    /// Regenerate an identical existing publication.
    #[arg(long)]
    force: bool,
    /// Explicit local override allowing same-version publication bytes to change.
    #[arg(long)]
    allow_mutable_version: bool,
    /// Optional Ed25519 signing seed file. Keep real credentials behind HBSE-managed paths.
    #[arg(long)]
    signing_key: Option<PathBuf>,
    /// Mark generated skill/pack artifacts deprecated.
    #[arg(long)]
    deprecated: bool,
    #[arg(long)]
    deprecation_reason: Option<String>,
    #[arg(long)]
    replacement_skill: Option<String>,
    #[arg(long)]
    replacement_pack: Option<String>,
    #[arg(long)]
    sunset_at: Option<String>,
}

#[derive(Args, Debug)]
struct SearchArgs {
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
    #[arg(long)]
    max_risk: Option<String>,
    #[arg(long)]
    limit: Option<usize>,
}

#[derive(Args, Debug)]
struct PackSearchArgs {
    #[arg(long)]
    task: Option<String>,
    #[arg(long)]
    category: Option<String>,
    #[arg(long)]
    issuer: Option<String>,
    #[arg(long)]
    max_risk: Option<String>,
    #[arg(long)]
    limit: Option<usize>,
}

#[derive(Args, Debug)]
struct CompatibilityArgs {
    id: String,
    #[arg(long, default_value = "0.1.0")]
    msp_version: String,
    #[arg(long = "manifest-version", default_value = "0.1.0")]
    supported_manifest_versions: Vec<String>,
    #[arg(long, default_value = "vegvisir")]
    runtime_name: String,
    #[arg(long = "format", default_value = "markdown")]
    supported_formats: Vec<String>,
    #[arg(long = "runtime-capability")]
    runtime_capabilities: Vec<String>,
    #[arg(long = "model-capability")]
    model_capabilities: Vec<String>,
    #[arg(long = "tool")]
    available_tools: Vec<String>,
    #[arg(long = "permission")]
    permissions: Vec<String>,
    #[arg(long)]
    context_window: Option<u64>,
    #[arg(long)]
    platform: Option<String>,
}

#[derive(Clone, Debug)]
struct LoadModeArg(LoadMode);

impl std::str::FromStr for LoadModeArg {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(value.parse()?))
    }
}

pub fn run_cli() -> anyhow::Result<()> {
    run_cli_from(std::env::args_os())
}

pub fn run_cli_from<I, T>(args: I) -> anyhow::Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    let registry = cli.registry.unwrap_or_else(crate::default_registry_root);
    let client = MspClient::open(&registry)?;

    match cli.command {
        Commands::Info => print_json(&client.info()),
        Commands::Summary => print_json(&client.summary()),
        Commands::Search(args) => {
            let request = SearchRequest {
                task: args.task,
                category: args.category,
                domain: args.domain,
                language: args.language,
                available_tools: args.available_tools,
                max_risk: parse_optional_risk(args.max_risk)?,
                limit: args.limit,
            };
            print_json(&client.search(request))
        }
        Commands::ImportSkiller(args) => {
            let request = ImportSkillerBundleRequest {
                bundle: args.bundle,
                issuer: args.issuer,
                force: args.force,
                allow_mutable_version: args.allow_mutable_version,
                signing_key: args.signing_key,
                deprecation: PublicationDeprecation {
                    deprecated: args.deprecated,
                    reason: args.deprecation_reason,
                    skill_replacement: args.replacement_skill,
                    pack_replacement: args.replacement_pack,
                    sunset_at: args.sunset_at,
                },
            };
            print_json(&client.import_skiller_bundle(request)?)
        }
        Commands::Load { id, mode } => print_json(&client.load_skill(&id, mode.0)?),
        Commands::Manifest { id } => print_json(&client.get_manifest(&id)?),
        Commands::VerifyTrust { id } => print_json(&client.verify_trust(&id)?),
        Commands::EvaluateTrust {
            id,
            policy,
            dependencies,
        } => {
            let policy = read_policy(policy)?;
            let request = TrustEvaluationRequest {
                id,
                policy,
                dependency_graph: dependencies,
            };
            print_json(&client.evaluate_trust(request)?)
        }
        Commands::CheckCompatibility(args) => {
            let request = CompatibilityRequest {
                skill_id: args.id,
                query: RuntimeCompatibilityQuery {
                    msp_version: Some(args.msp_version),
                    supported_manifest_versions: args.supported_manifest_versions,
                    runtime_name: Some(args.runtime_name),
                    runtime_version: None,
                    supported_formats: args.supported_formats,
                    runtime_capabilities: args.runtime_capabilities,
                    model_capabilities: args.model_capabilities,
                    available_tools: args.available_tools,
                    tool_versions: Default::default(),
                    permissions: args.permissions,
                    context_window: args.context_window,
                    platform: args.platform,
                },
            };
            print_json(&client.check_compatibility(request)?)
        }
        Commands::ResolveDependencies { id } => print_json(&client.resolve_dependencies(&id)?),
        Commands::DiscoverPacks(args) => {
            let request = PackSearchRequest {
                task: args.task,
                category: args.category,
                issuer: args.issuer,
                max_risk: parse_optional_risk(args.max_risk)?,
                limit: args.limit,
            };
            print_json(&client.discover_packs(request))
        }
        Commands::PackManifest { id } => print_json(&client.get_pack_manifest(&id)?),
    }
}

fn read_policy(path: PathBuf) -> anyhow::Result<SkillTrustPolicy> {
    let raw = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

fn parse_optional_risk(
    value: Option<String>,
) -> anyhow::Result<Option<crate::msp_core::RiskLevel>> {
    value.as_deref().map(parse_risk_level).transpose()
}

fn print_json(value: &impl serde::Serialize) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
