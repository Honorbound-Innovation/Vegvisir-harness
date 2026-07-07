use anyhow::Context;
use clap::Parser;
use msp_core::{
    ExecutionReport, MspInfo, PackSearchQuery, RuntimeCompatibilityQuery, SkillSearchQuery,
    SkillTrustPolicy,
};
use msp_registry::LocalRegistry;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "msp-server")]
#[command(about = "MSP v0.1 JSON-RPC stdio server")]
struct Args {
    /// Registry root containing skill.manifest.json and pack.manifest.json files.
    #[arg(long, default_value = "examples/registry")]
    registry: PathBuf,
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    id: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let registry = LocalRegistry::open(&args.registry)
        .with_context(|| format!("failed to open registry {}", args.registry.display()))?;
    serve_stdio(&registry)
}

fn serve_stdio(registry: &LocalRegistry) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(request) => handle_request(registry, request),
            Err(error) => JsonRpcResponse {
                jsonrpc: "2.0",
                result: None,
                error: Some(JsonRpcError {
                    code: -32700,
                    message: format!("parse error: {error}"),
                }),
                id: None,
            },
        };
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }
    Ok(())
}

fn handle_request(registry: &LocalRegistry, request: JsonRpcRequest) -> JsonRpcResponse {
    if request.jsonrpc != "2.0" {
        return error_response(request.id, -32600, "jsonrpc must be 2.0".to_string());
    }

    if !msp_core::core_methods()
        .iter()
        .any(|method| method == &request.method)
    {
        return error_response(
            request.id,
            -32601,
            format!("method not found: {}", request.method),
        );
    }

    let result = dispatch(registry, &request.method, request.params);
    match result {
        Ok(value) => JsonRpcResponse {
            jsonrpc: "2.0",
            result: Some(value),
            error: None,
            id: request.id,
        },
        Err(error) => error_response(request.id, -32000, error.to_string()),
    }
}

fn dispatch(registry: &LocalRegistry, method: &str, params: Value) -> anyhow::Result<Value> {
    match method {
        "msp.info" => Ok(serde_json::to_value(MspInfo::default())?),
        "registry.search" | "skills.discover" => {
            let query: SkillSearchQuery = if params.is_null() {
                SkillSearchQuery::default()
            } else {
                serde_json::from_value(params)?
            };
            Ok(serde_json::to_value(registry.search(&query))?)
        }
        "skills.get_manifest" => {
            let id = required_id(&params)?;
            Ok(serde_json::to_value(registry.get_manifest(&id)?)?)
        }
        "skills.load" => {
            let id = required_id(&params)?;
            Ok(serde_json::to_value(registry.load_skill(&id)?)?)
        }
        "skills.resolve_dependencies" => {
            let id = required_id(&params)?;
            Ok(serde_json::to_value(registry.resolve_dependencies(&id)?)?)
        }
        "skills.verify_result" => {
            let report = ExecutionReport::from_json_value(
                params
                    .get("execution_report")
                    .cloned()
                    .unwrap_or_else(|| params.clone()),
            )?;
            Ok(serde_json::to_value(
                registry.verify_execution_report(&report)?,
            )?)
        }
        "skills.check_compatibility" => {
            let id = required_id(&params)?;
            let query_value = params
                .get("runtime")
                .or_else(|| params.get("query"))
                .cloned()
                .unwrap_or_else(|| params.clone());
            let query: RuntimeCompatibilityQuery = serde_json::from_value(query_value)?;
            Ok(serde_json::to_value(
                registry.check_skill_compatibility(&id, &query)?,
            )?)
        }
        "packs.discover" => {
            let query: PackSearchQuery = if params.is_null() {
                PackSearchQuery::default()
            } else {
                serde_json::from_value(params)?
            };
            Ok(serde_json::to_value(registry.discover_packs(&query))?)
        }
        "packs.get_manifest" | "packs.load" => {
            let id = required_id(&params)?;
            Ok(serde_json::to_value(registry.load_pack(&id)?)?)
        }
        "packs.verify_trust" => {
            let id = required_id(&params)?;
            Ok(serde_json::to_value(registry.verify_pack_trust(&id)?)?)
        }
        "packs.evaluate_trust" => {
            let id = required_id(&params)?;
            let policy = SkillTrustPolicy::from_json_value(
                params
                    .get("policy")
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("missing policy"))?,
            )?;
            Ok(serde_json::to_value(
                registry.evaluate_pack_trust_policy(&policy, &id)?,
            )?)
        }
        "packs.validate_members" => {
            let id = required_id(&params)?;
            let policy = params
                .get("policy")
                .cloned()
                .map(SkillTrustPolicy::from_json_value)
                .transpose()?;
            Ok(serde_json::to_value(
                registry.validate_pack_members_with_policy(&id, policy.as_ref())?,
            )?)
        }
        "packs.evaluate_dependencies" => {
            let id = required_id(&params)?;
            let policy = SkillTrustPolicy::from_json_value(
                params
                    .get("policy")
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("missing policy"))?,
            )?;
            Ok(serde_json::to_value(
                registry.evaluate_pack_dependency_trust(&policy, &id)?,
            )?)
        }
        "trust.verify" => {
            let id = required_id(&params)?;
            Ok(serde_json::to_value(registry.verify_trust(&id)?)?)
        }
        "trust.evaluate" => {
            let id = required_id(&params)?;
            let policy = SkillTrustPolicy::from_json_value(
                params
                    .get("policy")
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("missing policy"))?,
            )?;
            Ok(serde_json::to_value(
                registry.evaluate_trust_policy(&policy, &id)?,
            )?)
        }
        "trust.evaluate_dependencies" => {
            let id = required_id(&params)?;
            let policy = SkillTrustPolicy::from_json_value(
                params
                    .get("policy")
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("missing policy"))?,
            )?;
            Ok(serde_json::to_value(
                registry.evaluate_dependency_trust(&policy, &id)?,
            )?)
        }
        _ => anyhow::bail!("method not found: {method}"),
    }
}

