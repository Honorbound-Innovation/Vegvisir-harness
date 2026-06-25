use msp_core::{MspSchemaKind, validate_json_schema};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str;
use std::{fs, process::ExitStatus};

#[derive(Debug, Clone, Copy)]
struct CliFixtureCase {
    slug: &'static str,
    schema: Option<MspSchemaKind>,
    json_stdout: bool,
}

#[derive(Debug, Clone, Copy)]
struct CliErrorFixtureCase {
    slug: &'static str,
    code: i32,
    stderr_contains: &'static str,
}

const CASES: &[CliFixtureCase] = &[
    CliFixtureCase {
        slug: "01-info",
        schema: None,
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "02-registry-index",
        schema: None,
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "03-registry-search",
        schema: Some(MspSchemaKind::RegistrySearchResultArray),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "04-skills-discover",
        schema: Some(MspSchemaKind::RegistrySearchResultArray),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "05-skills-manifest",
        schema: Some(MspSchemaKind::Manifest),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "06-skills-load",
        schema: Some(MspSchemaKind::SkillLoadResult),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "07-skills-resolve-dependencies",
        schema: Some(MspSchemaKind::DependencyResolutionResult),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "08-skills-verify-result",
        schema: Some(MspSchemaKind::SkillVerificationResult),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "09-skills-check-compatibility",
        schema: Some(MspSchemaKind::SkillCompatibilityResult),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "10-packs-discover",
        schema: Some(MspSchemaKind::PackSearchResultArray),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "11-packs-manifest",
        schema: Some(MspSchemaKind::SkillPack),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "12-packs-load",
        schema: Some(MspSchemaKind::SkillPack),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "13-packs-verify-trust",
        schema: Some(MspSchemaKind::TrustVerifyResult),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "14-packs-evaluate-trust",
        schema: Some(MspSchemaKind::TrustPolicyEvaluation),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "15-packs-validate-members",
        schema: Some(MspSchemaKind::PackMemberValidationResult),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "16-packs-evaluate-dependencies",
        schema: Some(MspSchemaKind::DependencyTrustEvaluationResult),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "17-trust-verify",
        schema: Some(MspSchemaKind::TrustVerifyResult),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "18-trust-verify-body",
        schema: Some(MspSchemaKind::TrustVerifyResult),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "19-trust-evaluate",
        schema: Some(MspSchemaKind::TrustPolicyEvaluation),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "20-trust-evaluate-dependencies",
        schema: Some(MspSchemaKind::DependencyTrustEvaluationResult),
        json_stdout: true,
    },
    CliFixtureCase {
        slug: "21-hash-body",
        schema: None,
        json_stdout: false,
    },
];

const ERROR_CASES: &[CliErrorFixtureCase] = &[
    CliErrorFixtureCase {
        slug: "01-missing-skill-manifest",
        code: 1,
        stderr_contains: "skill not found: skill.missing.example.v1",
    },
    CliErrorFixtureCase {
        slug: "02-missing-pack-manifest",
        code: 1,
        stderr_contains: "pack not found: pack.missing.example.v1",
    },
    CliErrorFixtureCase {
        slug: "03-invalid-risk-registry-search",
        code: 1,
        stderr_contains: "expected one of: low, medium, high, critical",
    },
    CliErrorFixtureCase {
        slug: "04-missing-policy",
        code: 1,
        stderr_contains: "failed to load policy examples/policies/missing.trust-policy.json",
    },
    CliErrorFixtureCase {
        slug: "05-missing-hash-file",
        code: 1,
        stderr_contains: "failed to hash examples/registry/missing.md",
    },
    CliErrorFixtureCase {
        slug: "06-invalid-tool-version",
        code: 2,
        stderr_contains: "invalid value 'read_file' for '--tool-version <TOOL_VERSIONS>': expected KEY=VALUE",
    },
    CliErrorFixtureCase {
        slug: "07-missing-report",
        code: 1,
        stderr_contains: "failed to load execution report examples/reports/missing.report.json",
    },
];

#[test]
fn cli_conformance_fixtures_are_well_formed_and_schema_valid() {
    for case in CASES {
        let args = read_args("commands", case.slug);
        assert!(!args.is_empty(), "{} args must not be empty", case.slug);

        let status = read_status("status", case.slug);
        assert_eq!(
            status["success"], true,
            "{} should be success fixture",
            case.slug
        );
        assert_eq!(status["code"], 0, "{} should exit zero", case.slug);

        let stderr = fs::read_to_string(fixture_path("stderr", case.slug, "stderr.txt"))
            .expect("read stderr fixture");
        assert!(stderr.is_empty(), "{} expected empty stderr", case.slug);

        let stdout = fs::read_to_string(fixture_path("stdout", case.slug, "stdout.txt"))
            .expect("read stdout fixture");
        assert_stdout_contract(case, &stdout);
    }
}

