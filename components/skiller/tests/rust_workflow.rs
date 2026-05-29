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

#[test]
fn older_minimal_skill_bundles_load_with_expanded_defaults() {
    let temp = tempdir().unwrap();
    let bundle = temp.path().join("old-bundle");
    std::fs::create_dir_all(bundle.join("skills")).unwrap();
    std::fs::create_dir_all(bundle.join("sources")).unwrap();
    std::fs::create_dir_all(bundle.join("graph")).unwrap();
    std::fs::create_dir_all(bundle.join("audit")).unwrap();

    std::fs::write(
        bundle.join("package.yaml"),
        r#"---
bundle_id: bundle-old
name: old-format
version: 0.1.0
domain: null
source_corpus: []
review_status: Candidate
publish_status: Unpublished
compatibility: {}
created_at: 2025-01-01T00:00:00Z
"#,
    )
    .unwrap();
    std::fs::write(bundle.join("sources/index.yaml"), "[]\n").unwrap();
    std::fs::write(bundle.join("sources/sections.yaml"), "[]\n").unwrap();
    std::fs::write(bundle.join("graph/concepts.yaml"), "[]\n").unwrap();
    std::fs::write(bundle.join("graph/dependencies.yaml"), "[]\n").unwrap();
    std::fs::write(bundle.join("audit/events.yaml"), "[]\n").unwrap();
    std::fs::write(bundle.join("candidates.yaml"), "[]\n").unwrap();
    std::fs::write(
        bundle.join("skills/skill-old.yaml"),
        r#"---
id: skill-old
title: Old bundle skill
summary: Minimal pre-expansion skill artifact.
skill_type: Procedure
scope: TaskLevel
status: Candidate
"#,
    )
    .unwrap();

    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args(["list", bundle.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn forge_validate_rejects_unsupported_source_and_out_of_range_scores() {
    let temp = tempdir().unwrap();
    let docs = temp.path().join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("ops.md"),
        "# Inspect Service\n\nYou should inspect service status before making changes.\n\n```\nsvc status\n```\n\nWarning: never restart production services without approval.\n",
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
                "forge-validation",
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

    let request_text = std::fs::read_to_string(handoff.join("forge-request.yaml")).unwrap();
    let request: serde_yaml::Value = serde_yaml::from_str(&request_text).unwrap();
    let request_id = request["request_id"].as_str().unwrap();
    let skill = request["candidate_skills"][0].clone();
    let skill_id = skill["id"].as_str().unwrap().to_string();
    let section_id = skill["source_section_ids"][0].as_str().unwrap().to_string();

    let mut bad_skill = skill;
    bad_skill["confidence"]["raw"] = serde_yaml::Value::from(1.2);
    bad_skill["evidence_breakdown"]["direct_extraction"] = serde_yaml::Value::from(0.8);
    bad_skill["evidence_breakdown"]["supporting_inference"] = serde_yaml::Value::from(0.4);
    bad_skill["citations"] = serde_yaml::to_value(vec![serde_yaml::Mapping::from_iter([
        (
            serde_yaml::Value::from("citation_id"),
            serde_yaml::Value::from("forged-bad-citation"),
        ),
        (
            serde_yaml::Value::from("source_id"),
            serde_yaml::Value::from("invented-source"),
        ),
        (
            serde_yaml::Value::from("section_id"),
            serde_yaml::Value::from(section_id.as_str()),
        ),
        (
            serde_yaml::Value::from("excerpt"),
            serde_yaml::Value::from("unsupported"),
        ),
    ])])
    .unwrap();

    let bad_confidence = serde_yaml::Mapping::from_iter([
        (
            serde_yaml::Value::from("raw"),
            serde_yaml::Value::from(-0.1),
        ),
        (
            serde_yaml::Value::from("extraction"),
            serde_yaml::Value::from(0.7),
        ),
        (
            serde_yaml::Value::from("inference"),
            serde_yaml::Value::from(0.1),
        ),
        (
            serde_yaml::Value::from("procedure"),
            serde_yaml::Value::from(0.5),
        ),
        (
            serde_yaml::Value::from("guardrail"),
            serde_yaml::Value::from(0.5),
        ),
        (
            serde_yaml::Value::from("eval"),
            serde_yaml::Value::from(0.4),
        ),
        (
            serde_yaml::Value::from("routing"),
            serde_yaml::Value::from(0.5),
        ),
        (
            serde_yaml::Value::from("source_quality"),
            serde_yaml::Value::from(0.5),
        ),
        (
            serde_yaml::Value::from("human_review"),
            serde_yaml::Value::from(0.0),
        ),
        (
            serde_yaml::Value::from("runtime"),
            serde_yaml::Value::from(0.0),
        ),
    ]);

    let response = serde_yaml::to_string(&serde_yaml::Mapping::from_iter([
        (
            serde_yaml::Value::from("request_id"),
            serde_yaml::Value::from(request_id),
        ),
        (
            serde_yaml::Value::from("pass_type"),
            serde_yaml::Value::from("SkillExpansion"),
        ),
        (
            serde_yaml::Value::from("generated_items"),
            serde_yaml::Value::Sequence(vec![]),
        ),
        (
            serde_yaml::Value::from("modified_items"),
            serde_yaml::Value::Sequence(vec![bad_skill]),
        ),
        (
            serde_yaml::Value::from("review_findings"),
            serde_yaml::Value::Sequence(vec![]),
        ),
        (
            serde_yaml::Value::from("confidence_updates"),
            serde_yaml::to_value(serde_yaml::Mapping::from_iter([(
                serde_yaml::Value::from(skill_id.as_str()),
                serde_yaml::Value::Mapping(bad_confidence),
            )]))
            .unwrap(),
        ),
        (
            serde_yaml::Value::from("evidence_records"),
            serde_yaml::Value::Sequence(vec![]),
        ),
        (
            serde_yaml::Value::from("required_human_review"),
            serde_yaml::Value::from(true),
        ),
        (
            serde_yaml::Value::from("audit_notes"),
            serde_yaml::Value::Sequence(vec![]),
        ),
    ]))
    .unwrap();
    let response_path = temp.path().join("bad-response.yaml");
    std::fs::write(&response_path, response).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_skiller"))
        .args([
            "forge-validate",
            bundle.to_str().unwrap(),
            "--request",
            handoff.join("forge-request.yaml").to_str().unwrap(),
            "--response",
            response_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("references missing source invented-source")
            || stderr.contains("must be between 0.0 and 1.0")
            || stderr.contains("total exceeds 1.0"),
        "stderr was: {stderr}"
    );
}

#[test]
fn validation_rejects_mismatched_citation_source_and_duplicate_skill_ids() {
    let temp = tempdir().unwrap();
    let docs = temp.path().join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("ops.md"),
        "# Inspect Service\n\nYou should inspect service status before making changes.\n\n```\nsvc status\n```\n\nWarning: never restart production services without approval.\n",
    )
    .unwrap();

    let bundle = temp.path().join("bundle");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "compile",
                docs.to_str().unwrap(),
                "--out",
                bundle.to_str().unwrap(),
                "--name",
                "citation-validation",
            ])
            .status()
            .unwrap()
            .success()
    );

    let mut skill_files: Vec<_> = std::fs::read_dir(bundle.join("skills"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("yaml"))
        .collect();
    skill_files.sort();
    let first_skill = skill_files.first().expect("compiled skill exists");
    let skill_text = std::fs::read_to_string(first_skill).unwrap();
    let mut duplicate_name = first_skill.file_stem().unwrap().to_os_string();
    duplicate_name.push("-duplicate.yaml");
    std::fs::write(bundle.join("skills").join(duplicate_name), &skill_text).unwrap();

    let section_text = std::fs::read_to_string(bundle.join("sources/sections.yaml")).unwrap();
    let section_id = section_text
        .lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("section_id: ")
                .or_else(|| line.strip_prefix("- section_id: "))
        })
        .expect("section id in generated bundle")
        .to_string();

    let bad_skill = format!(
        r#"---
id: forged-unsupported-citation
title: Unsupported citation should fail
summary: Citation source does not match the referenced section.
skill_type: Procedure
scope: TaskLevel
status: Candidate
source_section_ids:
  - {section_id}
citations:
  - citation_id: forged-bad-citation
    source_id: missing-source
    section_id: {section_id}
    excerpt: unsupported
"#
    );
    std::fs::write(
        bundle.join("skills/forged-unsupported-citation.yaml"),
        bad_skill,
    )
    .unwrap();

    let mut first_skill_yaml: serde_yaml::Value = serde_yaml::from_str(&skill_text).unwrap();
    first_skill_yaml["confidence"]["raw"] = serde_yaml::Value::from(1.2);
    first_skill_yaml["evidence_breakdown"]["direct_extraction"] = serde_yaml::Value::from(0.8);
    first_skill_yaml["evidence_breakdown"]["supporting_inference"] = serde_yaml::Value::from(0.4);
    first_skill_yaml["inference_records"] =
        serde_yaml::Value::Sequence(vec![serde_yaml::Value::Mapping({
            let mut m = serde_yaml::Mapping::new();
            m.insert(
                serde_yaml::Value::from("inference_id"),
                serde_yaml::Value::from("bad-inference"),
            );
            m.insert(
                serde_yaml::Value::from("candidate_ids_used"),
                serde_yaml::Value::Sequence(vec![serde_yaml::Value::from("missing-candidate")]),
            );
            m.insert(
                serde_yaml::Value::from("source_refs_used"),
                serde_yaml::Value::Sequence(vec![serde_yaml::Value::from("missing-section")]),
            );
            m.insert(
                serde_yaml::Value::from("reasoning_summary"),
                serde_yaml::Value::from("bad stored inference"),
            );
            m.insert(
                serde_yaml::Value::from("inference_type"),
                serde_yaml::Value::from("Expansion"),
            );
            m.insert(
                serde_yaml::Value::from("evidence_type"),
                serde_yaml::Value::from("SupportingInference"),
            );
            m.insert(
                serde_yaml::Value::from("confidence"),
                serde_yaml::Value::from(-0.1),
            );
            m.insert(
                serde_yaml::Value::from("unsupported_assumptions"),
                serde_yaml::Value::Sequence(vec![]),
            );
            m.insert(
                serde_yaml::Value::from("required_review"),
                serde_yaml::Value::from(true),
            );
            m.insert(
                serde_yaml::Value::from("risk_flags"),
                serde_yaml::Value::Sequence(vec![]),
            );
            m.insert(
                serde_yaml::Value::from("generated_by_agent"),
                serde_yaml::Value::from("test"),
            );
            m.insert(
                serde_yaml::Value::from("created_at"),
                serde_yaml::Value::from("2025-01-01T00:00:00Z"),
            );
            m
        })]);
    std::fs::write(
        first_skill,
        serde_yaml::to_string(&first_skill_yaml).unwrap(),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_skiller"))
        .args(["validate", bundle.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("duplicate skill id"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("references missing source missing-source"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("confidence.raw must be between 0.0 and 1.0"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("evidence_breakdown total exceeds 1.0 tolerance"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout
            .contains("inference record bad-inference references missing section missing-section"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains(
            "inference record bad-inference references unknown candidate missing-candidate"
        ),
        "stdout was: {stdout}"
    );
}

#[test]
fn readiness_blocks_unsafe_and_underreviewed_high_risk_publication() {
    let temp = tempdir().unwrap();
    let docs = temp.path().join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("deploy.md"),
        "# Deploy Service\n\nUse status first, then deploy only after approval.\n\n```\nsvcctl status demo\nsvcctl deploy demo\n```\n\nWarning: deployment changes external systems and requires rollback.\n",
    )
    .unwrap();

    let bundle = temp.path().join("bundle");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "compile",
                docs.to_str().unwrap(),
                "--out",
                bundle.to_str().unwrap(),
                "--name",
                "publication-gates",
            ])
            .status()
            .unwrap()
            .success()
    );

    let skill_path = std::fs::read_dir(bundle.join("skills"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|s| s.to_str()) == Some("yaml"))
        .unwrap();
    let skill_text = std::fs::read_to_string(&skill_path).unwrap();
    let mut skill_yaml: serde_yaml::Value = serde_yaml::from_str(&skill_text).unwrap();
    skill_yaml["status"] = serde_yaml::Value::from("Reviewed");
    skill_yaml["maturity"] = serde_yaml::Value::from("Level3Verified");
    skill_yaml["confidence"]["human_review"] = serde_yaml::Value::from(0.2);
    skill_yaml["runtime_policy"]["modify_external_systems"] = serde_yaml::Value::from(true);
    skill_yaml["runtime_policy"]["requires_user_approval"] = serde_yaml::Value::from(true);
    skill_yaml["runtime_policy"]["requires_backup_or_rollback"] = serde_yaml::Value::from(true);
    std::fs::write(&skill_path, serde_yaml::to_string(&skill_yaml).unwrap()).unwrap();

    let readiness = Command::new(env!("CARGO_BIN_EXE_skiller"))
        .args(["readiness", bundle.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(readiness.status.success());
    let stdout = String::from_utf8_lossy(&readiness.stdout);
    assert!(stdout.contains("ready: false"), "stdout was: {stdout}");
    assert!(
        stdout.contains("high-risk and requires human-approved Level4+ maturity"),
        "stdout was: {stdout}"
    );

    let registry = temp.path().join("registry");
    let publish = Command::new(env!("CARGO_BIN_EXE_skiller"))
        .args([
            "publish",
            bundle.to_str().unwrap(),
            "--registry",
            registry.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!publish.status.success());

    skill_yaml["status"] = serde_yaml::Value::from("Unsafe");
    skill_yaml["maturity"] = serde_yaml::Value::from("Level4HumanApproved");
    skill_yaml["confidence"]["human_review"] = serde_yaml::Value::from(0.9);
    std::fs::write(&skill_path, serde_yaml::to_string(&skill_yaml).unwrap()).unwrap();
    let readiness = Command::new(env!("CARGO_BIN_EXE_skiller"))
        .args(["readiness", bundle.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&readiness.stdout);
    assert!(
        stdout.contains("unsafe and cannot be published"),
        "stdout was: {stdout}"
    );
}

#[test]
fn agent_pack_does_not_promote_unsafe_archived_or_deprecated_skills() {
    let temp = tempdir().unwrap();
    let docs = temp.path().join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("ops.md"),
        "# Inspect Service\n\nUse status before mutation.\n\n```\nsvcctl status demo\n```\n",
    )
    .unwrap();

    let bundle = temp.path().join("bundle");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "compile",
                docs.to_str().unwrap(),
                "--out",
                bundle.to_str().unwrap(),
                "--name",
                "agent-pack-gates",
            ])
            .status()
            .unwrap()
            .success()
    );

    let skill_path = std::fs::read_dir(bundle.join("skills"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|s| s.to_str()) == Some("yaml"))
        .unwrap();
    let skill_text = std::fs::read_to_string(&skill_path).unwrap();
    let mut unsafe_skill: serde_yaml::Value = serde_yaml::from_str(&skill_text).unwrap();
    let unsafe_id = unsafe_skill["id"].as_str().unwrap().to_string();
    unsafe_skill["status"] = serde_yaml::Value::from("Unsafe");
    unsafe_skill["maturity"] = serde_yaml::Value::from("Level6Certified");
    std::fs::write(&skill_path, serde_yaml::to_string(&unsafe_skill).unwrap()).unwrap();

    let mut deprecated_skill = unsafe_skill.clone();
    let deprecated_id = format!("{unsafe_id}-deprecated");
    deprecated_skill["id"] = serde_yaml::Value::from(deprecated_id.clone());
    deprecated_skill["status"] = serde_yaml::Value::from("Deprecated");
    std::fs::write(
        bundle.join("skills").join(format!("{deprecated_id}.yaml")),
        serde_yaml::to_string(&deprecated_skill).unwrap(),
    )
    .unwrap();

    let mut reviewed_skill = unsafe_skill.clone();
    let reviewed_id = format!("{unsafe_id}-reviewed");
    reviewed_skill["id"] = serde_yaml::Value::from(reviewed_id.clone());
    reviewed_skill["status"] = serde_yaml::Value::from("Reviewed");
    reviewed_skill["maturity"] = serde_yaml::Value::from("Level3Verified");
    std::fs::write(
        bundle.join("skills").join(format!("{reviewed_id}.yaml")),
        serde_yaml::to_string(&reviewed_skill).unwrap(),
    )
    .unwrap();

    let out = temp.path().join("agent");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "build-agent-pack",
                bundle.to_str().unwrap(),
                "--agent",
                "Technical Documentation Agent",
                "--out",
                out.to_str().unwrap(),
            ])
            .status()
            .unwrap()
            .success()
    );
    let pack = std::fs::read_to_string(out.join("agent-pack.yaml")).unwrap();
    let pack_yaml: serde_yaml::Value = serde_yaml::from_str(&pack).unwrap();
    let required = pack_yaml["required_skills"].as_sequence().unwrap();
    let forbidden = pack_yaml["forbidden_skills"].as_sequence().unwrap();
    assert!(required.iter().any(|v| v.as_str() == Some(&reviewed_id)));
    assert!(!required.iter().any(|v| v.as_str() == Some(&unsafe_id)));
    assert!(!required.iter().any(|v| v.as_str() == Some(&deprecated_id)));
    assert!(forbidden.iter().any(|v| v.as_str() == Some(&unsafe_id)));
    assert!(forbidden.iter().any(|v| v.as_str() == Some(&deprecated_id)));
}

