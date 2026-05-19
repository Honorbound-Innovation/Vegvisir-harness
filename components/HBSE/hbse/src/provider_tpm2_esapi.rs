use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sha2::{Digest as ShaDigest, Sha256};
use thiserror::Error;
use tss_esapi::{
    attributes::ObjectAttributesBuilder,
    interface_types::{
        algorithm::{HashingAlgorithm, PublicAlgorithm},
        resource_handles::Hierarchy,
    },
    structures::{
        CreatePrimaryKeyResult, Digest, KeyedHashScheme, Private, Public, PublicBuilder,
        PublicKeyedHashParameters, SensitiveData, SymmetricCipherParameters,
        SymmetricDefinitionObject,
    },
    traits::{Marshall, UnMarshall},
    Context, TctiNameConf,
};

use crate::keys::KEY_SIZE;
use crate::serialization::{b64url_decode_no_padding, b64url_no_padding};

pub const TPM2_ESAPI_PROVIDER_ID: &str = "linux-tpm2-esapi-seal";

#[derive(Debug, Error)]
pub enum Tpm2EsapiProviderError {
    #[error("vault root key must be 32 bytes")]
    InvalidRootKeyLength,
    #[error("unsupported TPM provider binding")]
    UnsupportedProvider,
    #[error("TPM provider unavailable: {0}")]
    Unavailable(String),
    #[error("TPM sealed object identity mismatch")]
    IdentityMismatch,
    #[error("TPM returned invalid root key length")]
    InvalidRootKeyLengthFromTpm,
    #[error("provider binding decode failed: {0}")]
    Decode(#[from] base64::DecodeError),
    #[error("TSS2 error: {0}")]
    Tss(#[from] tss_esapi::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tpm2EsapiProviderStatus {
    pub available: bool,
    pub device_path: String,
    pub tcti: String,
    pub device_accessible: bool,
    pub direct_bindings_available: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tpm2EsapiProviderBinding {
    pub provider_id: String,
    pub vault_id: String,
    pub device_path: String,
    pub tcti: String,
    pub parent_hierarchy: String,
    pub public: String,
    pub private: String,
    pub public_info_sha256: String,
    pub assurance_level: String,
    pub warning: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxTpm2EsapiProvider {
    pub device_path: String,
}

impl Default for LinuxTpm2EsapiProvider {
    fn default() -> Self {
        Self {
            device_path: "/dev/tpmrm0".to_string(),
        }
    }
}

impl LinuxTpm2EsapiProvider {
    pub fn new(device_path: impl Into<String>) -> Self {
        Self {
            device_path: device_path.into(),
        }
    }

    pub fn detect(&self) -> Tpm2EsapiProviderStatus {
        let device = Path::new(&self.device_path);
        let device_accessible = device.exists() && readable_writable(device);
        let tcti = self.tcti();
        let direct_bindings_available = if device_accessible {
            TctiNameConf::from_str(&tcti).and_then(Context::new).is_ok()
        } else {
            false
        };
        let detail = if !device.exists() {
            format!("{} does not exist", self.device_path)
        } else if !device_accessible {
            format!(
                "{} exists but is not readable/writable by this user",
                self.device_path
            )
        } else if !direct_bindings_available {
            "TPM device exists, but native TSS2/ESAPI context creation failed".to_string()
        } else {
            "TPM device and native TSS2/ESAPI bindings are available".to_string()
        };
        Tpm2EsapiProviderStatus {
            available: device_accessible && direct_bindings_available,
            device_path: self.device_path.clone(),
            tcti,
            device_accessible,
            direct_bindings_available,
            detail,
        }
    }

    pub fn wrap_root_key(
        &self,
        vault_id: &str,
        root_key: &[u8],
    ) -> Result<Tpm2EsapiProviderBinding, Tpm2EsapiProviderError> {
        if root_key.len() != KEY_SIZE {
            return Err(Tpm2EsapiProviderError::InvalidRootKeyLength);
        }
        self.require_available()?;
        let mut context = self.context()?;
        let primary = create_primary(&mut context)?;
        let sensitive_data = SensitiveData::try_from(root_key.to_vec())?;
        let sealed_public_template = sealed_public_template()?;
        let (private, public) = context.execute_with_nullauth_session(|ctx| {
            ctx.create(
                primary.key_handle,
                sealed_public_template,
                None,
                Some(sensitive_data.clone()),
                None,
                None,
            )
            .map(|key| (key.out_private, key.out_public))
        })?;
        let public_bytes = public.marshall()?;
        Ok(Tpm2EsapiProviderBinding {
            provider_id: TPM2_ESAPI_PROVIDER_ID.to_string(),
            vault_id: vault_id.to_string(),
            device_path: self.device_path.clone(),
            tcti: self.tcti(),
            parent_hierarchy: "owner".to_string(),
            public: b64url_no_padding(&public_bytes),
            private: b64url_no_padding(private.value()),
            public_info_sha256: b64url_no_padding(&Sha256::digest(public_bytes)),
            assurance_level: "A2".to_string(),
            warning: "native TPM2-TSS ESAPI provider; sealed root key cannot be unwrapped without this TPM and owner hierarchy."
                .to_string(),
        })
    }

    pub fn unwrap_root_key(
        &self,
        binding: &Tpm2EsapiProviderBinding,
    ) -> Result<[u8; KEY_SIZE], Tpm2EsapiProviderError> {
        if binding.provider_id != TPM2_ESAPI_PROVIDER_ID {
            return Err(Tpm2EsapiProviderError::UnsupportedProvider);
        }
        self.require_available()?;
        let public_bytes = b64url_decode_no_padding(&binding.public)?;
        let actual_hash = b64url_no_padding(&Sha256::digest(&public_bytes));
        if !binding.public_info_sha256.is_empty() && binding.public_info_sha256 != actual_hash {
            return Err(Tpm2EsapiProviderError::IdentityMismatch);
        }
        let private = Private::try_from(b64url_decode_no_padding(&binding.private)?)?;
        let public = Public::unmarshall(&public_bytes)?;
        let mut context = self.context()?;
        let primary = create_primary(&mut context)?;
        let unsealed = context.execute_with_nullauth_session(|ctx| {
            let sealed = ctx.load(primary.key_handle, private.clone(), public.clone())?;
            ctx.unseal(sealed.into())
        })?;
        let root_key: Vec<u8> = unsealed.value().to_vec();
        root_key
            .try_into()
            .map_err(|_| Tpm2EsapiProviderError::InvalidRootKeyLengthFromTpm)
    }

    pub fn self_test(&self) -> Result<Tpm2EsapiProviderStatus, Tpm2EsapiProviderError> {
        let status = self.detect();
        if !status.available {
            return Ok(status);
        }
        let root_key: [u8; KEY_SIZE] = rand::random();
        let binding = self.wrap_root_key("self-test", &root_key)?;
        let unwrapped = self.unwrap_root_key(&binding)?;
        if unwrapped != root_key {
            return Err(Tpm2EsapiProviderError::IdentityMismatch);
        }
        Ok(status)
    }

    fn require_available(&self) -> Result<(), Tpm2EsapiProviderError> {
        let status = self.detect();
        if status.available {
            Ok(())
        } else {
            Err(Tpm2EsapiProviderError::Unavailable(status.detail))
        }
    }

    fn context(&self) -> Result<Context, Tpm2EsapiProviderError> {
        Ok(Context::new(TctiNameConf::from_str(&self.tcti())?)?)
    }

    fn tcti(&self) -> String {
        format!("device:{}", self.device_path)
    }
}

fn create_primary(context: &mut Context) -> Result<CreatePrimaryKeyResult, Tpm2EsapiProviderError> {
    let object_attributes = ObjectAttributesBuilder::new()
        .with_fixed_tpm(true)
        .with_fixed_parent(true)
        .with_st_clear(false)
        .with_sensitive_data_origin(true)
        .with_user_with_auth(true)
        .with_decrypt(true)
        .with_restricted(true)
        .build()?;

    let primary_pub = PublicBuilder::new()
        .with_public_algorithm(PublicAlgorithm::SymCipher)
        .with_name_hashing_algorithm(HashingAlgorithm::Sha256)
        .with_object_attributes(object_attributes)
        .with_symmetric_cipher_parameters(SymmetricCipherParameters::new(
            SymmetricDefinitionObject::AES_128_CFB,
        ))
        .with_symmetric_cipher_unique_identifier(Digest::default())
        .build()?;

    Ok(context.execute_with_nullauth_session(|ctx| {
        ctx.create_primary(Hierarchy::Owner, primary_pub, None, None, None, None)
    })?)
}

fn sealed_public_template() -> Result<Public, Tpm2EsapiProviderError> {
    let object_attributes = ObjectAttributesBuilder::new()
        .with_fixed_tpm(true)
        .with_fixed_parent(true)
        .with_st_clear(true)
        .with_user_with_auth(true)
        .build()?;

    Ok(PublicBuilder::new()
        .with_public_algorithm(PublicAlgorithm::KeyedHash)
        .with_name_hashing_algorithm(HashingAlgorithm::Sha256)
        .with_object_attributes(object_attributes)
        .with_keyed_hash_parameters(PublicKeyedHashParameters::new(KeyedHashScheme::Null))
        .with_keyed_hash_unique_identifier(Digest::default())
        .build()?)
}

fn readable_writable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let mode = metadata.permissions().mode();
    mode & 0o600 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_reports_missing_device() {
        let status = LinuxTpm2EsapiProvider::new("/definitely/not/a/tpm").detect();
        assert!(!status.available);
        assert!(!status.device_accessible);
        assert!(status.detail.contains("does not exist"));
    }
}
