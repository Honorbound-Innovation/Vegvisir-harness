use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use vegvisir_rust::{
    core::{
        AgentProfile, AgentProfileStore, McpConfigStore, ModelRegistry, ProviderRegistry,
        default_tool_definitions, load_skill_definitions, normalize_agent_id,
    },
    memory::default_vegvisir_data_root,
};

#[derive(Parser)]
#[command(
    name = "vegvisir-agent-admin",
    bin_name = "vegvisir-agent-admin",
    about = "Standalone Vegvisir agent registry administration tool"
)]
struct Cli {
    /// Vegvisir data root. Defaults to VEGVISIR_HOME, XDG_DATA_HOME/vegvisir, or ~/.local/share/vegvisir.
    #[arg(long, global = true)]
    data_root: Option<PathBuf>,
    /// Print machine-readable JSON where supported.
    #[arg(long, global = true)]
    json: bool,
    /// Workspace used for workspace-local skills and Skiller agent-pack discovery.
    #[arg(long, global = true)]
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Print registry paths used by this binary.
    Paths,
    /// Validate and summarize the agent registry.
    Doctor,
    /// Register missing built-in and Skiller-generated agent identities.
    Register(RegisterArgs),
    /// Validate one profile, or the whole registry when no id is supplied.
    Validate { id: Option<String> },
    /// Show recorded metrics and ability tracking data for one agent.
    Metrics { id: String },
    /// Compare two agent profiles.
    Compare {
        left_id: String,
        right_id: String,
        /// Include full prompt text in the comparison output.
        #[arg(long)]
        prompts: bool,
    },
    /// Show edit history for an agent, or all agents when no id is supplied.
    History { id: Option<String> },
    /// Set lifecycle status metadata. Active status requires validation without hard errors.
    Status { id: String, status: String },
    /// Tune scope metadata used by operators and subagent delegation planning.
    Scope(ScopeArgs),
    /// Replace tag metadata for filtering and domain specialization.
    Tags { id: String, tags: Vec<String> },
    /// Tune default work-budget metadata for future subagent use.
    Budget(BudgetArgs),
    /// List or show built-in agent templates.
    Templates {
        /// Optional template/mode id to show.
        id: Option<String>,
    },
    /// List registered agents.
    List(ListArgs),
    /// Show one agent profile.
    Show { id: String },
    /// Create a new agent profile.
    Create(CreateArgs),
    /// Create a new profile from a built-in template.
    #[command(name = "create-template", alias = "from-template")]
    CreateTemplate(CreateTemplateArgs),
    /// Design a profile with one command, including permissions and defaults.
    Design(DesignArgs),
    /// Update fields on an existing agent profile.
    Set(SetArgs),
    /// Set display name.
    Name { id: String, name: Vec<String> },
    /// Set mode.
    Mode { id: String, mode: String },
    /// Set description.
    #[command(alias = "description")]
    Describe {
        id: String,
        description: Vec<String>,
    },
    /// Set or clear provider. Use '-' or 'clear' to clear provider and model.
    Provider { id: String, provider: String },
    /// Set or clear model. Use '-' or 'clear' to clear.
    Model { id: String, model: String },
    /// Replace the system prompt from text or --prompt-file.
    #[command(alias = "system")]
    Prompt(PromptArgs),
    /// Allow one tool for an agent. Use '*' only when intentionally unrestricted.
    #[command(name = "allow-tool", alias = "tool")]
    AllowTool { id: String, tool: String },
    /// Revoke one tool from an agent.
    #[command(name = "revoke-tool")]
    RevokeTool { id: String, tool: String },
    /// Replace the tool allow-list.
    #[command(name = "set-tools")]
    SetTools { id: String, tools: Vec<String> },
    /// Enable one skill for an agent.
    #[command(name = "enable-skill", alias = "skill")]
    EnableSkill { id: String, skill: String },
    /// Disable one skill for an agent.
    #[command(name = "disable-skill")]
    DisableSkill { id: String, skill: String },
    /// Replace enabled skills.
    #[command(name = "set-skills")]
    SetSkills { id: String, skills: Vec<String> },
    /// Allow one MCP server for an agent.
    #[command(name = "allow-mcp", alias = "mcp")]
    AllowMcp { id: String, server: String },
    /// Revoke one MCP server from an agent.
    #[command(name = "revoke-mcp")]
    RevokeMcp { id: String, server: String },
    /// Replace allowed MCP servers.
    #[command(name = "set-mcp")]
    SetMcp { id: String, servers: Vec<String> },
    /// Bind a USRL contract reference.
    #[command(name = "bind-usrl", alias = "usrl")]
    BindUsrl { id: String, contract: String },
    /// Unbind a USRL contract reference.
    #[command(name = "unbind-usrl")]
    UnbindUsrl { id: String, contract: String },
    /// Replace bound USRL contract references.
    #[command(name = "set-usrl")]
    SetUsrl { id: String, contracts: Vec<String> },
    /// Set memory policy label.
    #[command(name = "memory-policy", alias = "memory")]
    MemoryPolicy { id: String, policy: String },
    /// Set explicit CMS scope ids for an agent.
    #[command(name = "cms-scope")]
    CmsScope {
        id: String,
        #[arg(long)]
        user: String,
        #[arg(long)]
        project: String,
    },
    /// Reset CMS scope to agent:<id> for user and project.
    #[command(name = "reset-cms-scope")]
    ResetCmsScope { id: String },
    /// Clone an existing profile to a new id.
    Clone {
        source_id: String,
        new_id: String,
        #[arg(long)]
        name: Option<String>,
        /// Overwrite destination if it already exists.
        #[arg(long)]
        force: bool,
    },
    /// Delete an agent profile. Requires --yes.
    Delete {
        id: String,
        #[arg(long)]
        yes: bool,
    },
    /// Export one profile to stdout or a JSON file.
    Export {
        id: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Import a profile JSON file into the registry.
    Import {
        path: PathBuf,
        /// Overwrite an existing profile with the same id.
        #[arg(long)]
        force: bool,
    },
    /// Launch a small interactive registry editor shell.
    Tui,
}

#[derive(Args, Default)]
struct ListArgs {
    /// Include prompt and metadata summary in text output.
    #[arg(long)]
    long: bool,
    /// Filter by mode.
    #[arg(long)]
    mode: Option<String>,
}

#[derive(Args, Default)]
struct RegisterArgs {
    /// Register only built-in templates.
    #[arg(long)]
    builtins_only: bool,
    /// Register only Skiller agent-pack/proposal artifacts.
    #[arg(long)]
    skiller_only: bool,
    /// Report what would be registered without writing profiles.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args)]
struct ScopeArgs {
    id: String,
    /// Primary scope/domain for the agent.
    #[arg(long)]
    primary: Option<String>,
    /// Comma-separated secondary scopes.
    #[arg(long, value_delimiter = ',')]
    secondary: Vec<String>,
    /// Workspace or repository scope label/path for this agent profile.
    #[arg(long = "workspace-scope")]
    workspace_scope: Option<String>,
    /// Comma-separated file-scope hints.
    #[arg(long, value_delimiter = ',')]
    file_scope: Vec<String>,
}

#[derive(Args)]
struct BudgetArgs {
    id: String,
    #[arg(long)]
    max_steps: Option<u64>,
    #[arg(long)]
    max_tool_calls: Option<u64>,
    #[arg(long)]
    max_read_bytes: Option<u64>,
    #[arg(long)]
    max_output_bytes: Option<u64>,
    #[arg(long, value_delimiter = ',')]
    allowed_tools: Vec<String>,
    #[arg(long)]
    notes: Option<String>,
    /// Clear the stored default work budget.
    #[arg(long)]
    clear: bool,
}

#[derive(Args)]
struct CreateArgs {
    id: String,
    /// Start from a built-in template/mode before applying other options.
    #[arg(long)]
    template: Option<String>,
    /// Agent mode, e.g. engineer, planner, tester, skiller, custom.
    #[arg(long, default_value = "custom")]
    mode: String,
    /// Display name. Defaults to the normalized id or template display name.
    #[arg(long)]
    name: Option<String>,
    /// Short description.
    #[arg(long)]
    description: Option<String>,
    /// System prompt text. Use --prompt-file for long prompts.
    #[arg(long, conflicts_with = "prompt_file")]
    prompt: Option<String>,
    /// File containing the system prompt.
    #[arg(long)]
    prompt_file: Option<PathBuf>,
    /// Default provider for this agent.
    #[arg(long)]
    provider: Option<String>,
    /// Default model for this agent.
    #[arg(long)]
    model: Option<String>,
    /// Comma-separated enabled tool names. Use '*' only when intentionally unrestricted.
    #[arg(long, value_delimiter = ',')]
    tools: Vec<String>,
    /// Append tools to the template/default list instead of replacing it.
    #[arg(long)]
    add_tools: bool,
    /// Comma-separated enabled skill names.
    #[arg(long, value_delimiter = ',')]
    skills: Vec<String>,
    /// Append skills to the template/default list instead of replacing it.
    #[arg(long)]
    add_skills: bool,
    /// Comma-separated enabled MCP server ids.
    #[arg(long, value_delimiter = ',')]
    mcp: Vec<String>,
    /// Comma-separated USRL contract refs.
    #[arg(long, value_delimiter = ',')]
    usrl: Vec<String>,
    /// Agent memory policy label.
    #[arg(long)]
    memory_policy: Option<String>,
    /// Overwrite an existing profile.
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct CreateTemplateArgs {
    mode: String,
    id: String,
    /// Display name override.
    #[arg(long)]
    name: Option<String>,
    /// Description override.
    #[arg(long)]
    description: Option<String>,
    /// Overwrite an existing profile.
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct DesignArgs {
    id: String,
    /// Agent mode/template.
    #[arg(long, default_value = "custom")]
    mode: String,
    /// Display name.
    #[arg(long)]
    name: String,
    /// System prompt text. Use --prompt-file for long prompts.
    #[arg(long, conflicts_with = "prompt_file")]
    prompt: Option<String>,
    /// File containing the system prompt.
    #[arg(long)]
    prompt_file: Option<PathBuf>,
    #[arg(long)]
    description: Option<String>,
    #[arg(long)]
    provider: Option<String>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long, value_delimiter = ',')]
    tools: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    skills: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    mcp: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    usrl: Vec<String>,
    #[arg(long)]
    memory_policy: Option<String>,
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct SetArgs {
    id: String,
    #[arg(long)]
    mode: Option<String>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    description: Option<String>,
    #[arg(long, conflicts_with = "prompt_file")]
    prompt: Option<String>,
    #[arg(long)]
    prompt_file: Option<PathBuf>,
    #[arg(long)]
    provider: Option<String>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long, value_delimiter = ',')]
    tools: Option<Vec<String>>,
    #[arg(long, value_delimiter = ',')]
    add_tools: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    remove_tools: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    skills: Option<Vec<String>>,
    #[arg(long, value_delimiter = ',')]
    add_skills: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    remove_skills: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    mcp: Option<Vec<String>>,
    #[arg(long, value_delimiter = ',')]
    add_mcp: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    remove_mcp: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    usrl: Option<Vec<String>>,
    #[arg(long, value_delimiter = ',')]
    add_usrl: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    remove_usrl: Vec<String>,
    #[arg(long)]
    memory_policy: Option<String>,
    #[arg(long)]
    cms_user: Option<String>,
    #[arg(long)]
    cms_project: Option<String>,
}

#[derive(Args)]
struct PromptArgs {
    id: String,
    /// Prompt text as remaining positional words.
    prompt: Vec<String>,
    /// File containing the system prompt.
    #[arg(long, conflicts_with = "prompt")]
    prompt_file: Option<PathBuf>,
}

#[derive(Clone, Serialize)]
struct AgentTemplate {
    mode: String,
    display_name: String,
    description: String,
    system_prompt: String,
    enabled_tools: Vec<String>,
    enabled_skills: Vec<String>,
    usrl_contracts: Vec<String>,
    memory_policy: String,
}

#[derive(Serialize)]
struct DoctorReport {
    agents_root: PathBuf,
    profile_count: usize,
    invalid_files: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Clone, Serialize)]
