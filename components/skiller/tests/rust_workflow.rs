use std::process::Command;
use tempfile::tempdir;

#[test]
fn compile_forge_review_and_agent_pack_workflow() {
    let temp = tempdir().unwrap();
    let docs = temp.path().join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("ops.md"),
        "# Diagnose Pods\n\nYou should inspect pod status first.\n\n```\nkubectl get pods\nkubectl logs demo\n```\n\nWarning: never delete production pods without approval.\n",
    )
    .unwrap();

    let bundle = temp.path().join("bundle");
    let forged = temp.path().join("forged");
    let review = temp.path().join("review");
    let agent = temp.path().join("agent");

    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "compile",
                docs.to_str().unwrap(),
                "--out",
                bundle.to_str().unwrap(),
                "--name",
                "workflow",
                "--domain",
                "kubernetes",
            ])
            .status()
            .unwrap()
            .success()
    );

    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "forge",
                bundle.to_str().unwrap(),
                "--out",
                forged.to_str().unwrap(),
                "--provider",
                "vegvisir",
                "--domain-profile",
                "kubernetes-operations",
                "--max-skills",
                "2",
            ])
            .status()
            .unwrap()
            .success()
    );

    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "review-agent",
                forged.to_str().unwrap(),
                "--out",
                review.to_str().unwrap(),
            ])
            .status()
            .unwrap()
            .success()
    );
    assert!(review.join("verifier-review.yaml").exists());
    assert!(review.join("verifier-review.md").exists());

    let reviewed = temp.path().join("reviewed");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "apply-review",
                forged.to_str().unwrap(),
                "--review",
                review.join("verifier-review.yaml").to_str().unwrap(),
                "--out",
                reviewed.to_str().unwrap(),
            ])
            .status()
            .unwrap()
            .success()
    );
    assert!(reviewed.join("audit/events.yaml").exists());

    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "build-agent-pack",
                forged.to_str().unwrap(),
                "--agent",
                "Cluster Diagnostic Agent",
                "--out",
                agent.to_str().unwrap(),
            ])
            .status()
            .unwrap()
            .success()
    );
    let pack = std::fs::read_to_string(agent.join("agent-pack.yaml")).unwrap();
    assert!(pack.contains("required_skills"));
    assert!(pack.contains("optional_skills"));
    assert!(pack.contains("approval_policy"));
}

#[test]
fn forge_handoff_and_validate_template_workflow() {
    let temp = tempdir().unwrap();
    let docs = temp.path().join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("cli.md"),
        "# Inspect Cache\n\nUse status commands before mutations.\n\n```\ncachectl status\n```\n",
    )
    .unwrap();

    let bundle = temp.path().join("bundle");
    let handoff = temp.path().join("handoff");

    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "compile",
                docs.to_str().unwrap(),
                "--out",
                bundle.to_str().unwrap(),
                "--name",
                "handoff-workflow",
                "--domain",
                "cli",
            ])
            .status()
            .unwrap()
            .success()
    );

    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "forge-handoff",
                bundle.to_str().unwrap(),
                "--out",
                handoff.to_str().unwrap(),
                "--pass",
                "skill-expansion",
                "--max-skills",
                "1",
            ])
            .status()
            .unwrap()
            .success()
    );

    assert!(handoff.join("forge-request.yaml").exists());
    assert!(handoff.join("forge-response-template.yaml").exists());
    let prompt = std::fs::read_to_string(handoff.join("vegvisir-prompt.md")).unwrap();
    assert!(prompt.contains("Return ONLY a valid `ForgeResponseEnvelope`"));

    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "forge-validate",
                bundle.to_str().unwrap(),
                "--request",
                handoff.join("forge-request.yaml").to_str().unwrap(),
                "--response",
                handoff
                    .join("forge-response-template.yaml")
                    .to_str()
                    .unwrap(),
            ])
            .status()
            .unwrap()
            .success()
    );
}
