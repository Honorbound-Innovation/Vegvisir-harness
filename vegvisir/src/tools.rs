use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, hash_map::DefaultHasher},
    fs::File,
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::Arc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use chrono::Utc;
use msp_client::{
    ImportSkillerBundleRequest as MspImportSkillerBundleRequest, LoadMode as MspLoadMode,
    MspClient, SearchRequest as MspSearchRequest,
};
use serde_json::{Map, Value, json};
use skiller::{
    compiler, forge as skiller_forge,
    models::{ForgePassType, ForgeRequestEnvelope, ForgeResponseEnvelope},
    registry as skiller_registry, runtime as skiller_runtime, semantic as skiller_semantic,
};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::{
    command_sandbox::{CommandSandboxConfig, build_sandboxed_command},
    core::repair_model_for_provider,
    environment::get_env,
    guardrails::GuardrailEngine,
    memory::{ContextPrepareOptions, VegvisirCms, VegvisirCmsConfig},
    observability::EventLogger,
    policy::{RuntimePolicy, RuntimeToolMetadata},
    privilege,
    sandbox::WorkspaceSandbox,
    subagents::{
        SubAgentFileChange, SubAgentFileChangeKind, SubAgentFileOwnership, SubAgentObservability,
        SubAgentObservedEvent, SubAgentObservedEventKind, SubAgentStatus, SubAgentTaskRecord,
        SubAgentWorkBudget,
    },
    types::{Observation, ToolCall},
};

const LIST_FILES_DEFAULT_LIMIT: usize = 500;
const LIST_FILES_MAX_LIMIT: usize = 2_000;
const CHATGPT_ARCHIVE_EXCERPT_CHARS: usize = 1_800;
const SUBAGENT_DIFF_TEXT_MAX_BYTES: u64 = 1024 * 1024;
pub const DEFAULT_ACTIVE_SUBAGENT_LIMIT: usize = 3;

const COMMAND_STREAM_READ_CHUNK_BYTES: usize = 8 * 1024;
const COMMAND_STREAM_LIVE_MAX_BYTES_PER_STREAM: usize = 128 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandOutputChunk {
    pub stream: String,
    pub chunk: String,
    pub truncated: bool,
}

pub type CommandOutputSink = Arc<dyn Fn(CommandOutputChunk) + Send + Sync>;

thread_local! {
    static COMMAND_OUTPUT_SINK: RefCell<Option<CommandOutputSink>> = RefCell::new(None);
}

pub fn with_command_output_sink<T>(sink: Option<CommandOutputSink>, f: impl FnOnce() -> T) -> T {
    struct SinkGuard(Option<CommandOutputSink>);

    impl Drop for SinkGuard {
        fn drop(&mut self) {
            let previous = self.0.take();
            COMMAND_OUTPUT_SINK.with(|slot| {
                *slot.borrow_mut() = previous;
            });
        }
    }

    let previous = COMMAND_OUTPUT_SINK.with(|slot| slot.replace(sink));
    let _guard = SinkGuard(previous);
    f()
}

fn current_command_output_sink() -> Option<CommandOutputSink> {
    COMMAND_OUTPUT_SINK.with(|slot| slot.borrow().clone())
}

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
        "script_generation" | "script-generation" | "ScriptGeneration" => {
            Ok(ForgePassType::ScriptGeneration)
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

fn skiller_bundle_handoff_observation_data(
    bundle: &skiller::models::SkillBundle,
    pass: ForgePassType,
    domain: Option<&str>,
    target: &ResolvedSkillerForgeModelTarget,
) -> (ForgeRequestEnvelope, String, String, Value) {
    let skill_count = bundle.skills.len();
    let mut forge_request =
        skiller_forge::build_vegvisir_handoff(bundle, pass, domain, skill_count.clamp(1, 100));
    apply_skiller_forge_model_target(&mut forge_request, target);
    let forge_system_prompt =
        skiller_forge::skiller_specialized_vegvisir_system_prompt().to_string();
    let forge_prompt = skiller_forge::vegvisir_prompt_markdown(&forge_request);
    let response_template =
        serde_json::to_value(skiller_forge::response_template_for(&forge_request))
            .unwrap_or(Value::Null);
    (
        forge_request,
        forge_system_prompt,
        forge_prompt,
        response_template,
    )
}

fn add_skiller_forge_observation_data(
    data: &mut Map<String, Value>,
    forge_request: &ForgeRequestEnvelope,
    forge_system_prompt: String,
    forge_prompt: String,
    response_template: Value,
    target: &ResolvedSkillerForgeModelTarget,
) {
    data.insert("forge_required_by_default".to_string(), json!(true));
    data.insert("forge_requires_provider_model".to_string(), json!(true));
    data.insert(
        "default_forge_model_provider".to_string(),
        json!(target.provider),
    );
    data.insert("default_forge_model".to_string(), json!(target.model));
    data.insert(
        "forge_model_target_source".to_string(),
        json!(target.source),
    );
    data.insert(
        "default_forge_pass".to_string(),
        json!(format!("{:?}", forge_request.pass_type)),
    );
    data.insert(
        "forge_request_id".to_string(),
        json!(forge_request.request_id),
    );
    data.insert(
        "forge_default_objective".to_string(),
        json!(skiller_forge::skiller_default_forge_objective()),
    );
    data.insert(
        "forge_system_prompt".to_string(),
        json!(forge_system_prompt),
    );
    data.insert(
        "forge_request".to_string(),
        serde_json::to_value(forge_request).unwrap_or(Value::Null),
    );
    data.insert("forge_response_template".to_string(), response_template);
    data.insert("forge_prompt".to_string(), json!(forge_prompt));
    data.insert(
        "recommended_apply_tool".to_string(),
        json!("skiller_forge_apply"),
    );
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

fn vegvisir_data_root_from_cms_config(config: &VegvisirCmsConfig) -> PathBuf {
    config
        .db_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(crate::memory::default_vegvisir_data_root)
}

fn default_msp_registry_path(data_root: &Path) -> PathBuf {
    get_env("VEGVISIR_MSP_REGISTRY")
        .map(PathBuf::from)
        .unwrap_or_else(|| data_root.join("msp").join("registry"))
}

fn default_skiller_bundle_root(data_root: &Path) -> PathBuf {
    get_env("VEGVISIR_SKILLER_BUNDLE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| data_root.join("skiller").join("bundles"))
}

fn msp_registry_path(args: &Map<String, Value>, data_root: &Path) -> PathBuf {
    args.get("registry")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_msp_registry_path(data_root))
}

fn looks_like_explicit_local_path(path: &str) -> bool {
    path == "."
        || path == ".."
        || path.starts_with("./")
        || path.starts_with("../")
        || path.starts_with('/')
        || path.starts_with('~')
        || path.contains(std::path::MAIN_SEPARATOR)
        || (std::path::MAIN_SEPARATOR != '/' && path.contains('/'))
}

fn skiller_bundle_output_path(
    args: &Map<String, Value>,
    data_root: &Path,
    default_name: &str,
) -> PathBuf {
    let raw = args
        .get("out")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(default_name);
    if looks_like_explicit_local_path(raw) {
        PathBuf::from(raw)
    } else {
        default_skiller_bundle_root(data_root).join(raw)
    }
}

fn skiller_bundle_reference_path(raw: &str, data_root: &Path) -> PathBuf {
    if looks_like_explicit_local_path(raw) {
        PathBuf::from(raw)
    } else {
        default_skiller_bundle_root(data_root).join(raw)
    }
}

fn resolve_workspace_or_global_path(
    sandbox: &WorkspaceSandbox,
    path: impl AsRef<Path>,
    allowed_global_roots: &[PathBuf],
) -> anyhow::Result<PathBuf> {
    match sandbox.resolve(path.as_ref()) {
        Ok(resolved) => Ok(resolved),
        Err(workspace_error) => {
            if !path.as_ref().is_absolute() {
                return Err(workspace_error);
            }
            let resolved = resolve_existing_or_missing(path.as_ref())?;
            for root in allowed_global_roots {
                let root = resolve_existing_or_missing(root)?;
                if resolved == root || resolved.starts_with(&root) {
                    return Ok(resolved);
                }
            }
            Err(workspace_error)
        }
    }
}

fn resolve_existing_or_missing(path: &Path) -> anyhow::Result<PathBuf> {
    if path.exists() {
        return Ok(path.canonicalize()?);
    }
    let mut existing = path;
    let mut missing_components = Vec::new();
    while !existing.exists() {
        let Some(parent) = existing.parent() else {
            anyhow::bail!("Path has no existing ancestor: {}", path.display());
        };
        if let Some(name) = existing.file_name() {
            missing_components.push(name.to_os_string());
        }
        existing = parent;
    }
    let mut resolved = existing.canonicalize()?;
    for component in missing_components.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn msp_json_observation(value: impl serde::Serialize) -> Observation {
    match serde_json::to_value(value) {
        Ok(Value::Object(data)) => {
            let content = serde_json::to_string_pretty(&data).unwrap_or_default();
            Observation {
                ok: true,
                content,
                data,
                error: None,
            }
        }
        Ok(value) => {
            let content = serde_json::to_string_pretty(&value).unwrap_or_default();
            let mut data = Map::new();
            data.insert("value".to_string(), value);
            Observation {
                ok: true,
                content,
                data,
                error: None,
            }
        }
        Err(error) => Observation::err(error.to_string(), "MspSerializationError"),
    }
}

fn json_string_array(args: &Map<String, Value>, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
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
        validate_tool_arguments(&self.name, args, &self.schema)
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

fn validate_tool_arguments(
    tool_name: &str,
    args: &Map<String, Value>,
    schema: &Value,
) -> anyhow::Result<()> {
    let value = Value::Object(args.clone());
    validate_json_schema_value(tool_name, tool_name.to_string(), &value, schema)
}

fn validate_json_schema_value(
    tool_name: &str,
    path: String,
    value: &Value,
    schema: &Value,
) -> anyhow::Result<()> {
    if schema.is_null() || schema == &json!(true) {
        return Ok(());
    }
    if schema == &json!(false) {
        anyhow::bail!("{path} is not allowed by schema for {tool_name}");
    }
    if let Some(expected) = schema.as_str() {
        return validate_schema_type(tool_name, &path, value, expected);
    }
    let Some(object) = schema.as_object() else {
        return Ok(());
    };

    validate_schema_combinators(tool_name, &path, value, schema, object)?;
    validate_schema_enum(tool_name, &path, value, object)?;
    validate_schema_type_constraints(tool_name, &path, value, object)?;
    validate_schema_object_constraints(tool_name, &path, value, object)?;
    validate_schema_array_constraints(tool_name, &path, value, object)?;
    validate_schema_string_constraints(tool_name, &path, value, object)?;
    validate_schema_number_constraints(tool_name, &path, value, object)?;
    Ok(())
}

fn validate_schema_combinators(
    tool_name: &str,
    path: &str,
    value: &Value,
    schema: &Value,
    object: &Map<String, Value>,
) -> anyhow::Result<()> {
    if let Some(all_of) = object.get("allOf").and_then(Value::as_array) {
        for branch in all_of {
            validate_json_schema_value(tool_name, path.to_string(), value, branch)?;
        }
    }
    if let Some(any_of) = object.get("anyOf").and_then(Value::as_array) {
        let mut errors = Vec::new();
        for branch in any_of {
            match validate_json_schema_value(tool_name, path.to_string(), value, branch) {
                Ok(()) => return Ok(()),
                Err(error) => errors.push(error.to_string()),
            }
        }
        anyhow::bail!(
            "{path} must match at least one anyOf schema for {tool_name}: {}",
            errors.join("; ")
        );
    }
    if let Some(one_of) = object.get("oneOf").and_then(Value::as_array) {
        let matches = one_of
            .iter()
            .filter(|branch| {
                validate_json_schema_value(tool_name, path.to_string(), value, branch).is_ok()
            })
            .count();
        if matches != 1 {
            anyhow::bail!(
                "{path} must match exactly one oneOf schema for {tool_name}; matched {matches}"
            );
        }
    }

    // If this schema is only a combinator wrapper, the branch validation above is complete.
    let non_combinator_keys = object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "allOf" | "anyOf" | "oneOf" | "description" | "title"
        )
    });
    if !non_combinator_keys
        && (object.contains_key("allOf")
            || object.contains_key("anyOf")
            || object.contains_key("oneOf"))
    {
        validate_json_schema_value(tool_name, path.to_string(), value, &json!({}))?;
    }
    let _ = schema;
    Ok(())
}

fn validate_schema_enum(
    tool_name: &str,
    path: &str,
    value: &Value,
    object: &Map<String, Value>,
) -> anyhow::Result<()> {
    let Some(allowed) = object.get("enum").and_then(Value::as_array) else {
        return Ok(());
    };
    if allowed.iter().any(|candidate| candidate == value) {
        return Ok(());
    }
    let allowed = allowed
        .iter()
        .map(|value| serde_json::to_string(value).unwrap_or_else(|_| value.to_string()))
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!("{path} must be one of [{allowed}] for {tool_name}")
}

fn validate_schema_type_constraints(
    tool_name: &str,
    path: &str,
    value: &Value,
    object: &Map<String, Value>,
) -> anyhow::Result<()> {
    let Some(type_spec) = object.get("type") else {
        return Ok(());
    };
    if let Some(expected) = type_spec.as_str() {
        return validate_schema_type(tool_name, path, value, expected);
    }
    if let Some(types) = type_spec.as_array() {
        if types
            .iter()
            .filter_map(Value::as_str)
            .any(|expected| validate_schema_type(tool_name, path, value, expected).is_ok())
        {
            return Ok(());
        }
        let labels = types
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" or ");
        anyhow::bail!("{path} must be {labels} for {tool_name}");
    }
    Ok(())
}

