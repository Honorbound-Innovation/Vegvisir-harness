use msp_core::{
    MspSchemaKind, SignaturePolicy, SkillTrustPolicy, TrustAction, TrustedIssuer,
    validate_json_schema,
};
use msp_registry::LocalRegistry;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, str};

const BUNDLE: &str = "examples/skiller-bundles/sample-rust-bundle";
const ISSUER: &str = "conformance.local";
const PACK_ID: &str = "pack.sample-rust-bundle.v1";
const SKILL_ID: &str = "skill.software_engineering.rust.refactor-module.v1";

#[test]
fn publish_import_skiller_uses_temp_registry_and_generates_loadable_artifacts() {
    let temp = TempDir::new("msp-cli-publish-conformance");
    let registry_root = temp.path().join("registry");

    let first = run_msp(&[
        "--registry",
        registry_root.to_str().expect("registry path utf-8"),
        "publish",
        "import-skiller",
        BUNDLE,
        "--issuer",
        ISSUER,
    ]);
    let first_report = assert_success_report(&registry_root, first);
    assert_publish_report_contract(&registry_root, &first_report, false);
    assert_generated_registry_contract(&registry_root, false, None);

    let duplicate = run_msp(&[
        "--registry",
        registry_root.to_str().expect("registry path utf-8"),
        "publish",
        "import-skiller",
        BUNDLE,
        "--issuer",
        ISSUER,
    ]);
    assert!(!duplicate.status.success(), "duplicate publish should fail");
    assert!(
        duplicate.stdout.is_empty(),
        "duplicate publish stdout should be empty, got: {}",
        String::from_utf8_lossy(&duplicate.stdout)
    );
    let duplicate_stderr = String::from_utf8_lossy(&duplicate.stderr);
    assert!(
        duplicate_stderr.contains("MSP skill publication target already exists"),
        "unexpected duplicate stderr: {duplicate_stderr}"
    );

    let forced = run_msp(&[
        "--registry",
        registry_root.to_str().expect("registry path utf-8"),
        "publish",
        "import-skiller",
        BUNDLE,
        "--issuer",
        ISSUER,
        "--force",
    ]);
    let forced_report = assert_success_report(&registry_root, forced);
    assert_publish_report_contract(&registry_root, &forced_report, false);
    assert_generated_registry_contract(&registry_root, false, None);
}

#[test]
fn publish_import_skiller_can_sign_generated_artifacts() {
    let temp = TempDir::new("msp-cli-publish-signing-conformance");
    let registry_root = temp.path().join("registry");
    let signing_key = temp.path().join("ed25519.seed");
    fs::write(
        &signing_key,
        "ed25519-seed:000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
    )
    .expect("write test signing seed");

    let output = run_msp(&[
        "--registry",
        registry_root.to_str().expect("registry path utf-8"),
        "publish",
        "import-skiller",
        BUNDLE,
        "--issuer",
        ISSUER,
        "--signing-key",
        signing_key.to_str().expect("signing path utf-8"),
    ]);
    let report = assert_success_report(&registry_root, output);
    assert_publish_report_contract(&registry_root, &report, true);
    let public_key_ref = report["public_key_ref"]
        .as_str()
        .expect("signed report public_key_ref")
        .to_string();
    assert_generated_registry_contract(&registry_root, true, Some(&public_key_ref));

    let registry = LocalRegistry::open(&registry_root).expect("generated registry opens");
    let policy = require_signed_policy(public_key_ref);
    let skill_eval = registry
        .evaluate_trust_policy(&policy, SKILL_ID)
        .expect("signed skill policy evaluation");
    assert!(skill_eval.allowed, "{skill_eval:?}");
    let pack_eval = registry
        .evaluate_pack_trust_policy(&policy, PACK_ID)
        .expect("signed pack policy evaluation");
    assert!(pack_eval.allowed, "{pack_eval:?}");
}

