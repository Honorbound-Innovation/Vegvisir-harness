use std::fs;
use std::path::Path;

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use rand::random;
use scrypt::{scrypt, Params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::keys::KEY_SIZE;
use crate::provider::ScryptParams;
use crate::serialization::{b64url_decode_no_padding, b64url_no_padding, canonical_json_bytes};

pub const SYSTEM_FINGERPRINT_PROVIDER_ID: &str = "system-fingerprint-scrypt-aesgcm";

#[derive(Debug, Error)]
pub enum SystemFingerprintProviderError {
    #[error("vault root key must be 32 bytes")]
    InvalidRootKeyLength,
    #[error("unsupported system fingerprint provider binding")]
    UnsupportedProvider,
    #[error("system fingerprint has no usable stable components")]
    NoUsableComponents,
    #[error("system fingerprint does not match this host")]
    FingerprintMismatch,
    #[error("invalid provider KDF parameters")]
    InvalidKdfParameters,
    #[error("provider binding decode failed: {0}")]
    Decode(#[from] base64::DecodeError),
    #[error("system fingerprint unlock failed")]
    UnlockFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemFingerprintComponent {
    pub name: String,
    pub source: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemFingerprintStatus {
    pub available: bool,
    pub component_count: usize,
    pub components: Vec<SystemFingerprintComponent>,
    pub fingerprint_sha256: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemFingerprintProviderBinding {
    pub provider_id: String,
    pub kdf: String,
    pub kdf_params: ScryptParams,
    pub salt: String,
    pub nonce: String,
    pub wrapped_root_key: String,
    pub fingerprint_sha256: String,
    pub components: Vec<SystemFingerprintComponent>,
    pub assurance_level: String,
    pub warning: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemFingerprintProvider {
    pub params: ScryptParams,
}

impl Default for SystemFingerprintProvider {
    fn default() -> Self {
        Self {
            params: ScryptParams::default(),
        }
    }
}

impl SystemFingerprintProvider {
    pub fn detect(&self) -> SystemFingerprintStatus {
        let components = collect_components().ok().flatten().unwrap_or_default();
        let available = !components.is_empty();
        let fingerprint_sha256 = if available {
            fingerprint_sha256(&components)
        } else {
            String::new()
        };
        let detail = if available {
            format!(
                "system fingerprint available from {} stable component(s)",
                components.len()
            )
        } else {
            "no usable stable system fingerprint components found".to_string()
        };
        SystemFingerprintStatus {
            available,
            component_count: components.len(),
            components,
            fingerprint_sha256,
            detail,
        }
    }

    pub fn wrap_root_key(
        &self,
        vault_id: &str,
        root_key: &[u8],
    ) -> Result<SystemFingerprintProviderBinding, SystemFingerprintProviderError> {
        if root_key.len() != KEY_SIZE {
            return Err(SystemFingerprintProviderError::InvalidRootKeyLength);
        }
        let components =
            collect_components()?.ok_or(SystemFingerprintProviderError::NoUsableComponents)?;
        let fingerprint = fingerprint_sha256(&components);
        let salt: [u8; 16] = random();
        let nonce: [u8; 12] = random();
        let wrapping_key = self.derive(&components, &salt)?;
        let aad = provider_aad(vault_id, &fingerprint);
        let cipher = Aes256Gcm::new_from_slice(&wrapping_key)
            .map_err(|_| SystemFingerprintProviderError::UnlockFailed)?;
        let wrapped_root_key = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: root_key,
                    aad: &aad,
                },
            )
            .map_err(|_| SystemFingerprintProviderError::UnlockFailed)?;
        Ok(SystemFingerprintProviderBinding {
            provider_id: SYSTEM_FINGERPRINT_PROVIDER_ID.to_string(),
            kdf: "scrypt".to_string(),
            kdf_params: self.params.clone(),
            salt: b64url_no_padding(&salt),
            nonce: b64url_no_padding(&nonce),
            wrapped_root_key: b64url_no_padding(&wrapped_root_key),
            fingerprint_sha256: fingerprint,
            components,
            assurance_level: "A1".to_string(),
            warning: "System fingerprint provider binds the vault to local machine identifiers but is not hardware-backed and may break after motherboard, OS identity, or VM image changes.".to_string(),
        })
    }

    pub fn unwrap_root_key(
        &self,
        vault_id: &str,
        binding: &SystemFingerprintProviderBinding,
    ) -> Result<[u8; KEY_SIZE], SystemFingerprintProviderError> {
        if binding.provider_id != SYSTEM_FINGERPRINT_PROVIDER_ID {
            return Err(SystemFingerprintProviderError::UnsupportedProvider);
        }
        let components =
            collect_components()?.ok_or(SystemFingerprintProviderError::NoUsableComponents)?;
        let fingerprint = fingerprint_sha256(&components);
        if fingerprint != binding.fingerprint_sha256 {
            return Err(SystemFingerprintProviderError::FingerprintMismatch);
        }
        let provider = SystemFingerprintProvider {
            params: binding.kdf_params.clone(),
        };
        let salt = b64url_decode_no_padding(&binding.salt)?;
        let nonce = b64url_decode_no_padding(&binding.nonce)?;
        let wrapped_root_key = b64url_decode_no_padding(&binding.wrapped_root_key)?;
        let wrapping_key = provider.derive(&components, &salt)?;
        let cipher = Aes256Gcm::new_from_slice(&wrapping_key)
            .map_err(|_| SystemFingerprintProviderError::UnlockFailed)?;
        let root_key = cipher
            .decrypt(
                Nonce::from_slice(to_nonce(&nonce)?),
                Payload {
                    msg: &wrapped_root_key,
                    aad: &provider_aad(vault_id, &binding.fingerprint_sha256),
                },
            )
            .map_err(|_| SystemFingerprintProviderError::UnlockFailed)?;
        root_key
            .try_into()
            .map_err(|_| SystemFingerprintProviderError::UnlockFailed)
    }

    fn derive(
        &self,
        components: &[SystemFingerprintComponent],
        salt: &[u8],
    ) -> Result<[u8; KEY_SIZE], SystemFingerprintProviderError> {
        let fingerprint = canonical_json_bytes(&json!({
            "provider_id": SYSTEM_FINGERPRINT_PROVIDER_ID,
            "components": components,
        }))
        .map_err(|_| SystemFingerprintProviderError::UnlockFailed)?;
        let log_n = self
            .params
            .n
            .checked_ilog2()
            .ok_or(SystemFingerprintProviderError::InvalidKdfParameters)? as u8;
        if 1u32.checked_shl(log_n.into()) != Some(self.params.n) {
            return Err(SystemFingerprintProviderError::InvalidKdfParameters);
        }
        let params = Params::new(log_n, self.params.r, self.params.p, KEY_SIZE)
            .map_err(|_| SystemFingerprintProviderError::InvalidKdfParameters)?;
        let mut output = [0u8; KEY_SIZE];
        scrypt(&fingerprint, salt, &params, &mut output)
            .map_err(|_| SystemFingerprintProviderError::InvalidKdfParameters)?;
        Ok(output)
    }
}

fn collect_components(
) -> Result<Option<Vec<SystemFingerprintComponent>>, SystemFingerprintProviderError> {
    let candidates = [
        ("machine-id", "/etc/machine-id"),
        ("dbus-machine-id", "/var/lib/dbus/machine-id"),
        ("dmi-product-uuid", "/sys/class/dmi/id/product_uuid"),
        ("dmi-board-serial", "/sys/class/dmi/id/board_serial"),
        ("dmi-product-serial", "/sys/class/dmi/id/product_serial"),
    ];
    let mut components = Vec::new();
    for (name, source) in candidates {
        if let Some(value) = read_component(source) {
            let digest = Sha256::digest(value.as_bytes());
            components.push(SystemFingerprintComponent {
                name: name.to_string(),
                source: source.to_string(),
                sha256: b64url_no_padding(&digest),
            });
        }
    }
    components.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.source.cmp(&right.source))
    });
    components.dedup_by(|left, right| left.name == right.name && left.sha256 == right.sha256);
    Ok(if components.is_empty() {
        None
    } else {
        Some(components)
    })
}

fn read_component(path: &str) -> Option<String> {
    let value = fs::read_to_string(Path::new(path)).ok()?;
    let value = value.trim();
    if value.is_empty()
        || value.eq_ignore_ascii_case("none")
        || value.eq_ignore_ascii_case("unknown")
        || value.chars().all(|item| item == '0' || item == '-')
    {
        None
    } else {
        Some(value.to_string())
    }
}

fn fingerprint_sha256(components: &[SystemFingerprintComponent]) -> String {
    let mut components = components.to_vec();
    components.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.source.cmp(&right.source))
    });
    let bytes = canonical_json_bytes(&json!({
        "provider_id": SYSTEM_FINGERPRINT_PROVIDER_ID,
        "components": components,
    }))
    .expect("system fingerprint serializes");
    b64url_no_padding(&Sha256::digest(bytes))
}