fn validate_schema_type(
    tool_name: &str,
    path: &str,
    value: &Value,
    expected: &str,
) -> anyhow::Result<()> {
    let ok = match expected {
        "string" => value.is_string(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "boolean" | "bool" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "null" => value.is_null(),
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        let article = if matches!(expected, "array" | "object" | "integer") {
            "an"
        } else {
            "a"
        };
        anyhow::bail!("{path} must be {article} {expected} for {tool_name}")
    }
}

fn validate_schema_object_constraints(
    tool_name: &str,
    path: &str,
    value: &Value,
    object: &Map<String, Value>,
) -> anyhow::Result<()> {
    let properties = object
        .get("properties")
        .and_then(Value::as_object)
        .or_else(|| shorthand_properties(object));
    let required = object
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();

    if properties.is_none() && required.is_empty() && !object.contains_key("additionalProperties") {
        return Ok(());
    }

    let Some(actual) = value.as_object() else {
        // A type constraint, if present, already emitted the precise error. Without type=object,
        // properties/required imply an object.
        anyhow::bail!("{path} must be an object for {tool_name}");
    };

    for key in required {
        if !actual.contains_key(key) {
            if path == tool_name {
                anyhow::bail!("Missing required argument for {tool_name}: {key}");
            }
            anyhow::bail!("{path}.{key} is required for {tool_name}");
        }
    }

    if let Some(properties) = properties {
        for (key, child_value) in actual {
            if let Some(child_schema) = properties.get(key) {
                validate_json_schema_value(
                    tool_name,
                    format!("{path}.{key}"),
                    child_value,
                    child_schema,
                )?;
                continue;
            }

            match object.get("additionalProperties") {
                Some(Value::Bool(false)) => {
                    anyhow::bail!("{path}.{key} is not an allowed argument for {tool_name}")
                }
                Some(Value::Object(_)) => validate_json_schema_value(
                    tool_name,
                    format!("{path}.{key}"),
                    child_value,
                    object.get("additionalProperties").expect("checked above"),
                )?,
                _ => {}
            }
        }
    }

    if let Some(min) = object.get("minProperties").and_then(Value::as_u64)
        && actual.len() < min as usize
    {
        anyhow::bail!("{path} must have at least {min} properties for {tool_name}");
    }
    if let Some(max) = object.get("maxProperties").and_then(Value::as_u64)
        && actual.len() > max as usize
    {
        anyhow::bail!("{path} must have at most {max} properties for {tool_name}");
    }
    Ok(())
}

fn shorthand_properties(object: &Map<String, Value>) -> Option<&Map<String, Value>> {
    let schema_keywords = [
        "type",
        "required",
        "properties",
        "additionalProperties",
        "items",
        "enum",
        "oneOf",
        "anyOf",
        "allOf",
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "minLength",
        "maxLength",
        "minItems",
        "maxItems",
        "minProperties",
        "maxProperties",
        "description",
        "title",
    ];
    let has_schema_keyword = object
        .keys()
        .any(|key| schema_keywords.contains(&key.as_str()));
    if has_schema_keyword {
        None
    } else {
        Some(object)
    }
}

fn validate_schema_array_constraints(
    tool_name: &str,
    path: &str,
    value: &Value,
    object: &Map<String, Value>,
) -> anyhow::Result<()> {
    if !object.contains_key("items")
        && !object.contains_key("minItems")
        && !object.contains_key("maxItems")
    {
        return Ok(());
    }
    let Some(items) = value.as_array() else {
        anyhow::bail!("{path} must be an array for {tool_name}");
    };
    if let Some(min) = object.get("minItems").and_then(Value::as_u64)
        && items.len() < min as usize
    {
        anyhow::bail!("{path} must contain at least {min} item(s) for {tool_name}");
    }
    if let Some(max) = object.get("maxItems").and_then(Value::as_u64)
        && items.len() > max as usize
    {
        anyhow::bail!("{path} must contain at most {max} item(s) for {tool_name}");
    }
    let Some(item_schema) = object.get("items") else {
        return Ok(());
    };
    if let Some(tuple_schemas) = item_schema.as_array() {
        for (index, child_schema) in tuple_schemas.iter().enumerate() {
            if let Some(child_value) = items.get(index) {
                validate_json_schema_value(
                    tool_name,
                    format!("{path}[{index}]"),
                    child_value,
                    child_schema,
                )?;
            }
        }
        return Ok(());
    }
    for (index, child_value) in items.iter().enumerate() {
        validate_json_schema_value(
            tool_name,
            format!("{path}[{index}]"),
            child_value,
            item_schema,
        )?;
    }
    Ok(())
}

fn validate_schema_string_constraints(
    tool_name: &str,
    path: &str,
    value: &Value,
    object: &Map<String, Value>,
) -> anyhow::Result<()> {
    if !object.contains_key("minLength")
        && !object.contains_key("maxLength")
        && !object.contains_key("pattern")
    {
        return Ok(());
    }
    let Some(text) = value.as_str() else {
        anyhow::bail!("{path} must be a string for {tool_name}");
    };
    let chars = text.chars().count() as u64;
    if let Some(min) = object.get("minLength").and_then(Value::as_u64)
        && chars < min
    {
        anyhow::bail!("{path} must be at least {min} character(s) for {tool_name}");
    }
    if let Some(max) = object.get("maxLength").and_then(Value::as_u64)
        && chars > max
    {
        anyhow::bail!("{path} must be at most {max} character(s) for {tool_name}");
    }
    // Deliberately do not implement JSON Schema regex patterns here. Tool handlers still perform
    // domain/path validation; this local subset focuses on structural validation without adding a
    // regex dependency or changing provider-visible schema contracts.
    Ok(())
}

fn validate_schema_number_constraints(
    tool_name: &str,
    path: &str,
    value: &Value,
    object: &Map<String, Value>,
) -> anyhow::Result<()> {
    if !object.contains_key("minimum")
        && !object.contains_key("maximum")
        && !object.contains_key("exclusiveMinimum")
        && !object.contains_key("exclusiveMaximum")
    {
        return Ok(());
    }
    let Some(number) = value.as_f64() else {
        anyhow::bail!("{path} must be a number for {tool_name}");
    };
    if let Some(min) = object.get("minimum").and_then(Value::as_f64)
        && number < min
    {
        anyhow::bail!("{path} must be >= {min} for {tool_name}");
    }
    if let Some(max) = object.get("maximum").and_then(Value::as_f64)
        && number > max
    {
        anyhow::bail!("{path} must be <= {max} for {tool_name}");
    }
    if let Some(min) = object.get("exclusiveMinimum").and_then(Value::as_f64)
        && number <= min
    {
        anyhow::bail!("{path} must be > {min} for {tool_name}");
    }
    if let Some(max) = object.get("exclusiveMaximum").and_then(Value::as_f64)
        && number >= max
    {
        anyhow::bail!("{path} must be < {max} for {tool_name}");
    }
    Ok(())
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

fn subagent_allowed_tools_from_env() -> Vec<String> {
    get_env("VEGVISIR_SUBAGENT_ALLOWED_TOOLS")
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|tool| !tool.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn tool_allowed_by_budget(allowed_tools: &[String], target: &str) -> bool {
    allowed_tools.iter().any(|allowed| {
        allowed == "*" || allowed == target || target.starts_with(&format!("{allowed}::"))
    })
}

impl ToolExecutor {
    pub fn execute(&mut self, call: ToolCall) -> Observation {
        let result = (|| {
            let tool = self.registry.get(&call.name)?;
            let subagent_allowed_tools = subagent_allowed_tools_from_env();
            if !subagent_allowed_tools.is_empty()
                && !tool_allowed_by_budget(&subagent_allowed_tools, &tool.name)
            {
                anyhow::bail!(
                    "Subagent budget denied tool `{}`; allowed_tools={}",
                    tool.name,
                    subagent_allowed_tools.join(",")
                );
            }
            let args = tool.normalize_args(call.args);
            tool.validate_args(&args)?;
            self.guardrails.authorize_tool(tool, &args)?;
            if !self.guardrails.policy.bypass_approvals_and_sandbox {
                self.runtime_policy
                    .authorize_tool_with_metadata(
                        &tool.name,
                        &args,
                        RuntimeToolMetadata {
                            risky: tool.risky,
                            safety_labels: Vec::new(),
                        },
                        &self.logger,
                    )
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
                } else if error_text.contains("Missing required argument")
                    || error_text.contains(" must be ")
                {
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

const NORMAL_SUDO_REJECTION: &str = "Direct sudo through normal command tools is disabled so sudo passwords cannot enter chat/session/trace history. Run /sudo auth, then use run_privileged_command.";
const PRIVILEGED_SUDO_REJECTION: &str = "Do not include sudo in run_privileged_command arguments. Run /sudo auth, then provide the underlying command; Vegvisir adds sudo -n internally.";

fn command_mentions_sudo_invocation(parts: &[&str]) -> bool {
    let Some((program, args)) = parts.split_first() else {
        return false;
    };
    if token_is_sudo_invocation(program.trim()) {
        return true;
    }

    if command_uses_shell_program(program) {
        return args
            .iter()
            .any(|arg| shell_snippet_mentions_sudo_invocation(arg));
    }

    false
}

fn command_uses_shell_program(program: &str) -> bool {
    let trimmed = program.trim();
    let basename = trimmed.rsplit('/').next().unwrap_or(trimmed);
    matches!(
        basename,
        "sh" | "bash" | "zsh" | "fish" | "dash" | "ksh" | "mksh" | "csh" | "tcsh"
    )
}

fn shell_snippet_mentions_sudo_invocation(snippet: &str) -> bool {
    let mut token = String::new();
    for ch in snippet.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.') {
            token.push(ch);
        } else {
            if token_is_sudo_invocation(&token) {
                return true;
            }
            token.clear();
        }
    }
    token_is_sudo_invocation(&token)
}

fn token_is_sudo_invocation(token: &str) -> bool {
    let trimmed = token.trim();
    trimmed == "sudo" || trimmed.ends_with("/sudo")
}

fn reject_sudo_misuse(parts: &[&str], privileged: bool) -> Option<Observation> {
    if !command_mentions_sudo_invocation(parts) {
        return None;
    }
    Some(if privileged {
        Observation::err(PRIVILEGED_SUDO_REJECTION, "SudoInvocationRejected")
    } else {
        Observation::err(NORMAL_SUDO_REJECTION, "SudoInvocationRejected")
    })
}

#[derive(Debug, Default)]
struct CommandStreamCapture {
    bytes: Vec<u8>,
    read_error: Option<String>,
}

fn spawn_command_stream_reader<R>(
    mut reader: R,
    stream_name: &'static str,
    output_sink: Option<CommandOutputSink>,
) -> JoinHandle<CommandStreamCapture>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut emitted_live_bytes = 0usize;
        let mut emitted_truncation_notice = false;
        let mut buffer = [0u8; COMMAND_STREAM_READ_CHUNK_BYTES];
        let read_error = loop {
            match reader.read(&mut buffer) {
                Ok(0) => break None,
                Ok(n) => {
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(sink) = &output_sink {
                        let remaining = COMMAND_STREAM_LIVE_MAX_BYTES_PER_STREAM
                            .saturating_sub(emitted_live_bytes);
                        if remaining > 0 {
                            let emit_len = n.min(remaining);
                            emitted_live_bytes = emitted_live_bytes.saturating_add(emit_len);
                            sink(CommandOutputChunk {
                                stream: stream_name.to_string(),
                                chunk: String::from_utf8_lossy(&buffer[..emit_len]).to_string(),
                                truncated: emit_len < n,
                            });
                        } else if !emitted_truncation_notice {
                            emitted_truncation_notice = true;
                            sink(CommandOutputChunk {
                                stream: stream_name.to_string(),
                                chunk: format!(
                                    "\n[Vegvisir live {stream_name} stream truncated after {} bytes; full captured output remains available in the final tool observation/artifacts.]\n",
                                    COMMAND_STREAM_LIVE_MAX_BYTES_PER_STREAM
                                ),
                                truncated: true,
                            });
                        }
                    }
                }
                Err(error) => break Some(error.to_string()),
            }
        };
        CommandStreamCapture { bytes, read_error }
    })
}

fn join_command_stream_reader(
    handle: Option<JoinHandle<CommandStreamCapture>>,
    stream_name: &str,
) -> CommandStreamCapture {
    handle
        .map(|handle| {
            handle.join().unwrap_or_else(|_| CommandStreamCapture {
                bytes: Vec::new(),
                read_error: Some(format!("{stream_name} reader thread panicked")),
            })
        })
        .unwrap_or_else(|| CommandStreamCapture {
            bytes: Vec::new(),
            read_error: Some(format!("{stream_name} pipe was unavailable")),
        })
}

fn command_stream_read_errors(
    stdout: &CommandStreamCapture,
    stderr: &CommandStreamCapture,
) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(error) = &stdout.read_error {
        errors.push(format!("stdout: {error}"));
    }
    if let Some(error) = &stderr.read_error {
        errors.push(format!("stderr: {error}"));
    }
    errors
}

#[allow(clippy::too_many_arguments)]
fn command_observation_data(
    parts: &[&str],
    sandboxed_command: &crate::command_sandbox::SandboxedCommand,
    include_command_in_data: bool,
    returncode: i32,
    timed_out: bool,
    timeout: u64,
    truncated: bool,
    privileged: bool,
    stdout_len: usize,
    stderr_len: usize,
    stream_read_errors: &[String],
) -> Map<String, Value> {
    let mut data = Map::new();
    if include_command_in_data {
        data.insert("command".to_string(), json!(parts));
    }
    data.insert(
        "command_sandboxed".to_string(),
        json!(sandboxed_command.sandboxed),
    );
    data.insert(
        "command_sandbox".to_string(),
        json!(sandboxed_command.sandbox_kind),
    );
    data.insert(
        "command_network_policy".to_string(),
        json!(sandboxed_command.network_policy),
    );
    data.insert("returncode".to_string(), json!(returncode));
    data.insert("timed_out".to_string(), json!(timed_out));
    data.insert("timeout_seconds".to_string(), json!(timeout));
    data.insert("output_truncated".to_string(), json!(truncated));
    data.insert("privileged".to_string(), json!(privileged));
    data.insert("stdout_bytes".to_string(), json!(stdout_len));
    data.insert("stderr_bytes".to_string(), json!(stderr_len));
    data.insert("streaming_capture".to_string(), json!(true));
    data.insert(
        "stream_capture_mode".to_string(),
        json!("incremental_pipe_drainers"),
    );
    data.insert("stream_read_errors".to_string(), json!(stream_read_errors));
    data.insert(
        "sudo_password_visibility".to_string(),
        json!(if privileged {
            "not-collected; sudo -n uses existing timestamp only"
        } else {
            "not-applicable"
        }),
    );
    data
}

fn execute_bounded_command(
    parts: &[&str],
    sandbox_config: &CommandSandboxConfig,
    timeout: u64,
    output_limit: usize,
    failure_error: &str,
    include_command_in_data: bool,
    privileged: bool,
) -> Observation {
    if parts.is_empty() {
        return Observation::err("Empty command", "ValueError");
    }
    if let Some(rejection) = reject_sudo_misuse(parts, privileged) {
        return rejection;
    }

    let effective_parts = if privileged {
        let sudo_status = privilege::sudo_status();
        if !sudo_status.authenticated {
            return Observation::err(
                format!(
                    "Privileged command requires an active sudo timestamp. {}",
                    sudo_status.message
                ),
                "SudoAuthenticationRequired",
            );
        }
        let mut prefixed = Vec::with_capacity(parts.len() + 2);
        prefixed.push("sudo");
        prefixed.push("-n");
        prefixed.extend_from_slice(parts);
        prefixed
    } else {
        parts.to_vec()
    };
    let sandboxed_command = match build_sandboxed_command(&effective_parts, sandbox_config) {
        Ok(command) => command,
        Err(error) => return Observation::err(error.to_string(), "CommandError"),
    };
    let mut command = Command::new(&sandboxed_command.program);
    command
        .args(&sandboxed_command.args)
        .current_dir(&sandboxed_command.current_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match spawn_command_in_own_process_group(&mut command) {
        Ok(child) => child,
        Err(error) => return Observation::err(error.to_string(), "CommandError"),
    };

    let output_sink = current_command_output_sink();
    let stdout_reader = child
        .stdout
        .take()
        .map(|reader| spawn_command_stream_reader(reader, "stdout", output_sink.clone()));
    let stderr_reader = child
        .stderr
        .take()
        .map(|reader| spawn_command_stream_reader(reader, "stderr", output_sink.clone()));
    let started = Instant::now();
    let mut timed_out = false;
    let mut status: Option<ExitStatus> = None;
    loop {
        match child.try_wait() {
            Ok(Some(exit_status)) => {
                status = Some(exit_status);
                break;
            }
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

    let status = match status {
        Some(status) => Ok(status),
        None => child.wait(),
    };
    let stdout_capture = join_command_stream_reader(stdout_reader, "stdout");
    let stderr_capture = join_command_stream_reader(stderr_reader, "stderr");

    match status {
        Ok(status) => {
            let mut content = String::new();
            content.push_str(&String::from_utf8_lossy(&stdout_capture.bytes));
            content.push_str(&String::from_utf8_lossy(&stderr_capture.bytes));
            let truncated = content.len() > output_limit;
            if truncated {
                content = compact_text_middle(&content, output_limit, "output");
            }
            let stream_read_errors = command_stream_read_errors(&stdout_capture, &stderr_capture);
            let data = command_observation_data(
                parts,
                &sandboxed_command,
                include_command_in_data,
                if timed_out {
                    -1
                } else {
                    status.code().unwrap_or(-1)
                },
                timed_out,
                timeout,
                truncated,
                privileged,
                stdout_capture.bytes.len(),
                stderr_capture.bytes.len(),
                &stream_read_errors,
            );
            Observation {
                ok: !timed_out && status.success() && stream_read_errors.is_empty(),
                content,
                data,
                error: if timed_out {
                    Some("CommandTimeout".to_string())
                } else if !stream_read_errors.is_empty() {
                    Some("CommandStreamReadError".to_string())
                } else {
                    (!status.success()).then(|| failure_error.to_string())
                },
            }
        }
        Err(error) => Observation::err(error.to_string(), "CommandError"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubagentProviderDefaults {
    pub provider: String,
    pub model: String,
    current_provider: Option<String>,
    current_model: Option<String>,
}

impl SubagentProviderDefaults {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        let provider = provider.into();
        let model = model.into();
        let provider = provider.trim();
        let model = repair_model_for_provider(provider, &model);
        Self {
            provider: provider.to_string(),
            model,
            current_provider: None,
            current_model: None,
        }
    }

    pub fn with_current_session(
        mut self,
        provider: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        let provider = provider.into();
        let model = model.into();
        self.current_provider = nonempty_subagent_provider_value(&provider);
        self.current_model = nonempty_subagent_provider_value(&model);
        self
    }

    pub fn materialized_defaults(&self) -> anyhow::Result<Self> {
        let (provider, model) = self.resolve_for_spawn_request(None, None)?;
        Ok(Self::new(provider, model))
    }

    pub fn resolve_for_spawn_request(
        &self,
        requested_provider: Option<String>,
        requested_model: Option<String>,
    ) -> anyhow::Result<(String, String)> {
        let requested_provider_current = requested_provider
            .as_deref()
            .is_some_and(is_current_provider_model_sentinel);
        let requested_model_current = requested_model
            .as_deref()
            .is_some_and(is_current_provider_model_sentinel);
        let default_provider_current = is_current_provider_model_sentinel(&self.provider);
        let default_model_current = is_current_provider_model_sentinel(&self.model);

        let needs_current_provider = requested_provider_current
            || default_provider_current
            || (requested_provider.is_none() && requested_model_current);
        let needs_current_model = requested_model_current
            || default_model_current
            || requested_provider_current
            || (requested_provider.is_none() && requested_model_current)
            || (requested_provider.is_none() && default_provider_current);

        let current_provider = if needs_current_provider || needs_current_model {
            Some(self.current_provider.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "spawn_subagent provider/model value `current` requires the parent session provider/model context, but this tool registry was built without it"
                )
            })?)
        } else {
            self.current_provider.as_deref()
        };
        let current_model = if needs_current_model
            || requested_model_current
            || default_model_current
        {
            Some(self.current_model.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "spawn_subagent model value `current` requires the parent session model context, but this tool registry was built without it"
                )
            })?)
        } else {
            self.current_model.as_deref()
        };

        let provider = match requested_provider.as_deref() {
            Some(value) if is_current_provider_model_sentinel(value) => {
                current_provider.unwrap_or_default().to_string()
            }
            Some(value) => value.trim().to_string(),
            None if requested_model_current || default_provider_current => {
                current_provider.unwrap_or_default().to_string()
            }
            None => self.provider.clone(),
        };

        let model = match requested_model.as_deref() {
            Some(value) if is_current_provider_model_sentinel(value) => {
                if let Some(current_provider) = current_provider
                    && !provider.trim().is_empty()
                    && provider != current_provider
                {
                    anyhow::bail!(
                        "spawn_subagent model `current` belongs to provider `{current_provider}`, but requested provider was `{provider}`"
                    );
                }
                current_model.unwrap_or_default().to_string()
            }
            Some(value) => value.trim().to_string(),
            None if requested_provider_current
                || requested_model_current
                || default_provider_current =>
            {
                current_model.unwrap_or_default().to_string()
            }
            None if default_model_current => current_model.unwrap_or_default().to_string(),
            None => self.model.clone(),
        };

        if is_current_provider_model_sentinel(&provider)
            || is_current_provider_model_sentinel(&model)
        {
            anyhow::bail!(
                "spawn_subagent provider/model resolved to literal `current`; this is a sentinel and must be materialized before launch"
            );
        }

        let model = repair_model_for_provider(&provider, &model);
        Ok((provider, model))
    }
}

fn is_current_provider_model_sentinel(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("current")
}

fn nonempty_subagent_provider_value(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillerForgeModelTargetDefaults {
    pub provider: String,
    pub model: String,
    current_provider: Option<String>,
    current_model: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSkillerForgeModelTarget {
    pub provider: String,
    pub model: String,
    pub source: String,
}

impl SkillerForgeModelTargetDefaults {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        let provider = normalize_skiller_forge_target_value(&provider.into())
            .unwrap_or_else(|| "current".to_string());
        let mut model = normalize_skiller_forge_target_value(&model.into())
            .unwrap_or_else(|| "current".to_string());
        if !is_current_provider_model_sentinel(&provider)
            && !is_current_provider_model_sentinel(&model)
        {
            model = repair_model_for_provider(&provider, &model);
        }
        Self {
            provider,
            model,
            current_provider: None,
            current_model: None,
        }
    }

    pub fn with_current_session(
        mut self,
        provider: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        self.current_provider = nonempty_subagent_provider_value(&provider.into());
        self.current_model = nonempty_subagent_provider_value(&model.into());
        self
    }

    pub fn resolve(&self) -> anyhow::Result<ResolvedSkillerForgeModelTarget> {
        let provider_is_current = is_current_provider_model_sentinel(&self.provider);
        let model_is_current = is_current_provider_model_sentinel(&self.model);

        let provider = if provider_is_current {
            self.current_provider
                .clone()
                .unwrap_or_else(|| skiller_forge::DEFAULT_FORGE_MODEL_PROVIDER.to_string())
        } else {
            self.provider.clone()
        };

        let model = if model_is_current {
            if provider_is_current {
                self.current_model
                    .clone()
                    .unwrap_or_else(|| skiller_forge::DEFAULT_FORGE_MODEL.to_string())
            } else if self.current_provider.as_deref() == Some(provider.as_str()) {
                self.current_model
                    .clone()
                    .unwrap_or_else(|| skiller_forge::DEFAULT_FORGE_MODEL.to_string())
            } else {
                anyhow::bail!(
                    "Skiller Forge model target is `current` but Skiller provider is fixed to `{provider}`; set /smodel explicitly or reset /sprovider current"
                );
            }
        } else {
            repair_model_for_provider(&provider, &self.model)
        };

        let source = match (
            provider_is_current,
            model_is_current,
            self.current_provider.is_some(),
            self.current_model.is_some(),
        ) {
            (true, true, true, true) => "main-session-current".to_string(),
            (true, true, _, _) => "legacy-default-no-session".to_string(),
            (false, false, _, _) => "skiller-explicit-override".to_string(),
            (false, true, _, _) => "skiller-provider-override-current-model".to_string(),
            (true, false, true, _) => "skiller-model-override-current-provider".to_string(),
            (true, false, false, _) => "skiller-model-override-legacy-provider".to_string(),
        };

        Ok(ResolvedSkillerForgeModelTarget {
            provider,
            model,
            source,
        })
    }
}

impl Default for SkillerForgeModelTargetDefaults {
    fn default() -> Self {
        Self::new("current", "current")
    }
}

fn normalize_skiller_forge_target_value(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || matches!(value, "default" | "clear" | "reset" | "unset") {
        None
    } else {
        Some(value.to_string())
    }
}

fn apply_skiller_forge_model_target(
    request: &mut ForgeRequestEnvelope,
    target: &ResolvedSkillerForgeModelTarget,
) {
    request.model_provider = Some(target.provider.clone());
    request.model = Some(target.model.clone());
    if let Some(provenance) = request.provider_provenance.as_mut() {
        provenance.model_provider = Some(target.provider.clone());
        provenance.model = Some(target.model.clone());
        provenance.caveats.push(format!(
            "Vegvisir resolved Skiller Forge model target from {}.",
            target.source
        ));
    }
}

impl Default for SubagentProviderDefaults {
    fn default() -> Self {
        Self::new("", "")
    }
}

pub const DEFAULT_SUBAGENT_MAX_STEPS: u64 = 4;
pub const DEFAULT_SUBAGENT_MIN_MAX_STEPS: u64 = 1;
pub const DEFAULT_SUBAGENT_MAX_MAX_STEPS: u64 = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubagentSpawnDefaults {
    pub default_max_steps: u64,
    pub min_max_steps: u64,
    pub max_max_steps: u64,
    pub work_budget: SubAgentWorkBudget,
}

impl SubagentSpawnDefaults {
    pub fn normalized(mut self) -> Self {
        self.min_max_steps = self.min_max_steps.max(1);
        self.max_max_steps = self.max_max_steps.max(self.min_max_steps);
        self.default_max_steps = self
            .default_max_steps
            .clamp(self.min_max_steps, self.max_max_steps);
        self.work_budget.max_tool_calls = self.work_budget.max_tool_calls.map(|value| value.max(1));
        self.work_budget.max_read_bytes = self.work_budget.max_read_bytes.map(|value| value.max(1));
        self.work_budget.max_output_bytes =
            self.work_budget.max_output_bytes.map(|value| value.max(1));
        self.work_budget.allowed_tools = self
            .work_budget
            .allowed_tools
            .into_iter()
            .map(|tool| tool.trim().to_string())
            .filter(|tool| !tool.is_empty())
            .collect();
        self
    }

    pub fn effective_max_steps(&self, requested: Option<u64>) -> u64 {
        let normalized = self.clone().normalized();
        requested
            .unwrap_or(normalized.default_max_steps)
            .clamp(normalized.min_max_steps, normalized.max_max_steps)
    }
}

impl Default for SubagentSpawnDefaults {
    fn default() -> Self {
        Self {
            default_max_steps: DEFAULT_SUBAGENT_MAX_STEPS,
            min_max_steps: DEFAULT_SUBAGENT_MIN_MAX_STEPS,
            max_max_steps: DEFAULT_SUBAGENT_MAX_MAX_STEPS,
            work_budget: SubAgentWorkBudget {
                max_steps: None,
                max_tool_calls: Some(8),
                max_read_bytes: Some(64 * 1024),
                max_output_bytes: Some(16 * 1024),
                allowed_tools: vec!["list_files".to_string(), "read_file".to_string()],
                notes: "Prefer targeted search/listing and small file excerpts. Do not read huge files in full; ask for a larger budget if needed.".to_string(),
            },
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
    build_builtin_registry_with_cms_mode_and_subagent_limit(
        workspace,
        cms_config,
        bypass_sandbox,
        DEFAULT_ACTIVE_SUBAGENT_LIMIT,
    )
}

pub fn build_builtin_registry_with_cms_mode_and_subagent_limit(
    workspace: impl AsRef<Path>,
    cms_config: VegvisirCmsConfig,
    bypass_sandbox: bool,
    active_subagent_limit: usize,
) -> anyhow::Result<ToolRegistry> {
    build_builtin_registry_with_cms_mode_subagent_limit_and_provider_defaults(
        workspace,
        cms_config,
        bypass_sandbox,
        active_subagent_limit,
        SubagentProviderDefaults::default(),
        SkillerForgeModelTargetDefaults::default(),
    )
}

pub fn build_builtin_registry_with_cms_mode_subagent_limit_and_provider_defaults(
    workspace: impl AsRef<Path>,
    cms_config: VegvisirCmsConfig,
    bypass_sandbox: bool,
    active_subagent_limit: usize,
    subagent_provider_defaults: SubagentProviderDefaults,
    skiller_forge_model_target_defaults: SkillerForgeModelTargetDefaults,
) -> anyhow::Result<ToolRegistry> {
    build_builtin_registry_with_cms_mode_subagent_config(
        workspace,
        cms_config,
        bypass_sandbox,
        active_subagent_limit,
        subagent_provider_defaults,
        SubagentSpawnDefaults::default(),
        skiller_forge_model_target_defaults,
    )
}

pub fn build_builtin_registry_with_cms_mode_subagent_config(
    workspace: impl AsRef<Path>,
    cms_config: VegvisirCmsConfig,
    bypass_sandbox: bool,
    active_subagent_limit: usize,
    subagent_provider_defaults: SubagentProviderDefaults,
    subagent_spawn_defaults: SubagentSpawnDefaults,
    skiller_forge_model_target_defaults: SkillerForgeModelTargetDefaults,
) -> anyhow::Result<ToolRegistry> {
    let sandbox = if bypass_sandbox {
        WorkspaceSandbox::new_unrestricted(workspace)?
    } else {
        WorkspaceSandbox::new(workspace)?
    };
    let data_root = vegvisir_data_root_from_cms_config(&cms_config);
    let default_msp_registry_root = default_msp_registry_path(&data_root);
    let default_skiller_bundle_root = default_skiller_bundle_root(&data_root);
    let global_skill_roots = vec![
        data_root.join("msp"),
        data_root.join("skiller"),
        default_msp_registry_root.clone(),
        default_skiller_bundle_root.clone(),
    ];
    let subagent_data_root = data_root.clone();
    let active_subagent_limit = active_subagent_limit.max(1);
    let subagent_spawn_defaults = subagent_spawn_defaults.normalized();
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
                    let max_read_bytes = get_env("VEGVISIR_SUBAGENT_MAX_READ_BYTES")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .filter(|value| *value > 0);
                    let truncated = max_read_bytes.is_some_and(|limit| original_bytes > limit);
                    let content = if let Some(limit) =
                        max_read_bytes.filter(|limit| original_bytes > *limit)
                    {
                        compact_text_middle(&content, limit, "read_file")
                    } else {
                        content
                    };
                    let mut data = Map::new();
                    data.insert("path".to_string(), json!(path));
                    data.insert("bytes".to_string(), json!(original_bytes));
                    data.insert("output_truncated".to_string(), json!(truncated));
                    if let Some(limit) = max_read_bytes {
                        data.insert("max_read_bytes".to_string(), json!(limit));
                    }
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

    let msp_client_info_sandbox = sandbox.clone();
    let msp_client_info_data_root = data_root.clone();
    let msp_client_info_global_roots = global_skill_roots.clone();
    registry.register(Tool::new(
        "msp_client_info",
        "Inspect the native MSP client component and local registry summary.",
        Arc::new(move |args| {
            let registry_path = msp_registry_path(&args, &msp_client_info_data_root);
            let registry_path = match resolve_workspace_or_global_path(
                &msp_client_info_sandbox,
                &registry_path,
                &msp_client_info_global_roots,
            ) {
                Ok(path) => path,
                Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
            };
            match MspClient::open(&registry_path) {
                Ok(client) => msp_json_observation(json!({
                    "info": client.info(),
                    "registry": client.summary(),
                })),
                Err(error) => Observation::err(error.to_string(), "MspClientError"),
            }
        }),
        json!({"properties": {"registry": "string"}}),
        false,
    ))?;

    let msp_client_search_sandbox = sandbox.clone();
    let msp_client_search_data_root = data_root.clone();
    let msp_client_search_global_roots = global_skill_roots.clone();
    registry.register(Tool::new(
        "msp_client_search",
        "Search skills in a local MSP registry through Vegvisir's native MSP client component.",
        Arc::new(move |args| {
            let registry_path = msp_registry_path(&args, &msp_client_search_data_root);
            let registry_path = match resolve_workspace_or_global_path(&msp_client_search_sandbox, &registry_path, &msp_client_search_global_roots) {
                Ok(path) => path,
                Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
            };
            let max_risk = match args.get("max_risk").and_then(Value::as_str) {
                Some(value) => match msp_client::parse_risk_level(value) {
                    Ok(risk) => Some(risk),
                    Err(error) => return Observation::err(error.to_string(), "ValueError"),
                },
                None => None,
            };
            let request = MspSearchRequest {
                task: args.get("task").and_then(Value::as_str).map(str::to_string),
                category: args.get("category").and_then(Value::as_str).map(str::to_string),
                domain: args.get("domain").and_then(Value::as_str).map(str::to_string),
                language: args.get("language").and_then(Value::as_str).map(str::to_string),
                available_tools: json_string_array(&args, "available_tools"),
                max_risk,
                limit: args.get("limit").and_then(Value::as_u64).map(|value| value as usize),
            };
            match MspClient::open(&registry_path) {
                Ok(client) => msp_json_observation(client.search(request)),
                Err(error) => Observation::err(error.to_string(), "MspClientError"),
            }
        }),
        json!({"properties": {"registry": "string", "task": "string", "category": "string", "domain": "string", "language": "string", "available_tools": "array", "max_risk": "string", "limit": "integer"}}),
        false,
    ))?;

    let msp_import_skiller_sandbox = sandbox.clone();
    let msp_import_skiller_data_root = data_root.clone();
    let msp_import_skiller_global_roots = global_skill_roots.clone();
    registry.register(Tool::new(
        "msp_client_import_skiller",
        "Import a current Skiller bundle into a local MSP registry through Vegvisir's native MSP client component.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else {
                return Observation::err("Missing bundle", "ValueError");
            };
            let Some(issuer) = args.get("issuer").and_then(Value::as_str) else {
                return Observation::err("Missing issuer", "ValueError");
            };
            let registry_path = msp_registry_path(&args, &msp_import_skiller_data_root);
            let registry_path = match resolve_workspace_or_global_path(&msp_import_skiller_sandbox, &registry_path, &msp_import_skiller_global_roots) {
                Ok(path) => path,
                Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
            };
            let bundle_path = match resolve_workspace_or_global_path(&msp_import_skiller_sandbox, bundle, &msp_import_skiller_global_roots) {
                Ok(path) => path,
                Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
            };
            let signing_key = match args.get("signing_key").and_then(Value::as_str) {
                Some(path) if !path.trim().is_empty() => match msp_import_skiller_sandbox.resolve(path) {
                    Ok(path) => Some(path),
                    Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
                },
                _ => None,
            };
            let request = MspImportSkillerBundleRequest {
                bundle: bundle_path,
                issuer: issuer.to_string(),
                force: args.get("force").and_then(Value::as_bool).unwrap_or(false),
                allow_mutable_version: args
                    .get("allow_mutable_version")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                signing_key,
                deprecation: msp_client::msp_publisher::PublicationDeprecation {
                    deprecated: args.get("deprecated").and_then(Value::as_bool).unwrap_or(false),
                    reason: args
                        .get("deprecation_reason")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    skill_replacement: args
                        .get("replacement_skill")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    pack_replacement: args
                        .get("replacement_pack")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    sunset_at: args
                        .get("sunset_at")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                },
            };
            match MspClient::open(&registry_path)
                .and_then(|client| client.import_skiller_bundle(request))
            {
                Ok(response) => msp_json_observation(response),
                Err(error) => Observation::err(error.to_string(), "MspImportSkillerError"),
            }
        }),
        json!({
            "required": ["bundle", "issuer"],
            "properties": {
                "registry": "string",
                "bundle": "string",
                "issuer": "string",
                "force": "boolean",
                "allow_mutable_version": "boolean",
                "signing_key": "string",
                "deprecated": "boolean",
                "deprecation_reason": "string",
                "replacement_skill": "string",
                "replacement_pack": "string",
                "sunset_at": "string"
            }
        }),
        true,
    ))?;

    let msp_client_load_sandbox = sandbox.clone();
    let msp_client_load_data_root = data_root.clone();
    let msp_client_load_global_roots = global_skill_roots.clone();
    registry.register(Tool::new(
        "msp_client_load",
        "Load MSP skill context from a local registry using mode card, body, extended, or raw.",
        Arc::new(move |args| {
            let Some(id) = args.get("id").and_then(Value::as_str) else {
                return Observation::err("Missing id", "ValueError");
            };
            let mode = match args
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("body")
                .parse::<MspLoadMode>()
            {
                Ok(mode) => mode,
                Err(error) => return Observation::err(error.to_string(), "ValueError"),
            };
            let registry_path = msp_registry_path(&args, &msp_client_load_data_root);
            let registry_path = match resolve_workspace_or_global_path(&msp_client_load_sandbox, &registry_path, &msp_client_load_global_roots) {
                Ok(path) => path,
                Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
            };
            match MspClient::open(&registry_path).and_then(|client| client.load_skill(id, mode)) {
                Ok(loaded) => {
                    let mut data = Map::new();
                    data.insert("id".to_string(), json!(loaded.id));
                    data.insert("mode".to_string(), json!(loaded.mode));
                    data.insert("manifest".to_string(), serde_json::to_value(&loaded.raw.manifest).unwrap_or(Value::Null));
                    data.insert("body_hash_valid".to_string(), json!(loaded.raw.body_hash_valid));
                    data.insert("dependency_ids".to_string(), json!(loaded.raw.dependency_ids));
                    Observation {
                        ok: true,
                        content: loaded.content,
                        data,
                        error: None,
                    }
                }
                Err(error) => Observation::err(error.to_string(), "MspLoadError"),
            }
        }),
        json!({"required": ["id"], "properties": {"registry": "string", "id": "string", "mode": "string"}}),
        false,
    ))?;

    let msp_client_manifest_sandbox = sandbox.clone();
    let msp_client_manifest_data_root = data_root.clone();
    let msp_client_manifest_global_roots = global_skill_roots.clone();
    registry.register(Tool::new(
        "msp_client_manifest",
        "Get an MSP skill manifest from a local registry.",
        Arc::new(move |args| {
            let Some(id) = args.get("id").and_then(Value::as_str) else {
                return Observation::err("Missing id", "ValueError");
            };
            let registry_path = msp_registry_path(&args, &msp_client_manifest_data_root);
            let registry_path = match resolve_workspace_or_global_path(
                &msp_client_manifest_sandbox,
                &registry_path,
                &msp_client_manifest_global_roots,
            ) {
                Ok(path) => path,
                Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
            };
            match MspClient::open(&registry_path).and_then(|client| client.get_manifest(id)) {
                Ok(manifest) => msp_json_observation(manifest),
                Err(error) => Observation::err(error.to_string(), "MspManifestError"),
            }
        }),
        json!({"required": ["id"], "properties": {"registry": "string", "id": "string"}}),
        false,
    ))?;

    let msp_verify_sandbox = sandbox.clone();
    let msp_verify_data_root = data_root.clone();
    let msp_verify_global_roots = global_skill_roots.clone();
    registry.register(Tool::new(
        "msp_client_verify_trust",
        "Verify the MSP trust hash/signature envelope for a skill body artifact.",
        Arc::new(move |args| {
            let Some(id) = args.get("id").and_then(Value::as_str) else {
                return Observation::err("Missing id", "ValueError");
            };
            let registry_path = msp_registry_path(&args, &msp_verify_data_root);
            let registry_path = match resolve_workspace_or_global_path(
                &msp_verify_sandbox,
                &registry_path,
                &msp_verify_global_roots,
            ) {
                Ok(path) => path,
                Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
            };
            match MspClient::open(&registry_path).and_then(|client| client.verify_trust(id)) {
                Ok(result) => msp_json_observation(result),
                Err(error) => Observation::err(error.to_string(), "MspTrustError"),
            }
        }),
        json!({"required": ["id"], "properties": {"registry": "string", "id": "string"}}),
        false,
    ))?;

    let msp_compat_sandbox = sandbox.clone();
    let msp_compat_data_root = data_root.clone();
    let msp_compat_global_roots = global_skill_roots.clone();
    registry.register(Tool::new(
        "msp_client_check_compatibility",
        "Check whether an MSP skill is compatible with a Vegvisir runtime/tool/model capability envelope.",
        Arc::new(move |args| {
            let Some(id) = args.get("id").and_then(Value::as_str) else {
                return Observation::err("Missing id", "ValueError");
            };
            let registry_path = msp_registry_path(&args, &msp_compat_data_root);
            let registry_path = match resolve_workspace_or_global_path(&msp_compat_sandbox, &registry_path, &msp_compat_global_roots) {
                Ok(path) => path,
                Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
            };
            let query = msp_client::msp_core::RuntimeCompatibilityQuery {
                msp_version: args.get("msp_version").and_then(Value::as_str).map(str::to_string),
                supported_manifest_versions: json_string_array(&args, "supported_manifest_versions"),
                runtime_name: args
                    .get("runtime_name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| Some("vegvisir".to_string())),
                runtime_version: args.get("runtime_version").and_then(Value::as_str).map(str::to_string),
                supported_formats: json_string_array(&args, "supported_formats"),
                runtime_capabilities: json_string_array(&args, "runtime_capabilities"),
                model_capabilities: json_string_array(&args, "model_capabilities"),
                available_tools: json_string_array(&args, "available_tools"),
                tool_versions: BTreeMap::new(),
                permissions: json_string_array(&args, "permissions"),
                context_window: args.get("context_window").and_then(Value::as_u64),
                platform: args.get("platform").and_then(Value::as_str).map(str::to_string),
            };
            let request = msp_client::CompatibilityRequest {
                skill_id: id.to_string(),
                query,
            };
            match MspClient::open(&registry_path).and_then(|client| client.check_compatibility(request)) {
                Ok(result) => msp_json_observation(result),
                Err(error) => Observation::err(error.to_string(), "MspCompatibilityError"),
            }
        }),
        json!({"required": ["id"], "properties": {"registry": "string", "id": "string", "msp_version": "string", "supported_manifest_versions": "array", "runtime_name": "string", "runtime_version": "string", "supported_formats": "array", "runtime_capabilities": "array", "model_capabilities": "array", "available_tools": "array", "permissions": "array", "context_window": "integer", "platform": "string"}}),
        false,
    ))?;

    let command_sandbox_config =
        CommandSandboxConfig::from_env(sandbox.root.clone(), bypass_sandbox)?;
    let run_sandbox_config = command_sandbox_config.clone();
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
            execute_bounded_command(
                &parts,
                &run_sandbox_config,
                timeout,
                output_limit,
                "CommandFailed",
                false,
                false,
            )
        }),
        json!({"required": ["command"], "properties": {"command": "array", "timeout": "integer", "output_limit": "integer"}}),
        true,
    ))?;

    let privileged_sandbox_config = command_sandbox_config.clone();
    registry.register(Tool::new(
        "run_privileged_command",
        "Run an allow-listed privileged command via sudo -n using an existing sudo timestamp; Vegvisir never reads or logs the sudo password.",
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
            execute_bounded_command(
                &parts,
                &privileged_sandbox_config,
                timeout,
                output_limit,
                "PrivilegedCommandFailed",
                false,
                true,
            )
        }),
        json!({"required": ["command"], "properties": {"command": "array", "timeout": "integer", "output_limit": "integer"}}),
        true,
    ))?;

    let test_root = sandbox.root.clone();
    let test_sandbox_config = command_sandbox_config.clone();
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
            execute_bounded_command(
                &parts,
                &test_sandbox_config,
                timeout,
                output_limit,
                "TestsFailed",
                true,
                false,
            )
        }),
        json!({"properties": {"command": "array", "timeout": "integer", "output_limit": "integer"}}),
        true,
    ))?;

    let skiller_compile_sandbox = sandbox.clone();
    let skiller_compile_data_root = data_root.clone();
    let skiller_compile_global_roots = global_skill_roots.clone();
    let skiller_compile_model_target_defaults = skiller_forge_model_target_defaults.clone();
    registry.register(Tool::new(
        "skiller_compile",
        "Compile local source files/directories into a deterministic Skiller draft bundle and return a Vegvisir Forge request/prompt for default model refinement before final use.",
        Arc::new(move |args| {
            let Some(input) = args.get("input").and_then(Value::as_str) else { return Observation::err("Missing input", "ValueError"); };
            let name = args.get("name").and_then(Value::as_str).unwrap_or("skiller-bundle");
            let out_path_raw = skiller_bundle_output_path(&args, &skiller_compile_data_root, name);
            let out_display = out_path_raw.display().to_string();
            let domain = args.get("domain").and_then(Value::as_str);
            let forge_model_target = match skiller_compile_model_target_defaults.resolve() { Ok(target) => target, Err(error) => return Observation::err(error.to_string(), "SkillerForgeModelTargetError") };
            let input_path = match skiller_compile_sandbox.resolve(input) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            let out_path = match resolve_workspace_or_global_path(&skiller_compile_sandbox, &out_path_raw, &skiller_compile_global_roots) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            match compiler::compile_path(&input_path, name, domain).and_then(|bundle| {
                let bundle_id = bundle.package.bundle_id.clone();
                let skill_count = bundle.skills.len();
                let source_count = bundle.sources.len();
                let (forge_request, forge_system_prompt, forge_prompt, response_template) =
                    skiller_bundle_handoff_observation_data(&bundle, ForgePassType::SkillExpansion, domain, &forge_model_target);
                skiller_registry::write_bundle(&bundle, &out_path)?;
                Ok((bundle_id, skill_count, source_count, forge_request, forge_system_prompt, forge_prompt, response_template))
            }) {
                Ok((bundle_id, skill_count, source_count, forge_request, forge_system_prompt, forge_prompt, response_template)) => {
                    let request_id = forge_request.request_id.clone();
                    let mut data = Map::new();
                    data.insert("bundle_id".to_string(), json!(bundle_id));
                    data.insert("skill_count".to_string(), json!(skill_count));
                    data.insert("source_count".to_string(), json!(source_count));
                    data.insert("out".to_string(), json!(out_display));
                    data.insert("global_install_root".to_string(), json!(skiller_compile_data_root.join("skiller").join("bundles")));
                    data.insert("deterministic_stage".to_string(), json!(true));
                    add_skiller_forge_observation_data(&mut data, &forge_request, forge_system_prompt, forge_prompt, response_template, &forge_model_target);
                    Observation { ok: true, content: format!("Compiled deterministic Skiller draft bundle {bundle_id} to {out_display} ({skill_count} skills, {source_count} sources). Forge refinement is required by default before treating this as agent-ready: use the included ForgeRequestEnvelope ({request_id}) as model context, then apply the model's ForgeResponseEnvelope with skiller_forge_apply."), data, error: None }
                }
                Err(error) => Observation::err(error.to_string(), "SkillerCompileError"),
            }
        }),
        json!({"required": ["input"], "properties": {"input": "string", "out": "string", "name": "string", "domain": "string"}}),
        true,
    ))?;

    let skiller_compile_cli_help_sandbox = sandbox.clone();
    let skiller_compile_cli_help_data_root = data_root.clone();
    let skiller_compile_cli_help_global_roots = global_skill_roots.clone();
    let skiller_compile_cli_help_model_target_defaults =
        skiller_forge_model_target_defaults.clone();
    registry.register(Tool::new(
        "skiller_compile_cli_help",
        "Compile captured CLI help/manpage text into a deterministic Skiller CLI draft bundle and return a Vegvisir Forge request/prompt for default model refinement before final use.",
        Arc::new(move |args| {
            let Some(input) = args.get("input").and_then(Value::as_str) else { return Observation::err("Missing input", "ValueError"); };
            let name = args.get("name").and_then(Value::as_str).unwrap_or("skiller-cli-help-bundle");
            let out_path_raw = skiller_bundle_output_path(&args, &skiller_compile_cli_help_data_root, name);
            let out_display = out_path_raw.display().to_string();
            let domain = args.get("domain").and_then(Value::as_str);
            let forge_model_target = match skiller_compile_cli_help_model_target_defaults.resolve() { Ok(target) => target, Err(error) => return Observation::err(error.to_string(), "SkillerForgeModelTargetError") };
            let input_path = match skiller_compile_cli_help_sandbox.resolve(input) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            let out_path = match resolve_workspace_or_global_path(&skiller_compile_cli_help_sandbox, &out_path_raw, &skiller_compile_cli_help_global_roots) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            match compiler::compile_cli_help(&input_path, name, domain).and_then(|bundle| {
                let bundle_id = bundle.package.bundle_id.clone();
                let skill_count = bundle.skills.len();
                let source_count = bundle.sources.len();
                let (forge_request, forge_system_prompt, forge_prompt, response_template) =
                    skiller_bundle_handoff_observation_data(&bundle, ForgePassType::SkillExpansion, domain, &forge_model_target);
                skiller_registry::write_bundle(&bundle, &out_path)?;
                Ok((bundle_id, skill_count, source_count, forge_request, forge_system_prompt, forge_prompt, response_template))
            }) {
                Ok((bundle_id, skill_count, source_count, forge_request, forge_system_prompt, forge_prompt, response_template)) => {
                    let request_id = forge_request.request_id.clone();
                    let mut data = Map::new();
                    data.insert("bundle_id".to_string(), json!(bundle_id));
                    data.insert("skill_count".to_string(), json!(skill_count));
                    data.insert("source_count".to_string(), json!(source_count));
                    data.insert("out".to_string(), json!(out_display));
                    data.insert("global_install_root".to_string(), json!(skiller_compile_cli_help_data_root.join("skiller").join("bundles")));
                    data.insert("deterministic_stage".to_string(), json!(true));
                    add_skiller_forge_observation_data(&mut data, &forge_request, forge_system_prompt, forge_prompt, response_template, &forge_model_target);
                    Observation { ok: true, content: format!("Compiled deterministic Skiller CLI help draft bundle {bundle_id} to {out_display} ({skill_count} skills, {source_count} sources). Forge refinement is required by default before treating this as agent-ready: use the included ForgeRequestEnvelope ({request_id}) as model context, then apply the model's ForgeResponseEnvelope with skiller_forge_apply."), data, error: None }
                }
                Err(error) => Observation::err(error.to_string(), "SkillerCompileError"),
            }
        }),
        json!({"required": ["input"], "properties": {"input": "string", "out": "string", "name": "string", "domain": "string"}}),
        true,
    ))?;

    let skiller_import_skill_sandbox = sandbox.clone();
    let skiller_import_skill_data_root = data_root.clone();
    let skiller_import_skill_global_roots = global_skill_roots.clone();
    let skiller_import_skill_model_target_defaults = skiller_forge_model_target_defaults.clone();
    registry.register(Tool::new(
        "skiller_import_skill",
        "Import pre-existing skill YAML/JSON or an existing Skiller bundle without deterministic raw-source generation, then prepare a ScriptGeneration Forge handoff for cleanup, helper scripts, validation, and review.",
        Arc::new(move |args| {
            let Some(input) = args.get("input").and_then(Value::as_str) else { return Observation::err("Missing input", "ValueError"); };
            let name = args.get("name").and_then(Value::as_str).unwrap_or("skiller-imported-bundle");
            let out_path_raw = skiller_bundle_output_path(&args, &skiller_import_skill_data_root, name);
            let out_display = out_path_raw.display().to_string();
            let domain = args.get("domain").and_then(Value::as_str);
            let forge_model_target = match skiller_import_skill_model_target_defaults.resolve() { Ok(target) => target, Err(error) => return Observation::err(error.to_string(), "SkillerForgeModelTargetError") };
            let input_path = match skiller_import_skill_sandbox.resolve(input) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            let out_path = match resolve_workspace_or_global_path(&skiller_import_skill_sandbox, &out_path_raw, &skiller_import_skill_global_roots) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            match compiler::import_skill_path(&input_path, name, domain).and_then(|bundle| {
                let bundle_id = bundle.package.bundle_id.clone();
                let skill_count = bundle.skills.len();
                let source_count = bundle.sources.len();
                let (forge_request, forge_system_prompt, forge_prompt, response_template) =
                    skiller_bundle_handoff_observation_data(&bundle, ForgePassType::ScriptGeneration, domain, &forge_model_target);
                skiller_registry::write_bundle(&bundle, &out_path)?;
                Ok((bundle_id, skill_count, source_count, forge_request, forge_system_prompt, forge_prompt, response_template))
            }) {
                Ok((bundle_id, skill_count, source_count, forge_request, forge_system_prompt, forge_prompt, response_template)) => {
                    let request_id = forge_request.request_id.clone();
                    let mut data = Map::new();
                    data.insert("bundle_id".to_string(), json!(bundle_id));
                    data.insert("skill_count".to_string(), json!(skill_count));
                    data.insert("source_count".to_string(), json!(source_count));
                    data.insert("out".to_string(), json!(out_display));
                    data.insert("global_install_root".to_string(), json!(skiller_import_skill_data_root.join("skiller").join("bundles")));
                    data.insert("import_mode".to_string(), json!("pre_existing_skill"));
                    data.insert("deterministic_stage".to_string(), json!(false));
                    data.insert("deterministic_generation".to_string(), json!("skipped"));
                    add_skiller_forge_observation_data(&mut data, &forge_request, forge_system_prompt, forge_prompt, response_template, &forge_model_target);
                    Observation { ok: true, content: format!("Imported pre-existing Skiller skill bundle {bundle_id} to {out_display} ({skill_count} skills, {source_count} sources). Deterministic raw-source generation was skipped. ScriptGeneration Forge refinement is required by default before treating this as agent-ready: use the included ForgeRequestEnvelope ({request_id}) as model context, then apply the model's ForgeResponseEnvelope with skiller_forge_apply."), data, error: None }
                }
                Err(error) => Observation::err(error.to_string(), "SkillerImportSkillError"),
            }
        }),
        json!({"required": ["input"], "properties": {"input": "string", "out": "string", "name": "string", "domain": "string"}}),
        true,
    ))?;

    let skiller_validate_sandbox = sandbox.clone();
    let skiller_validate_data_root = data_root.clone();
    let skiller_validate_global_roots = global_skill_roots.clone();
    registry.register(Tool::new(
        "skiller_validate",
        "Validate a Skiller skill bundle from inside Vegvisir.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else {
                return Observation::err("Missing bundle", "ValueError");
            };
            let bundle_ref = skiller_bundle_reference_path(bundle, &skiller_validate_data_root);
            let bundle_path = match resolve_workspace_or_global_path(
                &skiller_validate_sandbox,
                &bundle_ref,
                &skiller_validate_global_roots,
            ) {
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
    let skiller_route_data_root = data_root.clone();
    let skiller_route_global_roots = global_skill_roots.clone();
    registry.register(Tool::new(
        "skiller_route",
        "Route a user task/query to matching skills in a Skiller bundle.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else { return Observation::err("Missing bundle", "ValueError"); };
            let Some(query) = args.get("query").and_then(Value::as_str) else { return Observation::err("Missing query", "ValueError"); };
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(5).clamp(1, 50) as usize;
            let bundle_ref = skiller_bundle_reference_path(bundle, &skiller_route_data_root);
            let bundle_path = match resolve_workspace_or_global_path(&skiller_route_sandbox, &bundle_ref, &skiller_route_global_roots) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
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
    let skiller_load_data_root = data_root.clone();
    let skiller_load_global_roots = global_skill_roots.clone();
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
            let bundle_ref = skiller_bundle_reference_path(bundle, &skiller_load_data_root);
            let bundle_path = match resolve_workspace_or_global_path(&skiller_load_sandbox, &bundle_ref, &skiller_load_global_roots) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            match skiller_registry::read_bundle(&bundle_path).and_then(|bundle_data| skiller_runtime::load_skill(&bundle_data, skill_id, mode)) {
                Ok(content) => Observation::ok(content),
                Err(error) => Observation::err(error.to_string(), "SkillerLoadError"),
            }
        }),
        json!({"required": ["bundle", "skill_id"], "properties": {"bundle": "string", "skill_id": "string", "mode": "string"}}),
        false,
    ))?;

    let skiller_suspicious_sandbox = sandbox.clone();
    let skiller_suspicious_data_root = data_root.clone();
    let skiller_suspicious_global_roots = global_skill_roots.clone();
    registry.register(Tool::new(
        "skiller_suspicious_commands",
        "Summarize suspicious Skiller CliOperation targets, weak titles, and fallback-only Forge state without dumping full bundle YAML.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else {
                return Observation::err("Missing bundle", "ValueError");
            };
            let bundle_ref = skiller_bundle_reference_path(bundle, &skiller_suspicious_data_root);
            let bundle_path = match resolve_workspace_or_global_path(&skiller_suspicious_sandbox, &bundle_ref, &skiller_suspicious_global_roots) {
                Ok(path) => path,
                Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
            };
            match skiller_registry::read_bundle(&bundle_path) {
                Ok(bundle_data) => {
                    let mut findings = Vec::new();
                    let mut suspicious_cli_operation_count = 0usize;
                    let mut weak_title_count = 0usize;
                    for skill in &bundle_data.skills {
                        if matches!(skill.skill_type, skiller::models::SkillType::CliOperation) {
                            match skill.metadata.get("target_command") {
                                Some(command) => {
                                    if let Some(reason) = skiller_semantic::suspicious_cli_command_reason(command) {
                                        suspicious_cli_operation_count += 1;
                                        findings.push(json!({
                                            "kind": "suspicious_cli_operation",
                                            "skill_id": skill.id,
                                            "title": skill.title,
                                            "target_command": command,
                                            "reason": reason,
                                        }));
                                    }
                                }
                                None => {
                                    suspicious_cli_operation_count += 1;
                                    findings.push(json!({
                                        "kind": "suspicious_cli_operation",
                                        "skill_id": skill.id,
                                        "title": skill.title,
                                        "reason": "CliOperation has no target_command metadata",
                                    }));
                                }
                            }
                        }
                        if skiller_semantic::looks_like_weak_title(&skill.title) {
                            weak_title_count += 1;
                            findings.push(json!({
                                "kind": "weak_title",
                                "skill_id": skill.id,
                                "title": skill.title,
                                "target_command": skill.metadata.get("target_command"),
                                "reason": "title looks like a markdown/list/table/source fragment",
                            }));
                        }
                    }
                    let fallback_only_forge = bundle_data.forge_requests.iter().any(|request| {
                        request
                            .provider_provenance
                            .as_ref()
                            .map(|provenance| !provenance.live_reasoning)
                            .unwrap_or_else(|| request.provider.eq_ignore_ascii_case("vegvisir"))
                    });
                    if fallback_only_forge {
                        findings.push(json!({
                            "kind": "forge_provider_review",
                            "reason": "provider semantic review was not performed; Forge history lacks live provider-backed provenance",
                        }));
                    }
                    let report = json!({
                        "suspicious_cli_operation_count": suspicious_cli_operation_count,
                        "weak_title_count": weak_title_count,
                        "fallback_only_forge": fallback_only_forge,
                        "provider_reviewed": !fallback_only_forge,
                        "findings": findings,
                    });
                    let content = serde_json::to_string_pretty(&report).unwrap_or_else(|_| report.to_string());
                    let mut data = Map::new();
                    data.insert("report".into(), report.clone());
                    data.insert("suspicious_cli_operation_count".into(), json!(suspicious_cli_operation_count));
                    data.insert("weak_title_count".into(), json!(weak_title_count));
                    data.insert("fallback_only_forge".into(), json!(fallback_only_forge));
                    Observation { ok: true, content, data, error: None }
                }
                Err(error) => Observation::err(error.to_string(), "SkillerSuspiciousCommandsError"),
            }
        }),
        json!({"required": ["bundle"], "properties": {"bundle": "string"}}),
        false,
    ))?;

    let skiller_eval_sandbox = sandbox.clone();
    let skiller_eval_data_root = data_root.clone();
    let skiller_eval_global_roots = global_skill_roots.clone();
    registry.register(Tool::new(
        "skiller_eval",
        "Run deterministic structural evals for a Skiller bundle from inside Vegvisir.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else {
                return Observation::err("Missing bundle", "ValueError");
            };
            let bundle_ref = skiller_bundle_reference_path(bundle, &skiller_eval_data_root);
            let bundle_path = match resolve_workspace_or_global_path(
                &skiller_eval_sandbox,
                &bundle_ref,
                &skiller_eval_global_roots,
            ) {
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
    let skiller_readiness_data_root = data_root.clone();
    let skiller_readiness_global_roots = global_skill_roots.clone();
    registry.register(Tool::new(
        "skiller_readiness",
        "Assess Skiller bundle registry publication readiness from inside Vegvisir.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else {
                return Observation::err("Missing bundle", "ValueError");
            };
            let bundle_ref = skiller_bundle_reference_path(bundle, &skiller_readiness_data_root);
            let bundle_path = match resolve_workspace_or_global_path(
                &skiller_readiness_sandbox,
                &bundle_ref,
                &skiller_readiness_global_roots,
            ) {
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
    let skiller_forge_request_data_root = data_root.clone();
    let skiller_forge_request_global_roots = global_skill_roots.clone();
    let skiller_forge_request_model_target_defaults = skiller_forge_model_target_defaults.clone();
    registry.register(Tool::new(
        "skiller_forge_request",
        "Build a strict Vegvisir-provider Skiller Forge request envelope and model prompt for native agent/provider execution.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else { return Observation::err("Missing bundle", "ValueError"); };
            let pass = match parse_skiller_forge_pass(args.get("pass").and_then(Value::as_str)) { Ok(pass) => pass, Err(error) => return Observation::err(error.to_string(), "ValueError") };
            let domain_profile = args.get("domain_profile").and_then(Value::as_str);
            let max_skills = args.get("max_skills").and_then(Value::as_u64).unwrap_or(8).clamp(1, 100) as usize;
            let forge_model_target = match skiller_forge_request_model_target_defaults.resolve() { Ok(target) => target, Err(error) => return Observation::err(error.to_string(), "SkillerForgeModelTargetError") };
            let bundle_ref = skiller_bundle_reference_path(bundle, &skiller_forge_request_data_root);
            let bundle_path = match resolve_workspace_or_global_path(&skiller_forge_request_sandbox, &bundle_ref, &skiller_forge_request_global_roots) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            match skiller_registry::read_bundle(&bundle_path).map(|bundle_data| {
                let mut request = skiller_forge::build_vegvisir_handoff(&bundle_data, pass, domain_profile, max_skills);
                apply_skiller_forge_model_target(&mut request, &forge_model_target);
                request
            }) {
                Ok(request) => {
                    let system_prompt = skiller_forge::skiller_specialized_vegvisir_system_prompt().to_string();
                    let prompt = skiller_forge::vegvisir_prompt_markdown(&request);
                    let template = skiller_forge::response_template_for(&request);
                    let mut data = Map::new();
                    data.insert("request_id".to_string(), json!(request.request_id));
                    data.insert("provider".to_string(), json!(request.provider));
                    data.insert("model_provider".to_string(), json!(forge_model_target.provider));
                    data.insert("model".to_string(), json!(forge_model_target.model));
                    data.insert("forge_model_target_source".to_string(), json!(forge_model_target.source));
                    data.insert("pass_type".to_string(), json!(format!("{:?}", request.pass_type)));
                    data.insert("selected_skill_count".to_string(), json!(request.candidate_skills.len()));
                    data.insert("default_objective".to_string(), json!(skiller_forge::skiller_default_forge_objective()));
                    data.insert("system_prompt".to_string(), json!(system_prompt));
                    data.insert("request".to_string(), serde_json::to_value(&request).unwrap_or(Value::Null));
                    data.insert("response_template".to_string(), serde_json::to_value(&template).unwrap_or(Value::Null));
                    data.insert("prompt".to_string(), json!(prompt));
                    Observation { ok: true, content: format!("Default Forge model target: {}:{} ({})\n\n{}", forge_model_target.provider, forge_model_target.model, forge_model_target.source, prompt), data, error: None }
                }
                Err(error) => Observation::err(error.to_string(), "SkillerForgeRequestError"),
            }
        }),
        json!({"required": ["bundle"], "properties": {"bundle": "string", "pass": "string", "domain_profile": "string", "max_skills": "integer"}}),
        false,
    ))?;

    let skiller_forge_apply_sandbox = sandbox.clone();
    let skiller_forge_apply_data_root = data_root.clone();
    let skiller_forge_apply_global_roots = global_skill_roots.clone();
    registry.register(Tool::new(
        "skiller_forge_apply",
        "Validate and apply a Vegvisir-generated Skiller Forge response envelope to a bundle, writing simple output names to the user-global Skiller bundle store.",
        Arc::new(move |args| {
            let Some(bundle) = args.get("bundle").and_then(Value::as_str) else { return Observation::err("Missing bundle", "ValueError"); };
            let Some(out) = args.get("out").and_then(Value::as_str) else { return Observation::err("Missing out", "ValueError"); };
            let out_path_raw = skiller_bundle_output_path(&args, &skiller_forge_apply_data_root, out);
            let out_display = out_path_raw.display().to_string();
            let request_value = match args.get("request") { Some(value) => value.clone(), None => return Observation::err("Missing request", "ValueError") };
            let response_text = args.get("response").and_then(Value::as_str);
            let response_value = args.get("response_envelope").cloned();
            let bundle_ref = skiller_bundle_reference_path(bundle, &skiller_forge_apply_data_root);
            let bundle_path = match resolve_workspace_or_global_path(&skiller_forge_apply_sandbox, &bundle_ref, &skiller_forge_apply_global_roots) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
            let out_path = match resolve_workspace_or_global_path(&skiller_forge_apply_sandbox, &out_path_raw, &skiller_forge_apply_global_roots) { Ok(path) => path, Err(error) => return Observation::err(error.to_string(), "SandboxViolation") };
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
                    data.insert("out".to_string(), json!(out_display));
                    data.insert("apply_report".to_string(), serde_json::to_value(&report).unwrap_or(Value::Null));
                    Observation { ok: true, content: format!("Applied Vegvisir Skiller Forge response {} to {out_display} (skills: {} -> {}, human_review_required={}).", report.request_id, report.before_skill_count, report.after_skill_count, report.required_human_review), data, error: None }
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

    let subagent_board_path = subagent_data_root.join("subagents.json");
    let subagent_root = sandbox.root.clone();
    let subagent_sandbox = sandbox.clone();
    let spawn_subagent_board_path = subagent_board_path.clone();
    let spawn_subagent_provider_defaults = subagent_provider_defaults.clone();
    let spawn_subagent_defaults = subagent_spawn_defaults.clone();
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
            let requested_max_steps = args.get("max_steps").and_then(Value::as_u64);
            let effective_max_steps = spawn_subagent_defaults.effective_max_steps(requested_max_steps);
            let max_steps = effective_max_steps.to_string();
            let requested_provider = optional_nonempty_string(args.get("provider"));
            let requested_model = optional_nonempty_string(args.get("model"));
            let (provider, model) = match spawn_subagent_provider_defaults
                .resolve_for_spawn_request(requested_provider, requested_model)
            {
                Ok(resolved) => resolved,
                Err(error) => return Observation::err(error.to_string(), "SubagentProviderModelError"),
            };
            let agent = optional_nonempty_string(args.get("agent"));
            let work_budget = parse_subagent_work_budget(
                args.get("work_budget"),
                Some(effective_max_steps),
                &spawn_subagent_defaults.work_budget,
            );
            let workspace_scope_sandbox = match if bypass_sandbox {
                WorkspaceSandbox::new_unrestricted(&workspace)
            } else {
                WorkspaceSandbox::new(&workspace)
            } {
                Ok(sandbox) => sandbox,
                Err(error) => return Observation::err(error.to_string(), "SandboxViolation"),
            };
            let file_scope = match parse_subagent_file_scope(args.get("file_scope"), &workspace_scope_sandbox) {
                Ok(scope) => scope,
                Err(error) => return Observation::err(error.to_string(), "InvalidFileScope"),
            };

            if !bypass_sandbox {
                return Observation::err(
                    "Subagent spawning is currently limited to Vegvisir YOLO mode (--dangerously-bypass-approvals-and-sandbox). Restart Vegvisir in YOLO mode to delegate child agents.".to_string(),
                    "SubagentRequiresYolo",
                );
            }

            let board_path = spawn_subagent_board_path.clone();
            match active_subagent_count(&board_path) {
                Ok(active) if active >= active_subagent_limit => {
                    return Observation::err(
                        format!(
                            "Maximum active subagents reached ({active_subagent_limit}). Wait for a running task to finish or cancel one with /subagents cancel <id>."
                        ),
                        "SubagentLimit",
                    );
                }
                Err(error) => return Observation::err(error.to_string(), "SubagentBoardError"),
                _ => {}
            }
            if let Err(error) = validate_subagent_file_scope_available(&board_path, &file_scope) {
                return Observation::err(error.to_string(), "SubagentScopeConflict");
            }
            let record = SubAgentTaskRecord {
                id: Uuid::new_v4().to_string(),
                name: name.clone(),
                workspace: workspace.clone(),
                goal: goal.to_string(),
                parent_run_id: None,
                child_run_id: None,
                artifact_dir: None,
                ownership: Some(SubAgentFileOwnership {
                    read_scope: file_scope.clone(),
                    write_scope: Vec::new(),
                    exclusive_write: true,
                }),
                provider: Some(provider.clone()),
                model: Some(model.clone()),
                file_scope: file_scope.clone(),
                work_budget: work_budget.clone(),
                status: SubAgentStatus::Queued,
                created_at: Utc::now(),
                started_at: None,
                finished_at: None,
                checkpoint: None,
                final_answer: None,
                error: None,
                observability: SubAgentObservability {
                    launch_env_keys: vec!["VEGVISIR_SUBAGENT_RUN".to_string()],
                    ..SubAgentObservability::default()
                },
            };
            if let Err(error) = upsert_subagent_record(&board_path, record.clone()) {
                return Observation::err(error.to_string(), "SubagentBoardError");
            }

            let child_record = record.clone();
            let child_provider = provider.clone();
            let child_model = model.clone();
            let child_goal = apply_subagent_scope_to_goal(
                &apply_subagent_work_budget_to_goal(goal, &work_budget),
                &workspace,
                &file_scope,
            );
            thread::spawn(move || {
                run_spawned_subagent(
                    board_path,
                    child_record,
                    child_goal,
                    workspace,
                    max_steps,
                    child_provider,
                    child_model,
                    agent,
                    bypass_sandbox,
                    work_budget,
                );
            });

            let mut data = Map::new();
            data.insert("id".to_string(), json!(record.id));
            data.insert("name".to_string(), json!(record.name));
            data.insert("workspace".to_string(), json!(record.workspace));
            data.insert("provider".to_string(), json!(provider));
            data.insert("model".to_string(), json!(model));
            data.insert("file_scope".to_string(), json!(record.file_scope));
            data.insert("max_steps".to_string(), json!(effective_max_steps));
            data.insert("work_budget".to_string(), json!(record.work_budget));
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
                "agent": "string",
                "file_scope": "array",
                "work_budget": "object"
            }
        }),
        false,
    ))?;

    let subagents_list_board_path = subagent_board_path.clone();
    registry.register(Tool::new(
        "subagents_list",
        "List subagent task board records visible to the current Vegvisir session.",
        Arc::new(move |args| {
            let status_filter = optional_nonempty_string(args.get("status"))
                .map(|value| value.to_ascii_lowercase())
                .filter(|value| !matches!(value.as_str(), "all" | "any" | "*"));
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|value| value as usize)
                .unwrap_or(50)
                .clamp(1, 500);
            let mut records = match load_subagent_board_records(&subagents_list_board_path) {
                Ok(records) => records,
                Err(error) => return Observation::err(error.to_string(), "SubagentBoardError"),
            };
            records.sort_by(|left, right| right.created_at.cmp(&left.created_at));
            if let Some(status_filter) = status_filter {
                records.retain(|record| subagent_status_label(&record.status) == status_filter);
            }
            let total_records = records.len();
            let truncated = total_records > limit;
            records.truncate(limit);
            let mut data = Map::new();
            data.insert("board_path".to_string(), json!(subagents_list_board_path));
            data.insert("records".to_string(), json!(records));
            data.insert("total_records".to_string(), json!(total_records));
            data.insert("output_truncated".to_string(), json!(truncated));
            let content = if records.is_empty() {
                "No subagent task records.".to_string()
            } else {
                records
                    .iter()
                    .map(format_subagent_record_summary)
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            Observation {
                ok: true,
                content,
                data,
                error: None,
            }
        }),
        json!({"properties": {"status": "string", "limit": "integer"}}),
        false,
    ))?;

    let subagents_show_board_path = subagent_board_path.clone();
    registry.register(Tool::new(
        "subagents_show",
        "Show one subagent task board record by id or name.",
        Arc::new(move |args| {
            let Some(id_or_name) = args
                .get("id_or_name")
                .and_then(Value::as_str)
                .map(str::trim)
            else {
                return Observation::err("Missing id_or_name", "ValueError");
            };
            if id_or_name.is_empty() {
                return Observation::err("id_or_name must not be empty", "ValueError");
            }
            let records = match load_subagent_board_records(&subagents_show_board_path) {
                Ok(records) => records,
                Err(error) => return Observation::err(error.to_string(), "SubagentBoardError"),
            };
            let Some(record) = find_subagent_record_in(records, id_or_name) else {
                return Observation::err(
                    format!("Unknown subagent task: {id_or_name}"),
                    "NotFound",
                );
            };
            let mut data = Map::new();
            data.insert("board_path".to_string(), json!(subagents_show_board_path));
            data.insert("record".to_string(), json!(record));
            Observation {
                ok: true,
                content: match serde_json::to_string_pretty(&record) {
                    Ok(content) => content,
                    Err(error) => return Observation::err(error.to_string(), "SerializationError"),
                },
                data,
                error: None,
            }
        }),
        json!({"required": ["id_or_name"], "properties": {"id_or_name": "string"}}),
        false,
    ))?;

    let subagents_cancel_board_path = subagent_board_path;
    registry.register(Tool::new(
        "subagents_cancel",
        "Cancel one queued or running subagent task by id or name on the subagent board.",
        Arc::new(move |args| {
            let Some(id_or_name) = args
                .get("id_or_name")
                .and_then(Value::as_str)
                .map(str::trim)
            else {
                return Observation::err("Missing id_or_name", "ValueError");
            };
            if id_or_name.is_empty() {
                return Observation::err("id_or_name must not be empty", "ValueError");
            }
            let mut records = match load_subagent_board_records(&subagents_cancel_board_path) {
                Ok(records) => records,
                Err(error) => return Observation::err(error.to_string(), "SubagentBoardError"),
            };
            let Some(record) = records
                .iter_mut()
                .find(|record| record.id == id_or_name || record.name == id_or_name)
            else {
                return Observation::err(
                    format!("Unknown subagent task: {id_or_name}"),
                    "NotFound",
                );
            };
            if matches!(
                record.status,
                SubAgentStatus::Completed | SubAgentStatus::Failed | SubAgentStatus::Cancelled
            ) {
                let mut data = Map::new();
                data.insert("board_path".to_string(), json!(subagents_cancel_board_path));
                data.insert("record".to_string(), json!(record.clone()));
                return Observation {
                    ok: true,
                    content: format!(
                        "Subagent task {} is already {:?}.",
                        record.id, record.status
                    ),
                    data,
                    error: None,
                };
            }
            record.status = SubAgentStatus::Cancelled;
            record.finished_at = Some(Utc::now());
            let cancelled = record.clone();
            if let Err(error) = save_subagent_board_records(&subagents_cancel_board_path, &records)
            {
                return Observation::err(error.to_string(), "SubagentBoardError");
            }
            let mut data = Map::new();
            data.insert("board_path".to_string(), json!(subagents_cancel_board_path));
            data.insert("record".to_string(), json!(cancelled.clone()));
            Observation {
                ok: true,
                content: format!("Cancelled subagent task {}.", cancelled.id),
                data,
                error: None,
            }
        }),
        json!({"required": ["id_or_name"], "properties": {"id_or_name": "string"}}),
        false,
    ))?;

    Ok(registry)
}

