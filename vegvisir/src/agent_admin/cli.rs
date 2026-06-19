use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::memory::default_vegvisir_data_root;

use super::{
    join_required,
    registry::{AgentRegistryAdmin, ListField},
};

#[derive(Parser)]
#[command(
    name = "vegvisir-agent-admin",
    bin_name = "vegvisir-agent-admin",
    about = "Standalone Vegvisir agent registry administration tool"
)]
pub(super) struct Cli {
    /// Vegvisir data root. Defaults to VEGVISIR_HOME, XDG_DATA_HOME/vegvisir, or ~/.local/share/vegvisir.
    #[arg(long, global = true)]
    pub(super) data_root: Option<PathBuf>,
    /// Print machine-readable JSON where supported.
    #[arg(long, global = true)]
    pub(super) json: bool,
    /// Workspace used for workspace-local skills and Skiller agent-pack discovery.
    #[arg(long, global = true)]
    pub(super) workspace: Option<PathBuf>,
    #[command(subcommand)]
    pub(super) command: Option<Command>,
}

#[derive(Subcommand)]
pub(super) enum Command {
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
    Create(Box<CreateArgs>),
    /// Create a new profile from a built-in template.
    #[command(name = "create-template", alias = "from-template")]
    CreateTemplate(CreateTemplateArgs),
    /// Design a profile with one command, including permissions and defaults.
    Design(DesignArgs),
    /// Update fields on an existing agent profile.
    Set(Box<SetArgs>),
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
    /// Launch the full-screen agent registry browser/editor.
    Tui,
}

#[derive(Args, Default)]
pub(super) struct ListArgs {
    /// Include prompt and metadata summary in text output.
    #[arg(long)]
    pub(super) long: bool,
    /// Filter by mode.
    #[arg(long)]
    pub(super) mode: Option<String>,
}

#[derive(Args, Default)]
pub(super) struct RegisterArgs {
    /// Register only built-in templates.
    #[arg(long)]
    pub(super) builtins_only: bool,
    /// Register only Skiller agent-pack/proposal artifacts.
    #[arg(long)]
    pub(super) skiller_only: bool,
    /// Report what would be registered without writing profiles.
    #[arg(long)]
    pub(super) dry_run: bool,
}

#[derive(Args)]
pub(super) struct ScopeArgs {
    pub(super) id: String,
    /// Primary scope/domain for the agent.
    #[arg(long)]
    pub(super) primary: Option<String>,
    /// Comma-separated secondary scopes.
    #[arg(long, value_delimiter = ',')]
    pub(super) secondary: Vec<String>,
    /// Workspace or repository scope label/path for this agent profile.
    #[arg(long = "workspace-scope")]
    pub(super) workspace_scope: Option<String>,
    /// Comma-separated file-scope hints.
    #[arg(long, value_delimiter = ',')]
    pub(super) file_scope: Vec<String>,
}

#[derive(Args)]
pub(super) struct BudgetArgs {
    pub(super) id: String,
    #[arg(long)]
    pub(super) max_steps: Option<u64>,
    #[arg(long)]
    pub(super) max_tool_calls: Option<u64>,
    #[arg(long)]
    pub(super) max_read_bytes: Option<u64>,
    #[arg(long)]
    pub(super) max_output_bytes: Option<u64>,
    #[arg(long, value_delimiter = ',')]
    pub(super) allowed_tools: Vec<String>,
    #[arg(long)]
    pub(super) notes: Option<String>,
    /// Clear the stored default work budget.
    #[arg(long)]
    pub(super) clear: bool,
}

