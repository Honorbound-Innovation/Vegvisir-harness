use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::policy::{AccessPolicy, AccessRequest, DeliveryMode};
use crate::serialization::{b64url_no_padding, canonical_json_bytes, utc_millis};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TicketValidationError {
    #[error("ticket MAC mismatch")]
    MacMismatch,
    #[error("ticket revoked")]
    Revoked,
    #[error("ticket replay denied")]
    ReplayDenied,
    #[error("ticket expired")]
    Expired,
    #[error("ticket secret context mismatch")]
    SecretContextMismatch,
    #[error("ticket consumer context mismatch")]
    ConsumerContextMismatch,
    #[error("ticket purpose context mismatch")]
    PurposeContextMismatch,
    #[error("ticket delivery context mismatch")]
    DeliveryContextMismatch,
    #[error("ticket HTTP host context mismatch")]
    HttpHostContextMismatch,
    #[error("ticket HTTP scheme context mismatch")]
    HttpSchemeContextMismatch,
    #[error("ticket HTTP method context mismatch")]
    HttpMethodContextMismatch,
    #[error("ticket HTTP path context mismatch")]
    HttpPathContextMismatch,
    #[error("ticket HTTP request body context mismatch")]
    HttpRequestBodyContextMismatch,
    #[error("ticket OS user context mismatch")]
    OsUserContextMismatch,
    #[error("ticket executable path context mismatch")]
    ExecutablePathContextMismatch,
    #[error("ticket executable hash context mismatch")]
    ExecutableHashContextMismatch,
    #[error("ticket policy context mismatch")]
    PolicyContextMismatch,
    #[error("ticket broker session context mismatch")]
    BrokerSessionContextMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretAccessTicket {
    pub ticket_id: String,
    pub vault_id: String,
    #[serde(default)]
    pub secret_id: String,
    pub secret_ref: String,
    #[serde(default)]
    pub secret_version: u64,
    pub consumer: String,
    pub purpose: String,
    pub delivery_mode: DeliveryMode,
    pub http_host: Option<String>,
    pub http_scheme: Option<String>,
    pub http_method: Option<String>,
    pub http_path: Option<String>,
    pub http_request_body_bytes: Option<u64>,
    pub os_uid: Option<u32>,
    pub executable_path: Option<String>,
    pub executable_sha256: Option<String>,
    pub policy_id: String,
    pub policy_hash: String,
    #[serde(default)]
    pub exportable: bool,
    #[serde(default)]
    pub model_visible: bool,
    #[serde(default)]
    pub shell_visible: bool,
    #[serde(default)]
    pub broker_session_id: Option<String>,
    pub issued_at: String,
    pub expires_at: String,
    pub max_uses: u32,
    pub uses_remaining: u32,
    pub revoked: bool,
    pub mac: String,
}

impl SecretAccessTicket {
    fn payload(&self) -> serde_json::Value {
        let mut value = serde_json::to_value(self).expect("ticket serializes");
        value
            .as_object_mut()
            .expect("ticket serializes as object")
            .remove("mac");
        value
    }
}

#[derive(Debug, Clone)]
pub struct TicketManager {
    mac_key: Vec<u8>,
}

impl TicketManager {
    pub fn new(mac_key: impl Into<Vec<u8>>) -> Self {
        Self {
            mac_key: mac_key.into(),
        }
    }

    pub fn issue(
        &self,
        vault_id: &str,
        secret_id: &str,
        secret_version: u64,
        request: &AccessRequest,
        policy: &AccessPolicy,
        policy_hash: &str,
        broker_session_id: Option<String>,
    ) -> SecretAccessTicket {
        let issued_at = request.now;
        let expires_at = issued_at + Duration::seconds(policy.max_ticket_ttl_seconds as i64);
        let mut ticket = SecretAccessTicket {
            ticket_id: Uuid::new_v4().to_string(),
            vault_id: vault_id.to_string(),
            secret_id: secret_id.to_string(),
            secret_ref: request.secret_ref.clone(),
            secret_version,
            consumer: request.consumer.clone(),
            purpose: request.purpose.clone(),
            delivery_mode: request.delivery_mode,
            http_host: request.http_host.clone(),
            http_scheme: request.http_scheme.clone(),
            http_method: request.http_method.clone(),
            http_path: request.http_path.clone(),
            http_request_body_bytes: request.http_request_body_bytes,
            os_uid: request.os_uid,
            executable_path: request.executable_path.clone(),
            executable_sha256: request.executable_sha256.clone(),
            policy_id: policy.policy_id.clone(),
            policy_hash: policy_hash.to_string(),
            exportable: policy.exportable,
            model_visible: false,
            shell_visible: matches!(
                request.delivery_mode,
                DeliveryMode::ChildEnv | DeliveryMode::TerminalPrint | DeliveryMode::Raw
            ),
            broker_session_id,
            issued_at: utc_millis(issued_at),
            expires_at: utc_millis(expires_at),
            max_uses: policy.max_uses,
            uses_remaining: policy.max_uses,
            revoked: false,
            mac: String::new(),
        };
        ticket.mac = self.mac(&ticket.payload());
        ticket
    }

    pub fn validate(
        &self,
        ticket: &SecretAccessTicket,
        request: &AccessRequest,
        now: DateTime<Utc>,
    ) -> Result<(), TicketValidationError> {
        if self.mac(&ticket.payload()) != ticket.mac {
            return Err(TicketValidationError::MacMismatch);
        }
        if ticket.revoked {
            return Err(TicketValidationError::Revoked);
        }
        if ticket.uses_remaining < 1 {
            return Err(TicketValidationError::ReplayDenied);
        }
        let expires_at = DateTime::parse_from_rfc3339(&ticket.expires_at)
            .map_err(|_| TicketValidationError::Expired)?
            .with_timezone(&Utc);
        if now > expires_at {
            return Err(TicketValidationError::Expired);
        }
        if ticket.secret_ref != request.secret_ref {
            return Err(TicketValidationError::SecretContextMismatch);
        }
        if !ticket.broker_session_id_matches(request.broker_session_id.as_deref()) {
            return Err(TicketValidationError::BrokerSessionContextMismatch);
        }
        if ticket.consumer != request.consumer {
            return Err(TicketValidationError::ConsumerContextMismatch);
        }
        if ticket.purpose != request.purpose {
            return Err(TicketValidationError::PurposeContextMismatch);
        }
        if ticket.delivery_mode != request.delivery_mode {
            return Err(TicketValidationError::DeliveryContextMismatch);
        }
        if ticket.http_host != request.http_host {
            return Err(TicketValidationError::HttpHostContextMismatch);
        }
        if ticket.http_scheme != request.http_scheme {
            return Err(TicketValidationError::HttpSchemeContextMismatch);
        }
        if ticket.http_method != request.http_method {
            return Err(TicketValidationError::HttpMethodContextMismatch);
        }
        if ticket.http_path != request.http_path {
            return Err(TicketValidationError::HttpPathContextMismatch);
        }
        if ticket.http_request_body_bytes != request.http_request_body_bytes {
            return Err(TicketValidationError::HttpRequestBodyContextMismatch);
        }
        if ticket.os_uid != request.os_uid {
            return Err(TicketValidationError::OsUserContextMismatch);
        }
        if ticket.executable_path != request.executable_path {
            return Err(TicketValidationError::ExecutablePathContextMismatch);
        }
        if ticket.executable_sha256 != request.executable_sha256 {
            return Err(TicketValidationError::ExecutableHashContextMismatch);
        }
        Ok(())
    }

    pub fn validate_policy(
        &self,
        ticket: &SecretAccessTicket,
        policy: &AccessPolicy,
        current_policy_hash: &str,
    ) -> Result<(), TicketValidationError> {
        if ticket.policy_id != policy.policy_id || ticket.policy_hash != current_policy_hash {
            return Err(TicketValidationError::PolicyContextMismatch);
        }
        Ok(())
    }

    pub fn consume(
        &self,
        ticket: &SecretAccessTicket,
        request: &AccessRequest,
        now: DateTime<Utc>,
    ) -> Result<SecretAccessTicket, TicketValidationError> {
        self.validate(ticket, request, now)?;
        let mut updated = ticket.clone();
        updated.uses_remaining -= 1;
        updated.mac = self.mac(&updated.payload());
        Ok(updated)
    }

    pub fn revoke(
        &self,
        ticket: &SecretAccessTicket,
    ) -> Result<SecretAccessTicket, TicketValidationError> {
        if self.mac(&ticket.payload()) != ticket.mac {
            return Err(TicketValidationError::MacMismatch);
        }
        let mut updated = ticket.clone();
        updated.revoked = true;
        updated.mac = self.mac(&updated.payload());
        Ok(updated)
    }

    fn mac(&self, payload: &serde_json::Value) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.mac_key).expect("HMAC accepts any key");
        mac.update(&canonical_json_bytes(payload).expect("ticket payload is canonical"));
        b64url_no_padding(&mac.finalize().into_bytes())
    }
}