fn parse_subagent_work_budget(
    value: Option<&Value>,
    max_steps_value: Option<u64>,
    default_budget: &SubAgentWorkBudget,
) -> SubAgentWorkBudget {
    let mut budget = default_budget.clone();
    budget.max_steps = max_steps_value;
    let Some(Value::Object(object)) = value else {
        return budget;
    };
    if let Some(value) = object.get("max_tool_calls").and_then(Value::as_u64) {
        budget.max_tool_calls = Some(value);
    }
    if let Some(value) = object.get("max_read_bytes").and_then(Value::as_u64) {
        budget.max_read_bytes = Some(value);
    }
    if let Some(value) = object.get("max_output_bytes").and_then(Value::as_u64) {
        budget.max_output_bytes = Some(value);
    }
    if let Some(items) = object.get("allowed_tools").and_then(Value::as_array) {
        budget.allowed_tools = items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect();
    }
    if let Some(notes) = object.get("notes").and_then(Value::as_str) {
        budget.notes = notes.trim().to_string();
    }
    budget
}

fn apply_subagent_work_budget_to_goal(goal: &str, budget: &SubAgentWorkBudget) -> String {
    let mut lines = Vec::new();
    lines.push("[Vegvisir subagent work budget]".to_string());
    lines.push("This is a hard task-local budget envelope. Stay inside it and report if more budget is needed.".to_string());
    if let Some(value) = budget.max_steps {
        lines.push(format!("- max_steps: {value}"));
    }
    if let Some(value) = budget.max_tool_calls {
        lines.push(format!("- max_tool_calls: {value}"));
    }
    if let Some(value) = budget.max_read_bytes {
        lines.push(format!("- max_read_bytes_per_file: {value}"));
    }
    if let Some(value) = budget.max_output_bytes {
        lines.push(format!("- max_final_output_bytes: {value}"));
    }
    if !budget.allowed_tools.is_empty() {
        lines.push(format!(
            "- allowed_tools: {}",
            budget.allowed_tools.join(", ")
        ));
    }
    if !budget.notes.trim().is_empty() {
        lines.push(format!("- notes: {}", budget.notes.trim()));
    }
    lines.push("- If the task cannot be completed within this budget, stop with a concise blocked/needs-more-budget report.".to_string());
    lines.push("[/Vegvisir subagent work budget]".to_string());
    lines.push("".to_string());
    lines.push("[Vegvisir subagent final report contract]".to_string());
    lines.push("End with these sections: Task understood; Files inspected; Tools used; Findings; Changes made; Verification run; Risks/blockers.".to_string());
    lines.push("[/Vegvisir subagent final report contract]".to_string());
    format!("{}\n\nSubagent task:\n{}", lines.join("\n"), goal.trim())
}

