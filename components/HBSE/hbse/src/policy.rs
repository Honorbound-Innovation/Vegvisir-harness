use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    BrokeredHttp,
    BrokeredOperation,
    Callback,
    Pipe,
    Fd,
    TempFile,
    ChildEnv,
    Raw,
    TerminalPrint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessPolicy {
    pub policy_id: String,
    #[serde(default)]
    pub secret_refs: Vec<String>,
    #[serde(default)]
    pub allowed_consumers: Vec<String>,
    #[serde(default)]
    pub denied_consumers: Vec<String>,
    #[serde(default)]
    pub allowed_purposes: Vec<String>,
    #[serde(default)]
    pub denied_purposes: Vec<String>,
    #[serde(default)]
    pub allowed_delivery_modes: Vec<DeliveryMode>,
    #[serde(default)]
    pub allowed_http_hosts: Vec<String>,
    #[serde(default)]
    pub denied_http_hosts: Vec<String>,
    #[serde(default)]
    pub allowed_http_methods: Vec<String>,
    #[serde(default)]
    pub denied_http_methods: Vec<String>,
    #[serde(default)]
    pub allowed_http_path_prefixes: Vec<String>,
    #[serde(default)]
    pub denied_http_path_prefixes: Vec<String>,
    #[serde(default)]
    pub require_https_for_brokered_http: bool,
    #[serde(default)]
    pub max_http_request_body_bytes: Option<u64>,
    #[serde(default)]
    pub allowed_os_uids: Vec<u32>,
    #[serde(default)]
    pub denied_os_uids: Vec<u32>,
    #[serde(default)]
    pub allowed_executable_paths: Vec<String>,
    #[serde(default)]
    pub denied_executable_paths: Vec<String>,
    #[serde(default)]
    pub allowed_executable_sha256: Vec<String>,
    #[serde(default)]
    pub denied_executable_sha256: Vec<String>,
    #[serde(default)]
    pub exportable: bool,
    #[serde(default = "default_ticket_ttl")]
    pub max_ticket_ttl_seconds: u64,
    #[serde(default = "default_max_uses")]
    pub max_uses: u32,
    #[serde(default = "default_minimum_provider_assurance")]
    pub minimum_provider_assurance: String,
    #[serde(default)]
    pub require_mfa: bool,
    #[serde(default)]
    pub allow_unbound_plaintext_export: bool,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

fn default_ticket_ttl() -> u64 {
    60
}

fn default_max_uses() -> u32 {
    1
}

fn default_minimum_provider_assurance() -> String {
    "A1".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessRequest {
    pub secret_ref: String,
    pub consumer: String,
    pub purpose: String,
    pub delivery_mode: DeliveryMode,
    pub provider_assurance: String,
    pub raw_export_requested: bool,
    pub http_host: Option<String>,
    pub http_scheme: Option<String>,
    pub http_method: Option<String>,
    pub http_path: Option<String>,
    pub http_request_body_bytes: Option<u64>,
    pub os_uid: Option<u32>,
    pub executable_path: Option<String>,
    pub executable_sha256: Option<String>,
    pub mfa_verified: bool,
    pub broker_session_id: Option<String>,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationResult<'a> {
    pub decision: PolicyDecision,
    pub reason: String,
    pub policy: Option<&'a AccessPolicy>,
}

impl EvaluationResult<'_> {
    pub fn allowed(&self) -> bool {
        self.decision == PolicyDecision::Allow
    }
}

#[derive(Debug, Clone)]
pub struct PolicyEngine {
    policies: Vec<AccessPolicy>,
}

impl PolicyEngine {
    pub fn new(policies: Vec<AccessPolicy>) -> Self {
        Self { policies }
    }

    pub fn evaluate(&self, request: &AccessRequest) -> EvaluationResult<'_> {
        for policy in &self.policies {
            let result = self.evaluate_policy(policy, request);
            if result.decision == PolicyDecision::Allow {
                return result;
            }
        }
        EvaluationResult {
            decision: PolicyDecision::Deny,
            reason: "no matching allow policy".to_string(),
            policy: None,
        }
    }

    fn evaluate_policy<'a>(
        &'a self,
        policy: &'a AccessPolicy,
        request: &AccessRequest,
    ) -> EvaluationResult<'a> {
        macro_rules! deny {
            ($reason:expr) => {
                return EvaluationResult {
                    decision: PolicyDecision::Deny,
                    reason: $reason.to_string(),
                    policy: Some(policy),
                }
            };
        }

        if !policy.secret_refs.contains(&request.secret_ref)
            && !policy.secret_refs.iter().any(|item| item == "*")
        {
            deny!("secret reference not covered");
        }
        if let Some(expires_at) = policy.expires_at {
            if request.now > expires_at {
                deny!("policy expired");
            }
        }
        if policy.denied_consumers.contains(&request.consumer) {
            deny!("consumer explicitly denied");
        }
        if policy.denied_purposes.contains(&request.purpose) {
            deny!("purpose explicitly denied");
        }
        if !policy.allowed_consumers.contains(&request.consumer)
            && !policy.allowed_consumers.iter().any(|item| item == "*")
        {
            deny!("consumer not allowed");
        }
        if !policy.allowed_purposes.contains(&request.purpose)
            && !policy.allowed_purposes.iter().any(|item| item == "*")
        {
            deny!("purpose not allowed");
        }
        if !policy
            .allowed_delivery_modes
            .contains(&request.delivery_mode)
        {
            deny!("delivery mode not allowed");
        }
        if let Some(uid) = request.os_uid {
            if policy.denied_os_uids.contains(&uid) {
                deny!("OS user explicitly denied");
            }
        }
        if !policy.allowed_os_uids.is_empty()
            && !request
                .os_uid
                .map_or(false, |uid| policy.allowed_os_uids.contains(&uid))
        {
            deny!("OS user not allowed");
        }
        if let Some(path) = &request.executable_path {
            if policy.denied_executable_paths.contains(path) {
                deny!("executable path explicitly denied");
            }
        }
        if !policy.allowed_executable_paths.is_empty()
            && !request
                .executable_path
                .as_ref()
                .map_or(false, |path| policy.allowed_executable_paths.contains(path))
        {
            deny!("executable path not allowed");
        }
        if let Some(hash) = &request.executable_sha256 {
            if policy.denied_executable_sha256.contains(hash) {
                deny!("executable hash explicitly denied");
            }
        }
        if !policy.allowed_executable_sha256.is_empty()
            && !request.executable_sha256.as_ref().map_or(false, |hash| {
                policy.allowed_executable_sha256.contains(hash)
            })
        {
            deny!("executable hash not allowed");
        }
        if request.delivery_mode == DeliveryMode::BrokeredHttp {
            if let Err(result) = self.evaluate_brokered_http(policy, request) {
                return result;
            }
        }
        if request.raw_export_requested && !policy.exportable {
            deny!("raw export not allowed");
        }
        if request.raw_export_requested
            && matches!(
                request.delivery_mode,
                DeliveryMode::Raw | DeliveryMode::TerminalPrint
            )
            && !policy.allow_unbound_plaintext_export
            && !policy_has_peer_binding(policy)
        {
            deny!("raw plaintext export requires a non-spoofable peer binding");
        }
        if assurance_rank(&request.provider_assurance)
            < assurance_rank(&policy.minimum_provider_assurance)
        {
            deny!("provider assurance too low");
        }
        if policy.require_mfa && !request.mfa_verified {
            deny!("MFA required");
        }
        EvaluationResult {
            decision: PolicyDecision::Allow,
            reason: "allowed".to_string(),
            policy: Some(policy),
        }
    }

    fn evaluate_brokered_http<'a>(
        &self,
        policy: &'a AccessPolicy,
        request: &AccessRequest,
    ) -> Result<(), EvaluationResult<'a>> {
        macro_rules! deny {
            ($reason:expr) => {
                return Err(EvaluationResult {
                    decision: PolicyDecision::Deny,
                    reason: $reason.to_string(),
                    policy: Some(policy),
                })
            };
        }
        let Some(host) = request.http_host.as_ref() else {
            deny!("HTTP host required for brokered_http");
        };
        if policy.require_https_for_brokered_http && request.http_scheme.as_deref() != Some("https")
        {
            deny!("HTTPS required for brokered_http");
        }
        if policy.denied_http_hosts.contains(host) {
            deny!("HTTP host explicitly denied");
        }
        if !policy.allowed_http_hosts.is_empty()
            && !policy.allowed_http_hosts.contains(host)
            && !policy.allowed_http_hosts.iter().any(|item| item == "*")
        {
            deny!("HTTP host not allowed");
        }
        let Some(method) = request.http_method.as_ref() else {
            deny!("HTTP method required for brokered_http");
        };
        let method = method.to_ascii_uppercase();
        let allowed_methods = policy
            .allowed_http_methods
            .iter()
            .map(|item| item.to_ascii_uppercase())
            .collect::<Vec<_>>();
        let denied_methods = policy
            .denied_http_methods
            .iter()
            .map(|item| item.to_ascii_uppercase())
            .collect::<Vec<_>>();
        if denied_methods.contains(&method) {
            deny!("HTTP method explicitly denied");
        }
        if !allowed_methods.is_empty()
            && !allowed_methods.contains(&method)
            && !allowed_methods.iter().any(|item| item == "*")
        {
            deny!("HTTP method not allowed");
        }
        let Some(path) = request.http_path.as_ref() else {
            deny!("HTTP path required for brokered_http");
        };
        if policy
            .denied_http_path_prefixes
            .iter()
            .any(|prefix| path.starts_with(prefix))
        {
            deny!("HTTP path explicitly denied");
        }
        if !policy.allowed_http_path_prefixes.is_empty()
            && !policy
                .allowed_http_path_prefixes
                .iter()
                .any(|prefix| path.starts_with(prefix))
        {
            deny!("HTTP path not allowed");
        }
        if let (Some(limit), Some(size)) = (
            policy.max_http_request_body_bytes,
            request.http_request_body_bytes,
        ) {
            if size > limit {
                deny!("HTTP request body too large");
            }
        }
        Ok(())
    }
}