fn provider_aad(vault_id: &str, fingerprint_sha256: &str) -> Vec<u8> {
    canonical_json_bytes(&json!({
        "provider_id": SYSTEM_FINGERPRINT_PROVIDER_ID,
        "record_type": "vault-root-wrap",
        "vault_id": vault_id,
        "fingerprint_sha256": fingerprint_sha256,
    }))
    .expect("provider AAD serializes")
}

fn to_nonce(value: &[u8]) -> Result<&[u8; 12], SystemFingerprintProviderError> {
    value
        .try_into()
        .map_err(|_| SystemFingerprintProviderError::UnlockFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_component_rejects_empty_and_placeholder_values() {
        assert_eq!(read_component("/definitely/missing"), None);
    }

    #[test]
    fn fingerprint_is_stable_for_sorted_components() {
        let mut components = vec![
            SystemFingerprintComponent {
                name: "b".to_string(),
                source: "/b".to_string(),
                sha256: "two".to_string(),
            },
            SystemFingerprintComponent {
                name: "a".to_string(),
                source: "/a".to_string(),
                sha256: "one".to_string(),
            },
        ];
        let first = fingerprint_sha256(&components);
        components.swap(0, 1);
        components.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then(left.source.cmp(&right.source))
        });
        assert_eq!(first, fingerprint_sha256(&components));
    }
}
