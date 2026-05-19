use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use chrono::{DateTime, Utc};
use rand::random;
use serde_json::json;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::keys::{KeyHierarchy, KEY_SIZE};
use crate::records::{SecretRecord, SecretStatus, SecretType};
use crate::serialization::{b64url_no_padding, canonical_json_bytes, utc_millis};

pub const NONCE_SIZE: usize = 12;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("record belongs to a different vault")]
    WrongVault,
    #[error("secret AAD hash mismatch")]
    SecretAadHashMismatch,
    #[error("DEK-wrap AAD hash mismatch")]
    DekWrapAadHashMismatch,
    #[error("authenticated decryption failed")]
    AuthenticationFailed,
    #[error("record decoding failed: {0}")]
    Decode(#[from] base64::DecodeError),
    #[error("invalid nonce length")]
    InvalidNonceLength,
}

#[derive(Debug, Default, Clone)]
pub struct CryptoEngine;

impl CryptoEngine {
    pub fn encrypt_secret(
        &self,
        key_hierarchy: &KeyHierarchy,
        namespace_id: &str,
        secret_id: &str,
        secret_ref: &str,
        version: u64,
        plaintext: &[u8],
        secret_type: SecretType,
        policy_id: &str,
        policy_hash: &str,
        metadata_hash: &str,
        provider_policy_hash: &str,
    ) -> SecretRecord {
        let created_at = utc_millis(Utc::now());
        let dek: [u8; KEY_SIZE] = random();
        let kek = key_hierarchy.secret_kek(secret_id, version);
        let kdf_label = key_hierarchy.secret_kek_label(secret_id, version);
        let secret_aad = secret_aad(
            &key_hierarchy.vault_id,
            namespace_id,
            secret_id,
            secret_ref,
            version,
            secret_type,
            policy_id,
            policy_hash,
            metadata_hash,
            &created_at,
        );
        let wrap_aad = dek_wrap_aad(
            &key_hierarchy.vault_id,
            secret_id,
            version,
            &kdf_label,
            provider_policy_hash,
            &created_at,
        );
        let secret_nonce: [u8; NONCE_SIZE] = random();
        let dek_nonce: [u8; NONCE_SIZE] = random();
        let ciphertext = encrypt_aes_gcm(&dek, &secret_nonce, plaintext, &secret_aad)
            .expect("generated AES key and nonce are valid");
        let wrapped_dek = encrypt_aes_gcm(&kek, &dek_nonce, &dek, &wrap_aad)
            .expect("generated AES key and nonce are valid");

        SecretRecord {
            schema_version: "1".to_string(),
            vault_id: key_hierarchy.vault_id.clone(),
            namespace_id: namespace_id.to_string(),
            secret_id: secret_id.to_string(),
            secret_ref: secret_ref.to_string(),
            secret_version: version,
            status: SecretStatus::Active,
            secret_type,
            algorithm_id: "AES-256-GCM".to_string(),
            wrap_algorithm_id: "AES-256-GCM".to_string(),
            policy_id: policy_id.to_string(),
            policy_hash: policy_hash.to_string(),
            metadata_hash: metadata_hash.to_string(),
            provider_policy_hash: provider_policy_hash.to_string(),
            created_at,
            secret_nonce: b64url_no_padding(&secret_nonce),
            ciphertext: b64url_no_padding(&ciphertext),
            dek_nonce: b64url_no_padding(&dek_nonce),
            wrapped_dek: b64url_no_padding(&wrapped_dek),
            secret_aad_hash: hash_bytes(&secret_aad),
            dek_wrap_aad_hash: hash_bytes(&wrap_aad),
        }
    }

