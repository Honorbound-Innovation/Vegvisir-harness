use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{command_sandbox::command_requires_network_approval, tools::Tool};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub reason: String,
    pub tool_name: String,
    pub args: Map<String, Value>,
    pub risk_label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApprovalResolution {
    Pending,
    Approved,
    Denied,
    Missing,
}

const APPROVAL_LEDGER_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ApprovalLedgerState {
    #[serde(default = "approval_ledger_schema_version")]
    pub schema_version: u32,
    #[serde(default = "approval_ledger_timestamp")]
    pub created_at: String,
    #[serde(default = "approval_ledger_timestamp")]
    pub updated_at: String,
    #[serde(default)]
    pub pending: BTreeMap<String, ApprovalRequest>,
    #[serde(default)]
    pub rejected: Vec<ApprovalRequest>,
    #[serde(default)]
    pub approved_once: BTreeSet<String>,
    #[serde(default)]
    pub events: Vec<ApprovalLedgerEvent>,
    #[serde(default, skip)]
    pub approved_for_session: BTreeSet<String>,
}

impl Default for ApprovalLedgerState {
    fn default() -> Self {
        let now = approval_ledger_timestamp();
        Self {
            schema_version: APPROVAL_LEDGER_SCHEMA_VERSION,
            created_at: now.clone(),
            updated_at: now,
            pending: BTreeMap::new(),
            rejected: Vec::new(),
            approved_once: BTreeSet::new(),
            events: Vec::new(),
            approved_for_session: BTreeSet::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ApprovalLedgerEvent {
    pub at: String,
    pub event: ApprovalLedgerEventKind,
    pub approval_id: String,
    pub tool_name: String,
    pub risk_label: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_approval_id: Option<String>,
    #[serde(default)]
    pub session_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ApprovalLedgerEventKind {
    Requested,
    ApprovedOnce,
    ApprovedForSession,
    Edited,
    Denied,
    Consumed,
    Rejected,
    Cleared,
}

fn approval_ledger_schema_version() -> u32 {
    APPROVAL_LEDGER_SCHEMA_VERSION
}

fn approval_ledger_timestamp() -> String {
    Utc::now().to_rfc3339()
}

#[derive(Clone, Debug)]
pub struct ApprovalLedger {
    state: Arc<Mutex<ApprovalLedgerState>>,
    path: Option<PathBuf>,
}

impl Default for ApprovalLedger {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(ApprovalLedgerState::default())),
            path: None,
        }
    }
}

impl ApprovalLedger {
    pub fn new_persisted(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut state = if path.exists() {
            let mut loaded: ApprovalLedgerState =
                serde_json::from_str(&fs::read_to_string(&path)?)?;
            loaded.schema_version = APPROVAL_LEDGER_SCHEMA_VERSION;
            if loaded.created_at.trim().is_empty() {
                loaded.created_at = approval_ledger_timestamp();
            }
            if loaded.updated_at.trim().is_empty() {
                loaded.updated_at = loaded.created_at.clone();
            }
            loaded
        } else {
            ApprovalLedgerState::default()
        };
        // Session approvals are intentionally non-reusable across process restarts.
        state.approved_for_session.clear();
        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            path: Some(path),
        })
    }

    pub fn pending(&self) -> BTreeMap<String, ApprovalRequest> {
        self.state
            .lock()
            .map(|state| state.pending.clone())
            .unwrap_or_default()
    }

    pub fn clear_pending(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            let cleared = state.pending.values().cloned().collect::<Vec<_>>();
            state.pending.clear();
            for request in cleared {
                push_approval_event(
                    &mut state,
                    ApprovalLedgerEventKind::Cleared,
                    &request,
                    None,
                    false,
                );
            }
        }
        self.save();
    }

    pub fn pending_len(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.pending.len())
            .unwrap_or_default()
    }

    pub fn pending_ids(&self) -> Vec<String> {
        self.state
            .lock()
            .map(|state| state.pending.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn resolution(
        &self,
        id: &str,
        tool_name: &str,
        args: &Map<String, Value>,
    ) -> ApprovalResolution {
        self.state
            .lock()
            .map(|state| {
                if state.approved_once.contains(id)
                    || state
                        .approved_for_session
                        .contains(&approval_session_key(tool_name, args))
                {
                    ApprovalResolution::Approved
                } else if state.rejected.iter().any(|request| request.id == id) {
                    ApprovalResolution::Denied
                } else if state.pending.contains_key(id) {
                    ApprovalResolution::Pending
                } else {
                    ApprovalResolution::Missing
                }
            })
            .unwrap_or(ApprovalResolution::Missing)
    }

    pub fn enqueue(&mut self, request: ApprovalRequest) {
        if let Ok(mut state) = self.state.lock()
            && !state.pending.contains_key(&request.id)
        {
            state.pending.insert(request.id.clone(), request.clone());
            push_approval_event(
                &mut state,
                ApprovalLedgerEventKind::Requested,
                &request,
                None,
                false,
            );
        }
        self.save();
    }

    pub fn approve_once(&mut self, id: &str) -> bool {
        let mut approved = false;
        if let Ok(mut state) = self.state.lock()
            && let Some(request) = state.pending.get(id).cloned()
        {
            state.approved_once.insert(id.to_string());
            push_approval_event(
                &mut state,
                ApprovalLedgerEventKind::ApprovedOnce,
                &request,
                None,
                false,
            );
            approved = true;
        }
        self.save();
        approved
    }

    pub fn approve_once_request(&mut self, id: &str) -> Option<ApprovalRequest> {
        let request = self.state.lock().ok().and_then(|mut state| {
            let request = state.pending.get(id)?.clone();
            state.approved_once.insert(id.to_string());
            push_approval_event(
                &mut state,
                ApprovalLedgerEventKind::ApprovedOnce,
                &request,
                None,
                false,
            );
            Some(request)
        });
        self.save();
        request
    }

    pub fn approve_for_session(&mut self, id: &str) -> Option<ApprovalRequest> {
        let request = self.state.lock().ok().and_then(|mut state| {
            let request = state.pending.remove(id)?;
            state
                .approved_for_session
                .insert(approval_session_key(&request.tool_name, &request.args));
            push_approval_event(
                &mut state,
                ApprovalLedgerEventKind::ApprovedForSession,
                &request,
                None,
                true,
            );
            Some(request)
        });
        self.save();
        request
    }

    pub fn edit(&mut self, id: &str, args: Map<String, Value>) -> Option<ApprovalRequest> {
        let request = self.state.lock().ok().and_then(|mut state| {
            let mut request = state.pending.remove(id)?;
            let original = request.clone();
            request.args = args;
            request.id = approval_request_id(&request.tool_name, &request.args);
            state.pending.insert(request.id.clone(), request.clone());
            push_approval_event(
                &mut state,
                ApprovalLedgerEventKind::Edited,
                &original,
                Some(request.id.clone()),
                false,
            );
            push_approval_event(
                &mut state,
                ApprovalLedgerEventKind::Requested,
                &request,
                None,
                false,
            );
            Some(request)
        });
        self.save();
        request
    }

    pub fn deny(&mut self, id: &str) -> bool {
        let mut denied = false;
        if let Ok(mut state) = self.state.lock()
            && let Some(request) = state.pending.remove(id)
        {
            state.rejected.push(request.clone());
            push_approval_event(
                &mut state,
                ApprovalLedgerEventKind::Denied,
                &request,
                None,
                false,
            );
            denied = true;
        }
        self.save();
        denied
    }

    pub fn consume_approval(
        &mut self,
        id: &str,
        tool_name: &str,
        args: &Map<String, Value>,
    ) -> bool {
        let mut consumed = false;
        if let Ok(mut state) = self.state.lock() {
            let session_key = approval_session_key(tool_name, args);
            let session_approved = state.approved_for_session.contains(&session_key);
            let once_approved = state.approved_once.remove(id);
            if session_approved || once_approved {
                let request = state.pending.remove(id).unwrap_or_else(|| ApprovalRequest {
                    id: id.to_string(),
                    reason: if session_approved {
                        "Consumed session approval".to_string()
                    } else {
                        "Consumed one-time approval".to_string()
                    },
                    tool_name: tool_name.to_string(),
                    args: args.clone(),
                    risk_label: risk_label(tool_name).to_string(),
                });
                push_approval_event(
                    &mut state,
                    ApprovalLedgerEventKind::Consumed,
                    &request,
                    None,
                    session_approved,
                );
                consumed = true;
            }
        }
        self.save();
        consumed
    }

    pub fn reject(&mut self, request: ApprovalRequest) {
        if let Ok(mut state) = self.state.lock() {
            state.pending.remove(&request.id);
            state.rejected.push(request.clone());
            push_approval_event(
                &mut state,
                ApprovalLedgerEventKind::Rejected,
                &request,
                None,
                false,
            );
        }
        self.save();
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Some(parent) = path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            eprintln!(
                "warning: failed to create approval ledger directory {}: {error}",
                parent.display()
            );
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            state.schema_version = APPROVAL_LEDGER_SCHEMA_VERSION;
            state.updated_at = approval_ledger_timestamp();
            match serde_json::to_string_pretty(&*state) {
                Ok(json) => {
                    if let Err(error) = atomic_write(path, json.as_bytes()) {
                        eprintln!(
                            "warning: failed to save approval ledger {} atomically: {error}",
                            path.display()
                        );
                    }
                }
                Err(error) => {
                    eprintln!(
                        "warning: failed to serialize approval ledger {}: {error}",
                        path.display()
                    );
                }
            }
        }
    }
}

