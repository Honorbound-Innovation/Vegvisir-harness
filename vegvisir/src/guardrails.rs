use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::tools::Tool;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub reason: String,
    pub tool_name: String,
    pub args: Map<String, Value>,
    pub risk_label: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ApprovalLedgerState {
    pub pending: BTreeMap<String, ApprovalRequest>,
    pub rejected: Vec<ApprovalRequest>,
    pub approved_once: BTreeSet<String>,
    #[serde(default, skip)]
    pub approved_for_session: BTreeSet<String>,
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
        let state = if path.exists() {
            serde_json::from_str(&fs::read_to_string(&path)?)?
        } else {
            ApprovalLedgerState::default()
        };
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

    pub fn enqueue(&mut self, request: ApprovalRequest) {
        if let Ok(mut state) = self.state.lock() {
            state.pending.entry(request.id.clone()).or_insert(request);
        }
        self.save();
    }

    pub fn approve_once(&mut self, id: &str) -> bool {
        let mut approved = false;
        if let Ok(mut state) = self.state.lock()
            && state.pending.contains_key(id)
        {
            state.approved_once.insert(id.to_string());
            approved = true;
        }
        self.save();
        approved
    }

    pub fn approve_once_request(&mut self, id: &str) -> Option<ApprovalRequest> {
        let request = self.state.lock().ok().and_then(|mut state| {
            let request = state.pending.get(id)?.clone();
            state.approved_once.insert(id.to_string());
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
            Some(request)
        });
        self.save();
        request
    }

    pub fn edit(&mut self, id: &str, args: Map<String, Value>) -> Option<ApprovalRequest> {
        let request = self.state.lock().ok().and_then(|mut state| {
            let mut request = state.pending.remove(id)?;
            request.args = args;
            request.id = approval_request_id(&request.tool_name, &request.args);
            state.pending.insert(request.id.clone(), request.clone());
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
            state.rejected.push(request);
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
            if state
                .approved_for_session
                .contains(&approval_session_key(tool_name, args))
            {
                state.pending.remove(id);
                consumed = true;
            } else if state.approved_once.remove(id) {
                state.pending.remove(id);
                consumed = true;
            }
        }
        self.save();
        consumed
    }

    pub fn reject(&mut self, request: ApprovalRequest) {
        if let Ok(mut state) = self.state.lock() {
            state.pending.remove(&request.id);
            state.rejected.push(request);
        }
        self.save();
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(state) = self.state.lock() {
            let _ = fs::write(
                path,
                serde_json::to_string_pretty(&*state).unwrap_or_default(),
            );
        }
    }
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
            allowed_commands: ["pytest", "python", "ls", "pwd"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            denied_tools: BTreeSet::new(),
        }
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
        if tool.name == "run_command" {
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
        "write_file" => "filesystem-write",
        name if name.starts_with("mcp::") => "external-tool",
        _ => "risky-tool",
    }
}
