use chrono::Utc;
use rand::random;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::audit::{verify_audit_chain, AuditEvent, AuditManager, AuditVerificationError};
use crate::crypto::{hash_bytes, CryptoEngine, CryptoError};
use crate::keys::{KeyError, KeyHierarchy, KEY_SIZE};
use crate::mfa::{
    new_totp_config, verify_totp_code, MfaError, TotpConfig, TotpEnrollment, TOTP_SECRET_REF,
};
use crate::policy::{AccessPolicy, AccessRequest, PolicyDecision, PolicyEngine};
use crate::provider::{
    PassphraseProvider, PassphraseProviderBinding, ProviderError, PASSPHRASE_PROVIDER_ID,
};
use crate::provider_system::{
    SystemFingerprintProvider, SystemFingerprintProviderBinding, SystemFingerprintProviderError,
    SYSTEM_FINGERPRINT_PROVIDER_ID,
};
use crate::provider_tpm2::{
    LinuxTpm2ToolsProvider, Tpm2ProviderBinding, Tpm2ProviderError, TPM2_PROVIDER_ID,
};
use crate::provider_tpm2_esapi::{
    LinuxTpm2EsapiProvider, Tpm2EsapiProviderBinding, Tpm2EsapiProviderError,
    TPM2_ESAPI_PROVIDER_ID,
};
use crate::records::{SecretRecord, SecretStatus, SecretType};
use crate::recovery::{RecoveryError, RecoveryManager, RecoveryPackage};
use crate::redaction::redaction_fingerprint;
use crate::rotation::{RotationJob, RotationJobStatus};
use crate::serialization::{b64url_no_padding, utc_millis};
use crate::store::{SQLiteVaultStore, SecretSummary, StoreError, VaultHeader};
use crate::tickets::{policy_hash, SecretAccessTicket, TicketManager, TicketValidationError};

