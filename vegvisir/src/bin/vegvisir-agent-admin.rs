use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::{Value, json};
use vegvisir_rust::{
    core::{AgentProfile, AgentProfileStore, normalize_agent_id},
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
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Print registry paths used by this binary.
    Paths,
    /// Validate and summarize the agent registry.
    Doctor,
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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let data_root = cli.data_root.unwrap_or_else(default_vegvisir_data_root);
    let registry = AgentRegistryAdmin::new(data_root)?;
    match cli.command.unwrap_or(Command::List(ListArgs::default())) {
        Command::Paths => registry.print_paths(cli.json),
        Command::Doctor => registry.doctor(cli.json),
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
    fn new(data_root: PathBuf) -> anyhow::Result<Self> {
        let store = AgentProfileStore::new(data_root.join("agents"))?;
        Ok(Self { data_root, store })
    }

    fn print_paths(&self, json_output: bool) -> anyhow::Result<()> {
        print_json_or_text(
            json_output,
            &json!({
                "data_root": self.data_root,
                "agents_root": self.store.root,
            }),
            || {
                println!("data_root: {}", self.data_root.display());
                println!("agents_root: {}", self.store.root.display());
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
        print_saved(&profile, &saved, json_output)
    }

    fn tui(&self) -> anyhow::Result<()> {
        println!("Vegvisir Agent Admin — interactive registry editor");
        println!("Registry: {}", self.store.root.display());
        println!(
            "Commands: list, templates, show <id>, create <id>, create-template <mode> <id>, delete <id>, doctor, paths, help, quit"
        );
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
                ["help"] => println!(
                    "list\ntemplates [id]\nshow <id>\ncreate <id>\ncreate-template <mode> <id>\ndelete <id>\ndoctor\npaths\nquit\n\nFor full editing, use non-interactive subcommands such as set, prompt, allow-tool, bind-usrl, import, export."
                ),
                ["paths"] => self.print_paths(false)?,
                ["doctor"] => self.doctor(false)?,
                ["templates"] => self.templates(None, false)?,
                ["templates", id] => self.templates(Some(id), false)?,
                ["list"] => self.list(ListArgs::default(), false)?,
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
                ["delete", id] => self.delete_interactive(id)?,
                [unknown, ..] => println!("Unknown command: {unknown}. Type help."),
            }
        }
        Ok(())
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
        let admin = AgentRegistryAdmin::new(tmp.path().to_path_buf())?;
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
}
