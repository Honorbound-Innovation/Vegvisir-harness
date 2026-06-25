use msp_core::{MspSchemaKind, core_methods, validate_json_schema};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{fs, io::Write};

#[derive(Debug, Clone, Copy)]
struct FixtureCase {
    slug: &'static str,
    method: &'static str,
    schema: Option<MspSchemaKind>,
}

#[derive(Debug, Clone, Copy)]
struct ErrorFixtureCase {
    slug: &'static str,
    method: Option<&'static str>,
    code: i64,
    id: Option<&'static str>,
}

const CASES: &[FixtureCase] = &[
    FixtureCase {
        slug: "01-msp-info",
        method: "msp.info",
        schema: None,
    },
    FixtureCase {
        slug: "02-registry-search",
        method: "registry.search",
        schema: Some(MspSchemaKind::RegistrySearchResultArray),
    },
    FixtureCase {
        slug: "03-skills-discover",
        method: "skills.discover",
        schema: Some(MspSchemaKind::RegistrySearchResultArray),
    },
    FixtureCase {
        slug: "04-skills-get-manifest",
        method: "skills.get_manifest",
        schema: Some(MspSchemaKind::Manifest),
    },
    FixtureCase {
        slug: "05-skills-load",
        method: "skills.load",
        schema: Some(MspSchemaKind::SkillLoadResult),
    },
    FixtureCase {
        slug: "06-skills-resolve-dependencies",
        method: "skills.resolve_dependencies",
        schema: Some(MspSchemaKind::DependencyResolutionResult),
    },
    FixtureCase {
        slug: "07-skills-verify-result",
        method: "skills.verify_result",
        schema: Some(MspSchemaKind::SkillVerificationResult),
    },
    FixtureCase {
        slug: "08-skills-check-compatibility",
        method: "skills.check_compatibility",
        schema: Some(MspSchemaKind::SkillCompatibilityResult),
    },
    FixtureCase {
        slug: "09-packs-discover",
        method: "packs.discover",
        schema: Some(MspSchemaKind::PackSearchResultArray),
    },
    FixtureCase {
        slug: "10-packs-get-manifest",
        method: "packs.get_manifest",
        schema: Some(MspSchemaKind::SkillPack),
    },
    FixtureCase {
        slug: "11-packs-load",
        method: "packs.load",
        schema: Some(MspSchemaKind::SkillPack),
    },
    FixtureCase {
        slug: "12-packs-verify-trust",
        method: "packs.verify_trust",
        schema: Some(MspSchemaKind::TrustVerifyResult),
    },
    FixtureCase {
        slug: "13-packs-evaluate-trust",
        method: "packs.evaluate_trust",
        schema: Some(MspSchemaKind::TrustPolicyEvaluation),
    },
    FixtureCase {
        slug: "14-packs-validate-members",
        method: "packs.validate_members",
        schema: Some(MspSchemaKind::PackMemberValidationResult),
    },
    FixtureCase {
        slug: "15-packs-evaluate-dependencies",
        method: "packs.evaluate_dependencies",
        schema: Some(MspSchemaKind::DependencyTrustEvaluationResult),
    },
    FixtureCase {
        slug: "16-trust-verify",
        method: "trust.verify",
        schema: Some(MspSchemaKind::TrustVerifyResult),
    },
    FixtureCase {
        slug: "17-trust-evaluate",
        method: "trust.evaluate",
        schema: Some(MspSchemaKind::TrustPolicyEvaluation),
    },
    FixtureCase {
        slug: "18-trust-evaluate-dependencies",
        method: "trust.evaluate_dependencies",
        schema: Some(MspSchemaKind::DependencyTrustEvaluationResult),
    },
];

const ERROR_CASES: &[ErrorFixtureCase] = &[
    ErrorFixtureCase {
        slug: "01-parse-error",
        method: None,
        code: -32700,
        id: None,
    },
    ErrorFixtureCase {
        slug: "02-invalid-jsonrpc-version",
        method: Some("msp.info"),
        code: -32600,
        id: Some("err-invalid-version"),
    },
    ErrorFixtureCase {
        slug: "03-unknown-method",
        method: Some("unknown.method"),
        code: -32601,
        id: Some("err-unknown-method"),
    },
    ErrorFixtureCase {
        slug: "04-missing-id",
        method: Some("skills.get_manifest"),
        code: -32000,
        id: Some("err-missing-id"),
    },
    ErrorFixtureCase {
        slug: "05-missing-policy",
        method: Some("trust.evaluate"),
        code: -32000,
        id: Some("err-missing-policy"),
    },
    ErrorFixtureCase {
        slug: "06-invalid-search-risk",
        method: Some("registry.search"),
        code: -32000,
        id: Some("err-invalid-search-risk"),
    },
    ErrorFixtureCase {
        slug: "07-missing-skill",
        method: Some("skills.load"),
        code: -32000,
        id: Some("err-missing-skill"),
    },
];

#[test]
fn jsonrpc_conformance_fixtures_track_advertised_methods() {
    let fixture_methods: Vec<_> = CASES.iter().map(|case| case.method.to_string()).collect();
    assert_eq!(fixture_methods, core_methods());
}

#[test]
fn jsonrpc_conformance_response_fixtures_are_schema_valid() {
    for case in CASES {
        let request = read_json_fixture("requests", case.slug, "request");
        assert_eq!(request["jsonrpc"], "2.0");
        assert_eq!(request["method"], case.method);

        let response = read_json_fixture("responses", case.slug, "response");
        assert_success_response(case, &response);
    }
}

