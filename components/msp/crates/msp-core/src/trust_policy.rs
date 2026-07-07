use crate::{
    MspError, MspResult, MspSchemaKind, ReviewStatus, RiskLevel, SkillManifest, SkillPackManifest,
    parse_and_validate_json_schema,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustAction {
    Allow,
    Deny,
    RequireReview,
    RequireApproval,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedRegistry {
    pub id: String,
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust_anchor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedIssuer {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key_ref: Option<String>,
    #[serde(default = "default_allowed_risk_levels")]
    pub allowed_risk_levels: Vec<RiskLevel>,
}

fn default_allowed_risk_levels() -> Vec<RiskLevel> {
    vec![RiskLevel::Low, RiskLevel::Medium]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignaturePolicy {
    #[serde(default = "default_true")]
    pub require_signed_skills: bool,
    #[serde(default = "default_true")]
    pub require_signed_packs: bool,
    #[serde(default = "default_signature_algorithms")]
    pub allowed_algorithms: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_signature_algorithms() -> Vec<String> {
    vec!["ed25519".to_string()]
}

impl Default for SignaturePolicy {
    fn default() -> Self {
        Self {
            require_signed_skills: true,
            require_signed_packs: true,
            allowed_algorithms: default_signature_algorithms(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_auto_load_risk: Option<RiskLevel>,
    #[serde(default = "default_review_risks")]
    pub require_review_for: Vec<RiskLevel>,
    #[serde(default)]
    pub deny_risk_levels: Vec<RiskLevel>,
}

fn default_review_risks() -> Vec<RiskLevel> {
    vec![RiskLevel::High, RiskLevel::Critical]
}

impl Default for RiskPolicy {
    fn default() -> Self {
        Self {
            max_auto_load_risk: None,
            require_review_for: default_review_risks(),
            deny_risk_levels: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyPolicy {
    #[serde(default = "default_true")]
    pub allow_transitive_dependencies: bool,
    #[serde(default = "default_true")]
    pub require_explicit_dependency_graph: bool,
    #[serde(default = "default_true")]
    pub deny_cycles: bool,
    #[serde(default = "default_true")]
    pub require_dependency_trust_verification: bool,
}

impl Default for DependencyPolicy {
    fn default() -> Self {
        Self {
            allow_transitive_dependencies: true,
            require_explicit_dependency_graph: true,
            deny_cycles: true,
            require_dependency_trust_verification: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryPolicy {
    #[serde(default = "default_true")]
    pub allow_execution_reports: bool,
    #[serde(default = "default_true")]
    pub allow_failure_reports: bool,
    #[serde(default)]
    pub allow_content_upload: bool,
    #[serde(default)]
    pub redact_paths: bool,
}

impl Default for TelemetryPolicy {
    fn default() -> Self {
        Self {
            allow_execution_reports: true,
            allow_failure_reports: true,
            allow_content_upload: false,
            redact_paths: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrustPolicyRule {
    pub id: String,
    pub effect: TrustAction,
    #[serde(rename = "match")]
    pub match_clause: serde_json::Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillTrustPolicy {
    pub msp_version: String,
    pub policy_version: String,
    pub kind: String,
    pub id: String,
    pub name: String,
    pub default_action: TrustAction,
    #[serde(default)]
    pub trusted_registries: Vec<TrustedRegistry>,
    #[serde(default)]
    pub trusted_issuers: Vec<TrustedIssuer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<SignaturePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<RiskPolicy>,
    #[serde(default)]
    pub rules: Vec<TrustPolicyRule>,
    #[serde(default = "default_forbidden_behaviors")]
    pub forbidden_behaviors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_policy: Option<DependencyPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry_policy: Option<TelemetryPolicy>,
}

fn default_forbidden_behaviors() -> Vec<String> {
    [
        "override_host_policy",
        "override_user_policy",
        "grant_tool_permissions",
        "silently_load_dependencies",
        "alter_memory_policy",
        "alter_identity_policy",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrustPolicyEvaluation {
    pub skill_id: String,
    pub action: TrustAction,
    pub allowed: bool,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
    pub matched_rules: Vec<String>,
}

impl SkillTrustPolicy {
    pub fn from_path(path: impl AsRef<Path>) -> MspResult<Self> {
        let content = std::fs::read_to_string(path)?;
        let value = parse_and_validate_json_schema(MspSchemaKind::TrustPolicy, &content)?;
        Self::from_schema_validated_value(value)
    }

    pub fn from_json_value(value: Value) -> MspResult<Self> {
        crate::validate_json_schema(MspSchemaKind::TrustPolicy, &value)?;
        Self::from_schema_validated_value(value)
    }

    fn from_schema_validated_value(value: Value) -> MspResult<Self> {
        let policy: Self = serde_json::from_value(value)?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> MspResult<()> {
        if self.kind != "SkillTrustPolicy" {
            return Err(MspError::Trust(format!(
                "expected kind SkillTrustPolicy, got {}",
                self.kind
            )));
        }
        if self.id.trim().is_empty() || self.name.trim().is_empty() {
            return Err(MspError::Trust(
                "trust policy id and name must be non-empty".to_string(),
            ));
        }
        Ok(())
    }

    pub fn evaluate_skill(&self, manifest: &SkillManifest) -> TrustPolicyEvaluation {
        let mut action = self.default_action;
        let mut reasons = Vec::new();
        let mut warnings = Vec::new();
        let mut matched_rules = Vec::new();

        match manifest.trust.review_status {
            ReviewStatus::Revoked => push_decision(
                &mut action,
                TrustAction::Deny,
                &mut reasons,
                "skill review status is revoked",
            ),
            ReviewStatus::Deprecated => push_decision(
                &mut action,
                TrustAction::RequireReview,
                &mut reasons,
                "skill review status is deprecated",
            ),
            _ => {}
        }

        if let Some(risk) = self.risk.as_ref() {
            if risk.deny_risk_levels.contains(&manifest.trust.risk_level) {
                push_decision(
                    &mut action,
                    TrustAction::Deny,
                    &mut reasons,
                    &format!(
                        "risk level {:?} is denied by policy",
                        manifest.trust.risk_level
                    ),
                );
            }
            if risk.require_review_for.contains(&manifest.trust.risk_level) {
                push_decision(
                    &mut action,
                    TrustAction::RequireReview,
                    &mut reasons,
                    &format!(
                        "risk level {:?} requires review by policy",
                        manifest.trust.risk_level
                    ),
                );
            }
            if let Some(max_auto_load) = risk.max_auto_load_risk
                && manifest.trust.risk_level > max_auto_load
            {
                push_decision(
                    &mut action,
                    TrustAction::RequireReview,
                    &mut reasons,
                    &format!(
                        "risk level {:?} exceeds max_auto_load_risk {:?}",
                        manifest.trust.risk_level, max_auto_load
                    ),
                );
            }
        }

        if let Some(signature) = self.signature.as_ref()
            && signature.require_signed_skills
            && !manifest.trust.signed
        {
            push_decision(
                &mut action,
                TrustAction::RequireApproval,
                &mut reasons,
                "policy requires signed skills but skill is unsigned",
            );
        }

        if !self.trusted_issuers.is_empty() {
            match self
                .trusted_issuers
                .iter()
                .find(|issuer| issuer.id == manifest.trust.issuer)
            {
                Some(issuer)
                    if !issuer.allowed_risk_levels.is_empty()
                        && !issuer
                            .allowed_risk_levels
                            .contains(&manifest.trust.risk_level) =>
                {
                    push_decision(
                        &mut action,
                        TrustAction::RequireApproval,
                        &mut reasons,
                        &format!(
                            "issuer {} is trusted but not for risk level {:?}",
                            issuer.id, manifest.trust.risk_level
                        ),
                    );
                }
                Some(_) => {}
                None => push_decision(
                    &mut action,
                    TrustAction::RequireApproval,
                    &mut reasons,
                    &format!("issuer {} is not trusted by policy", manifest.trust.issuer),
                ),
            }
        }

        for behavior in &self.forbidden_behaviors {
            if !manifest.trust.forbidden_behaviors.contains(behavior) {
                warnings.push(format!(
                    "skill does not explicitly forbid policy behavior: {behavior}"
                ));
            }
        }

        for rule in &self.rules {
            if rule_matches(rule, manifest) {
                matched_rules.push(rule.id.clone());
                let reason = rule
                    .reason
                    .clone()
                    .unwrap_or_else(|| format!("matched trust policy rule {}", rule.id));
                push_decision(&mut action, rule.effect, &mut reasons, &reason);
            }
        }

        TrustPolicyEvaluation {
            skill_id: manifest.id.clone(),
            action,
            allowed: action == TrustAction::Allow,
            reasons,
            warnings,
            matched_rules,
        }
    }

    pub fn evaluate_pack(&self, manifest: &SkillPackManifest) -> TrustPolicyEvaluation {
        let mut action = self.default_action;
        let mut reasons = Vec::new();
        let warnings = Vec::new();
        let mut matched_rules = Vec::new();

        match manifest.trust.review_status {
            ReviewStatus::Revoked => push_decision(
                &mut action,
                TrustAction::Deny,
                &mut reasons,
                "pack review status is revoked",
            ),
            ReviewStatus::Deprecated => push_decision(
                &mut action,
                TrustAction::RequireReview,
                &mut reasons,
                "pack review status is deprecated",
            ),
            _ => {}
        }

        if let Some(risk) = self.risk.as_ref() {
            if risk.deny_risk_levels.contains(&manifest.trust.risk_level) {
                push_decision(
                    &mut action,
                    TrustAction::Deny,
                    &mut reasons,
                    &format!(
                        "risk level {:?} is denied by policy",
                        manifest.trust.risk_level
                    ),
                );
            }
            if risk.require_review_for.contains(&manifest.trust.risk_level) {
                push_decision(
                    &mut action,
                    TrustAction::RequireReview,
                    &mut reasons,
                    &format!(
                        "risk level {:?} requires review by policy",
                        manifest.trust.risk_level
                    ),
                );
            }
            if let Some(max_auto_load) = risk.max_auto_load_risk
                && manifest.trust.risk_level > max_auto_load
            {
                push_decision(
                    &mut action,
                    TrustAction::RequireReview,
                    &mut reasons,
                    &format!(
                        "risk level {:?} exceeds max_auto_load_risk {:?}",
                        manifest.trust.risk_level, max_auto_load
                    ),
                );
            }
        }

        if let Some(signature) = self.signature.as_ref()
            && signature.require_signed_packs
            && !manifest.trust.signed
        {
            push_decision(
                &mut action,
                TrustAction::RequireApproval,
                &mut reasons,
                "policy requires signed packs but pack is unsigned",
            );
        }

        if !self.trusted_issuers.is_empty() {
            match self
                .trusted_issuers
                .iter()
                .find(|issuer| issuer.id == manifest.trust.issuer)
            {
                Some(issuer)
                    if !issuer.allowed_risk_levels.is_empty()
                        && !issuer
                            .allowed_risk_levels
                            .contains(&manifest.trust.risk_level) =>
                {
                    push_decision(
                        &mut action,
                        TrustAction::RequireApproval,
                        &mut reasons,
                        &format!(
                            "issuer {} is trusted but not for risk level {:?}",
                            issuer.id, manifest.trust.risk_level
                        ),
                    );
                }
                Some(_) => {}
                None => push_decision(
                    &mut action,
                    TrustAction::RequireApproval,
                    &mut reasons,
                    &format!("issuer {} is not trusted by policy", manifest.trust.issuer),
                ),
            }
        }

        for rule in &self.rules {
            if rule_matches_pack(rule, manifest) {
                matched_rules.push(rule.id.clone());
                let reason = rule
                    .reason
                    .clone()
                    .unwrap_or_else(|| format!("matched trust policy rule {}", rule.id));
                push_decision(&mut action, rule.effect, &mut reasons, &reason);
            }
        }

        TrustPolicyEvaluation {
            skill_id: manifest.id.clone(),
            action,
            allowed: action == TrustAction::Allow,
            reasons,
            warnings,
            matched_rules,
        }
    }
}

fn push_decision(
    current: &mut TrustAction,
    candidate: TrustAction,
    reasons: &mut Vec<String>,
    reason: &str,
) {
    if precedence(candidate) > precedence(*current) {
        *current = candidate;
    }
    reasons.push(reason.to_string());
}

fn precedence(action: TrustAction) -> u8 {
    match action {
        TrustAction::Allow => 0,
        TrustAction::RequireReview => 1,
        TrustAction::RequireApproval => 2,
        TrustAction::Deny => 3,
    }
}

fn rule_matches(rule: &TrustPolicyRule, manifest: &SkillManifest) -> bool {
    rule.match_clause
        .iter()
        .all(|(key, expected)| match key.as_str() {
            "id" | "skill_id" => expected.as_str() == Some(manifest.id.as_str()),
            "issuer" => expected.as_str() == Some(manifest.trust.issuer.as_str()),
            "category" => expected.as_str() == Some(manifest.category.as_str()),
            "category_prefix" => expected
                .as_str()
                .is_some_and(|prefix| manifest.category.starts_with(prefix)),
            "risk_level" | "risk" => expected
                .as_str()
                .is_some_and(|risk| risk == risk_level_name(manifest.trust.risk_level)),
            "risk_at_least" => expected
                .as_str()
                .and_then(parse_risk_level)
                .is_some_and(|risk| manifest.trust.risk_level >= risk),
            "signed" => expected.as_bool() == Some(manifest.trust.signed),
            "permission" => expected.as_str().is_some_and(|permission| {
                manifest
                    .requirements
                    .permissions
                    .iter()
                    .any(|p| p == permission)
            }),
            "permissions_all" => expected.as_array().is_some_and(|items| {
                items.iter().filter_map(Value::as_str).all(|permission| {
                    manifest
                        .requirements
                        .permissions
                        .iter()
                        .any(|existing| existing == permission)
                })
            }),
            "permissions_any" => expected.as_array().is_some_and(|items| {
                items.iter().filter_map(Value::as_str).any(|permission| {
                    manifest
                        .requirements
                        .permissions
                        .iter()
                        .any(|existing| existing == permission)
                })
            }),
            _ => false,
        })
}

fn rule_matches_pack(rule: &TrustPolicyRule, manifest: &SkillPackManifest) -> bool {
    rule.match_clause
        .iter()
        .all(|(key, expected)| match key.as_str() {
            "id" | "pack_id" => expected.as_str() == Some(manifest.id.as_str()),
            "issuer" => expected.as_str() == Some(manifest.trust.issuer.as_str()),
            "category" => expected.as_str() == Some(manifest.category.as_str()),
            "category_prefix" => expected
                .as_str()
                .is_some_and(|prefix| manifest.category.starts_with(prefix)),
            "risk_level" | "risk" => expected
                .as_str()
                .is_some_and(|risk| risk == risk_level_name(manifest.trust.risk_level)),
            "risk_at_least" => expected
                .as_str()
                .and_then(parse_risk_level)
                .is_some_and(|risk| manifest.trust.risk_level >= risk),
            "signed" => expected.as_bool() == Some(manifest.trust.signed),
            _ => false,
        })
}

fn parse_risk_level(value: &str) -> Option<RiskLevel> {
    match value {
        "low" => Some(RiskLevel::Low),
        "medium" => Some(RiskLevel::Medium),
        "high" => Some(RiskLevel::High),
        "critical" => Some(RiskLevel::Critical),
        _ => None,
    }
}

fn risk_level_name(value: RiskLevel) -> &'static str {
    match value {
        RiskLevel::Low => "low",
        RiskLevel::Medium => "medium",
        RiskLevel::High => "high",
        RiskLevel::Critical => "critical",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Activation, ArtifactRef, HashDigest, Requirements, SchemaRefs, SkillArtifacts,
        TrustMetadata, VerificationSummary,
    };
    use std::collections::BTreeMap;

    fn manifest() -> SkillManifest {
        SkillManifest {
            msp_version: "0.1.0".to_string(),
            manifest_version: "0.1.0".to_string(),
            kind: "SkillManifest".to_string(),
            id: "skill.test.example.v1".to_string(),
            name: "Example".to_string(),
            version: "0.1.0".to_string(),
            category: "test/example".to_string(),
            summary: "Example".to_string(),
            description: None,
            keywords: vec![],
            formats: vec!["markdown".to_string()],
            primary_format: "markdown".to_string(),
            activation: Activation {
                task_patterns: vec!["test".to_string()],
                negative_patterns: vec![],
                required_context: vec![],
                optional_context: vec![],
                domains: vec![],
                languages: vec![],
            },
            requirements: Requirements {
                permissions: vec!["workspace_write".to_string()],
                ..Requirements::default()
            },
            schemas: SchemaRefs {
                input: "msp://schemas/in".to_string(),
                output: "msp://schemas/out".to_string(),
                context: None,
            },
            verification: VerificationSummary {
                contract: None,
                required_checks: vec!["manual_review".to_string()],
                optional_checks: vec![],
                minimum_evidence: vec![],
            },
            trust: TrustMetadata {
                hash: HashDigest::parse("sha256:abcd").unwrap(),
                signed: false,
                signature: None,
                issuer: "issuer.local".to_string(),
                license: None,
                review_status: ReviewStatus::SelfReviewed,
                risk_level: RiskLevel::Medium,
                forbidden_behaviors: default_forbidden_behaviors(),
                sandbox_profile: None,
            },
            dependencies: vec![],
            artifacts: SkillArtifacts {
                body: ArtifactRef {
                    uri: "skill.md".to_string(),
                    media_type: "text/markdown".to_string(),
                    hash: HashDigest::parse("sha256:abcd").unwrap(),
                    size_bytes: None,
                },
                verification_contract: None,
                examples: vec![],
                schemas: vec![],
            },
            provenance: None,
            compatibility: None,
            deprecation: None,
            extensions: BTreeMap::new(),
        }
    }

    #[test]
    fn denies_matching_rule() {
        let policy = SkillTrustPolicy {
            msp_version: "0.1.0".to_string(),
            policy_version: "0.1.0".to_string(),
            kind: "SkillTrustPolicy".to_string(),
            id: "trust.test.v1".to_string(),
            name: "Test".to_string(),
            default_action: TrustAction::Allow,
            trusted_registries: vec![],
            trusted_issuers: vec![],
            signature: None,
            risk: None,
            rules: vec![TrustPolicyRule {
                id: "deny-writes".to_string(),
                effect: TrustAction::Deny,
                match_clause: serde_json::Map::from_iter([(
                    "permission".to_string(),
                    Value::String("workspace_write".to_string()),
                )]),
                reason: Some("write permission denied".to_string()),
            }],
            forbidden_behaviors: vec![],
            dependency_policy: None,
            telemetry_policy: None,
        };
        let result = policy.evaluate_skill(&manifest());
        assert_eq!(result.action, TrustAction::Deny);
        assert!(!result.allowed);
        assert_eq!(result.matched_rules, vec!["deny-writes"]);
    }
}
