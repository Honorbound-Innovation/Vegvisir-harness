use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use chrono::Utc;
use serde_json::{Map, Value, json};
use skiller::{
    compiler, forge as skiller_forge,
    models::{ForgePassType, ForgeRequestEnvelope, ForgeResponseEnvelope},
    registry as skiller_registry, runtime as skiller_runtime,
};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::{
    guardrails::GuardrailEngine,
    memory::{ContextPrepareOptions, VegvisirCms, VegvisirCmsConfig},
    observability::EventLogger,
    policy::RuntimePolicy,
    sandbox::WorkspaceSandbox,
    subagents::{SubAgentStatus, SubAgentTaskRecord},
    types::{Observation, ToolCall},
};

const LIST_FILES_DEFAULT_LIMIT: usize = 500;
const LIST_FILES_MAX_LIMIT: usize = 2_000;
const CHATGPT_ARCHIVE_EXCERPT_CHARS: usize = 1_800;

fn parse_skiller_forge_pass(value: Option<&str>) -> anyhow::Result<ForgePassType> {
    let raw = value.unwrap_or("skill_expansion").trim();
    match raw {
        "interpretation" | "Interpretation" => Ok(ForgePassType::Interpretation),
        "skill_expansion" | "skill-expansion" | "SkillExpansion" => {
            Ok(ForgePassType::SkillExpansion)
        }
        "safety_and_governance" | "safety-and-governance" | "SafetyAndGovernance" => {
            Ok(ForgePassType::SafetyAndGovernance)
        }
        "eval_generation" | "eval-generation" | "EvalGeneration" => {
            Ok(ForgePassType::EvalGeneration)
        }
        "agent_role_mapping" | "agent-role-mapping" | "AgentRoleMapping" => {
            Ok(ForgePassType::AgentRoleMapping)
        }
        "critique" | "Critique" => Ok(ForgePassType::Critique),
        "verifier_review" | "verifier-review" | "VerifierReview" => {
            Ok(ForgePassType::VerifierReview)
        }
        "registry_readiness" | "registry-readiness" | "RegistryReadiness" => {
            Ok(ForgePassType::RegistryReadiness)
        }
        "skill_inference" | "skill-inference" | "SkillInference" => {
            Ok(ForgePassType::SkillInference)
        }
        "deduplication_and_scope" | "deduplication-and-scope" | "DeduplicationAndScope" => {
            Ok(ForgePassType::DeduplicationAndScope)
        }
        other => anyhow::bail!("Unsupported Skiller Forge pass: {other}"),
    }
}

fn parse_skiller_forge_response(raw: &str) -> anyhow::Result<ForgeResponseEnvelope> {
    let trimmed = raw.trim();
    if let Ok(response) = serde_yaml::from_str::<ForgeResponseEnvelope>(trimmed) {
        return Ok(response);
    }
    if let Some(fenced) = extract_fenced_yaml(trimmed)
        && let Ok(response) = serde_yaml::from_str::<ForgeResponseEnvelope>(&fenced)
    {
        return Ok(response);
    }
    if let Some(start) = trimmed.find("request_id:") {
        return serde_yaml::from_str::<ForgeResponseEnvelope>(&trimmed[start..])
            .map_err(|err| anyhow::anyhow!("failed to parse ForgeResponseEnvelope YAML: {err}"));
    }
    anyhow::bail!("model response did not contain a ForgeResponseEnvelope YAML document")
}

fn extract_fenced_yaml(text: &str) -> Option<String> {
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("```")
            && (trimmed == "```"
                || trimmed.eq_ignore_ascii_case("```yaml")
                || trimmed.eq_ignore_ascii_case("```yml"))
        {
            let mut block = String::new();
            for inner in lines.by_ref() {
                if inner.trim() == "```" {
                    return Some(block);
                }
                block.push_str(inner);
                block.push('\n');
            }
            return Some(block);
        }
    }
    None
}

