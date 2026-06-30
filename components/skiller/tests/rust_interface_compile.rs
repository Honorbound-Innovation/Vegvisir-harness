use std::process::Command;
use tempfile::tempdir;

#[test]
fn explicit_openapi_and_cli_help_compilers_work() {
    let temp = tempdir().unwrap();
    let api = temp.path().join("payments.yaml");
    std::fs::write(
        &api,
        "openapi: 3.0.0\npaths:\n  /payments:\n    get:\n      summary: List payments\n    post:\n      summary: Create payment\nGET /payments\nPOST /payments\n",
    )
    .unwrap();
    let api_bundle = temp.path().join("api-bundle");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "compile-openapi",
                api.to_str().unwrap(),
                "--out",
                api_bundle.to_str().unwrap(),
                "--name",
                "payments-api",
            ])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args(["validate", api_bundle.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    let api_skills = std::fs::read_dir(api_bundle.join("skills"))
        .unwrap()
        .count();
    assert!(api_skills > 0);

    let help = temp.path().join("deployctl-help.txt");
    std::fs::write(
        &help,
        "Usage: deployctl <command>\n\ndeployctl status\ndeployctl plan --dry-run\ndeployctl apply --env prod\nWarning: apply modifies external systems.\n",
    )
    .unwrap();
    let cli_bundle = temp.path().join("cli-bundle");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "compile-cli-help",
                help.to_str().unwrap(),
                "--out",
                cli_bundle.to_str().unwrap(),
                "--name",
                "deployctl-cli",
            ])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args(["validate", cli_bundle.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    let skills_dir = cli_bundle.join("skills");
    let skill_yaml = std::fs::read_dir(skills_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let skill = std::fs::read_to_string(skill_yaml).unwrap();
    assert!(skill.contains("CliOperation"));
    assert!(skill.contains("requires_user_approval: true"));
}

#[test]
fn deterministic_compile_filters_fake_cli_operations_from_prose_and_code() {
    let temp = tempdir().unwrap();
    let doc = temp.path().join("debugging.md");
    std::fs::write(
        &doc,
        r#"# Debugging and Error Recovery

Use this procedure when a regression appears.

1. STOP adding features or making changes
3. DIAGNOSE using the triage checklist
5. GUARD against recurrence

```javascript
try {
  recover();
}
```

Real command example:

```bash
cargo test -p skiller
```
"#,
    )
    .unwrap();
    let bundle = temp.path().join("debugging-bundle");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "compile",
                doc.to_str().unwrap(),
                "--out",
                bundle.to_str().unwrap(),
                "--name",
                "debugging",
            ])
            .status()
            .unwrap()
            .success()
    );

    let mut all_skills = String::new();
    for entry in std::fs::read_dir(bundle.join("skills")).unwrap() {
        let path = entry.unwrap().path();
        all_skills.push_str(&std::fs::read_to_string(path).unwrap());
        all_skills.push('\n');
    }
    assert!(
        all_skills.contains("target_command: cargo test -p skiller"),
        "{all_skills}"
    );
    assert!(
        !all_skills.contains("target_command: try {"),
        "{all_skills}"
    );
    assert!(
        !all_skills.contains("target_command: 1. STOP adding features or making changes"),
        "{all_skills}"
    );
    assert!(!all_skills.contains("Run `try {`"), "{all_skills}");
    assert!(
        !all_skills.contains("Run `1. STOP adding features or making changes`"),
        "{all_skills}"
    );

    let validate = Command::new(env!("CARGO_BIN_EXE_skiller"))
        .args(["validate", bundle.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        validate.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&validate.stdout),
        String::from_utf8_lossy(&validate.stderr)
    );
}

