use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand};
use serde_json::Value;
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
    /// List registered agents.
    List,
    /// Show one agent profile.
    Show { id: String },
    /// Create a new agent profile.
    Create(CreateArgs),
    /// Update fields on an existing agent profile.
    Set(SetArgs),
    /// Clone an existing profile to a new id.
    Clone {
        source_id: String,
        new_id: String,
        #[arg(long)]
        name: Option<String>,
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
    Import { path: PathBuf },
    /// Launch a small interactive registry editor shell.
    Tui,
}

#[derive(Args)]
struct CreateArgs {
    id: String,
    /// Agent mode, e.g. engineer, planner, tester, skiller, custom.
    #[arg(long, default_value = "custom")]
    mode: String,
    /// Display name. Defaults to the normalized id.
    #[arg(long)]
    name: Option<String>,
    /// Short description.
    #[arg(long, default_value = "")]
    description: String,
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
    /// Comma-separated enabled skill names.
    #[arg(long, value_delimiter = ',')]
    skills: Vec<String>,
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
    skills: Option<Vec<String>>,
    #[arg(long, value_delimiter = ',')]
    mcp: Option<Vec<String>>,
    #[arg(long, value_delimiter = ',')]
    usrl: Option<Vec<String>>,
    #[arg(long)]
    memory_policy: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let data_root = cli.data_root.unwrap_or_else(default_vegvisir_data_root);
    let registry = AgentRegistryAdmin::new(data_root)?;
    match cli.command.unwrap_or(Command::List) {
        Command::Paths => registry.print_paths(cli.json),
        Command::List => registry.list(cli.json),
        Command::Show { id } => registry.show(&id, cli.json),
        Command::Create(args) => registry.create(args, cli.json),
        Command::Set(args) => registry.set(args, cli.json),
        Command::Clone {
            source_id,
            new_id,
            name,
        } => registry.clone_profile(&source_id, &new_id, name, cli.json),
        Command::Delete { id, yes } => registry.delete(&id, yes, cli.json),
        Command::Export { id, out } => registry.export(&id, out),
        Command::Import { path } => registry.import(&path, cli.json),
        Command::Tui => registry.tui(),
    }
}

struct AgentRegistryAdmin {
    data_root: PathBuf,
    store: AgentProfileStore,
}

impl AgentRegistryAdmin {
    fn new(data_root: PathBuf) -> anyhow::Result<Self> {
        let store = AgentProfileStore::new(data_root.join("agents"))?;
        Ok(Self { data_root, store })
    }