#[test]
fn jsonrpc_error_conformance_fixtures_have_expected_codes() {
    for case in ERROR_CASES {
        if let Some(method) = case.method {
            let request = read_json_fixture("error-requests", case.slug, "request");
            assert_eq!(request["method"], method);
        } else {
            let request_path = fixture_path("error-requests", case.slug, "request");
            let request = fs::read_to_string(&request_path).unwrap_or_else(|error| {
                panic!("failed to read {}: {error}", request_path.display())
            });
            assert!(
                serde_json::from_str::<Value>(&request).is_err(),
                "{} should be an invalid JSON parse-error fixture",
                case.slug
            );
        }

        let response = read_json_fixture("error-responses", case.slug, "response");
        assert_error_response(case, &response);
    }
}

#[test]
fn msp_server_replays_jsonrpc_conformance_fixtures_byte_for_byte() {
    replay_and_assert_fixtures(
        CASES
            .iter()
            .map(|case| ("requests", "responses", case.slug)),
        |slug, actual| {
            let case = CASES
                .iter()
                .find(|case| case.slug == slug)
                .expect("known success case");
            assert_success_response(case, actual);
        },
    );
}

#[test]
fn msp_server_replays_jsonrpc_error_conformance_fixtures_byte_for_byte() {
    replay_and_assert_fixtures(
        ERROR_CASES
            .iter()
            .map(|case| ("error-requests", "error-responses", case.slug)),
        |slug, actual| {
            let case = ERROR_CASES
                .iter()
                .find(|case| case.slug == slug)
                .expect("known error case");
            assert_error_response(case, actual);
        },
    );
}

fn replay_and_assert_fixtures<I, F>(cases: I, assert_response: F)
where
    I: IntoIterator<Item = (&'static str, &'static str, &'static str)>,
    F: Fn(&str, &Value),
{
    let cases: Vec<_> = cases.into_iter().collect();
    let mut child = Command::new(env!("CARGO_BIN_EXE_msp-server"))
        .current_dir(workspace_root())
        .arg("--registry")
        .arg("examples/registry")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn msp-server");

    {
        let stdin = child.stdin.as_mut().expect("child stdin");
        for (request_dir, _, slug) in &cases {
            let request_path = fixture_path(request_dir, slug, "request");
            let request_bytes = fs::read(&request_path).unwrap_or_else(|error| {
                panic!("failed to read {}: {error}", request_path.display())
            });
            stdin
                .write_all(&request_bytes)
                .unwrap_or_else(|error| panic!("failed to write {slug}: {error}"));
        }
    }

    let output = child.wait_with_output().expect("msp-server exits");
    assert!(
        output.status.success(),
        "msp-server failed with status {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let actual_stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let actual_lines: Vec<_> = actual_stdout.lines().map(str::to_string).collect();
    assert_eq!(actual_lines.len(), cases.len());

    for ((_, response_dir, slug), actual_line) in cases.iter().zip(actual_lines) {
        let expected_path = fixture_path(response_dir, slug, "response");
        let expected_line = fs::read_to_string(&expected_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", expected_path.display()))
            .trim_end()
            .to_string();
        assert_eq!(
            actual_line, expected_line,
            "{slug} response drifted from golden fixture"
        );

        let actual: Value = serde_json::from_str(&actual_line)
            .unwrap_or_else(|error| panic!("{slug} response is not JSON: {error}"));
        assert_response(slug, &actual);
    }
}

fn assert_success_response(case: &FixtureCase, response: &Value) {
    assert_eq!(
        response["jsonrpc"], "2.0",
        "{} invalid JSON-RPC version",
        case.slug
    );
    assert!(
        response.get("error").is_none(),
        "{} unexpectedly returned error: {:?}",
        case.slug,
        response.get("error")
    );
    let result = response
        .get("result")
        .unwrap_or_else(|| panic!("{} missing result", case.slug));

    if case.method == "msp.info" {
        assert_eq!(result["methods"], serde_json::json!(core_methods()));
    }

    if let Some(schema) = case.schema {
        validate_json_schema(schema, result).unwrap_or_else(|error| {
            panic!(
                "{} result failed {schema:?} validation: {error}\n{result:#}",
                case.slug
            )
        });
    }
}

fn assert_error_response(case: &ErrorFixtureCase, response: &Value) {
    assert_eq!(
        response["jsonrpc"], "2.0",
        "{} invalid JSON-RPC version",
        case.slug
    );
    assert!(
        response.get("result").is_none(),
        "{} unexpectedly returned result: {:?}",
        case.slug,
        response.get("result")
    );
    let error = response
        .get("error")
        .unwrap_or_else(|| panic!("{} missing error", case.slug));
    assert_eq!(
        error["code"], case.code,
        "{} returned wrong error code",
        case.slug
    );
    assert!(
        error.get("message").and_then(Value::as_str).is_some(),
        "{} error missing string message",
        case.slug
    );

    match case.id {
        Some(id) => assert_eq!(response["id"], id, "{} returned wrong id", case.slug),
        None => assert!(
            response.get("id").is_none(),
            "{} should omit id, got {:?}",
            case.slug,
            response.get("id")
        ),
    }
}

fn read_json_fixture(kind: &str, slug: &str, suffix: &str) -> Value {
    let path = fixture_path(kind, slug, suffix);
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_json::from_str(&content)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

fn fixture_path(kind: &str, slug: &str, suffix: &str) -> PathBuf {
    workspace_root()
        .join("examples/conformance/jsonrpc/v0.1")
        .join(kind)
        .join(format!("{slug}.{suffix}.json"))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate is under workspace/crates/msp-server")
        .to_path_buf()
}
