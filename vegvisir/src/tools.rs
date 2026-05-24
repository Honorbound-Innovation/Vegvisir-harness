use std::{
    collections::HashMap,
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Map, Value, json};
use walkdir::WalkDir;

use crate::{
    guardrails::GuardrailEngine,
    memory::{ContextPrepareOptions, VegvisirCms, VegvisirCmsConfig},
    observability::EventLogger,
    policy::RuntimePolicy,
    sandbox::WorkspaceSandbox,
    types::{Observation, ToolCall},
};

const LIST_FILES_DEFAULT_LIMIT: usize = 500;
const LIST_FILES_MAX_LIMIT: usize = 2_000;

pub type ToolHandler = Arc<dyn Fn(Map<String, Value>) -> Observation + Send + Sync>;

#[derive(Clone)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub handler: ToolHandler,
    pub schema: Value,
    pub risky: bool,
    pub timeout_seconds: Option<u64>,
}

impl Tool {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        handler: ToolHandler,
        schema: Value,
        risky: bool,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            handler,
            schema,
            risky,
            timeout_seconds: None,
        }
    }

    pub fn validate_args(&self, args: &Map<String, Value>) -> anyhow::Result<()> {
        let required = self
            .schema
            .get("required")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let properties = self
            .schema
            .get("properties")
            .unwrap_or(&self.schema)
            .as_object()
            .cloned()
            .unwrap_or_default();
        for key in required {
            let Some(key) = key.as_str() else { continue };
            if !args.contains_key(key) {
                anyhow::bail!("Missing required argument for {}: {key}", self.name);
            }
        }
        for (key, value) in args {
            let expected = properties.get(key).and_then(|spec| {
                spec.as_str()
                    .map(str::to_string)
                    .or_else(|| spec.get("type").and_then(Value::as_str).map(str::to_string))
            });
            match expected.as_deref() {
                Some("string") if !value.is_string() => {
                    anyhow::bail!("{}.{key} must be a string", self.name)
                }
                Some("integer") if !value.is_i64() && !value.is_u64() => {
                    anyhow::bail!("{}.{key} must be an integer", self.name)
                }
                Some("array") if !value.is_array() => {
                    anyhow::bail!("{}.{key} must be an array", self.name)
                }
                Some("object") if !value.is_object() => {
                    anyhow::bail!("{}.{key} must be an object", self.name)
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Tool>,
}

impl ToolRegistry {
    pub fn register(&mut self, tool: Tool) -> anyhow::Result<()> {
        if self.tools.contains_key(&tool.name) {
            anyhow::bail!("Tool already registered: {}", tool.name);
        }
        self.tools.insert(tool.name.clone(), tool);
        Ok(())
    }

    pub fn get(&self, name: &str) -> anyhow::Result<&Tool> {
        self.tools
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool: {name}"))
    }

    pub fn list(&self) -> Vec<&Tool> {
        self.tools.values().collect()
    }

    pub fn schemas(&self) -> Vec<Value> {
        self.list()
            .into_iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.schema,
                    "risky": tool.risky,
                })
            })
            .collect()
    }
}

#[derive(Clone)]
pub struct ToolExecutor {
    pub registry: ToolRegistry,
    pub guardrails: GuardrailEngine,
    pub runtime_policy: RuntimePolicy,
    pub logger: EventLogger,
}

impl ToolExecutor {
    pub fn execute(&mut self, call: ToolCall) -> Observation {
        let result = (|| {
            let tool = self.registry.get(&call.name)?;
            tool.validate_args(&call.args)?;
            self.guardrails.authorize_tool(tool, &call.args)?;
            if !self.guardrails.policy.bypass_approvals_and_sandbox {
                self.runtime_policy
                    .authorize_tool(&tool.name, &call.args, &self.logger)
                    .map_err(anyhow::Error::msg)?;
            }
            self.logger
                .emit("tool_start", json!({"tool": call.name, "args": call.args}));
            let observation = (tool.handler)(call.args.clone());
            self.logger.emit(
                "tool_end",
                json!({"tool": call.name, "ok": observation.ok, "error": observation.error}),
            );
            Ok::<_, anyhow::Error>(observation)
        })();
        match result {
            Ok(observation) => observation,
            Err(error) => {
                let error_text = error.to_string();
                let error_kind = if error_text.contains("approval_id=") {
                    "ApprovalRequired"
                } else {
                    "ToolError"
                };
                self.logger.emit(
                    "tool_error",
                    json!({"tool": call.name, "error": error_text}),
                );
                Observation::err(error_text, error_kind)
            }
        }
    }
}