fn compact_excerpt(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        compact
    } else {
        let mut excerpt = compact
            .chars()
            .take(max_chars.saturating_sub(1))
            .collect::<String>();
        excerpt.push('…');
        excerpt
    }
}

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

    pub fn normalize_args(&self, args: Map<String, Value>) -> Map<String, Value> {
        let properties = self
            .schema
            .get("properties")
            .unwrap_or(&self.schema)
            .as_object()
            .cloned()
            .unwrap_or_default();
        args.into_iter()
            .map(|(key, value)| {
                let expected = properties.get(&key).and_then(|spec| {
                    spec.as_str()
                        .map(str::to_string)
                        .or_else(|| spec.get("type").and_then(Value::as_str).map(str::to_string))
                });
                let value = match expected.as_deref() {
                    Some("string") if !value.is_string() && !value.is_null() => {
                        Value::String(match value {
                            Value::Bool(value) => value.to_string(),
                            Value::Number(value) => value.to_string(),
                            other => serde_json::to_string(&other).unwrap_or_default(),
                        })
                    }
                    Some("integer") if value.is_string() => value
                        .as_str()
                        .and_then(|raw| raw.trim().parse::<i64>().ok())
                        .map(|number| json!(number))
                        .unwrap_or(value),
                    Some("array") if value.is_string() => value
                        .as_str()
                        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                        .filter(Value::is_array)
                        .unwrap_or(value),
                    Some("object") if value.is_string() => value
                        .as_str()
                        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                        .filter(Value::is_object)
                        .unwrap_or(value),
                    _ => value,
                };
                (key, value)
            })
            .collect()
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
            let args = tool.normalize_args(call.args);
            tool.validate_args(&args)?;
            self.guardrails.authorize_tool(tool, &args)?;
            if !self.guardrails.policy.bypass_approvals_and_sandbox {
                self.runtime_policy
                    .authorize_tool(&tool.name, &args, &self.logger)
                    .map_err(anyhow::Error::msg)?;
            }
            self.logger
                .emit("tool_start", json!({"tool": call.name, "args": args}));
            let observation = (tool.handler)(args.clone());
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
                } else if error_text.starts_with("Unknown tool:") {
                    "UnknownTool"
                } else if error_text.contains("Missing required argument") {
                    "InvalidToolArguments"
                } else if error_text.contains(" must be ") {
                    "InvalidToolArguments"
                } else if error_text.contains("not allowed")
                    || error_text.contains("requires human approval")
                {
                    "PermissionDenied"
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

fn compact_text_middle(value: &str, max_bytes: usize, label: &str) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let marker_budget = 160usize;
    let head_bytes = max_bytes.saturating_mul(2) / 3;
    let tail_bytes = max_bytes
        .saturating_sub(head_bytes)
        .saturating_sub(marker_budget);
    let mut head_end = head_bytes.min(value.len());
    while head_end > 0 && !value.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = value.len().saturating_sub(tail_bytes);
    while tail_start < value.len() && !value.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let head = &value[..head_end];
    let tail = &value[tail_start..];
    format!(
        "{head}\n[{label} compacted: omitted {} bytes from middle; original {} bytes, budget {} bytes]\n{tail}",
        value.len().saturating_sub(head.len() + tail.len()),
        value.len(),
        max_bytes
    )
}

fn spawn_command_in_own_process_group(command: &mut Command) -> std::io::Result<Child> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    command.spawn()
}

