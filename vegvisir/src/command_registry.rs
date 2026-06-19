use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::core::{CommandDefinition, ToolDefinition};

#[derive(Clone, Debug, Default)]
pub struct CommandRegistry {
    definitions: BTreeMap<String, CommandDefinition>,
    aliases: BTreeMap<String, String>,
}

impl CommandRegistry {
    pub fn with_defaults() -> Self {
        let mut registry = Self::default();
        for definition in default_command_definitions() {
            registry.register(definition);
        }
        registry
    }

    pub fn register(&mut self, definition: CommandDefinition) {
        for alias in &definition.aliases {
            self.aliases.insert(alias.clone(), definition.name.clone());
        }
        self.definitions.insert(definition.name.clone(), definition);
    }

    pub fn get(&self, name: &str) -> Option<&CommandDefinition> {
        self.definitions.get(&self.canonical(name))
    }

    pub fn all(&self) -> Vec<&CommandDefinition> {
        self.definitions.values().collect()
    }

    pub fn specs(&self) -> Vec<CommandSpec> {
        self.definitions
            .values()
            .map(CommandSpec::from_definition)
            .collect()
    }

    pub fn metadata_dump(&self) -> CommandRegistryDump {
        CommandRegistryDump {
            commands: self.specs(),
        }
    }

    pub fn validate(&self) -> Result<(), RegistryValidationError> {
        validate_command_definitions(self.definitions.values())
    }

    pub fn suggest(&self, prefix: &str) -> Vec<String> {
        let normalized = if prefix.starts_with('/') {
            prefix.to_string()
        } else {
            format!("/{prefix}")
        };
        self.definitions
            .keys()
            .filter(|name| name.starts_with(&normalized))
            .cloned()
            .collect()
    }

    pub fn parse(raw: &str) -> Option<(String, Vec<String>)> {
        Self::default().parse_with_aliases(raw)
    }

    pub fn parse_with_aliases(&self, raw: &str) -> Option<(String, Vec<String>)> {
        let stripped = raw.trim();
        if stripped.is_empty() {
            return None;
        }
        let (token, rest) = split_command(stripped);
        let raw_command = if token.starts_with('/') {
            token.to_string()
        } else {
            format!("/{token}")
        };
        let command = self.canonical(&raw_command);
        Some((command.clone(), command_args(&command, rest)))
    }

