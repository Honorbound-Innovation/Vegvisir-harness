use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::serialization::{
    b64url_decode_no_padding, b64url_no_padding, canonical_json_bytes, utc_millis,
};

#[derive(Debug, Error)]
pub enum ReleaseError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("canonical json error: {0}")]
    CanonicalJson(#[from] crate::serialization::SerializationError),
    #[error("pkcs8 error: {0}")]
    Pkcs8(#[from] pkcs8::Error),
    #[error("spki error: {0}")]
    Spki(#[from] pkcs8::spki::Error),
    #[error("signature error: {0}")]
    Signature(#[from] ed25519_dalek::SignatureError),
    #[error("base64 error: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactEntry {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub schema: String,
    pub project: String,
    pub version: String,
    pub created_at: String,
    pub source_digest: Option<String>,
    pub public_key_sha256: String,
    pub artifacts: Vec<ArtifactEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseSignature {
    pub mode: String,
    pub manifest: String,
    pub manifest_sha256: String,
    pub public_key_sha256: String,
    pub signature: String,
    pub signed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseCheck {
    pub name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseVerification {
    pub passed: bool,
    pub checks: Vec<ReleaseCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseKeygenResult {
    pub private_key_path: String,
    pub public_key_path: String,
    pub public_key_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseEvidence {
    pub output_dir: String,
    pub source_digest: String,
    pub signature_mode: String,
}

pub fn generate_release_evidence(
    output_dir: impl AsRef<Path>,
    project_root: impl AsRef<Path>,
    version: &str,
) -> Result<ReleaseEvidence, ReleaseError> {
    let output = output_dir.as_ref();
    let root = project_root.as_ref();
    fs::create_dir_all(output)?;
    let source_digest = source_digest(root)?;
    let components = cargo_lock_components(&root.join("Cargo.lock"))
        .or_else(|_| cargo_lock_components(&root.join("rust/Cargo.lock")))
        .unwrap_or_default();
    let sbom = serde_json::json!({
        "bomFormat": "CycloneDX-lite",
        "specVersion": "1.0-local",
        "metadata": {"component": {"name": "hbse", "version": version}},
        "components": components,
    });
    write_pretty_json(output.join("sbom.json"), &sbom)?;
    let provenance = serde_json::json!({
        "project": "hbse",
        "version": version,
        "created_at": utc_millis(Utc::now()),
        "rustc": rustc_version(),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "source_digest": source_digest,
    });
    write_pretty_json(output.join("provenance.json"), &provenance)?;
    let checklist = serde_json::json!({
        "crypto_tests": "run-required",
        "policy_tests": "run-required",
        "ticket_tests": "run-required",
        "audit_tests": "run-required",
        "redaction_tests": "run-required",
        "backup_restore_tests": "run-required",
        "external_security_review": "required-for-A4-plus",
        "real_hardware_provider_matrix": "required-for-A4-plus",
    });
    write_pretty_json(output.join("production_checklist.json"), &checklist)?;
    let signature = serde_json::json!({
        "mode": "unsigned-development-evidence",
        "source_digest": source_digest,
        "warning": "Run hbse release sign with an Ed25519 release key for production.",
    });
    write_pretty_json(output.join("artifact.sig"), &signature)?;
    let lock = serde_json::json!({
        "project": "hbse",
        "version": version,
        "source_digest": source_digest,
        "components": components,
    });
    write_pretty_json(output.join("dependency-lock.json"), &lock)?;
    write_openapi_evidence(root, output)?;
    write_proto_evidence(root, output)?;
    Ok(ReleaseEvidence {
        output_dir: output.display().to_string(),
        source_digest,
        signature_mode: "unsigned-development-evidence".to_string(),
    })
}

pub fn generate_signing_keypair(
    private_key_path: impl AsRef<Path>,
    public_key_path: impl AsRef<Path>,
    passphrase: Option<&str>,
) -> Result<ReleaseKeygenResult, ReleaseError> {
    let private_path = private_key_path.as_ref();
    let public_path = public_key_path.as_ref();
    let seed: [u8; 32] = rand::random();
    let signing_key = SigningKey::from_bytes(&seed);
    let private_pem = match passphrase {
        Some(passphrase) => {
            signing_key.to_pkcs8_encrypted_pem(&mut OsRng, passphrase.as_bytes(), LineEnding::LF)?
        }
        None => signing_key.to_pkcs8_pem(LineEnding::LF)?,
    };
    let public_pem = signing_key
        .verifying_key()
        .to_public_key_pem(LineEnding::LF)?;
    write_private_file(private_path, private_pem.as_bytes())?;
    if let Some(parent) = public_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(public_path, public_pem.as_bytes())?;
    Ok(ReleaseKeygenResult {
        private_key_path: private_path.display().to_string(),
        public_key_path: public_path.display().to_string(),
        public_key_sha256: hex_sha256(public_pem.as_bytes()),
    })
}

pub fn sign_release_artifacts(
    release_dir: impl AsRef<Path>,
    artifact_paths: &[PathBuf],
    private_key_path: impl AsRef<Path>,
    public_key_path: Option<impl AsRef<Path>>,
    key_passphrase: Option<&str>,
    version: &str,
) -> Result<(ArtifactManifest, ReleaseSignature), ReleaseError> {
    let release_path = release_dir.as_ref();
    fs::create_dir_all(release_path)?;
    let private_pem = fs::read_to_string(private_key_path)?;
    let signing_key = match key_passphrase {
        Some(passphrase) => SigningKey::from_pkcs8_encrypted_pem(&private_pem, passphrase)?,
        None => SigningKey::from_pkcs8_pem(&private_pem)?,
    };
    let public_pem = signing_key
        .verifying_key()
        .to_public_key_pem(LineEnding::LF)?;
    let public_path = public_key_path
        .as_ref()
        .map(|path| path.as_ref().to_path_buf())
        .unwrap_or_else(|| release_path.join("signing_public_key.pem"));
    if let Some(parent) = public_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&public_path, public_pem.as_bytes())?;

    let manifest = artifact_manifest(
        release_path,
        artifact_paths,
        version,
        &hex_sha256(public_pem.as_bytes()),
    )?;
    let manifest_path = release_path.join("artifacts.json");
    let manifest_bytes = canonical_json_bytes(&manifest)?;
    fs::write(&manifest_path, &manifest_bytes)?;
    let signature_bytes = signing_key.sign(&manifest_bytes).to_bytes();
    let signature = ReleaseSignature {
        mode: "ed25519".to_string(),
        manifest: "artifacts.json".to_string(),
        manifest_sha256: hex_sha256(&manifest_bytes),
        public_key_sha256: hex_sha256(public_pem.as_bytes()),
        signature: b64url_no_padding(&signature_bytes),
        signed_at: utc_millis(Utc::now()),
    };
    fs::write(
        release_path.join("artifact.sig"),
        serde_json::to_string_pretty(&signature)?,
    )?;
    Ok((manifest, signature))
}

pub fn verify_release_evidence(
    release_dir: impl AsRef<Path>,
    public_key_path: Option<impl AsRef<Path>>,
) -> ReleaseVerification {
    let root = release_dir.as_ref();
    let mut checks = Vec::new();
    for name in [
        "sbom.json",
        "provenance.json",
        "production_checklist.json",
        "artifact.sig",
        "dependency-lock.json",
        "openapi.json",
        "proto/hbse/v1/hbse.proto",
        "artifacts.json",
    ] {
        let path = root.join(name);
        let status = if name == "artifacts.json" && !path.exists() {
            "warn"
        } else if path.exists() {
            "pass"
        } else {
            "fail"
        };
        checks.push(check(
            name,
            status,
            if path.exists() { "exists" } else { "missing" },
        ));
    }
    for name in [
        "sbom.json",
        "provenance.json",
        "production_checklist.json",
        "artifact.sig",
        "dependency-lock.json",
        "openapi.json",
        "artifacts.json",
    ] {
        let path = root.join(name);
        if !path.exists() {
            continue;
        }
        match fs::read_to_string(&path).and_then(|raw| {
            serde_json::from_str::<Value>(&raw)
                .map(|_| ())
                .map_err(std::io::Error::other)
        }) {
            Ok(()) => checks.push(check(format!("{name}:json"), "pass", "valid JSON")),
            Err(err) => checks.push(check(format!("{name}:json"), "fail", err.to_string())),
        }
    }
    let signature_path = root.join("artifact.sig");
    if signature_path.exists() {
        match read_signature(&signature_path) {
            Ok(signature) if signature.mode == "unsigned-development-evidence" => checks.push(
                check("artifact.sig:mode", "warn", "unsigned-development-evidence"),
            ),
            Ok(signature) if signature.mode == "ed25519" => {
                checks.push(check("artifact.sig:mode", "pass", "ed25519"));
                checks.extend(verify_ed25519_signature(root, &signature, public_key_path));
            }
            Ok(signature) => checks.push(check(
                "artifact.sig:mode",
                "fail",
                format!("unsupported mode: {}", signature.mode),
            )),
            Err(err) => checks.push(check("artifact.sig:parse", "fail", err.to_string())),
        }
    }
    ReleaseVerification {
        passed: checks.iter().all(|item| item.status != "fail"),
        checks,
    }
}

fn artifact_manifest(
    release_dir: &Path,
    artifact_paths: &[PathBuf],
    version: &str,
    public_key_sha256: &str,
) -> Result<ArtifactManifest, ReleaseError> {
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    let mut paths = vec![
        release_dir.join("sbom.json"),
        release_dir.join("dependency-lock.json"),
        release_dir.join("provenance.json"),
        release_dir.join("production_checklist.json"),
        release_dir.join("openapi.json"),
        release_dir.join("proto/hbse/v1/hbse.proto"),
    ];
    paths.extend(artifact_paths.iter().cloned());
    for path in paths {
        if !path.is_file() {
            return Err(ReleaseError::Message(format!("{} missing", path.display())));
        }
        let resolved = path.canonicalize()?;
        if seen.insert(resolved) {
            entries.push(artifact_entry(&path)?);
        }
    }
    let source_digest = fs::read_to_string(release_dir.join("provenance.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| {
            value
                .get("source_digest")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    Ok(ArtifactManifest {
        schema: "hbse.release.artifacts.v1".to_string(),
        project: "hbse".to_string(),
        version: version.to_string(),
        created_at: utc_millis(Utc::now()),
        source_digest,
        public_key_sha256: public_key_sha256.to_string(),
        artifacts: entries,
    })
}

fn artifact_entry(path: &Path) -> Result<ArtifactEntry, ReleaseError> {
    let data = fs::read(path)?;
    Ok(ArtifactEntry {
        path: path.to_string_lossy().replace('\\', "/"),
        sha256: hex_sha256(&data),
        size: data.len() as u64,
    })
}

fn verify_ed25519_signature(
    release_dir: &Path,
    signature: &ReleaseSignature,
    public_key_path: Option<impl AsRef<Path>>,
) -> Vec<ReleaseCheck> {
    let mut checks = Vec::new();
    let manifest_path = release_dir.join(&signature.manifest);
    let manifest_bytes = match fs::read(&manifest_path) {
        Ok(data) => data,
        Err(_) => {
            return vec![check(
                "artifacts.json:signature",
                "fail",
                "manifest missing",
            )]
        }
    };
    let actual_manifest_sha = hex_sha256(&manifest_bytes);
    checks.push(check(
        "artifacts.json:sha256",
        if actual_manifest_sha == signature.manifest_sha256 {
            "pass"
        } else {
            "fail"
        },
        actual_manifest_sha,
    ));
    let manifest = match serde_json::from_slice::<ArtifactManifest>(&manifest_bytes) {
        Ok(value) => value,
        Err(err) => {
            checks.push(check("artifacts.json:parse", "fail", err.to_string()));
            return checks;
        }
    };
    checks.extend(verify_manifest_artifact_hashes(&manifest));
    let public_path = public_key_path
        .as_ref()
        .map(|path| path.as_ref().to_path_buf())
        .unwrap_or_else(|| release_dir.join("signing_public_key.pem"));
    let public_pem = match fs::read_to_string(&public_path) {
        Ok(value) => value,
        Err(_) => {
            checks.push(check(
                "artifact.sig:public_key",
                "fail",
                format!("{} missing", public_path.display()),
            ));
            return checks;
        }
    };
    let public_sha = hex_sha256(public_pem.as_bytes());
    checks.push(check(
        "artifact.sig:public_key",
        if public_sha == signature.public_key_sha256 {
            "pass"
        } else {
            "fail"
        },
        public_sha.clone(),
    ));
    if public_sha != signature.public_key_sha256 {
        return checks;
    }
    let verify_result = (|| -> Result<(), ReleaseError> {
        let verifying_key = VerifyingKey::from_public_key_pem(&public_pem)?;
        let signature_bytes = b64url_decode_no_padding(&signature.signature)?;
        let signature = Signature::try_from(signature_bytes.as_slice())?;
        verifying_key.verify(&canonical_json_bytes(&manifest)?, &signature)?;
        Ok(())
    })();
    checks.push(match verify_result {
        Ok(()) => check("artifact.sig:signature", "pass", "valid Ed25519 signature"),
        Err(err) => check("artifact.sig:signature", "fail", err.to_string()),
    });
    checks
}

fn verify_manifest_artifact_hashes(manifest: &ArtifactManifest) -> Vec<ReleaseCheck> {
    let mut checks = Vec::new();
    for item in &manifest.artifacts {
        let path = PathBuf::from(&item.path);
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(_) => {
                checks.push(check(format!("artifact:{}", item.path), "fail", "missing"));
                continue;
            }
        };
        let actual_sha = hex_sha256(&data);
        let actual_size = data.len() as u64;
        let status = if actual_sha == item.sha256 && actual_size == item.size {
            "pass"
        } else {
            "fail"
        };
        checks.push(check(
            format!("artifact:{}", item.path),
            status,
            format!("sha256={actual_sha} size={actual_size}"),
        ));
    }
    checks
}

fn read_signature(path: &Path) -> Result<ReleaseSignature, ReleaseError> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_pretty_json(path: impl AsRef<Path>, value: &Value) -> Result<(), ReleaseError> {
    fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn cargo_lock_components(path: &Path) -> Result<Vec<Value>, ReleaseError> {
    let raw = fs::read_to_string(path)?;
    let mut components = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_version: Option<String> = None;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            if let (Some(name), Some(version)) = (current_name.take(), current_version.take()) {
                components.push(serde_json::json!({
                    "type": "library",
                    "name": name,
                    "version": version,
                }));
            }
            current_name = None;
            current_version = None;
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("name = ") {
            current_name = Some(value.trim_matches('"').to_string());
        }
        if let Some(value) = trimmed.strip_prefix("version = ") {
            current_version = Some(value.trim_matches('"').to_string());
        }
    }
    if let (Some(name), Some(version)) = (current_name, current_version) {
        components.push(serde_json::json!({
            "type": "library",
            "name": name,
            "version": version,
        }));
    }
    Ok(components)
}

fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn write_openapi_evidence(root: &Path, output: &Path) -> Result<(), ReleaseError> {
    let destination = output.join("openapi.json");
    for source in [root.join("release/openapi.json"), root.join("openapi.json")] {
        if source.is_file() {
            if source.canonicalize().ok() != destination.canonicalize().ok() {
                fs::copy(source, destination)?;
            }
            return Ok(());
        }
    }
    write_pretty_json(
        destination,
        &serde_json::json!({
            "openapi": "3.1.0",
            "info": {
                "title": "HBSE",
                "version": "native-local",
            },
            "paths": {},
            "x-hbse-note": "Native release evidence generated without REST OpenAPI export input.",
        }),
    )
}

fn write_proto_evidence(root: &Path, output: &Path) -> Result<(), ReleaseError> {
    let source = root.join("proto/hbse/v1/hbse.proto");
    let destination = output.join("proto/hbse/v1/hbse.proto");
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    if source.is_file() {
        fs::copy(source, destination)?;
    } else {
        fs::write(destination, "syntax = \"proto3\";\npackage hbse.v1;\n")?;
    }
    Ok(())
}

fn source_digest(root: &Path) -> Result<String, ReleaseError> {
    let mut paths = Vec::new();
    collect_source_paths(root, root, &mut paths)?;
    paths.sort();
    let mut digest = Sha256::new();
    for path in paths {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        digest.update(relative.as_bytes());
        digest.update([0]);
        digest.update(fs::read(&path)?);
        digest.update([0]);
    }
    Ok(b64url_no_padding(&digest.finalize()))
}

fn collect_source_paths(
    root: &Path,
    current: &Path,
    paths: &mut Vec<PathBuf>,
) -> Result<(), ReleaseError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if entry.file_type()?.is_dir() {
            if matches!(
                name.as_ref(),
                ".git"
                    | ".venv"
                    | ".pytest_cache"
                    | "__pycache__"
                    | "build"
                    | "dist"
                    | "release"
                    | "target"
            ) {
                continue;
            }
            collect_source_paths(root, &path, paths)?;
        } else if path.is_file() {
            paths.push(path);
        }
    }
    let _ = root;
    Ok(())
}

fn write_private_file(path: &Path, data: &[u8]) -> Result<(), ReleaseError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(data)?;
    Ok(())
}

fn check(
    name: impl Into<String>,
    status: impl Into<String>,
    detail: impl Into<String>,
) -> ReleaseCheck {
    ReleaseCheck {
        name: name.into(),
        status: status.into(),
        detail: detail.into(),
    }
}

fn hex_sha256(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn ed25519_release_signature_verifies_and_detects_tamper() {
        let dir = tempdir().unwrap();
        let release = dir.path().join("release");
        fs::create_dir_all(release.join("proto/hbse/v1")).unwrap();
        for (name, contents) in [
            ("sbom.json", "{}"),
            ("dependency-lock.json", "{}"),
            ("provenance.json", r#"{"source_digest":"abc"}"#),
            ("production_checklist.json", "{}"),
            ("openapi.json", "{}"),
            ("proto/hbse/v1/hbse.proto", "syntax = \"proto3\";"),
        ] {
            fs::write(release.join(name), contents).unwrap();
        }
        let artifact = dir.path().join("hbse");
        fs::write(&artifact, "binary").unwrap();
        let private_key = dir.path().join("private.pem");
        let public_key = release.join("signing_public_key.pem");
        generate_signing_keypair(&private_key, &public_key, None).unwrap();
        sign_release_artifacts(
            &release,
            &[artifact.clone()],
            &private_key,
            Some(&public_key),
            None,
            "0.1.0",
        )
        .unwrap();
        let verified = verify_release_evidence(&release, Some(&public_key));
        assert!(verified.passed, "{verified:?}");
        fs::write(&artifact, "tampered").unwrap();
        let tampered = verify_release_evidence(&release, Some(&public_key));
        assert!(!tampered.passed);
        assert!(tampered
            .checks
            .iter()
            .any(|check| check.name.starts_with("artifact:") && check.status == "fail"));
    }
}