#[test]
fn publish_import_skiller_enforces_immutable_versions_by_default() {
    let temp = TempDir::new("msp-cli-publish-immutability-conformance");
    let registry_root = temp.path().join("registry");
    let bundle = temp.path().join("bundle");
    copy_dir_recursive(&workspace_root().join(BUNDLE), &bundle).expect("copy sample bundle");

    let first = run_msp(&[
        "--registry",
        registry_root.to_str().expect("registry path utf-8"),
        "publish",
        "import-skiller",
        bundle.to_str().expect("bundle path utf-8"),
        "--issuer",
        ISSUER,
    ]);
    let first_report = assert_success_report(&registry_root, first);
    assert_publish_report_contract(&registry_root, &first_report, false);

    let skill_yaml = bundle.join("skills/refactor-module.yaml");
    let original = fs::read_to_string(&skill_yaml).expect("read copied skill yaml");
    fs::write(
        &skill_yaml,
        original.replace(
            "Refactor a Rust module safely with incremental checks.",
            "Refactor a Rust module safely with release-governance checks.",
        ),
    )
    .expect("mutate copied bundle");

    let immutable_republish = run_msp(&[
        "--registry",
        registry_root.to_str().expect("registry path utf-8"),
        "publish",
        "import-skiller",
        bundle.to_str().expect("bundle path utf-8"),
        "--issuer",
        ISSUER,
        "--force",
    ]);
    assert!(
        !immutable_republish.status.success(),
        "byte-changing same-version republish should fail without explicit override"
    );
    assert!(
        immutable_republish.stdout.is_empty(),
        "failed republish stdout should be empty, got: {}",
        String::from_utf8_lossy(&immutable_republish.stdout)
    );
    let stderr = String::from_utf8_lossy(&immutable_republish.stderr);
    assert!(
        stderr.contains("published version is immutable by default"),
        "unexpected immutable republish stderr: {stderr}"
    );

    let mutable_republish = run_msp(&[
        "--registry",
        registry_root.to_str().expect("registry path utf-8"),
        "publish",
        "import-skiller",
        bundle.to_str().expect("bundle path utf-8"),
        "--issuer",
        ISSUER,
        "--force",
        "--allow-mutable-version",
    ]);
    let mutable_report = assert_success_report(&registry_root, mutable_republish);
    assert_publish_report_contract(&registry_root, &mutable_report, false);
    let registry = LocalRegistry::open(&registry_root).expect("generated registry opens");
    let loaded = registry.load_skill(SKILL_ID).expect("mutated skill loads");
    assert!(
        loaded.body.contains("release-governance checks"),
        "explicit mutable override should replace generated body"
    );
}

#[test]
fn publish_import_skiller_can_mark_generated_artifacts_deprecated() {
    let temp = TempDir::new("msp-cli-publish-deprecation-conformance");
    let registry_root = temp.path().join("registry");
    let replacement_skill = "skill.software_engineering.rust.refactor-module.v2";
    let replacement_pack = "pack.sample-rust-bundle.v2";
    let sunset_at = "2027-01-01T00:00:00Z";
    let reason = "superseded by v2 release";

    let output = run_msp(&[
        "--registry",
        registry_root.to_str().expect("registry path utf-8"),
        "publish",
        "import-skiller",
        BUNDLE,
        "--issuer",
        ISSUER,
        "--deprecated",
        "--deprecation-reason",
        reason,
        "--replacement-skill",
        replacement_skill,
        "--replacement-pack",
        replacement_pack,
        "--sunset-at",
        sunset_at,
    ]);
    let report = assert_success_report(&registry_root, output);
    assert_publish_report_contract(&registry_root, &report, false);

    let registry = LocalRegistry::open(&registry_root).expect("generated registry opens");
    let skill = registry
        .get_manifest(SKILL_ID)
        .expect("skill manifest loads");
    let skill_deprecation = skill.deprecation.expect("skill deprecation metadata");
    assert!(skill_deprecation.deprecated);
    assert_eq!(skill_deprecation.reason.as_deref(), Some(reason));
    assert_eq!(
        skill_deprecation.replacement.as_deref(),
        Some(replacement_skill)
    );
    assert_eq!(skill_deprecation.sunset_at.as_deref(), Some(sunset_at));

    let pack = registry.load_pack(PACK_ID).expect("pack manifest loads");
    let pack_deprecation = pack.deprecation.expect("pack deprecation metadata");
    assert!(pack_deprecation.deprecated);
    assert_eq!(pack_deprecation.reason.as_deref(), Some(reason));
    assert_eq!(
        pack_deprecation.replacement.as_deref(),
        Some(replacement_pack)
    );
    assert_eq!(pack_deprecation.sunset_at.as_deref(), Some(sunset_at));
    let pack_trust = registry
        .verify_pack_trust(PACK_ID)
        .expect("deprecated pack trust verifies");
    assert!(
        pack_trust.passed,
        "pack trust should cover deprecation metadata: {pack_trust:?}"
    );
}