struct ValidationIssue {
    severity: String,
    field: String,
    message: String,
}

#[derive(Clone, Serialize)]
struct ValidationReport {
    id: String,
    status: String,
    errors: Vec<ValidationIssue>,
    warnings: Vec<ValidationIssue>,
    recommendations: Vec<ValidationIssue>,
}

#[derive(Default, Serialize)]
struct RegisterReport {
    builtin_created: usize,
    skiller_created: usize,
    dry_run: bool,
    created_ids: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct AgentMetrics {
    #[serde(default)]
    tasks_completed: u64,
    #[serde(default)]
    tasks_failed: u64,
    #[serde(default)]
    tasks_cancelled: u64,
    #[serde(default)]
    verification_successes: u64,
    #[serde(default)]
    verification_failures: u64,
    #[serde(default)]
    scope_violations: u64,
    #[serde(default)]
    follow_up_fixes: u64,
    #[serde(default)]
    retries: u64,
    #[serde(default)]
    average_turnaround_ms: Option<u64>,
    #[serde(default)]
    capability_scores: BTreeMap<String, f64>,
    #[serde(default)]
    confidence: Option<f64>,
    #[serde(default)]
    last_evaluated: Option<String>,
    #[serde(default)]
    notes: Vec<String>,
}

#[derive(Serialize)]
struct MetricsReport {
    id: String,
    path: PathBuf,
    metrics: AgentMetrics,
    verification_success_rate: Option<f64>,
    task_success_rate: Option<f64>,
    warnings: Vec<String>,
}

#[derive(Serialize)]
struct AgentComparison {
    left_id: String,
    right_id: String,
    differences: Vec<FieldDifference>,
}

#[derive(Serialize)]
struct FieldDifference {
    field: String,
    left: Value,
    right: Value,
}

#[derive(Serialize, Deserialize)]
struct HistoryEvent {
    agent_id: String,
    action: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    metadata: BTreeMap<String, Value>,
    timestamp: String,
}

#[derive(Debug)]
enum SkillerAgentArtifact {
    Pack(PathBuf),
    ProposalIndex(PathBuf),
}

#[derive(Debug, Default, Deserialize)]
struct SkillerAgentPackOnDisk {
    #[serde(default)]
    agent_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    system_prompt_material: String,
    #[serde(default)]
    skill_ids: Vec<String>,
    #[serde(default)]
    tool_permissions: Vec<String>,
    #[serde(default)]
    memory_policy: String,
    #[serde(default)]
    source_bundle_ids: Vec<String>,
    #[serde(default)]
    source_bundle_name: String,
    #[serde(default)]
    source_bundle_version: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let data_root = cli.data_root.unwrap_or_else(default_vegvisir_data_root);
    let workspace = cli.workspace.unwrap_or(std::env::current_dir()?);
    let registry = AgentRegistryAdmin::new(data_root, workspace)?;
    match cli.command.unwrap_or(Command::List(ListArgs::default())) {
        Command::Paths => registry.print_paths(cli.json),
        Command::Doctor => registry.doctor(cli.json),
        Command::Register(args) => registry.register(args, cli.json),
        Command::Validate { id } => registry.validate(id.as_deref(), cli.json),
        Command::Metrics { id } => registry.metrics(&id, cli.json),
        Command::Compare {
            left_id,
            right_id,
            prompts,
        } => registry.compare(&left_id, &right_id, prompts, cli.json),
        Command::History { id } => registry.history(id.as_deref(), cli.json),
        Command::Status { id, status } => registry.status(&id, status, cli.json),
        Command::Scope(args) => registry.scope(args, cli.json),
        Command::Tags { id, tags } => registry.tags(&id, tags, cli.json),
        Command::Budget(args) => registry.budget(args, cli.json),
        Command::Templates { id } => registry.templates(id.as_deref(), cli.json),
        Command::List(args) => registry.list(args, cli.json),
        Command::Show { id } => registry.show(&id, cli.json),
        Command::Create(args) => registry.create(args, cli.json),
        Command::CreateTemplate(args) => registry.create_template(args, cli.json),
        Command::Design(args) => registry.design(args, cli.json),
        Command::Set(args) => registry.set(args, cli.json),
        Command::Name { id, name } => registry.name(&id, join_required("name", name)?, cli.json),
        Command::Mode { id, mode } => registry.mode(&id, mode, cli.json),
        Command::Describe { id, description } => {
            registry.describe(&id, join_required("description", description)?, cli.json)
        }
        Command::Provider { id, provider } => registry.provider(&id, provider, cli.json),
        Command::Model { id, model } => registry.model(&id, model, cli.json),
        Command::Prompt(args) => registry.prompt(args, cli.json),
        Command::AllowTool { id, tool } => {
            registry.add_to_list(&id, ListField::Tools, tool, cli.json)
        }
        Command::RevokeTool { id, tool } => {
            registry.remove_from_list(&id, ListField::Tools, &tool, cli.json)
        }
        Command::SetTools { id, tools } => {
            registry.replace_list(&id, ListField::Tools, tools, cli.json)
        }
        Command::EnableSkill { id, skill } => {
            registry.add_to_list(&id, ListField::Skills, skill, cli.json)
        }
        Command::DisableSkill { id, skill } => {
            registry.remove_from_list(&id, ListField::Skills, &skill, cli.json)
        }
        Command::SetSkills { id, skills } => {
            registry.replace_list(&id, ListField::Skills, skills, cli.json)
        }
        Command::AllowMcp { id, server } => {
            registry.add_to_list(&id, ListField::Mcp, server, cli.json)
        }
        Command::RevokeMcp { id, server } => {
            registry.remove_from_list(&id, ListField::Mcp, &server, cli.json)
        }
        Command::SetMcp { id, servers } => {
            registry.replace_list(&id, ListField::Mcp, servers, cli.json)
        }
        Command::BindUsrl { id, contract } => {
            registry.add_to_list(&id, ListField::Usrl, contract, cli.json)
        }
        Command::UnbindUsrl { id, contract } => {
            registry.remove_from_list(&id, ListField::Usrl, &contract, cli.json)
        }
        Command::SetUsrl { id, contracts } => {
            registry.replace_list(&id, ListField::Usrl, contracts, cli.json)
        }
        Command::MemoryPolicy { id, policy } => registry.memory_policy(&id, policy, cli.json),
        Command::CmsScope { id, user, project } => registry.cms_scope(&id, user, project, cli.json),
        Command::ResetCmsScope { id } => registry.reset_cms_scope(&id, cli.json),
        Command::Clone {
            source_id,
            new_id,
            name,
            force,
        } => registry.clone_profile(&source_id, &new_id, name, force, cli.json),
        Command::Delete { id, yes } => registry.delete(&id, yes, cli.json),
        Command::Export { id, out } => registry.export(&id, out),
        Command::Import { path, force } => registry.import(&path, force, cli.json),
        Command::Tui => registry.tui(),
    }
}

struct AgentRegistryAdmin {
    data_root: PathBuf,
    workspace: PathBuf,
    store: AgentProfileStore,
}

#[derive(Copy, Clone)]
enum ListField {
    Tools,
    Skills,
    Mcp,
    Usrl,
}

impl ListField {
    fn label(self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::Skills => "skills",
            Self::Mcp => "mcp_servers",
            Self::Usrl => "usrl_contracts",
        }
    }

    fn get_mut<'a>(self, profile: &'a mut AgentProfile) -> &'a mut Vec<String> {
        match self {
            Self::Tools => &mut profile.enabled_tools,
            Self::Skills => &mut profile.enabled_skills,
            Self::Mcp => &mut profile.enabled_mcp_servers,
            Self::Usrl => &mut profile.usrl_contracts,
        }
    }
}

impl AgentRegistryAdmin {
    fn new(data_root: PathBuf, workspace: PathBuf) -> anyhow::Result<Self> {
        let store = AgentProfileStore::new(data_root.join("agents"))?;
        Ok(Self {
            data_root,
            workspace,
            store,
        })
    }

    fn print_paths(&self, json_output: bool) -> anyhow::Result<()> {
        print_json_or_text(
            json_output,
            &json!({
                "data_root": self.data_root,
                "agents_root": self.store.root,
                "workspace": self.workspace,
            }),
            || {
                println!("data_root: {}", self.data_root.display());
                println!("agents_root: {}", self.store.root.display());
                println!("workspace: {}", self.workspace.display());
                Ok(())
            },
        )
    }

    fn doctor(&self, json_output: bool) -> anyhow::Result<()> {
        let (profiles, invalid_files) = self.store.list_lossy()?;
        let mut warnings = Vec::new();
        let mut ids = BTreeSet::new();
        let mut cms_scopes: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for profile in &profiles {
            if !ids.insert(profile.id.clone()) {
                warnings.push(format!("duplicate profile id loaded: {}", profile.id));
            }
            let expected_path = self.store.path_for(&profile.id);
            if !expected_path.exists() {
                warnings.push(format!(
                    "profile {} is not stored at expected path {}",
                    profile.id,
                    expected_path.display()
                ));
            }
            if profile.system_prompt.trim().is_empty() {
                warnings.push(format!("profile {} has an empty system prompt", profile.id));
            }
            if profile.display_name.trim().is_empty() {
                warnings.push(format!("profile {} has an empty display name", profile.id));
            }
            if profile
                .enabled_tools
                .iter()
                .any(|tool| tool.trim().is_empty())
            {
                warnings.push(format!("profile {} has an empty tool entry", profile.id));
            }
            if profile.current_model.is_some() && profile.current_provider.is_none() {
                warnings.push(format!(
                    "profile {} sets a model but no provider; runtime will inherit current provider",
                    profile.id
                ));
            }
            cms_scopes
                .entry((profile.cms_user_id.clone(), profile.cms_project_id.clone()))
                .or_default()
                .push(profile.id.clone());
        }
        for ((user, project), profile_ids) in cms_scopes {
            if profile_ids.len() > 1 {
                warnings.push(format!(
                    "profiles share CMS scope {user}/{project}: {}",
                    profile_ids.join(",")
                ));
            }
        }
        let report = DoctorReport {
            agents_root: self.store.root.clone(),
            profile_count: profiles.len(),
            invalid_files,
            warnings,
        };
        if json_output {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!("Agent registry: {}", report.agents_root.display());
            println!("Profiles: {}", report.profile_count);
            if report.invalid_files.is_empty() && report.warnings.is_empty() {
                println!("Status: ok");
            }
            if !report.invalid_files.is_empty() {
                println!("\nInvalid files:");
                for item in &report.invalid_files {
                    println!("- {item}");
                }
            }
            if !report.warnings.is_empty() {
                println!("\nWarnings:");
                for item in &report.warnings {
                    println!("- {item}");
                }
            }
        }
        Ok(())
    }