    fn print_paths(&self, json: bool) -> anyhow::Result<()> {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "data_root": self.data_root,
                    "agents_root": self.store.root,
                }))?
            );
        } else {
            println!("data_root: {}", self.data_root.display());
            println!("agents_root: {}", self.store.root.display());
        }
        Ok(())
    }

    fn list(&self, json: bool) -> anyhow::Result<()> {
        let (profiles, warnings) = self.store.list_lossy()?;
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
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

    fn show(&self, id: &str, json: bool) -> anyhow::Result<()> {
        let profile = self.store.load(id)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&profile)?);
        } else {
            print_profile(&profile);
        }
        Ok(())
    }

    fn create(&self, args: CreateArgs, json: bool) -> anyhow::Result<()> {
        let id = normalize_agent_id(&args.id);
        if id.is_empty() {
            bail!("agent id must contain at least one letter or number");
        }
        if self.store.path_for(&id).exists() && !args.force {
            bail!("agent {id} already exists; pass --force to overwrite");
        }
        let prompt = read_prompt(args.prompt, args.prompt_file)?.unwrap_or_else(|| {
            format!(
                "You are {name}, a Vegvisir custom agent. Work evidence-first, preserve user work, follow tool and secret boundaries, and report verification clearly.",
                name = args.name.as_deref().unwrap_or(&id)
            )
        });
        let mut profile = AgentProfile::new(&id, args.name.unwrap_or_else(|| id.clone()), prompt)?;
        profile.mode = normalized_or_default(&args.mode, "custom");
        profile.description = args.description;
        profile.current_provider = args.provider.filter(|value| !value.trim().is_empty());
        profile.current_model = args.model.filter(|value| !value.trim().is_empty());
        profile.enabled_tools = clean_list(args.tools);
        profile.enabled_skills = clean_list(args.skills);
        profile.enabled_mcp_servers = clean_list(args.mcp);
        profile.usrl_contracts = clean_list(args.usrl);
        if let Some(policy) = args.memory_policy.filter(|value| !value.trim().is_empty()) {
            profile.memory_policy = policy;
        }
        profile.metadata = admin_metadata("created");
        let path = self.store.save(&profile)?;
        print_saved(&profile, &path, json)
    }

    fn set(&self, args: SetArgs, json: bool) -> anyhow::Result<()> {
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
        if let Some(skills) = args.skills {
            profile.enabled_skills = clean_list(skills);
        }
        if let Some(mcp) = args.mcp {
            profile.enabled_mcp_servers = clean_list(mcp);
        }
        if let Some(usrl) = args.usrl {
            profile.usrl_contracts = clean_list(usrl);
        }
        if let Some(policy) = args.memory_policy {
            profile.memory_policy = policy;
        }
        profile.updated_at = chrono::Utc::now();
        profile.metadata.insert(
            "last_admin_action".to_string(),
            Value::String("set".to_string()),
        );
        let path = self.store.save(&profile)?;
        print_saved(&profile, &path, json)
    }

    fn clone_profile(
        &self,
        source_id: &str,
        new_id: &str,
        name: Option<String>,
        json: bool,
    ) -> anyhow::Result<()> {
        let mut profile = self.store.load(source_id)?;
        let normalized = normalize_agent_id(new_id);
        if normalized.is_empty() {
            bail!("new agent id must contain at least one letter or number");
        }
        if self.store.path_for(&normalized).exists() {
            bail!("agent {normalized} already exists");
        }
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
        print_saved(&profile, &path, json)
    }

    fn delete(&self, id: &str, yes: bool, json: bool) -> anyhow::Result<()> {
        if !yes {
            bail!("refusing to delete {id} without --yes");
        }
        let path = self.store.delete(id)?;
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
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

    fn import(&self, path: &PathBuf, json: bool) -> anyhow::Result<()> {
        let mut profile: AgentProfile = serde_json::from_str(
            &std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?,
        )?;
        profile.id = normalize_agent_id(&profile.id);
        if profile.id.is_empty() {
            bail!("imported agent id must contain at least one letter or number");
        }
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
        print_saved(&profile, &saved, json)
    }

    fn tui(&self) -> anyhow::Result<()> {
        println!("Vegvisir Agent Admin — interactive registry editor");
        println!("Registry: {}", self.store.root.display());
        println!("Commands: list, show <id>, create <id>, delete <id>, paths, help, quit");
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
                    "list\nshow <id>\ncreate <id>\ndelete <id>\npaths\nquit\n\nFor full editing, use non-interactive subcommands: create, set, clone, import, export."
                ),
                ["paths"] => self.print_paths(false)?,
                ["list"] => self.list(false)?,
                ["show", id] => self.show(id, false)?,
                ["create", id] => self.create_interactive(id)?,
                ["delete", id] => self.delete_interactive(id)?,
                [unknown, ..] => println!("Unknown command: {unknown}. Type help."),
            }
        }
        Ok(())
    }

    fn create_interactive(&self, id: &str) -> anyhow::Result<()> {
        let name = prompt_line("display name", Some(id))?;
        let mode = prompt_line("mode", Some("custom"))?;
        let description = prompt_line("description", Some(""))?;
        println!("Enter system prompt. Finish with a single '.' line.");
        let prompt = read_multiline_prompt()?;
        let args = CreateArgs {
            id: id.to_string(),
            mode,
            name: Some(name),
            description,
            prompt: Some(prompt),
            prompt_file: None,
            provider: None,
            model: None,
            tools: Vec::new(),
            skills: Vec::new(),
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
}

fn print_saved(profile: &AgentProfile, path: &std::path::Path, json: bool) -> anyhow::Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
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
    println!(
        "\n## System prompt\n\n```text\n{}\n```",
        profile.system_prompt
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
    values
        .into_iter()
        .flat_map(|value| value.split(',').map(str::to_string).collect::<Vec<_>>())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_list_splits_commas_and_drops_empty_items() {
        assert_eq!(
            clean_list(vec!["read_file, run_tests".to_string(), "".to_string()]),
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
}