fn required_id(params: &Value) -> anyhow::Result<String> {
    if let Some(id) = params.get("id").and_then(Value::as_str) {
        return Ok(id.to_string());
    }
    if let Some(id) = params.get("skill_id").and_then(Value::as_str) {
        return Ok(id.to_string());
    }
    if let Some(id) = params.get("pack_id").and_then(Value::as_str) {
        return Ok(id.to_string());
    }
    anyhow::bail!("missing id")
}

fn error_response(id: Option<Value>, code: i64, message: String) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        result: None,
        error: Some(JsonRpcError { code, message }),
        id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use msp_core::{MspSchemaKind, core_methods, validate_json_schema};
    use serde_json::json;
    use std::path::Path;

    const SKILL_ID: &str = "skill.rust.refactor.module.v1";
    const PACK_ID: &str = "pack.rust.engineering.v1";

    #[test]
    fn rejects_non_jsonrpc_2() {
        let registry = LocalRegistry::empty(".");
        let response = handle_request(
            &registry,
            JsonRpcRequest {
                jsonrpc: "1.0".to_string(),
                method: "msp.info".to_string(),
                params: Value::Null,
                id: Some(json!(1)),
            },
        );
        let error = response.error.expect("error response");
        assert_eq!(error.code, -32600);
    }

    #[test]
    fn unknown_method_uses_standard_jsonrpc_method_not_found_code() {
        let registry = LocalRegistry::empty(".");
        let response = handle_request(
            &registry,
            JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                method: "unknown.method".to_string(),
                params: Value::Null,
                id: Some(json!(1)),
            },
        );
        let error = response.error.expect("error response");
        assert_eq!(error.code, -32601);
        assert!(error.message.contains("method not found"));
    }

    #[test]
    fn all_advertised_core_methods_dispatch_successfully() {
        let registry = reference_registry();
        let policy = reference_policy_json();
        let report = reference_report_json();
        let runtime = compatibility_query_json();

        let cases = [
            ("msp.info", Value::Null, None),
            (
                "registry.search",
                json!({ "task": "refactor rust module" }),
                Some(MspSchemaKind::RegistrySearchResultArray),
            ),
            (
                "skills.discover",
                json!({ "task": "refactor rust module" }),
                Some(MspSchemaKind::RegistrySearchResultArray),
            ),
            (
                "skills.get_manifest",
                json!({ "id": SKILL_ID }),
                Some(MspSchemaKind::Manifest),
            ),
            (
                "skills.load",
                json!({ "id": SKILL_ID }),
                Some(MspSchemaKind::SkillLoadResult),
            ),
            (
                "skills.resolve_dependencies",
                json!({ "id": SKILL_ID }),
                Some(MspSchemaKind::DependencyResolutionResult),
            ),
            (
                "skills.verify_result",
                json!({ "execution_report": report }),
                Some(MspSchemaKind::SkillVerificationResult),
            ),
            (
                "skills.check_compatibility",
                json!({ "id": SKILL_ID, "runtime": runtime }),
                Some(MspSchemaKind::SkillCompatibilityResult),
            ),
            (
                "packs.discover",
                json!({ "task": "rust engineering" }),
                Some(MspSchemaKind::PackSearchResultArray),
            ),
            (
                "packs.get_manifest",
                json!({ "id": PACK_ID }),
                Some(MspSchemaKind::SkillPack),
            ),
            (
                "packs.load",
                json!({ "id": PACK_ID }),
                Some(MspSchemaKind::SkillPack),
            ),
            (
                "packs.verify_trust",
                json!({ "id": PACK_ID }),
                Some(MspSchemaKind::TrustVerifyResult),
            ),
            (
                "packs.evaluate_trust",
                json!({ "id": PACK_ID, "policy": policy.clone() }),
                Some(MspSchemaKind::TrustPolicyEvaluation),
            ),
            (
                "packs.validate_members",
                json!({ "id": PACK_ID }),
                Some(MspSchemaKind::PackMemberValidationResult),
            ),
            (
                "packs.evaluate_dependencies",
                json!({ "id": PACK_ID, "policy": policy.clone() }),
                Some(MspSchemaKind::DependencyTrustEvaluationResult),
            ),
            (
                "trust.verify",
                json!({ "id": SKILL_ID }),
                Some(MspSchemaKind::TrustVerifyResult),
            ),
            (
                "trust.evaluate",
                json!({ "id": SKILL_ID, "policy": policy.clone() }),
                Some(MspSchemaKind::TrustPolicyEvaluation),
            ),
            (
                "trust.evaluate_dependencies",
                json!({ "id": SKILL_ID, "policy": policy }),
                Some(MspSchemaKind::DependencyTrustEvaluationResult),
            ),
        ];

        let covered: Vec<_> = cases
            .iter()
            .map(|(method, _, _)| (*method).to_string())
            .collect();
        assert_eq!(
            covered,
            core_methods(),
            "test cases must track core_methods()"
        );

        for (method, params, schema) in cases {
            let value = dispatch(&registry, method, params)
                .unwrap_or_else(|error| panic!("{method} should dispatch successfully: {error}"));
            if method == "msp.info" {
                assert_eq!(value["methods"], json!(core_methods()));
            }
            if let Some(schema) = schema {
                validate_json_schema(schema, &value).unwrap_or_else(|error| {
                    panic!("{method} returned invalid {schema:?}: {error}\n{value:#}")
                });
            }
        }
    }

    #[test]
    fn aliases_return_equivalent_results() {
        let registry = reference_registry();
        assert_eq!(
            dispatch(
                &registry,
                "registry.search",
                json!({ "task": "refactor rust module" })
            )
            .unwrap(),
            dispatch(
                &registry,
                "skills.discover",
                json!({ "task": "refactor rust module" })
            )
            .unwrap(),
        );
        assert_eq!(
            dispatch(
                &registry,
                "packs.get_manifest",
                json!({ "pack_id": PACK_ID })
            )
            .unwrap(),
            dispatch(&registry, "packs.load", json!({ "pack_id": PACK_ID })).unwrap(),
        );
    }

    #[test]
    fn supported_parameter_shape_variants_are_equivalent() {
        let registry = reference_registry();
        let report = reference_report_json();
        let runtime = compatibility_query_json();

        assert_eq!(
            dispatch(&registry, "skills.verify_result", report.clone()).unwrap(),
            dispatch(
                &registry,
                "skills.verify_result",
                json!({ "execution_report": report })
            )
            .unwrap(),
        );
        assert_eq!(
            dispatch(
                &registry,
                "skills.check_compatibility",
                json!({
                    "id": SKILL_ID,
                    "msp_version": "0.1.0",
                    "supported_manifest_versions": ["0.1.0"],
                    "runtime_name": "msp-reference-test",
                    "supported_formats": ["markdown"],
                    "runtime_capabilities": ["workspace_read"],
                    "model_capabilities": ["code_reasoning"],
                    "available_tools": ["read_file", "write_file"],
                    "permissions": ["workspace_write"],
                    "context_window": 128000,
                    "platform": "linux"
                })
            )
            .unwrap(),
            dispatch(
                &registry,
                "skills.check_compatibility",
                json!({ "id": SKILL_ID, "runtime": runtime.clone() })
            )
            .unwrap(),
        );
        assert_eq!(
            dispatch(
                &registry,
                "skills.check_compatibility",
                json!({ "id": SKILL_ID, "runtime": runtime.clone() })
            )
            .unwrap(),
            dispatch(
                &registry,
                "skills.check_compatibility",
                json!({ "id": SKILL_ID, "query": runtime })
            )
            .unwrap(),
        );
    }

    #[test]
    fn policy_required_methods_return_clean_missing_policy_errors() {
        let registry = reference_registry();
        for (method, id) in [
            ("packs.evaluate_trust", PACK_ID),
            ("packs.evaluate_dependencies", PACK_ID),
            ("trust.evaluate", SKILL_ID),
            ("trust.evaluate_dependencies", SKILL_ID),
        ] {
            let response = handle_request(
                &registry,
                JsonRpcRequest {
                    jsonrpc: "2.0".to_string(),
                    method: method.to_string(),
                    params: json!({ "id": id }),
                    id: Some(json!(1)),
                },
            );
            let error = response
                .error
                .unwrap_or_else(|| panic!("{method} should require policy"));
            assert_eq!(error.code, -32000);
            assert_eq!(error.message, "missing policy");
        }
    }

    #[test]
    fn packs_discover_returns_search_results() {
        let registry = reference_registry();
        let response = handle_request(
            &registry,
            JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                method: "packs.discover".to_string(),
                params: json!({ "task": "rust engineering" }),
                id: Some(json!(1)),
            },
        );

        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.expect("packs.discover result");
        let results = result.as_array().expect("result array");
        assert!(
            results
                .iter()
                .any(|pack| pack.get("id").and_then(Value::as_str) == Some(PACK_ID)),
            "{results:?}"
        );
    }

    fn reference_registry() -> LocalRegistry {
        LocalRegistry::open(workspace_root().join("examples/registry"))
            .expect("open reference registry")
    }

    fn reference_policy_json() -> Value {
        serde_json::from_str(include_str!(
            "../../../examples/policies/local-reference.trust-policy.json"
        ))
        .expect("reference policy parses")
    }

    fn reference_report_json() -> Value {
        serde_json::from_str(include_str!(
            "../../../examples/reports/rust-refactor.report.json"
        ))
        .expect("reference execution report parses")
    }

    fn compatibility_query_json() -> Value {
        json!({
            "msp_version": "0.1.0",
            "supported_manifest_versions": ["0.1.0"],
            "runtime_name": "msp-reference-test",
            "supported_formats": ["markdown"],
            "runtime_capabilities": ["workspace_read"],
            "model_capabilities": ["code_reasoning"],
            "available_tools": ["read_file", "write_file"],
            "permissions": ["workspace_write"],
            "context_window": 128000,
            "platform": "linux"
        })
    }

    fn workspace_root() -> PathBuf {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .parent()
            .and_then(Path::parent)
            .expect("crate is under workspace/crates/msp-server")
            .to_path_buf()
    }
}