fn apply_subagent_scope_to_goal(goal: &str, workspace: &Path, file_scope: &[PathBuf]) -> String {
    if file_scope.is_empty() {
        return goal.to_string();
    }
    let scope_lines = file_scope
        .iter()
        .map(|path| {
            path.strip_prefix(workspace)
                .unwrap_or(path)
                .display()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    format!(
        "[Vegvisir subagent file scope]
Workspace: {}
Assigned file_scope paths are workspace-relative unless shown absolute. Treat these as your coordination boundary; inspect or modify only within this scope unless explicitly asked for broader review.
{}
[/Vegvisir subagent file scope]

{}",
        workspace.display(),
        scope_lines,
        goal.trim()
    )
}

fn optional_nonempty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| {
            let normalized = value.to_ascii_lowercase();
            !value.is_empty() && !matches!(normalized.as_str(), "default" | "none" | "null")
        })
        .map(str::to_string)
}

fn executable_exists(path: &Path) -> bool {
    path.is_file() || path.exists()
}

fn path_lookup_executable(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| executable_exists(candidate))
}

fn is_vegvisir_executable_name(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("vegvisir") | Some("vegvisir-rust")
    )
}

fn resolve_vegvisir_executable(workspace: &Path) -> anyhow::Result<PathBuf> {
    let mut checked = Vec::<PathBuf>::new();

    if let Some(path) = std::env::var_os("VEGVISIR_BIN").map(PathBuf::from) {
        checked.push(path.clone());
        if executable_exists(&path) {
            return Ok(path);
        }
    }

    if let Ok(current) = std::env::current_exe() {
        checked.push(current.clone());
        if executable_exists(&current) && is_vegvisir_executable_name(&current) {
            return Ok(current);
        }
        if let Some(parent) = current.parent() {
            for candidate in [parent.join("vegvisir"), parent.join("vegvisir-rust")] {
                checked.push(candidate.clone());
                if executable_exists(&candidate) {
                    return Ok(candidate);
                }
            }
        }
    }

    for candidate in [
        workspace.join("target/debug/vegvisir"),
        workspace.join("target/debug/vegvisir-rust"),
        workspace.join("target/release/vegvisir"),
        workspace.join("target/release/vegvisir-rust"),
        workspace.join("vegvisir/target/debug/vegvisir"),
        workspace.join("vegvisir/target/debug/vegvisir-rust"),
        workspace.join("vegvisir/target/release/vegvisir"),
        workspace.join("vegvisir/target/release/vegvisir-rust"),
    ] {
        checked.push(candidate.clone());
        if executable_exists(&candidate) {
            return Ok(candidate);
        }
    }

    for name in ["vegvisir", "vegvisir-rust"] {
        if let Some(candidate) = path_lookup_executable(name) {
            checked.push(candidate.clone());
            return Ok(candidate);
        }
    }

    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for candidate in [
            home.join(".local/bin/vegvisir"),
            home.join(".local/bin/vegvisir-rust"),
            home.join(".cargo/bin/vegvisir"),
            home.join(".cargo/bin/vegvisir-rust"),
        ] {
            checked.push(candidate.clone());
            if executable_exists(&candidate) {
                return Ok(candidate);
            }
        }
    }

    anyhow::bail!(
        "could not resolve Vegvisir executable for subagent launch; set VEGVISIR_BIN to the vegvisir/vegvisir-rust binary. Checked: {}",
        checked
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SubagentChildLaunch {
    goal: String,
    workspace: PathBuf,
    max_steps: String,
    provider: String,
    model: String,
    agent: Option<String>,
    bypass_sandbox: bool,
    work_budget: SubAgentWorkBudget,
}

fn subagent_child_env(launch: &SubagentChildLaunch) -> Vec<(String, String)> {
    let mut env = vec![("VEGVISIR_SUBAGENT_RUN".to_string(), "1".to_string())];
    if let Some(limit) = launch
        .work_budget
        .max_tool_calls
        .or(launch.work_budget.max_steps)
    {
        env.push((
            "VEGVISIR_MAX_TOOL_ROUNDS".to_string(),
            limit.max(1).to_string(),
        ));
    }
    if let Some(limit) = launch.work_budget.max_read_bytes {
        env.push((
            "VEGVISIR_SUBAGENT_MAX_READ_BYTES".to_string(),
            limit.max(1).to_string(),
        ));
    }
    if let Some(limit) = launch.work_budget.max_output_bytes {
        env.push((
            "VEGVISIR_SUBAGENT_MAX_OUTPUT_BYTES".to_string(),
            limit.max(1).to_string(),
        ));
    }
    if !launch.work_budget.allowed_tools.is_empty() {
        env.push((
            "VEGVISIR_SUBAGENT_ALLOWED_TOOLS".to_string(),
            launch.work_budget.allowed_tools.join(","),
        ));
    }
    env
}

fn subagent_child_argv(launch: SubagentChildLaunch) -> Vec<String> {
    let mut argv = Vec::<String>::new();
    argv.push("--json".to_string());
    argv.push("--isolated-session".to_string());
    if launch.bypass_sandbox {
        argv.push("--dangerously-bypass-approvals-and-sandbox".to_string());
    }
    let agent = launch.agent;
    if agent.is_none() {
        argv.push("--provider".to_string());
        argv.push(launch.provider);
        argv.push("--model".to_string());
        argv.push(launch.model);
    }
    argv.push("run".to_string());
    argv.push(launch.goal);
    argv.push("--workspace".to_string());
    argv.push(launch.workspace.display().to_string());
    argv.push("--max-steps".to_string());
    argv.push(launch.max_steps);
    if let Some(agent) = agent {
        argv.push("--agent".to_string());
        argv.push(agent);
    }
    argv
}

#[allow(clippy::too_many_arguments)]
fn run_spawned_subagent(
    board_path: PathBuf,
    mut record: SubAgentTaskRecord,
    goal: String,
    workspace: PathBuf,
    max_steps: String,
    provider: String,
    model: String,
    agent: Option<String>,
    bypass_sandbox: bool,
    work_budget: SubAgentWorkBudget,
) {
    record.status = SubAgentStatus::Running;
    record.started_at = Some(Utc::now());
    let before_snapshot = snapshot_workspace_files(&workspace);
    let _ = upsert_subagent_record(&board_path, record.clone());

    let result = (|| -> anyhow::Result<String> {
        let executable = resolve_vegvisir_executable(&workspace)?;
        let launch = SubagentChildLaunch {
            goal,
            workspace: workspace.clone(),
            max_steps,
            provider,
            model,
            agent,
            bypass_sandbox,
            work_budget: work_budget.clone(),
        };
        let env = subagent_child_env(&launch);
        let argv = subagent_child_argv(launch);
        record.observability.launch_argv = std::iter::once(executable.display().to_string())
            .chain(argv.iter().cloned())
            .collect();
        record.observability.launch_env_keys = env.iter().map(|(key, _)| key.clone()).collect();
        let _ = upsert_subagent_record(&board_path, record.clone());
        let output = Command::new(&executable)
            .args(&argv)
            .current_dir(&workspace)
            .envs(env.iter().map(|(key, value)| (key, value)))
            .output()
            .map_err(|error| {
                format_subagent_spawn_os_error(&executable, &argv, &workspace, &env, &error)
            })?;
        let mut text = String::new();
        text.push_str(&String::from_utf8_lossy(&output.stdout));
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        record
            .observability
            .events
            .extend(parse_subagent_json_observability(&text));
        if !output.status.success() {
            anyhow::bail!("{}", text.trim());
        }
        Ok(text)
    })();

    let after_snapshot = snapshot_workspace_files(&workspace);
    record.observability.file_changes = diff_workspace_snapshots(
        &workspace,
        before_snapshot.as_ref().ok(),
        after_snapshot.as_ref().ok(),
    );
    if let Err(error) = before_snapshot {
        record.observability.notes.push(format!(
            "Could not snapshot workspace before subagent run: {error}"
        ));
    }
    if let Err(error) = after_snapshot {
        record.observability.notes.push(format!(
            "Could not snapshot workspace after subagent run: {error}"
        ));
    }

    match result {
        Ok(output) => {
            record.status = SubAgentStatus::Completed;
            record.finished_at = Some(Utc::now());
            record.final_answer = Some(
                work_budget
                    .max_output_bytes
                    .map(|limit| {
                        compact_text_middle(
                            &output,
                            usize::try_from(limit).unwrap_or(usize::MAX),
                            "subagent output",
                        )
                    })
                    .unwrap_or(output),
            );
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

fn format_subagent_spawn_os_error(
    executable: &Path,
    argv: &[String],
    workspace: &Path,
    env: &[(String, String)],
    error: &std::io::Error,
) -> anyhow::Error {
    let os_code = error
        .raw_os_error()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "none".to_string());
    let kind = format!("{:?}", error.kind());
    let path_value = std::env::var_os("PATH")
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "<unset>".to_string());
    let env_keys = env
        .iter()
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let remediation = match error.kind() {
        std::io::ErrorKind::NotFound => {
            "Executable was not found from the child process environment. Set VEGVISIR_BIN to an absolute vegvisir/vegvisir-rust binary, verify the binary exists, and verify Desktop inherited PATH."
        }
        std::io::ErrorKind::PermissionDenied => {
            "Executable exists but is not runnable. Check chmod +x, filesystem mount noexec flags, and launcher permissions."
        }
        _ => "Inspect executable, workspace cwd, PATH, and child environment shown above.",
    };
    anyhow::anyhow!(
        "subagent process spawn failed\n  executable: {}\n  argv: {}\n  cwd: {}\n  os_error_kind: {}\n  os_error_code: {}\n  os_error: {}\n  PATH: {}\n  child_env_keys: {}\n  remediation: {}",
        executable.display(),
        argv.join(" "),
        workspace.display(),
        kind,
        os_code,
        error,
        path_value,
        env_keys,
        remediation
    )
}

fn parse_subagent_json_observability(output: &str) -> Vec<SubAgentObservedEvent> {
    let Ok(value) = serde_json::from_str::<Value>(output.trim()) else {
        return Vec::new();
    };
    value
        .get("events")
        .and_then(Value::as_array)
        .map(|events| {
            events
                .iter()
                .filter_map(parse_subagent_event)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_subagent_event(event: &Value) -> Option<SubAgentObservedEvent> {
    let kind = event.get("kind").and_then(Value::as_str)?;
    match kind {
        "tool_start" => Some(SubAgentObservedEvent {
            kind: SubAgentObservedEventKind::ToolStart,
            name: event
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string),
            args: event
                .get("args")
                .and_then(Value::as_str)
                .map(str::to_string),
            ok: None,
            summary: None,
            detail: None,
        }),
        "tool_end" => Some(SubAgentObservedEvent {
            kind: SubAgentObservedEventKind::ToolEnd,
            name: event
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string),
            args: None,
            ok: event.get("ok").and_then(Value::as_bool),
            summary: event
                .get("summary")
                .and_then(Value::as_str)
                .map(str::to_string),
            detail: event
                .get("detail")
                .and_then(Value::as_str)
                .map(str::to_string),
        }),
        "activity" => Some(SubAgentObservedEvent {
            kind: SubAgentObservedEventKind::Activity,
            name: None,
            args: None,
            ok: None,
            summary: event
                .get("activity")
                .and_then(Value::as_str)
                .map(str::to_string),
            detail: None,
        }),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkspaceFileSnapshot {
    bytes: u64,
    hash: u64,
    content: Option<String>,
}

fn snapshot_workspace_files(
    workspace: &Path,
) -> anyhow::Result<BTreeMap<PathBuf, WorkspaceFileSnapshot>> {
    let mut snapshot = BTreeMap::new();
    if !workspace.exists() {
        return Ok(snapshot);
    }
    for entry in WalkDir::new(workspace).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.components().any(|component| {
            matches!(
                component,
                std::path::Component::Normal(name)
                    if name == ".git" || name == "target" || name == ".vegvisir"
            )
        }) {
            continue;
        }
        let relative = path.strip_prefix(workspace).unwrap_or(path).to_path_buf();
        let metadata = entry.metadata()?;
        let bytes = metadata.len();
        let hash = hash_file(path)?;
        let content = if bytes <= SUBAGENT_DIFF_TEXT_MAX_BYTES {
            std::fs::read(path)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        } else {
            None
        };
        snapshot.insert(
            relative,
            WorkspaceFileSnapshot {
                bytes,
                hash,
                content,
            },
        );
    }
    Ok(snapshot)
}

fn hash_file(path: &Path) -> anyhow::Result<u64> {
    let mut file = File::open(path)?;
    let mut hasher = DefaultHasher::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        buffer[..read].hash(&mut hasher);
    }
    Ok(hasher.finish())
}

fn diff_workspace_snapshots(
    _workspace: &Path,
    before: Option<&BTreeMap<PathBuf, WorkspaceFileSnapshot>>,
    after: Option<&BTreeMap<PathBuf, WorkspaceFileSnapshot>>,
) -> Vec<SubAgentFileChange> {
    let (Some(before), Some(after)) = (before, after) else {
        return Vec::new();
    };
    let mut changes = Vec::new();
    for (path, after_file) in after {
        match before.get(path) {
            None => changes.push(SubAgentFileChange {
                path: path.clone(),
                change: SubAgentFileChangeKind::Created,
                before_bytes: None,
                after_bytes: Some(after_file.bytes),
                diff: after_file
                    .content
                    .as_ref()
                    .map(|content| simple_unified_diff(&path.display().to_string(), "", content)),
            }),
            Some(before_file) if before_file != after_file => changes.push(SubAgentFileChange {
                path: path.clone(),
                change: SubAgentFileChangeKind::Modified,
                before_bytes: Some(before_file.bytes),
                after_bytes: Some(after_file.bytes),
                diff: before_file
                    .content
                    .as_ref()
                    .zip(after_file.content.as_ref())
                    .map(|(before, after)| {
                        simple_unified_diff(&path.display().to_string(), before, after)
                    }),
            }),
            _ => {}
        }
    }
    for (path, before_file) in before {
        if !after.contains_key(path) {
            changes.push(SubAgentFileChange {
                path: path.clone(),
                change: SubAgentFileChangeKind::Deleted,
                before_bytes: Some(before_file.bytes),
                after_bytes: None,
                diff: before_file
                    .content
                    .as_ref()
                    .map(|content| simple_unified_diff(&path.display().to_string(), content, "")),
            });
        }
    }
    changes
}

fn parse_subagent_file_scope(
    value: Option<&Value>,
    sandbox: &WorkspaceSandbox,
) -> anyhow::Result<Vec<PathBuf>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let raw_items = match value {
        Value::String(raw) => raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>(),
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>(),
        _ => anyhow::bail!("file_scope must be a string or array of workspace paths"),
    };
    let mut scope = Vec::new();
    for item in raw_items {
        let path = sandbox.resolve(&item)?;
        if !scope.contains(&path) {
            scope.push(path);
        }
    }
    Ok(scope)
}

fn load_subagent_board_records(path: &Path) -> anyhow::Result<Vec<SubAgentTaskRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path)?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    match serde_json::from_str::<Vec<SubAgentTaskRecord>>(&text) {
        Ok(records) => Ok(records),
        Err(original_error) => {
            if let Some(records) = recover_subagent_board_records(&text) {
                let _ = save_subagent_board_records(path, &records);
                Ok(records)
            } else {
                Err(original_error.into())
            }
        }
    }
}