    fn register(&self, args: RegisterArgs, json_output: bool) -> anyhow::Result<()> {
        let mut report = RegisterReport {
            dry_run: args.dry_run,
            ..RegisterReport::default()
        };
        let (profiles, warnings) = self.store.list_lossy()?;
        report.warnings.extend(warnings);
        let mut known_ids = profiles
            .into_iter()
            .map(|profile| profile.id)
            .collect::<BTreeSet<_>>();

        if !args.skiller_only {
            for template in agent_templates() {
                if known_ids.contains(&template.mode) {
                    continue;
                }
                report.created_ids.push(template.mode.clone());
                report.builtin_created += 1;
                if !args.dry_run {
                    let mut profile = profile_from_template(&template.mode, &template.mode, None)?;
                    profile
                        .metadata
                        .insert("registered_identity".to_string(), Value::Bool(true));
                    profile
                        .metadata
                        .insert("identity_source".to_string(), json!("builtin-template"));
                    touch_metadata(&mut profile, "register-builtin");
                    let saved = self.store.save(&profile)?;
                    self.append_history(&profile, "register-builtin", &saved)?;
                }
                known_ids.insert(template.mode);
            }
        }

        if !args.builtins_only {
            for artifact in find_skiller_agent_artifacts(&self.workspace, &self.data_root) {
                match artifact {
                    Ok(SkillerAgentArtifact::Pack(path)) => {
                        match self.register_skiller_pack(&mut known_ids, &path, args.dry_run) {
                            Ok(Some(id)) => {
                                report.skiller_created += 1;
                                report.created_ids.push(id);
                            }
                            Ok(None) => {}
                            Err(error) => report.warnings.push(format!(
                                "skipped Skiller agent pack {}: {error}",
                                path.display()
                            )),
                        }
                    }
                    Ok(SkillerAgentArtifact::ProposalIndex(path)) => {
                        match self.register_skiller_proposals(&mut known_ids, &path, args.dry_run) {
                            Ok(ids) => {
                                report.skiller_created += ids.len();
                                report.created_ids.extend(ids);
                            }
                            Err(error) => report.warnings.push(format!(
                                "skipped Skiller agent proposal index {}: {error}",
                                path.display()
                            )),
                        }
                    }
                    Err(error) => report.warnings.push(error.to_string()),
                }
            }
        }

        if json_output {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!("Registered built-in agents: {}", report.builtin_created);
            println!("Registered Skiller agents: {}", report.skiller_created);
            if args.dry_run {
                println!("Dry run: no profiles written.");
            }
            if !report.created_ids.is_empty() {
                println!("IDs: {}", report.created_ids.join(","));
            }
            if !report.warnings.is_empty() {
                println!("\nWarnings:");
                for warning in &report.warnings {
                    println!("- {warning}");
                }
            }
        }
        Ok(())
    }

