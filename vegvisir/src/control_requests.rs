use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::guardrails::ApprovalRequest;

/// Versioned request envelope for UI, bridge, hook, or host-mediated control flows.
///
/// Control requests are coordination objects only. They do not grant permission by themselves:
/// callers must still run the resulting decision through Vegvisir's hard policy, HBSE boundary,
/// workspace scope checks, and approval ledger semantics before applying side effects.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlRequest<T> {
    pub request_id: String,
    pub run_id: String,
    pub subtype: String,
    pub payload: T,
    pub expires_at: Option<DateTime<Utc>>,
}

impl<T> ControlRequest<T> {
    pub fn map_payload<U>(self, f: impl FnOnce(T) -> U) -> ControlRequest<U> {
        ControlRequest {
            request_id: self.request_id,
            run_id: self.run_id,
            subtype: self.subtype,
            payload: f(self.payload),
            expires_at: self.expires_at,
        }
    }
}

/// Versioned response envelope for a previously emitted control request.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlResponse<T> {
    pub request_id: String,
    pub decision_source: String,
    pub payload: T,
}

pub type JsonControlRequest = ControlRequest<Value>;
pub type JsonControlResponse = ControlResponse<Value>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalControlPayload {
    pub approval_id: String,
    pub tool_name: String,
    pub risk_label: String,
    pub reason: String,
    pub args: Map<String, Value>,
}