fn terminate_child_process_group(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
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
    let subagent_data_root = cms_config
        .db_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| sandbox.root.join(".vegvisir"));
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
            let mut command = Command::new(parts[0]);
            command
                .args(&parts[1..])
                .current_dir(&run_root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = match spawn_command_in_own_process_group(&mut command) {
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
                            terminate_child_process_group(&mut child);
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
                        content = compact_text_middle(&content, output_limit, "output");
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
            let mut command = Command::new(parts[0]);
            command
                .args(&parts[1..])
                .current_dir(&test_root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = match spawn_command_in_own_process_group(&mut command) {
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
                            terminate_child_process_group(&mut child);
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
                        content = compact_text_middle(&content, output_limit, "output");
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

    let skiller_compile_sandbox = sandbox.clone();
    registry.register(Tool::new(
        "skiller_compile",
        "Compile local source files/directories into a deterministic Skiller draft bundle and return a Vegvisir Forge request/prompt for default model refinement before final use.",
        Arc::new(move |args| {
            let Some(input) = args.get("input").and_then(Value::as_str) else { return Observation::err("Missing input", "ValueError"); };
            let Some(out) = args.get("out").and_then(Value::as_str) else { return Observation::err("Missing out", "ValueError"); };
            let name = args.get("name").and_then(Value::as_str).unwrap_or("skiller-bundle");
            let domain = args.get("domain").and_then(Value::as_str);
            let input_path = match skiller_compile_sandbox.resolve(input) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            let out_path = match skiller_compile_sandbox.resolve(out) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            match compiler::compile_path(&input_path, name, domain).and_then(|bundle| {
                let bundle_id = bundle.package.bundle_id.clone();
                let skill_count = bundle.skills.len();
                let source_count = bundle.sources.len();
                let forge_request = skiller_forge::build_vegvisir_handoff(
                    &bundle,
                    ForgePassType::SkillExpansion,
                    domain,
                    skill_count.clamp(1, 100),
                );
                let forge_prompt = skiller_forge::vegvisir_prompt_markdown(&forge_request);
                let response_template = skiller_forge::response_template_for(&forge_request);
                skiller_registry::write_bundle(&bundle, &out_path)?;
                Ok((bundle_id, skill_count, source_count, forge_request, forge_prompt, response_template))
            }) {
                Ok((bundle_id, skill_count, source_count, forge_request, forge_prompt, response_template)) => {
                    let request_id = forge_request.request_id.clone();
                    let mut data = Map::new();
                    data.insert("bundle_id".to_string(), json!(bundle_id));
                    data.insert("skill_count".to_string(), json!(skill_count));
                    data.insert("source_count".to_string(), json!(source_count));
                    data.insert("out".to_string(), json!(out));
                    data.insert("deterministic_stage".to_string(), json!(true));
                    data.insert("forge_required_by_default".to_string(), json!(true));
                    data.insert("default_forge_pass".to_string(), json!("SkillExpansion"));
                    data.insert("forge_request_id".to_string(), json!(request_id));
                    data.insert("forge_request".to_string(), serde_json::to_value(&forge_request).unwrap_or(Value::Null));
                    data.insert("forge_response_template".to_string(), serde_json::to_value(&response_template).unwrap_or(Value::Null));
                    data.insert("forge_prompt".to_string(), json!(forge_prompt));
                    data.insert("recommended_apply_tool".to_string(), json!("skiller_forge_apply"));
                    Observation { ok: true, content: format!("Compiled deterministic Skiller draft bundle {bundle_id} to {out} ({skill_count} skills, {source_count} sources). Forge refinement is required by default before treating this as agent-ready: use the included ForgeRequestEnvelope ({request_id}) as model context, then apply the model's ForgeResponseEnvelope with skiller_forge_apply."), data, error: None }
                }
                Err(error) => Observation::err(error.to_string(), "SkillerCompileError"),
            }
        }),
        json!({"required": ["input", "out"], "properties": {"input": "string", "out": "string", "name": "string", "domain": "string"}}),
        true,
    ))?;

    let skiller_compile_cli_help_sandbox = sandbox.clone();
    registry.register(Tool::new(
        "skiller_compile_cli_help",
        "Compile captured CLI help/manpage text into a deterministic Skiller CLI draft bundle and return a Vegvisir Forge request/prompt for default model refinement before final use.",
        Arc::new(move |args| {
            let Some(input) = args.get("input").and_then(Value::as_str) else { return Observation::err("Missing input", "ValueError"); };
            let Some(out) = args.get("out").and_then(Value::as_str) else { return Observation::err("Missing out", "ValueError"); };
            let name = args.get("name").and_then(Value::as_str).unwrap_or("skiller-cli-help-bundle");
            let domain = args.get("domain").and_then(Value::as_str);
            let input_path = match skiller_compile_cli_help_sandbox.resolve(input) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            let out_path = match skiller_compile_cli_help_sandbox.resolve(out) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            match compiler::compile_cli_help(&input_path, name, domain).and_then(|bundle| {
                let bundle_id = bundle.package.bundle_id.clone();
                let skill_count = bundle.skills.len();
                let source_count = bundle.sources.len();
                let forge_request = skiller_forge::build_vegvisir_handoff(
                    &bundle,
                    ForgePassType::SkillExpansion,
                    domain,
                    skill_count.clamp(1, 100),
                );
                let forge_prompt = skiller_forge::vegvisir_prompt_markdown(&forge_request);
                let response_template = skiller_forge::response_template_for(&forge_request);
                skiller_registry::write_bundle(&bundle, &out_path)?;
                Ok((bundle_id, skill_count, source_count, forge_request, forge_prompt, response_template))
            }) {
                Ok((bundle_id, skill_count, source_count, forge_request, forge_prompt, response_template)) => {
                    let request_id = forge_request.request_id.clone();
                    let mut data = Map::new();
                    data.insert("bundle_id".to_string(), json!(bundle_id));
                    data.insert("skill_count".to_string(), json!(skill_count));
                    data.insert("source_count".to_string(), json!(source_count));
                    data.insert("out".to_string(), json!(out));
                    data.insert("deterministic_stage".to_string(), json!(true));
                    data.insert("forge_required_by_default".to_string(), json!(true));
                    data.insert("default_forge_pass".to_string(), json!("SkillExpansion"));
                    data.insert("forge_request_id".to_string(), json!(request_id));
                    data.insert("forge_request".to_string(), serde_json::to_value(&forge_request).unwrap_or(Value::Null));
                    data.insert("forge_response_template".to_string(), serde_json::to_value(&response_template).unwrap_or(Value::Null));
                    data.insert("forge_prompt".to_string(), json!(forge_prompt));
                    data.insert("recommended_apply_tool".to_string(), json!("skiller_forge_apply"));
                    Observation { ok: true, content: format!("Compiled deterministic Skiller CLI help draft bundle {bundle_id} to {out} ({skill_count} skills, {source_count} sources). Forge refinement is required by default before treating this as agent-ready: use the included ForgeRequestEnvelope ({request_id}) as model context, then apply the model's ForgeResponseEnvelope with skiller_forge_apply."), data, error: None }
                }
                Err(error) => Observation::err(error.to_string(), "SkillerCompileError"),
            }
        }),
        json!({"required": ["input", "out"], "properties": {"input": "string", "out": "string", "name": "string", "domain": "string"}}),
        true,
    ))?;

    let skiller_validate_sandbox = sandbox.clone();
    registry.register(Tool::new(
        "skiller_validate",
        "Validate a Skiller skill bundle from inside Vegvisir.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else {
                return Observation::err("Missing bundle", "ValueError");
            };
            let bundle_path = match skiller_validate_sandbox.resolve(bundle) {
                Ok(path) => path,
                Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
            };
            match skiller_registry::validate_bundle_path(&bundle_path) {
                Ok(report) => {
                    let valid = report.valid;
                    let content = serde_json::to_string_pretty(&report)
                        .unwrap_or_else(|_| format!("valid: {valid}"));
                    let mut data = Map::new();
                    data.insert("valid".to_string(), json!(valid));
                    data.insert(
                        "report".to_string(),
                        serde_json::to_value(&report).unwrap_or(Value::Null),
                    );
                    Observation {
                        ok: valid,
                        content,
                        data,
                        error: (!valid).then(|| "SkillerValidationFailed".to_string()),
                    }
                }
                Err(error) => Observation::err(error.to_string(), "SkillerValidateError"),
            }
        }),
        json!({"required": ["bundle"], "properties": {"bundle": "string"}}),
        false,
    ))?;

    let skiller_route_sandbox = sandbox.clone();
    registry.register(Tool::new(
        "skiller_route",
        "Route a user task/query to matching skills in a Skiller bundle.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else { return Observation::err("Missing bundle", "ValueError"); };
            let Some(query) = args.get("query").and_then(Value::as_str) else { return Observation::err("Missing query", "ValueError"); };
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(5).clamp(1, 50) as usize;
            let bundle_path = match skiller_route_sandbox.resolve(bundle) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            match skiller_registry::read_bundle(&bundle_path) {
                Ok(bundle_data) => {
                    let hits = skiller_runtime::route(&bundle_data, query, limit);
                    let content = if hits.is_empty() { "No matching skills.".to_string() } else { hits.iter().map(|hit| format!("{:.3}\t{}\t{}", hit.score, hit.skill_id, hit.title)).collect::<Vec<_>>().join("\n") };
                    let mut data = Map::new();
                    data.insert("hits".to_string(), json!(hits.iter().map(|hit| json!({"score": hit.score, "skill_id": hit.skill_id, "title": hit.title})).collect::<Vec<_>>()));
                    Observation { ok: true, content, data, error: None }
                }
                Err(error) => Observation::err(error.to_string(), "SkillerRouteError"),
            }
        }),
        json!({"required": ["bundle", "query"], "properties": {"bundle": "string", "query": "string", "limit": "integer"}}),
        false,
    ))?;

    let skiller_load_sandbox = sandbox.clone();
    registry.register(Tool::new(
        "skiller_load",
        "Materialize a Skiller skill card/body/extended context from inside Vegvisir.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else { return Observation::err("Missing bundle", "ValueError"); };
            let Some(skill_id) = args.get("skill_id").and_then(Value::as_str) else { return Observation::err("Missing skill_id", "ValueError"); };
            let mode = match args.get("mode").and_then(Value::as_str).unwrap_or("body").trim().to_ascii_lowercase().as_str() {
                "card" => skiller_runtime::LoadMode::Card,
                "body" => skiller_runtime::LoadMode::Body,
                "extended" => skiller_runtime::LoadMode::Extended,
                other => return Observation::err(format!("Unknown mode: {other}"), "ValueError"),
            };
            let bundle_path = match skiller_load_sandbox.resolve(bundle) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            match skiller_registry::read_bundle(&bundle_path).and_then(|bundle_data| skiller_runtime::load_skill(&bundle_data, skill_id, mode)) {
                Ok(content) => Observation::ok(content),
                Err(error) => Observation::err(error.to_string(), "SkillerLoadError"),
            }
        }),
        json!({"required": ["bundle", "skill_id"], "properties": {"bundle": "string", "skill_id": "string", "mode": "string"}}),
        false,
    ))?;

    let skiller_eval_sandbox = sandbox.clone();
    registry.register(Tool::new(
        "skiller_eval",
        "Run deterministic structural evals for a Skiller bundle from inside Vegvisir.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else {
                return Observation::err("Missing bundle", "ValueError");
            };
            let bundle_path = match skiller_eval_sandbox.resolve(bundle) {
                Ok(path) => path,
                Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
            };
            match skiller_registry::read_bundle(&bundle_path) {
                Ok(bundle_data) => {
                    let report = skiller_registry::eval_bundle(&bundle_data);
                    let passed = report.passed;
                    let content = serde_json::to_string_pretty(&report)
                        .unwrap_or_else(|_| format!("passed: {passed}"));
                    let mut data = Map::new();
                    data.insert("passed".to_string(), json!(passed));
                    data.insert(
                        "report".to_string(),
                        serde_json::to_value(&report).unwrap_or(Value::Null),
                    );
                    Observation {
                        ok: passed,
                        content,
                        data,
                        error: (!passed).then(|| "SkillerEvalFailed".to_string()),
                    }
                }
                Err(error) => Observation::err(error.to_string(), "SkillerEvalError"),
            }
        }),
        json!({"required": ["bundle"], "properties": {"bundle": "string"}}),
        false,
    ))?;

    let skiller_readiness_sandbox = sandbox.clone();
    registry.register(Tool::new(
        "skiller_readiness",
        "Assess Skiller bundle registry publication readiness from inside Vegvisir.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else {
                return Observation::err("Missing bundle", "ValueError");
            };
            let bundle_path = match skiller_readiness_sandbox.resolve(bundle) {
                Ok(path) => path,
                Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
            };
            match skiller_registry::read_bundle(&bundle_path) {
                Ok(bundle_data) => {
                    let report = skiller_registry::readiness_report(&bundle_data);
                    let ready = report.ready;
                    let content = serde_json::to_string_pretty(&report)
                        .unwrap_or_else(|_| format!("ready: {ready}"));
                    let mut data = Map::new();
                    data.insert("ready".to_string(), json!(ready));
                    data.insert(
                        "report".to_string(),
                        serde_json::to_value(&report).unwrap_or(Value::Null),
                    );
                    Observation {
                        ok: true,
                        content,
                        data,
                        error: None,
                    }
                }
                Err(error) => Observation::err(error.to_string(), "SkillerReadinessError"),
            }
        }),
        json!({"required": ["bundle"], "properties": {"bundle": "string"}}),
        false,
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

    let skiller_forge_request_sandbox = sandbox.clone();
    registry.register(Tool::new(
        "skiller_forge_request",
        "Build a strict Vegvisir-provider Skiller Forge request envelope and model prompt for native agent/provider execution.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else { return Observation::err("Missing bundle", "ValueError"); };
            let pass = match parse_skiller_forge_pass(args.get("pass").and_then(Value::as_str)) { Ok(pass) => pass, Err(error) => return Observation::err(error.to_string(), "ValueError") };
            let domain_profile = args.get("domain_profile").and_then(Value::as_str);
            let max_skills = args.get("max_skills").and_then(Value::as_u64).unwrap_or(8).clamp(1, 100) as usize;
            let bundle_path = match skiller_forge_request_sandbox.resolve(bundle) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            match skiller_registry::read_bundle(&bundle_path).map(|bundle_data| skiller_forge::build_vegvisir_handoff(&bundle_data, pass, domain_profile, max_skills)) {
                Ok(request) => {
                    let prompt = skiller_forge::vegvisir_prompt_markdown(&request);
                    let template = skiller_forge::response_template_for(&request);
                    let mut data = Map::new();
                    data.insert("request_id".to_string(), json!(request.request_id));
                    data.insert("provider".to_string(), json!(request.provider));
                    data.insert("pass_type".to_string(), json!(format!("{:?}", request.pass_type)));
                    data.insert("selected_skill_count".to_string(), json!(request.candidate_skills.len()));
                    data.insert("request".to_string(), serde_json::to_value(&request).unwrap_or(Value::Null));
                    data.insert("response_template".to_string(), serde_json::to_value(&template).unwrap_or(Value::Null));
                    data.insert("prompt".to_string(), json!(prompt));
                    Observation { ok: true, content: prompt, data, error: None }
                }
                Err(error) => Observation::err(error.to_string(), "SkillerForgeRequestError"),
            }
        }),
        json!({"required": ["bundle"], "properties": {"bundle": "string", "pass": "string", "domain_profile": "string", "max_skills": "integer"}}),
        false,
    ))?;

    let skiller_forge_apply_sandbox = sandbox.clone();
    registry.register(Tool::new(
        "skiller_forge_apply",
        "Validate and apply a Vegvisir-generated Skiller Forge response envelope to a bundle, writing the reviewed output bundle inside the workspace.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else { return Observation::err("Missing bundle", "ValueError"); };
            let Some(out) = args.get("out").and_then(Value::as_str) else { return Observation::err("Missing out", "ValueError"); };
            let request_value = match args.get("request") { Some(value) => value.clone(), None => return Observation::err("Missing request", "ValueError") };
            let response_text = args.get("response").and_then(Value::as_str);
            let response_value = args.get("response_envelope").cloned();
            let bundle_path = match skiller_forge_apply_sandbox.resolve(bundle) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            let out_path = match skiller_forge_apply_sandbox.resolve(out) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            let request = match serde_json::from_value(request_value).or_else(|json_err| serde_yaml::from_str::<ForgeRequestEnvelope>(args.get("request").and_then(Value::as_str).unwrap_or("")) .map_err(|yaml_err| anyhow::anyhow!("failed to parse Forge request as JSON ({json_err}) or YAML ({yaml_err})"))) {
                Ok(request) => request,
                Err(error) => return Observation::err(error.to_string(), "ValueError"),
            };
            let response = match response_value {
                Some(value) => match serde_json::from_value::<ForgeResponseEnvelope>(value) {
                    Ok(response) => response,
                    Err(error) => return Observation::err(format!("failed to parse response_envelope: {error}"), "ValueError"),
                },
                None => match response_text {
                    Some(text) => match parse_skiller_forge_response(text) {
                        Ok(response) => response,
                        Err(error) => return Observation::err(error.to_string(), "ValueError"),
                    },
                    None => return Observation::err("Missing response or response_envelope", "ValueError"),
                }
            };
            match skiller_registry::read_bundle(&bundle_path)
                .and_then(|bundle_data| skiller_forge::apply_external_response_with_report(bundle_data, request, response))
                .and_then(|(bundle_data, report)| {
                    skiller_registry::write_bundle(&bundle_data, &out_path)?;
                    Ok((bundle_data, report))
                })
            {
                Ok((bundle_data, report)) => {
                    let mut data = Map::new();
                    data.insert("bundle_id".to_string(), json!(bundle_data.package.bundle_id));
                    data.insert("out".to_string(), json!(out));
                    data.insert("apply_report".to_string(), serde_json::to_value(&report).unwrap_or(Value::Null));
                    Observation { ok: true, content: format!("Applied Vegvisir Skiller Forge response {} to {out} (skills: {} -> {}, human_review_required={}).", report.request_id, report.before_skill_count, report.after_skill_count, report.required_human_review), data, error: None }
                }
                Err(error) => Observation::err(error.to_string(), "SkillerForgeApplyError"),
            }
        }),
        json!({"required": ["bundle", "out", "request"], "properties": {"bundle": "string", "out": "string", "request": "object", "response": "string", "response_envelope": "object"}}),
        true,
    ))?;

    let chatgpt_archive_config = cms_config.clone();
    registry.register(Tool::new(
        "cms_search_chatgpt_archive",
        "Search the explicit-only imported ChatGPT archive corpus through CMS-v2. Use only when the user specifically asks about prior ChatGPT history/ideas or when an explicit reference-archive search is warranted; this does not search active project/global memory. Returns answer-ready excerpts with conversation/chunk citations.",
        Arc::new(move |args| {
            let Some(query) = args.get("query").and_then(Value::as_str) else {
                return Observation::err("Missing query", "ValueError");
            };
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(5) as usize;
            let excerpt_chars = args
                .get("excerpt_chars")
                .and_then(Value::as_u64)
                .map(|value| value as usize)
                .unwrap_or(CHATGPT_ARCHIVE_EXCERPT_CHARS)
                .clamp(200, 8_000);
            match VegvisirCms::open(chatgpt_archive_config.clone())
                .and_then(|cms| cms.retrieve_chatgpt_archive(query, limit))
            {
                Ok(bundle) => {
                    let mut structured_results = Vec::new();
                    let summaries = bundle
                        .results
                        .iter()
                        .enumerate()
                        .map(|(index, result)| {
                            let conversation = result
                                .memory
                                .metadata
                                .get("conversation_title")
                                .and_then(Value::as_str)
                                .unwrap_or(&result.memory.title);
                            let conversation_id = result
                                .memory
                                .metadata
                                .get("conversation_id")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let chunk = result
                                .memory
                                .metadata
                                .get("chunk_index")
                                .and_then(Value::as_str)
                                .unwrap_or("?");
                            let total = result
                                .memory
                                .metadata
                                .get("chunk_total")
                                .and_then(Value::as_str)
                                .unwrap_or("?");
                            let source_hash = result
                                .memory
                                .metadata
                                .get("source_hash")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let excerpt = compact_excerpt(&result.memory.body, excerpt_chars);
                            structured_results.push(json!({
                                "rank": index + 1,
                                "id": result.memory.id.0.clone(),
                                "title": result.memory.title.clone(),
                                "conversation_title": conversation,
                                "conversation_id": conversation_id,
                                "chunk_index": chunk,
                                "chunk_total": total,
                                "score": result.score,
                                "source_mode": format!("{:?}", result.source_mode),
                                "reason": result.reason.clone(),
                                "summary": result.memory.summary.clone(),
                                "excerpt": excerpt,
                                "source_hash": source_hash,
                                "metadata": result.memory.metadata.clone(),
                                "tags": result.memory.tags.clone(),
                                "claims": result.memory.claims.clone(),
                                "links": result.memory.links.clone(),
                            }));
                            let citation = if conversation_id.is_empty() {
                                format!("{} chunk {}/{}", conversation, chunk, total)
                            } else {
                                format!("{} ({}) chunk {}/{}", conversation, conversation_id, chunk, total)
                            };
                            format!(
                                "{}. {} [{:?} score {:.3}]\n   id: {}{}\n   summary: {}\n   excerpt: {}",
                                index + 1,
                                citation,
                                result.source_mode,
                                result.score,
                                result.memory.id.0,
                                if source_hash.is_empty() { String::new() } else { format!("\n   source_hash: {source_hash}") },
                                result.memory.summary,
                                excerpt,
                            )
                        })
                        .collect::<Vec<_>>();
                    let mut data = Map::new();
                    data.insert("results".to_string(), json!(structured_results));
                    data.insert("raw_results".to_string(), json!(bundle.results));
                    data.insert("trace".to_string(), json!(bundle.trace));
                    data.insert("corpus".to_string(), json!("chatgpt_archive"));
                    data.insert("retrieval_policy".to_string(), json!("explicit_only"));
                    data.insert("excerpt_chars".to_string(), json!(excerpt_chars));
                    Observation {
                        ok: true,
                        content: if summaries.is_empty() {
                            "No ChatGPT archive memories matched.".to_string()
                        } else {
                            summaries.join("\n\n")
                        },
                        data,
                        error: None,
                    }
                }
                Err(error) => Observation::err(error.to_string(), "CmsError"),
            }
        }),
        json!({
            "required": ["query"],
            "properties": {
                "query": "string",
                "limit": "integer",
                "excerpt_chars": "integer"
            }
        }),
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

    let subagent_root = sandbox.root.clone();
    let subagent_sandbox = sandbox.clone();
    registry.register(Tool::new(
        "spawn_subagent",
        "Delegate a bounded task to a background Vegvisir child agent and record it on the subagent board.",
        Arc::new(move |args| {
            let Some(goal) = args.get("goal").and_then(Value::as_str).map(str::trim) else {
                return Observation::err("Missing goal", "ValueError");
            };
            if goal.is_empty() {
                return Observation::err("Subagent goal must not be empty", "ValueError");
            }
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("subagent")
                .to_string();
            let workspace = match args.get("workspace").and_then(Value::as_str) {
                Some(path) => match subagent_sandbox.resolve(path) {
                    Ok(path) => path,
                    Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
                },
                None => subagent_root.clone(),
            };
            let max_steps = args
                .get("max_steps")
                .and_then(Value::as_u64)
                .unwrap_or(4)
                .clamp(1, 32)
                .to_string();
            let provider = args
                .get("provider")
                .and_then(Value::as_str)
                .map(str::to_string);
            let model = args
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string);
            let agent = args
                .get("agent")
                .and_then(Value::as_str)
                .map(str::to_string);

            let board_path = subagent_data_root.join("subagents.json");
            let record = SubAgentTaskRecord {
                id: Uuid::new_v4().to_string(),
                name: name.clone(),
                workspace: workspace.clone(),
                goal: goal.to_string(),
                status: SubAgentStatus::Queued,
                created_at: Utc::now(),
                started_at: None,
                finished_at: None,
                checkpoint: None,
                final_answer: None,
                error: None,
            };
            if let Err(error) = upsert_subagent_record(&board_path, record.clone()) {
                return Observation::err(error.to_string(), "SubagentBoardError");
            }

            let child_record = record.clone();
            let child_goal = goal.to_string();
            thread::spawn(move || {
                run_spawned_subagent(
                    board_path,
                    child_record,
                    child_goal,
                    workspace,
                    max_steps,
                    provider,
                    model,
                    agent,
                );
            });

            let mut data = Map::new();
            data.insert("id".to_string(), json!(record.id));
            data.insert("name".to_string(), json!(record.name));
            data.insert("workspace".to_string(), json!(record.workspace));
            data.insert("board_path".to_string(), json!(subagent_data_root.join("subagents.json")));
            Observation {
                ok: true,
                content: format!(
                    "Spawned subagent {} ({name}). Use `/subagents show {}` to inspect status.",
                    data["id"].as_str().unwrap_or(""),
                    data["id"].as_str().unwrap_or("")
                ),
                data,
                error: None,
            }
        }),
        json!({
            "required": ["goal"],
            "properties": {
                "goal": "string",
                "name": "string",
                "workspace": "string",
                "max_steps": "integer",
                "provider": "string",
                "model": "string",
                "agent": "string"
            }
        }),
        true,
    ))?;

    Ok(registry)
}

