use hmac::{Hmac, Mac};
use sha2::Sha256;
use thiserror::Error;

pub const KEY_SIZE: usize = 32;
pub const PROTOCOL_VERSION: &str = "v1";

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum KeyError {
    #[error("vault root key must be 32 bytes")]
    InvalidRootKeyLength,
    #[error("KDF label is not domain-separated")]
    InvalidLabel,
    #[error("KDF label must include HBSE protocol version")]
    MissingProtocolVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyHierarchy {
    pub vault_id: String,
    root_key: [u8; KEY_SIZE],
}

impl KeyHierarchy {
    pub fn new(vault_id: impl Into<String>, root_key: &[u8]) -> Result<Self, KeyError> {
        let root_key: [u8; KEY_SIZE] = root_key
            .try_into()
            .map_err(|_| KeyError::InvalidRootKeyLength)?;
        Ok(Self {
            vault_id: vault_id.into(),
            root_key,
        })
    }

    pub fn metadata_key(&self) -> [u8; KEY_SIZE] {
        self.derive(&format!("HBSE:v1:vault:{}:metadata", self.vault_id))
            .expect("internal label is valid")
    }

    pub fn audit_integrity_key(&self) -> [u8; KEY_SIZE] {
        self.derive(&format!("HBSE:v1:vault:{}:audit-integrity", self.vault_id))
            .expect("internal label is valid")
    }

    pub fn redaction_fingerprint_key(&self) -> [u8; KEY_SIZE] {
        self.derive(&format!(
            "HBSE:v1:vault:{}:redaction-fingerprint",
            self.vault_id
        ))
        .expect("internal label is valid")
    }

    pub fn ticket_mac_key(&self) -> [u8; KEY_SIZE] {
        self.derive(&format!("HBSE:v1:vault:{}:ticket-mac", self.vault_id))
            .expect("internal label is valid")
    }

    pub fn secret_kek(&self, secret_id: &str, version: u64) -> [u8; KEY_SIZE] {
        self.derive(&self.secret_kek_label(secret_id, version))
            .expect("internal label is valid")
    }

    pub fn secret_kek_label(&self, secret_id: &str, version: u64) -> String {
        format!(
            "HBSE:v1:vault:{}:secret:{}:version:{}:dek-wrap",
            self.vault_id, secret_id, version
        )
    }

    pub fn derive(&self, label: &str) -> Result<[u8; KEY_SIZE], KeyError> {
        counter_kdf_hmac_sha256(&self.root_key, label)
    }

    pub(crate) fn root_key_bytes(&self) -> &[u8; KEY_SIZE] {
        &self.root_key
    }
}

pub fn counter_kdf_hmac_sha256(
    root_key: &[u8; KEY_SIZE],
    label: &str,
) -> Result<[u8; KEY_SIZE], KeyError> {
    if label.is_empty() || matches!(label, "key" | "secret" | "encrypt" | "default") {
        return Err(KeyError::InvalidLabel);
    }
    if !label.contains(&format!("HBSE:{PROTOCOL_VERSION}:")) {
        return Err(KeyError::MissingProtocolVersion);
    }
    let mut result = Vec::with_capacity(KEY_SIZE);
    let mut counter = 1u32;
    while result.len() < KEY_SIZE {
        let mut block = Vec::new();
        block.extend_from_slice(&counter.to_be_bytes());
        block.extend_from_slice(label.as_bytes());
        block.push(0);
        block.extend_from_slice(&((KEY_SIZE * 8) as u32).to_be_bytes());
        let mut mac = HmacSha256::new_from_slice(root_key).expect("HMAC accepts any key");
        mac.update(&block);
        result.extend_from_slice(&mac.finalize().into_bytes());
        counter += 1;
    }
    let mut output = [0u8; KEY_SIZE];
    output.copy_from_slice(&result[..KEY_SIZE]);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn kdf_domain_separates_internal_keys_and_versions() {
        let keys = KeyHierarchy::new("vault-1", &[0x11; 32]).unwrap();
        let derived = HashSet::from([
            keys.metadata_key(),
            keys.audit_integrity_key(),
            keys.redaction_fingerprint_key(),
            keys.ticket_mac_key(),
            keys.secret_kek("api", 1),
            keys.secret_kek("api", 2),
        ]);
        assert_eq!(derived.len(), 6);
    }

    #[test]
    fn rejects_weak_labels() {
        let keys = KeyHierarchy::new("vault-1", &[0x11; 32]).unwrap();
        assert_eq!(keys.derive("key").unwrap_err(), KeyError::InvalidLabel);
        assert_eq!(
            keys.derive("HBSE:v0:vault:v:metadata").unwrap_err(),
            KeyError::MissingProtocolVersion
        );
    }
}