impl ApprovalControlPayload {
    pub fn from_approval_request(request: ApprovalRequest) -> Self {
        Self {
            approval_id: request.id,
            tool_name: request.tool_name,
            risk_label: request.risk_label,
            reason: request.reason,
            args: request.args,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalControlDecision {
    pub decision: ApprovalControlDecisionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_args: Option<Map<String, Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalControlDecisionKind {
    AllowOnce,
    AllowForSession,
    Deny,
    Cancel,
}

impl ControlRequest<ApprovalControlPayload> {
    pub fn approval(
        run_id: impl Into<String>,
        request: ApprovalRequest,
        expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        let approval_id = request.id.clone();
        Self {
            request_id: format!("ctrl_{approval_id}"),
            run_id: run_id.into(),
            subtype: CONTROL_SUBTYPE_APPROVAL.to_string(),
            payload: ApprovalControlPayload::from_approval_request(request),
            expires_at,
        }
    }
}

pub const CONTROL_SUBTYPE_APPROVAL: &str = "approval";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingControlRequestRecord {
    pub request: JsonControlRequest,
    pub created_at: DateTime<Utc>,
    pub status: ControlRequestStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<JsonControlResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<String>,
}

impl PendingControlRequestRecord {
    pub fn is_pending(&self) -> bool {
        self.status == ControlRequestStatus::Pending
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControlRequestStatus {
    Pending,
    Resolved,
    Cancelled,
    TimedOut,
    Aborted,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ControlResolveOutcome {
    Applied {
        request_id: String,
        subtype: String,
    },
    DuplicateIgnored {
        request_id: String,
        existing_status: ControlRequestStatus,
    },
    TimedOut {
        request_id: String,
    },
    UnknownRequest {
        request_id: String,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingControlRequests {
    records: BTreeMap<String, PendingControlRequestRecord>,
}

impl PendingControlRequests {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(
        &mut self,
        request: JsonControlRequest,
        created_at: DateTime<Utc>,
    ) -> Result<(), ControlRequestInsertError> {
        if self.records.contains_key(&request.request_id) {
            return Err(ControlRequestInsertError::DuplicateRequestId(
                request.request_id,
            ));
        }
        self.records.insert(
            request.request_id.clone(),
            PendingControlRequestRecord {
                request,
                created_at,
                status: ControlRequestStatus::Pending,
                resolved_at: None,
                response: None,
                terminal_reason: None,
            },
        );
        Ok(())
    }

    pub fn get(&self, request_id: &str) -> Option<&PendingControlRequestRecord> {
        self.records.get(request_id)
    }

    pub fn records(&self) -> Vec<&PendingControlRequestRecord> {
        self.records.values().collect()
    }

    pub fn pending(&self) -> Vec<&PendingControlRequestRecord> {
        self.records
            .values()
            .filter(|record| record.is_pending())
            .collect()
    }

    pub fn pending_len(&self) -> usize {
        self.records
            .values()
            .filter(|record| record.is_pending())
            .count()
    }

    pub fn resolve(
        &mut self,
        response: JsonControlResponse,
        now: DateTime<Utc>,
    ) -> ControlResolveOutcome {
        let request_id = response.request_id.clone();
        let Some(record) = self.records.get_mut(&request_id) else {
            return ControlResolveOutcome::UnknownRequest { request_id };
        };
        if record.status != ControlRequestStatus::Pending {
            return ControlResolveOutcome::DuplicateIgnored {
                request_id,
                existing_status: record.status,
            };
        }
        if is_expired(record.request.expires_at, now) {
            record.status = ControlRequestStatus::TimedOut;
            record.resolved_at = Some(now);
            record.terminal_reason = Some("control request expired before response".to_string());
            return ControlResolveOutcome::TimedOut { request_id };
        }
        record.status = ControlRequestStatus::Resolved;
        record.resolved_at = Some(now);
        record.response = Some(response);
        ControlResolveOutcome::Applied {
            request_id,
            subtype: record.request.subtype.clone(),
        }
    }

    pub fn cancel(
        &mut self,
        request_id: &str,
        reason: impl Into<String>,
        now: DateTime<Utc>,
    ) -> bool {
        let Some(record) = self.records.get_mut(request_id) else {
            return false;
        };
        if record.status != ControlRequestStatus::Pending {
            return false;
        }
        record.status = ControlRequestStatus::Cancelled;
        record.resolved_at = Some(now);
        record.terminal_reason = Some(reason.into());
        true
    }

    pub fn abort_pending(&mut self, reason: impl Into<String>, now: DateTime<Utc>) -> Vec<String> {
        let reason = reason.into();
        let mut aborted = Vec::new();
        for record in self.records.values_mut() {
            if record.status == ControlRequestStatus::Pending {
                record.status = ControlRequestStatus::Aborted;
                record.resolved_at = Some(now);
                record.terminal_reason = Some(reason.clone());
                aborted.push(record.request.request_id.clone());
            }
        }
        aborted
    }

    pub fn expire(&mut self, now: DateTime<Utc>) -> Vec<String> {
        let mut expired = Vec::new();
        for record in self.records.values_mut() {
            if record.status == ControlRequestStatus::Pending
                && is_expired(record.request.expires_at, now)
            {
                record.status = ControlRequestStatus::TimedOut;
                record.resolved_at = Some(now);
                record.terminal_reason = Some("control request expired".to_string());
                expired.push(record.request.request_id.clone());
            }
        }
        expired
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlRequestInsertError {
    DuplicateRequestId(String),
}

impl std::fmt::Display for ControlRequestInsertError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateRequestId(request_id) => {
                write!(formatter, "duplicate control request id: {request_id}")
            }
        }
    }
}

impl std::error::Error for ControlRequestInsertError {}

fn is_expired(expires_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    expires_at.is_some_and(|expires_at| now >= expires_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use serde_json::json;

    fn ts() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap()
    }

    fn json_request(id: &str, expires_at: Option<DateTime<Utc>>) -> JsonControlRequest {
        ControlRequest {
            request_id: id.to_string(),
            run_id: "run-1".to_string(),
            subtype: "approval".to_string(),
            payload: json!({"approval_id":"apr_1"}),
            expires_at,
        }
    }

    #[test]
    fn pending_control_request_resolves_once_and_guards_duplicates() {
        let mut pending = PendingControlRequests::new();
        pending.insert(json_request("ctrl-1", None), ts()).unwrap();
        assert_eq!(pending.pending_len(), 1);

        let response = ControlResponse {
            request_id: "ctrl-1".to_string(),
            decision_source: "local_ui".to_string(),
            payload: json!({"decision":"allow_once"}),
        };
        assert_eq!(
            pending.resolve(response.clone(), ts()),
            ControlResolveOutcome::Applied {
                request_id: "ctrl-1".to_string(),
                subtype: "approval".to_string()
            }
        );
        assert_eq!(pending.pending_len(), 0);
        assert_eq!(
            pending.resolve(response, ts()),
            ControlResolveOutcome::DuplicateIgnored {
                request_id: "ctrl-1".to_string(),
                existing_status: ControlRequestStatus::Resolved,
            }
        );
    }

    #[test]
    fn expired_control_request_cannot_be_resolved() {
        let mut pending = PendingControlRequests::new();
        pending
            .insert(
                json_request("ctrl-1", Some(ts() + Duration::seconds(5))),
                ts(),
            )
            .unwrap();

        let response = ControlResponse {
            request_id: "ctrl-1".to_string(),
            decision_source: "bridge".to_string(),
            payload: json!({"decision":"allow_once"}),
        };
        assert_eq!(
            pending.resolve(response, ts() + Duration::seconds(6)),
            ControlResolveOutcome::TimedOut {
                request_id: "ctrl-1".to_string()
            }
        );
        let record = pending.get("ctrl-1").unwrap();
        assert_eq!(record.status, ControlRequestStatus::TimedOut);
        assert!(record.response.is_none());
    }

    #[test]
    fn abort_pending_marks_only_pending_records() {
        let mut pending = PendingControlRequests::new();
        pending.insert(json_request("ctrl-1", None), ts()).unwrap();
        pending.insert(json_request("ctrl-2", None), ts()).unwrap();
        pending.cancel("ctrl-2", "user cancelled", ts());

        assert_eq!(
            pending.abort_pending("run aborted", ts()),
            vec!["ctrl-1".to_string()]
        );
        assert_eq!(
            pending.get("ctrl-1").unwrap().status,
            ControlRequestStatus::Aborted
        );
        assert_eq!(
            pending.get("ctrl-2").unwrap().status,
            ControlRequestStatus::Cancelled
        );
    }

    #[test]
    fn approval_control_request_preserves_approval_payload() {
        let mut args = Map::new();
        args.insert("path".to_string(), json!("example.txt"));
        let approval = ApprovalRequest {
            id: "apr_123".to_string(),
            reason: "Risky tool requires human approval: write_file".to_string(),
            tool_name: "write_file".to_string(),
            args: args.clone(),
            risk_label: "filesystem-write".to_string(),
        };

        let request = ControlRequest::approval("run-1", approval, Some(ts()));

        assert_eq!(request.request_id, "ctrl_apr_123");
        assert_eq!(request.subtype, CONTROL_SUBTYPE_APPROVAL);
        assert_eq!(request.payload.approval_id, "apr_123");
        assert_eq!(request.payload.args, args);
    }
}
