use std::collections::BTreeMap;

use crate::core::CommandDefinition;

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
            "/auto",
            "control prompt-contract autonomous working mode",
            "/auto [status|on|off]",
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
            "transcribe an audio file into the input buffer using a local Whisper-compatible CLI",
            "/speech transcribe <audio-file>|status",
            &["/stt"],
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
        cmd("/help", "show command reference", "/help", &[]),
        cmd(
            "/tools",
            "show available tools",
            "/tools [status|allow-risky|deny-risky|require-approval|no-approval|max-rounds <rounds>|max-rounds default]",
            &[],
        ),
        cmd(
            "/auto",
            "enable or disable autonomous working mode for unattended project work",
            "/auto [status|on|off]",
            &["/autonomous"],
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
            "/approvals [list|show <id>|approve <id>|session <id>|edit <id> <json-args>|deny <id>]",
            &["/approval"],
        ),
        cmd(
            "/skills",
            "show, compile, route, or load skills",
            "/skills [status|compile|route <query>|load [--tokens N] <query-or-subskill>|eval [target-or-eval]|forge <id> | <title> | <summary> | <body>|patch <id> | <op> | <path> | <value>|curate|detect|trace|promote <id>|archive <id>]",
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
            "/memory [status|recent|import-chatgpt <path>|search-chatgpt <query>] [--global] [--limit N]",
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
            "/context <message>",
            &[],
        ),
        cmd(
            "/model-request",
            "prepare provider-cacheable CMS-v2 model request",
            "/model-request <message>",
            &[],
        ),
        cmd("/models", "show available models", "/models", &[]),
        cmd("/model", "select active model", "/model [name]", &[]),
        cmd(
            "/provider",
            "select active provider",
            "/provider [name]",
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
            "/subagents [list|show <id-or-name>|cancel <id-or-name>|policy]",
            &["/workers"],
        ),
        cmd(
            "/mcp",
            "show configured MCP servers and tools",
            "/mcp [list|status|show|tools|reload|add-http|add-http-service|add-stdio|add-tool|remove-tool|remove|enable|disable]",
            &[],
        ),
        cmd(
            "/hbse",
            "show HBSE secret reference setup commands",
            "/hbse [provider <id>|mcp <server> [url]|service <name>|service add|show|enable|disable|remove|services]",
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