#[test]
fn readiness_enforces_source_rights_and_secret_scan_policy() {
    let temp = tempdir().unwrap();
    let docs = temp.path().join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("ops.md"),
        "# Inspect Service

Use status before mutation.

```
svcctl status demo
```
",
    )
    .unwrap();

    let bundle = temp.path().join("bundle");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "compile",
                docs.to_str().unwrap(),
                "--out",
                bundle.to_str().unwrap(),
                "--name",
                "source-policy-gates",
            ])
            .status()
            .unwrap()
            .success()
    );

    let package_path = bundle.join("package.yaml");
    let mut package_yaml: serde_yaml::Value =
        serde_yaml::from_str(&std::fs::read_to_string(&package_path).unwrap()).unwrap();
    package_yaml["compatibility"]["publish_visibility"] = serde_yaml::Value::from("public");
    std::fs::write(&package_path, serde_yaml::to_string(&package_yaml).unwrap()).unwrap();

    let sources_path = bundle.join("sources/index.yaml");
    let mut sources_yaml: serde_yaml::Value =
        serde_yaml::from_str(&std::fs::read_to_string(&sources_path).unwrap()).unwrap();
    let sources = sources_yaml.as_sequence_mut().unwrap();
    sources[0]["export_policy"] = serde_yaml::Value::from("PrivateOnly");
    sources[0]["permission_status"] =
        serde_yaml::from_str("!Blocked license forbids derived export").unwrap();
    sources[0]["secret_scan_status"] = serde_yaml::from_str(
        "!Findings
- token redacted",
    )
    .unwrap();
    std::fs::write(&sources_path, serde_yaml::to_string(&sources_yaml).unwrap()).unwrap();

    let validate = Command::new(env!("CARGO_BIN_EXE_skiller"))
        .args(["validate", bundle.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!validate.status.success());
    let validate_stdout = String::from_utf8_lossy(&validate.stdout);
    assert!(
        validate_stdout.contains("permission blocked"),
        "stdout was: {validate_stdout}"
    );
    assert!(
        validate_stdout.contains("unresolved secret-scan findings"),
        "stdout was: {validate_stdout}"
    );

    let readiness = Command::new(env!("CARGO_BIN_EXE_skiller"))
        .args(["readiness", bundle.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(readiness.status.success());
    let readiness_stdout = String::from_utf8_lossy(&readiness.stdout);
    assert!(
        readiness_stdout.contains("ready: false"),
        "stdout was: {readiness_stdout}"
    );
    assert!(
        readiness_stdout.contains("export policy does not allow public publication"),
        "stdout was: {readiness_stdout}"
    );
}

#[test]
fn telemetry_improvement_proposals_are_deterministic_risk_specific_and_indexed() {
    let temp = tempdir().unwrap();
    let docs = temp.path().join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("ops.md"),
        "# Restart Service\n\nUse status before mutation.\n\n```\nsvcctl status demo\nsvcctl restart demo\n```\n\nWarning: restart requires approval and rollback plan.\n",
    )
    .unwrap();

    let bundle = temp.path().join("bundle");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "compile",
                docs.to_str().unwrap(),
                "--out",
                bundle.to_str().unwrap(),
                "--name",
                "telemetry-proposals",
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
    let mut skill_yaml: serde_yaml::Value =
        serde_yaml::from_str(&std::fs::read_to_string(&skill_path).unwrap()).unwrap();
    skill_yaml["confidence"]["routing"] = serde_yaml::Value::from(0.2);
    skill_yaml["confidence"]["runtime"] = serde_yaml::Value::from(0.0);
    skill_yaml["confidence"]["human_review"] = serde_yaml::Value::from(0.0);
    skill_yaml["evidence_breakdown"]["direct_extraction"] = serde_yaml::Value::from(0.70);
    skill_yaml["evidence_breakdown"]["speculative_candidate"] = serde_yaml::Value::from(0.30);
    skill_yaml["runtime_policy"]["modify_files"] = serde_yaml::Value::from(true);
    skill_yaml["runtime_policy"]["requires_user_approval"] = serde_yaml::Value::from(true);
    skill_yaml["tool_requirements"] = serde_yaml::from_str(
        r#"
- name: svcctl
  requirement_type: Mutating
  permission_level: FileMutation
  dry_run_available: false
  rollback_required: true
"#,
    )
    .unwrap();
    skill_yaml["evals"] = serde_yaml::Value::Sequence(vec![]);
    std::fs::write(&skill_path, serde_yaml::to_string(&skill_yaml).unwrap()).unwrap();

    let out = temp.path().join("improvements");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_skiller"))
            .args([
                "improve-from-telemetry",
                bundle.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
            ])
            .status()
            .unwrap()
            .success()
    );

    let index = std::fs::read_to_string(out.join("index.yaml")).unwrap();
    assert!(index.contains("low-routing-confidence"));
    assert!(index.contains("missing-evals"));
    assert!(index.contains("speculative-evidence"));
    assert!(index.contains("operational-review-required"));

    let low_routing = std::fs::read_to_string(
        out.join(
            index
                .lines()
                .find(|line| line.contains("low-routing-confidence"))
                .unwrap()
                .trim_start_matches("- "),
        ),
    )
    .unwrap();
    assert!(low_routing.contains("static-analysis:routing-confidence"));
    assert!(low_routing.contains("requires_review: true"));

    let operational = std::fs::read_to_string(
        out.join(
            index
                .lines()
                .find(|line| line.contains("operational-review-required"))
                .unwrap()
                .trim_start_matches("- "),
        ),
    )
    .unwrap();
    assert!(operational.contains("risk: High"));
    assert!(operational.contains("approval/rollback guardrails"));

    let mut proposal_files: Vec<_> = std::fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with("proposal-"))
        .collect();
    proposal_files.sort();
    assert!(
        proposal_files
            .iter()
            .all(|name| !name.contains("proposal-") || !name.contains("00000000"))
    );
    assert_eq!(proposal_files.len(), 4);
}