fn run_spawned_subagent(
    board_path: PathBuf,
    mut record: SubAgentTaskRecord,
    goal: String,
    workspace: PathBuf,
    max_steps: String,
    provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
) {
    record.status = SubAgentStatus::Running;
    record.started_at = Some(Utc::now());
    let _ = upsert_subagent_record(&board_path, record.clone());

    let result = (|| -> anyhow::Result<String> {
        let executable = std::env::current_exe()?;
        let mut command = Command::new(executable);
        command.arg("--json");
        if let Some(provider) = provider {
            command.arg("--provider").arg(provider);
        }
        if let Some(model) = model {
            command.arg("--model").arg(model);
        }
        command
            .arg("run")
            .arg(goal)
            .arg("--workspace")
            .arg(workspace)
            .arg("--max-steps")
            .arg(max_steps);
        if let Some(agent) = agent {
            command.arg("--agent").arg(agent);
        }
        let output = command.output()?;
        let mut text = String::new();
        text.push_str(&String::from_utf8_lossy(&output.stdout));
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        if !output.status.success() {
            anyhow::bail!("{}", text.trim());
        }
        Ok(text)
    })();

    match result {
        Ok(output) => {
            record.status = SubAgentStatus::Completed;
            record.finished_at = Some(Utc::now());
            record.final_answer = Some(output);
            record.error = None;
        }
        Err(error) => {
            record.status = SubAgentStatus::Failed;
            record.finished_at = Some(Utc::now());
            record.error = Some(error.to_string());
        }
    }
    let _ = upsert_subagent_record(&board_path, record);
}