#[test]
fn cli_error_conformance_fixtures_are_well_formed_and_actionable() {
    for case in ERROR_CASES {
        let args = read_args("error-commands", case.slug);
        assert!(!args.is_empty(), "{} args must not be empty", case.slug);

        let status = read_status("error-status", case.slug);
        assert_eq!(
            status["success"], false,
            "{} should be error fixture",
            case.slug
        );
        assert_eq!(status["code"], case.code, "{} exit code", case.slug);

        let stdout = fs::read_to_string(fixture_path("error-stdout", case.slug, "stdout.txt"))
            .expect("read error stdout fixture");
        assert!(stdout.is_empty(), "{} expected empty stdout", case.slug);

        let stderr = fs::read_to_string(fixture_path("error-stderr", case.slug, "stderr.txt"))
            .expect("read error stderr fixture");
        assert!(
            stderr.contains(case.stderr_contains),
            "{} stderr should contain {:?}, got:\n{}",
            case.slug,
            case.stderr_contains,
            stderr
        );
        assert!(
            stderr.ends_with('\n'),
            "{} stderr should end with newline",
            case.slug
        );
    }
}

#[test]
fn msp_cli_replays_conformance_fixtures_byte_for_byte() {
    for case in CASES {
        let output = run_fixture("commands", case.slug);

        assert_status_matches("status", case.slug, output.status);
        assert_output_matches("stdout", case.slug, "stdout", &output.stdout);
        assert_output_matches("stderr", case.slug, "stderr", &output.stderr);

        let stdout = str::from_utf8(&output.stdout)
            .unwrap_or_else(|error| panic!("{} stdout not utf-8: {error}", case.slug));
        assert_stdout_contract(case, stdout);
    }
}

#[test]
fn msp_cli_replays_error_conformance_fixtures_byte_for_byte() {
    for case in ERROR_CASES {
        let output = run_fixture("error-commands", case.slug);

        assert_status_matches("error-status", case.slug, output.status);
        assert_output_matches("error-stdout", case.slug, "stdout", &output.stdout);
        assert_output_matches("error-stderr", case.slug, "stderr", &output.stderr);

        let stderr = str::from_utf8(&output.stderr)
            .unwrap_or_else(|error| panic!("{} stderr not utf-8: {error}", case.slug));
        assert!(
            stderr.contains(case.stderr_contains),
            "{} stderr should contain {:?}, got:\n{}",
            case.slug,
            case.stderr_contains,
            stderr
        );
    }
}

#[test]
fn cli_alias_fixtures_remain_identical() {
    assert_eq!(
        read_stdout_json("03-registry-search"),
        read_stdout_json("04-skills-discover")
    );
    assert_eq!(
        read_stdout_json("11-packs-manifest"),
        read_stdout_json("12-packs-load")
    );
    assert_eq!(
        read_stdout_json("17-trust-verify"),
        read_stdout_json("18-trust-verify-body")
    );
}

fn assert_stdout_contract(case: &CliFixtureCase, stdout: &str) {
    assert!(
        stdout.ends_with('\n'),
        "{} stdout should end with newline",
        case.slug
    );
    if case.json_stdout {
        let value: Value = serde_json::from_str(stdout).unwrap_or_else(|error| {
            panic!("{} stdout should be JSON: {error}\n{stdout}", case.slug)
        });
        if let Some(schema) = case.schema {
            validate_json_schema(schema, &value).unwrap_or_else(|error| {
                panic!(
                    "{} stdout failed {schema:?} validation: {error}\n{value:#}",
                    case.slug
                )
            });
        }
    } else {
        let digest = stdout.trim_end();
        let hex = digest
            .strip_prefix("sha256:")
            .unwrap_or_else(|| panic!("{} hash should use sha256: prefix", case.slug));
        assert_eq!(hex.len(), 64, "{} hash length", case.slug);
        assert!(
            hex.chars().all(|ch| ch.is_ascii_hexdigit()),
            "{} hash should be hex",
            case.slug
        );
    }
}

fn run_fixture(command_dir: &str, slug: &str) -> std::process::Output {
    let args = read_args(command_dir, slug);
    Command::new(env!("CARGO_BIN_EXE_msp-cli"))
        .current_dir(workspace_root())
        .args(&args)
        .output()
        .unwrap_or_else(|error| panic!("failed to run {slug}: {error}"))
}

fn assert_status_matches(status_dir: &str, slug: &str, actual: ExitStatus) {
    let expected = read_status(status_dir, slug);
    assert_eq!(
        actual.success(),
        expected["success"].as_bool().expect("status success bool"),
        "{slug} success mismatch"
    );
    assert_eq!(
        actual.code(),
        expected["code"].as_i64().map(|code| code as i32),
        "{slug} exit code mismatch"
    );
}

fn assert_output_matches(fixture_dir: &str, slug: &str, stream: &str, actual: &[u8]) {
    let expected_path = fixture_path(fixture_dir, slug, &format!("{stream}.txt"));
    let expected = fs::read(&expected_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", expected_path.display()));
    assert_eq!(
        actual,
        expected.as_slice(),
        "{slug} {stream} drifted from golden fixture"
    );
}

fn read_args(command_dir: &str, slug: &str) -> Vec<String> {
    let path = fixture_path(command_dir, slug, "args.json");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_json::from_str(&content)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

fn read_status(status_dir: &str, slug: &str) -> Value {
    let path = fixture_path(status_dir, slug, "status.json");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_json::from_str(&content)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

fn read_stdout_json(slug: &str) -> Value {
    let path = fixture_path("stdout", slug, "stdout.txt");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_json::from_str(&content)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

fn fixture_path(kind: &str, slug: &str, suffix: &str) -> PathBuf {
    workspace_root()
        .join("examples/conformance/cli/v0.1")
        .join(kind)
        .join(format!("{slug}.{suffix}"))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate is under workspace/crates/msp-cli")
        .to_path_buf()
}