fn recover_subagent_board_records(text: &str) -> Option<Vec<SubAgentTaskRecord>> {
    let trimmed = text.trim_start_matches(['\u{feff}', '\u{200b}', '\u{2060}']);
    let start = trimmed.find('[')?;
    let candidate = &trimmed[start..];

    if let Ok(records) = serde_json::from_str::<Vec<SubAgentTaskRecord>>(candidate) {
        return Some(records);
    }

    recover_subagent_board_records_from_partial_array(candidate)
}

fn recover_subagent_board_records_from_partial_array(
    candidate: &str,
) -> Option<Vec<SubAgentTaskRecord>> {
    let mut records = Vec::new();
    let bytes = candidate.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let start = i;
            let mut depth = 0i32;
            let mut in_string = false;
            let mut escaped = false;
            while i < bytes.len() {
                let b = bytes[i];
                if in_string {
                    if escaped {
                        escaped = false;
                    } else if b == b'\\' {
                        escaped = true;
                    } else if b == b'"' {
                        in_string = false;
                    }
                } else {
                    match b {
                        b'"' => in_string = true,
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                let slice = &candidate[start..=i];
                                if let Ok(record) =
                                    serde_json::from_str::<SubAgentTaskRecord>(slice)
                                {
                                    records.push(record);
                                }
                                break;
                            }
                        }
                        b']' if depth == 0 => break,
                        _ => {}
                    }
                }
                i += 1;
            }
        }
        i += 1;
    }

    if records.is_empty() {
        None
    } else {
        Some(records)
    }
}

