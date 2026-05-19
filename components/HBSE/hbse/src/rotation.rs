use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::records::SecretRecord;
use crate::serialization::utc_millis;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotationJobStatus {
    Staged,
    Verified,
    Promoted,
    RolledBack,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RotationJob {
    pub job_id: String,
    pub vault_id: String,
    pub secret_ref: String,
    pub staged_version: u64,
    pub status: RotationJobStatus,
    pub created_at: String,
    pub updated_at: String,
    pub staged_record: SecretRecord,
    #[serde(default = "default_verifier")]
    pub verifier: String,
    pub failure_reason: Option<String>,
}

impl RotationJob {
    pub fn create(
        vault_id: impl Into<String>,
        secret_ref: impl Into<String>,
        staged_version: u64,
        staged_record: SecretRecord,
    ) -> Self {
        let now = utc_millis(Utc::now());
        Self {
            job_id: Uuid::new_v4().to_string(),
            vault_id: vault_id.into(),
            secret_ref: secret_ref.into(),
            staged_version,
            status: RotationJobStatus::Staged,
            created_at: now.clone(),
            updated_at: now,
            staged_record,
            verifier: default_verifier(),
            failure_reason: None,
        }
    }

    pub fn transition(&self, status: RotationJobStatus, failure_reason: Option<String>) -> Self {
        let mut updated = self.clone();
        updated.status = status;
        updated.updated_at = utc_millis(Utc::now());
        updated.failure_reason = failure_reason;
        updated
    }
}

pub fn rotation_status_string(status: RotationJobStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn default_verifier() -> String {
    "decrypt-self-test".to_string()
}

#[cfg(test)]
mod tests {
    use crate::crypto::{hash_bytes, CryptoEngine};
    use crate::keys::{KeyHierarchy, KEY_SIZE};
    use crate::records::SecretType;

    use super::*;

    #[test]
    fn rotation_job_transitions_with_updated_timestamp() {
        let keys = KeyHierarchy::new("vault", &[9u8; KEY_SIZE]).unwrap();
        let record = CryptoEngine.encrypt_secret(
            &keys,
            "default",
            "secret-id",
            "secret://rotation",
            1,
            b"value",
            SecretType::Generic,
            "default-deny",
            "policy-hash",
            &hash_bytes(b"secret://rotation"),
            "unbound-provider-policy",
        );
        let job = RotationJob::create("vault", "secret://rotation", 1, record);
        assert_eq!(job.status, RotationJobStatus::Staged);
        let verified = job.transition(RotationJobStatus::Verified, None);
        assert_eq!(verified.status, RotationJobStatus::Verified);
        assert_eq!(verified.verifier, "decrypt-self-test");
    }
}