fn push_approval_event(
    state: &mut ApprovalLedgerState,
    event: ApprovalLedgerEventKind,
    request: &ApprovalRequest,
    replacement_approval_id: Option<String>,
    session_only: bool,
) {
    state.events.push(ApprovalLedgerEvent {
        at: approval_ledger_timestamp(),
        event,
        approval_id: request.id.clone(),
        tool_name: request.tool_name.clone(),
        risk_label: request.risk_label.clone(),
        reason: request.reason.clone(),
        replacement_approval_id,
        session_only,
    });
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("approval-ledger.json");
    let tmp_path = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp_path);
    })
}

#[derive(Clone, Debug)]
pub struct PermissionPolicy {
    pub allow_risky_tools: bool,
    pub require_human_approval: bool,
    pub bypass_approvals_and_sandbox: bool,
    pub allowed_commands: BTreeSet<String>,
    pub denied_tools: BTreeSet<String>,
}

impl Default for PermissionPolicy {
    fn default() -> Self {
        Self {
            allow_risky_tools: false,
            require_human_approval: false,
            bypass_approvals_and_sandbox: false,
            allowed_commands: default_allowed_commands(),
            denied_tools: BTreeSet::new(),
        }
    }
}

pub fn default_allowed_commands() -> BTreeSet<String> {
    [
        "awk", "cargo", "cat", "find", "git", "grep", "head", "ls", "nl", "node", "npm", "python",
        "python3", "pytest", "pwd", "rg", "sed", "tail", "test", "wc",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

pub fn normalize_command_name(command: &str) -> Option<String> {
    let command = command.trim();
    if command.is_empty()
        || command.starts_with('-')
        || command.contains('/')
        || command.contains('\\')
    {
        return None;
    }
    Some(command.to_string())
}

pub fn command_name_from_args(args: &Map<String, Value>) -> Option<String> {
    args.get("command")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .and_then(normalize_command_name)
}

fn command_parts_from_args(args: &Map<String, Value>) -> Vec<&str> {
    args.get("command")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CommandApprovalRequirement {
    risk_label: &'static str,
    reason: String,
}

fn command_approval_requirement(parts: &[&str]) -> Option<CommandApprovalRequirement> {
    let program = parts.first()?.trim();
    if program.is_empty() {
        return None;
    }
    let subcommand = command_primary_subcommand(program, parts);
    if shell_command_string_invocation(program, parts) {
        return Some(CommandApprovalRequirement {
            risk_label: "command-dynamic-shell",
            reason: format!(
                "Shell interpreter command strings require human approval: {}",
                compact_command_display(parts)
            ),
        });
    }
    match program {
        "git" => match subcommand {
            Some("push") => Some(CommandApprovalRequirement {
                risk_label: "command-external-write",
                reason: format!(
                    "Git push can publish local commits to a remote repository and requires human approval: {}",
                    compact_command_display(parts)
                ),
            }),
            Some("clean") => Some(CommandApprovalRequirement {
                risk_label: "command-destructive",
                reason: format!(
                    "Git clean can delete untracked workspace files and requires human approval: {}",
                    compact_command_display(parts)
                ),
            }),
            Some("reset") if command_has_arg(parts, "--hard") => Some(CommandApprovalRequirement {
                risk_label: "command-destructive",
                reason: format!(
                    "Git reset --hard can discard local workspace changes and requires human approval: {}",
                    compact_command_display(parts)
                ),
            }),
            Some("rebase") => Some(CommandApprovalRequirement {
                risk_label: "command-history-rewrite",
                reason: format!(
                    "Git rebase rewrites local history and requires human approval: {}",
                    compact_command_display(parts)
                ),
            }),
            _ => None,
        },
        "cargo" => match subcommand {
            Some("publish" | "yank" | "owner" | "login" | "logout") => {
                Some(CommandApprovalRequirement {
                    risk_label: "command-registry-write",
                    reason: format!(
                        "Cargo registry/account operation requires human approval: {}",
                        compact_command_display(parts)
                    ),
                })
            }
            _ => None,
        },
        "npm" | "npx" | "pnpm" | "yarn" => match subcommand {
            Some(
                "publish" | "unpublish" | "deprecate" | "dist-tag" | "owner" | "token" | "access"
                | "adduser" | "login" | "logout" | "profile" | "team",
            ) => Some(CommandApprovalRequirement {
                risk_label: "command-registry-write",
                reason: format!(
                    "Package registry/account operation requires human approval: {}",
                    compact_command_display(parts)
                ),
            }),
            _ => None,
        },
        "gh" => match subcommand {
            Some("auth") => Some(CommandApprovalRequirement {
                risk_label: "command-credential-access",
                reason: format!(
                    "GitHub CLI authentication operation requires human approval: {}",
                    compact_command_display(parts)
                ),
            }),
            Some("release" | "repo" | "gist" | "issue" | "pr" | "api") => {
                Some(CommandApprovalRequirement {
                    risk_label: "command-external-write",
                    reason: format!(
                        "GitHub CLI operation can affect external resources and requires human approval: {}",
                        compact_command_display(parts)
                    ),
                })
            }
            _ => None,
        },
        "docker" | "podman" => match subcommand {
            Some("push" | "login" | "logout") => Some(CommandApprovalRequirement {
                risk_label: "command-registry-write",
                reason: format!(
                    "Container registry/account operation requires human approval: {}",
                    compact_command_display(parts)
                ),
            }),
            Some("run") if parts.iter().any(|part| part.trim() == "--privileged") => {
                Some(CommandApprovalRequirement {
                    risk_label: "command-privileged-container",
                    reason: format!(
                        "Privileged container execution requires human approval: {}",
                        compact_command_display(parts)
                    ),
                })
            }
            _ => None,
        },
        _ => None,
    }
}

fn command_primary_subcommand<'a>(program: &str, parts: &'a [&'a str]) -> Option<&'a str> {
    let mut index = 1;
    while index < parts.len() {
        let part = parts[index].trim();
        if part.is_empty() {
            index += 1;
            continue;
        }
        if part == "--" {
            return parts
                .get(index + 1)
                .map(|value| value.trim())
                .filter(|value| !value.is_empty());
        }
        if program == "cargo" && part.starts_with('+') {
            index += 1;
            continue;
        }
        if part.starts_with('-') {
            index += if command_option_consumes_next(program, part) {
                2
            } else {
                1
            };
            continue;
        }
        return Some(part);
    }
    None
}

fn command_option_consumes_next(program: &str, option: &str) -> bool {
    if option.contains('=') {
        return false;
    }
    match program {
        "git" => matches!(
            option,
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace"
        ),
        "cargo" => matches!(option, "--manifest-path" | "--target-dir" | "--config"),
        "npm" | "npx" | "pnpm" | "yarn" => matches!(
            option,
            "--prefix" | "--workspace" | "-w" | "--userconfig" | "--registry"
        ),
        "gh" => matches!(option, "--repo" | "-R" | "--hostname"),
        "docker" | "podman" => matches!(option, "--config" | "--context" | "-H"),
        _ => false,
    }
}

fn shell_command_string_invocation(program: &str, parts: &[&str]) -> bool {
    let shell = matches!(
        program,
        "sh" | "bash" | "zsh" | "fish" | "ksh" | "dash" | "pwsh" | "powershell" | "cmd"
    );
    shell
        && parts
            .iter()
            .skip(1)
            .any(|part| shell_command_eval_flag(program, part.trim()))
}

fn shell_command_eval_flag(program: &str, flag: &str) -> bool {
    match program {
        "cmd" => flag.eq_ignore_ascii_case("/c"),
        "pwsh" | "powershell" => matches!(
            flag,
            "-Command" | "-EncodedCommand" | "-command" | "-encodedcommand"
        ),
        _ => {
            flag == "-c"
                || (flag.starts_with('-')
                    && !flag.starts_with("--")
                    && flag.chars().skip(1).any(|ch| ch == 'c'))
        }
    }
}

fn command_has_arg(parts: &[&str], needle: &str) -> bool {
    parts.iter().any(|part| part.trim() == needle)
}

fn compact_command_display(parts: &[&str]) -> String {
    let display = parts.join(" ");
    const MAX: usize = 180;
    if display.chars().count() <= MAX {
        display
    } else {
        let mut truncated = display
            .chars()
            .take(MAX.saturating_sub(1))
            .collect::<String>();
        truncated.push('…');
        truncated
    }
}

#[derive(Clone, Debug, Default)]
pub struct GuardrailEngine {
    pub policy: PermissionPolicy,
    pub approvals: ApprovalLedger,
}

impl GuardrailEngine {
    pub fn authorize_tool(&mut self, tool: &Tool, args: &Map<String, Value>) -> anyhow::Result<()> {
        if self.policy.bypass_approvals_and_sandbox {
            return Ok(());
        }
        if self.policy.denied_tools.contains(&tool.name) {
            anyhow::bail!("Tool is denied by policy: {}", tool.name);
        }
        let mut approval_granted = false;
        if sudo_guarded_tool_name(&tool.name) {
            reject_unsafe_sudo_invocation(&tool.name, args)?;
        }
        if command_policy_tool_name(&tool.name)
            && let Some(executable) = command_name_from_args(args)
            && !self.policy.allowed_commands.contains(&executable)
        {
            let request_id = approval_request_id(&tool.name, args);
            if self
                .approvals
                .consume_approval(&request_id, &tool.name, args)
            {
                approval_granted = true;
            } else {
                let request = ApprovalRequest {
                    id: request_id,
                    reason: format!(
                        "Shell command is not allow-listed: {executable}. Approve once or allow this command for the session."
                    ),
                    tool_name: tool.name.clone(),
                    args: args.clone(),
                    risk_label: "command-allow".to_string(),
                };
                let id = request.id.clone();
                let reason = request.reason.clone();
                self.approvals.enqueue(request);
                anyhow::bail!("{reason}; approval_id={id}");
            }
        }
        if command_policy_tool_name(&tool.name) && !approval_granted {
            let parts = command_parts_from_args(args);
            if let Some(requirement) = command_approval_requirement(&parts) {
                let request_id = approval_request_id(&tool.name, args);
                if self
                    .approvals
                    .consume_approval(&request_id, &tool.name, args)
                {
                    approval_granted = true;
                } else {
                    let request = ApprovalRequest {
                        id: request_id,
                        reason: requirement.reason,
                        tool_name: tool.name.clone(),
                        args: args.clone(),
                        risk_label: requirement.risk_label.to_string(),
                    };
                    let id = request.id.clone();
                    let reason = request.reason.clone();
                    self.approvals.enqueue(request);
                    anyhow::bail!("{reason}; approval_id={id}");
                }
            }
        }
        if command_policy_tool_name(&tool.name) && !approval_granted {
            let parts = command_parts_from_args(args);
            if command_requires_network_approval(&parts)? {
                let request_id = approval_request_id(&tool.name, args);
                if self
                    .approvals
                    .consume_approval(&request_id, &tool.name, args)
                {
                    approval_granted = true;
                } else {
                    let command_display = compact_command_display(&parts);
                    let request = ApprovalRequest {
                        id: request_id,
                        reason: format!(
                            "Command network access requires human approval: {command_display}"
                        ),
                        tool_name: tool.name.clone(),
                        args: args.clone(),
                        risk_label: "command-network".to_string(),
                    };
                    let id = request.id.clone();
                    let reason = request.reason.clone();
                    self.approvals.enqueue(request);
                    anyhow::bail!("{reason}; approval_id={id}");
                }
            }
        }
        if self.policy.require_human_approval && tool.risky && !self.policy.allow_risky_tools {
            let request_id = approval_request_id(&tool.name, args);
            if self
                .approvals
                .consume_approval(&request_id, &tool.name, args)
            {
                approval_granted = true;
            } else {
                let request = ApprovalRequest {
                    id: request_id,
                    reason: format!("Risky tool requires human approval: {}", tool.name),
                    tool_name: tool.name.clone(),
                    args: args.clone(),
                    risk_label: risk_label(&tool.name).to_string(),
                };
                let id = request.id.clone();
                let reason = request.reason.clone();
                self.approvals.enqueue(request);
                anyhow::bail!("{reason}; approval_id={id}");
            }
        }
        if tool.risky && !self.policy.allow_risky_tools && !approval_granted {
            anyhow::bail!("Risky tool requires permission: {}", tool.name);
        }
        if command_policy_tool_name(&tool.name) && !approval_granted {
            let executable = args
                .get("command")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(Value::as_str)
                .unwrap_or("");
            if !self.policy.allowed_commands.contains(executable) {
                anyhow::bail!("Command is not allow-listed: {executable}");
            }
        }
        Ok(())
    }
}

fn reject_unsafe_sudo_invocation(tool_name: &str, args: &Map<String, Value>) -> anyhow::Result<()> {
    let parts = args
        .get("command")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    if parts.is_empty() {
        return Ok(());
    }
    if tool_name == "run_privileged_command" && command_mentions_sudo_invocation(&parts) {
        anyhow::bail!(
            "Do not include sudo in run_privileged_command arguments. Run /sudo auth, then provide the underlying command; Vegvisir executes it through the private local supervisor."
        );
    }
    if tool_name != "run_privileged_command" && command_mentions_sudo_invocation(&parts) {
        anyhow::bail!(
            "Direct sudo through normal command tools is disabled so sudo passwords cannot enter chat/session/trace history. Run /sudo auth, then use run_privileged_command."
        );
    }
    Ok(())
}

fn command_mentions_sudo_invocation(parts: &[&str]) -> bool {
    parts.iter().any(|part| {
        let trimmed = part.trim();
        trimmed == "sudo"
            || trimmed.ends_with("/sudo")
            || trimmed.starts_with("sudo ")
            || trimmed.contains(" sudo ")
            || trimmed.contains(";sudo ")
            || trimmed.contains("; sudo ")
            || trimmed.contains("&&sudo ")
            || trimmed.contains("&& sudo ")
            || trimmed.contains("|sudo ")
            || trimmed.contains("| sudo ")
    })
}

fn sudo_guarded_tool_name(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "run_command" | "run_privileged_command" | "run_tests"
    )
}

fn command_policy_tool_name(tool_name: &str) -> bool {
    matches!(tool_name, "run_command" | "run_privileged_command")
}

fn approval_request_id(tool_name: &str, args: &Map<String, Value>) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    approval_session_key(tool_name, args).hash(&mut hasher);
    format!("apr_{:016x}", hasher.finish())
}

fn approval_session_key(tool_name: &str, args: &Map<String, Value>) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    tool_name.hash(&mut hasher);
    serde_json::to_string(args)
        .unwrap_or_default()
        .hash(&mut hasher);
    format!("{}:{:016x}", tool_name, hasher.finish())
}

fn risk_label(tool_name: &str) -> &'static str {
    match tool_name {
        "run_command" => "command-execution",
        "run_privileged_command" => "privileged-command-execution",
        "write_file" => "filesystem-write",
        name if name.starts_with("mcp::") => "external-tool",
        _ => "risky-tool",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tools::Tool, types::Observation};
    use serde_json::json;

    fn sample_request() -> ApprovalRequest {
        let mut args = Map::new();
        args.insert("path".to_string(), json!("example.txt"));
        ApprovalRequest {
            id: approval_request_id("write_file", &args),
            reason: "Risky tool requires human approval: write_file".to_string(),
            tool_name: "write_file".to_string(),
            args,
            risk_label: "filesystem-write".to_string(),
        }
    }

    fn run_command_tool() -> Tool {
        Tool::new(
            "run_command",
            "test",
            std::sync::Arc::new(|_| Observation::ok("")),
            serde_json::json!({"required": ["command"], "properties": {"command": "array"}}),
            true,
        )
    }

    fn run_command_engine_with_allowed(commands: &[&str]) -> GuardrailEngine {
        GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                allowed_commands: commands.iter().map(|command| command.to_string()).collect(),
                ..PermissionPolicy::default()
            },
            approvals: ApprovalLedger::default(),
        }
    }

    #[test]
    fn command_policy_allows_safe_allowlisted_subcommands() -> anyhow::Result<()> {
        let mut engine = run_command_engine_with_allowed(&["git", "cargo", "npm"]);
        let tool = run_command_tool();

        for command in [
            json!(["git", "status"]),
            json!(["git", "-C", ".", "status"]),
            json!(["cargo", "test"]),
            json!(["npm", "test"]),
        ] {
            let args = json!({"command": command}).as_object().cloned().unwrap();
            engine.authorize_tool(&tool, &args)?;
        }
        Ok(())
    }

    #[test]
    fn command_policy_requires_approval_for_git_push_even_when_git_is_allowed() {
        let mut engine = run_command_engine_with_allowed(&["git"]);
        let tool = run_command_tool();
        let args = json!({"command": ["git", "push", "origin", "dev"]})
            .as_object()
            .cloned()
            .unwrap();

        let error = engine.authorize_tool(&tool, &args).unwrap_err().to_string();

        assert!(
            error.contains("Git push can publish local commits"),
            "{error}"
        );
        let pending = engine.approvals.pending();
        let request = pending.values().next().expect("pending approval");
        assert_eq!(request.risk_label, "command-external-write");
    }

    #[test]
    fn command_policy_requires_approval_for_destructive_git_subcommands() {
        let tool = run_command_tool();
        for command in [
            json!(["git", "clean", "-fd"]),
            json!(["git", "reset", "--hard"]),
        ] {
            let mut engine = run_command_engine_with_allowed(&["git"]);
            let args = json!({"command": command}).as_object().cloned().unwrap();
            let error = engine.authorize_tool(&tool, &args).unwrap_err().to_string();

            assert!(
                error.contains("requires human approval"),
                "expected approval error, got {error}"
            );
            let pending = engine.approvals.pending();
            let request = pending.values().next().expect("pending approval");
            assert_eq!(request.risk_label, "command-destructive");
        }
    }

    #[test]
    fn command_policy_requires_approval_for_package_publish() {
        let mut engine = run_command_engine_with_allowed(&["cargo", "npm"]);
        let tool = run_command_tool();
        let args = json!({"command": ["cargo", "publish"]})
            .as_object()
            .cloned()
            .unwrap();

        let error = engine.authorize_tool(&tool, &args).unwrap_err().to_string();

        assert!(
            error.contains("Cargo registry/account operation"),
            "{error}"
        );
        let pending = engine.approvals.pending();
        let request = pending.values().next().expect("pending approval");
        assert_eq!(request.risk_label, "command-registry-write");
    }

    #[test]
    fn command_policy_requires_approval_for_shell_command_strings() {
        let mut engine = run_command_engine_with_allowed(&["bash"]);
        let tool = run_command_tool();
        let args = json!({"command": ["bash", "-lc", "echo dynamic"]})
            .as_object()
            .cloned()
            .unwrap();

        let error = engine.authorize_tool(&tool, &args).unwrap_err().to_string();

        assert!(
            error.contains("Shell interpreter command strings require human approval"),
            "{error}"
        );
        let pending = engine.approvals.pending();
        let request = pending.values().next().expect("pending approval");
        assert_eq!(request.risk_label, "command-dynamic-shell");
    }

    #[test]
    fn guardrails_reject_direct_sudo_through_run_command() {
        let mut engine = GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                ..PermissionPolicy::default()
            },
            approvals: ApprovalLedger::default(),
        };
        let tool = Tool::new(
            "run_command",
            "test",
            std::sync::Arc::new(|_| Observation::ok("")),
            serde_json::json!({"required": ["command"], "properties": {"command": "array"}}),
            true,
        );
        let args = serde_json::json!({"command": ["sudo", "id"]})
            .as_object()
            .cloned()
            .unwrap();

        let error = engine.authorize_tool(&tool, &args).unwrap_err().to_string();

        assert!(error.contains("Direct sudo through normal command tools is disabled"));
    }

    #[test]
    fn guardrails_reject_nested_sudo_patterns() {
        let mut engine = GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                allowed_commands: ["bash".to_string()].into_iter().collect(),
                ..PermissionPolicy::default()
            },
            approvals: ApprovalLedger::default(),
        };
        let tool = Tool::new(
            "run_command",
            "test",
            std::sync::Arc::new(|_| Observation::ok("")),
            serde_json::json!({"required": ["command"], "properties": {"command": "array"}}),
            true,
        );
        let args = serde_json::json!({"command": ["bash", "-lc", "printf x | sudo -S id"]})
            .as_object()
            .cloned()
            .unwrap();

        let error = engine.authorize_tool(&tool, &args).unwrap_err().to_string();

        assert!(error.contains("Direct sudo through normal command tools is disabled"));
    }

    #[test]
    fn guardrails_reject_nested_sudo_for_privileged_tool() {
        let mut engine = GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                allowed_commands: ["sudo".to_string()].into_iter().collect(),
                ..PermissionPolicy::default()
            },
            approvals: ApprovalLedger::default(),
        };
        let tool = Tool::new(
            "run_privileged_command",
            "test",
            std::sync::Arc::new(|_| Observation::ok("")),
            serde_json::json!({"required": ["command"], "properties": {"command": "array"}}),
            true,
        );
        let args = serde_json::json!({"command": ["sudo", "id"]})
            .as_object()
            .cloned()
            .unwrap();

        let error = engine.authorize_tool(&tool, &args).unwrap_err().to_string();

        assert!(error.contains("Do not include sudo"));
    }

    #[test]
    fn approval_ledger_clear_pending_removes_stale_requests() {
        let mut ledger = ApprovalLedger::default();
        let request = sample_request();
        let id = request.id.clone();
        ledger.enqueue(request);
        assert_eq!(ledger.pending_len(), 1);

        ledger.clear_pending();

        assert_eq!(ledger.pending_len(), 0);
        assert!(matches!(
            ledger.resolution(&id, "write_file", &serde_json::Map::new()),
            ApprovalResolution::Missing
        ));
    }

    #[test]
    fn approval_ledger_persists_schema_timestamps_events_and_uses_atomic_replace()
    -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("approvals.json");
        let mut ledger = ApprovalLedger::new_persisted(&path)?;
        let request = sample_request();
        let id = request.id.clone();

        ledger.enqueue(request.clone());
        assert!(ledger.approve_once(&id));
        assert!(ledger.consume_approval(&id, &request.tool_name, &request.args));

        let text = std::fs::read_to_string(&path)?;
        let json: serde_json::Value = serde_json::from_str(&text)?;
        assert_eq!(json["schema_version"], APPROVAL_LEDGER_SCHEMA_VERSION);
        assert!(
            json["created_at"]
                .as_str()
                .is_some_and(|value| value.contains('T'))
        );
        assert!(
            json["updated_at"]
                .as_str()
                .is_some_and(|value| value.contains('T'))
        );
        assert_eq!(json["pending"].as_object().map(|v| v.len()), Some(0));
        assert!(
            json["approved_once"]
                .as_array()
                .is_some_and(|items| items.is_empty())
        );
        let events = json["events"].as_array().expect("events array");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["event"], "requested");
        assert_eq!(events[1]["event"], "approved_once");
        assert_eq!(events[2]["event"], "consumed");
        assert_eq!(events[2]["approval_id"], id);
        assert!(!tmp.path().read_dir()?.any(|entry| {
            entry
                .ok()
                .and_then(|entry| entry.file_name().into_string().ok())
                .is_some_and(|name| name.contains(".tmp-"))
        }));
        Ok(())
    }

    #[test]
    fn approval_ledger_persists_session_approval_audit_without_reusing_it() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("approvals.json");
        let mut ledger = ApprovalLedger::new_persisted(&path)?;
        let request = sample_request();
        let id = request.id.clone();

        ledger.enqueue(request.clone());
        assert!(ledger.approve_for_session(&id).is_some());
        assert_eq!(
            ledger.resolution(&id, &request.tool_name, &request.args),
            ApprovalResolution::Approved
        );

        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        let events = json["events"].as_array().expect("events array");
        assert_eq!(events[1]["event"], "approved_for_session");
        assert_eq!(events[1]["session_only"], true);
        assert!(json.get("approved_for_session").is_none());

        let reloaded = ApprovalLedger::new_persisted(&path)?;
        assert_eq!(
            reloaded.resolution(&id, &request.tool_name, &request.args),
            ApprovalResolution::Missing
        );
        Ok(())
    }

    #[test]
    fn approval_ledger_reports_pending_approved_and_denied_resolution() {
        let mut ledger = ApprovalLedger::default();
        let request = sample_request();
        ledger.enqueue(request.clone());

        assert_eq!(
            ledger.resolution(&request.id, &request.tool_name, &request.args),
            ApprovalResolution::Pending
        );
        assert!(ledger.approve_once(&request.id));
        assert_eq!(
            ledger.resolution(&request.id, &request.tool_name, &request.args),
            ApprovalResolution::Approved
        );
        assert!(ledger.consume_approval(&request.id, &request.tool_name, &request.args));
        assert_eq!(
            ledger.resolution(&request.id, &request.tool_name, &request.args),
            ApprovalResolution::Missing
        );

        ledger.enqueue(request.clone());
        assert!(ledger.deny(&request.id));
        assert_eq!(
            ledger.resolution(&request.id, &request.tool_name, &request.args),
            ApprovalResolution::Denied
        );
    }
}
