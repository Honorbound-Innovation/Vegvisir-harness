use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use rand::random;
use scrypt::{scrypt, Params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::keys::KEY_SIZE;
use crate::serialization::{b64url_decode_no_padding, b64url_no_padding, canonical_json_bytes};

pub const PASSPHRASE_PROVIDER_ID: &str = "passphrase-scrypt-aesgcm";

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("vault root key must be 32 bytes")]
    InvalidRootKeyLength,
    #[error("passphrase must not be empty")]
    EmptyPassphrase,
    #[error("unsupported provider binding")]
    UnsupportedProvider,
    #[error("invalid provider KDF parameters")]
    InvalidKdfParameters,
    #[error("provider binding decode failed: {0}")]
    Decode(#[from] base64::DecodeError),
    #[error("passphrase unlock failed")]
    UnlockFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScryptParams {
    pub n: u32,
    pub r: u32,
    pub p: u32,
}

impl Default for ScryptParams {
    fn default() -> Self {
        Self {
            n: 1 << 14,
            r: 8,
            p: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassphraseProviderBinding {
    pub provider_id: String,
    pub kdf: String,
    pub kdf_params: ScryptParams,
    pub salt: String,
    pub nonce: String,
    pub wrapped_root_key: String,
    pub assurance_level: String,
    pub warning: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassphraseProvider {
    pub params: ScryptParams,
}

impl Default for PassphraseProvider {
    fn default() -> Self {
        Self {
            params: ScryptParams::default(),
        }
    }
}

impl PassphraseProvider {
    pub fn wrap_root_key(
        &self,
        vault_id: &str,
        root_key: &[u8],
        passphrase: &str,
    ) -> Result<PassphraseProviderBinding, ProviderError> {
        if root_key.len() != KEY_SIZE {
            return Err(ProviderError::InvalidRootKeyLength);
        }
        let salt: [u8; 16] = random();
        let nonce: [u8; 12] = random();
        let wrapping_key = self.derive(passphrase, &salt)?;
        let aad = provider_aad(vault_id);
        let cipher =
            Aes256Gcm::new_from_slice(&wrapping_key).map_err(|_| ProviderError::UnlockFailed)?;
        let wrapped_root_key = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: root_key,
                    aad: &aad,
                },
            )
            .map_err(|_| ProviderError::UnlockFailed)?;
        Ok(PassphraseProviderBinding {
            provider_id: PASSPHRASE_PROVIDER_ID.to_string(),
            kdf: "scrypt".to_string(),
            kdf_params: self.params.clone(),
            salt: b64url_no_padding(&salt),
            nonce: b64url_no_padding(&nonce),
            wrapped_root_key: b64url_no_padding(&wrapped_root_key),
            assurance_level: "A1".to_string(),
            warning: "MVP passphrase provider; enroll a hardware provider before production use."
                .to_string(),
        })
    }

    pub fn unwrap_root_key(
        &self,
        vault_id: &str,
        binding: &PassphraseProviderBinding,
        passphrase: &str,
    ) -> Result<[u8; KEY_SIZE], ProviderError> {
        if binding.provider_id != PASSPHRASE_PROVIDER_ID {
            return Err(ProviderError::UnsupportedProvider);
        }
        let provider = PassphraseProvider {
            params: binding.kdf_params.clone(),
        };
        let salt = b64url_decode_no_padding(&binding.salt)?;
        let nonce = b64url_decode_no_padding(&binding.nonce)?;
        let wrapped_root_key = b64url_decode_no_padding(&binding.wrapped_root_key)?;
        let wrapping_key = provider.derive(passphrase, &salt)?;
        let cipher =
            Aes256Gcm::new_from_slice(&wrapping_key).map_err(|_| ProviderError::UnlockFailed)?;
        let root_key = cipher
            .decrypt(
                Nonce::from_slice(to_nonce(&nonce)?),
                Payload {
                    msg: &wrapped_root_key,
                    aad: &provider_aad(vault_id),
                },
            )
            .map_err(|_| ProviderError::UnlockFailed)?;
        root_key.try_into().map_err(|_| ProviderError::UnlockFailed)
    }

    fn derive(&self, passphrase: &str, salt: &[u8]) -> Result<[u8; KEY_SIZE], ProviderError> {
        if passphrase.is_empty() {
            return Err(ProviderError::EmptyPassphrase);
        }
        let log_n = self
            .params
            .n
            .checked_ilog2()
            .ok_or(ProviderError::InvalidKdfParameters)? as u8;
        if 1u32.checked_shl(log_n.into()) != Some(self.params.n) {
            return Err(ProviderError::InvalidKdfParameters);
        }
        let params = Params::new(log_n, self.params.r, self.params.p, KEY_SIZE)
            .map_err(|_| ProviderError::InvalidKdfParameters)?;
        let mut output = [0u8; KEY_SIZE];
        scrypt(passphrase.as_bytes(), salt, &params, &mut output)
            .map_err(|_| ProviderError::InvalidKdfParameters)?;
        Ok(output)
    }
}

pub fn provider_aad(vault_id: &str) -> Vec<u8> {
    canonical_json_bytes(&json!({
        "provider_id": PASSPHRASE_PROVIDER_ID,
        "record_type": "vault-root-wrap",
        "vault_id": vault_id,
    }))
    .expect("provider AAD serializes")
}

fn to_nonce(value: &[u8]) -> Result<&[u8; 12], ProviderError> {
    value.try_into().map_err(|_| ProviderError::UnlockFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passphrase_provider_wraps_and_unwraps_root_key() {
        let provider = PassphraseProvider::default();
        let root_key = [0x42; KEY_SIZE];
        let binding = provider
            .wrap_root_key("vault-1", &root_key, "correct horse battery staple")
            .unwrap();

        assert_ne!(binding.wrapped_root_key.as_bytes(), root_key);
        assert_eq!(binding.provider_id, PASSPHRASE_PROVIDER_ID);
        assert_eq!(
            provider
                .unwrap_root_key("vault-1", &binding, "correct horse battery staple")
                .unwrap(),
            root_key
        );
    }

    #[test]
    fn passphrase_provider_rejects_wrong_passphrase() {
        let provider = PassphraseProvider::default();
        let binding = provider
            .wrap_root_key("vault-1", &[0x42; KEY_SIZE], "right")
            .unwrap();

        assert!(matches!(
            provider.unwrap_root_key("vault-1", &binding, "wrong"),
            Err(ProviderError::UnlockFailed)
        ));
    }
}