    pub fn decrypt_secret(
        &self,
        key_hierarchy: &KeyHierarchy,
        record: &SecretRecord,
    ) -> Result<Vec<u8>, CryptoError> {
        if record.vault_id != key_hierarchy.vault_id {
            return Err(CryptoError::WrongVault);
        }
        let kdf_label = key_hierarchy.secret_kek_label(&record.secret_id, record.secret_version);
        let secret_aad = matching_secret_aad(record)?;
        let wrap_aad = matching_dek_wrap_aad(record, &kdf_label)?;
        let kek = key_hierarchy.secret_kek(&record.secret_id, record.secret_version);
        let dek = decrypt_aes_gcm(
            &kek,
            &to_nonce(&record.dek_nonce_bytes()?)?,
            &record.wrapped_dek_bytes()?,
            &wrap_aad,
        )?;
        decrypt_aes_gcm(
            to_key(&dek)?,
            &to_nonce(&record.secret_nonce_bytes()?)?,
            &record.ciphertext_bytes()?,
            &secret_aad,
        )
    }
}

fn matching_secret_aad(record: &SecretRecord) -> Result<Vec<u8>, CryptoError> {
    for created_at in created_at_aad_candidates(&record.created_at) {
        let aad = secret_aad(
            &record.vault_id,
            &record.namespace_id,
            &record.secret_id,
            &record.secret_ref,
            record.secret_version,
            record.secret_type,
            &record.policy_id,
            &record.policy_hash,
            &record.metadata_hash,
            &created_at,
        );
        if hash_bytes(&aad) == record.secret_aad_hash {
            return Ok(aad);
        }
    }
    Err(CryptoError::SecretAadHashMismatch)
}

fn matching_dek_wrap_aad(record: &SecretRecord, kdf_label: &str) -> Result<Vec<u8>, CryptoError> {
    for created_at in created_at_aad_candidates(&record.created_at) {
        let aad = dek_wrap_aad(
            &record.vault_id,
            &record.secret_id,
            record.secret_version,
            kdf_label,
            &record.provider_policy_hash,
            &created_at,
        );
        if hash_bytes(&aad) == record.dek_wrap_aad_hash {
            return Ok(aad);
        }
    }
    Err(CryptoError::DekWrapAadHashMismatch)
}

fn created_at_aad_candidates(created_at: &str) -> Vec<String> {
    let mut candidates = vec![created_at.to_string()];
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(created_at) {
        let millis = utc_millis(timestamp.with_timezone(&Utc));
        if millis != created_at {
            candidates.push(millis);
        }
    }
    candidates
}

pub fn hash_bytes(value: &[u8]) -> String {
    b64url_no_padding(&Sha256::digest(value))
}

pub fn secret_aad(
    vault_id: &str,
    namespace_id: &str,
    secret_id: &str,
    secret_ref: &str,
    version: u64,
    secret_type: SecretType,
    policy_id: &str,
    policy_hash: &str,
    metadata_hash: &str,
    created_at: &str,
) -> Vec<u8> {
    canonical_json_bytes(&json!({
        "aad_version": "1",
        "record_type": "secret",
        "algorithm_id": "AES-256-GCM",
        "schema_version": "1",
        "vault_id": vault_id,
        "namespace_id": namespace_id,
        "secret_id": secret_id,
        "secret_ref": secret_ref,
        "version": version,
        "secret_type": secret_type,
        "policy_id": policy_id,
        "policy_hash": policy_hash,
        "metadata_hash": metadata_hash,
        "created_at": created_at,
    }))
    .expect("secret AAD serializes")
}

pub fn dek_wrap_aad(
    vault_id: &str,
    secret_id: &str,
    version: u64,
    kdf_label: &str,
    provider_policy_hash: &str,
    created_at: &str,
) -> Vec<u8> {
    canonical_json_bytes(&json!({
        "aad_version": "1",
        "wrap_algorithm_id": "AES-256-GCM",
        "vault_id": vault_id,
        "secret_id": secret_id,
        "version": version,
        "kdf_label": kdf_label,
        "provider_policy_hash": provider_policy_hash,
        "created_at": created_at,
    }))
    .expect("DEK wrap AAD serializes")
}

fn encrypt_aes_gcm(
    key: &[u8; KEY_SIZE],
    nonce: &[u8; NONCE_SIZE],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::AuthenticationFailed)?;
    cipher
        .encrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| CryptoError::AuthenticationFailed)
}