    fn validate(&self, id: Option<&str>, json_output: bool) -> anyhow::Result<()> {
        if let Some(id) = id {
            let profile = self.store.load(id)?;
            let report = self.validate_profile(&profile)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_validation_report(&report);
            }
            return Ok(());
        }
        let (profiles, warnings) = self.store.list_lossy()?;
        let mut reports = Vec::new();
        for profile in &profiles {
            reports.push(self.validate_profile(profile)?);
        }
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "agents_root": self.store.root,
                    "load_warnings": warnings,
                    "reports": reports,
                }))?
            );
        } else {
            println!(
                "Validated {} profile(s) in {}",
                reports.len(),
                self.store.root.display()
            );
            for warning in warnings {
                println!("Load warning: {warning}");
            }
            for report in &reports {
                print_validation_report(report);
            }
        }
        Ok(())
    }

    fn metrics(&self, id: &str, json_output: bool) -> anyhow::Result<()> {
        self.store.load(id)?;
        let path = self.metrics_path(id);
        let metrics = if path.exists() {
            serde_json::from_str(
                &fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
            )?
        } else {
            AgentMetrics::default()
        };
        let task_total = metrics.tasks_completed + metrics.tasks_failed + metrics.tasks_cancelled;
        let verified_total = metrics.verification_successes + metrics.verification_failures;
        let report = MetricsReport {
            id: normalize_agent_id(id),
            path,
            verification_success_rate: ratio(metrics.verification_successes, verified_total),
            task_success_rate: ratio(metrics.tasks_completed, task_total),
            warnings: if task_total == 0 {
                vec!["no recorded task metrics for this agent".to_string()]
            } else {
                Vec::new()
            },
            metrics,
        };
        if json_output {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_metrics_report(&report);
        }
        Ok(())
    }

    fn compare(
        &self,
        left_id: &str,
        right_id: &str,
        prompts: bool,
        json_output: bool,
    ) -> anyhow::Result<()> {
        let left = self.store.load(left_id)?;
        let right = self.store.load(right_id)?;
        let mut differences = Vec::new();
        push_diff(
            &mut differences,
            "mode",
            json!(left.mode),
            json!(right.mode),
        );
        push_diff(
            &mut differences,
            "display_name",
            json!(left.display_name),
            json!(right.display_name),
        );
        push_diff(
            &mut differences,
            "description",
            json!(left.description),
            json!(right.description),
        );
        push_diff(
            &mut differences,
            "provider",
            json!(left.current_provider),
            json!(right.current_provider),
        );
        push_diff(
            &mut differences,
            "model",
            json!(left.current_model),
            json!(right.current_model),
        );
        push_diff(
            &mut differences,
            "tools",
            json!(left.enabled_tools),
            json!(right.enabled_tools),
        );
        push_diff(
            &mut differences,
            "skills",
            json!(left.enabled_skills),
            json!(right.enabled_skills),
        );
        push_diff(
            &mut differences,
            "mcp_servers",
            json!(left.enabled_mcp_servers),
            json!(right.enabled_mcp_servers),
        );
        push_diff(
            &mut differences,
            "usrl_contracts",
            json!(left.usrl_contracts),
            json!(right.usrl_contracts),
        );
        push_diff(
            &mut differences,
            "cms_user_id",
            json!(left.cms_user_id),
            json!(right.cms_user_id),
        );
        push_diff(
            &mut differences,
            "cms_project_id",
            json!(left.cms_project_id),
            json!(right.cms_project_id),
        );
        push_diff(
            &mut differences,
            "memory_policy",
            json!(left.memory_policy),
            json!(right.memory_policy),
        );
        push_diff(
            &mut differences,
            "status",
            metadata_json(&left, "status"),
            metadata_json(&right, "status"),
        );
        push_diff(
            &mut differences,
            "primary_scope",
            metadata_json(&left, "primary_scope"),
            metadata_json(&right, "primary_scope"),
        );
        push_diff(
            &mut differences,
            "tags",
            metadata_json(&left, "tags"),
            metadata_json(&right, "tags"),
        );
        if prompts {
            push_diff(
                &mut differences,
                "system_prompt",
                json!(left.system_prompt),
                json!(right.system_prompt),
            );
        } else if left.system_prompt != right.system_prompt {
            differences.push(FieldDifference {
                field: "system_prompt".to_string(),
                left: json!({"bytes": left.system_prompt.len(), "sha256": prompt_digest(&left.system_prompt)}),
                right: json!({"bytes": right.system_prompt.len(), "sha256": prompt_digest(&right.system_prompt)}),
            });
        }
        let comparison = AgentComparison {
            left_id: left.id,
            right_id: right.id,
            differences,
        };
        if json_output {
            println!("{}", serde_json::to_string_pretty(&comparison)?);
        } else {
            print_comparison(&comparison);
        }
        Ok(())
    }

    fn history(&self, id: Option<&str>, json_output: bool) -> anyhow::Result<()> {
        let mut events = self.load_history()?;
        if let Some(id) = id {
            let id = normalize_agent_id(id);
            events.retain(|event| normalize_agent_id(&event.agent_id) == id);
        }
        if json_output {
            println!("{}", serde_json::to_string_pretty(&events)?);
        } else if events.is_empty() {
            println!("No history recorded.");
        } else {
            for event in events.iter().rev().take(100) {
                println!(
                    "{} {:<18} {} {}",
                    event.timestamp, event.agent_id, event.action, event.summary
                );
            }
        }
        Ok(())
    }

    fn status(&self, id: &str, status: String, json_output: bool) -> anyhow::Result<()> {
        let status = normalize_agent_id(&status);
        let allowed = [
            "draft",
            "active",
            "paused",
            "deprecated",
            "archived",
            "broken",
        ];
        if !allowed.contains(&status.as_str()) {
            bail!("status must be one of: {}", allowed.join(","));
        }
        let mut profile = self.store.load(id)?;
        if status == "active" {
            let report = self.validate_profile(&profile)?;
            if !report.errors.is_empty() {
                bail!(
                    "refusing to activate {}: validation has {} error(s)",
                    profile.id,
                    report.errors.len()
                );
            }
        }
        profile
            .metadata
            .insert("status".to_string(), Value::String(status));
        self.save_touched(profile, "status", json_output)
    }

    fn scope(&self, args: ScopeArgs, json_output: bool) -> anyhow::Result<()> {
        let mut profile = self.store.load(&args.id)?;
        if let Some(primary) = args.primary {
            profile
                .metadata
                .insert("primary_scope".to_string(), Value::String(primary));
        }
        if !args.secondary.is_empty() {
            profile.metadata.insert(
                "secondary_scopes".to_string(),
                json!(clean_list(args.secondary)),
            );
        }
        if let Some(workspace_scope) = args.workspace_scope {
            profile.metadata.insert(
                "workspace_scope".to_string(),
                Value::String(workspace_scope),
            );
        }
        if !args.file_scope.is_empty() {
            profile.metadata.insert(
                "file_scope_hints".to_string(),
                json!(clean_list(args.file_scope)),
            );
        }
        self.save_touched(profile, "scope", json_output)
    }

    fn tags(&self, id: &str, tags: Vec<String>, json_output: bool) -> anyhow::Result<()> {
        let mut profile = self.store.load(id)?;
        profile
            .metadata
            .insert("tags".to_string(), json!(clean_list(tags)));
        self.save_touched(profile, "tags", json_output)
    }

    fn budget(&self, args: BudgetArgs, json_output: bool) -> anyhow::Result<()> {
        let mut profile = self.store.load(&args.id)?;
        if args.clear {
            profile.metadata.remove("default_work_budget");
        } else {
            let mut budget = profile
                .metadata
                .get("default_work_budget")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !budget.is_object() {
                budget = json!({});
            }
            let map = budget.as_object_mut().expect("object ensured");
            if let Some(value) = args.max_steps {
                map.insert("max_steps".to_string(), json!(value));
            }
            if let Some(value) = args.max_tool_calls {
                map.insert("max_tool_calls".to_string(), json!(value));
            }
            if let Some(value) = args.max_read_bytes {
                map.insert("max_read_bytes".to_string(), json!(value));
            }
            if let Some(value) = args.max_output_bytes {
                map.insert("max_output_bytes".to_string(), json!(value));
            }
            if !args.allowed_tools.is_empty() {
                map.insert(
                    "allowed_tools".to_string(),
                    json!(clean_list(args.allowed_tools)),
                );
            }
            if let Some(notes) = args.notes {
                map.insert("notes".to_string(), json!(notes));
            }
            profile
                .metadata
                .insert("default_work_budget".to_string(), budget);
        }
        self.save_touched(profile, "budget", json_output)
    }

    fn templates(&self, id: Option<&str>, json_output: bool) -> anyhow::Result<()> {
        if let Some(id) = id {
            let template = agent_template(id).with_context(|| format!("unknown template: {id}"))?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&template)?);
            } else {
                print_template(&template);
            }
            return Ok(());
        }
        let templates = agent_templates();
        if json_output {
            println!("{}", serde_json::to_string_pretty(&templates)?);
        } else {
            for template in templates {
                println!(
                    "{:<14} {:<22} tools={} skills={}",
                    template.mode,
                    template.display_name,
                    list_or_dash(&template.enabled_tools),
                    list_or_dash(&template.enabled_skills)
                );
            }
        }
        Ok(())
    }

    fn list(&self, args: ListArgs, json_output: bool) -> anyhow::Result<()> {
        let (mut profiles, warnings) = self.store.list_lossy()?;
        if let Some(mode) = args.mode {
            let mode = normalize_agent_id(&mode);
            profiles.retain(|profile| normalize_agent_id(&profile.mode) == mode);
        }
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "agents_root": self.store.root,
                    "profiles": profiles,
                    "warnings": warnings,
                }))?
            );
            return Ok(());
        }
        if profiles.is_empty() {
            println!("No agents found in {}", self.store.root.display());
        } else {
            for profile in profiles {
                println!(
                    "{:<24} mode={:<14} name={} provider={} model={} tools={} skills={}",
                    profile.id,
                    profile.mode,
                    profile.display_name,
                    profile.current_provider.as_deref().unwrap_or("-"),
                    profile.current_model.as_deref().unwrap_or("-"),
                    list_or_dash(&profile.enabled_tools),
                    list_or_dash(&profile.enabled_skills),
                );
                if args.long {
                    println!("  description: {}", dash_if_empty(&profile.description));
                    println!("  cms: {}/{}", profile.cms_user_id, profile.cms_project_id);
                    println!("  mcp: {}", list_or_dash(&profile.enabled_mcp_servers));
                    println!("  usrl: {}", list_or_dash(&profile.usrl_contracts));
                    println!("  memory_policy: {}", profile.memory_policy);
                    println!("  prompt_bytes: {}", profile.system_prompt.len());
                }
            }
        }
        if !warnings.is_empty() {
            println!("\nWarnings:");
            for warning in warnings {
                println!("- {warning}");
            }
        }
        Ok(())
    }

    fn show(&self, id: &str, json_output: bool) -> anyhow::Result<()> {
        let profile = self.store.load(id)?;
        if json_output {
            println!("{}", serde_json::to_string_pretty(&profile)?);
        } else {
            print_profile(&profile);
        }
        Ok(())
    }

    fn create(&self, args: CreateArgs, json_output: bool) -> anyhow::Result<()> {
        let id = normalize_agent_id(&args.id);
        if id.is_empty() {
            bail!("agent id must contain at least one letter or number");
        }
        self.ensure_can_write(&id, args.force)?;

        let mut profile = if let Some(template_id) = &args.template {
            profile_from_template(template_id, &id, args.name.as_deref())?
        } else {
            let prompt = read_prompt(args.prompt.clone(), args.prompt_file.clone())?.unwrap_or_else(|| {
                format!(
                    "You are {name}, a Vegvisir custom agent. Work evidence-first, preserve user work, follow tool and secret boundaries, and report verification clearly.",
                    name = args.name.as_deref().unwrap_or(&id)
                )
            });
            AgentProfile::new(&id, args.name.clone().unwrap_or_else(|| id.clone()), prompt)?
        };

        if args.template.is_none() || args.mode != "custom" {
            profile.mode = normalized_or_default(&args.mode, "custom");
        }
        if let Some(name) = args.name {
            profile.display_name = name;
        }
        if let Some(description) = args.description {
            profile.description = description;
        }
        if let Some(prompt) = read_prompt(args.prompt, args.prompt_file)? {
            profile.system_prompt = prompt;
        }
        profile.current_provider = args.provider.and_then(none_marker);
        profile.current_model = args.model.and_then(none_marker);
        if !args.tools.is_empty() {
            let tools = clean_list(args.tools);
            if args.add_tools {
                append_unique(&mut profile.enabled_tools, tools);
            } else {
                profile.enabled_tools = tools;
            }
        }
        if !args.skills.is_empty() {
            let skills = clean_list(args.skills);
            if args.add_skills {
                append_unique(&mut profile.enabled_skills, skills);
            } else {
                profile.enabled_skills = skills;
            }
        }
        if !args.mcp.is_empty() {
            profile.enabled_mcp_servers = clean_list(args.mcp);
        }
        if !args.usrl.is_empty() {
            profile.usrl_contracts = clean_list(args.usrl);
        }
        if let Some(policy) = args.memory_policy.filter(|value| !value.trim().is_empty()) {
            profile.memory_policy = policy;
        }
        touch_metadata(&mut profile, "created");
        let path = self.store.save(&profile)?;
        self.append_history(&profile, "created", &path)?;
        print_saved(&profile, &path, json_output)
    }

    fn create_template(&self, args: CreateTemplateArgs, json_output: bool) -> anyhow::Result<()> {
        let id = normalize_agent_id(&args.id);
        if id.is_empty() {
            bail!("agent id must contain at least one letter or number");
        }
        self.ensure_can_write(&id, args.force)?;
        let mut profile = profile_from_template(&args.mode, &id, args.name.as_deref())?;
        if let Some(description) = args.description {
            profile.description = description;
        }
        touch_metadata(&mut profile, "created-from-template");
        let path = self.store.save(&profile)?;
        self.append_history(&profile, "created-from-template", &path)?;
        print_saved(&profile, &path, json_output)
    }

    fn design(&self, args: DesignArgs, json_output: bool) -> anyhow::Result<()> {
        let prompt = read_prompt(args.prompt, args.prompt_file)?
            .with_context(|| "design requires --prompt or --prompt-file")?;
        let create_args = CreateArgs {
            id: args.id,
            template: agent_template(&args.mode).map(|_| args.mode.clone()),
            mode: args.mode,
            name: Some(args.name),
            description: args.description,
            prompt: Some(prompt),
            prompt_file: None,
            provider: args.provider,
            model: args.model,
            tools: args.tools,
            add_tools: false,
            skills: args.skills,
            add_skills: false,
            mcp: args.mcp,
            usrl: args.usrl,
            memory_policy: args.memory_policy,
            force: args.force,
        };
        self.create(create_args, json_output)
    }

    fn set(&self, args: SetArgs, json_output: bool) -> anyhow::Result<()> {
        let mut profile = self.store.load(&args.id)?;
        if let Some(mode) = args.mode {
            profile.mode = normalized_or_default(&mode, "custom");
        }
        if let Some(name) = args.name {
            profile.display_name = name;
        }
        if let Some(description) = args.description {
            profile.description = description;
        }
        if let Some(prompt) = read_prompt(args.prompt, args.prompt_file)? {
            profile.system_prompt = prompt;
        }
        if let Some(provider) = args.provider {
            profile.current_provider = none_marker(provider);
            if profile.current_provider.is_none() {
                profile.current_model = None;
            }
        }
        if let Some(model) = args.model {
            profile.current_model = none_marker(model);
        }
        if let Some(tools) = args.tools {
            profile.enabled_tools = clean_list(tools);
        }
        append_unique(&mut profile.enabled_tools, clean_list(args.add_tools));
        remove_all(&mut profile.enabled_tools, &clean_list(args.remove_tools));
        if let Some(skills) = args.skills {
            profile.enabled_skills = clean_list(skills);
        }
        append_unique(&mut profile.enabled_skills, clean_list(args.add_skills));
        remove_all(&mut profile.enabled_skills, &clean_list(args.remove_skills));
        if let Some(mcp) = args.mcp {
            profile.enabled_mcp_servers = clean_list(mcp);
        }
        append_unique(&mut profile.enabled_mcp_servers, clean_list(args.add_mcp));
        remove_all(
            &mut profile.enabled_mcp_servers,
            &clean_list(args.remove_mcp),
        );
        if let Some(usrl) = args.usrl {
            profile.usrl_contracts = clean_list(usrl);
        }
        append_unique(&mut profile.usrl_contracts, clean_list(args.add_usrl));
        remove_all(&mut profile.usrl_contracts, &clean_list(args.remove_usrl));
        if let Some(policy) = args.memory_policy {
            profile.memory_policy = policy;
        }
        match (args.cms_user, args.cms_project) {
            (Some(user), Some(project)) => {
                profile.cms_user_id = user;
                profile.cms_project_id = project;
            }
            (None, None) => {}
            _ => bail!("--cms-user and --cms-project must be passed together"),
        }
        self.save_touched(profile, "set", json_output)
    }

    fn name(&self, id: &str, name: String, json_output: bool) -> anyhow::Result<()> {
        let mut profile = self.store.load(id)?;
        profile.display_name = name;
        self.save_touched(profile, "name", json_output)
    }

    fn mode(&self, id: &str, mode: String, json_output: bool) -> anyhow::Result<()> {
        let mut profile = self.store.load(id)?;
        profile.mode = normalized_or_default(&mode, "custom");
        self.save_touched(profile, "mode", json_output)
    }

    fn describe(&self, id: &str, description: String, json_output: bool) -> anyhow::Result<()> {
        let mut profile = self.store.load(id)?;
        profile.description = description;
        self.save_touched(profile, "describe", json_output)
    }

    fn provider(&self, id: &str, provider: String, json_output: bool) -> anyhow::Result<()> {
        let mut profile = self.store.load(id)?;
        profile.current_provider = none_marker(provider);
        if profile.current_provider.is_none() {
            profile.current_model = None;
        }
        self.save_touched(profile, "provider", json_output)
    }

    fn model(&self, id: &str, model: String, json_output: bool) -> anyhow::Result<()> {
        let mut profile = self.store.load(id)?;
        profile.current_model = none_marker(model);
        self.save_touched(profile, "model", json_output)
    }

    fn prompt(&self, args: PromptArgs, json_output: bool) -> anyhow::Result<()> {
        let mut profile = self.store.load(&args.id)?;
        let prompt = if let Some(path) = args.prompt_file {
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?
        } else {
            join_required("prompt", args.prompt)?
        };
        profile.system_prompt = prompt;
        self.save_touched(profile, "prompt", json_output)
    }

    fn add_to_list(
        &self,
        id: &str,
        field: ListField,
        value: String,
        json_output: bool,
    ) -> anyhow::Result<()> {
        let mut profile = self.store.load(id)?;
        append_unique(field.get_mut(&mut profile), clean_list(vec![value]));
        self.save_touched(profile, &format!("add-{}", field.label()), json_output)
    }

    fn remove_from_list(
        &self,
        id: &str,
        field: ListField,
        value: &str,
        json_output: bool,
    ) -> anyhow::Result<()> {
        let mut profile = self.store.load(id)?;
        remove_all(
            field.get_mut(&mut profile),
            &clean_list(vec![value.to_string()]),
        );
        self.save_touched(profile, &format!("remove-{}", field.label()), json_output)
    }

    fn replace_list(
        &self,
        id: &str,
        field: ListField,
        values: Vec<String>,
        json_output: bool,
    ) -> anyhow::Result<()> {
        let mut profile = self.store.load(id)?;
        *field.get_mut(&mut profile) = clean_list(values);
        self.save_touched(profile, &format!("set-{}", field.label()), json_output)
    }

    fn memory_policy(&self, id: &str, policy: String, json_output: bool) -> anyhow::Result<()> {
        let mut profile = self.store.load(id)?;
        profile.memory_policy = policy;
        self.save_touched(profile, "memory-policy", json_output)
    }

    fn cms_scope(
        &self,
        id: &str,
        user: String,
        project: String,
        json_output: bool,
    ) -> anyhow::Result<()> {
        if user.trim().is_empty() || project.trim().is_empty() {
            bail!("CMS user and project ids must be non-empty");
        }
        let mut profile = self.store.load(id)?;
        profile.cms_user_id = user;
        profile.cms_project_id = project;
        self.save_touched(profile, "cms-scope", json_output)
    }

    fn reset_cms_scope(&self, id: &str, json_output: bool) -> anyhow::Result<()> {
        let mut profile = self.store.load(id)?;
        let scope = format!("agent:{}", profile.id);
        profile.cms_user_id = scope.clone();
        profile.cms_project_id = scope;
        self.save_touched(profile, "reset-cms-scope", json_output)
    }

    fn clone_profile(
        &self,
        source_id: &str,
        new_id: &str,
        name: Option<String>,
        force: bool,
        json_output: bool,
    ) -> anyhow::Result<()> {
        let mut profile = self.store.load(source_id)?;
        let normalized = normalize_agent_id(new_id);
        if normalized.is_empty() {
            bail!("new agent id must contain at least one letter or number");
        }
        self.ensure_can_write(&normalized, force)?;
        profile.id = normalized.clone();
        if let Some(name) = name {
            profile.display_name = name;
        }
        let cms_scope = format!("agent:{normalized}");
        profile.cms_user_id = cms_scope.clone();
        profile.cms_project_id = cms_scope;
        profile.created_at = chrono::Utc::now();
        profile.updated_at = profile.created_at;
        profile.metadata = admin_metadata("cloned");
        profile.metadata.insert(
            "cloned_from".to_string(),
            Value::String(source_id.to_string()),
        );
        let path = self.store.save(&profile)?;
        self.append_history(&profile, "cloned", &path)?;
        print_saved(&profile, &path, json_output)
    }

    fn delete(&self, id: &str, yes: bool, json_output: bool) -> anyhow::Result<()> {
        if !yes {
            bail!("refusing to delete {id} without --yes");
        }
        let path = self.store.delete(id)?;
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "deleted": id,
                    "path": path,
                }))?
            );
        } else {
            println!("Deleted agent {id} at {}", path.display());
        }
        Ok(())
    }

    fn export(&self, id: &str, out: Option<PathBuf>) -> anyhow::Result<()> {
        let profile = self.store.load(id)?;
        let text = serde_json::to_string_pretty(&profile)?;
        if let Some(path) = out {
            std::fs::write(&path, text)?;
            println!("Exported agent {} to {}", profile.id, path.display());
        } else {
            println!("{text}");
        }
        Ok(())
    }

    fn import(&self, path: &PathBuf, force: bool, json_output: bool) -> anyhow::Result<()> {
        let mut profile: AgentProfile = serde_json::from_str(
            &std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?,
        )?;
        profile.id = normalize_agent_id(&profile.id);
        if profile.id.is_empty() {
            bail!("imported agent id must contain at least one letter or number");
        }
        self.ensure_can_write(&profile.id, force)?;
        profile.updated_at = chrono::Utc::now();
        profile.metadata.insert(
            "last_admin_action".to_string(),
            Value::String("imported".to_string()),
        );
        profile.metadata.insert(
            "imported_from".to_string(),
            Value::String(path.display().to_string()),
        );
        let saved = self.store.save(&profile)?;
        self.append_history(&profile, "imported", &saved)?;
        print_saved(&profile, &saved, json_output)
    }

    fn tui(&self) -> anyhow::Result<()> {
        println!("Vegvisir Agent Admin — interactive registry control plane");
        println!("Registry: {}", self.store.root.display());
        println!("Workspace: {}", self.workspace.display());
        println!("Type help for commands. This shell edits the same profiles used by /agent.");
        let mut line = String::new();
        loop {
            print!("agent-admin> ");
            io::stdout().flush()?;
            line.clear();
            if io::stdin().read_line(&mut line)? == 0 {
                break;
            }
            let args = line.split_whitespace().collect::<Vec<_>>();
            match args.as_slice() {
                [] => {}
                ["quit"] | ["exit"] => break,
                ["help"] => print_tui_help(),
                ["paths"] => self.print_paths(false)?,
                ["doctor"] => self.doctor(false)?,
                ["register"] => self.register(RegisterArgs::default(), false)?,
                ["register", "--dry-run"] => self.register(
                    RegisterArgs {
                        dry_run: true,
                        ..RegisterArgs::default()
                    },
                    false,
                )?,
                ["validate"] => self.validate(None, false)?,
                ["validate", id] => self.validate(Some(id), false)?,
                ["metrics", id] => self.metrics(id, false)?,
                ["history"] => self.history(None, false)?,
                ["history", id] => self.history(Some(id), false)?,
                ["compare", left, right] => self.compare(left, right, false, false)?,
                ["templates"] => self.templates(None, false)?,
                ["templates", id] => self.templates(Some(id), false)?,
                ["list"] => self.list(ListArgs::default(), false)?,
                ["list", "--long"] => self.list(
                    ListArgs {
                        long: true,
                        mode: None,
                    },
                    false,
                )?,
                ["show", id] => self.show(id, false)?,
                ["create", id] => self.create_interactive(id)?,
                ["create-template", mode, id] => self.create_template(
                    CreateTemplateArgs {
                        mode: (*mode).to_string(),
                        id: (*id).to_string(),
                        name: None,
                        description: None,
                        force: false,
                    },
                    false,
                )?,
                ["edit", id] | ["tune", id] => self.edit_interactive(id)?,
                ["status", id, status] => self.status(id, (*status).to_string(), false)?,
                ["tags", id, tags @ ..] => self.tags(
                    id,
                    tags.iter().map(|value| (*value).to_string()).collect(),
                    false,
                )?,
                ["allow-tool", id, tool] => {
                    self.add_to_list(id, ListField::Tools, (*tool).to_string(), false)?
                }
                ["revoke-tool", id, tool] => {
                    self.remove_from_list(id, ListField::Tools, tool, false)?
                }
                ["enable-skill", id, skill] => {
                    self.add_to_list(id, ListField::Skills, (*skill).to_string(), false)?
                }
                ["disable-skill", id, skill] => {
                    self.remove_from_list(id, ListField::Skills, skill, false)?
                }
                ["bind-usrl", id, contract] => {
                    self.add_to_list(id, ListField::Usrl, (*contract).to_string(), false)?
                }
                ["unbind-usrl", id, contract] => {
                    self.remove_from_list(id, ListField::Usrl, contract, false)?
                }
                ["delete", id] => self.delete_interactive(id)?,
                [unknown, ..] => println!("Unknown command: {unknown}. Type help."),
            }
        }
        Ok(())
    }

    fn edit_interactive(&self, id: &str) -> anyhow::Result<()> {
        let mut profile = self.store.load(id)?;
        println!("Editing {} ({})", profile.id, profile.display_name);
        let name = prompt_line("display name", Some(&profile.display_name))?;
        if !name.trim().is_empty() {
            profile.display_name = name;
        }
        let description = prompt_line("description", Some(&profile.description))?;
        profile.description = description;
        let status_default = profile
            .metadata
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("draft");
        let status = prompt_line(
            "status [draft|active|paused|deprecated|archived|broken]",
            Some(status_default),
        )?;
        let status = normalize_agent_id(&status);
        if !status.is_empty() {
            profile
                .metadata
                .insert("status".to_string(), Value::String(status));
        }
        let primary_scope = prompt_line(
            "primary scope",
            profile
                .metadata
                .get("primary_scope")
                .and_then(Value::as_str),
        )?;
        if !primary_scope.trim().is_empty() {
            profile
                .metadata
                .insert("primary_scope".to_string(), Value::String(primary_scope));
        }
        let tags_default = profile
            .metadata
            .get("tags")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(",")
            });
        let tags = prompt_line("tags comma-separated", tags_default.as_deref())?;
        if !tags.trim().is_empty() {
            profile
                .metadata
                .insert("tags".to_string(), json!(clean_list(vec![tags])));
        }
        let provider = prompt_line("provider (- to clear)", profile.current_provider.as_deref())?;
        if !provider.trim().is_empty() {
            profile.current_provider = none_marker(provider);
            if profile.current_provider.is_none() {
                profile.current_model = None;
            }
        }
        let model = prompt_line("model (- to clear)", profile.current_model.as_deref())?;
        if !model.trim().is_empty() {
            profile.current_model = none_marker(model);
        }
        let tools = prompt_line(
            "tools comma-separated",
            Some(&list_or_dash(&profile.enabled_tools)),
        )?;
        if tools != "-" {
            profile.enabled_tools = clean_list(vec![tools]);
        }
        let skills = prompt_line(
            "skills comma-separated",
            Some(&list_or_dash(&profile.enabled_skills)),
        )?;
        if skills != "-" {
            profile.enabled_skills = clean_list(vec![skills]);
        }
        let usrl = prompt_line(
            "USRL comma-separated",
            Some(&list_or_dash(&profile.usrl_contracts)),
        )?;
        if usrl != "-" {
            profile.usrl_contracts = clean_list(vec![usrl]);
        }
        let edit_prompt = prompt_line("replace prompt? [no/file/manual]", Some("no"))?;
        match edit_prompt.as_str() {
            "file" => {
                let path = prompt_line("prompt file", None)?;
                profile.system_prompt =
                    fs::read_to_string(&path).with_context(|| format!("read {path}"))?;
            }
            "manual" => {
                println!("Enter system prompt. Finish with a single '.' line.");
                profile.system_prompt = read_multiline_prompt()?;
            }
            _ => {}
        }
        let report = self.validate_profile(&profile)?;
        print_validation_report(&report);
        if !report.errors.is_empty() {
            let answer = prompt_line("save despite validation errors? type yes", Some("no"))?;
            if answer != "yes" {
                println!("Edit cancelled.");
                return Ok(());
            }
        }
        self.save_touched(profile, "interactive-edit", false)
    }

    fn validate_profile(&self, profile: &AgentProfile) -> anyhow::Result<ValidationReport> {
        let providers = ProviderRegistry::default_catalog()?;
        let models = ModelRegistry::default_catalog()?;
        let tools = default_tool_definitions()?;
        let skills = load_skill_definitions(&self.workspace, &self.data_root)?;
        let mcp_servers = McpConfigStore::new(self.data_root.join("mcp.json"))
            .load()
            .unwrap_or_default();
        let tool_names = tools
            .into_iter()
            .map(|tool| tool.name)
            .collect::<BTreeSet<_>>();
        let skill_names = skills
            .into_iter()
            .map(|skill| skill.name)
            .collect::<BTreeSet<_>>();
        let mcp_ids = mcp_servers
            .into_iter()
            .map(|server| server.id)
            .collect::<BTreeSet<_>>();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut recommendations = Vec::new();

        if profile.id.trim().is_empty() {
            errors.push(issue("error", "id", "agent id is empty"));
        }
        if normalize_agent_id(&profile.id) != profile.id {
            errors.push(issue("error", "id", "agent id is not normalized"));
        }
        if profile.display_name.trim().is_empty() {
            errors.push(issue("error", "display_name", "display name is empty"));
        }
        if profile.system_prompt.trim().is_empty() {
            errors.push(issue("error", "system_prompt", "system prompt is empty"));
        }
        if secret_like(&profile.system_prompt) {
            errors.push(issue(
                "error",
                "system_prompt",
                "prompt appears to contain secret-like material",
            ));
        }
        if profile.description.trim().is_empty() {
            recommendations.push(issue(
                "recommendation",
                "description",
                "add a concise description for registry operators",
            ));
        }
        if profile
            .metadata
            .get("primary_scope")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            recommendations.push(issue(
                "recommendation",
                "metadata.primary_scope",
                "set a primary scope for better delegation and filtering",
            ));
        }
        if profile.memory_policy.trim().is_empty() {
            warnings.push(issue("warning", "memory_policy", "memory policy is empty"));
        }
        if profile.cms_user_id.trim().is_empty() || profile.cms_project_id.trim().is_empty() {
            errors.push(issue(
                "error",
                "cms_scope",
                "CMS user/project ids must be non-empty",
            ));
        }
        if let Some(provider) = &profile.current_provider
            && providers.get(provider).is_none()
        {
            errors.push(issue(
                "error",
                "current_provider",
                format!("unknown provider: {provider}"),
            ));
        }
        if let Some(model) = &profile.current_model {
            match models.get(model) {
                Some(model_info) => {
                    if let Some(provider) = &profile.current_provider {
                        if !models.is_model_allowed_for_provider(model_info, provider) {
                            errors.push(issue(
                                "error",
                                "current_model",
                                format!("model {model} is not allowed for provider {provider}"),
                            ));
                        }
                    } else {
                        warnings.push(issue(
                            "warning",
                            "current_model",
                            "model is set but provider is inherited at runtime",
                        ));
                    }
                }
                None => errors.push(issue(
                    "error",
                    "current_model",
                    format!("unknown model: {model}"),
                )),
            }
        }
        for tool in &profile.enabled_tools {
            if tool != "*" && !tool_names.contains(tool) {
                warnings.push(issue(
                    "warning",
                    "enabled_tools",
                    format!("unknown tool: {tool}"),
                ));
            }
            if tool == "*" {
                warnings.push(issue(
                    "warning",
                    "enabled_tools",
                    "wildcard tool access should be used only for trusted operator-reviewed agents",
                ));
            }
        }
        for skill in &profile.enabled_skills {
            if !skill_names.contains(skill) {
                warnings.push(issue(
                    "warning",
                    "enabled_skills",
                    format!("unknown skill in current workspace/data root: {skill}"),
                ));
            }
        }
        for server in &profile.enabled_mcp_servers {
            if !mcp_ids.contains(server) {
                warnings.push(issue(
                    "warning",
                    "enabled_mcp_servers",
                    format!("unknown MCP server in data root mcp.json: {server}"),
                ));
            }
        }
        if profile.enabled_tools.is_empty() {
            recommendations.push(issue(
                "recommendation",
                "enabled_tools",
                "agent has no enabled tools; confirm this is intentional",
            ));
        }
        Ok(ValidationReport {
            id: profile.id.clone(),
            status: if errors.is_empty() {
                "ready"
            } else {
                "blocked"
            }
            .to_string(),
            errors,
            warnings,
            recommendations,
        })
    }

    fn metrics_path(&self, id: &str) -> PathBuf {
        self.data_root
            .join("agents")
            .join("metrics")
            .join(format!("{}.json", normalize_agent_id(id)))
    }

    fn history_path(&self) -> PathBuf {
        self.data_root
            .join("agents")
            .join("history")
            .join("events.jsonl")
    }

    fn load_history(&self) -> anyhow::Result<Vec<HistoryEvent>> {
        let path = self.history_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut events = Vec::new();
        for (index, line) in fs::read_to_string(&path)?.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<HistoryEvent>(line) {
                Ok(event) => events.push(event),
                Err(error) => events.push(HistoryEvent {
                    agent_id: "-".to_string(),
                    action: "invalid-history-record".to_string(),
                    summary: format!("{}:{}: {error}", path.display(), index + 1),
                    metadata: BTreeMap::new(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                }),
            }
        }
        Ok(events)
    }

    fn append_history(
        &self,
        profile: &AgentProfile,
        action: &str,
        path: &Path,
    ) -> anyhow::Result<()> {
        let history_path = self.history_path();
        if let Some(parent) = history_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "path".to_string(),
            Value::String(path.display().to_string()),
        );
        metadata.insert("mode".to_string(), Value::String(profile.mode.clone()));
        metadata.insert("status".to_string(), metadata_json(profile, "status"));
        let event = HistoryEvent {
            agent_id: profile.id.clone(),
            action: action.to_string(),
            summary: format!(
                "{} ({}, mode={})",
                profile.display_name, profile.id, profile.mode
            ),
            metadata,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&history_path)?;
        writeln!(file, "{}", serde_json::to_string(&event)?)?;
        Ok(())
    }

    fn register_skiller_pack(
        &self,
        known_ids: &mut BTreeSet<String>,
        path: &Path,
        dry_run: bool,
    ) -> anyhow::Result<Option<String>> {
        let pack: SkillerAgentPackOnDisk = serde_yaml::from_str(&fs::read_to_string(path)?)?;
        let id = normalize_agent_id(&pack.agent_name);
        if id.is_empty() || known_ids.contains(&id) {
            return Ok(None);
        }
        if !dry_run {
            let prompt = if pack.system_prompt_material.trim().is_empty() {
                format!(
                    "You are {}. Operate as a Skiller-generated specialist agent for source-grounded technical work. Use selected Skiller skills as evidence, respect runtime policy, cite sources when possible, and escalate unsupported or unsafe requests.",
                    pack.agent_name
                )
            } else {
                pack.system_prompt_material.clone()
            };
            let mut profile = AgentProfile::new(&id, &pack.agent_name, prompt)?;
            profile.mode = "skiller".to_string();
            profile.description = if pack.description.trim().is_empty() {
                format!(
                    "Skiller-generated agent pack loaded from {}",
                    path.display()
                )
            } else {
                pack.description.clone()
            };
            profile.enabled_skills = pack.skill_ids.clone();
            profile.enabled_tools = pack
                .tool_permissions
                .iter()
                .filter_map(|permission| permission.split(':').next())
                .map(str::trim)
                .filter(|tool| !tool.is_empty())
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            profile.memory_policy = if pack.memory_policy.trim().is_empty() {
                "agent-scoped".to_string()
            } else {
                pack.memory_policy.clone()
            };
            profile
                .metadata
                .insert("registered_identity".to_string(), json!(true));
            profile
                .metadata
                .insert("identity_source".to_string(), json!("skiller-agent-pack"));
            profile.metadata.insert(
                "artifact_path".to_string(),
                json!(path.display().to_string()),
            );
            profile.metadata.insert(
                "source_bundle_ids".to_string(),
                json!(pack.source_bundle_ids),
            );
            profile.metadata.insert(
                "source_bundle_name".to_string(),
                json!(pack.source_bundle_name),
            );
            profile.metadata.insert(
                "source_bundle_version".to_string(),
                json!(pack.source_bundle_version),
            );
            touch_metadata(&mut profile, "register-skiller-pack");
            let saved = self.store.save(&profile)?;
            self.append_history(&profile, "register-skiller-pack", &saved)?;
        }
        known_ids.insert(id.clone());
        Ok(Some(id))
    }

    fn register_skiller_proposals(
        &self,
        known_ids: &mut BTreeSet<String>,
        index_path: &Path,
        dry_run: bool,
    ) -> anyhow::Result<Vec<String>> {
        let index: skiller::agents::AgentProposalIndex =
            serde_yaml::from_str(&fs::read_to_string(index_path)?)?;
        let base = index_path.parent().unwrap_or_else(|| Path::new("."));
        let mut created = Vec::new();
        for entry in &index.proposals {
            if entry.file.contains("..") || Path::new(&entry.file).is_absolute() {
                continue;
            }
            let proposal_path = base.join(&entry.file);
            let proposal: skiller::models::AgentProfileProposal =
                serde_yaml::from_str(&fs::read_to_string(&proposal_path)?)?;
            let id = normalize_agent_id(&proposal.agent_id);
            if id.is_empty() || known_ids.contains(&id) {
                continue;
            }
            if !dry_run {
                let prompt = format!(
                    "You are {}.\n\nPurpose: {}\n\nRuntime context policy: {}\nReview policy: {}\nEscalation policy: {}\n\nRecommended Skiller skills:\n{}",
                    proposal.agent_name,
                    proposal.agent_purpose,
                    proposal.runtime_context_policy,
                    proposal.review_policy,
                    proposal.escalation_policy,
                    proposal
                        .recommended_skills
                        .iter()
                        .map(|skill| format!("- {skill}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                let mut profile = AgentProfile::new(&id, &proposal.agent_name, prompt)?;
                profile.mode = "skiller".to_string();
                profile.description = proposal.agent_purpose.clone();
                profile.enabled_skills = proposal.recommended_skills.clone();
                profile.enabled_tools = proposal.required_tools.clone();
                profile.memory_policy = "agent-scoped".to_string();
                profile
                    .metadata
                    .insert("registered_identity".to_string(), json!(true));
                profile.metadata.insert(
                    "identity_source".to_string(),
                    json!("skiller-agent-proposal"),
                );
                profile.metadata.insert(
                    "artifact_path".to_string(),
                    json!(proposal_path.display().to_string()),
                );
                profile
                    .metadata
                    .insert("source_bundle_id".to_string(), json!(index.bundle_id));
                profile
                    .metadata
                    .insert("source_bundle_name".to_string(), json!(index.bundle_name));
                profile.metadata.insert(
                    "ready_for_packaging".to_string(),
                    json!(proposal.proposal_readiness.ready_for_packaging),
                );
                profile.metadata.insert(
                    "ready_for_default_use_candidate".to_string(),
                    json!(proposal.proposal_readiness.ready_for_default_use_candidate),
                );
                touch_metadata(&mut profile, "register-skiller-proposal");
                let saved = self.store.save(&profile)?;
                self.append_history(&profile, "register-skiller-proposal", &saved)?;
            }
            known_ids.insert(id.clone());
            created.push(id);
        }
        Ok(created)
    }

    fn create_interactive(&self, id: &str) -> anyhow::Result<()> {
        let template = prompt_line("template/mode (blank for custom)", Some(""))?;
        let name = prompt_line("display name", Some(id))?;
        let mode = prompt_line(
            "mode",
            Some(if template.is_empty() {
                "custom"
            } else {
                &template
            }),
        )?;
        let description = prompt_line("description", Some(""))?;
        println!("Enter system prompt. Finish with a single '.' line.");
        let prompt = read_multiline_prompt()?;
        let args = CreateArgs {
            id: id.to_string(),
            template: (!template.is_empty()).then_some(template),
            mode,
            name: Some(name),
            description: Some(description),
            prompt: Some(prompt),
            prompt_file: None,
            provider: None,
            model: None,
            tools: Vec::new(),
            add_tools: false,
            skills: Vec::new(),
            add_skills: false,
            mcp: Vec::new(),
            usrl: Vec::new(),
            memory_policy: None,
            force: false,
        };
        self.create(args, false)
    }

    fn delete_interactive(&self, id: &str) -> anyhow::Result<()> {
        let answer = prompt_line(&format!("delete {id}? type yes"), Some("no"))?;
        if answer == "yes" {
            self.delete(id, true, false)
        } else {
            println!("Delete cancelled.");
            Ok(())
        }
    }

    fn save_touched(
        &self,
        mut profile: AgentProfile,
        action: &str,
        json_output: bool,
    ) -> anyhow::Result<()> {
        profile.updated_at = chrono::Utc::now();
        touch_metadata(&mut profile, action);
        let path = self.store.save(&profile)?;
        self.append_history(&profile, action, &path)?;
        print_saved(&profile, &path, json_output)
    }

    fn ensure_can_write(&self, id: &str, force: bool) -> anyhow::Result<()> {
        if self.store.path_for(id).exists() && !force {
            bail!("agent {id} already exists; pass --force to overwrite");
        }
        Ok(())
    }
}

fn print_saved(profile: &AgentProfile, path: &Path, json_output: bool) -> anyhow::Result<()> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "id": profile.id,
                "path": path,
                "profile": profile,
            }))?
        );
    } else {
        println!(
            "Saved agent {} ({}, mode={}) at {}",
            profile.id,
            profile.display_name,
            profile.mode,
            path.display()
        );
    }
    Ok(())
}