fn upsert_subagent_record(path: &Path, record: SubAgentTaskRecord) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut records = if path.exists() {
        serde_json::from_str::<Vec<SubAgentTaskRecord>>(&std::fs::read_to_string(path)?)?
    } else {
        Vec::new()
    };
    if let Some(existing) = records.iter_mut().find(|existing| existing.id == record.id) {
        *existing = record;
    } else {
        records.push(record);
    }
    std::fs::write(path, serde_json::to_string_pretty(&records)?)?;
    Ok(())
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

#[cfg(test)]
mod skiller_tool_tests {
    use super::*;
    use crate::memory::VegvisirCmsConfig;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn skiller_tools_compile_validate_route_and_load_cli_help() -> anyhow::Result<()> {
        let workspace = TempDir::new()?;
        std::fs::write(
            workspace.path().join("safebackup-help.txt"),
            "safebackup - safe local backup utility\n\nUsage:\n  safebackup scan <path>\n  safebackup delete <backup-id> --yes\n\nCommands:\n  scan       Inspect a directory. Read-only.\n  delete     Delete a backup permanently. Destructive operation. Requires --yes.\n",
        )?;
        let cms_config = VegvisirCmsConfig {
            db_path: workspace.path().join("cms-v2.sqlite3"),
            user_id: "test-user".to_string(),
            project_id: Some("test-project".to_string()),
            context_mode: cms_v2::ecm::ContextMode::Project,
            commit_writebacks: true,
        };
        let mut executor = ToolExecutor {
            registry: build_builtin_registry_with_cms_and_mode(
                workspace.path(),
                cms_config,
                false,
            )?,
            guardrails: GuardrailEngine {
                policy: crate::guardrails::PermissionPolicy {
                    allow_risky_tools: true,
                    require_human_approval: false,
                    ..crate::guardrails::PermissionPolicy::default()
                },
                approvals: crate::guardrails::ApprovalLedger::default(),
            },
            runtime_policy: RuntimePolicy::default(),
            logger: EventLogger::new(None),
        };

        let compile = executor.execute(ToolCall {
            name: "skiller_compile_cli_help".to_string(),
            args: serde_json::from_value(json!({
                "input": "safebackup-help.txt",
                "out": "bundle",
                "name": "safebackup",
                "domain": "cli-safety"
            }))?,
        });
        assert!(compile.ok, "{}", compile.content);
        assert!(
            compile
                .content
                .contains("Forge refinement is required by default")
        );
        assert_eq!(
            compile.data.get("forge_required_by_default"),
            Some(&json!(true))
        );
        assert_eq!(
            compile.data.get("default_forge_pass"),
            Some(&json!("SkillExpansion"))
        );
        assert_eq!(
            compile.data.get("recommended_apply_tool"),
            Some(&json!("skiller_forge_apply"))
        );
        assert!(compile.data.get("forge_request").is_some());
        assert!(compile.data.get("forge_response_template").is_some());
        assert!(
            compile
                .data
                .get("forge_prompt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("Vegvisir Skiller Forge Request")
        );
        assert!(workspace.path().join("bundle/package.yaml").exists());

        let validate = executor.execute(ToolCall {
            name: "skiller_validate".to_string(),
            args: serde_json::from_value(json!({"bundle": "bundle"}))?,
        });
        assert!(validate.ok, "{}", validate.content);

        let route = executor.execute(ToolCall {
            name: "skiller_route".to_string(),
            args: serde_json::from_value(
                json!({"bundle": "bundle", "query": "cli workflow overview", "limit": 3}),
            )?,
        });
        assert!(route.ok, "{}", route.content);
        let hits = route
            .data
            .get("hits")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(!hits.is_empty(), "expected route hits: {}", route.content);
        let skill_id = hits[0]
            .get("skill_id")
            .and_then(Value::as_str)
            .expect("route hit skill_id")
            .to_string();

        let load = executor.execute(ToolCall {
            name: "skiller_load".to_string(),
            args: serde_json::from_value(
                json!({"bundle": "bundle", "skill_id": skill_id, "mode": "extended"}),
            )?,
        });
        assert!(load.ok, "{}", load.content);
        assert!(load.content.contains("safebackup") || load.content.contains("delete"));

        Ok(())
    }
    #[test]
    fn skiller_tools_build_and_apply_vegvisir_forge_envelope() -> anyhow::Result<()> {
        let workspace = TempDir::new()?;
        std::fs::write(
            workspace.path().join("release.md"),
            "# Release workflow\n\nRun tests before release. Do not claim verification passed without evidence. Publishing requires explicit approval.\n",
        )?;
        let registry = build_builtin_registry_with_cms_and_mode(
            workspace.path(),
            VegvisirCmsConfig::for_workspace(workspace.path()),
            true,
        )?;
        let mut executor = ToolExecutor {
            registry,
            guardrails: GuardrailEngine {
                policy: crate::guardrails::PermissionPolicy {
                    allow_risky_tools: true,
                    require_human_approval: false,
                    ..crate::guardrails::PermissionPolicy::default()
                },
                approvals: crate::guardrails::ApprovalLedger::default(),
            },
            runtime_policy: RuntimePolicy::default(),
            logger: EventLogger::new(None),
        };

        let compile = executor.execute(ToolCall {
            name: "skiller_compile".to_string(),
            args: serde_json::from_value(json!({
                "input": "release.md",
                "out": "bundle",
                "name": "release",
                "domain": "release-management"
            }))?,
        });
        assert!(compile.ok, "{}", compile.content);
        assert!(
            compile
                .content
                .contains("Forge refinement is required by default")
        );
        assert_eq!(
            compile.data.get("forge_required_by_default"),
            Some(&json!(true))
        );
        assert!(compile.data.get("forge_request").is_some());
        assert!(compile.data.get("forge_response_template").is_some());

        let request_obs = executor.execute(ToolCall {
            name: "skiller_forge_request".to_string(),
            args: serde_json::from_value(json!({
                "bundle": "bundle",
                "pass": "skill_expansion",
                "max_skills": 2
            }))?,
        });
        assert!(request_obs.ok, "{}", request_obs.content);
        assert!(request_obs.content.contains("ForgeResponseEnvelope"));
        assert_eq!(request_obs.data.get("provider"), Some(&json!("vegvisir")));
        let request = request_obs
            .data
            .get("request")
            .cloned()
            .expect("request data");
        let response_template = request_obs
            .data
            .get("response_template")
            .cloned()
            .expect("response template data");

        let apply = executor.execute(ToolCall {
            name: "skiller_forge_apply".to_string(),
            args: serde_json::from_value(json!({
                "bundle": "bundle",
                "out": "forged-bundle",
                "request": request,
                "response_envelope": response_template
            }))?,
        });
        assert!(apply.ok, "{}", apply.content);
        assert!(workspace.path().join("forged-bundle/package.yaml").exists());
        assert!(apply.data.get("apply_report").is_some());
        Ok(())
    }
}
