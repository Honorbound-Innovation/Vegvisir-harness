use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::provider::PASSPHRASE_PROVIDER_ID;
use crate::provider_system::{SystemFingerprintProvider, SYSTEM_FINGERPRINT_PROVIDER_ID};
use crate::provider_tpm2::{LinuxTpm2ToolsProvider, TPM2_PROVIDER_ID};
use crate::provider_tpm2_esapi::{LinuxTpm2EsapiProvider, TPM2_ESAPI_PROVIDER_ID};
use crate::provider_yubikey::{YubikeyPivProvider, YUBIKEY_PIV_PROVIDER_ID};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCatalogEntry {
    pub provider_id: String,
    pub name: String,
    pub assurance_level: String,
    pub hardware_backed: bool,
    pub vault_binding_supported: bool,
    pub available: bool,
    pub detail: String,
    pub warning: Option<String>,
    pub status: Value,
}

pub fn local_provider_catalog(tpm_device: &str) -> Vec<ProviderCatalogEntry> {
    let tpm2_status = LinuxTpm2ToolsProvider::new(tpm_device).detect();
    let tpm2_esapi_status = LinuxTpm2EsapiProvider::new(tpm_device).detect();
    let system_status = SystemFingerprintProvider::default().detect();
    let yubikey_status = YubikeyPivProvider::detect();

    vec![
        ProviderCatalogEntry {
            provider_id: PASSPHRASE_PROVIDER_ID.to_string(),
            name: "Passphrase".to_string(),
            assurance_level: "A1".to_string(),
            hardware_backed: false,
            vault_binding_supported: true,
            available: true,
            detail: "passphrase provider is always available".to_string(),
            warning: Some(
                "Security depends on passphrase strength and local vault file protection."
                    .to_string(),
            ),
            status: json!({
                "available": true,
                "requires_secret_input": true,
            }),
        },
        ProviderCatalogEntry {
            provider_id: TPM2_ESAPI_PROVIDER_ID.to_string(),
            name: "Linux TPM2 Direct".to_string(),
            assurance_level: "A2".to_string(),
            hardware_backed: true,
            vault_binding_supported: true,
            available: tpm2_esapi_status.available,
            detail: tpm2_esapi_status.detail.clone(),
            warning: Some("Native TPM2-TSS ESAPI provider.".to_string()),
            status: serde_json::to_value(tpm2_esapi_status).unwrap_or_else(|_| json!({})),
        },
        ProviderCatalogEntry {
            provider_id: TPM2_PROVIDER_ID.to_string(),
            name: "Linux TPM2 Tools".to_string(),
            assurance_level: "A2".to_string(),
            hardware_backed: true,
            vault_binding_supported: true,
            available: tpm2_status.available,
            detail: tpm2_status.detail.clone(),
            warning: Some("Current TPM2 implementation uses the tpm2-tools bridge.".to_string()),
            status: serde_json::to_value(tpm2_status).unwrap_or_else(|_| json!({})),
        },
        ProviderCatalogEntry {
            provider_id: SYSTEM_FINGERPRINT_PROVIDER_ID.to_string(),
            name: "System Fingerprint".to_string(),
            assurance_level: "A1".to_string(),
            hardware_backed: false,
            vault_binding_supported: true,
            available: system_status.available,
            detail: system_status.detail.clone(),
            warning: Some(
                "Binds to local machine identifiers but is not a hardware security boundary."
                    .to_string(),
            ),
            status: serde_json::to_value(system_status).unwrap_or_else(|_| json!({})),
        },
        ProviderCatalogEntry {
            provider_id: YUBIKEY_PIV_PROVIDER_ID.to_string(),
            name: "YubiKey/PIV".to_string(),
            assurance_level: "A2".to_string(),
            hardware_backed: true,
            vault_binding_supported: false,
            available: yubikey_status.available,
            detail: yubikey_status.detail.clone(),
            warning: Some(
                "Readiness detection only; vault root-key wrap/unwrap is not implemented yet."
                    .to_string(),
            ),
            status: serde_json::to_value(yubikey_status).unwrap_or_else(|_| json!({})),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_local_providers() {
        let providers = local_provider_catalog("/dev/definitely-not-a-real-tpm");
        let ids: Vec<&str> = providers
            .iter()
            .map(|provider| provider.provider_id.as_str())
            .collect();
        assert!(ids.contains(&PASSPHRASE_PROVIDER_ID));
        assert!(ids.contains(&TPM2_PROVIDER_ID));
        assert!(ids.contains(&TPM2_ESAPI_PROVIDER_ID));
        assert!(ids.contains(&SYSTEM_FINGERPRINT_PROVIDER_ID));
        assert!(ids.contains(&YUBIKEY_PIV_PROVIDER_ID));
    }

    #[test]
    fn catalog_marks_yubikey_as_detection_only() {
        let providers = local_provider_catalog("/dev/definitely-not-a-real-tpm");
        let yubikey = providers
            .iter()
            .find(|provider| provider.provider_id == YUBIKEY_PIV_PROVIDER_ID)
            .expect("yubikey provider entry");
        assert!(yubikey.hardware_backed);
        assert!(!yubikey.vault_binding_supported);
    }
}