pub fn build_builtin_registry(workspace: impl AsRef<Path>) -> anyhow::Result<ToolRegistry> {
    build_builtin_registry_with_cms(
        workspace.as_ref(),
        VegvisirCmsConfig::for_workspace(workspace.as_ref()),
    )
}

pub fn build_builtin_registry_with_cms(
    workspace: impl AsRef<Path>,
    cms_config: VegvisirCmsConfig,
) -> anyhow::Result<ToolRegistry> {
    build_builtin_registry_with_cms_and_mode(workspace, cms_config, false)
}

pub fn build_builtin_registry_with_cms_and_mode(
    workspace: impl AsRef<Path>,
    cms_config: VegvisirCmsConfig,
    bypass_sandbox: bool,
) -> anyhow::Result<ToolRegistry> {
    let sandbox = if bypass_sandbox {
        WorkspaceSandbox::new_unrestricted(workspace)?
    } else {
        WorkspaceSandbox::new(workspace)?
    };
    let mut registry = ToolRegistry::default();

    let list_sandbox = sandbox.clone();
    registry.register(Tool::new(
        "list_files",
        "List files under a workspace path.",
        Arc::new(move |args| {
            let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|value| value as usize)
                .unwrap_or(LIST_FILES_DEFAULT_LIMIT)
                .clamp(1, LIST_FILES_MAX_LIMIT);
            let root = match list_sandbox.resolve(path) {
                Ok(root) => root,
                Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
            };
            if !root.exists() {
                return Observation::err(format!("Path does not exist: {path}"), "NotFound");
            }
            let mut files = WalkDir::new(&root)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_file())
                .filter_map(|entry| {
                    entry
                        .path()
                        .strip_prefix(&list_sandbox.root)
                        .ok()
                        .map(|path| path.to_string_lossy().to_string())
                        .or_else(|| Some(entry.path().display().to_string()))
                })
                .collect::<Vec<_>>();
            files.sort();
            let total_files = files.len();
            let truncated = total_files > limit;
            files.truncate(limit);
            let mut data = Map::new();
            data.insert("files".to_string(), json!(files.clone()));
            data.insert("total_files".to_string(), json!(total_files));
            data.insert("output_truncated".to_string(), json!(truncated));
            let mut content = files.join("\n");
            if truncated {
                content.push_str(&format!(
                    "\n[list_files truncated at {limit} of {total_files} files; narrow path or raise limit up to {LIST_FILES_MAX_LIMIT}]"
                ));
            }
            Observation {
                ok: true,
                content,
                data,
                error: None,
            }
        }),
        json!({"properties": {"path": "string", "limit": "integer"}}),
        false,
    ))?;

    let read_sandbox = sandbox.clone();
    registry.register(Tool::new(
        "read_file",
        "Read a UTF-8 file from the workspace.",
        Arc::new(move |args| {
            let Some(path) = args.get("path").and_then(Value::as_str) else {
                return Observation::err("Missing path", "ValueError");
            };
            match read_sandbox.read_text(path) {
                Ok(content) => {
                    let original_bytes = content.len();
                    let mut data = Map::new();
                    data.insert("path".to_string(), json!(path));
                    data.insert("bytes".to_string(), json!(original_bytes));
                    data.insert("output_truncated".to_string(), json!(false));
                    Observation {
                        ok: true,
                        content,
                        data,
                        error: None,
                    }
                }
                Err(error) => Observation::err(error.to_string(), "ReadError"),
            }
        }),
        json!({"required": ["path"], "properties": {"path": "string"}}),
        false,
    ))?;

    let write_sandbox = sandbox.clone();
    registry.register(Tool::new(
        "write_file",
        "Write a UTF-8 file inside the workspace.",
        Arc::new(move |args| {
            let Some(path) = args.get("path").and_then(Value::as_str) else {
                return Observation::err("Missing path", "ValueError");
            };
            let Some(content) = args.get("content").and_then(Value::as_str) else {
                return Observation::err("Missing content", "ValueError");
            };
            let previous_content = write_sandbox.read_text(path).ok();
            match write_sandbox.write_text(path, content) {
                Ok(target) => {
                    let relative = target.strip_prefix(&write_sandbox.root).unwrap_or(&target);
                    let mut data = Map::new();
                    data.insert("path".to_string(), json!(path));
                    data.insert("bytes".to_string(), json!(content.len()));
                    if previous_content.as_deref() != Some(content) {
                        data.insert(
                            "diff".to_string(),
                            json!(simple_unified_diff(
                                &relative.display().to_string(),
                                previous_content.as_deref().unwrap_or(""),
                                content,
                            )),
                        );
                    }
                    Observation {
                        ok: true,
                        content: format!("Wrote {}", relative.display()),
                        data,
                        error: None,
                    }
                }
                Err(error) => Observation::err(error.to_string(), "WriteError"),
            }
        }),
        json!({"required": ["path", "content"], "properties": {"path": "string", "content": "string"}}),
        true,
    ))?;

    let run_root = sandbox.root.clone();
    registry.register(Tool::new(
        "run_command",
        "Run an allow-listed command in the workspace.",
        Arc::new(move |args| {
            let Some(command) = args.get("command").and_then(Value::as_array) else {
                return Observation::err("Missing command", "ValueError");
            };
            let parts = command.iter().filter_map(Value::as_str).collect::<Vec<_>>();
            if parts.is_empty() {
                return Observation::err("Empty command", "ValueError");
            }
            let timeout = args
                .get("timeout")
                .and_then(Value::as_u64)
                .unwrap_or(30)
                .clamp(1, 3600);
            let output_limit = args
                .get("output_limit")
                .and_then(Value::as_u64)
                .unwrap_or(20000)
                .clamp(1024, 1_000_000) as usize;
            let mut child = match Command::new(parts[0])
                .args(&parts[1..])
                .current_dir(&run_root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(error) => return Observation::err(error.to_string(), "CommandError"),
            };
            let started = Instant::now();
            let mut timed_out = false;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => {
                        if started.elapsed() >= Duration::from_secs(timeout) {
                            timed_out = true;
                            let _ = child.kill();
                            break;
                        }
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(error) => return Observation::err(error.to_string(), "CommandError"),
                }
            }
            match child.wait_with_output() {
                Ok(output) => {
                    let mut content = String::new();
                    content.push_str(&String::from_utf8_lossy(&output.stdout));
                    content.push_str(&String::from_utf8_lossy(&output.stderr));
                    let truncated = content.len() > output_limit;
                    if truncated {
                        content = format!(
                            "{}\n[output truncated at {} bytes]",
                            content.chars().take(output_limit).collect::<String>(),
                            output_limit
                        );
                    }
                    let mut data = Map::new();
                    data.insert(
                        "returncode".to_string(),
                        json!(if timed_out {
                            -1
                        } else {
                            output.status.code().unwrap_or(-1)
                        }),
                    );
                    data.insert("timed_out".to_string(), json!(timed_out));
                    data.insert("timeout_seconds".to_string(), json!(timeout));
                    data.insert("output_truncated".to_string(), json!(truncated));
                    Observation {
                        ok: !timed_out && output.status.success(),
                        content,
                        data,
                        error: if timed_out {
                            Some("CommandTimeout".to_string())
                        } else {
                            (!output.status.success()).then(|| "CommandFailed".to_string())
                        },
                    }
                }
                Err(error) => Observation::err(error.to_string(), "CommandError"),
            }
        }),
        json!({"required": ["command"], "properties": {"command": "array", "timeout": "integer", "output_limit": "integer"}}),
        true,
    ))?;

    let test_root = sandbox.root.clone();
    registry.register(Tool::new(
        "run_tests",
        "Run the workspace test suite with a bounded command.",
        Arc::new(move |args| {
            let parts = if let Some(command) = args.get("command").and_then(Value::as_array) {
                command.iter().filter_map(Value::as_str).collect::<Vec<_>>()
            } else if test_root.join("Cargo.toml").exists() {
                vec!["cargo", "test"]
            } else if test_root.join("package.json").exists() {
                vec!["npm", "test"]
            } else if test_root.join("pyproject.toml").exists()
                || test_root.join("pytest.ini").exists()
                || test_root.join("setup.py").exists()
            {
                vec!["python", "-m", "pytest"]
            } else {
                return Observation::err(
                    "Could not infer test command. Provide command=[...]",
                    "ValueError",
                );
            };
            if parts.is_empty() {
                return Observation::err("Empty test command", "ValueError");
            }
            let timeout = args
                .get("timeout")
                .and_then(Value::as_u64)
                .unwrap_or(120)
                .clamp(1, 3600);
            let output_limit = args
                .get("output_limit")
                .and_then(Value::as_u64)
                .unwrap_or(40000)
                .clamp(1024, 1_000_000) as usize;
            let mut child = match Command::new(parts[0])
                .args(&parts[1..])
                .current_dir(&test_root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(error) => return Observation::err(error.to_string(), "CommandError"),
            };
            let started = Instant::now();
            let mut timed_out = false;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => {
                        if started.elapsed() >= Duration::from_secs(timeout) {
                            timed_out = true;
                            let _ = child.kill();
                            break;
                        }
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(error) => return Observation::err(error.to_string(), "CommandError"),
                }
            }
            match child.wait_with_output() {
                Ok(output) => {
                    let mut content = String::new();
                    content.push_str(&String::from_utf8_lossy(&output.stdout));
                    content.push_str(&String::from_utf8_lossy(&output.stderr));
                    let truncated = content.len() > output_limit;
                    if truncated {
                        content = format!(
                            "{}\n[output truncated at {} bytes]",
                            content.chars().take(output_limit).collect::<String>(),
                            output_limit
                        );
                    }
                    let mut data = Map::new();
                    data.insert("command".to_string(), json!(parts));
                    data.insert(
                        "returncode".to_string(),
                        json!(if timed_out {
                            -1
                        } else {
                            output.status.code().unwrap_or(-1)
                        }),
                    );
                    data.insert("timed_out".to_string(), json!(timed_out));
                    data.insert("timeout_seconds".to_string(), json!(timeout));
                    data.insert("output_truncated".to_string(), json!(truncated));
                    Observation {
                        ok: !timed_out && output.status.success(),
                        content,
                        data,
                        error: if timed_out {
                            Some("CommandTimeout".to_string())
                        } else {
                            (!output.status.success()).then(|| "TestsFailed".to_string())
                        },
                    }
                }
                Err(error) => Observation::err(error.to_string(), "CommandError"),
            }
        }),
        json!({"properties": {"command": "array", "timeout": "integer", "output_limit": "integer"}}),
        true,
    ))?;

    let remember_config = cms_config.clone();
    registry.register(Tool::new(
        "cms_remember",
        "Store a durable memory through CMS-v2.",
        Arc::new(move |args| {
            let memory_type = args
                .get("type")
                .and_then(Value::as_str)
                .or_else(|| args.get("memory_type").and_then(Value::as_str))
                .unwrap_or("note");
            let Some(title) = args.get("title").and_then(Value::as_str) else {
                return Observation::err("Missing title", "ValueError");
            };
            let Some(content) = args.get("content").and_then(Value::as_str) else {
                return Observation::err("Missing content", "ValueError");
            };
            match VegvisirCms::open(remember_config.clone())
                .and_then(|mut cms| cms.remember(memory_type, title, content))
            {
                Ok(result) => {
                    let mut data = Map::new();
                    data.insert("memory_id".to_string(), json!(result.memory_id.0));
                    data.insert("created_new".to_string(), json!(result.created_new));
                    data.insert("updated_existing".to_string(), json!(result.updated_existing));
                    Observation {
                        ok: true,
                        content: format!(
                            "Remembered memory {}",
                            data["memory_id"].as_str().unwrap_or("")
                        ),
                        data,
                        error: None,
                    }
                }
                Err(error) => Observation::err(error.to_string(), "CmsError"),
            }
        }),
        json!({"required": ["title", "content"], "properties": {"type": "string", "title": "string", "content": "string"}}),
        false,
    ))?;

    let recall_config = cms_config.clone();
    registry.register(Tool::new(
        "cms_recall",
        "Retrieve relevant memories through CMS-v2.",
        Arc::new(move |args| {
            let Some(query) = args.get("query").and_then(Value::as_str) else {
                return Observation::err("Missing query", "ValueError");
            };
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(5) as usize;
            match VegvisirCms::open(recall_config.clone())
                .and_then(|mut cms| cms.retrieve(query, limit))
            {
                Ok(bundle) => {
                    let summaries = bundle
                        .results
                        .iter()
                        .map(|result| {
                            format!(
                                "- {} [{}]: {}",
                                result.memory.title, result.memory.id.0, result.memory.summary
                            )
                        })
                        .collect::<Vec<_>>();
                    let mut data = Map::new();
                    data.insert("results".to_string(), json!(bundle.results));
                    data.insert("trace".to_string(), json!(bundle.trace));
                    Observation {
                        ok: true,
                        content: if summaries.is_empty() {
                            "No CMS memories matched.".to_string()
                        } else {
                            summaries.join("\n")
                        },
                        data,
                        error: None,
                    }
                }
                Err(error) => Observation::err(error.to_string(), "CmsError"),
            }
        }),
        json!({"required": ["query"], "properties": {"query": "string", "limit": "integer"}}),
        false,
    ))?;

    let recent_config = cms_config.clone();
    registry.register(Tool::new(
        "cms_recent",
        "Return recent CMS-v2 memories for the local session user.",
        Arc::new(move |args| {
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(6) as usize;
            match VegvisirCms::open(recent_config.clone())
                .and_then(|mut cms| cms.retrieve("", limit.clamp(1, 20)))
            {
                Ok(bundle) => {
                    let memories = bundle
                        .results
                        .iter()
                        .map(|result| {
                            json!({
                                "id": result.memory.id.0,
                                "type": result.memory.memory_type,
                                "title": result.memory.title,
                                "summary": result.memory.summary,
                                "content": result.memory.body,
                            })
                        })
                        .collect::<Vec<_>>();
                    let mut data = Map::new();
                    data.insert("memories".to_string(), json!(memories));
                    Observation {
                        ok: true,
                        content: if memories.is_empty() {
                            "No recent CMS memories are available.".to_string()
                        } else {
                            serde_json::to_string_pretty(&json!({"memories": memories}))
                                .unwrap_or_default()
                        },
                        data,
                        error: None,
                    }
                }
                Err(error) => Observation::err(error.to_string(), "CmsError"),
            }
        }),
        json!({"properties": {"limit": "integer"}}),
        false,
    ))?;

    let context_config = cms_config;
    let legacy_context_config = context_config.clone();
    let cached_prompt_config = context_config.clone();
    registry.register(Tool::new(
        "cms_prepare_context",
        "Prepare ECM context from CMS-v2 for a message.",
        Arc::new(move |args| {
            let Some(message) = args.get("message").and_then(Value::as_str) else {
                return Observation::err("Missing message", "ValueError");
            };
            let options = context_options_from_args(&args);
            match VegvisirCms::open(context_config.clone())
                .and_then(|mut cms| cms.prepare_context_with_options(message, options))
            {
                Ok(prepared) => {
                    let mut data = Map::new();
                    data.insert("trace_id".to_string(), json!(prepared.trace_id));
                    data.insert(
                        "included_memory_ids".to_string(),
                        json!(
                            prepared
                                .included_memory_ids
                                .iter()
                                .map(|memory_id| memory_id.0.clone())
                                .collect::<Vec<_>>()
                        ),
                    );
                    data.insert("token_estimate".to_string(), json!(prepared.token_estimate));
                    Observation {
                        ok: true,
                        content: prepared.packed_text,
                        data,
                        error: None,
                    }
                }
                Err(error) => Observation::err(error.to_string(), "CmsError"),
            }
        }),
        json!({"required": ["message"], "properties": {"message": "string", "mode": "string", "model_context_window": "integer"}}),
        false,
    ))?;

    registry.register(Tool::new(
        "eternium_prepare_context",
        "Compatibility alias for cms_prepare_context. Prepare CMS-v2 context for a user message using recall and budgeting.",
        Arc::new(move |args| {
            let Some(message) = args
                .get("user_message")
                .or_else(|| args.get("message"))
                .and_then(Value::as_str)
            else {
                return Observation::err("Missing user_message", "ValueError");
            };
            let options = context_options_from_args(&args);
            match VegvisirCms::open(legacy_context_config.clone())
                .and_then(|mut cms| cms.prepare_context_with_options(message, options))
            {
                Ok(prepared) => {
                    let mut data = Map::new();
                    data.insert("trace_id".to_string(), json!(prepared.trace_id));
                    data.insert(
                        "included_memory_ids".to_string(),
                        json!(
                            prepared
                                .included_memory_ids
                                .iter()
                                .map(|memory_id| memory_id.0.clone())
                                .collect::<Vec<_>>()
                        ),
                    );
                    data.insert("token_estimate".to_string(), json!(prepared.token_estimate));
                    data.insert("context_prompt".to_string(), json!(prepared.packed_text));
                    Observation {
                        ok: true,
                        content: serde_json::to_string_pretty(&data).unwrap_or_default(),
                        data,
                        error: None,
                    }
                }
                Err(error) => Observation::err(error.to_string(), "CmsError"),
            }
        }),
        json!({"required": ["user_message"], "properties": {"user_message": "string", "mode": "string", "model_context_window": "integer"}}),
        false,
    ))?;

    registry.register(Tool::new(
        "cms_prepare_model_request",
        "Prepare a provider-cacheable model request envelope from CMS-v2 ECM context.",
        Arc::new(move |args| {
            let Some(message) = args.get("message").and_then(Value::as_str) else {
                return Observation::err("Missing message", "ValueError");
            };
            let provider = args
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or("local");
            let model = args
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("unspecified");
            match VegvisirCms::open(cached_prompt_config.clone())
                .and_then(|mut cms| cms.prepare_cached_prompt(message, provider, model))
            {
                Ok(envelope) => {
                    let mut data = Map::new();
                    data.insert("manifest".to_string(), json!(envelope.manifest));
                    data.insert("cache_hint".to_string(), json!(envelope.model_request.cache_hint));
                    Observation {
                        ok: true,
                        content: envelope.model_request.prompt,
                        data,
                        error: None,
                    }
                }
                Err(error) => Observation::err(error.to_string(), "CmsError"),
            }
        }),
        json!({"required": ["message"], "properties": {"message": "string", "provider": "string", "model": "string"}}),
        false,
    ))?;

    Ok(registry)
}