impl SecretAccessTicket {
    fn broker_session_id_matches(&self, request_session: Option<&str>) -> bool {
        match (&self.broker_session_id, request_session) {
            (Some(ticket_session), Some(request_session)) => ticket_session == request_session,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }
}

pub fn policy_hash(policy: &AccessPolicy) -> String {
    let canonical = canonical_json_bytes(policy).expect("policy serializes canonically");
    b64url_no_padding(&Sha256::digest(canonical))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{AccessPolicy, AccessRequest, DeliveryMode};

    fn request() -> AccessRequest {
        AccessRequest {
            secret_ref: "secret://default/api".to_string(),
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
    fn ticket_replay_revocation_and_context_checks_fail() {
        let manager = TicketManager::new(vec![7; 32]);
        let policy = AccessPolicy {
            policy_id: "p1".to_string(),
            secret_refs: vec!["secret://default/api".to_string()],
            allowed_consumers: vec!["cli".to_string()],
            allowed_purposes: vec!["deploy".to_string()],
            allowed_delivery_modes: vec![DeliveryMode::TerminalPrint],
            exportable: true,
            ..serde_json::from_value(serde_json::json!({"policy_id":"defaults"})).unwrap()
        };
        let request = request();
        let ticket = manager.issue("vault-1", "api", 1, &request, &policy, "hash", None);
        let consumed = manager.consume(&ticket, &request, Utc::now()).unwrap();
        assert_eq!(
            manager
                .consume(&consumed, &request, Utc::now())
                .unwrap_err(),
            TicketValidationError::ReplayDenied
        );

        let revoked = manager.revoke(&ticket).unwrap();
        assert_eq!(
            manager
                .validate(&revoked, &request, Utc::now())
                .unwrap_err(),
            TicketValidationError::Revoked
        );

        let mut wrong = request.clone();
        wrong.consumer = "other".to_string();
        assert_eq!(
            manager.validate(&ticket, &wrong, Utc::now()).unwrap_err(),
            TicketValidationError::ConsumerContextMismatch
        );
    }

    #[test]
    fn broker_session_bound_ticket_requires_matching_session() {
        let manager = TicketManager::new(vec![7; 32]);
        let policy = AccessPolicy {
            policy_id: "p1".to_string(),
            secret_refs: vec!["secret://default/api".to_string()],
            allowed_consumers: vec!["cli".to_string()],
            allowed_purposes: vec!["deploy".to_string()],
            allowed_delivery_modes: vec![DeliveryMode::TerminalPrint],
            exportable: true,
            ..serde_json::from_value(serde_json::json!({"policy_id":"defaults"})).unwrap()
        };
        let mut request = request();
        request.broker_session_id = Some("session-1".to_string());
        let ticket = manager.issue(
            "vault-1",
            "api",
            1,
            &request,
            &policy,
            "hash",
            Some("session-1".to_string()),
        );

        request.broker_session_id = Some("session-2".to_string());
        assert_eq!(
            manager.validate(&ticket, &request, Utc::now()).unwrap_err(),
            TicketValidationError::BrokerSessionContextMismatch
        );
    }
}
