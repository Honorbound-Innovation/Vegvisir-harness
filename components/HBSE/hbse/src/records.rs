use serde::{Deserialize, Serialize};

use crate::serialization::b64url_decode_no_padding;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretStatus {
    Active,
    Staged,
    Disabled,
    Destroyed,
}

impl Default for SecretStatus {
    fn default() -> Self {
        Self::Active
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretType {
    ApiKey,
    AccessToken,
    RefreshToken,
    Password,
    Passphrase,
    Token,
    MnemonicPhrase,
    SshKey,
    PrivateKey,
    Certificate,
    Credential,
    JsonCredential,
    Generic,
}

impl Default for SecretType {
    fn default() -> Self {
        Self::Generic
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRecord {
    #[serde(default = "schema_version")]
    pub schema_version: String,
    pub vault_id: String,
    pub namespace_id: String,
    pub secret_id: String,
    pub secret_ref: String,
    pub secret_version: u64,
    #[serde(default)]
    pub status: SecretStatus,
    #[serde(default)]
    pub secret_type: SecretType,
    #[serde(default = "algorithm_id")]
    pub algorithm_id: String,
    #[serde(default = "algorithm_id")]
    pub wrap_algorithm_id: String,
    #[serde(default = "default_policy_id")]
    pub policy_id: String,
    pub policy_hash: String,
    pub metadata_hash: String,
    #[serde(default = "default_provider_policy_hash")]
    pub provider_policy_hash: String,
    pub created_at: String,
    pub secret_nonce: String,
    pub ciphertext: String,
    pub dek_nonce: String,
    pub wrapped_dek: String,
    pub secret_aad_hash: String,
    pub dek_wrap_aad_hash: String,
}

impl SecretRecord {
    pub fn ciphertext_bytes(&self) -> Result<Vec<u8>, base64::DecodeError> {
        b64url_decode_no_padding(&self.ciphertext)
    }

    pub fn secret_nonce_bytes(&self) -> Result<Vec<u8>, base64::DecodeError> {
        b64url_decode_no_padding(&self.secret_nonce)
    }

    pub fn dek_nonce_bytes(&self) -> Result<Vec<u8>, base64::DecodeError> {
        b64url_decode_no_padding(&self.dek_nonce)
    }

    pub fn wrapped_dek_bytes(&self) -> Result<Vec<u8>, base64::DecodeError> {
        b64url_decode_no_padding(&self.wrapped_dek)
    }
}

fn schema_version() -> String {
    "1".to_string()
}

fn algorithm_id() -> String {
    "AES-256-GCM".to_string()
}

fn default_policy_id() -> String {
    "default-deny".to_string()
}

fn default_provider_policy_hash() -> String {
    "unbound-provider-policy".to_string()
}
