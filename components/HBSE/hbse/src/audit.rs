use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::serialization::{b64url_no_padding, canonical_json_bytes, utc_millis};

type HmacSha256 = Hmac<Sha256>;

pub const ZERO_HASH: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuditVerificationError {
    #[error("audit previous hash mismatch")]
    PreviousHashMismatch,
    #[error("audit event hash mismatch")]
    EventHashMismatch,
    #[error("audit event MAC mismatch")]
    EventMacMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_id: String,
    pub timestamp: String,
    pub vault_id: String,
    pub namespace_id: String,
    pub event_type: String,
    pub severity: String,
    pub decision: String,
    pub previous_hash: String,
    pub event_hash: String,
    pub event_mac: String,
    pub metadata: Map<String, Value>,
}

impl AuditEvent {
    fn mac_payload(&self) -> Value {
        let mut value = serde_json::to_value(self).expect("audit event serializes");
        value
            .as_object_mut()
            .expect("audit event serializes as object")
            .remove("event_mac");
        value
    }

    fn hash_payload(&self) -> Value {
        let mut value = serde_json::to_value(self).expect("audit event serializes");
        let object = value
            .as_object_mut()
            .expect("audit event serializes as object");
        object.remove("event_hash");
        object.remove("event_mac");
        value
    }
}

#[derive(Debug, Clone)]
pub struct AuditManager {
    mac_key: Vec<u8>,
    events: Vec<AuditEvent>,
}

impl AuditManager {
    pub fn new(mac_key: impl Into<Vec<u8>>, events: Vec<AuditEvent>) -> Self {
        Self {
            mac_key: mac_key.into(),
            events,
        }
    }

    pub fn append(
        &mut self,
        vault_id: &str,
        namespace_id: &str,
        event_type: &str,
        severity: &str,
        decision: &str,
        metadata: Map<String, Value>,
    ) -> AuditEvent {
        let previous_hash = self
            .events
            .last()
            .map(|event| event.event_hash.clone())
            .unwrap_or_else(|| ZERO_HASH.to_string());
        let base = json!({
            "event_id": Uuid::new_v4().to_string(),
            "timestamp": utc_millis(chrono::Utc::now()),
            "vault_id": vault_id,
            "namespace_id": namespace_id,
            "event_type": event_type,
            "severity": severity,
            "decision": decision,
            "previous_hash": previous_hash,
            "metadata": metadata,
        });
        let event_hash = b64url_no_padding(&Sha256::digest(
            canonical_json_bytes(&base).expect("audit base serializes"),
        ));
        let mut mac_payload = base.as_object().expect("base is object").clone();
        mac_payload.insert("event_hash".to_string(), Value::String(event_hash.clone()));
        let event_mac = audit_mac(&self.mac_key, &Value::Object(mac_payload.clone()));
        let event: AuditEvent = serde_json::from_value(Value::Object({
            mac_payload.insert("event_mac".to_string(), Value::String(event_mac));
            mac_payload
        }))
        .expect("constructed audit event is valid");
        self.events.push(event.clone());
        event
    }
}

pub fn verify_audit_chain(
    events: &[AuditEvent],
    mac_key: &[u8],
) -> Result<(), AuditVerificationError> {
    let mut previous_hash = ZERO_HASH.to_string();
    for event in events {
        if event.previous_hash != previous_hash {
            return Err(AuditVerificationError::PreviousHashMismatch);
        }
        let expected_hash = b64url_no_padding(&Sha256::digest(
            canonical_json_bytes(&event.hash_payload()).expect("audit hash payload serializes"),
        ));
        if expected_hash != event.event_hash {
            return Err(AuditVerificationError::EventHashMismatch);
        }
        let expected_mac = audit_mac(mac_key, &event.mac_payload());
        if expected_mac != event.event_mac {
            return Err(AuditVerificationError::EventMacMismatch);
        }
        previous_hash = event.event_hash.clone();
    }
    Ok(())
}

fn audit_mac(mac_key: &[u8], payload: &Value) -> String {
    let mut mac = HmacSha256::new_from_slice(mac_key).expect("HMAC accepts any key");
    mac.update(&canonical_json_bytes(payload).expect("audit payload serializes"));
    b64url_no_padding(&mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_chain_verifies_and_tampering_fails() {
        let key = vec![9; 32];
        let mut manager = AuditManager::new(key.clone(), vec![]);
        let first = manager.append(
            "vault",
            "default",
            "vault.initialized",
            "info",
            "allow",
            Map::new(),
        );
        let second = manager.append(
            "vault",
            "default",
            "secret.stored",
            "info",
            "allow",
            Map::new(),
        );

        assert!(verify_audit_chain(&[first.clone(), second.clone()], &key).is_ok());
        let mut tampered = second;
        tampered.decision = "deny".to_string();
        assert_eq!(
            verify_audit_chain(&[first, tampered], &key).unwrap_err(),
            AuditVerificationError::EventHashMismatch
        );
    }
}