fn assert_success_report(registry_root: &Path, output: Output) -> Value {
    assert!(
        output.status.success(),
        "msp-cli publish import-skiller failed\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "successful publish should not write stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "publish stdout should be JSON: {error}\nstdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    assert_eq!(report["registry"], registry_root.to_string_lossy().as_ref());
    validate_json_schema(MspSchemaKind::PublicationReport, &report)
        .expect("publish report validates against publication-report schema");
    report
}

fn assert_publish_report_contract(registry_root: &Path, report: &Value, signed: bool) {
    assert_eq!(report["pack_id"], PACK_ID);
    assert_eq!(report["skills_published"], serde_json::json!([SKILL_ID]));
    assert_eq!(report["warnings"], serde_json::json!([]));
    assert_eq!(report["signed"], signed);
    if signed {
        assert!(
            report["public_key_ref"]
                .as_str()
                .is_some_and(|value| value.starts_with("ed25519:")),
            "signed report should include public_key_ref: {report:#}"
        );
        assert!(
            report["public_key_sha256"]
                .as_str()
                .is_some_and(|value| value.starts_with("sha256:")),
            "signed report should include public_key_sha256: {report:#}"
        );
    } else {
        assert!(report.get("public_key_ref").is_none());
        assert!(report.get("public_key_sha256").is_none());
    }

    let files = report["files_written"]
        .as_array()
        .expect("files_written array");
    assert_eq!(
        files.len(),
        4,
        "expected skill body, verify, manifest, pack"
    );
    for file in files {
        let path = PathBuf::from(file.as_str().expect("file path string"));
        assert!(
            path.starts_with(registry_root),
            "published file should stay inside temp registry: {}",
            path.display()
        );
        assert!(
            path.exists(),
            "published file should exist: {}",
            path.display()
        );
    }

    assert!(
        registry_root
            .join("skills")
            .join(SKILL_ID)
            .join("skill.md")
            .exists()
    );
    assert!(
        registry_root
            .join("skills")
            .join(SKILL_ID)
            .join("verify.json")
            .exists()
    );
    assert!(
        registry_root
            .join("skills")
            .join(SKILL_ID)
            .join("skill.manifest.json")
            .exists()
    );
    assert!(
        registry_root
            .join("packs")
            .join(PACK_ID)
            .join("pack.manifest.json")
            .exists()
    );
}

fn assert_generated_registry_contract(
    registry_root: &Path,
    signed: bool,
    public_key_ref: Option<&str>,
) {
    let registry = LocalRegistry::open(registry_root).expect("generated registry opens");
    assert_eq!(registry.skill_count(), 1);
    assert_eq!(registry.pack_count(), 1);

    let manifest = registry
        .get_manifest(SKILL_ID)
        .expect("skill manifest loads");
    assert_eq!(manifest.id, SKILL_ID);
    assert_eq!(manifest.version, "0.1.0");
    assert_eq!(manifest.trust.issuer, ISSUER);
    assert_eq!(manifest.trust.signed, signed);
    if signed {
        let signature = manifest
            .trust
            .signature
            .as_deref()
            .expect("skill signature");
        assert!(signature.starts_with(public_key_ref.expect("public key ref")));
    } else {
        assert!(manifest.trust.signature.is_none());
    }

    let loaded = registry.load_skill(SKILL_ID).expect("skill loads");
    assert!(loaded.body_hash_valid);
    assert!(loaded.verification_contract.is_some());
    assert_eq!(loaded.manifest.id, SKILL_ID);
    assert!(loaded.body.contains("# Refactor Rust Module"));
    assert!(loaded.body.contains("## Procedure"));
    assert!(
        loaded
            .body
            .contains("Make one behavior-preserving change at a time.")
    );

    let trust = registry.verify_trust(SKILL_ID).expect("trust verifies");
    assert!(
        trust.passed,
        "trust should pass for generated body hash/signature: {trust:?}"
    );
    assert_eq!(trust.signature.is_some(), signed);
    if signed {
        assert_eq!(
            trust
                .signature
                .as_ref()
                .and_then(|signature| signature.public_key_ref.as_deref()),
            public_key_ref
        );
    }

    let pack = registry.load_pack(PACK_ID).expect("pack loads");
    assert_eq!(pack.id, PACK_ID);
    assert_eq!(pack.skills.len(), 1);
    assert_eq!(pack.skills[0].id, SKILL_ID);
    assert_eq!(
        pack.skills[0].manifest_uri,
        format!("skills/{SKILL_ID}/skill.manifest.json")
    );
    assert_eq!(pack.trust.issuer, ISSUER);
    assert_eq!(pack.trust.signed, signed);
    if signed {
        let signature = pack.trust.signature.as_deref().expect("pack signature");
        assert!(signature.starts_with(public_key_ref.expect("public key ref")));
    } else {
        assert!(pack.trust.signature.is_none());
    }

    let pack_trust = registry
        .verify_pack_trust(PACK_ID)
        .expect("pack trust verifies");
    assert!(
        pack_trust.passed,
        "pack trust hash/signature should verify: {pack_trust:?}"
    );
    assert_eq!(pack_trust.signature.is_some(), signed);

    let members = registry
        .validate_pack_members(PACK_ID)
        .expect("pack members validate");
    assert!(members.valid, "pack members should validate: {members:?}");
}

fn require_signed_policy(public_key_ref: String) -> SkillTrustPolicy {
    SkillTrustPolicy {
        msp_version: "0.1.0".to_string(),
        policy_version: "0.1.0".to_string(),
        kind: "SkillTrustPolicy".to_string(),
        id: "trust.conformance.signed.v1".to_string(),
        name: "Signed conformance policy".to_string(),
        default_action: TrustAction::Allow,
        trusted_registries: vec![],
        trusted_issuers: vec![TrustedIssuer {
            id: ISSUER.to_string(),
            public_key_ref: Some(public_key_ref),
            allowed_risk_levels: vec![],
        }],
        signature: Some(SignaturePolicy::default()),
        risk: None,
        rules: vec![],
        forbidden_behaviors: vec![],
        dependency_policy: None,
        telemetry_policy: None,
    }
}

fn run_msp(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_msp-cli"))
        .current_dir(workspace_root())
        .args(args)
        .output()
        .expect("run msp-cli")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate is under workspace/crates/msp-cli")
        .to_path_buf()
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{nonce}"));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
