use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use chrono::Utc;
use rand::random;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::keys::KEY_SIZE;
use crate::provider::{PassphraseProvider, PassphraseProviderBinding, ProviderError};
use crate::serialization::utc_millis;

const MNEMONIC_WORDS: &[&str; 64] = &[
    "anchor", "artist", "atlas", "autumn", "beacon", "binary", "border", "brisk", "canyon",
    "cedar", "cipher", "cobalt", "comet", "coral", "delta", "dune", "ember", "fabric", "falcon",
    "field", "forest", "frost", "garden", "harbor", "hazel", "honor", "index", "ivory", "jacket",
    "juniper", "kernel", "lantern", "legend", "magnet", "meadow", "mirror", "nebula", "nickel",
    "onyx", "orbit", "prairie", "quartz", "raven", "river", "rocket", "saffron", "signal",
    "silver", "summit", "temple", "thunder", "timber", "ultra", "velvet", "violet", "voyage",
    "walnut", "winter", "xenon", "yellow", "zenith", "zephyr", "zinc", "zircon",
];

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPackage {
    #[serde(default = "format_version")]
    pub format_version: u64,
    pub recovery_id: String,
    pub vault_id: String,
    pub created_at: String,
    pub root_binding: serde_json::Value,
    #[serde(default = "warning")]
    pub warning: String,
}

impl RecoveryPackage {
    pub fn read(path: impl AsRef<Path>) -> Result<Self, RecoveryError> {
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), RecoveryError> {
        let path = path.as_ref();
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
        file.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RecoveryManager {
    provider: PassphraseProvider,
}

impl Default for RecoveryManager {
    fn default() -> Self {
        Self {
            provider: PassphraseProvider::default(),
        }
    }
}

impl RecoveryManager {
    pub fn create_package(
        &self,
        vault_id: &str,
        root_key: &[u8; KEY_SIZE],
        recovery_secret: &str,
    ) -> Result<RecoveryPackage, RecoveryError> {
        let recovery_id = Uuid::new_v4().to_string();
        let mut binding = serde_json::to_value(self.provider.wrap_root_key(
            vault_id,
            root_key,
            recovery_secret,
        )?)?;
        if let Some(object) = binding.as_object_mut() {
            object.insert(
                "recovery_id".to_string(),
                serde_json::Value::String(recovery_id.clone()),
            );
        }
        Ok(RecoveryPackage {
            format_version: format_version(),
            recovery_id,
            vault_id: vault_id.to_string(),
            created_at: utc_millis(Utc::now()),
            root_binding: binding,
            warning: warning(),
        })
    }

    pub fn unwrap_root_key(
        &self,
        package: &RecoveryPackage,
        recovery_secret: &str,
    ) -> Result<[u8; KEY_SIZE], RecoveryError> {
        let binding: PassphraseProviderBinding =
            serde_json::from_value(package.root_binding.clone())?;
        Ok(self
            .provider
            .unwrap_root_key(&package.vault_id, &binding, recovery_secret)?)
    }
}

fn format_version() -> u64 {
    1
}

fn warning() -> String {
    "Recovery package can rewrap the vault root key. Protect it separately.".to_string()
}

pub fn generate_mnemonic_phrase() -> String {
    let entropy: [u8; 18] = random();
    let mut words = Vec::with_capacity(24);
    let mut buffer = 0u32;
    let mut bits = 0u8;
    for byte in entropy {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= 6 {
            let index = ((buffer >> (bits - 6)) & 0x3f) as usize;
            words.push(MNEMONIC_WORDS[index]);
            bits -= 6;
        }
    }
    words.join(" ")
}

pub fn normalize_mnemonic_phrase(value: &str) -> String {
    value
        .split_whitespace()
        .map(|word| word.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_package_unwraps_only_with_recovery_secret() {
        let root_key = [9u8; KEY_SIZE];
        let package = RecoveryManager::default()
            .create_package("vault-1", &root_key, "correct recovery secret")
            .unwrap();

        let recovered = RecoveryManager::default()
            .unwrap_root_key(&package, "correct recovery secret")
            .unwrap();
        assert_eq!(recovered, root_key);
        assert!(RecoveryManager::default()
            .unwrap_root_key(&package, "wrong recovery secret")
            .is_err());
    }

    #[test]
    fn mnemonic_recovery_phrase_is_normalized_and_usable() {
        let phrase = generate_mnemonic_phrase();
        assert_eq!(phrase.split_whitespace().count(), 24);
        let normalized = normalize_mnemonic_phrase(&format!("  {}  ", phrase.to_uppercase()));
        assert_eq!(normalized, phrase);

        let root_key = [9u8; KEY_SIZE];
        let package = RecoveryManager::default()
            .create_package("vault-1", &root_key, &phrase)
            .unwrap();
        let recovered = RecoveryManager::default()
            .unwrap_root_key(&package, &normalized)
            .unwrap();
        assert_eq!(recovered, root_key);
    }
}
