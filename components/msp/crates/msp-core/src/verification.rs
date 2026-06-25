use crate::{MspResult, MspSchemaKind, parse_and_validate_json_schema, validate_json_schema};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationContract {
    pub msp_version: String,
    pub contract_version: String,
    pub kind: String,
    pub id: String,
    pub skill_id: String,
    #[serde(default)]
    pub skill_version: Option<String>,
    pub checks: Vec<VerificationCheck>,
    pub success_criteria: SuccessCriteria,
    #[serde(default)]
    pub evidence_requirements: Vec<EvidenceRequirement>,
    #[serde(default)]
    pub failure_taxonomy: Vec<FailureTaxonomyEntry>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationCheck {
    pub id: String,
    #[serde(rename = "type")]
    pub check_type: String,
    pub required: bool,
    pub description: String,
    #[serde(default)]
    pub expected: Option<serde_json::Value>,
    #[serde(default)]
    pub evidence_keys: Vec<String>,
    #[serde(default = "default_weight")]
    pub weight: f64,
}

fn default_weight() -> f64 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuccessCriteria {
    pub required_checks_pass: bool,
    #[serde(default)]
    pub minimum_score: Option<f64>,
    #[serde(default)]
    pub minimum_confidence: Option<f64>,
    #[serde(default)]
    pub allowed_warnings: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRequirement {
    pub key: String,
    #[serde(rename = "type")]
    pub evidence_type: String,
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureTaxonomyEntry {
    pub code: String,
    pub description: String,
    #[serde(default)]
    pub severity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub msp_version: String,
    pub kind: String,
    pub id: String,
    pub skill_id: String,
    pub skill_version: String,
    #[serde(default)]
    pub verification_contract_id: Option<String>,
    pub runtime: RuntimeReport,
    pub started_at: String,
    pub completed_at: String,
    pub status: String,
    #[serde(default)]
    pub files_changed: Vec<FileChange>,
    #[serde(default)]
    pub commands_run: Vec<CommandResult>,
    #[serde(default)]
    pub artifacts: Vec<ReportArtifact>,
    #[serde(default)]
    pub policy_decisions: Vec<PolicyDecision>,
    #[serde(default)]
    pub evidence: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub results: BTreeMap<String, String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub errors: Vec<ReportError>,
}

impl ExecutionReport {
    pub fn from_path(path: impl AsRef<Path>) -> MspResult<Self> {
        let content = std::fs::read_to_string(path)?;
        let value = parse_and_validate_json_schema(MspSchemaKind::ExecutionReport, &content)?;
        Ok(serde_json::from_value(value)?)
    }

    pub fn from_json_value(value: serde_json::Value) -> MspResult<Self> {
        validate_json_schema(MspSchemaKind::ExecutionReport, &value)?;
        Ok(serde_json::from_value(value)?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeReport {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub change_type: String,
    #[serde(default)]
    pub before_hash: Option<String>,
    #[serde(default)]
    pub after_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandResult {
    pub command: Vec<String>,
    pub exit_code: i32,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub stdout_excerpt: Option<String>,
    #[serde(default)]
    pub stderr_excerpt: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportArtifact {
    pub uri: String,
    pub media_type: String,
    #[serde(default)]
    pub hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub policy: String,
    pub decision: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub severity: Option<String>,
}

impl VerificationContract {
    pub fn from_path(path: impl AsRef<Path>) -> MspResult<Self> {
        let content = std::fs::read_to_string(path)?;
        let value = parse_and_validate_json_schema(MspSchemaKind::VerificationContract, &content)?;
        Ok(serde_json::from_value(value)?)
    }

    pub fn from_json_value(value: serde_json::Value) -> MspResult<Self> {
        validate_json_schema(MspSchemaKind::VerificationContract, &value)?;
        Ok(serde_json::from_value(value)?)
    }

    pub fn verify_report(&self, report: &ExecutionReport) -> crate::SkillVerificationResult {
        let mut failed_checks = Vec::new();
        let mut warnings = Vec::new();
        let mut failures = Vec::new();
        let mut check_results = Vec::new();
        let mut evidence_results = Vec::new();
        let mut earned = 0.0;
        let mut possible = 0.0;

        if self.skill_id != report.skill_id {
            failures.push(self.failure(
                "skill_id_mismatch",
                format!(
                    "execution report skill_id {} does not match contract skill_id {}",
                    report.skill_id, self.skill_id
                ),
                None,
            ));
            failed_checks.push("skill_id_mismatch".to_string());
        }

        if let Some(expected_version) = self.skill_version.as_ref()
            && expected_version != &report.skill_version
        {
            failures.push(self.failure(
                "skill_version_mismatch",
                format!(
                    "execution report skill_version {} does not match contract skill_version {}",
                    report.skill_version, expected_version
                ),
                None,
            ));
            failed_checks.push("skill_version_mismatch".to_string());
        }

        if let Some(contract_id) = report.verification_contract_id.as_ref()
            && contract_id != &self.id
        {
            failures.push(self.failure(
                "verification_contract_id_mismatch",
                format!(
                    "execution report verification_contract_id {} does not match contract id {}",
                    contract_id, self.id
                ),
                None,
            ));
            failed_checks.push("verification_contract_id_mismatch".to_string());
        }

        if !matches!(
            report.status.as_str(),
            "completed" | "completed_with_warnings" | "not_applicable"
        ) {
            failures.push(self.failure(
                "execution_status_failed",
                format!("execution report status is {}", report.status),
                None,
            ));
            failed_checks.push("execution_status_failed".to_string());
        } else if report.status == "completed_with_warnings" {
            warnings.push("execution_status_completed_with_warnings".to_string());
        } else if report.status == "not_applicable" {
            warnings.push("execution_status_not_applicable".to_string());
        }

        for error in &report.errors {
            let severity = error.severity.as_deref().unwrap_or("error");
            let failure = crate::VerificationFailure {
                code: error.code.clone(),
                message: error.message.clone(),
                severity: Some(severity.to_string()),
                check_id: None,
            };
            if matches!(severity, "error" | "fatal") {
                failed_checks.push(format!("report_error:{}", error.code));
                failures.push(failure);
            } else {
                warnings.push(format!("report_error:{}", error.code));
            }
        }

        for requirement in &self.evidence_requirements {
            let present = report.evidence.contains_key(&requirement.key);
            let mut reasons = Vec::new();
            let mut evidence_warnings = Vec::new();
            if requirement.required && !present {
                let code = format!("missing_evidence:{}", requirement.key);
                failed_checks.push(code.clone());
                reasons.push(format!("required evidence {} is missing", requirement.key));
                failures.push(self.failure(
                    "missing_evidence",
                    format!("required evidence {} is missing", requirement.key),
                    None,
                ));
            } else if !requirement.required && !present {
                evidence_warnings.push(format!("optional evidence {} is missing", requirement.key));
                warnings.push(format!("optional_evidence_missing:{}", requirement.key));
            }
            evidence_results.push(crate::VerificationEvidenceResult {
                key: requirement.key.clone(),
                evidence_type: requirement.evidence_type.clone(),
                required: requirement.required,
                present,
                reasons,
                warnings: evidence_warnings,
            });
        }

        for check in &self.checks {
            possible += check.weight;
            let status = report
                .results
                .get(&check.id)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
            let missing_evidence: Vec<_> = check
                .evidence_keys
                .iter()
                .filter(|key| !report.evidence.contains_key(*key))
                .cloned()
                .collect();
            let mut reasons = Vec::new();
            let mut check_warnings = Vec::new();
            let mut check_passed = false;
            let mut score_earned = 0.0;

            match status.as_str() {
                "passed" if missing_evidence.is_empty() => {
                    check_passed = true;
                    score_earned = check.weight;
                    earned += check.weight;
                }
                "passed" => {
                    reasons.push(format!(
                        "check {} reported passed but required check evidence is missing: {}",
                        check.id,
                        missing_evidence.join(", ")
                    ));
                    if check.required {
                        failed_checks.push(check.id.clone());
                    } else {
                        check_warnings.push("optional check missing evidence".to_string());
                        warnings.push(check.id.clone());
                    }
                }
                "warning" => {
                    check_warnings.push(format!("check {} reported warning", check.id));
                    warnings.push(check.id.clone());
                    if check.required {
                        reasons.push(format!("required check {} reported warning", check.id));
                        failed_checks.push(check.id.clone());
                    }
                }
                "skipped" if !check.required => {
                    check_warnings.push(format!("optional check {} was skipped", check.id));
                    warnings.push(check.id.clone());
                }
                "skipped" => {
                    reasons.push(format!("required check {} was skipped", check.id));
                    failed_checks.push(check.id.clone());
                }
                "failed" => {
                    reasons.push(format!("check {} failed", check.id));
                    if check.required {
                        failed_checks.push(check.id.clone());
                    } else {
                        check_warnings.push(format!("optional check {} failed", check.id));
                        warnings.push(check.id.clone());
                    }
                }
                _ if check.required => {
                    reasons.push(format!(
                        "required check {} has unknown or missing result status {}",
                        check.id, status
                    ));
                    failed_checks.push(check.id.clone());
                }
                _ => {
                    check_warnings.push(format!(
                        "optional check {} has unknown or missing result status {}",
                        check.id, status
                    ));
                    warnings.push(check.id.clone());
                }
            }

            if check.required && !check_passed && !reasons.is_empty() {
                for reason in &reasons {
                    failures.push(self.failure(&check.id, reason.clone(), Some(check.id.clone())));
                }
            }

            check_results.push(crate::VerificationCheckResult {
                id: check.id.clone(),
                check_type: check.check_type.clone(),
                required: check.required,
                status,
                passed: check_passed,
                score_earned,
                score_possible: check.weight,
                evidence_keys: check.evidence_keys.clone(),
                missing_evidence,
                reasons,
                warnings: check_warnings,
            });
        }

        failed_checks.sort();
        failed_checks.dedup();
        warnings.sort();
        warnings.dedup();

        let score = if possible > 0.0 {
            earned / possible
        } else {
            0.0
        };
        let required_checks_passed = failed_checks.is_empty();
        let minimum_score = self.success_criteria.minimum_score.unwrap_or(1.0);
        let score_passed = score >= minimum_score;
        let evidence_required = self
            .evidence_requirements
            .iter()
            .filter(|requirement| requirement.required)
            .count();
        let evidence_present = self
            .evidence_requirements
            .iter()
            .filter(|requirement| {
                requirement.required && report.evidence.contains_key(&requirement.key)
            })
            .count();
        let confidence = if evidence_required == 0 {
            if report
                .errors
                .iter()
                .any(|error| error.severity.as_deref() == Some("fatal"))
            {
                0.0
            } else {
                1.0
            }
        } else {
            evidence_present as f64 / evidence_required as f64
        };
        let confidence_passed = self
            .success_criteria
            .minimum_confidence
            .is_none_or(|minimum| confidence >= minimum);
        let warning_count = warnings.len() as u64;
        let warnings_passed = self
            .success_criteria
            .allowed_warnings
            .is_none_or(|limit| warning_count <= limit);
        let criteria = crate::VerificationCriteriaResult {
            required_checks_passed,
            minimum_score,
            score_passed,
            minimum_confidence: self.success_criteria.minimum_confidence,
            confidence,
            confidence_passed,
            allowed_warnings: self.success_criteria.allowed_warnings,
            warning_count,
            warnings_passed,
        };
        let passed = required_checks_passed && score_passed && confidence_passed && warnings_passed;

        crate::SkillVerificationResult {
            skill_id: report.skill_id.clone(),
            passed,
            score,
            confidence,
            failed_checks,
            warnings,
            check_results,
            evidence_results,
            criteria,
            failures,
        }
    }

    fn failure(
        &self,
        code: &str,
        message: String,
        check_id: Option<String>,
    ) -> crate::VerificationFailure {
        let taxonomy = self
            .failure_taxonomy
            .iter()
            .find(|entry| entry.code == code);
        crate::VerificationFailure {
            code: code.to_string(),
            message,
            severity: taxonomy.and_then(|entry| entry.severity.clone()),
            check_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> VerificationContract {
        VerificationContract {
            msp_version: "0.1.0".to_string(),
            contract_version: "0.1.0".to_string(),
            kind: "SkillVerificationContract".to_string(),
            id: "verify.test.v1".to_string(),
            skill_id: "skill.test.v1".to_string(),
            skill_version: Some("0.1.0".to_string()),
            checks: vec![VerificationCheck {
                id: "tests_pass".to_string(),
                check_type: "command_result".to_string(),
                required: true,
                description: "Tests pass".to_string(),
                expected: None,
                evidence_keys: vec!["test_log".to_string()],
                weight: 1.0,
            }],
            success_criteria: SuccessCriteria {
                required_checks_pass: true,
                minimum_score: Some(1.0),
                minimum_confidence: Some(1.0),
                allowed_warnings: Some(0),
            },
            evidence_requirements: vec![EvidenceRequirement {
                key: "test_log".to_string(),
                evidence_type: "test_result".to_string(),
                required: true,
                description: None,
            }],
            failure_taxonomy: vec![FailureTaxonomyEntry {
                code: "missing_evidence".to_string(),
                description: "Evidence missing".to_string(),
                severity: Some("error".to_string()),
            }],
            notes: None,
        }
    }

    fn report() -> ExecutionReport {
        ExecutionReport {
            msp_version: "0.1.0".to_string(),
            kind: "SkillExecutionReport".to_string(),
            id: "exec.test.1".to_string(),
            skill_id: "skill.test.v1".to_string(),
            skill_version: "0.1.0".to_string(),
            verification_contract_id: Some("verify.test.v1".to_string()),
            runtime: RuntimeReport {
                name: "test".to_string(),
                version: "0".to_string(),
                model: None,
                capabilities: vec![],
            },
            started_at: "2026-01-01T00:00:00Z".to_string(),
            completed_at: "2026-01-01T00:00:00Z".to_string(),
            status: "completed".to_string(),
            files_changed: vec![],
            commands_run: vec![],
            artifacts: vec![],
            policy_decisions: vec![],
            evidence: BTreeMap::from([("test_log".to_string(), serde_json::json!({"ok": true}))]),
            results: BTreeMap::from([("tests_pass".to_string(), "passed".to_string())]),
            notes: None,
            errors: vec![],
        }
    }

    #[test]
    fn verifies_required_checks() {
        let result = contract().verify_report(&report());
        assert!(result.passed, "{result:?}");
        assert_eq!(result.score, 1.0);
        assert_eq!(result.confidence, 1.0);
        assert!(result.failed_checks.is_empty());
        assert!(result.failures.is_empty());
        assert_eq!(result.check_results[0].id, "tests_pass");
    }

    #[test]
    fn reports_missing_required_evidence_with_failure_details() {
        let mut report = report();
        report.evidence.clear();

        let result = contract().verify_report(&report);

        assert!(!result.passed);
        assert!(
            result
                .failed_checks
                .contains(&"missing_evidence:test_log".to_string())
        );
        assert_eq!(result.confidence, 0.0);
        assert!(
            result
                .failures
                .iter()
                .any(|failure| failure.code == "missing_evidence")
        );
        assert!(!result.evidence_results[0].present);
        assert_eq!(result.check_results[0].missing_evidence, vec!["test_log"]);
    }

    #[test]
    fn reports_contract_and_skill_version_mismatch() {
        let mut report = report();
        report.verification_contract_id = Some("verify.other.v1".to_string());
        report.skill_version = "9.9.9".to_string();

        let result = contract().verify_report(&report);

        assert!(!result.passed);
        assert!(
            result
                .failed_checks
                .contains(&"verification_contract_id_mismatch".to_string())
        );
        assert!(
            result
                .failed_checks
                .contains(&"skill_version_mismatch".to_string())
        );
    }

    #[test]
    fn report_errors_affect_verification_result() {
        let mut report = report();
        report.errors.push(ReportError {
            code: "runtime.failed".to_string(),
            message: "runtime failure".to_string(),
            severity: Some("fatal".to_string()),
        });

        let result = contract().verify_report(&report);

        assert!(!result.passed);
        assert!(
            result
                .failed_checks
                .contains(&"report_error:runtime.failed".to_string())
        );
        assert!(
            result
                .failures
                .iter()
                .any(|failure| failure.code == "runtime.failed")
        );
    }
}