fn print_tui_help() {
    println!(
        "commands:\n  list [--long]\n  show <id>\n  templates [id]\n  register [--dry-run]\n  validate [id]\n  doctor\n  create <id>\n  create-template <mode> <id>\n  edit <id>\n  status <id> <draft|active|paused|deprecated|archived|broken>\n  tags <id> <tag...>\n  allow-tool <id> <tool> / revoke-tool <id> <tool>\n  enable-skill <id> <skill> / disable-skill <id> <skill>\n  bind-usrl <id> <contract> / unbind-usrl <id> <contract>\n  compare <left> <right>\n  metrics <id>\n  history [id]\n  delete <id>\n  paths\n  quit"
    );
}

fn print_validation_report(report: &ValidationReport) {
    println!("\nValidation {}: {}", report.id, report.status);
    if report.errors.is_empty() && report.warnings.is_empty() && report.recommendations.is_empty() {
        println!("  ok");
        return;
    }
    for issue in &report.errors {
        println!("  ERROR {}: {}", issue.field, issue.message);
    }
    for issue in &report.warnings {
        println!("  WARN  {}: {}", issue.field, issue.message);
    }
    for issue in &report.recommendations {
        println!("  REC   {}: {}", issue.field, issue.message);
    }
}

fn print_metrics_report(report: &MetricsReport) {
    println!("# Metrics: {}", report.id);
    println!("path: {}", report.path.display());
    println!("tasks_completed: {}", report.metrics.tasks_completed);
    println!("tasks_failed: {}", report.metrics.tasks_failed);
    println!("tasks_cancelled: {}", report.metrics.tasks_cancelled);
    println!(
        "task_success_rate: {}",
        percent_or_dash(report.task_success_rate)
    );
    println!(
        "verification_success_rate: {}",
        percent_or_dash(report.verification_success_rate)
    );
    println!("scope_violations: {}", report.metrics.scope_violations);
    println!("follow_up_fixes: {}", report.metrics.follow_up_fixes);
    println!("retries: {}", report.metrics.retries);
    if !report.metrics.capability_scores.is_empty() {
        println!("capability_scores:");
        for (name, score) in &report.metrics.capability_scores {
            println!("  {name}: {score:.2}");
        }
    }
    for warning in &report.warnings {
        println!("warning: {warning}");
    }
}

