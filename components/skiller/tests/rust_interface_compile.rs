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