fn context_options_from_args(args: &Map<String, Value>) -> ContextPrepareOptions {
    let mut options = ContextPrepareOptions::default();
    if let Some(mode) = args.get("mode").and_then(Value::as_str) {
        let (context_mode, no_memory) = parse_context_mode(mode);
        options.mode = context_mode;
        if no_memory {
            options
                .metadata
                .insert("memory_mode".to_string(), json!("none"));
        }
    }
    if let Some(context_window) = args.get("model_context_window").and_then(Value::as_u64) {
        options.budget = Some(cms_v2::ecm::ContextBudget {
            max_tokens: context_window as usize,
            ..cms_v2::ecm::ContextBudget::default()
        });
    }
    options
}

fn parse_context_mode(mode: &str) -> (cms_v2::ecm::ContextMode, bool) {
    match mode.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "private" | "no_memory" | "none" | "disabled" | "off" => {
            (cms_v2::ecm::ContextMode::Minimal, true)
        }
        "minimal" => (cms_v2::ecm::ContextMode::Minimal, false),
        "session" => (cms_v2::ecm::ContextMode::Session, false),
        "balanced" | "project" => (cms_v2::ecm::ContextMode::Project, false),
        "deep_project" => (cms_v2::ecm::ContextMode::DeepProject, false),
        "research" => (cms_v2::ecm::ContextMode::Research, false),
        "coding" => (cms_v2::ecm::ContextMode::Coding, false),
        "debugging" | "debug" => (cms_v2::ecm::ContextMode::Debugging, false),
        "architecture" | "arch" => (cms_v2::ecm::ContextMode::Architecture, false),
        "memory_recall" | "recall" => (cms_v2::ecm::ContextMode::MemoryRecall, false),
        "decision_review" | "decision" => (cms_v2::ecm::ContextMode::DecisionReview, false),
        _ => (cms_v2::ecm::ContextMode::Project, false),
    }
}

fn simple_unified_diff(path: &str, old: &str, new: &str) -> String {
    let old_lines = old.lines().collect::<Vec<_>>();
    let new_lines = new.lines().collect::<Vec<_>>();
    let mut diff = String::new();
    diff.push_str(&format!("diff --git a/{path} b/{path}\n"));
    diff.push_str(&format!("--- a/{path}\n"));
    diff.push_str(&format!("+++ b/{path}\n"));
    diff.push_str(&format!(
        "@@ -1,{} +1,{} @@\n",
        old_lines.len(),
        new_lines.len()
    ));
    for line in old_lines {
        diff.push('-');
        diff.push_str(line);
        diff.push('\n');
    }
    for line in new_lines {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    diff
}