fn save_subagent_board_records(path: &Path, records: &[SubAgentTaskRecord]) -> anyhow::Result<()> {
    atomic_write_json(path, &serde_json::to_string_pretty(records)?)
}

fn atomic_write_json(path: &Path, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("{}.tmp", Uuid::new_v4().simple()));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(())
}

fn find_subagent_record_in(
    records: Vec<SubAgentTaskRecord>,
    id_or_name: &str,
) -> Option<SubAgentTaskRecord> {
    records
        .into_iter()
        .find(|record| record.id == id_or_name || record.name == id_or_name)
}

fn subagent_status_label(status: &SubAgentStatus) -> &'static str {
    match status {
        SubAgentStatus::Queued => "queued",
        SubAgentStatus::Running => "running",
        SubAgentStatus::Completed => "completed",
        SubAgentStatus::Failed => "failed",
        SubAgentStatus::Cancelled => "cancelled",
    }
}

fn format_subagent_record_summary(record: &SubAgentTaskRecord) -> String {
    format!(
        "{}  name={} status={} provider={} model={} workspace={} scope={} goal={}",
        record.id,
        record.name,
        subagent_status_label(&record.status),
        record.provider.as_deref().unwrap_or("-"),
        record.model.as_deref().unwrap_or("-"),
        record.workspace.display(),
        record
            .file_scope
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(","),
        record.goal
    )
}

fn validate_subagent_file_scope_available(
    path: &Path,
    requested: &[PathBuf],
) -> anyhow::Result<()> {
    if requested.is_empty() {
        return Ok(());
    }
    let records = load_subagent_board_records(path)?;
    for record in records.into_iter().filter(|record| {
        matches!(
            record.status,
            SubAgentStatus::Queued | SubAgentStatus::Running
        )
    }) {
        if record.file_scope.is_empty() {
            continue;
        }
        for requested_path in requested {
            for active_path in &record.file_scope {
                if scopes_overlap(requested_path, active_path) {
                    anyhow::bail!(
                        "subagent file scope overlaps active task {} ({}): requested {} overlaps {}",
                        record.id,
                        record.name,
                        requested_path.display(),
                        active_path.display()
                    );
                }
            }
        }
    }
    Ok(())
}

fn scopes_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn active_subagent_count(path: &Path) -> anyhow::Result<usize> {
    let records = load_subagent_board_records(path)?;
    Ok(records
        .into_iter()
        .filter(|record| {
            matches!(
                record.status,
                SubAgentStatus::Queued | SubAgentStatus::Running
            )
        })
        .count())
}