fn decrypt_aes_gcm(
    key: &[u8; KEY_SIZE],
    nonce: &[u8; NONCE_SIZE],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::AuthenticationFailed)?;
    cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| CryptoError::AuthenticationFailed)
}

fn to_nonce(value: &[u8]) -> Result<[u8; NONCE_SIZE], CryptoError> {
    value
        .try_into()
        .map_err(|_| CryptoError::InvalidNonceLength)
}

fn to_key(value: &[u8]) -> Result<&[u8; KEY_SIZE], CryptoError> {
    value
        .try_into()
        .map_err(|_| CryptoError::AuthenticationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::KeyHierarchy;

    #[test]
    fn secret_round_trip_uses_wrapped_dek() {
        let engine = CryptoEngine;
        let keys = KeyHierarchy::new("vault-1", &[0x11; 32]).unwrap();
        let record = engine.encrypt_secret(
            &keys,
            "default",
            "openai",
            "secret://default/openai",
            1,
            b"sk-test",
            SecretType::ApiKey,
            "default-deny",
            "policy-hash",
            "metadata-hash",
            "unbound-provider-policy",
        );

        assert_ne!(record.ciphertext, "sk-test");
        assert_eq!(engine.decrypt_secret(&keys, &record).unwrap(), b"sk-test");
    }

    #[test]
    fn tampered_policy_hash_fails_before_decryption() {
        let engine = CryptoEngine;
        let keys = KeyHierarchy::new("vault-1", &[0x11; 32]).unwrap();
        let mut record = engine.encrypt_secret(
            &keys,
            "default",
            "db",
            "secret://default/db",
            1,
            b"password",
            SecretType::Generic,
            "default-deny",
            "policy-hash",
            "metadata-hash",
            "unbound-provider-policy",
        );
        record.policy_hash = "different".to_string();

        assert!(matches!(
            engine.decrypt_secret(&keys, &record),
            Err(CryptoError::SecretAadHashMismatch)
        ));
    }

    #[test]
    fn python_created_microsecond_timestamp_records_decrypt() {
        let engine = CryptoEngine;
        let keys = KeyHierarchy::new("vault-1", &[0x11; 32]).unwrap();
        let mut record = engine.encrypt_secret(
            &keys,
            "default",
            "openai",
            "secret://default/openai",
            1,
            b"sk-test",
            SecretType::ApiKey,
            "default-deny",
            "policy-hash",
            "metadata-hash",
            "unbound-provider-policy",
        );
        let millisecond_created_at = record.created_at.clone();
        record.created_at = millisecond_created_at.replace('Z', "456Z");

        assert_eq!(engine.decrypt_secret(&keys, &record).unwrap(), b"sk-test");
    }

    #[test]
    fn wrong_secret_id_fails_authenticated_unwrap() {
        let engine = CryptoEngine;
        let keys = KeyHierarchy::new("vault-1", &[0x11; 32]).unwrap();
        let mut record = engine.encrypt_secret(
            &keys,
            "default",
            "db",
            "secret://default/db",
            1,
            b"password",
            SecretType::Generic,
            "default-deny",
            "policy-hash",
            "metadata-hash",
            "unbound-provider-policy",
        );
        record.secret_id = "other".to_string();
        record.secret_aad_hash = hash_bytes(&secret_aad(
            &record.vault_id,
            &record.namespace_id,
            "other",
            &record.secret_ref,
            record.secret_version,
            record.secret_type,
            &record.policy_id,
            &record.policy_hash,
            &record.metadata_hash,
            &record.created_at,
        ));
        record.dek_wrap_aad_hash = hash_bytes(&dek_wrap_aad(
            &record.vault_id,
            "other",
            record.secret_version,
            &keys.secret_kek_label("other", record.secret_version),
            &record.provider_policy_hash,
            &record.created_at,
        ));

        assert!(matches!(
            engine.decrypt_secret(&keys, &record),
            Err(CryptoError::AuthenticationFailed)
        ));
    }
}