fn print_comparison(comparison: &AgentComparison) {
    println!(
        "# Compare {} -> {}",
        comparison.left_id, comparison.right_id
    );
    if comparison.differences.is_empty() {
        println!("No differences in compared fields.");
        return;
    }
    for diff in &comparison.differences {
        println!("\n## {}", diff.field);
        println!("left: {}", compact_json(&diff.left));
        println!("right: {}", compact_json(&diff.right));
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unprintable>".to_string())
}

fn issue(severity: &str, field: &str, message: impl Into<String>) -> ValidationIssue {
    ValidationIssue {
        severity: severity.to_string(),
        field: field.to_string(),
        message: message.into(),
    }
}

fn ratio(part: u64, total: u64) -> Option<f64> {
    if total == 0 {
        None
    } else {
        Some(part as f64 / total as f64)
    }
}

fn percent_or_dash(value: Option<f64>) -> String {
    value
        .map(|value| format!("{:.1}%", value * 100.0))
        .unwrap_or_else(|| "-".to_string())
}

fn push_diff(differences: &mut Vec<FieldDifference>, field: &str, left: Value, right: Value) {
    if left != right {
        differences.push(FieldDifference {
            field: field.to_string(),
            left,
            right,
        });
    }
}

fn metadata_json(profile: &AgentProfile, key: &str) -> Value {
    profile.metadata.get(key).cloned().unwrap_or(Value::Null)
}

fn prompt_digest(prompt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompt.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn secret_like(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let patterns = [
        "api_key",
        "apikey",
        "secret_key",
        "access_token",
        "refresh_token",
        "private key",
        "-----begin",
        "password=",
        "authorization: bearer",
    ];
    patterns.iter().any(|pattern| lower.contains(pattern))
}

fn find_skiller_agent_artifacts(
    cwd: &Path,
    data_root: &Path,
) -> Vec<anyhow::Result<SkillerAgentArtifact>> {
    let roots = [
        cwd.join(".vegvisir").join("agent-packs"),
        cwd.join(".vegvisir").join("skiller"),
        cwd.join(".vegvisir").join("skiller-agent-packs"),
        data_root.join("agent-packs"),
        data_root.join("skiller"),
        data_root.join("skiller-agent-packs"),
    ];
    let mut artifacts = Vec::new();
    let mut seen = BTreeSet::new();
    for root in roots {
        collect_skiller_agent_artifacts(&root, 6, &mut seen, &mut artifacts);
    }
    artifacts
}

fn collect_skiller_agent_artifacts(
    path: &Path,
    remaining_depth: usize,
    seen: &mut BTreeSet<PathBuf>,
    artifacts: &mut Vec<anyhow::Result<SkillerAgentArtifact>>,
) {
    if remaining_depth == 0 || !path.exists() {
        return;
    }
    let Ok(metadata) = fs::metadata(path) else {
        artifacts.push(Err(anyhow::anyhow!("could not inspect {}", path.display())));
        return;
    };
    if metadata.is_file() {
        match path.file_name().and_then(|name| name.to_str()) {
            Some("agent-pack.yaml") if seen.insert(path.to_path_buf()) => {
                artifacts.push(Ok(SkillerAgentArtifact::Pack(path.to_path_buf())))
            }
            Some("agent-proposals-index.yaml") if seen.insert(path.to_path_buf()) => {
                artifacts.push(Ok(SkillerAgentArtifact::ProposalIndex(path.to_path_buf())))
            }
            _ => {}
        }
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        artifacts.push(Err(anyhow::anyhow!("could not list {}", path.display())));
        return;
    };
    for entry in entries.flatten() {
        collect_skiller_agent_artifacts(&entry.path(), remaining_depth - 1, seen, artifacts);
    }
}

fn print_profile(profile: &AgentProfile) {
    println!("# Agent: {}", profile.display_name);
    println!("id: {}", profile.id);
    println!("mode: {}", profile.mode);
    println!("description: {}", dash_if_empty(&profile.description));
    println!("cms_user_id: {}", profile.cms_user_id);
    println!("cms_project_id: {}", profile.cms_project_id);
    println!("memory_scope: {}", profile.memory_scope);
    println!(
        "provider: {}",
        profile.current_provider.as_deref().unwrap_or("-")
    );
    println!("model: {}", profile.current_model.as_deref().unwrap_or("-"));
    println!("tools: {}", list_or_dash(&profile.enabled_tools));
    println!("skills: {}", list_or_dash(&profile.enabled_skills));
    println!(
        "mcp_servers: {}",
        list_or_dash(&profile.enabled_mcp_servers)
    );
    println!("usrl_contracts: {}", list_or_dash(&profile.usrl_contracts));
    println!("memory_policy: {}", profile.memory_policy);
    if !profile.metadata.is_empty() {
        println!(
            "metadata: {}",
            serde_json::to_string(&profile.metadata).unwrap_or_else(|_| "{}".to_string())
        );
    }
    println!(
        "\n## System prompt\n\n```text\n{}\n```",
        profile.system_prompt
    );
}

fn print_template(template: &AgentTemplate) {
    println!("# Template: {}", template.display_name);
    println!("mode: {}", template.mode);
    println!("description: {}", template.description);
    println!("tools: {}", list_or_dash(&template.enabled_tools));
    println!("skills: {}", list_or_dash(&template.enabled_skills));
    println!("usrl_contracts: {}", list_or_dash(&template.usrl_contracts));
    println!("memory_policy: {}", template.memory_policy);
    println!(
        "\n## System prompt\n\n```text\n{}\n```",
        template.system_prompt
    );
}

fn read_prompt(
    prompt: Option<String>,
    prompt_file: Option<PathBuf>,
) -> anyhow::Result<Option<String>> {
    if let Some(path) = prompt_file {
        return Ok(Some(
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
        ));
    }
    Ok(prompt)
}

fn read_multiline_prompt() -> anyhow::Result<String> {
    let mut out = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        if io::stdin().read_line(&mut line)? == 0 {
            break;
        }
        if line.trim_end() == "." {
            break;
        }
        out.push_str(&line);
    }
    if out.trim().is_empty() {
        Ok(
            "You are a Vegvisir custom agent. Work evidence-first and preserve user control."
                .to_string(),
        )
    } else {
        Ok(out.trim_end().to_string())
    }
}

fn prompt_line(label: &str, default: Option<&str>) -> anyhow::Result<String> {
    match default {
        Some(default) => print!("{label} [{default}]: "),
        None => print!("{label}: "),
    }
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let trimmed = line.trim();
    if trimmed.is_empty() {
        Ok(default.unwrap_or_default().to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn clean_list(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut cleaned = Vec::new();
    for value in values {
        for item in value.split(',') {
            let item = item.trim();
            if !item.is_empty() && seen.insert(item.to_string()) {
                cleaned.push(item.to_string());
            }
        }
    }
    cleaned
}

fn append_unique(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target.contains(&value) {
            target.push(value);
        }
    }
}

fn remove_all(target: &mut Vec<String>, values: &[String]) {
    target.retain(|item| !values.contains(item));
}

fn list_or_dash(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(",")
    }
}

fn dash_if_empty(value: &str) -> &str {
    if value.trim().is_empty() { "-" } else { value }
}

fn normalized_or_default(value: &str, default: &str) -> String {
    let normalized = normalize_agent_id(value);
    if normalized.is_empty() {
        default.to_string()
    } else {
        normalized
    }
}

fn none_marker(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "-" || trimmed.eq_ignore_ascii_case("clear") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn join_required(label: &str, values: Vec<String>) -> anyhow::Result<String> {
    let joined = values.join(" ").trim().to_string();
    if joined.is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(joined)
}

fn admin_metadata(action: &str) -> BTreeMap<String, Value> {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "managed_by".to_string(),
        Value::String("vegvisir-agent-admin".to_string()),
    );
    metadata.insert(
        "last_admin_action".to_string(),
        Value::String(action.to_string()),
    );
    metadata
}

fn touch_metadata(profile: &mut AgentProfile, action: &str) {
    profile.metadata.insert(
        "managed_by".to_string(),
        Value::String("vegvisir-agent-admin".to_string()),
    );
    profile.metadata.insert(
        "last_admin_action".to_string(),
        Value::String(action.to_string()),
    );
}

fn print_json_or_text<F>(json_output: bool, value: &Value, text: F) -> anyhow::Result<()>
where
    F: FnOnce() -> anyhow::Result<()>,
{
    if json_output {
        println!("{}", serde_json::to_string_pretty(value)?);
        Ok(())
    } else {
        text()
    }
}

fn profile_from_template(
    mode: &str,
    id: &str,
    name_override: Option<&str>,
) -> anyhow::Result<AgentProfile> {
    let template = agent_template(mode).with_context(|| format!("unknown template: {mode}"))?;
    let mut profile = AgentProfile::new(
        id,
        name_override.unwrap_or(&template.display_name),
        &template.system_prompt,
    )?;
    profile.mode = template.mode.clone();
    profile.description = template.description.clone();
    profile.enabled_tools = template.enabled_tools.clone();
    profile.enabled_skills = template.enabled_skills.clone();
    profile.usrl_contracts = template.usrl_contracts.clone();
    profile.memory_policy = template.memory_policy.clone();
    profile
        .metadata
        .insert("template".to_string(), Value::String(template.mode));
    profile
        .metadata
        .insert("registered_identity".to_string(), Value::Bool(false));
    profile
        .metadata
        .insert("identity_source".to_string(), json!("agent-admin-template"));
    Ok(profile)
}

fn agent_template(mode: &str) -> Option<AgentTemplate> {
    let normalized = normalize_agent_id(mode);
    agent_templates()
        .into_iter()
        .find(|template| template.mode == normalized)
}

fn agent_templates() -> Vec<AgentTemplate> {
    vec![
        template(
            "planner",
            "Planner",
            "Decomposes goals into staged, verifiable plans.",
            "You are a planning specialist. Convert ambiguous goals into concrete phases, dependencies, risks, acceptance checks, and next actions. Do not edit files unless explicitly asked through an enabled tool path.",
            &[
                "list_files",
                "read_file",
                "cms_recall",
                "cms_recent",
                "cms_search_chatgpt_archive",
                "cms_prepare_context",
                "save_session",
            ],
        ),
        template(
            "researcher",
            "Researcher",
            "Finds, compares, and summarizes project evidence.",
            "You are a research specialist. Gather relevant local context, distinguish evidence from inference, cite files or memories when available, and produce concise findings with uncertainty called out.",
            &[
                "list_files",
                "read_file",
                "cms_recall",
                "cms_recent",
                "cms_search_chatgpt_archive",
                "cms_remember",
                "cms_prepare_context",
            ],
        ),
        template(
            "orchestrator",
            "Orchestrator",
            "Coordinates specialist agents and tracks execution state.",
            "You are an orchestration specialist. Break work into bounded tasks, delegate when useful, merge results, maintain task state, and keep execution aligned with the user's current objective.",
            &[
                "list_files",
                "read_file",
                "cms_recall",
                "cms_recent",
                "cms_search_chatgpt_archive",
                "cms_prepare_context",
                "spawn_subagent",
                "save_session",
                "audit_log",
            ],
        ),
        template(
            "engineer",
            "Engineer",
            "Implements scoped code changes with verification.",
            "You are an engineering specialist. Read the surrounding code before changing it, make minimal coherent edits, preserve existing behavior unless intentionally changed, and verify with focused tests.",
            &[
                "list_files",
                "read_file",
                "write_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_search_chatgpt_archive",
                "cms_remember",
                "cms_prepare_context",
                "audit_log",
            ],
        ),
        template(
            "coder",
            "Coder",
            "Focuses on implementation details and local patches.",
            "You are a coding specialist. Implement the requested behavior directly, keep patches small, follow local style, and report the exact verification performed.",
            &[
                "list_files",
                "read_file",
                "write_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_search_chatgpt_archive",
                "cms_remember",
            ],
        ),
        template(
            "tester",
            "Tester",
            "Designs and runs verification for changed behavior.",
            "You are a testing specialist. Identify behavioral risk, add or run targeted tests, explain failures in terms of expected versus actual behavior, and avoid unrelated rewrites.",
            &[
                "list_files",
                "read_file",
                "write_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_search_chatgpt_archive",
                "cms_remember",
                "audit_log",
            ],
        ),
        template_with_skills(
            "agent-red",
            "Agent Red",
            "Security-oriented review and adversarial analysis with delegated reconnaissance, risk gating, and evidence-backed mitigation planning.",
            "You are Agent Red, a security specialist for authorized defensive review. Focus on abuse cases, privilege boundaries, secret handling, prompt/tool injection paths, unsafe execution, supply-chain risk, and concrete mitigations. Work evidence-first: inspect relevant files, tests, traces, memories, and tool outputs before making security claims. Use bounded subagents for independent reconnaissance or test planning when the review is broad. Use CMS context tools only for non-secret project memory and never request, expose, transform, or store plaintext secrets. Treat offensive techniques as analysis context only; do not provide persistence, stealth, credential theft, exploitation deployment, or unauthorized access guidance.",
            &[
                "list_files",
                "read_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_recent",
                "cms_search_chatgpt_archive",
                "cms_remember",
                "cms_prepare_context",
                "cms_prepare_model_request",
                "spawn_subagent",
                "audit_log",
            ],
            &[
                "repo-orientation",
                "code-review",
                "test-repair",
                "risk-check",
            ],
        ),
    ]
}

fn template(
    mode: &str,
    display_name: &str,
    description: &str,
    system_prompt: &str,
    enabled_tools: &[&str],
) -> AgentTemplate {
    template_with_skills(
        mode,
        display_name,
        description,
        system_prompt,
        enabled_tools,
        &[],
    )
}

fn template_with_skills(
    mode: &str,
    display_name: &str,
    description: &str,
    system_prompt: &str,
    enabled_tools: &[&str],
    enabled_skills: &[&str],
) -> AgentTemplate {
    AgentTemplate {
        mode: mode.to_string(),
        display_name: display_name.to_string(),
        description: description.to_string(),
        system_prompt: system_prompt.to_string(),
        enabled_tools: enabled_tools.iter().map(|tool| tool.to_string()).collect(),
        enabled_skills: enabled_skills
            .iter()
            .map(|skill| skill.to_string())
            .collect(),
        usrl_contracts: Vec::new(),
        memory_policy: "agent-scoped".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clean_list_splits_commas_dedupes_and_drops_empty_items() {
        assert_eq!(
            clean_list(vec![
                "read_file, run_tests".to_string(),
                "read_file".to_string(),
                "".to_string()
            ]),
            vec!["read_file".to_string(), "run_tests".to_string()]
        );
    }

    #[test]
    fn none_marker_clears_dash_and_clear() {
        assert_eq!(none_marker("-".to_string()), None);
        assert_eq!(none_marker("clear".to_string()), None);
        assert_eq!(
            none_marker("openai".to_string()),
            Some("openai".to_string())
        );
    }

    #[test]
    fn cli_global_workspace_and_scope_workspace_scope_do_not_collide() {
        let cli = Cli::parse_from([
            "vegvisir-agent-admin",
            "--workspace",
            "/tmp/workspace",
            "scope",
            "engineer",
            "--workspace-scope",
            "repo",
        ]);
        assert_eq!(cli.workspace, Some(PathBuf::from("/tmp/workspace")));
        match cli.command {
            Some(Command::Scope(args)) => {
                assert_eq!(args.workspace_scope.as_deref(), Some("repo"));
            }
            _ => panic!("expected scope command"),
        }
    }

    #[test]
    fn template_profile_carries_template_permissions() -> anyhow::Result<()> {
        let profile = profile_from_template("engineer", "build-engineer", Some("Build Engineer"))?;
        assert_eq!(profile.id, "build-engineer");
        assert_eq!(profile.mode, "engineer");
        assert!(profile.enabled_tools.contains(&"run_tests".to_string()));
        assert_eq!(profile.memory_policy, "agent-scoped");
        Ok(())
    }

    #[test]
    fn registry_create_set_and_doctor_flow() -> anyhow::Result<()> {
        let tmp = tempdir()?;
        let admin = AgentRegistryAdmin::new(tmp.path().join("data"), tmp.path().join("workspace"))?;
        admin.create_template(
            CreateTemplateArgs {
                mode: "tester".to_string(),
                id: "qa".to_string(),
                name: Some("QA".to_string()),
                description: None,
                force: false,
            },
            true,
        )?;
        admin.add_to_list("qa", ListField::Tools, "spawn_subagent".to_string(), true)?;
        admin.provider("qa", "openai-sso".to_string(), true)?;
        let profile = admin.store.load("qa")?;
        assert_eq!(profile.display_name, "QA");
        assert_eq!(profile.current_provider.as_deref(), Some("openai-sso"));
        assert!(
            profile
                .enabled_tools
                .contains(&"spawn_subagent".to_string())
        );
        Ok(())
    }

    #[test]
    fn validate_status_history_and_register_flow() -> anyhow::Result<()> {
        let tmp = tempdir()?;
        let admin = AgentRegistryAdmin::new(tmp.path().join("data"), tmp.path().join("workspace"))?;

        admin.register(
            RegisterArgs {
                builtins_only: true,
                skiller_only: false,
                dry_run: false,
            },
            true,
        )?;
        let engineer = admin.store.load("engineer")?;
        assert_eq!(engineer.mode, "engineer");

        let report = admin.validate_profile(&engineer)?;
        assert!(
            report.errors.is_empty(),
            "unexpected validation errors: {}",
            serde_json::to_string(&report.errors)?
        );

        admin.status("engineer", "active".to_string(), true)?;
        admin.scope(
            ScopeArgs {
                id: "engineer".to_string(),
                primary: Some("implementation".to_string()),
                secondary: vec!["rust".to_string(), "tests".to_string()],
                workspace_scope: Some("repo".to_string()),
                file_scope: vec!["vegvisir/src".to_string()],
            },
            true,
        )?;
        admin.tags(
            "engineer",
            vec!["runtime".to_string(), "coding".to_string()],
            true,
        )?;
        admin.budget(
            BudgetArgs {
                id: "engineer".to_string(),
                max_steps: Some(8),
                max_tool_calls: Some(20),
                max_read_bytes: Some(65536),
                max_output_bytes: Some(16384),
                allowed_tools: vec!["read_file".to_string(), "run_tests".to_string()],
                notes: Some("focused engineering work".to_string()),
                clear: false,
            },
            true,
        )?;

        let profile = admin.store.load("engineer")?;
        assert_eq!(
            profile.metadata.get("status").and_then(Value::as_str),
            Some("active")
        );
        assert_eq!(
            profile
                .metadata
                .get("primary_scope")
                .and_then(Value::as_str),
            Some("implementation")
        );
        assert!(profile.metadata.get("default_work_budget").is_some());
        let history = admin.load_history()?;
        assert!(history.iter().any(|event| event.agent_id == "engineer"));
        Ok(())
    }
}