pub const DEFAULT_POLICY_ID: &str = "default-deny";
pub const DEFAULT_POLICY_HASH: &str = "mvp-default-deny-policy";
pub const DEFAULT_PROVIDER_POLICY_HASH: &str = "unbound-provider-policy";
pub const PLAINTEXT_EXPORT_CONFIG_KEY: &str = "config.plaintext_export";

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("{0}")]
    Provider(#[from] ProviderError),
    #[error("{0}")]
    Tpm2Provider(#[from] Tpm2ProviderError),
    #[error("{0}")]
    Tpm2EsapiProvider(#[from] Tpm2EsapiProviderError),
    #[error("{0}")]
    SystemFingerprintProvider(#[from] SystemFingerprintProviderError),
    #[error("{0}")]
    Recovery(#[from] RecoveryError),
    #[error("{0}")]
    Crypto(#[from] CryptoError),
    #[error("{0}")]
    Keys(#[from] KeyError),
    #[error("{0}")]
    Mfa(#[from] MfaError),
    #[error("{0}")]
    Audit(#[from] AuditVerificationError),
    #[error("{0}")]
    Ticket(#[from] TicketValidationError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("passphrase is required for this vault provider")]
    PassphraseRequired,
    #[error("unsupported provider: {0}")]
    UnsupportedProvider(String),
    #[error("secret reference must start with secret://")]
    InvalidSecretRef,
    #[error("secret status is {0}")]
    SecretUnavailable(String),
    #[error("ticket not found")]
    TicketNotFound,
    #[error("policy denied access: {0}")]
    PolicyDenied(String),
    #[error("rotation job not found")]
    RotationJobNotFound,
    #[error("rotation job is not in required state: {0}")]
    InvalidRotationState(String),
    #[error("new passphrase is required for passphrase provider enrollment")]
    NewPassphraseRequired,
    #[error("recovery package belongs to a different vault")]
    RecoveryPackageVaultMismatch,
    #[error("TOTP MFA is not enrolled")]
    MfaNotEnrolled,
    #[error("invalid TOTP MFA code")]
    MfaInvalidCode,
    #[error("plaintext export is disabled by local vault config")]
    PlaintextExportDisabled,
    #[error("plaintext export enablement requires enrolled TOTP MFA unless explicitly overridden")]
    PlaintextExportMfaEnrollmentRequired,
    #[error("plaintext export requires verified TOTP MFA")]
    PlaintextExportMfaRequired,
    #[error("ticket policy is no longer active or compatible")]
    TicketPolicyChanged,
    #[error("ticket is bound to a stale secret version")]
    TicketSecretVersionStale,
}

#[derive(Debug, Clone)]
pub struct LocalVault {
    pub store: SQLiteVaultStore,
    pub crypto: CryptoEngine,
    pub provider: PassphraseProvider,
}

impl LocalVault {
    pub fn new(store: SQLiteVaultStore) -> Self {
        Self {
            store,
            crypto: CryptoEngine,
            provider: PassphraseProvider::default(),
        }
    }

    pub fn init(
        &self,
        passphrase: &str,
        namespace_id: impl Into<String>,
    ) -> Result<VaultHeader, VaultError> {
        self.init_passphrase(passphrase, namespace_id)
    }

    pub fn init_passphrase(
        &self,
        passphrase: &str,
        namespace_id: impl Into<String>,
    ) -> Result<VaultHeader, VaultError> {
        let vault_id = Uuid::new_v4().to_string();
        let namespace_id = namespace_id.into();
        let root_key: [u8; KEY_SIZE] = random();
        let binding = self
            .provider
            .wrap_root_key(&vault_id, &root_key, passphrase)?;
        let header = VaultHeader {
            schema_version: crate::store::SCHEMA_VERSION,
            vault_id: vault_id.clone(),
            namespace_id,
            provider_binding: serde_json::to_value(binding)?,
            created_at: utc_millis(Utc::now()),
        };
        self.store.create_vault(&header)?;
        let keys = KeyHierarchy::new(vault_id, &root_key)?;
        self.append_audit(
            &header,
            &keys,
            "vault.initialized",
            "info",
            "allow",
            map_from_json(json!({
                "provider_id": header.provider_binding.get("provider_id").and_then(|value| value.as_str()),
                "assurance_level": header.provider_binding.get("assurance_level").and_then(|value| value.as_str()),
            })),
        )?;
        Ok(header)
    }

    pub fn init_tpm2(
        &self,
        namespace_id: impl Into<String>,
        device_path: &str,
    ) -> Result<VaultHeader, VaultError> {
        let vault_id = Uuid::new_v4().to_string();
        let namespace_id = namespace_id.into();
        let root_key: [u8; KEY_SIZE] = random();
        let binding =
            LinuxTpm2ToolsProvider::new(device_path).wrap_root_key(&vault_id, &root_key)?;
        let header = VaultHeader {
            schema_version: crate::store::SCHEMA_VERSION,
            vault_id: vault_id.clone(),
            namespace_id,
            provider_binding: serde_json::to_value(binding)?,
            created_at: utc_millis(Utc::now()),
        };
        self.store.create_vault(&header)?;
        let keys = KeyHierarchy::new(vault_id, &root_key)?;
        self.append_audit(
            &header,
            &keys,
            "vault.initialized",
            "info",
            "allow",
            map_from_json(json!({
                "provider_id": header.provider_binding.get("provider_id").and_then(|value| value.as_str()),
                "assurance_level": header.provider_binding.get("assurance_level").and_then(|value| value.as_str()),
            })),
        )?;
        Ok(header)
    }

    pub fn init_tpm2_esapi(
        &self,
        namespace_id: impl Into<String>,
        device_path: &str,
    ) -> Result<VaultHeader, VaultError> {
        let vault_id = Uuid::new_v4().to_string();
        let namespace_id = namespace_id.into();
        let root_key: [u8; KEY_SIZE] = random();
        let binding =
            LinuxTpm2EsapiProvider::new(device_path).wrap_root_key(&vault_id, &root_key)?;
        let header = VaultHeader {
            schema_version: crate::store::SCHEMA_VERSION,
            vault_id: vault_id.clone(),
            namespace_id,
            provider_binding: serde_json::to_value(binding)?,
            created_at: utc_millis(Utc::now()),
        };
        self.store.create_vault(&header)?;
        let keys = KeyHierarchy::new(vault_id, &root_key)?;
        self.append_audit(
            &header,
            &keys,
            "vault.initialized",
            "info",
            "allow",
            map_from_json(json!({
                "provider_id": header.provider_binding.get("provider_id").and_then(|value| value.as_str()),
                "assurance_level": header.provider_binding.get("assurance_level").and_then(|value| value.as_str()),
            })),
        )?;
        Ok(header)
    }

    pub fn init_system_fingerprint(
        &self,
        namespace_id: impl Into<String>,
    ) -> Result<VaultHeader, VaultError> {
        let vault_id = Uuid::new_v4().to_string();
        let namespace_id = namespace_id.into();
        let root_key: [u8; KEY_SIZE] = random();
        let binding = SystemFingerprintProvider::default().wrap_root_key(&vault_id, &root_key)?;
        let header = VaultHeader {
            schema_version: crate::store::SCHEMA_VERSION,
            vault_id: vault_id.clone(),
            namespace_id,
            provider_binding: serde_json::to_value(binding)?,
            created_at: utc_millis(Utc::now()),
        };
        self.store.create_vault(&header)?;
        let keys = KeyHierarchy::new(vault_id, &root_key)?;
        self.append_audit(
            &header,
            &keys,
            "vault.initialized",
            "info",
            "allow",
            map_from_json(json!({
                "provider_id": header.provider_binding.get("provider_id").and_then(|value| value.as_str()),
                "assurance_level": header.provider_binding.get("assurance_level").and_then(|value| value.as_str()),
            })),
        )?;
        Ok(header)
    }

    pub fn status(&self) -> Result<VaultHeader, VaultError> {
        Ok(self.store.load_header()?)
    }

    pub fn plaintext_export_enabled(&self) -> Result<bool, VaultError> {
        Ok(self
            .store
            .get_metadata(PLAINTEXT_EXPORT_CONFIG_KEY)?
            .as_deref()
            == Some("enabled"))
    }

    pub fn set_plaintext_export_enabled(
        &self,
        passphrase: &str,
        enabled: bool,
        allow_without_mfa: bool,
    ) -> Result<(), VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        if enabled && !allow_without_mfa && !self.totp_mfa_enrolled()? {
            return Err(VaultError::PlaintextExportMfaEnrollmentRequired);
        }
        self.store.set_metadata(
            PLAINTEXT_EXPORT_CONFIG_KEY,
            if enabled { "enabled" } else { "disabled" },
        )?;
        self.append_audit(
            &header,
            &keys,
            if enabled {
                "config.plaintext_export_enabled"
            } else {
                "config.plaintext_export_disabled"
            },
            "critical",
            "allow",
            map_from_json(json!({
                "plaintext_export_enabled": enabled,
                "allow_without_mfa": allow_without_mfa,
            })),
        )?;
        Ok(())
    }

    pub fn enforce_plaintext_export_allowed(&self, mfa_verified: bool) -> Result<(), VaultError> {
        if !self.plaintext_export_enabled()? {
            return Err(VaultError::PlaintextExportDisabled);
        }
        if self.totp_mfa_enrolled()? && !mfa_verified {
            return Err(VaultError::PlaintextExportMfaRequired);
        }
        Ok(())
    }

    pub fn rewrap_provider(
        &self,
        current_passphrase: Option<&str>,
        new_provider: &str,
        new_passphrase: Option<&str>,
        tpm_device: &str,
    ) -> Result<VaultHeader, VaultError> {
        let (header, keys) = self.unlock(current_passphrase)?;
        let binding = match new_provider {
            "passphrase" => {
                let new_passphrase = new_passphrase.ok_or(VaultError::NewPassphraseRequired)?;
                serde_json::to_value(self.provider.wrap_root_key(
                    &header.vault_id,
                    keys.root_key_bytes(),
                    new_passphrase,
                )?)?
            }
            "tpm2" => serde_json::to_value(
                LinuxTpm2ToolsProvider::new(tpm_device)
                    .wrap_root_key(&header.vault_id, keys.root_key_bytes())?,
            )?,
            "tpm2-direct" | "tpm2-esapi" => serde_json::to_value(
                LinuxTpm2EsapiProvider::new(tpm_device)
                    .wrap_root_key(&header.vault_id, keys.root_key_bytes())?,
            )?,
            "system-fingerprint" => serde_json::to_value(
                SystemFingerprintProvider::default()
                    .wrap_root_key(&header.vault_id, keys.root_key_bytes())?,
            )?,
            _ => return Err(VaultError::UnsupportedProvider(new_provider.to_string())),
        };
        let updated = VaultHeader {
            provider_binding: binding,
            ..header.clone()
        };
        self.store.update_header(&updated)?;
        self.append_audit(
            &updated,
            &keys,
            "provider.rewrapped",
            "high",
            "allow",
            map_from_json(json!({
                "old_provider_id": header.provider_binding.get("provider_id").and_then(|value| value.as_str()),
                "new_provider_id": updated.provider_binding.get("provider_id").and_then(|value| value.as_str()),
                "assurance_level": updated.provider_binding.get("assurance_level").and_then(|value| value.as_str()),
            })),
        )?;
        Ok(updated)
    }

    pub fn create_recovery_package(
        &self,
        passphrase: Option<&str>,
        recovery_secret: &str,
    ) -> Result<RecoveryPackage, VaultError> {
        let (header, keys) = self.unlock(passphrase)?;
        let package = RecoveryManager::default().create_package(
            &header.vault_id,
            keys.root_key_bytes(),
            recovery_secret,
        )?;
        self.append_audit(
            &header,
            &keys,
            "recovery.package_created",
            "critical",
            "allow",
            map_from_json(json!({
                "recovery_id": package.recovery_id,
            })),
        )?;
        Ok(package)
    }

    pub fn recover_provider_from_package(
        &self,
        package: &RecoveryPackage,
        recovery_secret: &str,
        new_provider: &str,
        new_passphrase: Option<&str>,
        tpm_device: &str,
    ) -> Result<VaultHeader, VaultError> {
        let header = self.store.load_header()?;
        if package.vault_id != header.vault_id {
            return Err(VaultError::RecoveryPackageVaultMismatch);
        }
        let root_key = RecoveryManager::default().unwrap_root_key(package, recovery_secret)?;
        let keys = KeyHierarchy::new(header.vault_id.clone(), &root_key)?;
        let binding = match new_provider {
            "passphrase" => {
                let new_passphrase = new_passphrase.ok_or(VaultError::NewPassphraseRequired)?;
                serde_json::to_value(self.provider.wrap_root_key(
                    &header.vault_id,
                    &root_key,
                    new_passphrase,
                )?)?
            }
            "tpm2" => serde_json::to_value(
                LinuxTpm2ToolsProvider::new(tpm_device)
                    .wrap_root_key(&header.vault_id, &root_key)?,
            )?,
            "tpm2-direct" | "tpm2-esapi" => serde_json::to_value(
                LinuxTpm2EsapiProvider::new(tpm_device)
                    .wrap_root_key(&header.vault_id, &root_key)?,
            )?,
            "system-fingerprint" => serde_json::to_value(
                SystemFingerprintProvider::default().wrap_root_key(&header.vault_id, &root_key)?,
            )?,
            _ => return Err(VaultError::UnsupportedProvider(new_provider.to_string())),
        };
        let updated = VaultHeader {
            provider_binding: binding,
            ..header.clone()
        };
        self.store.update_header(&updated)?;
        self.append_audit(
            &updated,
            &keys,
            "recovery.used",
            "critical",
            "allow",
            map_from_json(json!({
                "recovery_id": package.recovery_id,
                "new_provider_id": updated.provider_binding.get("provider_id").and_then(|value| value.as_str()),
            })),
        )?;
        Ok(updated)
    }

    pub fn put_secret(
        &self,
        secret_ref: &str,
        plaintext: &[u8],
        passphrase: &str,
        secret_type: SecretType,
    ) -> Result<u64, VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        let version = self.store.latest_version(secret_ref)?.unwrap_or(0) + 1;
        let record = self.crypto.encrypt_secret(
            &keys,
            &header.namespace_id,
            &secret_id_from_ref(secret_ref)?,
            secret_ref,
            version,
            plaintext,
            secret_type,
            DEFAULT_POLICY_ID,
            DEFAULT_POLICY_HASH,
            &hash_bytes(secret_ref.as_bytes()),
            DEFAULT_PROVIDER_POLICY_HASH,
        );
        self.store.save_secret_record(&record)?;
        let fingerprint = redaction_fingerprint(&keys.redaction_fingerprint_key(), plaintext);
        self.store
            .save_redaction_fingerprint(secret_ref, version, &fingerprint)?;
        self.append_audit(
            &header,
            &keys,
            "secret.stored",
            "info",
            "allow",
            map_from_json(json!({
                "secret_ref": secret_ref,
                "secret_version": version,
            })),
        )?;
        Ok(version)
    }

    pub fn get_secret(&self, secret_ref: &str, passphrase: &str) -> Result<Vec<u8>, VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        let record = self.store.load_latest_secret(secret_ref)?;
        if record.status != SecretStatus::Active {
            self.append_audit(
                &header,
                &keys,
                "secret.access_denied",
                "high",
                "deny",
                map_from_json(json!({
                    "secret_ref": secret_ref,
                    "secret_version": record.secret_version,
                    "reason": format!("secret status is {}", status_string(record.status)),
                })),
            )?;
            return Err(VaultError::SecretUnavailable(status_string(record.status)));
        }
        let plaintext = self.crypto.decrypt_secret(&keys, &record)?;
        self.append_audit(
            &header,
            &keys,
            "secret.materialized",
            "critical",
            "allow",
            map_from_json(json!({
                "secret_ref": secret_ref,
                "secret_version": record.secret_version,
            })),
        )?;
        Ok(plaintext)
    }

    pub fn list_secrets(&self) -> Result<Vec<SecretSummary>, VaultError> {
        Ok(self.store.list_secrets()?)
    }

    pub fn enroll_totp_mfa(
        &self,
        passphrase: &str,
        issuer: &str,
        account: &str,
    ) -> Result<TotpEnrollment, VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        let config = new_totp_config(issuer, account);
        let version = self.store.latest_version(TOTP_SECRET_REF)?.unwrap_or(0) + 1;
        let plaintext = serde_json::to_vec(&config)?;
        let record = self.crypto.encrypt_secret(
            &keys,
            &header.namespace_id,
            &secret_id_from_ref(TOTP_SECRET_REF)?,
            TOTP_SECRET_REF,
            version,
            &plaintext,
            SecretType::Token,
            DEFAULT_POLICY_ID,
            DEFAULT_POLICY_HASH,
            &hash_bytes(TOTP_SECRET_REF.as_bytes()),
            DEFAULT_PROVIDER_POLICY_HASH,
        );
        self.store.save_secret_record(&record)?;
        self.append_audit(
            &header,
            &keys,
            "mfa.totp_enrolled",
            "high",
            "allow",
            map_from_json(json!({
                "issuer": issuer,
                "account": account,
                "digits": config.digits,
                "period_seconds": config.period_seconds,
            })),
        )?;
        Ok(config.enrollment())
    }

    pub fn totp_mfa_enrolled(&self) -> Result<bool, VaultError> {
        match self.store.load_latest_secret(TOTP_SECRET_REF) {
            Ok(record) => Ok(record.status == SecretStatus::Active),
            Err(StoreError::SecretNotFound(_)) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    pub fn verify_totp_mfa(&self, passphrase: &str, code: &str) -> Result<(), VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        let record = match self.store.load_latest_secret(TOTP_SECRET_REF) {
            Ok(record) => record,
            Err(StoreError::SecretNotFound(_)) => return Err(VaultError::MfaNotEnrolled),
            Err(err) => return Err(err.into()),
        };
        if record.status != SecretStatus::Active {
            return Err(VaultError::MfaNotEnrolled);
        }
        let plaintext = self.crypto.decrypt_secret(&keys, &record)?;
        let config: TotpConfig = serde_json::from_slice(&plaintext)?;
        let now = Utc::now().timestamp().max(0) as u64;
        if verify_totp_code(&config, code, now, 1)? {
            self.append_audit(
                &header,
                &keys,
                "mfa.totp_verified",
                "info",
                "allow",
                map_from_json(json!({
                    "issuer": config.issuer,
                    "account": config.account,
                })),
            )?;
            Ok(())
        } else {
            self.append_audit(
                &header,
                &keys,
                "mfa.totp_denied",
                "high",
                "deny",
                map_from_json(json!({
                    "issuer": config.issuer,
                    "account": config.account,
                    "reason": "invalid code",
                })),
            )?;
            Err(VaultError::MfaInvalidCode)
        }
    }

    pub fn load_latest_secret(&self, secret_ref: &str) -> Result<SecretRecord, VaultError> {
        Ok(self.store.load_latest_secret(secret_ref)?)
    }

    pub fn disable_secret(
        &self,
        secret_ref: &str,
        passphrase: &str,
    ) -> Result<SecretRecord, VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        let record = self
            .store
            .save_updated_secret_status(secret_ref, SecretStatus::Disabled)?;
        self.append_audit(
            &header,
            &keys,
            "secret.disabled",
            "high",
            "allow",
            map_from_json(json!({
                "secret_ref": secret_ref,
                "secret_version": record.secret_version,
            })),
        )?;
        Ok(record)
    }

    pub fn destroy_secret(
        &self,
        secret_ref: &str,
        passphrase: &str,
    ) -> Result<SecretRecord, VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        let record = self
            .store
            .save_updated_secret_status(secret_ref, SecretStatus::Destroyed)?;
        self.append_audit(
            &header,
            &keys,
            "secret.destroyed",
            "critical",
            "allow",
            map_from_json(json!({
                "secret_ref": secret_ref,
                "secret_version": record.secret_version,
            })),
        )?;
        Ok(record)
    }

    pub fn list_audit_events(&self) -> Result<Vec<AuditEvent>, VaultError> {
        Ok(self.store.list_audit_events()?)
    }

    pub fn verify_audit(&self, passphrase: &str) -> Result<(), VaultError> {
        let (_header, keys) = self.unlock(Some(passphrase))?;
        let events = self.store.list_audit_events()?;
        verify_audit_chain(&events, &keys.audit_integrity_key())?;
        Ok(())
    }

    pub fn save_policy(
        &self,
        policy: AccessPolicy,
        passphrase: &str,
    ) -> Result<AccessPolicy, VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        self.store.save_policy(&policy)?;
        self.append_audit(
            &header,
            &keys,
            "policy.saved",
            "high",
            "allow",
            map_from_json(json!({
                "policy_id": policy.policy_id,
            })),
        )?;
        Ok(policy)
    }

    pub fn list_policies(&self) -> Result<Vec<AccessPolicy>, VaultError> {
        Ok(self.store.list_policies()?)
    }

    pub fn evaluate_policy(
        &self,
        request: AccessRequest,
    ) -> Result<(bool, String, Option<String>), VaultError> {
        let policies = self.store.list_policies()?;
        let engine = PolicyEngine::new(policies);
        let result = engine.evaluate(&request);
        let policy_id = result.policy.map(|policy| policy.policy_id.clone());
        Ok((
            result.decision == PolicyDecision::Allow,
            result.reason,
            policy_id,
        ))
    }

    pub fn issue_ticket(
        &self,
        request: AccessRequest,
        passphrase: &str,
    ) -> Result<SecretAccessTicket, VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        let record = self.store.load_latest_secret(&request.secret_ref)?;
        if record.status != SecretStatus::Active {
            return Err(VaultError::SecretUnavailable(status_string(record.status)));
        }
        let policies = self.store.list_policies()?;
        let engine = PolicyEngine::new(policies);
        let result = engine.evaluate(&request);
        let Some(policy) = result.policy else {
            self.append_audit(
                &header,
                &keys,
                "ticket.issue_denied",
                "high",
                "deny",
                map_from_json(json!({
                    "secret_ref": request.secret_ref,
                    "consumer": request.consumer,
                    "purpose": request.purpose,
                    "reason": result.reason,
                })),
            )?;
            return Err(VaultError::PolicyDenied(result.reason));
        };
        if result.decision != PolicyDecision::Allow {
            self.append_audit(
                &header,
                &keys,
                "ticket.issue_denied",
                "high",
                "deny",
                map_from_json(json!({
                    "secret_ref": request.secret_ref,
                    "consumer": request.consumer,
                    "purpose": request.purpose,
                    "policy_id": policy.policy_id,
                    "reason": result.reason,
                })),
            )?;
            return Err(VaultError::PolicyDenied(result.reason));
        }

        let ticket = TicketManager::new(keys.ticket_mac_key()).issue(
            &header.vault_id,
            &record.secret_id,
            record.secret_version,
            &request,
            policy,
            &policy_hash(policy),
            request.broker_session_id.clone(),
        );
        self.store.save_ticket(&ticket)?;
        self.append_audit(
            &header,
            &keys,
            "ticket.issued",
            "info",
            "allow",
            map_from_json(json!({
                "secret_ref": ticket.secret_ref,
                "consumer": ticket.consumer,
                "purpose": ticket.purpose,
                "delivery_mode": delivery_mode_string(ticket.delivery_mode),
                "ticket_id": ticket.ticket_id,
                "policy_id": ticket.policy_id,
            })),
        )?;
        Ok(ticket)
    }

    pub fn validate_ticket(
        &self,
        ticket_id: &str,
        request: AccessRequest,
        passphrase: &str,
    ) -> Result<SecretAccessTicket, VaultError> {
        let (_header, keys) = self.unlock(Some(passphrase))?;
        let ticket = self
            .store
            .load_ticket(ticket_id)?
            .ok_or(VaultError::TicketNotFound)?;
        let manager = TicketManager::new(keys.ticket_mac_key());
        manager.validate(&ticket, &request, Utc::now())?;
        let policy = self.active_policy_for_ticket(&ticket)?;
        manager.validate_policy(&ticket, &policy, &policy_hash(&policy))?;
        self.ensure_ticket_secret_is_current(&ticket)?;
        Ok(ticket)
    }

    pub fn renew_ticket(
        &self,
        ticket_id: &str,
        request: AccessRequest,
        passphrase: &str,
    ) -> Result<SecretAccessTicket, VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        let old_ticket = self.validate_ticket(ticket_id, request.clone(), passphrase)?;
        let policy = self.active_policy_for_ticket(&old_ticket)?;
        let renewed = TicketManager::new(keys.ticket_mac_key()).issue(
            &header.vault_id,
            &old_ticket.secret_id,
            old_ticket.secret_version,
            &request,
            &policy,
            &policy_hash(&policy),
            old_ticket.broker_session_id.clone(),
        );
        let revoked = TicketManager::new(keys.ticket_mac_key()).revoke(&old_ticket)?;
        self.store.save_ticket(&revoked)?;
        self.store.save_ticket(&renewed)?;
        self.append_audit(
            &header,
            &keys,
            "ticket.renewed",
            "info",
            "allow",
            map_from_json(json!({
                "old_ticket_id": old_ticket.ticket_id,
                "ticket_id": renewed.ticket_id,
                "secret_ref": renewed.secret_ref,
                "policy_id": renewed.policy_id,
            })),
        )?;
        Ok(renewed)
    }

    pub fn consume_ticket_for_secret(
        &self,
        ticket_id: &str,
        request: AccessRequest,
        passphrase: &str,
    ) -> Result<Vec<u8>, VaultError> {
        if request.raw_export_requested {
            self.enforce_plaintext_export_allowed(request.mfa_verified)?;
        }
        let (header, keys) = self.unlock(Some(passphrase))?;
        let ticket = self
            .store
            .load_ticket(ticket_id)?
            .ok_or(VaultError::TicketNotFound)?;
        let manager = TicketManager::new(keys.ticket_mac_key());
        let updated = manager.consume(&ticket, &request, Utc::now())?;
        let policy = self.active_policy_for_ticket(&ticket)?;
        manager.validate_policy(&ticket, &policy, &policy_hash(&policy))?;
        self.ensure_ticket_secret_is_current(&ticket)?;
        self.store.save_ticket_if_current(&ticket, &updated)?;
        let record = if ticket.secret_version == 0 {
            self.store.load_latest_secret(&ticket.secret_ref)?
        } else {
            self.store
                .load_secret_version(&ticket.secret_ref, ticket.secret_version)?
        };
        if record.status != SecretStatus::Active {
            self.append_audit(
                &header,
                &keys,
                "secret.access_denied",
                "high",
                "deny",
                map_from_json(json!({
                    "secret_ref": ticket.secret_ref,
                    "ticket_id": ticket.ticket_id,
                    "reason": format!("secret status is {}", status_string(record.status)),
                })),
            )?;
            return Err(VaultError::SecretUnavailable(status_string(record.status)));
        }
        let plaintext = self.crypto.decrypt_secret(&keys, &record)?;
        self.append_audit(
            &header,
            &keys,
            "secret.materialized",
            if request.raw_export_requested {
                "critical"
            } else {
                "info"
            },
            "allow",
            map_from_json(json!({
                "secret_ref": ticket.secret_ref,
                "ticket_id": ticket.ticket_id,
                "consumer": request.consumer,
                "purpose": request.purpose,
                "delivery_mode": delivery_mode_string(request.delivery_mode),
            })),
        )?;
        Ok(plaintext)
    }

    pub fn revoke_ticket(
        &self,
        ticket_id: &str,
        passphrase: &str,
    ) -> Result<SecretAccessTicket, VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        let ticket = self
            .store
            .load_ticket(ticket_id)?
            .ok_or(VaultError::TicketNotFound)?;
        let revoked = TicketManager::new(keys.ticket_mac_key()).revoke(&ticket)?;
        self.store.save_ticket(&revoked)?;
        self.append_audit(
            &header,
            &keys,
            "ticket.revoked",
            "high",
            "allow",
            map_from_json(json!({
                "ticket_id": ticket_id,
                "secret_ref": ticket.secret_ref,
            })),
        )?;
        Ok(revoked)
    }

    pub fn revoke_all_tickets(&self, passphrase: &str, reason: &str) -> Result<usize, VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        let manager = TicketManager::new(keys.ticket_mac_key());
        let mut count = 0;
        for ticket in self.store.list_tickets()? {
            if ticket.revoked {
                continue;
            }
            let revoked = manager.revoke(&ticket)?;
            self.store.save_ticket(&revoked)?;
            count += 1;
        }
        self.append_audit(
            &header,
            &keys,
            "lockdown.tickets_revoked",
            "critical",
            "allow",
            map_from_json(json!({
                "revoked_tickets": count,
                "reason": reason,
            })),
        )?;
        Ok(count)
    }

    pub fn list_tickets(&self) -> Result<Vec<SecretAccessTicket>, VaultError> {
        Ok(self.store.list_tickets()?)
    }

    pub fn load_ticket(&self, ticket_id: &str) -> Result<SecretAccessTicket, VaultError> {
        self.store
            .load_ticket(ticket_id)?
            .ok_or(VaultError::TicketNotFound)
    }

    fn active_policy_for_ticket(
        &self,
        ticket: &SecretAccessTicket,
    ) -> Result<AccessPolicy, VaultError> {
        self.store
            .list_policies()?
            .into_iter()
            .find(|policy| policy.policy_id == ticket.policy_id)
            .ok_or(VaultError::TicketPolicyChanged)
    }

    fn ensure_ticket_secret_is_current(
        &self,
        ticket: &SecretAccessTicket,
    ) -> Result<(), VaultError> {
        if ticket.secret_version == 0 {
            return Ok(());
        }
        let latest = self.store.load_latest_secret(&ticket.secret_ref)?;
        if latest.secret_version == ticket.secret_version && latest.status == SecretStatus::Active {
            Ok(())
        } else {
            Err(VaultError::TicketSecretVersionStale)
        }
    }

    pub fn start_rotation(
        &self,
        secret_ref: &str,
        new_plaintext: &[u8],
        passphrase: &str,
        secret_type: SecretType,
    ) -> Result<RotationJob, VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        let staged_version = self.store.latest_version(secret_ref)?.unwrap_or(0) + 1;
        let mut staged_record = self.crypto.encrypt_secret(
            &keys,
            &header.namespace_id,
            &secret_id_from_ref(secret_ref)?,
            secret_ref,
            staged_version,
            new_plaintext,
            secret_type,
            DEFAULT_POLICY_ID,
            DEFAULT_POLICY_HASH,
            &hash_bytes(secret_ref.as_bytes()),
            DEFAULT_PROVIDER_POLICY_HASH,
        );
        staged_record.status = SecretStatus::Staged;
        let job = RotationJob::create(
            header.vault_id.clone(),
            secret_ref.to_string(),
            staged_version,
            staged_record,
        );
        self.store.save_rotation_job(&job)?;
        self.append_audit(
            &header,
            &keys,
            "rotation.started",
            "info",
            "allow",
            map_from_json(json!({
                "job_id": job.job_id,
                "secret_ref": secret_ref,
                "staged_version": staged_version,
            })),
        )?;
        Ok(job)
    }

    pub fn verify_rotation(
        &self,
        job_id: &str,
        passphrase: &str,
    ) -> Result<RotationJob, VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        let job = self
            .store
            .load_rotation_job(job_id)?
            .ok_or(VaultError::RotationJobNotFound)?;
        if job.status != RotationJobStatus::Staged {
            return Err(VaultError::InvalidRotationState(rotation_state_string(
                job.status,
            )));
        }
        self.crypto.decrypt_secret(&keys, &job.staged_record)?;
        let verified = job.transition(RotationJobStatus::Verified, None);
        self.store.save_rotation_job(&verified)?;
        self.append_audit(
            &header,
            &keys,
            "rotation.verified",
            "info",
            "allow",
            map_from_json(json!({
                "job_id": job_id,
                "secret_ref": verified.secret_ref,
            })),
        )?;
        Ok(verified)
    }

    pub fn promote_rotation(
        &self,
        job_id: &str,
        passphrase: &str,
    ) -> Result<RotationJob, VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        let job = self
            .store
            .load_rotation_job(job_id)?
            .ok_or(VaultError::RotationJobNotFound)?;
        if job.status != RotationJobStatus::Verified {
            return Err(VaultError::InvalidRotationState(rotation_state_string(
                job.status,
            )));
        }
        let plaintext = self.crypto.decrypt_secret(&keys, &job.staged_record)?;
        let mut active_record = job.staged_record.clone();
        active_record.status = SecretStatus::Active;
        self.store.save_secret_record(&active_record)?;
        let fingerprint = redaction_fingerprint(&keys.redaction_fingerprint_key(), &plaintext);
        self.store
            .save_redaction_fingerprint(&job.secret_ref, job.staged_version, &fingerprint)?;
        let promoted = job.transition(RotationJobStatus::Promoted, None);
        self.store.save_rotation_job(&promoted)?;
        self.append_audit(
            &header,
            &keys,
            "rotation.promoted",
            "high",
            "allow",
            map_from_json(json!({
                "job_id": job_id,
                "secret_ref": promoted.secret_ref,
                "version": promoted.staged_version,
            })),
        )?;
        Ok(promoted)
    }

    pub fn rollback_rotation(
        &self,
        job_id: &str,
        passphrase: &str,
    ) -> Result<RotationJob, VaultError> {
        let (header, keys) = self.unlock(Some(passphrase))?;
        let job = self
            .store
            .load_rotation_job(job_id)?
            .ok_or(VaultError::RotationJobNotFound)?;
        if !matches!(
            job.status,
            RotationJobStatus::Staged | RotationJobStatus::Verified
        ) {
            return Err(VaultError::InvalidRotationState(rotation_state_string(
                job.status,
            )));
        }
        let rolled_back = job.transition(RotationJobStatus::RolledBack, None);
        self.store.save_rotation_job(&rolled_back)?;
        self.append_audit(
            &header,
            &keys,
            "rotation.rolled_back",
            "critical",
            "allow",
            map_from_json(json!({
                "job_id": job_id,
                "secret_ref": rolled_back.secret_ref,
            })),
        )?;
        Ok(rolled_back)
    }

    pub fn list_rotation_jobs(&self) -> Result<Vec<RotationJob>, VaultError> {
        Ok(self.store.list_rotation_jobs()?)
    }

    fn unlock(&self, passphrase: Option<&str>) -> Result<(VaultHeader, KeyHierarchy), VaultError> {
        let header = self.store.load_header()?;
        let provider_id = header
            .provider_binding
            .get("provider_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let root_key = if provider_id == TPM2_PROVIDER_ID {
            let binding: Tpm2ProviderBinding =
                serde_json::from_value(header.provider_binding.clone())?;
            LinuxTpm2ToolsProvider::new(
                header
                    .provider_binding
                    .get("device_path")
                    .and_then(|value| value.as_str())
                    .unwrap_or("/dev/tpmrm0"),
            )
            .unwrap_root_key(&binding)?
        } else if provider_id == TPM2_ESAPI_PROVIDER_ID {
            let binding: Tpm2EsapiProviderBinding =
                serde_json::from_value(header.provider_binding.clone())?;
            LinuxTpm2EsapiProvider::new(
                header
                    .provider_binding
                    .get("device_path")
                    .and_then(|value| value.as_str())
                    .unwrap_or("/dev/tpmrm0"),
            )
            .unwrap_root_key(&binding)?
        } else if provider_id == PASSPHRASE_PROVIDER_ID {
            let passphrase = passphrase.ok_or(VaultError::PassphraseRequired)?;
            let binding: PassphraseProviderBinding =
                serde_json::from_value(header.provider_binding.clone())?;
            self.provider
                .unwrap_root_key(&header.vault_id, &binding, passphrase)?
        } else if provider_id == SYSTEM_FINGERPRINT_PROVIDER_ID {
            let binding: SystemFingerprintProviderBinding =
                serde_json::from_value(header.provider_binding.clone())?;
            SystemFingerprintProvider::default().unwrap_root_key(&header.vault_id, &binding)?
        } else {
            return Err(VaultError::UnsupportedProvider(provider_id.to_string()));
        };
        let keys = KeyHierarchy::new(header.vault_id.clone(), &root_key)?;
        Ok((header, keys))
    }

    fn append_audit(
        &self,
        header: &VaultHeader,
        keys: &KeyHierarchy,
        event_type: &str,
        severity: &str,
        decision: &str,
        metadata: Map<String, Value>,
    ) -> Result<AuditEvent, VaultError> {
        let mac_key = keys.audit_integrity_key();
        let vault_id = header.vault_id.clone();
        let namespace_id = header.namespace_id.clone();
        Ok(self.store.append_audit_event(move |events| {
            let mut manager = AuditManager::new(mac_key, events);
            manager.append(
                &vault_id,
                &namespace_id,
                event_type,
                severity,
                decision,
                metadata,
            )
        })?)
    }
}

