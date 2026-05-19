use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use thiserror::Error;

use crate::keys::KEY_SIZE;
use crate::serialization::{b64url_decode_no_padding, b64url_no_padding};

pub const TPM2_PROVIDER_ID: &str = "linux-tpm2-tools-seal";

#[derive(Debug, Error)]
pub enum Tpm2ProviderError {
    #[error("vault root key must be 32 bytes")]
    InvalidRootKeyLength,
    #[error("unsupported TPM provider binding")]
    UnsupportedProvider,
    #[error("TPM provider unavailable: {0}")]
    Unavailable(String),
    #[error("missing command: {0}")]
    MissingCommand(String),
    #[error("{0} failed: {1}")]
    CommandFailed(String, String),
    #[error("TPM sealed object identity mismatch")]
    IdentityMismatch,
    #[error("TPM returned invalid root key length")]
    InvalidRootKeyLengthFromTpm,
    #[error("provider binding decode failed: {0}")]
    Decode(#[from] base64::DecodeError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tpm2ProviderStatus {
    pub available: bool,
    pub device_path: String,
    pub tools_available: bool,
    pub device_accessible: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tpm2ProviderBinding {
    pub provider_id: String,
    pub vault_id: String,
    pub device_path: String,
    pub parent_hierarchy: String,
    pub public: String,
    pub private: String,
    pub public_info_sha256: String,
    pub assurance_level: String,
    pub warning: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxTpm2ToolsProvider {
    pub device_path: String,
}

impl Default for LinuxTpm2ToolsProvider {
    fn default() -> Self {
        Self {
            device_path: "/dev/tpmrm0".to_string(),
        }
    }
}

impl LinuxTpm2ToolsProvider {
    pub fn new(device_path: impl Into<String>) -> Self {
        Self {
            device_path: device_path.into(),
        }
    }

    pub fn detect(&self) -> Tpm2ProviderStatus {
        let tools_available = [
            "tpm2_createprimary",
            "tpm2_create",
            "tpm2_load",
            "tpm2_unseal",
            "tpm2_readpublic",
        ]
        .into_iter()
        .all(command_exists);
        let device = Path::new(&self.device_path);
        let device_accessible = device.exists() && readable_writable(device);
        let detail = if !device.exists() {
            format!("{} does not exist", self.device_path)
        } else if !device_accessible {
            format!(
                "{} exists but is not readable/writable by this user",
                self.device_path
            )
        } else if !tools_available {
            "required tpm2-tools commands are not installed".to_string()
        } else {
            "TPM device and tpm2-tools are available".to_string()
        };
        Tpm2ProviderStatus {
            available: tools_available && device_accessible,
            device_path: self.device_path.clone(),
            tools_available,
            device_accessible,
            detail,
        }
    }

    pub fn wrap_root_key(
        &self,
        vault_id: &str,
        root_key: &[u8],
    ) -> Result<Tpm2ProviderBinding, Tpm2ProviderError> {
        if root_key.len() != KEY_SIZE {
            return Err(Tpm2ProviderError::InvalidRootKeyLength);
        }
        self.require_available()?;
        let dir = tempdir()?;
        let secret_path = dir.path().join("root.key");
        let primary_ctx = dir.path().join("primary.ctx");
        let sealed_pub = dir.path().join("sealed.pub");
        let sealed_priv = dir.path().join("sealed.priv");
        let sealed_ctx = dir.path().join("sealed.ctx");
        fs::write(&secret_path, root_key)?;
        let result = (|| {
            self.run(
                "tpm2_createprimary",
                &["-C", "o", "-G", "ecc", "-c", path_str(&primary_ctx)?],
                false,
            )?;
            self.run(
                "tpm2_create",
                &[
                    "-C",
                    path_str(&primary_ctx)?,
                    "-i",
                    path_str(&secret_path)?,
                    "-u",
                    path_str(&sealed_pub)?,
                    "-r",
                    path_str(&sealed_priv)?,
                ],
                false,
            )?;
            self.run(
                "tpm2_load",
                &[
                    "-C",
                    path_str(&primary_ctx)?,
                    "-u",
                    path_str(&sealed_pub)?,
                    "-r",
                    path_str(&sealed_priv)?,
                    "-c",
                    path_str(&sealed_ctx)?,
                ],
                false,
            )?;
            self.run("tpm2_readpublic", &["-c", path_str(&sealed_ctx)?], true)
        })();
        let _ = fs::write(&secret_path, vec![0u8; KEY_SIZE]);
        let public_info = result?;
        Ok(Tpm2ProviderBinding {
            provider_id: TPM2_PROVIDER_ID.to_string(),
            vault_id: vault_id.to_string(),
            device_path: self.device_path.clone(),
            parent_hierarchy: "owner".to_string(),
            public: b64url_no_padding(&fs::read(sealed_pub)?),
            private: b64url_no_padding(&fs::read(sealed_priv)?),
            public_info_sha256: b64url_no_padding(&Sha256::digest(public_info)),
            assurance_level: "A2".to_string(),
            warning:
                "tpm2-tools bridge provider; direct TPM bindings are preferred for production."
                    .to_string(),
        })
    }

    pub fn unwrap_root_key(
        &self,
        binding: &Tpm2ProviderBinding,
    ) -> Result<[u8; KEY_SIZE], Tpm2ProviderError> {
        if binding.provider_id != TPM2_PROVIDER_ID {
            return Err(Tpm2ProviderError::UnsupportedProvider);
        }
        self.require_available()?;
        let dir = tempdir()?;
        let primary_ctx = dir.path().join("primary.ctx");
        let sealed_pub = dir.path().join("sealed.pub");
        let sealed_priv = dir.path().join("sealed.priv");
        let sealed_ctx = dir.path().join("sealed.ctx");
        fs::write(&sealed_pub, b64url_decode_no_padding(&binding.public)?)?;
        fs::write(&sealed_priv, b64url_decode_no_padding(&binding.private)?)?;
        self.run(
            "tpm2_createprimary",
            &["-C", "o", "-G", "ecc", "-c", path_str(&primary_ctx)?],
            false,
        )?;
        self.run(
            "tpm2_load",
            &[
                "-C",
                path_str(&primary_ctx)?,
                "-u",
                path_str(&sealed_pub)?,
                "-r",
                path_str(&sealed_priv)?,
                "-c",
                path_str(&sealed_ctx)?,
            ],
            false,
        )?;
        let public_info = self.run("tpm2_readpublic", &["-c", path_str(&sealed_ctx)?], true)?;
        let actual_hash = b64url_no_padding(&Sha256::digest(public_info));
        if !binding.public_info_sha256.is_empty() && binding.public_info_sha256 != actual_hash {
            return Err(Tpm2ProviderError::IdentityMismatch);
        }
        let root_key = self.run("tpm2_unseal", &["-c", path_str(&sealed_ctx)?], true)?;
        root_key
            .try_into()
            .map_err(|_| Tpm2ProviderError::InvalidRootKeyLengthFromTpm)
    }

    pub fn self_test(&self) -> Result<Tpm2ProviderStatus, Tpm2ProviderError> {
        let status = self.detect();
        if !status.available {
            return Ok(status);
        }
        let root_key: [u8; KEY_SIZE] = rand::random();
        let binding = self.wrap_root_key("self-test", &root_key)?;
        let unwrapped = self.unwrap_root_key(&binding)?;
        if unwrapped != root_key {
            return Err(Tpm2ProviderError::CommandFailed(
                "tpm2_unseal".to_string(),
                "TPM seal/unseal self-test mismatch".to_string(),
            ));
        }
        Ok(status)
    }

    fn require_available(&self) -> Result<(), Tpm2ProviderError> {
        let status = self.detect();
        if status.available {
            Ok(())
        } else {
            Err(Tpm2ProviderError::Unavailable(status.detail))
        }
    }

    fn run(
        &self,
        command: &str,
        args: &[&str],
        capture: bool,
    ) -> Result<Vec<u8>, Tpm2ProviderError> {
        let mut process = Command::new(command);
        process
            .args(args)
            .env("TPM2TOOLS_TCTI", format!("device:{}", self.device_path))
            .stderr(Stdio::piped());
        if capture {
            process.stdout(Stdio::piped());
        } else {
            process.stdout(Stdio::null());
        }
        let output = process.output().map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => Tpm2ProviderError::MissingCommand(command.to_string()),
            _ => Tpm2ProviderError::Io(err),
        })?;
        if !output.status.success() {
            return Err(Tpm2ProviderError::CommandFailed(
                command.to_string(),
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(if capture { output.stdout } else { Vec::new() })
    }
}

fn command_exists(command: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|path| {
            let candidate = path.join(command);
            candidate.is_file()
        })
    })
}

fn readable_writable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let mode = metadata.permissions().mode();
    mode & 0o600 != 0
}

fn path_str(path: &Path) -> Result<&str, Tpm2ProviderError> {
    path.to_str()
        .ok_or_else(|| Tpm2ProviderError::Unavailable("temporary path is not UTF-8".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_reports_missing_device() {
        let status = LinuxTpm2ToolsProvider::new("/definitely/not/a/tpm").detect();
        assert!(!status.available);
        assert!(!status.device_accessible);
        assert!(status.detail.contains("does not exist"));
    }
}