#[test]
fn validation_rejects_legacy_suspicious_cli_operation() {
    let temp = tempdir().unwrap();
    let help = temp.path().join("tool-help.txt");
    std::fs::write(&help, "Usage: tool <command>\n\ntool status --json\n").unwrap();
    let bundle = temp.path().join("tool-bundle");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "compile-cli-help",
                help.to_str().unwrap(),
                "--out",
                bundle.to_str().unwrap(),
                "--name",
                "tool-cli",
            ])
            .status()
            .unwrap()
            .success()
    );

    let skill_path = std::fs::read_dir(bundle.join("skills"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let skill_yaml = std::fs::read_to_string(&skill_path)
        .unwrap()
        .replace("Run `tool status --json`", "Run `try {`")
        .replace(
            "target_command: tool status --json",
            "target_command: try {",
        )
        .replace("tool_name: tool", "tool_name: try");
    std::fs::write(&skill_path, skill_yaml).unwrap();

    let validate = Command::new(env!("CARGO_BIN_EXE_skiller"))
        .args(["validate", bundle.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!validate.status.success());
    let stdout = String::from_utf8_lossy(&validate.stdout);
    assert!(stdout.contains("suspicious CliOperation"), "{stdout}");
    assert!(stdout.contains("programming_syntax"), "{stdout}");
}

#[test]
fn vegvisir_forge_fallback_marks_provider_review_absent() {
    let temp = tempdir().unwrap();
    let help = temp.path().join("deployctl-help.txt");
    std::fs::write(
        &help,
        "Usage: deployctl <command>\n\ndeployctl status --json\n",
    )
    .unwrap();
    let bundle = temp.path().join("deployctl-bundle");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "compile-cli-help",
                help.to_str().unwrap(),
                "--out",
                bundle.to_str().unwrap(),
                "--name",
                "deployctl-cli",
            ])
            .status()
            .unwrap()
            .success()
    );
    let forged = temp.path().join("deployctl-forged");
    let forge = Command::new(env!("CARGO_BIN_EXE_skiller"))
        .env_remove("SKILLER_VEGVISIR_FORGE_ADAPTER")
        .args([
            "forge",
            bundle.to_str().unwrap(),
            "--out",
            forged.to_str().unwrap(),
            "--provider",
            "vegvisir",
        ])
        .output()
        .unwrap();
    assert!(
        forge.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&forge.stdout),
        String::from_utf8_lossy(&forge.stderr)
    );

    let responses = std::fs::read_to_string(forged.join("forge_responses.yaml")).unwrap();
    assert!(
        responses.contains("semantic_review: deterministic_fallback_only"),
        "{responses}"
    );
    assert!(
        responses.contains("provider_reviewed: 'false'")
            || responses.contains("provider_reviewed: \"false\""),
        "{responses}"
    );
    let summary = std::fs::read_to_string(forged.join("forge_summary.yaml")).unwrap();
    assert!(summary.contains("live_reasoning: false"), "{summary}");
}

#[test]
fn suspicious_commands_reports_compact_semantic_diagnostics() {
    let temp = tempdir().unwrap();
    let help = temp.path().join("tool-help.txt");
    std::fs::write(&help, "Usage: tool <command>\n\ntool status --json\n").unwrap();
    let bundle = temp.path().join("tool-bundle");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "compile-cli-help",
                help.to_str().unwrap(),
                "--out",
                bundle.to_str().unwrap(),
                "--name",
                "tool-cli",
            ])
            .status()
            .unwrap()
            .success()
    );

    let skill_path = std::fs::read_dir(bundle.join("skills"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let skill_yaml = std::fs::read_to_string(&skill_path)
        .unwrap()
        .replace("Run `tool status --json`", "Run `1. STOP adding features`")
        .replace(
            "target_command: tool status --json",
            "target_command: 1. STOP adding features",
        )
        .replace("tool_name: tool", "tool_name: '1.'");
    std::fs::write(&skill_path, skill_yaml).unwrap();

    let report = Command::new(env!("CARGO_BIN_EXE_skiller"))
        .args(["suspicious-commands", bundle.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(report.status.success());
    let stdout = String::from_utf8_lossy(&report.stdout);
    assert!(
        stdout.contains("suspicious_cli_operation_count: 1"),
        "{stdout}"
    );
    assert!(stdout.contains("suspicious_cli_operation"), "{stdout}");
    assert!(stdout.contains("numbered_process_step"), "{stdout}");
}