#[derive(Args)]
pub(super) struct CreateArgs {
    pub(super) id: String,
    /// Start from a built-in template/mode before applying other options.
    #[arg(long)]
    pub(super) template: Option<String>,
    /// Agent mode, e.g. engineer, planner, tester, skiller, custom.
    #[arg(long, default_value = "custom")]
    pub(super) mode: String,
    /// Display name. Defaults to the normalized id or template display name.
    #[arg(long)]
    pub(super) name: Option<String>,
    /// Short description.
    #[arg(long)]
    pub(super) description: Option<String>,
    /// System prompt text. Use --prompt-file for long prompts.
    #[arg(long, conflicts_with = "prompt_file")]
    pub(super) prompt: Option<String>,
    /// File containing the system prompt.
    #[arg(long)]
    pub(super) prompt_file: Option<PathBuf>,
    /// Default provider for this agent.
    #[arg(long)]
    pub(super) provider: Option<String>,
    /// Default model for this agent.
    #[arg(long)]
    pub(super) model: Option<String>,
    /// Comma-separated enabled tool names. Use '*' only when intentionally unrestricted.
    #[arg(long, value_delimiter = ',')]
    pub(super) tools: Vec<String>,
    /// Append tools to the template/default list instead of replacing it.
    #[arg(long)]
    pub(super) add_tools: bool,
    /// Comma-separated enabled skill names.
    #[arg(long, value_delimiter = ',')]
    pub(super) skills: Vec<String>,
    /// Append skills to the template/default list instead of replacing it.
    #[arg(long)]
    pub(super) add_skills: bool,
    /// Comma-separated enabled MCP server ids.
    #[arg(long, value_delimiter = ',')]
    pub(super) mcp: Vec<String>,
    /// Comma-separated USRL contract refs.
    #[arg(long, value_delimiter = ',')]
    pub(super) usrl: Vec<String>,
    /// Agent memory policy label.
    #[arg(long)]
    pub(super) memory_policy: Option<String>,
    /// Overwrite an existing profile.
    #[arg(long)]
    pub(super) force: bool,
}

#[derive(Args)]
pub(super) struct CreateTemplateArgs {
    pub(super) mode: String,
    pub(super) id: String,
    /// Display name override.
    #[arg(long)]
    pub(super) name: Option<String>,
    /// Description override.
    #[arg(long)]
    pub(super) description: Option<String>,
    /// Overwrite an existing profile.
    #[arg(long)]
    pub(super) force: bool,
}

#[derive(Args)]
pub(super) struct DesignArgs {
    pub(super) id: String,
    /// Agent mode/template.
    #[arg(long, default_value = "custom")]
    pub(super) mode: String,
    /// Display name.
    #[arg(long)]
    pub(super) name: String,
    /// System prompt text. Use --prompt-file for long prompts.
    #[arg(long, conflicts_with = "prompt_file")]
    pub(super) prompt: Option<String>,
    /// File containing the system prompt.
    #[arg(long)]
    pub(super) prompt_file: Option<PathBuf>,
    #[arg(long)]
    pub(super) description: Option<String>,
    #[arg(long)]
    pub(super) provider: Option<String>,
    #[arg(long)]
    pub(super) model: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub(super) tools: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub(super) skills: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub(super) mcp: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub(super) usrl: Vec<String>,
    #[arg(long)]
    pub(super) memory_policy: Option<String>,
    #[arg(long)]
    pub(super) force: bool,
}

#[derive(Args)]
pub(super) struct SetArgs {
    pub(super) id: String,
    #[arg(long)]
    pub(super) mode: Option<String>,
    #[arg(long)]
    pub(super) name: Option<String>,
    #[arg(long)]
    pub(super) description: Option<String>,
    #[arg(long, conflicts_with = "prompt_file")]
    pub(super) prompt: Option<String>,
    #[arg(long)]
    pub(super) prompt_file: Option<PathBuf>,
    #[arg(long)]
    pub(super) provider: Option<String>,
    #[arg(long)]
    pub(super) model: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub(super) tools: Option<Vec<String>>,
    #[arg(long, value_delimiter = ',')]
    pub(super) add_tools: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub(super) remove_tools: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub(super) skills: Option<Vec<String>>,
    #[arg(long, value_delimiter = ',')]
    pub(super) add_skills: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub(super) remove_skills: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub(super) mcp: Option<Vec<String>>,
    #[arg(long, value_delimiter = ',')]
    pub(super) add_mcp: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub(super) remove_mcp: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub(super) usrl: Option<Vec<String>>,
    #[arg(long, value_delimiter = ',')]
    pub(super) add_usrl: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub(super) remove_usrl: Vec<String>,
    #[arg(long)]
    pub(super) memory_policy: Option<String>,
    #[arg(long)]
    pub(super) cms_user: Option<String>,
    #[arg(long)]
    pub(super) cms_project: Option<String>,
}

#[derive(Args)]
pub(super) struct PromptArgs {
    pub(super) id: String,
    /// Prompt text as remaining positional words.
    pub(super) prompt: Vec<String>,
    /// File containing the system prompt.
    #[arg(long, conflicts_with = "prompt")]
    pub(super) prompt_file: Option<PathBuf>,
}

pub fn run_agent_admin_cli() -> anyhow::Result<()> {
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
        Command::Create(args) => registry.create(*args, cli.json),
        Command::CreateTemplate(args) => registry.create_template(args, cli.json),
        Command::Design(args) => registry.design(args, cli.json),
        Command::Set(args) => registry.set(*args, cli.json),
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