fn policy_has_peer_binding(policy: &AccessPolicy) -> bool {
    !policy.allowed_os_uids.is_empty()
        || !policy.allowed_executable_paths.is_empty()
        || !policy.allowed_executable_sha256.is_empty()
}

fn assurance_rank(level: &str) -> i32 {
    match level {
        "A0" => 0,
        "A1" => 1,
        "A2" => 2,
        "A3" => 3,
        "A4" => 4,
        "A5" => 5,
        _ => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn denies_by_default() {
        assert_eq!(
            PolicyEngine::new(vec![]).evaluate(&request()).decision,
            PolicyDecision::Deny
        );
    }

    #[test]
    fn allows_exact_match() {
        let policy = AccessPolicy {
            policy_id: "p1".to_string(),
            secret_refs: vec!["secret://default/api".to_string()],
            allowed_consumers: vec!["cli".to_string()],
            allowed_purposes: vec!["deploy".to_string()],
            allowed_delivery_modes: vec![DeliveryMode::TerminalPrint],
            exportable: true,
            allow_unbound_plaintext_export: true,
            ..serde_json::from_value(serde_json::json!({"policy_id":"defaults"})).unwrap()
        };

        assert!(PolicyEngine::new(vec![policy])
            .evaluate(&request())
            .allowed());
    }

    #[test]
    fn raw_plaintext_export_requires_peer_binding_by_default() {
        let policy = AccessPolicy {
            policy_id: "p1".to_string(),
            secret_refs: vec!["secret://default/api".to_string()],
            allowed_consumers: vec!["cli".to_string()],
            allowed_purposes: vec!["deploy".to_string()],
            allowed_delivery_modes: vec![DeliveryMode::TerminalPrint],
            exportable: true,
            ..serde_json::from_value(serde_json::json!({"policy_id":"defaults"})).unwrap()
        };
        let engine = PolicyEngine::new(vec![policy.clone()]);
        assert_eq!(engine.evaluate(&request()).decision, PolicyDecision::Deny);

        let mut bound_policy = policy;
        bound_policy.allowed_os_uids = vec![1000];
        let mut bound_request = request();
        bound_request.os_uid = Some(1000);
        assert!(PolicyEngine::new(vec![bound_policy])
            .evaluate(&bound_request)
            .allowed());
    }

    #[test]
    fn require_mfa_denies_until_request_is_verified() {
        let policy = AccessPolicy {
            policy_id: "p1".to_string(),
            secret_refs: vec!["secret://default/api".to_string()],
            allowed_consumers: vec!["cli".to_string()],
            allowed_purposes: vec!["deploy".to_string()],
            allowed_delivery_modes: vec![DeliveryMode::TerminalPrint],
            exportable: true,
            allow_unbound_plaintext_export: true,
            require_mfa: true,
            ..serde_json::from_value(serde_json::json!({"policy_id":"defaults"})).unwrap()
        };
        let engine = PolicyEngine::new(vec![policy]);
        let mut request = request();

        let denied = engine.evaluate(&request);
        assert_eq!(denied.decision, PolicyDecision::Deny);

        request.mfa_verified = true;
        assert!(engine.evaluate(&request).allowed());
    }
}