pub fn status_string(status: SecretStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn map_from_json(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

pub fn delivery_mode_string(delivery_mode: crate::policy::DeliveryMode) -> String {
    serde_json::to_value(delivery_mode)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn rotation_state_string(status: RotationJobStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn secret_id_from_ref(secret_ref: &str) -> Result<String, VaultError> {
    if !secret_ref.starts_with("secret://") {
        return Err(VaultError::InvalidSecretRef);
    }
    let digest = Sha256::digest(secret_ref.as_bytes());
    Ok(b64url_no_padding(&digest[..18]))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::policy::DeliveryMode;

    fn allow_policy() -> AccessPolicy {
        AccessPolicy {
            policy_id: "p1".to_string(),
            secret_refs: vec!["secret://service/api".to_string()],
            allowed_consumers: vec!["cli".to_string()],
            allowed_purposes: vec!["deploy".to_string()],
            allowed_delivery_modes: vec![DeliveryMode::TerminalPrint],
            exportable: true,
            max_ticket_ttl_seconds: 60,
            max_uses: 1,
            ..serde_json::from_value(serde_json::json!({"policy_id":"defaults"})).unwrap()
        }
    }

    fn access_request() -> AccessRequest {
        AccessRequest {
            secret_ref: "secret://service/api".to_string(),
            consumer: "cli".to_string(),
            purpose: "deploy".to_string(),
            delivery_mode: DeliveryMode::TerminalPrint,
            provider_assurance: "A1".to_string(),
            raw_export_requested: true,
            http_host: None,
            http_scheme: None,
            http_method: None,
            http_path: None,
            http_request_body_bytes: None,
            os_uid: None,
            executable_path: None,
            executable_sha256: None,
            mfa_verified: false,
            broker_session_id: None,
            now: Utc::now(),
        }
    }

    #[test]
    fn local_vault_initializes_and_round_trips_secret() {
        let dir = tempdir().unwrap();
        let vault = LocalVault::new(SQLiteVaultStore::new(dir.path().join("vault.db")));
        let header = vault.init("passphrase", "default").unwrap();

        assert_eq!(header.namespace_id, "default");
        assert_eq!(
            header.provider_binding["provider_id"],
            serde_json::json!(PASSPHRASE_PROVIDER_ID)
        );

        let version = vault
            .put_secret(
                "secret://service/api",
                b"sk-test",
                "passphrase",
                SecretType::ApiKey,
            )
            .unwrap();
        assert_eq!(version, 1);
        assert_eq!(
            vault
                .get_secret("secret://service/api", "passphrase")
                .unwrap(),
            b"sk-test"
        );
        assert_eq!(
            vault.list_secrets().unwrap()[0].secret_ref,
            "secret://service/api"
        );
    }

    #[test]
    fn plaintext_export_is_disabled_by_default_and_requires_mfa_enrollment() {
        let dir = tempdir().unwrap();
        let vault = LocalVault::new(SQLiteVaultStore::new(dir.path().join("vault.db")));
        vault.init("passphrase", "default").unwrap();

        assert!(!vault.plaintext_export_enabled().unwrap());
        assert!(matches!(
            vault.enforce_plaintext_export_allowed(false),
            Err(VaultError::PlaintextExportDisabled)
        ));

        assert!(matches!(
            vault.set_plaintext_export_enabled("passphrase", true, false),
            Err(VaultError::PlaintextExportMfaEnrollmentRequired)
        ));
        assert!(!vault.plaintext_export_enabled().unwrap());

        vault
            .enroll_totp_mfa("passphrase", "HBSE", "test@example.local")
            .unwrap();
        vault
            .set_plaintext_export_enabled("passphrase", true, false)
            .unwrap();
        assert!(vault.plaintext_export_enabled().unwrap());
        assert!(matches!(
            vault.enforce_plaintext_export_allowed(false),
            Err(VaultError::PlaintextExportMfaRequired)
        ));
        vault
            .enforce_plaintext_export_allowed(true)
            .expect("verified MFA permits configured plaintext export");
    }

    #[test]
    fn disabled_secret_is_not_materialized() {
        let dir = tempdir().unwrap();
        let vault = LocalVault::new(SQLiteVaultStore::new(dir.path().join("vault.db")));
        vault.init("passphrase", "default").unwrap();
        vault
            .put_secret(
                "secret://service/api",
                b"sk-test",
                "passphrase",
                SecretType::ApiKey,
            )
            .unwrap();

        let record = vault
            .disable_secret("secret://service/api", "passphrase")
            .unwrap();
        assert_eq!(record.status, SecretStatus::Disabled);
        assert!(matches!(
            vault.get_secret("secret://service/api", "passphrase"),
            Err(VaultError::SecretUnavailable(status)) if status == "disabled"
        ));
    }

    #[test]
    fn audit_chain_records_vault_and_secret_events() {
        let dir = tempdir().unwrap();
        let vault = LocalVault::new(SQLiteVaultStore::new(dir.path().join("vault.db")));
        vault.init("passphrase", "default").unwrap();
        vault
            .put_secret(
                "secret://service/api",
                b"sk-test",
                "passphrase",
                SecretType::ApiKey,
            )
            .unwrap();
        vault
            .get_secret("secret://service/api", "passphrase")
            .unwrap();

        let events = vault.list_audit_events().unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.event_type.as_str())
                .collect::<Vec<_>>(),
            vec!["vault.initialized", "secret.stored", "secret.materialized"]
        );
        vault.verify_audit("passphrase").unwrap();
    }

    #[test]
    fn rotation_promotes_only_after_verification() {
        let dir = tempdir().unwrap();
        let vault = LocalVault::new(SQLiteVaultStore::new(dir.path().join("vault.db")));
        vault.init("passphrase", "default").unwrap();
        vault
            .put_secret(
                "secret://service/api",
                b"old-secret",
                "passphrase",
                SecretType::ApiKey,
            )
            .unwrap();
        let job = vault
            .start_rotation(
                "secret://service/api",
                b"new-secret",
                "passphrase",
                SecretType::ApiKey,
            )
            .unwrap();

        assert_eq!(
            vault
                .get_secret("secret://service/api", "passphrase")
                .unwrap(),
            b"old-secret"
        );
        assert!(matches!(
            vault.promote_rotation(&job.job_id, "passphrase"),
            Err(VaultError::InvalidRotationState(status)) if status == "staged"
        ));

        vault.verify_rotation(&job.job_id, "passphrase").unwrap();
        vault.promote_rotation(&job.job_id, "passphrase").unwrap();
        assert_eq!(
            vault
                .get_secret("secret://service/api", "passphrase")
                .unwrap(),
            b"new-secret"
        );
    }

    #[test]
    fn ticket_consume_denies_replay_policy_change_and_stale_secret_version() {
        let dir = tempdir().unwrap();
        let vault = LocalVault::new(SQLiteVaultStore::new(dir.path().join("vault.db")));
        vault.init("passphrase", "default").unwrap();
        vault
            .set_plaintext_export_enabled("passphrase", true, true)
            .unwrap();
        vault
            .put_secret(
                "secret://service/api",
                b"old-secret",
                "passphrase",
                SecretType::ApiKey,
            )
            .unwrap();
        vault.save_policy(allow_policy(), "passphrase").unwrap();
        let request = access_request();
        let ticket = vault
            .issue_ticket(request.clone(), "passphrase")
            .expect("ticket issued");

        assert_eq!(
            vault
                .consume_ticket_for_secret(&ticket.ticket_id, request.clone(), "passphrase")
                .unwrap(),
            b"old-secret"
        );
        assert!(matches!(
            vault.consume_ticket_for_secret(&ticket.ticket_id, request.clone(), "passphrase"),
            Err(VaultError::Ticket(TicketValidationError::ReplayDenied))
        ));

        let mut changed_policy = allow_policy();
        changed_policy.max_uses = 2;
        vault.save_policy(changed_policy, "passphrase").unwrap();
        let policy_changed_ticket = vault
            .issue_ticket(request.clone(), "passphrase")
            .expect("ticket issued");
        let mut changed_again = allow_policy();
        changed_again.max_ticket_ttl_seconds = 120;
        vault.save_policy(changed_again, "passphrase").unwrap();
        assert!(matches!(
            vault.consume_ticket_for_secret(
                &policy_changed_ticket.ticket_id,
                request.clone(),
                "passphrase"
            ),
            Err(VaultError::Ticket(
                TicketValidationError::PolicyContextMismatch
            ))
        ));

        vault.save_policy(allow_policy(), "passphrase").unwrap();
        let stale_ticket = vault
            .issue_ticket(request.clone(), "passphrase")
            .expect("ticket issued");
        vault
            .put_secret(
                "secret://service/api",
                b"new-secret",
                "passphrase",
                SecretType::ApiKey,
            )
            .unwrap();
        assert!(matches!(
            vault.consume_ticket_for_secret(&stale_ticket.ticket_id, request, "passphrase"),
            Err(VaultError::TicketSecretVersionStale)
        ));
    }

    #[test]
    fn ticket_renew_revokes_old_ticket_and_extends_authorization() {
        let dir = tempdir().unwrap();
        let vault = LocalVault::new(SQLiteVaultStore::new(dir.path().join("vault.db")));
        vault.init("passphrase", "default").unwrap();
        vault
            .set_plaintext_export_enabled("passphrase", true, true)
            .unwrap();
        vault
            .put_secret(
                "secret://service/api",
                b"sk-test",
                "passphrase",
                SecretType::ApiKey,
            )
            .unwrap();
        vault.save_policy(allow_policy(), "passphrase").unwrap();
        let request = access_request();
        let ticket = vault.issue_ticket(request.clone(), "passphrase").unwrap();
        let renewed = vault
            .renew_ticket(&ticket.ticket_id, request.clone(), "passphrase")
            .unwrap();

        assert_ne!(ticket.ticket_id, renewed.ticket_id);
        assert!(vault.load_ticket(&ticket.ticket_id).unwrap().revoked);
        assert_eq!(
            vault
                .consume_ticket_for_secret(&renewed.ticket_id, request, "passphrase")
                .unwrap(),
            b"sk-test"
        );
    }

    #[test]
    fn provider_rewrap_changes_passphrase_without_reencrypting_secret() {
        let dir = tempdir().unwrap();
        let vault = LocalVault::new(SQLiteVaultStore::new(dir.path().join("vault.db")));
        vault.init("old-passphrase", "default").unwrap();
        vault
            .put_secret(
                "secret://service/api",
                b"secret",
                "old-passphrase",
                SecretType::ApiKey,
            )
            .unwrap();

        let header = vault
            .rewrap_provider(
                Some("old-passphrase"),
                "passphrase",
                Some("new-passphrase"),
                "/dev/tpmrm0",
            )
            .unwrap();
        assert_eq!(
            header.provider_binding["provider_id"],
            serde_json::json!(PASSPHRASE_PROVIDER_ID)
        );
        assert!(matches!(
            vault.get_secret("secret://service/api", "old-passphrase"),
            Err(VaultError::Provider(_))
        ));
        assert_eq!(
            vault
                .get_secret("secret://service/api", "new-passphrase")
                .unwrap(),
            b"secret"
        );
    }

    #[test]
    fn secret_refs_must_use_scheme() {
        assert!(matches!(
            secret_id_from_ref("plain-name"),
            Err(VaultError::InvalidSecretRef)
        ));
    }
}