fn upsert_subagent_record(path: &Path, record: SubAgentTaskRecord) -> anyhow::Result<()> {
    let mut records = load_subagent_board_records(path)?;
    if let Some(existing) = records.iter_mut().find(|existing| existing.id == record.id) {
        *existing = record;
    } else {
        records.push(record);
    }
    save_subagent_board_records(path, &records)
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
    use std::{
        ffi::{OsStr, OsString},
        sync::{Mutex, OnceLock},
    };
    use tempfile::TempDir;

    fn env_var_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env var test lock poisoned")
    }

    fn test_tool(schema: Value) -> Tool {
        Tool::new(
            "test_tool",
            "test tool",
            Arc::new(|_| Observation::ok("ok")),
            schema,
            false,
        )
    }

    #[test]
    fn bounded_command_drains_stdout_and_stderr_incrementally() -> anyhow::Result<()> {
        let workspace = TempDir::new()?;
        let sandbox = CommandSandboxConfig::path_only(workspace.path());
        let script = "import sys\nsys.stdout.write('O' * 131072)\nsys.stdout.flush()\nsys.stderr.write('ERR-LINE\\n')\nsys.stderr.flush()\n";

        let obs = execute_bounded_command(
            &["python3", "-c", script],
            &sandbox,
            10,
            1_000_000,
            "CommandFailed",
            true,
            false,
        );

        assert!(obs.ok, "{} {:?}", obs.content, obs.error);
        assert_eq!(obs.data.get("streaming_capture"), Some(&json!(true)));
        assert_eq!(
            obs.data.get("stream_capture_mode"),
            Some(&json!("incremental_pipe_drainers"))
        );
        assert_eq!(obs.data.get("stdout_bytes"), Some(&json!(131072)));
        assert_eq!(obs.data.get("stderr_bytes"), Some(&json!(9)));
        assert_eq!(obs.data.get("stream_read_errors"), Some(&json!([])));
        assert_eq!(obs.data.get("returncode"), Some(&json!(0)));
        assert_eq!(obs.data.get("timed_out"), Some(&json!(false)));
        assert!(obs.content.starts_with("OOO"));
        assert!(obs.content.ends_with("ERR-LINE\n"));
        Ok(())
    }

    #[test]
    fn bounded_command_stream_capture_preserves_timeout_metadata() -> anyhow::Result<()> {
        let workspace = TempDir::new()?;
        let sandbox = CommandSandboxConfig::path_only(workspace.path());
        let script = "import sys, time\nsys.stdout.write('before-timeout\\n')\nsys.stdout.flush()\ntime.sleep(5)\n";

        let obs = execute_bounded_command(
            &["python3", "-c", script],
            &sandbox,
            1,
            100_000,
            "CommandFailed",
            false,
            false,
        );

        assert!(!obs.ok);
        assert_eq!(obs.error.as_deref(), Some("CommandTimeout"));
        assert_eq!(obs.data.get("streaming_capture"), Some(&json!(true)));
        assert_eq!(obs.data.get("timed_out"), Some(&json!(true)));
        assert_eq!(obs.data.get("returncode"), Some(&json!(-1)));
        assert_eq!(obs.data.get("stream_read_errors"), Some(&json!([])));
        assert!(obs.content.contains("before-timeout"));
        Ok(())
    }

    #[test]
    fn bounded_command_emits_live_output_chunks_through_scoped_sink() -> anyhow::Result<()> {
        let workspace = TempDir::new()?;
        let sandbox = CommandSandboxConfig::path_only(workspace.path());
        let chunks = std::sync::Arc::new(Mutex::new(Vec::<CommandOutputChunk>::new()));
        let sink_chunks = std::sync::Arc::clone(&chunks);
        let sink: CommandOutputSink = std::sync::Arc::new(move |chunk| {
            sink_chunks.lock().expect("chunks lock").push(chunk);
        });

        let obs = with_command_output_sink(Some(sink), || {
            execute_bounded_command(
                &[
                    "python3",
                    "-c",
                    "import sys\nsys.stdout.write('live-out\\n')\nsys.stdout.flush()\nsys.stderr.write('live-err\\n')\nsys.stderr.flush()\n",
                ],
                &sandbox,
                5,
                4096,
                "CommandFailed",
                false,
                false,
            )
        });

        assert!(obs.ok, "{obs:?}");
        let chunks = chunks.lock().expect("chunks lock").clone();
        assert!(
            chunks
                .iter()
                .any(|chunk| chunk.stream == "stdout" && chunk.chunk.contains("live-out"))
        );
        assert!(
            chunks
                .iter()
                .any(|chunk| chunk.stream == "stderr" && chunk.chunk.contains("live-err"))
        );
        assert!(chunks.iter().all(|chunk| !chunk.truncated));
        Ok(())
    }

    #[test]
    fn tool_validator_preserves_shorthand_schema_compatibility() -> anyhow::Result<()> {
        let tool = test_tool(
            json!({"required": ["path"], "properties": {"path": "string", "limit": "integer"}}),
        );
        let args = tool.normalize_args(serde_json::from_value(json!({"path": 123, "limit": "5"}))?);
        tool.validate_args(&args)?;
        assert_eq!(args.get("path"), Some(&json!("123")));
        assert_eq!(args.get("limit"), Some(&json!(5)));
        Ok(())
    }

    #[test]
    fn tool_validator_rejects_unknown_properties_when_schema_is_closed() -> anyhow::Result<()> {
        let tool = test_tool(json!({
            "type": "object",
            "required": ["path"],
            "additionalProperties": false,
            "properties": {"path": {"type": "string"}}
        }));
        let args = serde_json::from_value(json!({"path": "README.md", "extra": true}))?;
        let error = tool.validate_args(&args).unwrap_err().to_string();
        assert!(
            error.contains("test_tool.extra is not an allowed argument"),
            "{error}"
        );
        Ok(())
    }

    #[test]
    fn tool_validator_checks_nested_objects_arrays_and_enums() -> anyhow::Result<()> {
        let tool = test_tool(json!({
            "type": "object",
            "required": ["request"],
            "additionalProperties": false,
            "properties": {
                "request": {
                    "type": "object",
                    "required": ["mode", "items"],
                    "additionalProperties": false,
                    "properties": {
                        "mode": {"type": "string", "enum": ["fast", "thorough"]},
                        "items": {"type": "array", "minItems": 1, "items": {"type": "object", "required": ["path"], "properties": {"path": {"type": "string"}}}}
                    }
                }
            }
        }));
        let valid = serde_json::from_value(
            json!({"request": {"mode": "fast", "items": [{"path": "src/lib.rs"}]}}),
        )?;
        tool.validate_args(&valid)?;

        let invalid_enum = serde_json::from_value(
            json!({"request": {"mode": "slow", "items": [{"path": "src/lib.rs"}]}}),
        )?;
        let error = tool.validate_args(&invalid_enum).unwrap_err().to_string();
        assert!(
            error.contains("test_tool.request.mode must be one of"),
            "{error}"
        );

        let invalid_item =
            serde_json::from_value(json!({"request": {"mode": "fast", "items": [{"path": 42}]}}))?;
        let error = tool.validate_args(&invalid_item).unwrap_err().to_string();
        assert!(
            error.contains("test_tool.request.items[0].path must be a string"),
            "{error}"
        );
        Ok(())
    }

    #[test]
    fn tool_validator_checks_string_numeric_and_array_bounds() -> anyhow::Result<()> {
        let tool = test_tool(json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "minLength": 2, "maxLength": 4},
                "count": {"type": "integer", "minimum": 1, "maximum": 3},
                "tags": {"type": "array", "minItems": 1, "maxItems": 2, "items": {"type": "string"}}
            }
        }));
        let valid = serde_json::from_value(json!({"name": "abc", "count": 2, "tags": ["x", "y"]}))?;
        tool.validate_args(&valid)?;

        let too_short = serde_json::from_value(json!({"name": "a"}))?;
        let error = tool.validate_args(&too_short).unwrap_err().to_string();
        assert!(
            error.contains("test_tool.name must be at least 2 character"),
            "{error}"
        );

        let too_large = serde_json::from_value(json!({"count": 4}))?;
        let error = tool.validate_args(&too_large).unwrap_err().to_string();
        assert!(error.contains("test_tool.count must be <= 3"), "{error}");

        let too_many = serde_json::from_value(json!({"tags": ["x", "y", "z"]}))?;
        let error = tool.validate_args(&too_many).unwrap_err().to_string();
        assert!(
            error.contains("test_tool.tags must contain at most 2 item"),
            "{error}"
        );
        Ok(())
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(previous) = &self.previous {
                    std::env::set_var(self.key, previous);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    #[test]
    fn command_execution_boundary_rejects_direct_sudo_before_spawn() {
        let parts = ["sudo", "-n", "true"];
        let rejection = reject_sudo_misuse(&parts, false).expect("sudo must be rejected");

        assert!(!rejection.ok);
        assert_eq!(rejection.error.as_deref(), Some("SudoInvocationRejected"));
        assert!(
            rejection
                .content
                .contains("Direct sudo through normal command tools")
        );
    }

    #[test]
    fn command_execution_boundary_rejects_nested_sudo_before_spawn() {
        let parts = ["bash", "-lc", "printf x | sudo -S id"];
        let rejection = reject_sudo_misuse(&parts, false).expect("nested sudo must be rejected");

        assert!(!rejection.ok);
        assert_eq!(rejection.error.as_deref(), Some("SudoInvocationRejected"));
        assert!(
            rejection
                .content
                .contains("Direct sudo through normal command tools")
        );
    }

    #[test]
    fn command_execution_boundary_rejects_privileged_tool_sudo_before_auth_check() {
        let parts = ["sudo", "id"];
        let rejection = reject_sudo_misuse(&parts, true).expect("privileged sudo must be rejected");

        assert!(!rejection.ok);
        assert_eq!(rejection.error.as_deref(), Some("SudoInvocationRejected"));
        assert!(rejection.content.contains("Do not include sudo"));
    }

    #[test]
    fn command_execution_boundary_allows_non_shell_text_arguments_mentioning_sudo() {
        let parts = ["rg", "sudo", "vegvisir/src"];
        assert!(reject_sudo_misuse(&parts, false).is_none());

        let script = format!("print('contains word {} but is not shell')", "sudo");
        let parts = ["python", "-c", script.as_str()];
        assert!(reject_sudo_misuse(&parts, false).is_none());
    }

    #[test]
    fn command_execution_boundary_allows_words_containing_sudo() {
        let parts = ["bash", "-lc", "printf pseudocode"];
        assert!(reject_sudo_misuse(&parts, false).is_none());
    }

    #[test]
    fn skiller_tools_default_to_user_global_bundle_store() -> anyhow::Result<()> {
        let workspace_one = TempDir::new()?;
        let workspace_two = TempDir::new()?;
        let data_root = TempDir::new()?;
        std::fs::write(
            workspace_one.path().join("global-help.txt"),
            "globaltool - reusable utility\n\nUsage:\n  globaltool inspect <path>\n\n$ globaltool inspect ./src\n",
        )?;
        let cms_config = VegvisirCmsConfig {
            db_path: data_root.path().join("cms-v2.sqlite3"),
            user_id: "test-user".to_string(),
            project_id: Some("test-project".to_string()),
            context_mode: cms_v2::ecm::ContextMode::Project,
            commit_writebacks: true,
        };
        let mut executor_one = ToolExecutor {
            registry: build_builtin_registry_with_cms_and_mode(
                workspace_one.path(),
                cms_config.clone(),
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

        let compile = executor_one.execute(ToolCall {
            name: "skiller_compile_cli_help".to_string(),
            args: serde_json::from_value(json!({
                "input": "global-help.txt",
                "name": "global-help"
            }))?,
        });
        assert!(compile.ok, "{}", compile.content);
        let global_bundle = data_root.path().join("skiller/bundles/global-help");
        assert!(global_bundle.join("package.yaml").exists());
        assert_eq!(
            compile.data.get("out"),
            Some(&json!(global_bundle.display().to_string()))
        );

        let mut executor_two = ToolExecutor {
            registry: build_builtin_registry_with_cms_and_mode(
                workspace_two.path(),
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
        let route = executor_two.execute(ToolCall {
            name: "skiller_route".to_string(),
            args: serde_json::from_value(json!({
                "bundle": "global-help",
                "query": "inspect path"
            }))?,
        });
        assert!(route.ok, "{}", route.content);
        assert!(route.content.contains("globaltool"), "{}", route.content);
        Ok(())
    }

    #[test]
    fn skiller_forge_handoff_uses_current_session_model_target() -> anyhow::Result<()> {
        let workspace = TempDir::new()?;
        std::fs::write(
            workspace.path().join("deployctl-help.txt"),
            "deployctl - deployment utility\n\nUsage:\n  deployctl status --json\n",
        )?;
        let cms_config = VegvisirCmsConfig {
            db_path: workspace.path().join("cms-v2.sqlite3"),
            user_id: "test-user".to_string(),
            project_id: Some("test-project".to_string()),
            context_mode: cms_v2::ecm::ContextMode::Project,
            commit_writebacks: true,
        };
        let registry = build_builtin_registry_with_cms_mode_subagent_limit_and_provider_defaults(
            workspace.path(),
            cms_config,
            false,
            3,
            SubagentProviderDefaults::default(),
            SkillerForgeModelTargetDefaults::default()
                .with_current_session("anthropic-hbse", "claude-sonnet-4.5"),
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
            name: "skiller_compile_cli_help".to_string(),
            args: serde_json::from_value(json!({
                "input": "deployctl-help.txt",
                "out": "./bundle",
                "name": "deployctl"
            }))?,
        });
        assert!(compile.ok, "{}", compile.content);
        assert_eq!(
            compile.data.get("default_forge_model_provider"),
            Some(&json!("anthropic-hbse"))
        );
        assert_eq!(
            compile.data.get("default_forge_model"),
            Some(&json!("claude-sonnet-4.5"))
        );
        assert_eq!(
            compile.data.get("forge_model_target_source"),
            Some(&json!("main-session-current"))
        );
        assert_eq!(
            compile
                .data
                .get("forge_request")
                .and_then(|request| request.get("model_provider")),
            Some(&json!("anthropic-hbse"))
        );
        assert_eq!(
            compile
                .data
                .get("forge_request")
                .and_then(|request| request.get("model")),
            Some(&json!("claude-sonnet-4.5"))
        );

        let request_obs = executor.execute(ToolCall {
            name: "skiller_forge_request".to_string(),
            args: serde_json::from_value(json!({
                "bundle": "./bundle",
                "pass": "skill_expansion"
            }))?,
        });
        assert!(request_obs.ok, "{}", request_obs.content);
        assert!(
            request_obs
                .content
                .contains("Default Forge model target: anthropic-hbse:claude-sonnet-4.5 (main-session-current)"),
            "{}",
            request_obs.content
        );
        assert_eq!(
            request_obs.data.get("model_provider"),
            Some(&json!("anthropic-hbse"))
        );
        assert_eq!(
            request_obs.data.get("model"),
            Some(&json!("claude-sonnet-4.5"))
        );
        Ok(())
    }

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
                "out": "./bundle",
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
        assert_eq!(
            compile.data.get("forge_default_objective"),
            Some(&json!(skiller_forge::skiller_default_forge_objective()))
        );
        assert!(
            compile
                .data
                .get("forge_system_prompt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("Skiller Skill Forge mode")
        );
        assert!(compile.data.get("forge_request").is_some());
        assert_eq!(
            compile
                .data
                .get("forge_request")
                .and_then(|request| request.get("default_objective")),
            Some(&json!(skiller_forge::skiller_default_forge_objective()))
        );
        assert!(
            compile
                .data
                .get("forge_request")
                .and_then(|request| request.get("system_prompt"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("Skiller Skill Forge mode")
        );
        assert!(compile.data.get("forge_response_template").is_some());
        assert!(
            compile
                .data
                .get("forge_prompt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("enhancement, expansion, cleanup, validation, and verification")
        );
        assert!(workspace.path().join("bundle/package.yaml").exists());

        let validate = executor.execute(ToolCall {
            name: "skiller_validate".to_string(),
            args: serde_json::from_value(json!({"bundle": "./bundle"}))?,
        });
        assert!(validate.ok, "{}", validate.content);

        let route = executor.execute(ToolCall {
            name: "skiller_route".to_string(),
            args: serde_json::from_value(
                json!({"bundle": "./bundle", "query": "cli workflow overview", "limit": 3}),
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
                json!({"bundle": "./bundle", "skill_id": skill_id, "mode": "extended"}),
            )?,
        });
        assert!(load.ok, "{}", load.content);
        assert!(load.content.contains("safebackup") || load.content.contains("delete"));

        Ok(())
    }

    #[test]
    fn subagent_work_budget_wraps_child_goal() {
        let budget = SubAgentWorkBudget {
            max_steps: Some(6),
            max_tool_calls: Some(9),
            max_read_bytes: Some(1234),
            max_output_bytes: Some(5678),
            allowed_tools: vec!["list_files".to_string(), "read_file".to_string()],
            notes: "avoid giant reads".to_string(),
        };

        let wrapped = apply_subagent_work_budget_to_goal("inspect renderer", &budget);

        assert!(wrapped.contains("[Vegvisir subagent work budget]"));
        assert!(wrapped.contains("max_steps: 6"));
        assert!(wrapped.contains("max_tool_calls: 9"));
        assert!(wrapped.contains("max_read_bytes_per_file: 1234"));
        assert!(wrapped.contains("allowed_tools: list_files, read_file"));
        assert!(wrapped.contains("avoid giant reads"));
        assert!(wrapped.contains("Subagent task:\ninspect renderer"));
    }

    #[test]
    fn subagent_default_work_budget_is_bounded_for_review() {
        let budget = parse_subagent_work_budget(
            None,
            Some(5),
            &SubagentSpawnDefaults::default().work_budget,
        );

        assert_eq!(budget.max_steps, Some(5));
        assert_eq!(budget.max_tool_calls, Some(8));
        assert_eq!(budget.max_read_bytes, Some(64 * 1024));
        assert_eq!(budget.max_output_bytes, Some(16 * 1024));
        assert!(budget.allowed_tools.contains(&"list_files".to_string()));
        assert!(budget.notes.contains("targeted"));
    }

    #[test]
    fn optional_subagent_cli_values_ignore_placeholders() {
        assert_eq!(optional_nonempty_string(Some(&json!(""))), None);
        assert_eq!(optional_nonempty_string(Some(&json!("default"))), None);
        assert_eq!(optional_nonempty_string(Some(&json!("none"))), None);
        assert_eq!(optional_nonempty_string(Some(&json!("null"))), None);
        assert_eq!(
            optional_nonempty_string(Some(&json!("provider-a"))),
            Some("provider-a".to_string())
        );
    }

    #[test]
    fn resolve_vegvisir_executable_honors_explicit_env() -> anyhow::Result<()> {
        let _env_lock = env_var_test_lock();
        let workspace = TempDir::new()?;
        let bin = workspace.path().join("custom-vegvisir");
        std::fs::write(&bin, "#!/bin/sh\n")?;
        let guard = EnvVarGuard::set("VEGVISIR_BIN", &bin);

        assert_eq!(resolve_vegvisir_executable(workspace.path())?, bin);
        drop(guard);
        Ok(())
    }

    #[test]
    fn resolve_vegvisir_executable_finds_workspace_release_binary() -> anyhow::Result<()> {
        let _env_lock = env_var_test_lock();
        let workspace = TempDir::new()?;
        let bin = workspace.path().join("target/release/vegvisir-rust");
        std::fs::create_dir_all(bin.parent().expect("bin parent"))?;
        std::fs::write(&bin, "#!/bin/sh\n")?;
        let _bin_guard = EnvVarGuard::remove("VEGVISIR_BIN");
        let _path_guard = EnvVarGuard::set("PATH", "");

        assert_eq!(resolve_vegvisir_executable(workspace.path())?, bin);
        Ok(())
    }

    #[test]
    fn resolve_vegvisir_executable_reports_checked_paths_when_missing() -> anyhow::Result<()> {
        let _env_lock = env_var_test_lock();
        let workspace = TempDir::new()?;
        let fake_home = workspace.path().join("home-without-vegvisir");
        std::fs::create_dir_all(&fake_home)?;
        let _bin_guard = EnvVarGuard::set("VEGVISIR_BIN", workspace.path().join("missing-bin"));
        let _path_guard = EnvVarGuard::set("PATH", "");
        let _home_guard = EnvVarGuard::set("HOME", &fake_home);

        let error = resolve_vegvisir_executable(workspace.path()).expect_err("missing binary");
        let message = error.to_string();
        assert!(message.contains("could not resolve Vegvisir executable"));
        assert!(message.contains("VEGVISIR_BIN"));
        assert!(message.contains("missing-bin"));
        assert!(message.contains("target/release/vegvisir-rust"));
        Ok(())
    }

    #[test]
    fn subagent_child_env_applies_work_budget_tool_rounds() {
        let env = subagent_child_env(&SubagentChildLaunch {
            goal: "inspect".to_string(),
            workspace: PathBuf::from("/tmp/workspace"),
            max_steps: "5".to_string(),
            provider: "demo".to_string(),
            model: "demo-local".to_string(),
            agent: None,
            bypass_sandbox: true,
            work_budget: SubAgentWorkBudget {
                max_steps: Some(5),
                max_tool_calls: Some(7),
                max_read_bytes: Some(1),
                max_output_bytes: Some(8192),
                allowed_tools: vec!["read_file".to_string(), "rg".to_string()],
                notes: String::new(),
            },
        });

        assert!(env.contains(&("VEGVISIR_SUBAGENT_RUN".to_string(), "1".to_string())));
        assert!(env.contains(&("VEGVISIR_MAX_TOOL_ROUNDS".to_string(), "7".to_string())));
        assert!(env.contains(&(
            "VEGVISIR_SUBAGENT_MAX_READ_BYTES".to_string(),
            "1".to_string()
        )));
        assert!(env.contains(&(
            "VEGVISIR_SUBAGENT_MAX_OUTPUT_BYTES".to_string(),
            "8192".to_string()
        )));
        assert!(env.contains(&(
            "VEGVISIR_SUBAGENT_ALLOWED_TOOLS".to_string(),
            "read_file,rg".to_string()
        )));
    }

    #[test]
    fn subagent_child_argv_propagates_yolo_flag_only_when_parent_bypasses_sandbox() {
        let workspace = PathBuf::from("/tmp/workspace");
        let normal = subagent_child_argv(SubagentChildLaunch {
            goal: "inspect only".to_string(),
            workspace: workspace.clone(),
            max_steps: "2".to_string(),
            provider: "provider-a".to_string(),
            model: "model-a".to_string(),
            agent: None,
            bypass_sandbox: false,
            work_budget: SubAgentWorkBudget::default(),
        });
        assert!(
            !normal
                .iter()
                .any(|arg| arg == "--dangerously-bypass-approvals-and-sandbox")
        );
        assert_eq!(normal[0], "--json");
        assert_eq!(normal[1], "--isolated-session");

        let yolo = subagent_child_argv(SubagentChildLaunch {
            goal: "inspect only".to_string(),
            workspace,
            max_steps: "2".to_string(),
            provider: "provider-a".to_string(),
            model: "model-a".to_string(),
            agent: None,
            bypass_sandbox: true,
            work_budget: SubAgentWorkBudget::default(),
        });
        assert!(
            yolo.iter()
                .any(|arg| arg == "--dangerously-bypass-approvals-and-sandbox")
        );
        assert_eq!(yolo[0], "--json");
        assert_eq!(yolo[1], "--isolated-session");
        assert_eq!(yolo[2], "--dangerously-bypass-approvals-and-sandbox");
    }

    #[test]
    fn subagent_child_argv_uses_agent_profile_without_provider_model_overrides() {
        let argv = subagent_child_argv(SubagentChildLaunch {
            goal: "inspect with registered profile".to_string(),
            workspace: PathBuf::from("/tmp/workspace"),
            max_steps: "3".to_string(),
            provider: "parent-provider".to_string(),
            model: "parent-model".to_string(),
            agent: Some("researcher".to_string()),
            bypass_sandbox: true,
            work_budget: SubAgentWorkBudget::default(),
        });

        assert!(argv.contains(&"--isolated-session".to_string()));
        assert!(
            argv.windows(2)
                .any(|pair| pair == ["--agent", "researcher"])
        );
        assert!(
            !argv
                .iter()
                .any(|arg| arg == "--provider" || arg == "--model"),
            "registered Agent-Admin profile launches must not receive separate provider/model overrides: {argv:?}"
        );
        assert!(
            !argv
                .iter()
                .any(|arg| arg == "parent-provider" || arg == "parent-model")
        );
    }

    #[test]
    fn subagent_board_tools_list_show_and_cancel_records() -> anyhow::Result<()> {
        let workspace = TempDir::new()?;
        let mut cms_config = VegvisirCmsConfig::for_workspace(workspace.path());
        cms_config.db_path = workspace.path().join(".vegvisir/cms-v2.sqlite3");
        let board_path = cms_config
            .db_path
            .parent()
            .expect("cms db parent")
            .join("subagents.json");
        std::fs::create_dir_all(board_path.parent().expect("board parent"))?;
        let now = Utc::now();
        let records = vec![SubAgentTaskRecord {
            id: "task-1".to_string(),
            name: "planner".to_string(),
            workspace: workspace.path().to_path_buf(),
            goal: "Inspect subagent visibility".to_string(),
            parent_run_id: None,
            child_run_id: None,
            artifact_dir: None,
            ownership: None,
            provider: None,
            model: None,
            file_scope: vec![workspace.path().join("vegvisir/src/subagents.rs")],
            work_budget: SubAgentWorkBudget::default(),
            status: SubAgentStatus::Running,
            created_at: now,
            started_at: Some(now),
            finished_at: None,
            checkpoint: None,
            final_answer: None,
            error: None,
            observability: SubAgentObservability::default(),
        }];
        std::fs::write(&board_path, serde_json::to_string_pretty(&records)?)?;
        let registry =
            build_builtin_registry_with_cms_and_mode(workspace.path(), cms_config, true)?;
        let mut executor = ToolExecutor {
            registry,
            guardrails: GuardrailEngine {
                policy: crate::guardrails::PermissionPolicy {
                    allow_risky_tools: true,
                    require_human_approval: false,
                    bypass_approvals_and_sandbox: true,
                    ..crate::guardrails::PermissionPolicy::default()
                },
                approvals: crate::guardrails::ApprovalLedger::default(),
            },
            runtime_policy: RuntimePolicy::default(),
            logger: EventLogger::new(None),
        };

        for name in ["subagents_list", "subagents_show", "subagents_cancel"] {
            let tool = executor.registry.get(name)?;
            assert!(!tool.risky, "{name} should be inspect/control, not risky");
        }

        let listed = executor.execute(ToolCall {
            name: "subagents_list".to_string(),
            args: serde_json::from_value(json!({"status": "running"}))?,
        });
        assert!(listed.ok, "{}", listed.content);
        assert!(listed.content.contains("task-1"));
        assert!(listed.content.contains("status=running"));
        assert_eq!(listed.data.get("total_records"), Some(&json!(1)));

        let listed_all = executor.execute(ToolCall {
            name: "subagents_list".to_string(),
            args: serde_json::from_value(json!({"status": "all"}))?,
        });
        assert!(listed_all.ok, "{}", listed_all.content);
        assert!(listed_all.content.contains("task-1"));
        assert!(listed_all.content.contains("status=running"));
        assert_eq!(listed_all.data.get("total_records"), Some(&json!(1)));

        let shown = executor.execute(ToolCall {
            name: "subagents_show".to_string(),
            args: serde_json::from_value(json!({"id_or_name": "planner"}))?,
        });
        assert!(shown.ok, "{}", shown.content);
        assert!(shown.content.contains("Inspect subagent visibility"));
        assert_eq!(
            shown.data.get("record").and_then(|record| record.get("id")),
            Some(&json!("task-1"))
        );

        let cancelled = executor.execute(ToolCall {
            name: "subagents_cancel".to_string(),
            args: serde_json::from_value(json!({"id_or_name": "task-1"}))?,
        });
        assert!(cancelled.ok, "{}", cancelled.content);
        assert!(cancelled.content.contains("Cancelled subagent task task-1"));

        let shown_after = executor.execute(ToolCall {
            name: "subagents_show".to_string(),
            args: serde_json::from_value(json!({"id_or_name": "planner"}))?,
        });
        assert!(shown_after.ok, "{}", shown_after.content);
        assert!(shown_after.content.contains("\"status\": \"cancelled\""));
        Ok(())
    }

    #[test]
    fn spawn_subagent_requires_yolo_mode_for_now() -> anyhow::Result<()> {
        let workspace = TempDir::new()?;
        let registry = build_builtin_registry_with_cms_and_mode(
            workspace.path(),
            VegvisirCmsConfig::for_workspace(workspace.path()),
            false,
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

        let observation = executor.execute(ToolCall {
            name: "spawn_subagent".to_string(),
            args: serde_json::from_value(json!({
                "goal": "inspect without editing",
                "file_scope": ["."]
            }))?,
        });

        assert!(!observation.ok);
        assert_eq!(observation.error.as_deref(), Some("SubagentRequiresYolo"));
        assert!(observation.content.contains("YOLO mode"));
        Ok(())
    }

    #[test]
    fn spawn_subagent_uses_configurable_active_limit() -> anyhow::Result<()> {
        let workspace = TempDir::new()?;
        let mut cms_config = VegvisirCmsConfig::for_workspace(workspace.path());
        cms_config.db_path = workspace.path().join(".vegvisir/cms-v2.sqlite3");
        let board_path = cms_config
            .db_path
            .parent()
            .expect("cms db parent")
            .join("subagents.json");
        std::fs::create_dir_all(board_path.parent().expect("board parent"))?;
        let now = Utc::now();
        let records = vec![SubAgentTaskRecord {
            id: "active-1".to_string(),
            name: "active".to_string(),
            workspace: workspace.path().to_path_buf(),
            goal: "existing".to_string(),
            parent_run_id: None,
            child_run_id: None,
            artifact_dir: None,
            ownership: None,
            provider: None,
            model: None,
            file_scope: vec![workspace.path().join("other")],
            work_budget: SubAgentWorkBudget::default(),
            status: SubAgentStatus::Running,
            created_at: now,
            started_at: Some(now),
            finished_at: None,
            checkpoint: None,
            final_answer: None,
            error: None,
            observability: SubAgentObservability::default(),
        }];
        std::fs::write(&board_path, serde_json::to_string_pretty(&records)?)?;
        let registry = build_builtin_registry_with_cms_mode_and_subagent_limit(
            workspace.path(),
            cms_config,
            true,
            1,
        )?;
        let mut executor = ToolExecutor {
            registry,
            guardrails: GuardrailEngine {
                policy: crate::guardrails::PermissionPolicy {
                    allow_risky_tools: true,
                    require_human_approval: false,
                    bypass_approvals_and_sandbox: true,
                    ..crate::guardrails::PermissionPolicy::default()
                },
                approvals: crate::guardrails::ApprovalLedger::default(),
            },
            runtime_policy: RuntimePolicy::default(),
            logger: EventLogger::new(None),
        };

        let observation = executor.execute(ToolCall {
            name: "spawn_subagent".to_string(),
            args: serde_json::from_value(json!({
                "goal": "inspect without editing",
                "file_scope": ["fresh"]
            }))?,
        });

        assert!(!observation.ok);
        assert_eq!(observation.error.as_deref(), Some("SubagentLimit"));
        assert!(
            observation
                .content
                .contains("Maximum active subagents reached (1)")
        );
        Ok(())
    }

    #[test]
    fn spawn_subagent_uses_configurable_spawn_defaults() -> anyhow::Result<()> {
        let _env_lock = env_var_test_lock();
        let workspace = TempDir::new()?;
        let fake_bin = workspace.path().join("fake-vegvisir");
        std::fs::write(
            &fake_bin,
            r#"#!/bin/sh
echo '{"events":[]}'; exit 0
"#,
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&fake_bin)?.permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&fake_bin, permissions)?;
        }
        let _bin_guard = EnvVarGuard::set("VEGVISIR_BIN", &fake_bin);
        let mut cms_config = VegvisirCmsConfig::for_workspace(workspace.path());
        cms_config.db_path = workspace.path().join(".vegvisir/cms-v2.sqlite3");
        let board_path = cms_config
            .db_path
            .parent()
            .expect("cms db parent")
            .join("subagents.json");
        let registry = build_builtin_registry_with_cms_mode_subagent_config(
            workspace.path(),
            cms_config,
            true,
            3,
            SubagentProviderDefaults::new("provider-x", "model-x"),
            SubagentSpawnDefaults {
                default_max_steps: 11,
                min_max_steps: 2,
                max_max_steps: 40,
                work_budget: SubAgentWorkBudget {
                    max_steps: None,
                    max_tool_calls: Some(17),
                    max_read_bytes: Some(222_222),
                    max_output_bytes: Some(33_333),
                    allowed_tools: vec!["read_file".to_string(), "rg".to_string()],
                    notes: "custom defaults for deep review".to_string(),
                },
            },
            SkillerForgeModelTargetDefaults::default(),
        )?;
        let mut executor = ToolExecutor {
            registry,
            guardrails: GuardrailEngine {
                policy: crate::guardrails::PermissionPolicy {
                    allow_risky_tools: true,
                    require_human_approval: false,
                    bypass_approvals_and_sandbox: true,
                    ..crate::guardrails::PermissionPolicy::default()
                },
                approvals: crate::guardrails::ApprovalLedger::default(),
            },
            runtime_policy: RuntimePolicy::default(),
            logger: EventLogger::new(None),
        };

        let observation = executor.execute(ToolCall {
            name: "spawn_subagent".to_string(),
            args: serde_json::from_value(json!({
                "goal": "inspect configurable defaults",
                "name": "configurable-defaults-check",
                "file_scope": ["."]
            }))?,
        });

        assert!(observation.ok, "{}", observation.content);
        assert_eq!(observation.data.get("max_steps"), Some(&json!(11)));
        assert_eq!(
            observation
                .data
                .get("work_budget")
                .and_then(|budget| budget.get("max_tool_calls")),
            Some(&json!(17))
        );
        assert_eq!(
            observation
                .data
                .get("work_budget")
                .and_then(|budget| budget.get("max_read_bytes")),
            Some(&json!(222_222))
        );
        assert_eq!(
            observation
                .data
                .get("work_budget")
                .and_then(|budget| budget.get("allowed_tools")),
            Some(&json!(["read_file", "rg"]))
        );
        let records = load_subagent_board_records(&board_path)?;
        let record = records
            .iter()
            .find(|record| record.name == "configurable-defaults-check")
            .expect("spawned configurable-defaults-check record");
        assert_eq!(record.work_budget.max_steps, Some(11));
        assert_eq!(record.work_budget.max_tool_calls, Some(17));
        assert_eq!(record.work_budget.max_read_bytes, Some(222_222));
        assert_eq!(record.work_budget.max_output_bytes, Some(33_333));
        assert!(record.work_budget.allowed_tools.contains(&"rg".to_string()));
        assert_eq!(record.work_budget.notes, "custom defaults for deep review");
        Ok(())
    }

    #[test]
    fn spawn_subagent_repairs_retired_openai_sso_codex_mini_model() -> anyhow::Result<()> {
        let _env_lock = env_var_test_lock();
        let workspace = TempDir::new()?;
        let fake_bin = workspace.path().join("fake-vegvisir");
        std::fs::write(
            &fake_bin,
            r#"#!/bin/sh
echo '{"events":[]}'; exit 0
"#,
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&fake_bin)?.permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&fake_bin, permissions)?;
        }
        let _bin_guard = EnvVarGuard::set("VEGVISIR_BIN", &fake_bin);
        let mut cms_config = VegvisirCmsConfig::for_workspace(workspace.path());
        cms_config.db_path = workspace.path().join(".vegvisir/cms-v2.sqlite3");
        let board_path = cms_config
            .db_path
            .parent()
            .expect("cms db parent")
            .join("subagents.json");
        let registry = build_builtin_registry_with_cms_mode_subagent_limit_and_provider_defaults(
            workspace.path(),
            cms_config,
            true,
            3,
            SubagentProviderDefaults::new("openai-sso", "gpt-5.1-codex-mini"),
            SkillerForgeModelTargetDefaults::default(),
        )?;
        let mut executor = ToolExecutor {
            registry,
            guardrails: GuardrailEngine {
                policy: crate::guardrails::PermissionPolicy {
                    allow_risky_tools: true,
                    require_human_approval: false,
                    bypass_approvals_and_sandbox: true,
                    ..crate::guardrails::PermissionPolicy::default()
                },
                approvals: crate::guardrails::ApprovalLedger::default(),
            },
            runtime_policy: RuntimePolicy::default(),
            logger: EventLogger::new(None),
        };

        let observation = executor.execute(ToolCall {
            name: "spawn_subagent".to_string(),
            args: serde_json::from_value(json!({
                "goal": "inspect repaired model",
                "name": "repaired-model-check",
                "provider": "openai-sso",
                "model": "gpt-5.1-codex-mini",
                "file_scope": ["."]
            }))?,
        });

        assert!(observation.ok, "{}", observation.content);
        assert_eq!(observation.data.get("provider"), Some(&json!("openai-sso")));
        assert_eq!(observation.data.get("model"), Some(&json!("gpt-5.4-mini")));
        let mut records = load_subagent_board_records(&board_path)?;
        for _ in 0..20 {
            let finished = records.iter().any(|record| {
                record.name == "repaired-model-check"
                    && !matches!(
                        record.status,
                        SubAgentStatus::Queued | SubAgentStatus::Running
                    )
            });
            if finished {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
            records = load_subagent_board_records(&board_path)?;
        }
        let record = records
            .iter()
            .find(|record| record.name == "repaired-model-check")
            .expect("spawned repaired-model-check record");
        assert_eq!(record.provider.as_deref(), Some("openai-sso"));
        assert_eq!(record.model.as_deref(), Some("gpt-5.4-mini"));
        assert!(
            record
                .observability
                .launch_argv
                .windows(2)
                .any(|pair| pair == ["--model", "gpt-5.4-mini"]),
            "expected repaired launch argv, got {:?}",
            record.observability.launch_argv
        );
        assert!(
            !record
                .observability
                .launch_argv
                .iter()
                .any(|arg| arg == "gpt-5.1-codex-mini"),
            "retired model should not be passed to child argv"
        );
        Ok(())
    }

    #[test]
    fn spawn_subagent_materializes_current_provider_model_sentinel() -> anyhow::Result<()> {
        let _env_lock = env_var_test_lock();
        let workspace = TempDir::new()?;
        let fake_bin = workspace.path().join("fake-vegvisir");
        std::fs::write(
            &fake_bin,
            r#"#!/bin/sh
echo '{"events":[]}'; exit 0
"#,
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&fake_bin)?.permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&fake_bin, permissions)?;
        }
        let _bin_guard = EnvVarGuard::set("VEGVISIR_BIN", &fake_bin);
        let mut cms_config = VegvisirCmsConfig::for_workspace(workspace.path());
        cms_config.db_path = workspace.path().join(".vegvisir/cms-v2.sqlite3");
        let board_path = cms_config
            .db_path
            .parent()
            .expect("cms db parent")
            .join("subagents.json");
        let registry = build_builtin_registry_with_cms_mode_subagent_limit_and_provider_defaults(
            workspace.path(),
            cms_config,
            true,
            3,
            SubagentProviderDefaults::new("openai-sso", "gpt-5.4-mini")
                .with_current_session("anthropic-hbse", "claude-sonnet-4.5"),
            SkillerForgeModelTargetDefaults::default(),
        )?;
        let mut executor = ToolExecutor {
            registry,
            guardrails: GuardrailEngine {
                policy: crate::guardrails::PermissionPolicy {
                    allow_risky_tools: true,
                    require_human_approval: false,
                    bypass_approvals_and_sandbox: true,
                    ..crate::guardrails::PermissionPolicy::default()
                },
                approvals: crate::guardrails::ApprovalLedger::default(),
            },
            runtime_policy: RuntimePolicy::default(),
            logger: EventLogger::new(None),
        };

        let observation = executor.execute(ToolCall {
            name: "spawn_subagent".to_string(),
            args: serde_json::from_value(json!({
                "goal": "inspect current sentinel",
                "name": "current-sentinel-check",
                "provider": "current",
                "model": "current",
                "file_scope": ["."]
            }))?,
        });

        assert!(observation.ok, "{}", observation.content);
        assert_eq!(
            observation.data.get("provider"),
            Some(&json!("anthropic-hbse"))
        );
        assert_eq!(
            observation.data.get("model"),
            Some(&json!("claude-sonnet-4.5"))
        );
        let mut records = load_subagent_board_records(&board_path)?;
        for _ in 0..20 {
            let finished = records.iter().any(|record| {
                record.name == "current-sentinel-check"
                    && !matches!(
                        record.status,
                        SubAgentStatus::Queued | SubAgentStatus::Running
                    )
            });
            if finished {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
            records = load_subagent_board_records(&board_path)?;
        }
        let record = records
            .iter()
            .find(|record| record.name == "current-sentinel-check")
            .expect("spawned current-sentinel-check record");
        assert_eq!(record.provider.as_deref(), Some("anthropic-hbse"));
        assert_eq!(record.model.as_deref(), Some("claude-sonnet-4.5"));
        assert!(
            !record
                .observability
                .launch_argv
                .iter()
                .any(|arg| arg == "current"),
            "current sentinel must not leak into child argv: {:?}",
            record.observability.launch_argv
        );
        Ok(())
    }

    #[test]
    fn spawn_subagent_rejects_current_sentinel_without_parent_context() -> anyhow::Result<()> {
        let workspace = TempDir::new()?;
        let registry = build_builtin_registry_with_cms_mode_subagent_limit_and_provider_defaults(
            workspace.path(),
            VegvisirCmsConfig::for_workspace(workspace.path()),
            true,
            3,
            SubagentProviderDefaults::new("openai-sso", "gpt-5.4-mini"),
            SkillerForgeModelTargetDefaults::default(),
        )?;
        let mut executor = ToolExecutor {
            registry,
            guardrails: GuardrailEngine {
                policy: crate::guardrails::PermissionPolicy {
                    allow_risky_tools: true,
                    require_human_approval: false,
                    bypass_approvals_and_sandbox: true,
                    ..crate::guardrails::PermissionPolicy::default()
                },
                approvals: crate::guardrails::ApprovalLedger::default(),
            },
            runtime_policy: RuntimePolicy::default(),
            logger: EventLogger::new(None),
        };

        let observation = executor.execute(ToolCall {
            name: "spawn_subagent".to_string(),
            args: serde_json::from_value(json!({
                "goal": "inspect current sentinel without context",
                "name": "current-sentinel-missing-context",
                "provider": "current",
                "model": "current",
                "file_scope": ["."]
            }))?,
        });

        assert!(!observation.ok);
        assert_eq!(
            observation.error.as_deref(),
            Some("SubagentProviderModelError")
        );
        assert!(
            observation
                .content
                .contains("requires the parent session provider/model context"),
            "{}",
            observation.content
        );
        Ok(())
    }

    #[test]
    fn spawn_subagent_materializes_default_provider_and_model() -> anyhow::Result<()> {
        let _env_lock = env_var_test_lock();
        let workspace = TempDir::new()?;
        let fake_bin = workspace.path().join("fake-vegvisir");
        std::fs::write(
            &fake_bin,
            r#"#!/bin/sh
echo '{"events":[]}'; exit 0
"#,
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&fake_bin)?.permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&fake_bin, permissions)?;
        }
        let _bin_guard = EnvVarGuard::set("VEGVISIR_BIN", &fake_bin);
        let mut cms_config = VegvisirCmsConfig::for_workspace(workspace.path());
        cms_config.db_path = workspace.path().join(".vegvisir/cms-v2.sqlite3");
        let board_path = cms_config
            .db_path
            .parent()
            .expect("cms db parent")
            .join("subagents.json");
        let defaults = serde_json::from_str::<Value>(include_str!("defaults/subagents.json"))?;
        let provider = defaults
            .get("default_provider")
            .and_then(Value::as_str)
            .expect("default subagent provider configured");
        let model = defaults
            .get("default_model")
            .and_then(Value::as_str)
            .expect("default subagent model configured");
        let registry = build_builtin_registry_with_cms_mode_subagent_limit_and_provider_defaults(
            workspace.path(),
            cms_config,
            true,
            3,
            SubagentProviderDefaults::new(provider, model),
            SkillerForgeModelTargetDefaults::default(),
        )?;
        let mut executor = ToolExecutor {
            registry,
            guardrails: GuardrailEngine {
                policy: crate::guardrails::PermissionPolicy {
                    allow_risky_tools: true,
                    require_human_approval: false,
                    bypass_approvals_and_sandbox: true,
                    ..crate::guardrails::PermissionPolicy::default()
                },
                approvals: crate::guardrails::ApprovalLedger::default(),
            },
            runtime_policy: RuntimePolicy::default(),
            logger: EventLogger::new(None),
        };

        let observation = executor.execute(ToolCall {
            name: "spawn_subagent".to_string(),
            args: serde_json::from_value(json!({
                "goal": "inspect defaults",
                "name": "defaults-check",
                "file_scope": ["."]
            }))?,
        });

        assert!(observation.ok, "{}", observation.content);
        assert_eq!(observation.data.get("provider"), Some(&json!(provider)));
        assert_eq!(observation.data.get("model"), Some(&json!(model)));
        let records = load_subagent_board_records(&board_path)?;
        let record = records
            .iter()
            .find(|record| record.name == "defaults-check")
            .expect("spawned defaults-check record");
        assert_eq!(record.provider.as_deref(), Some(provider));
        assert_eq!(record.model.as_deref(), Some(model));
        assert!(
            record
                .observability
                .launch_env_keys
                .iter()
                .any(|key| key == "VEGVISIR_SUBAGENT_RUN"),
            "subagent child launches must be marked so they cannot write provider/model back to the main session config: {:?}",
            record.observability.launch_env_keys
        );
        Ok(())
    }

    #[test]
    fn load_subagent_board_records_recovers_from_prefixed_garbage() -> anyhow::Result<()> {
        let workspace = TempDir::new()?;
        let board_path = workspace.path().join("subagents.json");
        let records = vec![SubAgentTaskRecord {
            id: "task-1".to_string(),
            name: "planner".to_string(),
            workspace: workspace.path().to_path_buf(),
            goal: "Inspect subagent visibility".to_string(),
            parent_run_id: None,
            child_run_id: None,
            artifact_dir: None,
            ownership: None,
            provider: None,
            model: None,
            file_scope: vec![workspace.path().join("vegvisir/src/subagents.rs")],
            work_budget: SubAgentWorkBudget::default(),
            status: SubAgentStatus::Running,
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
            finished_at: None,
            checkpoint: None,
            final_answer: None,
            error: None,
            observability: SubAgentObservability::default(),
        }];
        let payload = format!("\naa{}", serde_json::to_string_pretty(&records)?);
        std::fs::write(&board_path, payload)?;

        let loaded = load_subagent_board_records(&board_path)?;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "task-1");

        let rewritten = std::fs::read_to_string(&board_path)?;
        assert!(rewritten.trim_start().starts_with('['));
        Ok(())
    }

    #[test]
    fn load_subagent_board_records_recovers_from_truncated_json() -> anyhow::Result<()> {
        let workspace = TempDir::new()?;
        let board_path = workspace.path().join("subagents.json");
        let records = vec![SubAgentTaskRecord {
            id: "task-2".to_string(),
            name: "truncated".to_string(),
            workspace: workspace.path().to_path_buf(),
            goal: "Inspect truncated board recovery".to_string(),
            parent_run_id: None,
            child_run_id: None,
            artifact_dir: None,
            ownership: None,
            provider: None,
            model: None,
            file_scope: Vec::new(),
            work_budget: SubAgentWorkBudget::default(),
            status: SubAgentStatus::Queued,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            checkpoint: None,
            final_answer: None,
            error: None,
            observability: SubAgentObservability::default(),
        }];
        let payload = format!(
            "{}
TRAILING_GARBAGE",
            serde_json::to_string_pretty(&records)?
        );
        std::fs::write(&board_path, payload)?;

        let loaded = load_subagent_board_records(&board_path)?;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "task-2");

        let contents = std::fs::read_to_string(&board_path)?;
        assert!(contents.trim_start().starts_with('['));
        Ok(())
    }

    #[test]
    fn skiller_import_skill_tool_uses_script_generation_handoff() -> anyhow::Result<()> {
        let workspace = TempDir::new()?;
        std::fs::write(
            workspace.path().join("skill.yaml"),
            r#"skill:
  id: imported-maintenance-check
  title: Imported Maintenance Check
  summary: Inspect service maintenance readiness.
  procedure:
    - Review the maintenance window.
    - Gather read-only health evidence.
  guardrails:
    - Do not mutate production systems.
  runtime_policy:
    conceptual_answer: true
    recommend_commands: true
    run_read_only_commands: true
    modify_files: false
    modify_external_systems: false
    requires_user_approval: true
    requires_backup_or_rollback: false
    handles_secrets: false
    handles_licensed_source: false
"#,
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

        let import = executor.execute(ToolCall {
            name: "skiller_import_skill".to_string(),
            args: serde_json::from_value(json!({
                "input": "skill.yaml",
                "out": "./imported-bundle",
                "name": "maintenance",
                "domain": "operations"
            }))?,
        });
        assert!(import.ok, "{}", import.content);
        assert!(
            import
                .content
                .contains("Deterministic raw-source generation was skipped")
        );
        assert_eq!(
            import.data.get("import_mode"),
            Some(&json!("pre_existing_skill"))
        );
        assert_eq!(import.data.get("deterministic_stage"), Some(&json!(false)));
        assert_eq!(
            import.data.get("deterministic_generation"),
            Some(&json!("skipped"))
        );
        assert_eq!(
            import.data.get("forge_required_by_default"),
            Some(&json!(true))
        );
        assert_eq!(
            import.data.get("default_forge_pass"),
            Some(&json!("ScriptGeneration"))
        );
        assert_eq!(
            import.data.get("recommended_apply_tool"),
            Some(&json!("skiller_forge_apply"))
        );
        assert!(
            import
                .data
                .get("forge_prompt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("ScriptGeneration")
        );
        assert!(
            workspace
                .path()
                .join("imported-bundle/package.yaml")
                .exists()
        );

        let validate = executor.execute(ToolCall {
            name: "skiller_validate".to_string(),
            args: serde_json::from_value(json!({"bundle": "./imported-bundle"}))?,
        });
        assert!(validate.ok, "{}", validate.content);
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
                "out": "./bundle",
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
        assert_eq!(
            compile.data.get("forge_default_objective"),
            Some(&json!(skiller_forge::skiller_default_forge_objective()))
        );
        assert!(
            compile
                .data
                .get("forge_system_prompt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("Skiller Skill Forge mode")
        );
        assert!(compile.data.get("forge_request").is_some());
        assert_eq!(
            compile
                .data
                .get("forge_request")
                .and_then(|request| request.get("default_objective")),
            Some(&json!(skiller_forge::skiller_default_forge_objective()))
        );
        assert!(
            compile
                .data
                .get("forge_request")
                .and_then(|request| request.get("system_prompt"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("Skiller Skill Forge mode")
        );
        assert!(compile.data.get("forge_response_template").is_some());

        let request_obs = executor.execute(ToolCall {
            name: "skiller_forge_request".to_string(),
            args: serde_json::from_value(json!({
                "bundle": "./bundle",
                "pass": "skill_expansion",
                "max_skills": 2
            }))?,
        });
        assert!(request_obs.ok, "{}", request_obs.content);
        assert!(request_obs.content.contains("ForgeResponseEnvelope"));
        assert!(
            request_obs
                .content
                .contains("Skiller-specialized Vegvisir system prompt")
        );
        assert_eq!(request_obs.data.get("provider"), Some(&json!("vegvisir")));
        assert_eq!(
            request_obs.data.get("default_objective"),
            Some(&json!(skiller_forge::skiller_default_forge_objective()))
        );
        assert!(
            request_obs
                .data
                .get("system_prompt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("Skiller Skill Forge mode")
        );
        assert_eq!(
            request_obs
                .data
                .get("request")
                .and_then(|request| request.get("default_objective")),
            Some(&json!(skiller_forge::skiller_default_forge_objective()))
        );
        assert!(
            request_obs
                .data
                .get("request")
                .and_then(|request| request.get("system_prompt"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("Skiller Skill Forge mode")
        );
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
                "bundle": "./bundle",
                "out": "./forged-bundle",
                "request": request,
                "response_envelope": response_template
            }))?,
        });
        assert!(apply.ok, "{}", apply.content);
        assert!(workspace.path().join("forged-bundle/package.yaml").exists());
        assert!(apply.data.get("apply_report").is_some());
        Ok(())
    }

    #[test]
    fn skiller_suspicious_commands_tool_reports_compact_diagnostics() -> anyhow::Result<()> {
        let workspace = TempDir::new()?;
        std::fs::write(
            workspace.path().join("tool-help.txt"),
            "Usage: tool <command>\n\ntool status --json\n",
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
            name: "skiller_compile_cli_help".to_string(),
            args: serde_json::from_value(json!({
                "input": "tool-help.txt",
                "out": "./bundle",
                "name": "tool-cli"
            }))?,
        });
        assert!(compile.ok, "{}", compile.content);

        let skill_path = std::fs::read_dir(workspace.path().join("bundle/skills"))?
            .next()
            .expect("skill file")?
            .path();
        let skill_yaml = std::fs::read_to_string(&skill_path)?
            .replace("Run `tool status --json`", "Run `try {`")
            .replace(
                "target_command: tool status --json",
                "target_command: try {",
            )
            .replace("tool_name: tool", "tool_name: try");
        std::fs::write(&skill_path, skill_yaml)?;

        let report = executor.execute(ToolCall {
            name: "skiller_suspicious_commands".to_string(),
            args: serde_json::from_value(json!({"bundle": "./bundle"}))?,
        });
        assert!(report.ok, "{}", report.content);
        assert_eq!(
            report.data.get("suspicious_cli_operation_count"),
            Some(&json!(1))
        );
        assert!(
            report.content.contains("programming_syntax"),
            "{}",
            report.content
        );
        Ok(())
    }
}