    pub fn canonical(&self, name: &str) -> String {
        let normalized = if name.starts_with('/') {
            name.to_string()
        } else {
            format!("/{name}")
        };
        self.aliases.get(&normalized).cloned().unwrap_or(normalized)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandRegistryDump {
    pub commands: Vec<CommandSpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandSpec {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub summary: String,
    pub description: Option<String>,
    pub category: CommandCategory,
    pub availability: CommandAvailability,
    pub safety: CommandSafety,
    pub contexts: Vec<ExecutionContext>,
    pub argument_hint: Option<String>,
    pub supports_noninteractive: bool,
    pub hidden: bool,
    pub source: CommandSource,
}

impl CommandSpec {
    pub fn from_definition(definition: &CommandDefinition) -> Self {
        let category = infer_command_category(&definition.name);
        let safety = infer_command_safety(&definition.name);
        let contexts = infer_command_contexts(&definition.name, &category, &safety);
        let availability = if definition.delegates_to_agent {
            CommandAvailability::ModelInvocable
        } else {
            CommandAvailability::UserInvocable
        };
        Self {
            id: command_id(&definition.name),
            name: definition.name.clone(),
            aliases: definition.aliases.clone(),
            summary: definition.description.clone(),
            description: Some(definition.description.clone()),
            category,
            availability,
            safety,
            contexts,
            argument_hint: argument_hint_from_usage(&definition.usage),
            supports_noninteractive: supports_noninteractive(&definition.name),
            hidden: false,
            source: CommandSource::Builtin,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CommandCategory {
    CoreSession,
    Workspace,
    MemoryContext,
    ModelProvider,
    AgentsTasks,
    Skills,
    SecurityPermissions,
    Diagnostics,
    Media,
    Configuration,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandAvailability {
    UserInvocable,
    ModelInvocable,
    Internal,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandSafety {
    ReadOnly,
    SessionMutation,
    WorkspaceMutation,
    ExternalEffect,
    Destructive,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionContext {
    LocalCli,
    Tui,
    Api,
    BackgroundWorker,
    Subagent,
    RemoteBridge,
    Mcp,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandSource {
    Builtin,
    Filesystem,
    Skill,
    Mcp,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolRegistryDump {
    pub tools: Vec<ToolSpec>,
}

#[derive(Clone, Debug, Default)]
pub struct ToolRegistry {
    specs: BTreeMap<String, ToolSpec>,
}

impl ToolRegistry {
    pub fn from_definitions(definitions: impl IntoIterator<Item = ToolDefinition>) -> Self {
        let mut registry = Self::default();
        for definition in definitions {
            registry.register(ToolSpec::from_definition(&definition));
        }
        registry
    }

    pub fn register(&mut self, spec: ToolSpec) {
        self.specs.insert(spec.id.clone(), spec);
    }

    pub fn get(&self, id: &str) -> Option<&ToolSpec> {
        self.specs.get(id)
    }

    pub fn all(&self) -> Vec<&ToolSpec> {
        self.specs.values().collect()
    }

    pub fn metadata_dump(&self) -> ToolRegistryDump {
        ToolRegistryDump {
            tools: self.specs.values().cloned().collect(),
        }
    }

    pub fn validate(&self) -> Result<(), RegistryValidationError> {
        let mut errors = Vec::new();
        for spec in self.specs.values() {
            if spec.id.trim().is_empty() {
                errors.push("tool id must not be empty".to_string());
            }
            if spec.display_name.trim().is_empty() {
                errors.push(format!("tool {} display_name must not be empty", spec.id));
            }
            if spec.contexts.is_empty() {
                errors.push(format!(
                    "tool {} must declare at least one context",
                    spec.id
                ));
            }
            if spec.safety.read_only && spec.safety.destructive {
                errors.push(format!(
                    "tool {} cannot be both read_only and destructive",
                    spec.id
                ));
            }
            if spec.safety.requires_hbse
                && spec.contexts.contains(&ExecutionContext::RemoteBridge)
                && !spec.safety.transcript_visibility.redacts_secrets()
            {
                errors.push(format!(
                    "tool {} requires HBSE but does not declare secret-redacting transcript visibility",
                    spec.id
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(RegistryValidationError { errors })
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolSpec {
    pub id: String,
    pub display_name: String,
    pub summary: String,
    pub read_only: bool,
    pub destructive: bool,
    pub concurrency_safe: bool,
    pub requires_user_interaction: bool,
    pub interrupt_behavior: InterruptBehavior,
    pub approval_category: ApprovalCategory,
    pub transcript_visibility: TranscriptVisibility,
    pub contexts: Vec<ExecutionContext>,
    pub safety: ToolSafety,
}

impl ToolSpec {
    pub fn from_definition(definition: &ToolDefinition) -> Self {
        let read_only = !definition.risky;
        let destructive = matches!(definition.name.as_str(), "write_file" | "run_command");
        let approval_category = if destructive || definition.risky {
            ApprovalCategory::RiskyTool
        } else {
            ApprovalCategory::None
        };
        let transcript_visibility = if definition.category == "memory" {
            TranscriptVisibility::RedactedArguments
        } else {
            TranscriptVisibility::Visible
        };
        let safety = ToolSafety {
            read_only,
            destructive,
            requires_hbse: false,
            transcript_visibility: transcript_visibility.clone(),
        };
        Self {
            id: definition.name.clone(),
            display_name: definition.name.clone(),
            summary: definition.description.clone(),
            read_only,
            destructive,
            concurrency_safe: read_only,
            requires_user_interaction: approval_category != ApprovalCategory::None,
            interrupt_behavior: InterruptBehavior::CancelSafe,
            approval_category,
            transcript_visibility,
            contexts: infer_tool_contexts(definition),
            safety,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolSafety {
    pub read_only: bool,
    pub destructive: bool,
    pub requires_hbse: bool,
    pub transcript_visibility: TranscriptVisibility,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InterruptBehavior {
    CancelSafe,
    BestEffort,
    NotInterruptible,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalCategory {
    None,
    RiskyTool,
    Destructive,
    ExternalEffect,
    SecretAccess,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptVisibility {
    Visible,
    RedactedArguments,
    RedactedOutput,
    Hidden,
}

impl TranscriptVisibility {
    fn redacts_secrets(&self) -> bool {
        matches!(
            self,
            TranscriptVisibility::RedactedArguments
                | TranscriptVisibility::RedactedOutput
                | TranscriptVisibility::Hidden
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryValidationError {
    pub errors: Vec<String>,
}

impl std::fmt::Display for RegistryValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "registry validation failed: {}",
            self.errors.join("; ")
        )
    }
}

impl std::error::Error for RegistryValidationError {}

pub fn validate_default_command_definitions() -> Result<(), RegistryValidationError> {
    validate_command_definitions(default_command_definitions().iter())
}

fn validate_command_definitions<'a>(
    definitions: impl IntoIterator<Item = &'a CommandDefinition>,
) -> Result<(), RegistryValidationError> {
    let definitions = definitions.into_iter().collect::<Vec<_>>();
    let names = definitions
        .iter()
        .map(|definition| definition.name.clone())
        .collect::<BTreeSet<_>>();
    let mut seen_names = BTreeSet::new();
    let mut seen_aliases: BTreeMap<String, String> = BTreeMap::new();
    let mut errors = Vec::new();
    for definition in definitions {
        if !definition.name.starts_with('/') {
            errors.push(format!("command {} must start with '/'", definition.name));
        }
        if !seen_names.insert(definition.name.clone()) {
            errors.push(format!("duplicate command name {}", definition.name));
        }
        let spec = CommandSpec::from_definition(definition);
        if spec.contexts.is_empty() {
            errors.push(format!(
                "command {} must declare at least one execution context",
                definition.name
            ));
        }
        for alias in &definition.aliases {
            if !alias.starts_with('/') {
                errors.push(format!(
                    "alias {} for command {} must start with '/'",
                    alias, definition.name
                ));
            }
            if names.contains(alias) {
                errors.push(format!(
                    "alias {} for command {} conflicts with command name",
                    alias, definition.name
                ));
            }
            if let Some(owner) = seen_aliases.insert(alias.clone(), definition.name.clone()) {
                errors.push(format!(
                    "alias {} is shared by commands {} and {}",
                    alias, owner, definition.name
                ));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(RegistryValidationError { errors })
    }
}

pub fn default_command_definitions() -> Vec<CommandDefinition> {
    vec![
        cmd("/new", "start a new session", "/new [name]", &[]),
        cmd("/sessions", "list saved sessions", "/sessions", &[]),
        cmd("/load", "load a saved session", "/load <session-id>", &[]),
        cmd(
            "/workspace",
            "show or set active workspace path",
            "/workspace [path]",
            &["/cwd"],
        ),
        cmd(
            "/projects",
            "list or switch saved project workspaces",
            "/projects [list|use <path-or-alias>|name <alias> [path]|forget <alias>]",
            &["/project"],
        ),
        cmd("/reset", "reset current conversation state", "/reset", &[]),
        cmd("/clear", "clear the screen", "/clear", &[]),
        cmd("/redraw", "force UI repaint", "/redraw", &[]),
        cmd(
            "/cancel",
            "cancel an in-flight model response",
            "/cancel",
            &["/stop"],
        ),
        cmd(
            "/turn-repair",
            "detect and revive a stuck/dead model turn",
            "/turn-repair [force]",
            &["/repair-turn", "/revive-turn"],
        ),
        cmd(
            "/recover",
            "recover from a stuck turn or inspect latest run replay plan",
            "/recover [turn|force|last]",
            &[],
        ),
        cmd(
            "/auto",
            "control prompt-contract autonomous working mode",
            "/auto [status|on|off|level <0-6>]",
            &["/autonomous"],
        ),
        cmd(
            "/autonomy",
            "control deterministic TUI autonomous run mode",
            "/autonomy [on|off|status|stop|validate [plan]|resume <plan>|max-steps <n>|max-attempts <n>]",
            &[],
        ),
        cmd("/history", "show conversation history", "/history", &[]),
        cmd(
            "/status",
            "show session token counts and telemetry",
            "/status",
            &["/session-status", "/telemetry"],
        ),
        cmd(
            "/diff",
            "show the current workspace git diff; supports delta and difftastic when installed",
            "/diff [semantic|difftastic|delta|unified] [--staged|--cached|--stat] [path]",
            &[],
        ),
        cmd(
            "/runs",
            "list, inspect, export, diff, or replay-plan run artifact bundles",
            "/runs [list|show|open|export|diff|replay-plan] <run-id|latest>",
            &["/run-artifacts"],
        ),
        cmd("/save", "save the current session", "/save", &[]),
        cmd("/retry", "retry last assistant response", "/retry", &[]),
        cmd("/undo", "remove last exchange", "/undo", &[]),
        cmd("/title", "set session title", "/title [name]", &[]),
        cmd("/branch", "branch current session", "/branch [name]", &[]),
        cmd("/fork", "fork current session", "/fork", &["/clone"]),
        cmd(
            "/compress",
            "summarize/compress current context",
            "/compress [topic]",
            &[],
        ),
        cmd(
            "/system",
            "view or edit harness system prompt",
            "/system [show|print|view|set|append|clear|default] [text]",
            &[],
        ),
        cmd(
            "/system-prompt",
            "print active harness system prompt",
            "/system-prompt",
            &[],
        ),
        cmd(
            "/agent",
            "create, select, and inspect persistent custom agents",
            "/agent [list|templates|create|design|create-template|clone|import|export|use|show|delete|mode|provider|model|prompt|describe|allow-tool|revoke-tool|enable-skill|disable-skill|bind-usrl|unbind-usrl|allow-mcp|revoke-mcp|clear] [id]",
            &[],
        ),
        cmd(
            "/agents",
            "inspect agents and configure subagent concurrency",
            "/agents [max=<n>|max <n>|list]",
            &[],
        ),
        cmd(
            "/attach",
            "attach file or image to next message",
            "/attach [path|clear]",
            &[],
        ),
        cmd(
            "/ka",
            "list, show, set, create, import, or edit the active communication ka/persona",
            "/ka [list|show [id]|set <id>|create <id> [name]|import <path>|edit <id>|clear|default]",
            &["/persona", "/soul"],
        ),
        cmd(
            "/profile",
            "show or update the local user profile",
            "/profile [show|path|init|help|set <field> <value>|add <spoken_languages|coding_languages> <value>|remove <spoken_languages|coding_languages> <value>|clear]",
            &["/user"],
        ),
        cmd(
            "/speech",
            "transcribe audio into the input buffer using OpenAI/HBSE speech-to-text",
            "/speech status|transcribe <audio-file>|ptt|ptt-key <key>|ptt-seconds <n>",
            &["/stt"],
        ),
        cmd(
            "/tts",
            "synthesize text to speech using OpenAI/HBSE and play or save MP3 audio",
            "/tts [--voice <voice>] [--out <path>] [--no-play] <text>",
            &["/speak"],
        ),
        cmd(
            "/summary",
            "generate a structured session summary; can save to file or CMS memory",
            "/summary [--handoff] [--save] [--file <path>] [--memory] [--global] [--since-start|--since-last]",
            &["/session-summary"],
        ),
        cmd(
            "/handoff",
            "generate an agent handoff summary for resuming work",
            "/handoff [--save] [--file <path>] [--memory] [--global]",
            &[],
        ),
        cmd(
            "/help",
            "show command reference",
            "/help [--json] [--context <name>]",
            &[],
        ),
        cmd(
            "/commands",
            "list command registry metadata",
            "/commands [--json] [--context <local-cli|tui|api|subagent|remote-bridge>]",
            &[],
        ),
        cmd(
            "/tools",
            "show available tools",
            "/tools [status|explain <tool>|allow-risky|deny-risky|require-approval|no-approval|max-rounds <rounds>|max-rounds default]",
            &[],
        ),
        cmd(
            "/tool-limit",
            "show or set max tool-call rounds per model turn",
            "/tool-limit [show|<rounds>|default]",
            &["/tool-rounds", "/max-tools"],
        ),
        cmd(
            "/approvals",
            "inspect and manage pending risky tool approvals",
            "/approvals [list|show|explain <id>|approve <id>|session <id>|edit <id> <json-args>|deny <id>]",
            &["/approval"],
        ),
        cmd(
            "/permissions",
            "explain active permission policy and guardrail decisions",
            "/permissions [status|explain <tool-name> [json-args]|pending [approval-id]|--json]",
            &["/permission"],
        ),
        cmd(
            "/tasks",
            "list and inspect session task manager records",
            "/tasks [list|show <task-id>|events|--json]",
            &["/jobs"],
        ),
        cmd(
            "/skills",
            "show, compile, route, or load skills",
            "/skills [status|audit|trust|provenance|registry status|compile|route|load|eval|forge|patch|curate|detect|trace|promote|archive]",
            &[],
        ),
        cmd(
            "/recall",
            "retrieve memories from CMS-v2",
            "/recall [--limit N] [--global] <query>",
            &[],
        ),
        cmd(
            "/memory",
            "inspect CMS-v2 memory scope, recent memories, or import ChatGPT exports",
            "/memory [status|recent|used-this-turn|writes-this-session|why <id>|diff <a> <b>|quarantine <id>|forget <id>|export [--global] [--out file]|import-chatgpt <path>|search-chatgpt <query>]",
            &["/memories"],
        ),
        cmd(
            "/remember",
            "store a durable CMS-v2 memory",
            "/remember <title> | <content>",
            &[],
        ),
        cmd(
            "/context",
            "prepare ECM context for a message",
            "/context [explain|budget|sources] <message> | /context last | /context diff-last",
            &[],
        ),
        cmd(
            "/model-request",
            "prepare provider-cacheable CMS-v2 model request",
            "/model-request <message>",
            &[],
        ),
        cmd("/models", "show available models", "/models", &[]),
        cmd(
            "/model",
            "select active model",
            "/model [name|compare <model...>]",
            &[],
        ),
        cmd(
            "/effort",
            "show or set model reasoning effort",
            "/effort [minimal|low|medium|high|default]",
            &["/reasoning", "/reasoning-effort"],
        ),
        cmd(
            "/fast",
            "enable or disable fast mode for supported OpenAI/Anthropic models",
            "/fast [on|off|status]",
            &[],
        ),
        cmd(
            "/provider",
            "select active provider",
            "/provider [name|diagnose [provider]]",
            &[],
        ),
        cmd("/providers", "show provider auth status", "/providers", &[]),
        cmd("/auth", "show provider auth setup", "/auth [provider]", &[]),
        cmd(
            "/verify",
            "run production readiness checks",
            "/verify [all|auth|mcp|agent|memory|runtime|evals]",
            &[],
        ),
        cmd(
            "/eval",
            "run deterministic harness evaluation checks",
            "/eval [all|memory|security|tools|injection|golden|file <path>]",
            &["/evals"],
        ),
        cmd(
            "/trace",
            "show recent harness trace events",
            "/trace [--limit N] [--json]",
            &["/traces"],
        ),
        cmd(
            "/work",
            "open recent work and tool activity view",
            "/work [--limit N]",
            &["/activity", "/timeline"],
        ),
        cmd(
            "/subagents",
            "inspect durable subagent task records",
            "/subagents [list|show|timeline|diff|events|artifacts|ownership|cancel|policy|max|config]",
            &["/workers"],
        ),
        cmd(
            "/mcp",
            "show configured MCP servers and tools",
            "/mcp [list|status|auth-map|show|tools|reload|add-http|add-http-service|add-stdio|add-tool|remove-tool|remove|enable|disable]",
            &[],
        ),
        cmd(
            "/hbse",
            "show HBSE secret reference setup commands",
            "/hbse [status|usage-this-session|usage-this-run|provider <id>|mcp <server> [url]|service <name>|service add|show|enable|disable|remove|services]",
            &[],
        ),
        cmd(
            "/config",
            "show or update local Vegvisir configuration",
            "/config [status|user <id>|path]",
            &[],
        ),
        cmd("/exit", "exit application", "/exit", &["/quit"]),
    ]
}

fn cmd(name: &str, description: &str, usage: &str, aliases: &[&str]) -> CommandDefinition {
    CommandDefinition {
        name: name.to_string(),
        description: description.to_string(),
        usage: usage.to_string(),
        aliases: aliases.iter().map(|alias| alias.to_string()).collect(),
        delegates_to_agent: false,
    }
}

fn command_id(name: &str) -> String {
    name.trim_start_matches('/').replace('-', "_")
}

fn argument_hint_from_usage(usage: &str) -> Option<String> {
    usage.split_once(char::is_whitespace).and_then(|(_, hint)| {
        let hint = hint.trim();
        (!hint.is_empty()).then(|| hint.to_string())
    })
}

fn supports_noninteractive(name: &str) -> bool {
    matches!(
        name,
        "/help"
            | "/commands"
            | "/tools"
            | "/status"
            | "/providers"
            | "/models"
            | "/model"
            | "/provider"
            | "/verify"
            | "/eval"
            | "/trace"
            | "/permissions"
            | "/tasks"
            | "/memory"
            | "/context"
            | "/model-request"
            | "/skills"
            | "/subagents"
            | "/mcp"
            | "/hbse"
            | "/config"
            | "/runs"
            | "/diff"
    )
}

fn infer_command_category(name: &str) -> CommandCategory {
    match name {
        "/workspace" | "/projects" | "/attach" | "/diff" => CommandCategory::Workspace,
        "/recall" | "/memory" | "/remember" | "/context" | "/model-request" | "/compress" => {
            CommandCategory::MemoryContext
        }
        "/models" | "/model" | "/effort" | "/fast" | "/provider" | "/providers" | "/auth" => {
            CommandCategory::ModelProvider
        }
        "/agent" | "/agents" | "/subagents" | "/tasks" | "/work" | "/auto" | "/autonomy" => {
            CommandCategory::AgentsTasks
        }
        "/skills" => CommandCategory::Skills,
        "/tools" | "/approvals" | "/permissions" | "/hbse" | "/mcp" => {
            CommandCategory::SecurityPermissions
        }
        "/status" | "/verify" | "/eval" | "/trace" | "/runs" | "/recover" | "/turn-repair" => {
            CommandCategory::Diagnostics
        }
        "/speech" | "/tts" => CommandCategory::Media,
        "/system" | "/system-prompt" | "/profile" | "/ka" | "/config" => {
            CommandCategory::Configuration
        }
        _ => CommandCategory::CoreSession,
    }
}

fn infer_command_safety(name: &str) -> CommandSafety {
    match name {
        "/exit" | "/quit" | "/reset" | "/clear" | "/cancel" | "/stop" => {
            CommandSafety::SessionMutation
        }
        "/workspace" | "/projects" | "/system" | "/agent" | "/agents" | "/ka" | "/profile"
        | "/model" | "/provider" | "/effort" | "/fast" | "/tools" | "/approvals" | "/skills"
        | "/memory" | "/remember" | "/hbse" | "/mcp" | "/config" => CommandSafety::SessionMutation,
        "/attach" | "/speech" | "/tts" => CommandSafety::ExternalEffect,
        "/diff" | "/eval" | "/verify" => CommandSafety::ReadOnly,
        _ => CommandSafety::ReadOnly,
    }
}

fn infer_command_contexts(
    name: &str,
    category: &CommandCategory,
    safety: &CommandSafety,
) -> Vec<ExecutionContext> {
    let mut contexts = vec![ExecutionContext::LocalCli, ExecutionContext::Tui];
    if supports_noninteractive(name) {
        contexts.push(ExecutionContext::Api);
    }
    if matches!(safety, CommandSafety::ReadOnly)
        && !matches!(
            category,
            CommandCategory::CoreSession | CommandCategory::Media
        )
    {
        contexts.push(ExecutionContext::Subagent);
    }
    if supports_noninteractive(name)
        && matches!(
            safety,
            CommandSafety::ReadOnly | CommandSafety::SessionMutation
        )
        && !matches!(category, CommandCategory::Media)
    {
        contexts.push(ExecutionContext::RemoteBridge);
    }
    contexts.sort();
    contexts.dedup();
    contexts
}

fn infer_tool_contexts(definition: &ToolDefinition) -> Vec<ExecutionContext> {
    let mut contexts = vec![
        ExecutionContext::LocalCli,
        ExecutionContext::Tui,
        ExecutionContext::Api,
    ];
    if !definition.risky {
        contexts.push(ExecutionContext::Subagent);
        contexts.push(ExecutionContext::RemoteBridge);
    }
    if definition.category == "memory" || definition.category == "context" {
        contexts.push(ExecutionContext::Subagent);
    }
    contexts.sort();
    contexts.dedup();
    contexts
}

fn split_command(raw: &str) -> (&str, &str) {
    raw.split_once(char::is_whitespace)
        .map(|(command, rest)| (command, rest.trim()))
        .unwrap_or((raw, ""))
}

fn command_args(command_name: &str, rest: &str) -> Vec<String> {
    if rest.is_empty() {
        return Vec::new();
    }
    if command_name == "/profile" {
        let mut parts = rest.splitn(3, char::is_whitespace);
        let first = parts.next().unwrap_or("");
        if matches!(first, "set" | "add" | "remove") {
            let field = parts.next().unwrap_or("").trim();
            let value = parts.next().unwrap_or("").trim();
            return [first, field, value]
                .into_iter()
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect();
        }
        return rest.split_whitespace().map(str::to_string).collect();
    }
    if command_name == "/system" {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let first = parts.next().unwrap_or("");
        let second = parts.next().unwrap_or("").trim();
        if matches!(first, "set" | "append") && !second.is_empty() {
            return vec![first.to_string(), second.to_string()];
        }
        return vec![rest.to_string()];
    }
    if command_name == "/approvals" {
        let mut parts = rest.splitn(3, char::is_whitespace);
        let first = parts.next().unwrap_or("");
        if first == "edit" {
            let id = parts.next().unwrap_or("").trim();
            let json = parts.next().unwrap_or("").trim();
            return [first, id, json]
                .into_iter()
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect();
        }
    }
    rest.split_whitespace().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::default_tool_definitions;

    #[test]
    fn default_command_registry_validates_unique_names_and_aliases() {
        validate_default_command_definitions().expect("default command registry should validate");
        CommandRegistry::with_defaults()
            .validate()
            .expect("registered default commands should validate");
    }

    #[test]
    fn registry_rejects_duplicate_command_names() {
        let duplicate = [
            cmd("/same", "one", "/same", &[]),
            cmd("/same", "two", "/same", &[]),
        ];
        let error = validate_command_definitions(duplicate.iter()).unwrap_err();
        assert!(
            error
                .errors
                .iter()
                .any(|line| line.contains("duplicate command name /same")),
            "unexpected errors: {:#?}",
            error.errors
        );
    }

    #[test]
    fn registry_rejects_alias_conflicting_with_command_name() {
        let definitions = [
            cmd("/agent", "one", "/agent", &[]),
            cmd("/agents", "two", "/agents", &["/agent"]),
        ];
        let error = validate_command_definitions(definitions.iter()).unwrap_err();
        assert!(
            error.errors.iter().any(|line| line
                .contains("alias /agent for command /agents conflicts with command name")),
            "unexpected errors: {:#?}",
            error.errors
        );
    }

    #[test]
    fn command_registry_dump_is_deterministic_and_typed() {
        let registry = CommandRegistry::with_defaults();
        let dump = registry.metadata_dump();
        assert!(
            dump.commands
                .windows(2)
                .all(|pair| pair[0].name <= pair[1].name)
        );
        let help = dump
            .commands
            .iter()
            .find(|command| command.name == "/help")
            .expect("help command has spec");
        assert_eq!(help.category, CommandCategory::CoreSession);
        assert_eq!(help.source, CommandSource::Builtin);
        assert!(help.contexts.contains(&ExecutionContext::Tui));
    }

    #[test]
    fn permissions_command_is_registered_for_discovery() {
        let registry = CommandRegistry::with_defaults();
        let permissions = registry
            .get("/permissions")
            .expect("permissions command is registered");
        assert_eq!(permissions.name, "/permissions");
        assert!(permissions.aliases.contains(&"/permission".to_string()));
        let spec = CommandSpec::from_definition(permissions);
        assert_eq!(spec.category, CommandCategory::SecurityPermissions);
        assert!(spec.supports_noninteractive);
        assert!(spec.contexts.contains(&ExecutionContext::Api));
    }

    #[test]
    fn tasks_command_is_registered_for_discovery() {
        let registry = CommandRegistry::with_defaults();
        let tasks = registry.get("/tasks").expect("tasks command is registered");
        assert_eq!(tasks.name, "/tasks");
        assert!(tasks.aliases.contains(&"/jobs".to_string()));
        let spec = CommandSpec::from_definition(tasks);
        assert_eq!(spec.category, CommandCategory::AgentsTasks);
        assert_eq!(spec.safety, CommandSafety::ReadOnly);
        assert!(spec.supports_noninteractive);
        assert!(spec.contexts.contains(&ExecutionContext::Api));
        assert!(spec.contexts.contains(&ExecutionContext::Subagent));
    }
    #[test]
    fn default_tool_registry_validates_current_tools() -> anyhow::Result<()> {
        let registry = ToolRegistry::from_definitions(default_tool_definitions()?);
        registry.validate()?;
        let read_file = registry
            .get("read_file")
            .expect("read_file tool registered");
        assert!(read_file.read_only);
        assert_eq!(read_file.approval_category, ApprovalCategory::None);
        let write_file = registry
            .get("write_file")
            .expect("write_file tool registered");
        assert!(write_file.destructive);
        assert_eq!(write_file.approval_category, ApprovalCategory::RiskyTool);
        Ok(())
    }

    #[test]
    fn ka_command_aliases_parse_to_canonical_command() {
        let registry = CommandRegistry::with_defaults();
        let (command, args) = registry
            .parse_with_aliases("/persona set chaotic_competent")
            .expect("persona alias should parse");
        assert_eq!(command, "/ka");
        assert_eq!(
            args,
            vec!["set".to_string(), "chaotic_competent".to_string()]
        );
        let (command, _) = registry
            .parse_with_aliases("/soul set chaotic_competent")
            .expect("deprecated soul alias should still parse");
        assert_eq!(command, "/ka");
        let ka = registry.get("/ka").expect("ka command exists");
        assert!(ka.aliases.contains(&"/persona".to_string()));
        assert!(ka.aliases.contains(&"/soul".to_string()));
    }

    #[test]
    fn agent_and_agents_commands_remain_distinct() {
        let registry = CommandRegistry::with_defaults();
        assert_eq!(registry.canonical("/agent"), "/agent");
        assert_eq!(registry.canonical("/agents"), "/agents");
    }

    #[test]
    fn tts_command_alias_parses_to_canonical_command() {
        let registry = CommandRegistry::with_defaults();
        let (command, args) = registry
            .parse_with_aliases("/speak --voice alloy hello world")
            .expect("tts alias should parse");
        assert_eq!(command, "/tts");
        assert_eq!(
            args,
            vec![
                "--voice".to_string(),
                "alloy".to_string(),
                "hello".to_string(),
                "world".to_string()
            ]
        );
        let tts = registry.get("/tts").expect("tts command exists");
        assert!(tts.aliases.contains(&"/speak".to_string()));
    }

    #[test]
    fn speech_command_alias_parses_to_canonical_command() {
        let registry = CommandRegistry::with_defaults();
        let (command, args) = registry
            .parse_with_aliases("/stt transcribe sample.wav")
            .expect("speech alias should parse");
        assert_eq!(command, "/speech");
        assert_eq!(
            args,
            vec!["transcribe".to_string(), "sample.wav".to_string()]
        );
        let speech = registry.get("/speech").expect("speech command exists");
        assert!(speech.aliases.contains(&"/stt".to_string()));
    }
}
