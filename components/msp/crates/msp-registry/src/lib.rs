//! Local filesystem MSP registry.

use msp_core::{
    CompatibilityIssue, CompatibilitySeverity, DependencyNode, DependencyResolution,
    DependencyResolutionResult, DependencyTrustEvaluationNode, DependencyTrustEvaluationResult,
    DependencyType, ExecutionReport, HashAlgorithm, HashDigest, MspError, MspResult,
    PackMemberValidationNode, PackMemberValidationResult, PackSearchQuery, PackSearchResult,
    RegistrySearchResult, ResolutionStrategy, RuntimeCompatibilityQuery, SkillCompatibilityResult,
    SkillDependency, SkillLoadResult, SkillManifest, SkillPackManifest, SkillSearchQuery,
    SkillTrustPolicy, SkillVerificationResult, TrustAction, TrustMetadata, TrustPolicyEvaluation,
    TrustVerifyResult, VerificationContract, verify_signature_bytes, verify_signature_file,
};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct LocalRegistry {
    root: PathBuf,
    skills: BTreeMap<String, RegistrySkill>,
    packs: BTreeMap<String, RegistryPack>,
}

#[derive(Debug, Clone)]
pub struct RegistrySkill {
    pub manifest_path: PathBuf,
    pub manifest: SkillManifest,
}

#[derive(Debug, Clone)]
pub struct RegistryPack {
    pub manifest_path: PathBuf,
    pub manifest: SkillPackManifest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DependencyRootKind {
    Skill,
    Pack,
}

struct DependencyTrustWalk<'a> {
    result: &'a mut DependencyTrustEvaluationResult,
    visiting: BTreeSet<String>,
    visited: BTreeSet<String>,
    stack: Vec<String>,
}

impl LocalRegistry {
    pub fn open(root: impl Into<PathBuf>) -> MspResult<Self> {
        let root = root.into();
        let mut registry = Self {
            root,
            skills: BTreeMap::new(),
            packs: BTreeMap::new(),
        };
        registry.reindex()?;
        Ok(registry)
    }

    pub fn empty(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            skills: BTreeMap::new(),
            packs: BTreeMap::new(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn skill_count(&self) -> usize {
        self.skills.len()
    }

    pub fn pack_count(&self) -> usize {
        self.packs.len()
    }

    pub fn reindex(&mut self) -> MspResult<()> {
        self.skills.clear();
        self.packs.clear();
        if !self.root.exists() {
            return Ok(());
        }

        for entry in WalkDir::new(&self.root).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            match file_name {
                "skill.manifest.json" => {
                    let manifest = SkillManifest::from_path(path)?;
                    self.skills.insert(
                        manifest.id.clone(),
                        RegistrySkill {
                            manifest_path: path.to_path_buf(),
                            manifest,
                        },
                    );
                }
                "pack.manifest.json" => {
                    let manifest = SkillPackManifest::from_path(path)?;
                    self.packs.insert(
                        manifest.id.clone(),
                        RegistryPack {
                            manifest_path: path.to_path_buf(),
                            manifest,
                        },
                    );
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn skills(&self) -> impl Iterator<Item = &RegistrySkill> {
        self.skills.values()
    }

    pub fn packs(&self) -> impl Iterator<Item = &RegistryPack> {
        self.packs.values()
    }

    pub fn get_manifest(&self, id: &str) -> MspResult<SkillManifest> {
        self.skills
            .get(id)
            .map(|skill| skill.manifest.clone())
            .ok_or_else(|| MspError::SkillNotFound(id.to_string()))
    }

    pub fn get_pack_manifest(&self, id: &str) -> MspResult<SkillPackManifest> {
        self.packs
            .get(id)
            .map(|pack| pack.manifest.clone())
            .ok_or_else(|| MspError::PackNotFound(id.to_string()))
    }

    pub fn search(&self, query: &SkillSearchQuery) -> Vec<RegistrySearchResult> {
        let mut results: Vec<_> = self
            .skills
            .values()
            .filter_map(|skill| {
                let score = score_manifest(&skill.manifest, query);
                (score > 0).then(|| RegistrySearchResult::from_manifest(&skill.manifest, score))
            })
            .collect();
        results.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        results
    }

    pub fn discover_packs(&self, query: &PackSearchQuery) -> Vec<PackSearchResult> {
        let mut results: Vec<_> = self
            .packs
            .values()
            .filter_map(|pack| {
                let score = score_pack_manifest(&pack.manifest, query);
                (score > 0).then(|| PackSearchResult::from_manifest(&pack.manifest, score))
            })
            .collect();
        results.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        results
    }

    pub fn load_skill(&self, id: &str) -> MspResult<SkillLoadResult> {
        let skill = self
            .skills
            .get(id)
            .ok_or_else(|| MspError::SkillNotFound(id.to_string()))?;
        let body_path =
            self.resolve_artifact_path(&skill.manifest_path, &skill.manifest.artifacts.body.uri)?;
        let body = std::fs::read_to_string(&body_path)?;
        let actual_hash =
            HashDigest::from_file(&body_path, skill.manifest.artifacts.body.hash.algorithm)?;
        let body_hash_valid = actual_hash == skill.manifest.artifacts.body.hash;
        if !body_hash_valid {
            return Err(MspError::HashMismatch {
                artifact: body_path.display().to_string(),
                expected: skill.manifest.artifacts.body.hash.to_string(),
                actual: actual_hash.to_string(),
            });
        }

        let verification_contract =
            if let Some(artifact) = &skill.manifest.artifacts.verification_contract {
                let path = self.resolve_artifact_path(&skill.manifest_path, &artifact.uri)?;
                let contract = VerificationContract::from_path(path)?;
                Some(serde_json::to_value(contract)?)
            } else {
                None
            };

        Ok(SkillLoadResult {
            manifest: skill.manifest.clone(),
            body,
            body_hash_valid,
            verification_contract,
            dependency_ids: skill
                .manifest
                .dependencies
                .iter()
                .map(|dependency| dependency.id.clone())
                .collect(),
        })
    }

    pub fn load_pack(&self, id: &str) -> MspResult<SkillPackManifest> {
        self.get_pack_manifest(id)
    }

    pub fn check_skill_compatibility(
        &self,
        id: &str,
        query: &RuntimeCompatibilityQuery,
    ) -> MspResult<SkillCompatibilityResult> {
        let skill = self
            .skills
            .get(id)
            .ok_or_else(|| MspError::SkillNotFound(id.to_string()))?;
        Ok(evaluate_skill_compatibility(&skill.manifest, query))
    }

    pub fn validate_pack_members(&self, id: &str) -> MspResult<PackMemberValidationResult> {
        self.validate_pack_members_with_policy(id, None)
    }

    pub fn validate_pack_members_with_policy(
        &self,
        id: &str,
        policy: Option<&SkillTrustPolicy>,
    ) -> MspResult<PackMemberValidationResult> {
        let pack = self
            .packs
            .get(id)
            .ok_or_else(|| MspError::PackNotFound(id.to_string()))?;

        let mut result = PackMemberValidationResult {
            pack_id: id.to_string(),
            valid: true,
            members: Vec::new(),
            missing: Vec::new(),
            duplicate_ids: Vec::new(),
            reasons: Vec::new(),
            warnings: Vec::new(),
        };
        let mut seen_ids = BTreeSet::new();

        for member in &pack.manifest.skills {
            if !seen_ids.insert(member.id.clone()) {
                result.duplicate_ids.push(member.id.clone());
                result
                    .reasons
                    .push(format!("pack declares duplicate member id {}", member.id));
            }

            let mut node = PackMemberValidationNode {
                id: member.id.clone(),
                expected_version: member.version.clone(),
                manifest_uri: member.manifest_uri.clone(),
                required: member.required,
                role: member.role.clone(),
                resolved_path: None,
                exists: false,
                indexed: false,
                id_matches: false,
                version_matches: false,
                trust_evaluation: None,
                valid: true,
                reasons: Vec::new(),
                warnings: Vec::new(),
            };

            match self.resolve_registry_relative_path(&member.manifest_uri) {
                Ok(path) => {
                    node.resolved_path = Some(path.display().to_string());
                    node.exists = path.is_file();
                    if !node.exists {
                        if member.required {
                            node.valid = false;
                            node.reasons.push(format!(
                                "required pack member {} manifest is missing at {}",
                                member.id,
                                path.display()
                            ));
                            result.missing.push(member.id.clone());
                        } else {
                            node.warnings.push(format!(
                                "optional pack member {} manifest is missing at {}",
                                member.id,
                                path.display()
                            ));
                        }
                    } else {
                        match SkillManifest::from_path(&path) {
                            Ok(manifest) => {
                                node.id_matches = manifest.id == member.id;
                                node.version_matches = manifest.version == member.version;
                                if !node.id_matches {
                                    node.valid = false;
                                    node.reasons.push(format!(
                                        "pack member id mismatch for {}: manifest contains {}",
                                        member.id, manifest.id
                                    ));
                                }
                                if !node.version_matches {
                                    node.valid = false;
                                    node.reasons.push(format!(
                                        "pack member {} version mismatch: expected {}, got {}",
                                        member.id, member.version, manifest.version
                                    ));
                                }

                                if let Some(indexed) = self.skills.get(&member.id) {
                                    node.indexed = true;
                                    if let (Ok(expected), Ok(actual)) =
                                        (path.canonicalize(), indexed.manifest_path.canonicalize())
                                        && expected != actual
                                    {
                                        node.valid = false;
                                        node.reasons.push(format!(
                                            "pack member {} manifest URI resolves to {}, but registry index points to {}",
                                            member.id,
                                            expected.display(),
                                            actual.display()
                                        ));
                                    }
                                } else {
                                    node.valid = false;
                                    node.reasons.push(format!(
                                        "pack member {} is not indexed in the local registry",
                                        member.id
                                    ));
                                }

                                if let Some(policy) = policy {
                                    match self.evaluate_trust_policy(policy, &member.id) {
                                        Ok(evaluation) => {
                                            if !evaluation.allowed {
                                                node.valid = false;
                                                node.reasons.push(format!(
                                                    "pack member {} is not allowed by policy: {:?}",
                                                    member.id, evaluation.action
                                                ));
                                                node.reasons.extend(evaluation.reasons.clone());
                                            }
                                            node.warnings.extend(evaluation.warnings.clone());
                                            node.trust_evaluation = Some(evaluation);
                                        }
                                        Err(error) => {
                                            node.valid = false;
                                            node.reasons.push(format!(
                                                "failed to evaluate trust for pack member {}: {}",
                                                member.id, error
                                            ));
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                node.valid = false;
                                node.reasons.push(format!(
                                    "failed to load pack member {} manifest at {}: {}",
                                    member.id,
                                    path.display(),
                                    error
                                ));
                            }
                        }
                    }
                }
                Err(error) => {
                    node.valid = false;
                    node.reasons.push(format!(
                        "invalid manifest_uri for pack member {}: {}",
                        member.id, error
                    ));
                }
            }

            if !node.valid {
                result.valid = false;
                result.reasons.extend(node.reasons.clone());
            }
            result.warnings.extend(node.warnings.clone());
            result.members.push(node);
        }

        if !result.duplicate_ids.is_empty() {
            result.valid = false;
        }
        Ok(result)
    }

    pub fn verify_pack_trust(&self, id: &str) -> MspResult<TrustVerifyResult> {
        self.verify_pack_trust_with_allowed_algorithms(id, &["ed25519".to_string()])
    }

    pub fn verify_pack_trust_with_policy(
        &self,
        id: &str,
        policy: &SkillTrustPolicy,
    ) -> MspResult<TrustVerifyResult> {
        let allowed_algorithms = policy
            .signature
            .as_ref()
            .map(|signature| signature.allowed_algorithms.as_slice())
            .unwrap_or(&[]);
        self.verify_pack_trust_with_allowed_algorithms(id, allowed_algorithms)
    }

    fn verify_pack_trust_with_allowed_algorithms(
        &self,
        id: &str,
        allowed_algorithms: &[String],
    ) -> MspResult<TrustVerifyResult> {
        let pack = self
            .packs
            .get(id)
            .ok_or_else(|| MspError::PackNotFound(id.to_string()))?;
        self.verify_pack_manifest_trust(pack, allowed_algorithms)
    }

    pub fn verify_trust(&self, id: &str) -> MspResult<TrustVerifyResult> {
        self.verify_trust_with_allowed_algorithms(id, &["ed25519".to_string()])
    }

    pub fn verify_trust_with_policy(
        &self,
        id: &str,
        policy: &SkillTrustPolicy,
    ) -> MspResult<TrustVerifyResult> {
        let allowed_algorithms = policy
            .signature
            .as_ref()
            .map(|signature| signature.allowed_algorithms.as_slice())
            .unwrap_or(&[]);
        self.verify_trust_with_allowed_algorithms(id, allowed_algorithms)
    }

    fn verify_trust_with_allowed_algorithms(
        &self,
        id: &str,
        allowed_algorithms: &[String],
    ) -> MspResult<TrustVerifyResult> {
        let skill = self
            .skills
            .get(id)
            .ok_or_else(|| MspError::SkillNotFound(id.to_string()))?;
        self.verify_skill_manifest_trust(
            skill,
            skill.manifest.trust.hash.clone(),
            allowed_algorithms,
        )
    }

    pub fn verify_body_artifact(&self, id: &str) -> MspResult<TrustVerifyResult> {
        let skill = self
            .skills
            .get(id)
            .ok_or_else(|| MspError::SkillNotFound(id.to_string()))?;
        self.verify_skill_manifest_trust(
            skill,
            skill.manifest.artifacts.body.hash.clone(),
            &["ed25519".to_string()],
        )
    }

    pub fn evaluate_trust_policy(
        &self,
        policy: &SkillTrustPolicy,
        id: &str,
    ) -> MspResult<TrustPolicyEvaluation> {
        let skill = self
            .skills
            .get(id)
            .ok_or_else(|| MspError::SkillNotFound(id.to_string()))?;
        let mut evaluation = policy.evaluate_skill(&skill.manifest);
        let signature_required = policy
            .signature
            .as_ref()
            .is_some_and(|signature| signature.require_signed_skills);
        let issuer_binding_required = policy.trusted_issuers.iter().any(|issuer| {
            issuer.id == skill.manifest.trust.issuer && issuer.public_key_ref.is_some()
        });

        if signature_required || issuer_binding_required {
            let verification = self.verify_trust_with_policy(id, policy)?;
            if signature_required && !verification.passed {
                evaluation.allowed = false;
                evaluation.action = msp_core::TrustAction::RequireApproval;
                evaluation.reasons.push(
                    "policy requires signed skills but cryptographic verification failed"
                        .to_string(),
                );
                if !verification.hash_passed {
                    evaluation.reasons.push(format!(
                        "hash verification failed: expected {}, got {}",
                        verification.expected_hash, verification.actual_hash
                    ));
                }
                if let Some(signature) = verification.signature.as_ref() {
                    evaluation.reasons.extend(signature.reasons.clone());
                }
            }
            self.evaluate_issuer_key_binding(policy, skill, &verification, &mut evaluation);
        }
        Ok(evaluation)
    }

    pub fn evaluate_pack_trust_policy(
        &self,
        policy: &SkillTrustPolicy,
        id: &str,
    ) -> MspResult<TrustPolicyEvaluation> {
        let pack = self
            .packs
            .get(id)
            .ok_or_else(|| MspError::PackNotFound(id.to_string()))?;
        let mut evaluation = policy.evaluate_pack(&pack.manifest);
        let signature_required = policy
            .signature
            .as_ref()
            .is_some_and(|signature| signature.require_signed_packs);
        let issuer_binding_required = policy.trusted_issuers.iter().any(|issuer| {
            issuer.id == pack.manifest.trust.issuer && issuer.public_key_ref.is_some()
        });

        if signature_required || issuer_binding_required {
            let verification = self.verify_pack_trust_with_policy(id, policy)?;
            if signature_required && !verification.passed {
                evaluation.allowed = false;
                evaluation.action = TrustAction::RequireApproval;
                evaluation.reasons.push(
                    "policy requires signed packs but cryptographic verification failed"
                        .to_string(),
                );
                if !verification.hash_passed {
                    evaluation.reasons.push(format!(
                        "pack hash verification failed: expected {}, got {}",
                        verification.expected_hash, verification.actual_hash
                    ));
                }
                if let Some(signature) = verification.signature.as_ref() {
                    evaluation.reasons.extend(signature.reasons.clone());
                }
            }
            self.evaluate_pack_issuer_key_binding(policy, pack, &verification, &mut evaluation);
        }
        Ok(evaluation)
    }

    pub fn evaluate_dependency_trust(
        &self,
        policy: &SkillTrustPolicy,
        id: &str,
    ) -> MspResult<DependencyTrustEvaluationResult> {
        let root_evaluation = self.evaluate_trust_policy(policy, id)?;
        let mut result = self.new_dependency_trust_result(id, root_evaluation);

        if !result.root_evaluation.allowed {
            result.reasons.push(format!(
                "root skill {} is not allowed by policy: {:?}",
                id, result.root_evaluation.action
            ));
        }

        let mut walk = DependencyTrustWalk {
            result: &mut result,
            visiting: BTreeSet::new(),
            visited: BTreeSet::new(),
            stack: Vec::new(),
        };
        self.evaluate_dependency_trust_inner(policy, id, DependencyRootKind::Skill, 0, &mut walk)?;
        self.finalize_dependency_trust_result(policy, &mut result);

        Ok(result)
    }

    pub fn evaluate_pack_dependency_trust(
        &self,
        policy: &SkillTrustPolicy,
        id: &str,
    ) -> MspResult<DependencyTrustEvaluationResult> {
        let root_evaluation = self.evaluate_pack_trust_policy(policy, id)?;
        let mut result = self.new_dependency_trust_result(id, root_evaluation);

        if !result.root_evaluation.allowed {
            result.reasons.push(format!(
                "root pack {} is not allowed by policy: {:?}",
                id, result.root_evaluation.action
            ));
        }

        let mut walk = DependencyTrustWalk {
            result: &mut result,
            visiting: BTreeSet::new(),
            visited: BTreeSet::new(),
            stack: Vec::new(),
        };
        self.evaluate_dependency_trust_inner(policy, id, DependencyRootKind::Pack, 0, &mut walk)?;
        self.finalize_dependency_trust_result(policy, &mut result);

        Ok(result)
    }

    fn new_dependency_trust_result(
        &self,
        id: &str,
        root_evaluation: TrustPolicyEvaluation,
    ) -> DependencyTrustEvaluationResult {
        DependencyTrustEvaluationResult {
            root: id.to_string(),
            allowed: root_evaluation.allowed,
            root_evaluation,
            dependencies: Vec::new(),
            missing: Vec::new(),
            cycles: Vec::new(),
            reasons: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn finalize_dependency_trust_result(
        &self,
        policy: &SkillTrustPolicy,
        result: &mut DependencyTrustEvaluationResult,
    ) {
        if let Some(dependency_policy) = policy.dependency_policy.as_ref() {
            if dependency_policy.deny_cycles && !result.cycles.is_empty() {
                result.allowed = false;
                result
                    .reasons
                    .push("dependency policy denies dependency cycles".to_string());
            }
            if !dependency_policy.allow_transitive_dependencies
                && result.dependencies.iter().any(|node| node.depth > 1)
            {
                result.allowed = false;
                result
                    .reasons
                    .push("dependency policy denies transitive dependencies".to_string());
            }
        }

        if !result.missing.is_empty() {
            result.allowed = false;
        }
        if result.dependencies.iter().any(|node| !node.allowed) {
            result.allowed = false;
        }
    }

    pub fn resolve_dependencies(&self, id: &str) -> MspResult<DependencyResolutionResult> {
        if !self.skills.contains_key(id) {
            return Err(MspError::SkillNotFound(id.to_string()));
        }
        let mut result = DependencyResolutionResult {
            root: id.to_string(),
            nodes: Vec::new(),
            missing: Vec::new(),
            cycles: Vec::new(),
        };
        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        let mut stack = Vec::new();
        self.resolve_dependencies_inner(id, &mut result, &mut visiting, &mut visited, &mut stack)?;
        Ok(result)
    }

    pub fn verify_execution_report(
        &self,
        report: &ExecutionReport,
    ) -> MspResult<SkillVerificationResult> {
        let skill = self
            .skills
            .get(&report.skill_id)
            .ok_or_else(|| MspError::SkillNotFound(report.skill_id.clone()))?;
        let artifact = skill
            .manifest
            .artifacts
            .verification_contract
            .as_ref()
            .ok_or_else(|| {
                MspError::Verification("skill has no verification contract artifact".to_string())
            })?;
        let path = self.resolve_artifact_path(&skill.manifest_path, &artifact.uri)?;
        let contract = VerificationContract::from_path(path)?;
        Ok(contract.verify_report(report))
    }

    fn evaluate_issuer_key_binding(
        &self,
        policy: &SkillTrustPolicy,
        skill: &RegistrySkill,
        verification: &TrustVerifyResult,
        evaluation: &mut TrustPolicyEvaluation,
    ) {
        let Some(trusted_issuer) = policy
            .trusted_issuers
            .iter()
            .find(|issuer| issuer.id == skill.manifest.trust.issuer)
        else {
            return;
        };
        let Some(expected_key_ref) = trusted_issuer.public_key_ref.as_deref() else {
            return;
        };

        let Some(signature) = verification.signature.as_ref() else {
            evaluation.allowed = false;
            evaluation.action = msp_core::TrustAction::RequireApproval;
            evaluation.reasons.push(format!(
                "issuer {} requires public key binding {} but skill is unsigned",
                trusted_issuer.id, expected_key_ref
            ));
            return;
        };

        if !verification.passed {
            evaluation.allowed = false;
            evaluation.action = msp_core::TrustAction::RequireApproval;
            evaluation.reasons.push(format!(
                "issuer {} public key binding requires successful trust verification",
                trusted_issuer.id
            ));
            if !verification.hash_passed {
                evaluation.reasons.push(format!(
                    "hash verification failed: expected {}, got {}",
                    verification.expected_hash, verification.actual_hash
                ));
            }
            evaluation.reasons.extend(signature.reasons.clone());
        }

        let matches_public_key = signature.public_key_ref.as_deref() == Some(expected_key_ref)
            || signature.public_key_sha256.as_deref() == Some(expected_key_ref);
        if !matches_public_key {
            evaluation.allowed = false;
            evaluation.action = msp_core::TrustAction::RequireApproval;
            let actual = signature
                .public_key_ref
                .as_deref()
                .or(signature.public_key_sha256.as_deref())
                .unwrap_or("<unknown>");
            evaluation.reasons.push(format!(
                "issuer {} public key binding mismatch: expected {}, got {}",
                trusted_issuer.id, expected_key_ref, actual
            ));
        }
    }

    fn evaluate_pack_issuer_key_binding(
        &self,
        policy: &SkillTrustPolicy,
        pack: &RegistryPack,
        verification: &TrustVerifyResult,
        evaluation: &mut TrustPolicyEvaluation,
    ) {
        let Some(trusted_issuer) = policy
            .trusted_issuers
            .iter()
            .find(|issuer| issuer.id == pack.manifest.trust.issuer)
        else {
            return;
        };
        let Some(expected_key_ref) = trusted_issuer.public_key_ref.as_deref() else {
            return;
        };

        let Some(signature) = verification.signature.as_ref() else {
            evaluation.allowed = false;
            evaluation.action = TrustAction::RequireApproval;
            evaluation.reasons.push(format!(
                "issuer {} requires public key binding {} but pack is unsigned",
                trusted_issuer.id, expected_key_ref
            ));
            return;
        };

        if !verification.passed {
            evaluation.allowed = false;
            evaluation.action = TrustAction::RequireApproval;
            evaluation.reasons.push(format!(
                "issuer {} public key binding requires successful pack trust verification",
                trusted_issuer.id
            ));
            if !verification.hash_passed {
                evaluation.reasons.push(format!(
                    "pack hash verification failed: expected {}, got {}",
                    verification.expected_hash, verification.actual_hash
                ));
            }
            evaluation.reasons.extend(signature.reasons.clone());
        }

        let matches_public_key = signature.public_key_ref.as_deref() == Some(expected_key_ref)
            || signature.public_key_sha256.as_deref() == Some(expected_key_ref);
        if !matches_public_key {
            evaluation.allowed = false;
            evaluation.action = TrustAction::RequireApproval;
            let actual = signature
                .public_key_ref
                .as_deref()
                .or(signature.public_key_sha256.as_deref())
                .unwrap_or("<unknown>");
            evaluation.reasons.push(format!(
                "issuer {} public key binding mismatch: expected {}, got {}",
                trusted_issuer.id, expected_key_ref, actual
            ));
        }
    }

    fn verify_pack_manifest_trust(
        &self,
        pack: &RegistryPack,
        allowed_algorithms: &[String],
    ) -> MspResult<TrustVerifyResult> {
        let artifact = pack.manifest_path.display().to_string();
        let canonical_bytes = canonical_pack_trust_bytes(&pack.manifest)?;
        let actual = HashDigest::from_bytes(pack.manifest.trust.hash.algorithm, &canonical_bytes);
        let hash_passed = actual == pack.manifest.trust.hash;
        let signature = pack.manifest.trust.signed.then(|| {
            verify_signature_bytes(
                artifact.clone(),
                &canonical_bytes,
                pack.manifest.trust.signed,
                pack.manifest.trust.signature.as_deref(),
                allowed_algorithms,
            )
        });
        let signature_passed = signature.as_ref().is_none_or(|signature| signature.passed);
        Ok(TrustVerifyResult {
            artifact,
            expected_hash: pack.manifest.trust.hash.to_string(),
            actual_hash: actual.to_string(),
            hash_passed,
            signature,
            passed: hash_passed && signature_passed,
        })
    }

    fn verify_skill_manifest_trust(
        &self,
        skill: &RegistrySkill,
        expected_hash: HashDigest,
        allowed_algorithms: &[String],
    ) -> MspResult<TrustVerifyResult> {
        let path =
            self.resolve_artifact_path(&skill.manifest_path, &skill.manifest.artifacts.body.uri)?;
        let actual = HashDigest::from_file(&path, expected_hash.algorithm)?;
        let hash_passed = actual == expected_hash;
        let signature = skill
            .manifest
            .trust
            .signed
            .then(|| {
                verify_signature_file(
                    &path,
                    skill.manifest.trust.signed,
                    skill.manifest.trust.signature.as_deref(),
                    allowed_algorithms,
                )
            })
            .transpose()?;
        let signature_passed = signature.as_ref().is_none_or(|signature| signature.passed);
        Ok(TrustVerifyResult {
            artifact: path.display().to_string(),
            expected_hash: expected_hash.to_string(),
            actual_hash: actual.to_string(),
            hash_passed,
            signature,
            passed: hash_passed && signature_passed,
        })
    }

    fn evaluate_dependency_trust_inner(
        &self,
        policy: &SkillTrustPolicy,
        id: &str,
        kind: DependencyRootKind,
        depth: u32,
        walk: &mut DependencyTrustWalk<'_>,
    ) -> MspResult<()> {
        if walk.visited.contains(id) {
            return Ok(());
        }
        if walk.visiting.contains(id) {
            let start = walk.stack.iter().position(|entry| entry == id).unwrap_or(0);
            walk.result.cycles.push(walk.stack[start..].to_vec());
            return Ok(());
        }
        walk.visiting.insert(id.to_string());
        walk.stack.push(id.to_string());

        let dependencies = match kind {
            DependencyRootKind::Skill => self
                .skills
                .get(id)
                .ok_or_else(|| MspError::SkillNotFound(id.to_string()))?
                .manifest
                .dependencies
                .clone(),
            DependencyRootKind::Pack => self
                .packs
                .get(id)
                .ok_or_else(|| MspError::PackNotFound(id.to_string()))?
                .manifest
                .dependencies
                .clone(),
        };

        for dependency in &dependencies {
            let node = self.evaluate_dependency_edge(policy, id, depth + 1, dependency)?;
            let recurse_kind = match dependency.dependency_type {
                DependencyType::Skill if node.resolved => Some(DependencyRootKind::Skill),
                DependencyType::SkillPack if node.resolved => Some(DependencyRootKind::Pack),
                _ => None,
            };
            let should_recurse = recurse_kind.is_some()
                && policy
                    .dependency_policy
                    .as_ref()
                    .map(|dependency_policy| dependency_policy.allow_transitive_dependencies)
                    .unwrap_or(true);
            walk.result.dependencies.push(node);

            if dependency.required && !self.dependency_resolved(dependency) {
                walk.result.missing.push(dependency.id.clone());
            }

            if should_recurse {
                self.evaluate_dependency_trust_inner(
                    policy,
                    &dependency.id,
                    recurse_kind.expect("recurse kind checked above"),
                    depth + 1,
                    walk,
                )?;
            }
        }

        walk.stack.pop();
        walk.visiting.remove(id);
        walk.visited.insert(id.to_string());
        Ok(())
    }

    fn evaluate_dependency_edge(
        &self,
        policy: &SkillTrustPolicy,
        parent: &str,
        depth: u32,
        dependency: &SkillDependency,
    ) -> MspResult<DependencyTrustEvaluationNode> {
        let mut resolved = self.dependency_resolved(dependency);
        let mut reasons = Vec::new();
        let mut warnings = Vec::new();
        let mut matched_rules = Vec::new();
        let mut action = None;
        let mut allowed = true;

        if !resolved {
            if dependency.required {
                allowed = false;
                reasons.push(format!("required dependency {} is missing", dependency.id));
            } else {
                warnings.push(format!("optional dependency {} is missing", dependency.id));
            }
        }

        let dependency_policy_requires_trust = policy
            .dependency_policy
            .as_ref()
            .map(|dependency_policy| dependency_policy.require_dependency_trust_verification)
            .unwrap_or(true);

        if dependency.dependency_type == DependencyType::Skill {
            if let Some(skill) = self.skills.get(&dependency.id) {
                let evaluation = self.evaluate_trust_policy(policy, &dependency.id)?;
                action = Some(evaluation.action);
                matched_rules.extend(evaluation.matched_rules);
                warnings.extend(evaluation.warnings);
                if dependency_policy_requires_trust && !evaluation.allowed {
                    allowed = false;
                    reasons.push(format!(
                        "dependency {} is not allowed by policy: {:?}",
                        dependency.id, evaluation.action
                    ));
                    reasons.extend(evaluation.reasons);
                } else if !evaluation.allowed {
                    warnings.push(format!(
                        "dependency {} would not be allowed by policy, but dependency trust verification is disabled",
                        dependency.id
                    ));
                }

                if let Err(reason) = dependency_version_match_error(
                    dependency,
                    &skill.manifest.version,
                    "dependency",
                ) {
                    resolved = false;
                    allowed = false;
                    reasons.push(reason);
                }

                if let Some(dependency_policy) = policy.dependency_policy.as_ref()
                    && !dependency_policy.allow_transitive_dependencies
                    && depth == 1
                    && !skill.manifest.dependencies.is_empty()
                {
                    allowed = false;
                    reasons.push(format!(
                        "dependency {} has transitive dependencies but policy forbids them",
                        dependency.id
                    ));
                }

                if let Some(trust) = dependency.trust.as_ref() {
                    if let Some(required_issuer) = trust.required_issuer.as_ref()
                        && skill.manifest.trust.issuer != *required_issuer
                    {
                        allowed = false;
                        reasons.push(format!(
                            "dependency {} issuer mismatch: expected {}, got {}",
                            dependency.id, required_issuer, skill.manifest.trust.issuer
                        ));
                    }
                    if let Some(required_hash) = trust.required_hash.as_ref()
                        && skill.manifest.trust.hash != *required_hash
                    {
                        allowed = false;
                        reasons.push(format!(
                            "dependency {} hash mismatch: expected {}, got {}",
                            dependency.id, required_hash, skill.manifest.trust.hash
                        ));
                    }
                    if trust.must_be_signed {
                        let verification = self.verify_trust_with_policy(&dependency.id, policy)?;
                        if !skill.manifest.trust.signed {
                            allowed = false;
                            reasons.push(format!(
                                "dependency {} must be signed but is unsigned",
                                dependency.id
                            ));
                        } else if !verification.passed {
                            allowed = false;
                            reasons.push(format!(
                                "dependency {} signature verification failed",
                                dependency.id
                            ));
                            if !verification.hash_passed {
                                reasons.push(format!(
                                    "dependency {} hash mismatch: expected {}, got {}",
                                    dependency.id,
                                    verification.expected_hash,
                                    verification.actual_hash
                                ));
                            }
                            if let Some(signature) = verification.signature {
                                reasons.extend(signature.reasons);
                            }
                        }
                    }
                    if !trust.allowed_registries.is_empty() {
                        warnings.push(format!(
                            "dependency {} declares allowed_registries, but local registry identity verification is not implemented",
                            dependency.id
                        ));
                    }
                }
            }
        } else if dependency.dependency_type == DependencyType::SkillPack {
            if let Some(pack) = self.packs.get(&dependency.id) {
                let evaluation = self.evaluate_pack_trust_policy(policy, &dependency.id)?;
                action = Some(evaluation.action);
                matched_rules.extend(evaluation.matched_rules);
                warnings.extend(evaluation.warnings);
                if dependency_policy_requires_trust && !evaluation.allowed {
                    allowed = false;
                    reasons.push(format!(
                        "pack dependency {} is not allowed by policy: {:?}",
                        dependency.id, evaluation.action
                    ));
                    reasons.extend(evaluation.reasons);
                } else if !evaluation.allowed {
                    warnings.push(format!(
                        "pack dependency {} would not be allowed by policy, but dependency trust verification is disabled",
                        dependency.id
                    ));
                }

                if let Err(reason) = dependency_version_match_error(
                    dependency,
                    &pack.manifest.version,
                    "pack dependency",
                ) {
                    resolved = false;
                    allowed = false;
                    reasons.push(reason);
                }

                if let Some(dependency_policy) = policy.dependency_policy.as_ref()
                    && !dependency_policy.allow_transitive_dependencies
                    && depth == 1
                    && !pack.manifest.dependencies.is_empty()
                {
                    allowed = false;
                    reasons.push(format!(
                        "pack dependency {} has transitive dependencies but policy forbids them",
                        dependency.id
                    ));
                }

                if let Some(trust) = dependency.trust.as_ref() {
                    if let Some(required_issuer) = trust.required_issuer.as_ref()
                        && pack.manifest.trust.issuer != *required_issuer
                    {
                        allowed = false;
                        reasons.push(format!(
                            "pack dependency {} issuer mismatch: expected {}, got {}",
                            dependency.id, required_issuer, pack.manifest.trust.issuer
                        ));
                    }
                    if let Some(required_hash) = trust.required_hash.as_ref()
                        && pack.manifest.trust.hash != *required_hash
                    {
                        allowed = false;
                        reasons.push(format!(
                            "pack dependency {} hash mismatch: expected {}, got {}",
                            dependency.id, required_hash, pack.manifest.trust.hash
                        ));
                    }
                    if trust.must_be_signed {
                        let verification =
                            self.verify_pack_trust_with_policy(&dependency.id, policy)?;
                        if !pack.manifest.trust.signed {
                            allowed = false;
                            reasons.push(format!(
                                "pack dependency {} must be signed but is unsigned",
                                dependency.id
                            ));
                        } else if !verification.passed {
                            allowed = false;
                            reasons.push(format!(
                                "pack dependency {} signature verification failed",
                                dependency.id
                            ));
                            if !verification.hash_passed {
                                reasons.push(format!(
                                    "pack dependency {} hash mismatch: expected {}, got {}",
                                    dependency.id,
                                    verification.expected_hash,
                                    verification.actual_hash
                                ));
                            }
                            if let Some(signature) = verification.signature {
                                reasons.extend(signature.reasons);
                            }
                        }
                    }
                    if !trust.allowed_registries.is_empty() {
                        warnings.push(format!(
                            "pack dependency {} declares allowed_registries, but local registry identity verification is not implemented",
                            dependency.id
                        ));
                    }
                }
            }
        } else if dependency.required {
            warnings.push(format!(
                "required non-skill dependency {} of type {:?} is treated as externally resolved in local v0.1",
                dependency.id, dependency.dependency_type
            ));
        }

        Ok(DependencyTrustEvaluationNode {
            parent: parent.to_string(),
            depth,
            id: dependency.id.clone(),
            dependency_type: dependency.dependency_type.clone(),
            requirement: dependency.requirement.clone(),
            required: dependency.required,
            resolved,
            action,
            allowed,
            reasons,
            warnings,
            matched_rules,
        })
    }

    fn dependency_resolved(&self, dependency: &SkillDependency) -> bool {
        match dependency.dependency_type {
            DependencyType::Skill => self.skills.contains_key(&dependency.id),
            DependencyType::SkillPack => self.packs.contains_key(&dependency.id),
            DependencyType::Schema
            | DependencyType::VerificationContract
            | DependencyType::TrustAnchor
            | DependencyType::FormatAdapter
            | DependencyType::RuntimeCapability => true,
        }
    }

    fn resolve_dependencies_inner(
        &self,
        id: &str,
        result: &mut DependencyResolutionResult,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
        stack: &mut Vec<String>,
    ) -> MspResult<()> {
        if visited.contains(id) {
            return Ok(());
        }
        if visiting.contains(id) {
            let start = stack.iter().position(|entry| entry == id).unwrap_or(0);
            result.cycles.push(stack[start..].to_vec());
            return Ok(());
        }
        visiting.insert(id.to_string());
        stack.push(id.to_string());

        let skill = self
            .skills
            .get(id)
            .ok_or_else(|| MspError::SkillNotFound(id.to_string()))?;
        for dependency in &skill.manifest.dependencies {
            if dependency.dependency_type == DependencyType::SkillPack {
                let resolved = self.packs.get(&dependency.id).is_some_and(|pack| {
                    dependency_version_match_error(
                        dependency,
                        &pack.manifest.version,
                        "pack dependency",
                    )
                    .is_ok()
                });
                result.nodes.push(DependencyNode {
                    id: dependency.id.clone(),
                    requirement: dependency.requirement.clone(),
                    required: dependency.required,
                    resolved,
                });
                if dependency.required && !self.packs.contains_key(&dependency.id) {
                    result.missing.push(dependency.id.clone());
                }
                continue;
            }
            if dependency.dependency_type != DependencyType::Skill {
                result.nodes.push(DependencyNode {
                    id: dependency.id.clone(),
                    requirement: dependency.requirement.clone(),
                    required: dependency.required,
                    resolved: true,
                });
                continue;
            }
            let Some(skill) = self.skills.get(&dependency.id) else {
                result.nodes.push(DependencyNode {
                    id: dependency.id.clone(),
                    requirement: dependency.requirement.clone(),
                    required: dependency.required,
                    resolved: false,
                });
                if dependency.required {
                    result.missing.push(dependency.id.clone());
                }
                continue;
            };
            let resolved =
                dependency_version_match_error(dependency, &skill.manifest.version, "dependency")
                    .is_ok();
            result.nodes.push(DependencyNode {
                id: dependency.id.clone(),
                requirement: dependency.requirement.clone(),
                required: dependency.required,
                resolved,
            });
            if !resolved {
                continue;
            }
            self.resolve_dependencies_inner(&dependency.id, result, visiting, visited, stack)?;
        }

        stack.pop();
        visiting.remove(id);
        visited.insert(id.to_string());
        Ok(())
    }

    fn resolve_registry_relative_path(&self, uri: &str) -> MspResult<PathBuf> {
        if uri.contains("://") || uri.starts_with("file:") {
            return Err(MspError::InvalidRequest(format!(
                "local registry URI must be a relative path, got {uri}"
            )));
        }

        let relative = Path::new(uri);
        if relative.is_absolute() {
            return Err(MspError::InvalidRequest(format!(
                "local registry URI must not be absolute: {uri}"
            )));
        }
        if relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(MspError::InvalidRequest(format!(
                "local registry URI must not escape the registry root: {uri}"
            )));
        }

        let candidate = self.root.join(relative);
        let root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        if let Ok(canonical_candidate) = candidate.canonicalize()
            && !canonical_candidate.starts_with(&root)
        {
            return Err(MspError::InvalidRequest(format!(
                "resolved registry path escapes registry root: {}",
                canonical_candidate.display()
            )));
        }
        Ok(candidate)
    }

    fn resolve_artifact_path(&self, manifest_path: &Path, uri: &str) -> MspResult<PathBuf> {
        if uri.contains("://") || uri.starts_with("file:") {
            return Err(MspError::InvalidRequest(format!(
                "local registry artifact URI must be a relative path, got {uri}"
            )));
        }

        let relative = Path::new(uri);
        if relative.is_absolute() {
            return Err(MspError::InvalidRequest(format!(
                "local registry artifact URI must not be absolute: {uri}"
            )));
        }
        if relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(MspError::InvalidRequest(format!(
                "local registry artifact URI must not escape its manifest directory: {uri}"
            )));
        }

        let base = manifest_path.parent().unwrap_or(&self.root);
        let candidate = base.join(relative);
        let root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        if let Ok(canonical_candidate) = candidate.canonicalize()
            && !canonical_candidate.starts_with(&root)
        {
            return Err(MspError::InvalidRequest(format!(
                "resolved artifact path escapes registry root: {}",
                canonical_candidate.display()
            )));
        }
        Ok(candidate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedSemver {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Option<String>,
}

impl ParsedSemver {
    fn parse(value: &str) -> Result<Self, String> {
        let without_build = value.split_once('+').map_or(value, |(version, _)| version);
        let (core, prerelease) = without_build
            .split_once('-')
            .map_or((without_build, None), |(core, prerelease)| {
                (core, Some(prerelease.to_string()))
            });
        let mut parts = core.split('.');
        let major = parse_semver_part(parts.next(), value, "major")?;
        let minor = parse_semver_part(parts.next(), value, "minor")?;
        let patch = parse_semver_part(parts.next(), value, "patch")?;
        if parts.next().is_some() {
            return Err(format!("{value} is not a three-part semantic version"));
        }
        if let Some(prerelease) = prerelease.as_deref() {
            validate_prerelease(prerelease, value)?;
        }
        Ok(Self {
            major,
            minor,
            patch,
            prerelease,
        })
    }
}

fn parse_semver_part(part: Option<&str>, original: &str, name: &str) -> Result<u64, String> {
    let part = part.ok_or_else(|| format!("{original} is missing a {name} version component"))?;
    if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!(
            "{original} has an invalid {name} version component"
        ));
    }
    if part.len() > 1 && part.starts_with('0') {
        return Err(format!(
            "{original} has a leading-zero {name} version component"
        ));
    }
    part.parse::<u64>()
        .map_err(|_| format!("{original} has an out-of-range {name} version component"))
}

fn dependency_version_match_error(
    dependency: &SkillDependency,
    actual_version: &str,
    label: &str,
) -> Result<(), String> {
    let resolution = dependency
        .resolution
        .as_ref()
        .cloned()
        .unwrap_or(DependencyResolution {
            strategy: ResolutionStrategy::Compatible,
            allow_prerelease: false,
        });
    if version_matches_requirement(&dependency.requirement, actual_version, &resolution) {
        return Ok(());
    }
    Err(format!(
        "{label} {} version mismatch: requirement {} with strategy {:?} does not match actual {}",
        dependency.id, dependency.requirement, resolution.strategy, actual_version
    ))
}

fn version_matches_requirement(
    requirement: &str,
    actual_version: &str,
    resolution: &DependencyResolution,
) -> bool {
    let Ok(required) = ParsedSemver::parse(requirement) else {
        return false;
    };
    let Ok(actual) = ParsedSemver::parse(actual_version) else {
        return false;
    };
    if actual.prerelease.is_some() && !resolution.allow_prerelease {
        return false;
    }
    if !semver_at_least(&actual, &required) {
        return false;
    }

    match resolution.strategy {
        ResolutionStrategy::Exact | ResolutionStrategy::Manual => actual == required,
        ResolutionStrategy::Compatible => compatible_semver_match(&required, &actual),
        ResolutionStrategy::LatestPatch => {
            actual.major == required.major && actual.minor == required.minor
        }
        ResolutionStrategy::LatestMinor => actual.major == required.major,
    }
}

fn semver_at_least(actual: &ParsedSemver, required: &ParsedSemver) -> bool {
    let actual_core = (actual.major, actual.minor, actual.patch);
    let required_core = (required.major, required.minor, required.patch);
    if actual_core != required_core {
        return actual_core > required_core;
    }
    match (&actual.prerelease, &required.prerelease) {
        (None, None) => true,
        (None, Some(_)) => true,
        (Some(_), None) => false,
        (Some(actual_pre), Some(required_pre)) => {
            compare_prerelease(actual_pre, required_pre) != Ordering::Less
        }
    }
}

fn validate_prerelease(prerelease: &str, original: &str) -> Result<(), String> {
    if prerelease.is_empty() {
        return Err(format!("{original} has an empty prerelease component"));
    }
    for identifier in prerelease.split('.') {
        if identifier.is_empty() {
            return Err(format!("{original} has an empty prerelease identifier"));
        }
        if !identifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(format!("{original} has an invalid prerelease identifier"));
        }
        if is_numeric_identifier(identifier) && identifier.len() > 1 && identifier.starts_with('0')
        {
            return Err(format!(
                "{original} has a leading-zero numeric prerelease identifier"
            ));
        }
    }
    Ok(())
}

fn compare_prerelease(left: &str, right: &str) -> Ordering {
    let mut left_parts = left.split('.');
    let mut right_parts = right.split('.');
    loop {
        match (left_parts.next(), right_parts.next()) {
            (Some(left), Some(right)) => {
                let ordering = compare_prerelease_identifier(left, right);
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }
    }
}

fn compare_prerelease_identifier(left: &str, right: &str) -> Ordering {
    match (is_numeric_identifier(left), is_numeric_identifier(right)) {
        (true, true) => left
            .parse::<u64>()
            .unwrap_or(u64::MAX)
            .cmp(&right.parse::<u64>().unwrap_or(u64::MAX)),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => left.cmp(right),
    }
}

fn is_numeric_identifier(value: &str) -> bool {
    value.chars().all(|c| c.is_ascii_digit())
}

fn compatible_semver_match(required: &ParsedSemver, actual: &ParsedSemver) -> bool {
    if actual.major != required.major {
        return false;
    }
    if required.major > 0 {
        return true;
    }
    if actual.minor != required.minor {
        return false;
    }
    if required.minor > 0 {
        return true;
    }
    actual.patch == required.patch
}

fn evaluate_skill_compatibility(
    manifest: &SkillManifest,
    query: &RuntimeCompatibilityQuery,
) -> SkillCompatibilityResult {
    let mut issues = Vec::new();
    let mut warnings = Vec::new();

    let msp_version_compatible = check_msp_version(manifest, query, &mut issues, &mut warnings);
    let manifest_version_compatible = check_manifest_version(manifest, query, &mut issues);
    let format_compatible = check_formats(manifest, query, &mut issues);
    let runtime_capabilities_compatible = check_required_values(
        "runtime_capability",
        "runtime_capabilities",
        &manifest.requirements.runtime_capabilities,
        &query.runtime_capabilities,
        true,
        &mut issues,
    );
    let model_capabilities_compatible = check_required_values(
        "model_capability",
        "model_capabilities",
        &manifest.requirements.model_capabilities,
        &query.model_capabilities,
        true,
        &mut issues,
    );
    let tools_compatible = check_tools(manifest, query, &mut issues, &mut warnings);
    let permissions_compatible = check_required_values(
        "permission",
        "permissions",
        &manifest.requirements.permissions,
        &query.permissions,
        true,
        &mut issues,
    );
    let context_window_compatible = check_context_window(manifest, query, &mut issues);
    let platform_compatible = check_platform(manifest, query, &mut issues, &mut warnings);
    let known_runtime = check_known_runtime(manifest, query, &mut warnings);

    let compatible = issues
        .iter()
        .all(|issue| issue.severity != CompatibilitySeverity::Error);
    let dimensions = [
        msp_version_compatible,
        manifest_version_compatible,
        format_compatible,
        runtime_capabilities_compatible,
        model_capabilities_compatible,
        tools_compatible,
        permissions_compatible,
        context_window_compatible,
        platform_compatible,
    ];
    let score = dimensions.iter().filter(|compatible| **compatible).count() as f64
        / dimensions.len() as f64;

    SkillCompatibilityResult {
        skill_id: manifest.id.clone(),
        compatible,
        score,
        msp_version_compatible,
        manifest_version_compatible,
        format_compatible,
        runtime_capabilities_compatible,
        model_capabilities_compatible,
        tools_compatible,
        permissions_compatible,
        context_window_compatible,
        platform_compatible,
        known_runtime,
        issues,
        warnings,
    }
}

fn check_msp_version(
    manifest: &SkillManifest,
    query: &RuntimeCompatibilityQuery,
    issues: &mut Vec<CompatibilityIssue>,
    warnings: &mut Vec<String>,
) -> bool {
    let Some(runtime_version) = query.msp_version.as_deref() else {
        issues.push(compat_issue(
            "missing_runtime_msp_version",
            "msp_version",
            CompatibilitySeverity::Error,
            true,
            "runtime did not declare an MSP protocol version".to_string(),
        ));
        return false;
    };

    let manifest_requirement = DependencyResolution {
        strategy: ResolutionStrategy::Compatible,
        allow_prerelease: true,
    };
    let mut ok = version_matches_requirement(
        &manifest.msp_version,
        runtime_version,
        &manifest_requirement,
    );
    if !ok {
        issues.push(compat_issue(
            "msp_version_mismatch",
            "msp_version",
            CompatibilitySeverity::Error,
            true,
            format!(
                "runtime MSP version {} is not compatible with skill MSP version {}",
                runtime_version, manifest.msp_version
            ),
        ));
    }

    if let Some(compatibility) = manifest.compatibility.as_ref() {
        if let Some(minimum) = compatibility.min_msp_version.as_deref() {
            match (
                ParsedSemver::parse(runtime_version),
                ParsedSemver::parse(minimum),
            ) {
                (Ok(runtime), Ok(minimum)) if semver_at_least(&runtime, &minimum) => {}
                (Ok(_), Ok(_)) => {
                    issues.push(compat_issue(
                        "below_min_msp_version",
                        "msp_version",
                        CompatibilitySeverity::Error,
                        true,
                        format!(
                            "runtime MSP version {} is below skill minimum MSP version {}",
                            runtime_version, minimum
                        ),
                    ));
                    ok = false;
                }
                _ => warnings.push(format!(
                    "could not parse MSP version bound {} or runtime version {}",
                    minimum, runtime_version
                )),
            }
        }
        if let Some(maximum) = compatibility.max_msp_version.as_deref() {
            match (
                ParsedSemver::parse(runtime_version),
                ParsedSemver::parse(maximum),
            ) {
                (Ok(runtime), Ok(maximum)) if semver_at_least(&maximum, &runtime) => {}
                (Ok(_), Ok(_)) => {
                    issues.push(compat_issue(
                        "above_max_msp_version",
                        "msp_version",
                        CompatibilitySeverity::Error,
                        true,
                        format!(
                            "runtime MSP version {} is above skill maximum MSP version {}",
                            runtime_version, maximum
                        ),
                    ));
                    ok = false;
                }
                _ => warnings.push(format!(
                    "could not parse MSP version bound {} or runtime version {}",
                    maximum, runtime_version
                )),
            }
        }
    }
    ok
}

fn check_manifest_version(
    manifest: &SkillManifest,
    query: &RuntimeCompatibilityQuery,
    issues: &mut Vec<CompatibilityIssue>,
) -> bool {
    if query.supported_manifest_versions.is_empty()
        || query
            .supported_manifest_versions
            .iter()
            .any(|version| version == &manifest.manifest_version)
    {
        return true;
    }
    issues.push(compat_issue(
        "unsupported_manifest_version",
        "manifest_version",
        CompatibilitySeverity::Error,
        true,
        format!(
            "skill manifest version {} is not listed in runtime supported_manifest_versions",
            manifest.manifest_version
        ),
    ));
    false
}

fn check_formats(
    manifest: &SkillManifest,
    query: &RuntimeCompatibilityQuery,
    issues: &mut Vec<CompatibilityIssue>,
) -> bool {
    if query.supported_formats.is_empty()
        || query.supported_formats.iter().any(|format| {
            manifest
                .formats
                .iter()
                .any(|skill_format| skill_format == format)
        })
    {
        return true;
    }
    issues.push(compat_issue(
        "unsupported_format",
        "formats",
        CompatibilitySeverity::Error,
        true,
        format!(
            "runtime formats {:?} do not include any skill format {:?}",
            query.supported_formats, manifest.formats
        ),
    ));
    false
}

fn check_required_values(
    code: &str,
    dimension: &str,
    required_values: &[String],
    available_values: &[String],
    required: bool,
    issues: &mut Vec<CompatibilityIssue>,
) -> bool {
    let mut ok = true;
    for value in required_values {
        if !available_values.iter().any(|available| available == value) {
            ok = false;
            issues.push(compat_issue(
                &format!("missing_{code}"),
                dimension,
                CompatibilitySeverity::Error,
                required,
                format!("runtime is missing required {dimension} value {value}"),
            ));
        }
    }
    ok
}

fn check_tools(
    manifest: &SkillManifest,
    query: &RuntimeCompatibilityQuery,
    issues: &mut Vec<CompatibilityIssue>,
    warnings: &mut Vec<String>,
) -> bool {
    let mut ok = true;
    for tool in &manifest.requirements.tools {
        let available = query
            .available_tools
            .iter()
            .any(|available| available == &tool.name);
        if !available && tool.required {
            ok = false;
            issues.push(compat_issue(
                "missing_required_tool",
                "tools",
                CompatibilitySeverity::Error,
                true,
                format!("runtime is missing required tool {}", tool.name),
            ));
            continue;
        }
        if !available {
            warnings.push(format!("optional tool {} is not available", tool.name));
            continue;
        }
        if let Some(minimum_version) = tool.minimum_version.as_deref() {
            match query.tool_versions.get(&tool.name) {
                Some(actual_version)
                    if version_matches_requirement(
                        minimum_version,
                        actual_version,
                        &DependencyResolution {
                            strategy: ResolutionStrategy::Compatible,
                            allow_prerelease: true,
                        },
                    ) => {}
                Some(actual_version) if tool.required => {
                    ok = false;
                    issues.push(compat_issue(
                        "tool_version_too_low",
                        "tools",
                        CompatibilitySeverity::Error,
                        true,
                        format!(
                            "tool {} version {} does not satisfy minimum {}",
                            tool.name, actual_version, minimum_version
                        ),
                    ));
                }
                Some(actual_version) => warnings.push(format!(
                    "optional tool {} version {} does not satisfy minimum {}",
                    tool.name, actual_version, minimum_version
                )),
                None if tool.required => {
                    ok = false;
                    issues.push(compat_issue(
                        "missing_tool_version",
                        "tools",
                        CompatibilitySeverity::Error,
                        true,
                        format!(
                            "runtime did not declare version for required tool {} with minimum {}",
                            tool.name, minimum_version
                        ),
                    ));
                }
                None => warnings.push(format!(
                    "runtime did not declare version for optional tool {} with minimum {}",
                    tool.name, minimum_version
                )),
            }
        }
    }
    ok
}

fn check_context_window(
    manifest: &SkillManifest,
    query: &RuntimeCompatibilityQuery,
    issues: &mut Vec<CompatibilityIssue>,
) -> bool {
    let Some(required_window) = manifest.requirements.min_context_window else {
        return true;
    };
    match query.context_window {
        Some(actual) if actual >= required_window => true,
        Some(actual) => {
            issues.push(compat_issue(
                "context_window_too_small",
                "context_window",
                CompatibilitySeverity::Error,
                true,
                format!(
                    "runtime context window {} is below required {}",
                    actual, required_window
                ),
            ));
            false
        }
        None => {
            issues.push(compat_issue(
                "missing_context_window",
                "context_window",
                CompatibilitySeverity::Error,
                true,
                format!("runtime did not declare required context window {required_window}"),
            ));
            false
        }
    }
}

fn check_platform(
    manifest: &SkillManifest,
    query: &RuntimeCompatibilityQuery,
    issues: &mut Vec<CompatibilityIssue>,
    warnings: &mut Vec<String>,
) -> bool {
    if manifest.requirements.supported_platforms.is_empty() {
        return true;
    }
    let Some(platform) = query.platform.as_deref() else {
        warnings.push(format!(
            "skill declares supported platforms {:?}, but runtime platform is unknown",
            manifest.requirements.supported_platforms
        ));
        return true;
    };
    if manifest
        .requirements
        .supported_platforms
        .iter()
        .any(|supported| supported == platform || supported == "any")
    {
        return true;
    }
    issues.push(compat_issue(
        "unsupported_platform",
        "platform",
        CompatibilitySeverity::Error,
        true,
        format!(
            "runtime platform {} is not in skill supported_platforms {:?}",
            platform, manifest.requirements.supported_platforms
        ),
    ));
    false
}

fn check_known_runtime(
    manifest: &SkillManifest,
    query: &RuntimeCompatibilityQuery,
    warnings: &mut Vec<String>,
) -> bool {
    let Some(compatibility) = manifest.compatibility.as_ref() else {
        return true;
    };
    if compatibility.known_runtimes.is_empty() {
        return true;
    }
    let Some(runtime_name) = query.runtime_name.as_deref() else {
        warnings.push(format!(
            "skill declares known runtimes {:?}, but runtime_name was not provided",
            compatibility.known_runtimes
        ));
        return false;
    };
    let known = compatibility
        .known_runtimes
        .iter()
        .any(|runtime| runtime == runtime_name);
    if !known {
        warnings.push(format!(
            "runtime {} is not listed in skill known_runtimes {:?}",
            runtime_name, compatibility.known_runtimes
        ));
    }
    known
}

fn compat_issue(
    code: &str,
    dimension: &str,
    severity: CompatibilitySeverity,
    required: bool,
    message: String,
) -> CompatibilityIssue {
    CompatibilityIssue {
        code: code.to_string(),
        dimension: dimension.to_string(),
        severity,
        required,
        message,
    }
}

fn canonical_pack_trust_bytes(manifest: &SkillPackManifest) -> MspResult<Vec<u8>> {
    let mut canonical = manifest.clone();
    clear_pack_trust_proof_fields(&mut canonical.trust);
    Ok(serde_json::to_vec(&canonical)?)
}

fn clear_pack_trust_proof_fields(trust: &mut TrustMetadata) {
    trust.hash = HashDigest::from_bytes(trust.hash.algorithm, b"");
    trust.signed = false;
    trust.signature = None;
}

fn score_pack_manifest(manifest: &SkillPackManifest, query: &PackSearchQuery) -> u32 {
    if query
        .max_risk
        .is_some_and(|max_risk| manifest.trust.risk_level > max_risk)
    {
        return 0;
    }
    if let Some(issuer) = &query.issuer
        && manifest.trust.issuer != *issuer
    {
        return 0;
    }

    let mut score = 0;
    if query.task.is_none() && query.category.is_none() && query.issuer.is_none() {
        score += 1;
    }

    if let Some(category) = &query.category
        && manifest.category.contains(category)
    {
        score += 20;
    }

    if let Some(issuer) = &query.issuer
        && manifest.trust.issuer == *issuer
    {
        score += 10;
    }

    if let Some(task) = &query.task {
        let task = task.to_ascii_lowercase();
        for value in [&manifest.name, &manifest.summary, &manifest.category] {
            let normalized = value.to_ascii_lowercase().replace('/', " ");
            if task.contains(&normalized) || normalized.contains(&task) {
                score += 20;
            } else {
                for token in normalized.split_whitespace() {
                    if token.len() > 2 && task.contains(token) {
                        score += 2;
                    }
                }
            }
        }
        for member in &manifest.skills {
            let member_text = format!("{} {}", member.id, member.role.as_deref().unwrap_or(""))
                .to_ascii_lowercase()
                .replace(['.', '_', '-'], " ");
            for token in member_text.split_whitespace() {
                if token.len() > 2 && task.contains(token) {
                    score += 1;
                }
            }
        }
    }

    score
}

fn score_manifest(manifest: &SkillManifest, query: &SkillSearchQuery) -> u32 {
    if query
        .max_risk
        .is_some_and(|max_risk| manifest.trust.risk_level > max_risk)
    {
        return 0;
    }

    let mut score = 0;
    if query.task.is_none()
        && query.category.is_none()
        && query.domain.is_none()
        && query.language.is_none()
    {
        score += 1;
    }

    if let Some(category) = &query.category
        && manifest.category.contains(category)
    {
        score += 20;
    }
    if let Some(domain) = &query.domain
        && manifest
            .activation
            .domains
            .iter()
            .any(|value| value == domain)
    {
        score += 15;
    }
    if let Some(language) = &query.language
        && manifest
            .activation
            .languages
            .iter()
            .any(|value| value == language)
    {
        score += 15;
    }
    if let Some(task) = &query.task {
        let task = task.to_ascii_lowercase();
        for pattern in &manifest.activation.task_patterns {
            if task.contains(&pattern.to_ascii_lowercase()) {
                score += 30;
            } else {
                for token in pattern.split_whitespace() {
                    if token.len() > 2 && task.contains(&token.to_ascii_lowercase()) {
                        score += 2;
                    }
                }
            }
        }
        for keyword in &manifest.keywords {
            if task.contains(&keyword.to_ascii_lowercase()) {
                score += 5;
            }
        }
        if task.contains(&manifest.category.replace('/', " ")) {
            score += 8;
        }
    }

    if !query.available_tools.is_empty() {
        let required_tools: Vec<_> = manifest
            .requirements
            .tools
            .iter()
            .filter(|tool| tool.required)
            .map(|tool| tool.name.as_str())
            .collect();
        if required_tools
            .iter()
            .all(|required| query.available_tools.iter().any(|tool| tool == required))
        {
            score += 10;
        } else {
            return 0;
        }
    }

    score
}

pub fn hash_file(path: impl AsRef<Path>) -> MspResult<HashDigest> {
    HashDigest::from_file(path, HashAlgorithm::Sha256)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use msp_core::{
        Activation, ArtifactRef, DependencyPolicy, DependencyResolution, DependencyTrust,
        ExecutionReport, MspSchemaKind, PackSkillRef, ParsedSignature, Provenance, Requirements,
        ResolutionStrategy, ReviewStatus, RiskLevel, RiskPolicy, RuntimeCompatibilityQuery,
        SchemaRefs, SignaturePolicy, SkillArtifacts, SkillDependency, SkillSearchQuery,
        TrustAction, TrustMetadata, TrustedIssuer, VerificationSummary, validate_json_schema,
    };
    use rand::rngs::OsRng;
    use std::collections::BTreeMap;

    #[test]
    fn empty_registry_indexes_zero() {
        let registry = LocalRegistry::empty(".");
        assert_eq!(registry.skill_count(), 0);
    }

    #[test]
    fn search_filters_skills_by_max_risk() {
        let mut registry = LocalRegistry::empty(".");
        let low = test_manifest("skill.low.v1", "issuer.local", vec![]);
        insert_test_skill(&mut registry, low);
        let mut high = test_manifest("skill.high.v1", "issuer.local", vec![]);
        high.trust.risk_level = RiskLevel::High;
        insert_test_skill(&mut registry, high);

        let results = registry.search(&SkillSearchQuery {
            max_risk: Some(RiskLevel::Medium),
            ..SkillSearchQuery::default()
        });

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "skill.low.v1");
    }

    #[test]
    fn discovers_packs_with_structured_filters() {
        let root = unique_temp_registry_root("discover-packs");
        let mut registry = LocalRegistry::empty(&root);
        let mut rust_pack = test_pack_manifest("pack.rust.engineering.v1", "issuer.rust");
        rust_pack.name = "Rust Engineering Pack".to_string();
        rust_pack.summary = "Rust refactoring and test repair skills".to_string();
        rust_pack.category = "software/rust".to_string();
        insert_test_pack(&mut registry, &root, rust_pack, None);
        let mut python_pack = test_pack_manifest("pack.python.engineering.v1", "issuer.python");
        python_pack.name = "Python Engineering Pack".to_string();
        python_pack.summary = "Python maintenance skills".to_string();
        python_pack.category = "software/python".to_string();
        python_pack.trust.risk_level = RiskLevel::High;
        insert_test_pack(&mut registry, &root, python_pack, None);

        let results = registry.discover_packs(&PackSearchQuery {
            task: Some("rust test repair".to_string()),
            category: Some("rust".to_string()),
            issuer: Some("issuer.rust".to_string()),
            max_risk: Some(RiskLevel::Medium),
        });

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "pack.rust.engineering.v1");
        assert_eq!(results[0].skill_count, 1);
        assert_eq!(results[0].required_skill_count, 1);
        assert_eq!(results[0].issuer, "issuer.rust");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_artifact_uri_escape() {
        let registry = LocalRegistry::empty("/tmp/msp-registry-test");
        let error = registry
            .resolve_artifact_path(
                Path::new("/tmp/msp-registry-test/skill.manifest.json"),
                "../secret",
            )
            .unwrap_err();
        assert!(error.to_string().contains("must not escape"));
    }

    #[test]
    fn rejects_absolute_artifact_uri() {
        let registry = LocalRegistry::empty("/tmp/msp-registry-test");
        let error = registry
            .resolve_artifact_path(
                Path::new("/tmp/msp-registry-test/skill.manifest.json"),
                "/etc/passwd",
            )
            .unwrap_err();
        assert!(error.to_string().contains("must not be absolute"));
    }

    #[test]
    fn open_rejects_schema_invalid_manifest() {
        let root = unique_temp_registry_root("schema-invalid-manifest");
        let skill_dir = root.join("bad_skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("skill.manifest.json"),
            r#"{
              "msp_version": "0.1.0",
              "manifest_version": "0.1.0",
              "kind": "SkillManifest",
              "id": "skill.test.example.v1",
              "name": "Example",
              "version": "0.1.0",
              "category": "test/example",
              "summary": "Example skill",
              "formats": ["markdown"],
              "primary_format": "markdown",
              "activation": {"task_patterns": ["test"]},
              "requirements": {},
              "schemas": {"input": "msp://schemas/input.v1", "output": "msp://schemas/output.v1"},
              "verification": {"required_checks": ["manual_review"]},
              "trust": {
                "hash": "sha256:abcd",
                "signed": false,
                "issuer": "test",
                "risk_level": "low"
              },
              "artifacts": {
                "body": {"uri": "skill.md", "media_type": "text/markdown", "hash": "sha256:abcd"}
              },
              "unexpected": true
            }"#,
        )
        .unwrap();

        let error = LocalRegistry::open(&root).unwrap_err();
        assert!(error.to_string().contains("schema validation failed"));

        let _ = std::fs::remove_dir_all(root);
    }

    fn unique_temp_registry_root(name: &str) -> PathBuf {
        let unique = format!(
            "msp-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    fn test_manifest(id: &str, issuer: &str, dependencies: Vec<SkillDependency>) -> SkillManifest {
        SkillManifest {
            msp_version: "0.1.0".to_string(),
            manifest_version: "0.1.0".to_string(),
            kind: "SkillManifest".to_string(),
            id: id.to_string(),
            name: format!("Test {id}"),
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
            requirements: Requirements::default(),
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
                issuer: issuer.to_string(),
                license: None,
                review_status: ReviewStatus::SelfReviewed,
                risk_level: RiskLevel::Low,
                forbidden_behaviors: vec![],
                sandbox_profile: None,
            },
            dependencies,
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

    fn test_pack_manifest(id: &str, issuer: &str) -> SkillPackManifest {
        SkillPackManifest {
            msp_version: "0.1.0".to_string(),
            manifest_version: "0.1.0".to_string(),
            kind: "SkillPackManifest".to_string(),
            id: id.to_string(),
            name: format!("Test Pack {id}"),
            version: "0.1.0".to_string(),
            category: "test/example".to_string(),
            summary: "Example pack".to_string(),
            description: None,
            skills: vec![PackSkillRef {
                id: "skill.child.v1".to_string(),
                version: "0.1.0".to_string(),
                manifest_uri: "skill.child.v1/skill.manifest.json".to_string(),
                required: true,
                role: None,
            }],
            dependencies: vec![],
            trust: TrustMetadata {
                hash: HashDigest::parse("sha256:abcd").unwrap(),
                signed: false,
                signature: None,
                issuer: issuer.to_string(),
                license: None,
                review_status: ReviewStatus::SelfReviewed,
                risk_level: RiskLevel::Low,
                forbidden_behaviors: vec![],
                sandbox_profile: None,
            },
            provenance: Some(Provenance {
                author: Some("test".to_string()),
                ..Provenance::default()
            }),
            deprecation: None,
            extensions: BTreeMap::new(),
        }
    }

    fn insert_test_pack(
        registry: &mut LocalRegistry,
        root: &Path,
        mut manifest: SkillPackManifest,
        signing_key: Option<&SigningKey>,
    ) {
        let pack_dir = root.join(&manifest.id);
        std::fs::create_dir_all(&pack_dir).unwrap();
        let canonical = canonical_pack_trust_bytes(&manifest).unwrap();
        manifest.trust.hash = HashDigest::from_bytes(HashAlgorithm::Sha256, &canonical);
        if let Some(signing_key) = signing_key {
            let signature = signing_key.sign(&canonical);
            manifest.trust.signed = true;
            manifest.trust.signature = Some(
                ParsedSignature::encode_ed25519(
                    signing_key.verifying_key().as_bytes(),
                    &signature.to_bytes(),
                )
                .unwrap(),
            );
        }
        registry.packs.insert(
            manifest.id.clone(),
            RegistryPack {
                manifest_path: pack_dir.join("pack.manifest.json"),
                manifest,
            },
        );
    }

    fn require_signed_packs_policy() -> SkillTrustPolicy {
        let mut policy = allow_all_policy();
        policy.signature = Some(SignaturePolicy {
            require_signed_skills: false,
            require_signed_packs: true,
            allowed_algorithms: vec!["ed25519".to_string()],
        });
        policy
    }

    fn skill_dependency(id: &str) -> SkillDependency {
        SkillDependency {
            dependency_type: DependencyType::Skill,
            id: id.to_string(),
            requirement: "0.1.0".to_string(),
            required: true,
            purpose: None,
            trust: None,
            resolution: None,
        }
    }

    fn pack_dependency(id: &str) -> SkillDependency {
        SkillDependency {
            dependency_type: DependencyType::SkillPack,
            id: id.to_string(),
            requirement: "0.1.0".to_string(),
            required: true,
            purpose: None,
            trust: None,
            resolution: None,
        }
    }

    fn dependency_resolution(strategy: ResolutionStrategy) -> DependencyResolution {
        DependencyResolution {
            strategy,
            allow_prerelease: false,
        }
    }

    fn dependency_resolution_with_prerelease(strategy: ResolutionStrategy) -> DependencyResolution {
        DependencyResolution {
            strategy,
            allow_prerelease: true,
        }
    }

    fn set_skill_version(manifest: &mut SkillManifest, version: &str) {
        manifest.version = version.to_string();
    }

    fn set_pack_version(manifest: &mut SkillPackManifest, version: &str) {
        manifest.version = version.to_string();
    }

    fn allow_all_policy() -> SkillTrustPolicy {
        SkillTrustPolicy {
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
            rules: vec![],
            forbidden_behaviors: vec![],
            dependency_policy: Some(DependencyPolicy::default()),
            telemetry_policy: None,
        }
    }

    fn insert_test_skill(registry: &mut LocalRegistry, manifest: SkillManifest) {
        registry.skills.insert(
            manifest.id.clone(),
            RegistrySkill {
                manifest_path: PathBuf::from("skill.manifest.json"),
                manifest,
            },
        );
    }

    fn insert_file_backed_skill(
        registry: &mut LocalRegistry,
        root: &Path,
        mut manifest: SkillManifest,
        body: &[u8],
        signing_key: Option<&SigningKey>,
    ) {
        let skill_dir = root.join(&manifest.id);
        std::fs::create_dir_all(&skill_dir).unwrap();
        let body_path = skill_dir.join("skill.md");
        std::fs::write(&body_path, body).unwrap();
        let body_hash = HashDigest::from_file(&body_path, HashAlgorithm::Sha256).unwrap();
        manifest.trust.hash = body_hash.clone();
        manifest.artifacts.body.hash = body_hash;
        manifest.artifacts.body.size_bytes = Some(body.len() as u64);
        if let Some(signing_key) = signing_key {
            let signature = signing_key.sign(body);
            manifest.trust.signed = true;
            manifest.trust.signature = Some(
                ParsedSignature::encode_ed25519(
                    signing_key.verifying_key().as_bytes(),
                    &signature.to_bytes(),
                )
                .unwrap(),
            );
        }
        let manifest_path = skill_dir.join("skill.manifest.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        registry.skills.insert(
            manifest.id.clone(),
            RegistrySkill {
                manifest_path,
                manifest,
            },
        );
    }

    fn require_signed_policy() -> SkillTrustPolicy {
        let mut policy = allow_all_policy();
        policy.signature = Some(SignaturePolicy {
            require_signed_skills: true,
            require_signed_packs: false,
            allowed_algorithms: vec!["ed25519".to_string()],
        });
        policy
    }

    #[test]
    fn verifies_signed_skill_body_artifact() {
        let root = unique_temp_registry_root("signed-skill");
        let mut registry = LocalRegistry::empty(&root);
        let signing_key = SigningKey::generate(&mut OsRng);
        insert_file_backed_skill(
            &mut registry,
            &root,
            test_manifest("skill.signed.v1", "issuer.local", vec![]),
            b"signed body",
            Some(&signing_key),
        );

        let result = registry.verify_trust("skill.signed.v1").unwrap();

        assert!(result.hash_passed);
        assert!(
            result
                .signature
                .as_ref()
                .is_some_and(|signature| signature.passed)
        );
        assert!(result.passed);

        let evaluation = registry
            .evaluate_trust_policy(&require_signed_policy(), "skill.signed.v1")
            .unwrap();
        assert!(evaluation.allowed);

        let _ = std::fs::remove_dir_all(root);
    }

    fn issuer_bound_policy(issuer: &str, public_key_ref: String) -> SkillTrustPolicy {
        let mut policy = allow_all_policy();
        policy.trusted_issuers = vec![TrustedIssuer {
            id: issuer.to_string(),
            public_key_ref: Some(public_key_ref),
            allowed_risk_levels: vec![],
        }];
        policy
    }

    fn signed_skill_public_key_ref(registry: &LocalRegistry, id: &str) -> String {
        let signature = registry
            .skills
            .get(id)
            .unwrap()
            .manifest
            .trust
            .signature
            .as_deref()
            .unwrap();
        ParsedSignature::parse(signature).unwrap().public_key_ref()
    }

    fn signed_skill_public_key_sha256_ref(registry: &LocalRegistry, id: &str) -> String {
        let signature = registry
            .skills
            .get(id)
            .unwrap()
            .manifest
            .trust
            .signature
            .as_deref()
            .unwrap();
        ParsedSignature::parse(signature)
            .unwrap()
            .public_key_sha256_ref()
    }

    #[test]
    fn trusts_signed_skill_when_issuer_key_ref_matches() {
        let root = unique_temp_registry_root("issuer-key-match");
        let mut registry = LocalRegistry::empty(&root);
        let signing_key = SigningKey::generate(&mut OsRng);
        insert_file_backed_skill(
            &mut registry,
            &root,
            test_manifest("skill.signed.v1", "issuer.local", vec![]),
            b"signed body",
            Some(&signing_key),
        );
        let key_ref = signed_skill_public_key_ref(&registry, "skill.signed.v1");

        let evaluation = registry
            .evaluate_trust_policy(
                &issuer_bound_policy("issuer.local", key_ref),
                "skill.signed.v1",
            )
            .unwrap();

        assert!(evaluation.allowed, "{evaluation:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn trusts_signed_skill_when_issuer_key_hash_ref_matches() {
        let root = unique_temp_registry_root("issuer-key-hash-match");
        let mut registry = LocalRegistry::empty(&root);
        let signing_key = SigningKey::generate(&mut OsRng);
        insert_file_backed_skill(
            &mut registry,
            &root,
            test_manifest("skill.signed.v1", "issuer.local", vec![]),
            b"signed body",
            Some(&signing_key),
        );
        let key_ref = signed_skill_public_key_sha256_ref(&registry, "skill.signed.v1");

        let evaluation = registry
            .evaluate_trust_policy(
                &issuer_bound_policy("issuer.local", key_ref),
                "skill.signed.v1",
            )
            .unwrap();

        assert!(evaluation.allowed, "{evaluation:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_tampered_skill_even_when_issuer_key_ref_matches() {
        let root = unique_temp_registry_root("issuer-key-tampered");
        let mut registry = LocalRegistry::empty(&root);
        let signing_key = SigningKey::generate(&mut OsRng);
        insert_file_backed_skill(
            &mut registry,
            &root,
            test_manifest("skill.signed.v1", "issuer.local", vec![]),
            b"original body",
            Some(&signing_key),
        );
        let key_ref = signed_skill_public_key_ref(&registry, "skill.signed.v1");
        std::fs::write(
            root.join("skill.signed.v1").join("skill.md"),
            b"tampered body",
        )
        .unwrap();

        let evaluation = registry
            .evaluate_trust_policy(
                &issuer_bound_policy("issuer.local", key_ref),
                "skill.signed.v1",
            )
            .unwrap();

        assert!(!evaluation.allowed);
        assert!(evaluation.reasons.iter().any(|reason| {
            reason.contains("requires successful trust verification")
                || reason.contains("signature verification failed")
                || reason.contains("hash verification failed")
        }));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_signed_skill_when_issuer_key_ref_mismatches() {
        let root = unique_temp_registry_root("issuer-key-mismatch");
        let mut registry = LocalRegistry::empty(&root);
        let signing_key = SigningKey::generate(&mut OsRng);
        let other_key = SigningKey::generate(&mut OsRng);
        insert_file_backed_skill(
            &mut registry,
            &root,
            test_manifest("skill.signed.v1", "issuer.local", vec![]),
            b"signed body",
            Some(&signing_key),
        );
        let other_signature = other_key.sign(b"other body");
        let other_ref = ParsedSignature::parse(
            &ParsedSignature::encode_ed25519(
                other_key.verifying_key().as_bytes(),
                &other_signature.to_bytes(),
            )
            .unwrap(),
        )
        .unwrap()
        .public_key_ref();

        let evaluation = registry
            .evaluate_trust_policy(
                &issuer_bound_policy("issuer.local", other_ref),
                "skill.signed.v1",
            )
            .unwrap();

        assert!(!evaluation.allowed);
        assert!(
            evaluation
                .reasons
                .iter()
                .any(|reason| reason.contains("public key binding mismatch"))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unsigned_skill_when_issuer_key_ref_is_required() {
        let root = unique_temp_registry_root("issuer-key-unsigned");
        let mut registry = LocalRegistry::empty(&root);
        insert_file_backed_skill(
            &mut registry,
            &root,
            test_manifest("skill.unsigned.v1", "issuer.local", vec![]),
            b"unsigned body",
            None,
        );

        let evaluation = registry
            .evaluate_trust_policy(
                &issuer_bound_policy("issuer.local", "ed25519:required-key".to_string()),
                "skill.unsigned.v1",
            )
            .unwrap();

        assert!(!evaluation.allowed);
        assert!(
            evaluation
                .reasons
                .iter()
                .any(|reason| reason.contains("requires public key binding"))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_tampered_signed_skill_body_artifact() {
        let root = unique_temp_registry_root("tampered-signed-skill");
        let mut registry = LocalRegistry::empty(&root);
        let signing_key = SigningKey::generate(&mut OsRng);
        insert_file_backed_skill(
            &mut registry,
            &root,
            test_manifest("skill.signed.v1", "issuer.local", vec![]),
            b"original body",
            Some(&signing_key),
        );
        std::fs::write(
            root.join("skill.signed.v1").join("skill.md"),
            b"tampered body",
        )
        .unwrap();

        let result = registry.verify_trust("skill.signed.v1").unwrap();

        assert!(!result.hash_passed);
        assert!(
            result
                .signature
                .as_ref()
                .is_some_and(|signature| !signature.passed)
        );
        assert!(!result.passed);

        let evaluation = registry
            .evaluate_trust_policy(&require_signed_policy(), "skill.signed.v1")
            .unwrap();
        assert!(!evaluation.allowed);
        assert!(evaluation.reasons.iter().any(|reason| {
            reason.contains("cryptographic verification failed")
                || reason.contains("signature verification failed")
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verifies_unsigned_pack_trust_hash() {
        let root = unique_temp_registry_root("unsigned-pack");
        let mut registry = LocalRegistry::empty(&root);
        insert_test_pack(
            &mut registry,
            &root,
            test_pack_manifest("pack.test.v1", "issuer.local"),
            None,
        );

        let result = registry.verify_pack_trust("pack.test.v1").unwrap();

        assert!(result.hash_passed);
        assert!(result.signature.is_none());
        assert!(result.passed);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unsigned_pack_when_policy_requires_signed_packs() {
        let root = unique_temp_registry_root("unsigned-pack-policy");
        let mut registry = LocalRegistry::empty(&root);
        insert_test_pack(
            &mut registry,
            &root,
            test_pack_manifest("pack.test.v1", "issuer.local"),
            None,
        );

        let evaluation = registry
            .evaluate_pack_trust_policy(&require_signed_packs_policy(), "pack.test.v1")
            .unwrap();

        assert!(!evaluation.allowed);
        assert!(
            evaluation
                .reasons
                .iter()
                .any(|reason| reason.contains("requires signed packs"))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn trusts_signed_pack_when_issuer_key_ref_matches() {
        let root = unique_temp_registry_root("signed-pack-issuer");
        let mut registry = LocalRegistry::empty(&root);
        let signing_key = SigningKey::generate(&mut OsRng);
        insert_test_pack(
            &mut registry,
            &root,
            test_pack_manifest("pack.test.v1", "issuer.local"),
            Some(&signing_key),
        );
        let result = registry.verify_pack_trust("pack.test.v1").unwrap();
        let key_ref = result
            .signature
            .as_ref()
            .and_then(|signature| signature.public_key_ref.clone())
            .unwrap();

        assert!(result.passed);
        let evaluation = registry
            .evaluate_pack_trust_policy(
                &issuer_bound_policy("issuer.local", key_ref),
                "pack.test.v1",
            )
            .unwrap();

        assert!(evaluation.allowed);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_signed_pack_when_issuer_key_ref_mismatches() {
        let root = unique_temp_registry_root("signed-pack-key-mismatch");
        let mut registry = LocalRegistry::empty(&root);
        let signing_key = SigningKey::generate(&mut OsRng);
        let other_key = SigningKey::generate(&mut OsRng);
        insert_test_pack(
            &mut registry,
            &root,
            test_pack_manifest("pack.test.v1", "issuer.local"),
            Some(&signing_key),
        );
        let other_signature = other_key.sign(b"other pack");
        let other_ref = ParsedSignature::parse(
            &ParsedSignature::encode_ed25519(
                other_key.verifying_key().as_bytes(),
                &other_signature.to_bytes(),
            )
            .unwrap(),
        )
        .unwrap()
        .public_key_ref();

        let evaluation = registry
            .evaluate_pack_trust_policy(
                &issuer_bound_policy("issuer.local", other_ref),
                "pack.test.v1",
            )
            .unwrap();

        assert!(!evaluation.allowed);
        assert!(
            evaluation
                .reasons
                .iter()
                .any(|reason| reason.contains("public key binding mismatch"))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_tampered_signed_pack_manifest_metadata() {
        let root = unique_temp_registry_root("tampered-signed-pack");
        let mut registry = LocalRegistry::empty(&root);
        let signing_key = SigningKey::generate(&mut OsRng);
        insert_test_pack(
            &mut registry,
            &root,
            test_pack_manifest("pack.test.v1", "issuer.local"),
            Some(&signing_key),
        );
        registry
            .packs
            .get_mut("pack.test.v1")
            .unwrap()
            .manifest
            .summary = "Tampered summary".to_string();

        let result = registry.verify_pack_trust("pack.test.v1").unwrap();

        assert!(!result.hash_passed);
        assert!(
            result
                .signature
                .as_ref()
                .is_some_and(|signature| !signature.passed)
        );
        assert!(!result.passed);

        let evaluation = registry
            .evaluate_pack_trust_policy(&require_signed_packs_policy(), "pack.test.v1")
            .unwrap();
        assert!(!evaluation.allowed);
        assert!(evaluation.reasons.iter().any(|reason| {
            reason.contains("cryptographic verification failed")
                || reason.contains("signature verification failed")
                || reason.contains("pack hash verification failed")
        }));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn validates_pack_members_for_file_backed_skill() {
        let root = unique_temp_registry_root("pack-members-valid");
        let mut registry = LocalRegistry::empty(&root);
        insert_file_backed_skill(
            &mut registry,
            &root,
            test_manifest("skill.child.v1", "issuer.local", vec![]),
            b"child body",
            None,
        );
        insert_test_pack(
            &mut registry,
            &root,
            test_pack_manifest("pack.test.v1", "issuer.local"),
            None,
        );

        let result = registry.validate_pack_members("pack.test.v1").unwrap();

        assert!(result.valid, "{result:?}");
        assert_eq!(result.members.len(), 1);
        assert!(result.members[0].exists);
        assert!(result.members[0].indexed);
        assert!(result.members[0].id_matches);
        assert!(result.members[0].version_matches);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pack_member_validation_rejects_missing_required_member() {
        let root = unique_temp_registry_root("pack-members-missing");
        let mut registry = LocalRegistry::empty(&root);
        insert_test_pack(
            &mut registry,
            &root,
            test_pack_manifest("pack.test.v1", "issuer.local"),
            None,
        );

        let result = registry.validate_pack_members("pack.test.v1").unwrap();

        assert!(!result.valid);
        assert_eq!(result.missing, vec!["skill.child.v1".to_string()]);
        assert!(
            result
                .reasons
                .iter()
                .any(|reason| reason.contains("missing")),
            "{result:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pack_member_validation_rejects_manifest_uri_escape() {
        let root = unique_temp_registry_root("pack-members-escape");
        let mut registry = LocalRegistry::empty(&root);
        let mut pack = test_pack_manifest("pack.test.v1", "issuer.local");
        pack.skills[0].manifest_uri = "../skill.manifest.json".to_string();
        insert_test_pack(&mut registry, &root, pack, None);

        let result = registry.validate_pack_members("pack.test.v1").unwrap();

        assert!(!result.valid);
        assert!(result.reasons.iter().any(|reason| {
            reason.contains("invalid manifest_uri") && reason.contains("must not escape")
        }));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pack_member_validation_rejects_member_version_mismatch() {
        let root = unique_temp_registry_root("pack-members-version-mismatch");
        let mut registry = LocalRegistry::empty(&root);
        insert_file_backed_skill(
            &mut registry,
            &root,
            test_manifest("skill.child.v1", "issuer.local", vec![]),
            b"child body",
            None,
        );
        let mut pack = test_pack_manifest("pack.test.v1", "issuer.local");
        pack.skills[0].version = "9.9.9".to_string();
        insert_test_pack(&mut registry, &root, pack, None);

        let result = registry.validate_pack_members("pack.test.v1").unwrap();

        assert!(!result.valid);
        assert!(
            result
                .reasons
                .iter()
                .any(|reason| reason.contains("version mismatch")),
            "{result:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pack_member_validation_rejects_duplicate_member_ids() {
        let root = unique_temp_registry_root("pack-members-duplicate");
        let mut registry = LocalRegistry::empty(&root);
        insert_file_backed_skill(
            &mut registry,
            &root,
            test_manifest("skill.child.v1", "issuer.local", vec![]),
            b"child body",
            None,
        );
        let mut pack = test_pack_manifest("pack.test.v1", "issuer.local");
        pack.skills.push(pack.skills[0].clone());
        insert_test_pack(&mut registry, &root, pack, None);

        let result = registry.validate_pack_members("pack.test.v1").unwrap();

        assert!(!result.valid);
        assert_eq!(result.duplicate_ids, vec!["skill.child.v1".to_string()]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pack_member_validation_can_apply_member_trust_policy() {
        let root = unique_temp_registry_root("pack-members-policy");
        let mut registry = LocalRegistry::empty(&root);
        insert_file_backed_skill(
            &mut registry,
            &root,
            test_manifest("skill.child.v1", "issuer.local", vec![]),
            b"child body",
            None,
        );
        insert_test_pack(
            &mut registry,
            &root,
            test_pack_manifest("pack.test.v1", "issuer.local"),
            None,
        );
        let mut policy = allow_all_policy();
        policy.risk = Some(RiskPolicy {
            max_auto_load_risk: None,
            require_review_for: vec![],
            deny_risk_levels: vec![RiskLevel::Low],
        });

        let result = registry
            .validate_pack_members_with_policy("pack.test.v1", Some(&policy))
            .unwrap();

        assert!(!result.valid);
        assert!(result.members[0].trust_evaluation.is_some());
        assert!(result.reasons.iter().any(|reason| {
            reason.contains("not allowed by policy") || reason.contains("denied by policy")
        }));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dependency_resolution_accepts_compatible_skill_version() {
        let mut registry = LocalRegistry::empty(".");
        let mut dependency = skill_dependency("skill.child.v1");
        dependency.requirement = "1.2.3".to_string();
        dependency.resolution = Some(dependency_resolution(ResolutionStrategy::Compatible));
        insert_test_skill(
            &mut registry,
            test_manifest("skill.parent.v1", "issuer.local", vec![dependency]),
        );
        let mut child = test_manifest("skill.child.v1", "issuer.local", vec![]);
        set_skill_version(&mut child, "1.9.0");
        insert_test_skill(&mut registry, child);

        let result = registry.resolve_dependencies("skill.parent.v1").unwrap();

        assert!(result.nodes[0].resolved, "{result:?}");
        assert!(result.missing.is_empty());

        let trust = registry
            .evaluate_dependency_trust(&allow_all_policy(), "skill.parent.v1")
            .unwrap();
        assert!(trust.allowed, "{trust:?}");
    }

    #[test]
    fn dependency_resolution_rejects_incompatible_skill_major_version() {
        let mut registry = LocalRegistry::empty(".");
        let mut dependency = skill_dependency("skill.child.v1");
        dependency.requirement = "1.2.3".to_string();
        dependency.resolution = Some(dependency_resolution(ResolutionStrategy::Compatible));
        insert_test_skill(
            &mut registry,
            test_manifest("skill.parent.v1", "issuer.local", vec![dependency]),
        );
        let mut child = test_manifest("skill.child.v1", "issuer.local", vec![]);
        set_skill_version(&mut child, "2.0.0");
        insert_test_skill(&mut registry, child);

        let result = registry.resolve_dependencies("skill.parent.v1").unwrap();

        assert!(!result.nodes[0].resolved, "{result:?}");
        assert!(result.missing.is_empty());

        let trust = registry
            .evaluate_dependency_trust(&allow_all_policy(), "skill.parent.v1")
            .unwrap();
        assert!(!trust.allowed);
        assert!(
            trust.dependencies[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("version mismatch"))
        );
    }

    #[test]
    fn dependency_trust_enforces_latest_patch_strategy_for_skills() {
        let mut registry = LocalRegistry::empty(".");
        let mut dependency = skill_dependency("skill.child.v1");
        dependency.requirement = "1.2.3".to_string();
        dependency.resolution = Some(dependency_resolution(ResolutionStrategy::LatestPatch));
        insert_test_skill(
            &mut registry,
            test_manifest("skill.parent.v1", "issuer.local", vec![dependency]),
        );
        let mut child = test_manifest("skill.child.v1", "issuer.local", vec![]);
        set_skill_version(&mut child, "1.2.9");
        insert_test_skill(&mut registry, child);

        let result = registry
            .evaluate_dependency_trust(&allow_all_policy(), "skill.parent.v1")
            .unwrap();

        assert!(result.allowed, "{result:?}");
    }

    #[test]
    fn dependency_trust_rejects_latest_patch_minor_change_for_skills() {
        let mut registry = LocalRegistry::empty(".");
        let mut dependency = skill_dependency("skill.child.v1");
        dependency.requirement = "1.2.3".to_string();
        dependency.resolution = Some(dependency_resolution(ResolutionStrategy::LatestPatch));
        insert_test_skill(
            &mut registry,
            test_manifest("skill.parent.v1", "issuer.local", vec![dependency]),
        );
        let mut child = test_manifest("skill.child.v1", "issuer.local", vec![]);
        set_skill_version(&mut child, "1.3.0");
        insert_test_skill(&mut registry, child);

        let result = registry
            .evaluate_dependency_trust(&allow_all_policy(), "skill.parent.v1")
            .unwrap();

        assert!(!result.allowed);
        assert!(
            result.dependencies[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("LatestPatch"))
        );
    }

    #[test]
    fn dependency_trust_enforces_latest_minor_strategy_for_packs() {
        let root = unique_temp_registry_root("pack-dep-latest-minor");
        let mut registry = LocalRegistry::empty(&root);
        let mut dependency = pack_dependency("pack.child.v1");
        dependency.requirement = "1.2.3".to_string();
        dependency.resolution = Some(dependency_resolution(ResolutionStrategy::LatestMinor));
        let mut root_pack = test_pack_manifest("pack.root.v1", "issuer.local");
        root_pack.dependencies = vec![dependency];
        insert_test_pack(&mut registry, &root, root_pack, None);
        let mut child_pack = test_pack_manifest("pack.child.v1", "issuer.local");
        set_pack_version(&mut child_pack, "1.9.0");
        insert_test_pack(&mut registry, &root, child_pack, None);

        let result = registry
            .evaluate_pack_dependency_trust(&allow_all_policy(), "pack.root.v1")
            .unwrap();

        assert!(result.allowed, "{result:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dependency_trust_rejects_exact_pack_version_mismatch() {
        let root = unique_temp_registry_root("pack-dep-exact-version");
        let mut registry = LocalRegistry::empty(&root);
        let mut dependency = pack_dependency("pack.child.v1");
        dependency.requirement = "1.2.3".to_string();
        dependency.resolution = Some(dependency_resolution(ResolutionStrategy::Exact));
        let mut root_pack = test_pack_manifest("pack.root.v1", "issuer.local");
        root_pack.dependencies = vec![dependency];
        insert_test_pack(&mut registry, &root, root_pack, None);
        let mut child_pack = test_pack_manifest("pack.child.v1", "issuer.local");
        set_pack_version(&mut child_pack, "1.2.4");
        insert_test_pack(&mut registry, &root, child_pack, None);

        let result = registry
            .evaluate_pack_dependency_trust(&allow_all_policy(), "pack.root.v1")
            .unwrap();

        assert!(!result.allowed);
        assert!(
            result.dependencies[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("Exact"))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dependency_trust_treats_manual_strategy_as_exact() {
        let mut registry = LocalRegistry::empty(".");
        let mut dependency = skill_dependency("skill.child.v1");
        dependency.requirement = "1.2.3".to_string();
        dependency.resolution = Some(dependency_resolution(ResolutionStrategy::Manual));
        insert_test_skill(
            &mut registry,
            test_manifest("skill.parent.v1", "issuer.local", vec![dependency]),
        );
        let mut child = test_manifest("skill.child.v1", "issuer.local", vec![]);
        set_skill_version(&mut child, "1.2.4");
        insert_test_skill(&mut registry, child);

        let result = registry
            .evaluate_dependency_trust(&allow_all_policy(), "skill.parent.v1")
            .unwrap();

        assert!(!result.allowed);
        assert!(
            result.dependencies[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("Manual"))
        );
    }

    #[test]
    fn dependency_trust_uses_numeric_prerelease_identifier_ordering() {
        let mut registry = LocalRegistry::empty(".");
        let mut dependency = skill_dependency("skill.child.v1");
        dependency.requirement = "1.2.3-alpha.2".to_string();
        dependency.resolution = Some(dependency_resolution_with_prerelease(
            ResolutionStrategy::LatestPatch,
        ));
        insert_test_skill(
            &mut registry,
            test_manifest("skill.parent.v1", "issuer.local", vec![dependency]),
        );
        let mut child = test_manifest("skill.child.v1", "issuer.local", vec![]);
        set_skill_version(&mut child, "1.2.3-alpha.10");
        insert_test_skill(&mut registry, child);

        let result = registry
            .evaluate_dependency_trust(&allow_all_policy(), "skill.parent.v1")
            .unwrap();

        assert!(result.allowed, "{result:?}");
    }

    #[test]
    fn dependency_trust_rejects_prerelease_at_same_base_as_lower_than_stable_requirement() {
        let mut registry = LocalRegistry::empty(".");
        let mut dependency = skill_dependency("skill.child.v1");
        dependency.requirement = "1.2.3".to_string();
        dependency.resolution = Some(dependency_resolution_with_prerelease(
            ResolutionStrategy::LatestPatch,
        ));
        insert_test_skill(
            &mut registry,
            test_manifest("skill.parent.v1", "issuer.local", vec![dependency]),
        );
        let mut child = test_manifest("skill.child.v1", "issuer.local", vec![]);
        set_skill_version(&mut child, "1.2.3-alpha.1");
        insert_test_skill(&mut registry, child);

        let result = registry
            .evaluate_dependency_trust(&allow_all_policy(), "skill.parent.v1")
            .unwrap();

        assert!(!result.allowed);
        assert!(
            result.dependencies[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("version mismatch"))
        );
    }

    #[test]
    fn dependency_trust_rejects_prerelease_without_allow_prerelease() {
        let mut registry = LocalRegistry::empty(".");
        let mut dependency = skill_dependency("skill.child.v1");
        dependency.requirement = "1.2.3".to_string();
        dependency.resolution = Some(dependency_resolution(ResolutionStrategy::LatestPatch));
        insert_test_skill(
            &mut registry,
            test_manifest("skill.parent.v1", "issuer.local", vec![dependency]),
        );
        let mut child = test_manifest("skill.child.v1", "issuer.local", vec![]);
        set_skill_version(&mut child, "1.2.4-alpha.1");
        insert_test_skill(&mut registry, child);

        let result = registry
            .evaluate_dependency_trust(&allow_all_policy(), "skill.parent.v1")
            .unwrap();

        assert!(!result.allowed);
        assert!(result.dependencies[0].reasons.iter().any(|reason| {
            reason.contains("allow_prerelease") || reason.contains("version mismatch")
        }));
    }

    #[test]
    fn dependency_trust_allows_prerelease_when_enabled() {
        let mut registry = LocalRegistry::empty(".");
        let mut dependency = skill_dependency("skill.child.v1");
        dependency.requirement = "1.2.3".to_string();
        dependency.resolution = Some(dependency_resolution_with_prerelease(
            ResolutionStrategy::LatestPatch,
        ));
        insert_test_skill(
            &mut registry,
            test_manifest("skill.parent.v1", "issuer.local", vec![dependency]),
        );
        let mut child = test_manifest("skill.child.v1", "issuer.local", vec![]);
        set_skill_version(&mut child, "1.2.4-alpha.1");
        insert_test_skill(&mut registry, child);

        let result = registry
            .evaluate_dependency_trust(&allow_all_policy(), "skill.parent.v1")
            .unwrap();

        assert!(result.allowed, "{result:?}");
    }

    #[test]
    fn pack_dependency_trust_allows_resolved_allowed_pack_dependency() {
        let root = unique_temp_registry_root("pack-dep-resolved");
        let mut registry = LocalRegistry::empty(&root);
        let mut root_pack = test_pack_manifest("pack.root.v1", "issuer.local");
        root_pack.dependencies = vec![pack_dependency("pack.child.v1")];
        insert_test_pack(&mut registry, &root, root_pack, None);
        insert_test_pack(
            &mut registry,
            &root,
            test_pack_manifest("pack.child.v1", "issuer.local"),
            None,
        );

        let result = registry
            .evaluate_pack_dependency_trust(&allow_all_policy(), "pack.root.v1")
            .unwrap();

        assert!(result.allowed, "{result:?}");
        assert_eq!(result.dependencies.len(), 1);
        assert_eq!(
            result.dependencies[0].dependency_type,
            DependencyType::SkillPack
        );
        assert_eq!(result.dependencies[0].action, Some(TrustAction::Allow));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pack_dependency_trust_rejects_missing_required_pack_dependency() {
        let root = unique_temp_registry_root("pack-dep-missing");
        let mut registry = LocalRegistry::empty(&root);
        let mut root_pack = test_pack_manifest("pack.root.v1", "issuer.local");
        root_pack.dependencies = vec![pack_dependency("pack.missing.v1")];
        insert_test_pack(&mut registry, &root, root_pack, None);

        let result = registry
            .evaluate_pack_dependency_trust(&allow_all_policy(), "pack.root.v1")
            .unwrap();

        assert!(!result.allowed);
        assert_eq!(result.missing, vec!["pack.missing.v1".to_string()]);
        assert!(
            result.dependencies[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("required dependency"))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pack_dependency_trust_rejects_pack_dependency_denied_by_policy() {
        let root = unique_temp_registry_root("pack-dep-policy-deny");
        let mut registry = LocalRegistry::empty(&root);
        let mut root_pack = test_pack_manifest("pack.root.v1", "issuer.local");
        root_pack.dependencies = vec![pack_dependency("pack.child.v1")];
        insert_test_pack(&mut registry, &root, root_pack, None);
        let mut child_pack = test_pack_manifest("pack.child.v1", "issuer.local");
        child_pack.trust.risk_level = RiskLevel::Critical;
        insert_test_pack(&mut registry, &root, child_pack, None);
        let mut policy = allow_all_policy();
        policy.risk = Some(RiskPolicy {
            max_auto_load_risk: None,
            require_review_for: vec![],
            deny_risk_levels: vec![RiskLevel::Critical],
        });

        let result = registry
            .evaluate_pack_dependency_trust(&policy, "pack.root.v1")
            .unwrap();

        assert!(!result.allowed);
        assert!(result.dependencies[0].reasons.iter().any(|reason| {
            reason.contains("pack dependency") || reason.contains("denied by policy")
        }));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pack_dependency_trust_rejects_pack_dependency_version_mismatch() {
        let root = unique_temp_registry_root("pack-dep-version");
        let mut registry = LocalRegistry::empty(&root);
        let mut dependency = pack_dependency("pack.child.v1");
        dependency.requirement = "9.9.9".to_string();
        let mut root_pack = test_pack_manifest("pack.root.v1", "issuer.local");
        root_pack.dependencies = vec![dependency];
        insert_test_pack(&mut registry, &root, root_pack, None);
        insert_test_pack(
            &mut registry,
            &root,
            test_pack_manifest("pack.child.v1", "issuer.local"),
            None,
        );

        let result = registry
            .evaluate_pack_dependency_trust(&allow_all_policy(), "pack.root.v1")
            .unwrap();

        assert!(!result.allowed);
        assert!(
            result.dependencies[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("version mismatch"))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pack_dependency_trust_detects_pack_dependency_cycles() {
        let root = unique_temp_registry_root("pack-dep-cycle");
        let mut registry = LocalRegistry::empty(&root);
        let mut root_pack = test_pack_manifest("pack.root.v1", "issuer.local");
        root_pack.dependencies = vec![pack_dependency("pack.child.v1")];
        insert_test_pack(&mut registry, &root, root_pack, None);
        let mut child_pack = test_pack_manifest("pack.child.v1", "issuer.local");
        child_pack.dependencies = vec![pack_dependency("pack.root.v1")];
        insert_test_pack(&mut registry, &root, child_pack, None);

        let result = registry
            .evaluate_pack_dependency_trust(&allow_all_policy(), "pack.root.v1")
            .unwrap();

        assert!(!result.allowed);
        assert_eq!(
            result.cycles,
            vec![vec![
                "pack.root.v1".to_string(),
                "pack.child.v1".to_string()
            ]]
        );
        assert!(
            result
                .reasons
                .iter()
                .any(|reason| reason.contains("denies dependency cycles"))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    fn compatibility_query() -> RuntimeCompatibilityQuery {
        RuntimeCompatibilityQuery {
            msp_version: Some("0.1.0".to_string()),
            supported_manifest_versions: vec!["0.1.0".to_string()],
            runtime_name: Some("msp-reference-test".to_string()),
            runtime_version: Some("0.1.0".to_string()),
            supported_formats: vec!["markdown".to_string()],
            runtime_capabilities: vec!["workspace_read".to_string()],
            model_capabilities: vec!["code_reasoning".to_string()],
            available_tools: vec!["read_file".to_string(), "write_file".to_string()],
            tool_versions: BTreeMap::from([
                ("read_file".to_string(), "1.2.0".to_string()),
                ("write_file".to_string(), "1.0.0".to_string()),
            ]),
            permissions: vec!["workspace_write".to_string()],
            context_window: Some(128_000),
            platform: Some("linux".to_string()),
        }
    }

    fn compatibility_manifest() -> SkillManifest {
        let mut manifest = test_manifest("skill.compat.v1", "issuer.local", vec![]);
        manifest.requirements.runtime_capabilities = vec!["workspace_read".to_string()];
        manifest.requirements.model_capabilities = vec!["code_reasoning".to_string()];
        manifest.requirements.tools = vec![
            msp_core::ToolRequirement {
                name: "read_file".to_string(),
                required: true,
                purpose: None,
                minimum_version: Some("1.0.0".to_string()),
            },
            msp_core::ToolRequirement {
                name: "optional_lint".to_string(),
                required: false,
                purpose: None,
                minimum_version: None,
            },
        ];
        manifest.requirements.permissions = vec!["workspace_write".to_string()];
        manifest.requirements.min_context_window = Some(64_000);
        manifest.requirements.supported_platforms = vec!["linux".to_string()];
        manifest.compatibility = Some(msp_core::Compatibility {
            min_msp_version: Some("0.1.0".to_string()),
            max_msp_version: Some("0.2.0".to_string()),
            known_runtimes: vec!["msp-reference-test".to_string()],
        });
        manifest
    }

    #[test]
    fn compatibility_allows_matching_runtime() {
        let mut registry = LocalRegistry::empty(".");
        insert_test_skill(&mut registry, compatibility_manifest());

        let result = registry
            .check_skill_compatibility("skill.compat.v1", &compatibility_query())
            .unwrap();

        assert!(result.compatible, "{result:?}");
        assert!(result.tools_compatible);
        assert!(result.permissions_compatible);
        assert!(result.context_window_compatible);
        assert!(result.known_runtime);
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("optional tool"))
        );
    }

    #[test]
    fn compatibility_rejects_missing_required_tool_permission_and_context() {
        let mut registry = LocalRegistry::empty(".");
        insert_test_skill(&mut registry, compatibility_manifest());
        let mut query = compatibility_query();
        query.available_tools.clear();
        query.permissions.clear();
        query.context_window = Some(1024);

        let result = registry
            .check_skill_compatibility("skill.compat.v1", &query)
            .unwrap();

        assert!(!result.compatible);
        assert!(!result.tools_compatible);
        assert!(!result.permissions_compatible);
        assert!(!result.context_window_compatible);
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.code == "missing_required_tool")
        );
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.code == "missing_permission")
        );
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.code == "context_window_too_small")
        );
    }

    #[test]
    fn compatibility_rejects_unsupported_format_platform_and_manifest_version() {
        let mut registry = LocalRegistry::empty(".");
        insert_test_skill(&mut registry, compatibility_manifest());
        let mut query = compatibility_query();
        query.supported_formats = vec!["lsl".to_string()];
        query.supported_manifest_versions = vec!["9.9.9".to_string()];
        query.platform = Some("windows".to_string());

        let result = registry
            .check_skill_compatibility("skill.compat.v1", &query)
            .unwrap();

        assert!(!result.compatible);
        assert!(!result.format_compatible);
        assert!(!result.manifest_version_compatible);
        assert!(!result.platform_compatible);
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported_format")
        );
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported_platform")
        );
    }

    #[test]
    fn compatibility_rejects_msp_version_bounds() {
        let mut registry = LocalRegistry::empty(".");
        insert_test_skill(&mut registry, compatibility_manifest());
        let mut query = compatibility_query();
        query.msp_version = Some("1.0.0".to_string());

        let result = registry
            .check_skill_compatibility("skill.compat.v1", &query)
            .unwrap();

        assert!(!result.compatible);
        assert!(!result.msp_version_compatible);
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.code == "msp_version_mismatch")
        );
    }

    #[test]
    fn dependency_trust_allows_resolved_allowed_dependency() {
        let mut registry = LocalRegistry::empty(".");
        insert_test_skill(
            &mut registry,
            test_manifest(
                "skill.parent.v1",
                "issuer.local",
                vec![skill_dependency("skill.child.v1")],
            ),
        );
        insert_test_skill(
            &mut registry,
            test_manifest("skill.child.v1", "issuer.local", vec![]),
        );

        let result = registry
            .evaluate_dependency_trust(&allow_all_policy(), "skill.parent.v1")
            .unwrap();

        assert!(result.allowed);
        assert_eq!(result.dependencies.len(), 1);
        assert_eq!(result.dependencies[0].action, Some(TrustAction::Allow));
        assert!(result.dependencies[0].reasons.is_empty());
    }

    #[test]
    fn dependency_trust_rejects_tampered_required_signed_dependency() {
        let root = unique_temp_registry_root("tampered-signed-dependency");
        let mut dependency = skill_dependency("skill.child.v1");
        dependency.trust = Some(DependencyTrust {
            required_issuer: None,
            required_hash: None,
            must_be_signed: true,
            allowed_registries: vec![],
        });
        let mut registry = LocalRegistry::empty(&root);
        insert_file_backed_skill(
            &mut registry,
            &root,
            test_manifest("skill.parent.v1", "issuer.local", vec![dependency]),
            b"parent body",
            None,
        );
        let signing_key = SigningKey::generate(&mut OsRng);
        insert_file_backed_skill(
            &mut registry,
            &root,
            test_manifest("skill.child.v1", "issuer.local", vec![]),
            b"child original",
            Some(&signing_key),
        );
        std::fs::write(
            root.join("skill.child.v1").join("skill.md"),
            b"child tampered",
        )
        .unwrap();

        let result = registry
            .evaluate_dependency_trust(&allow_all_policy(), "skill.parent.v1")
            .unwrap();

        assert!(!result.allowed);
        assert!(result.dependencies[0].reasons.iter().any(|reason| {
            reason.contains("signature verification failed") || reason.contains("hash mismatch")
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dependency_trust_rejects_dependency_issuer_mismatch() {
        let mut dependency = skill_dependency("skill.child.v1");
        dependency.trust = Some(DependencyTrust {
            required_issuer: Some("issuer.required".to_string()),
            required_hash: None,
            must_be_signed: false,
            allowed_registries: vec![],
        });
        let mut registry = LocalRegistry::empty(".");
        insert_test_skill(
            &mut registry,
            test_manifest("skill.parent.v1", "issuer.local", vec![dependency]),
        );
        insert_test_skill(
            &mut registry,
            test_manifest("skill.child.v1", "issuer.actual", vec![]),
        );

        let result = registry
            .evaluate_dependency_trust(&allow_all_policy(), "skill.parent.v1")
            .unwrap();

        assert!(!result.allowed);
        assert!(
            result.dependencies[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("issuer mismatch"))
        );
    }

    #[test]
    fn dependency_trust_rejects_transitive_dependencies_when_policy_forbids_them() {
        let mut policy = allow_all_policy();
        policy.dependency_policy = Some(DependencyPolicy {
            allow_transitive_dependencies: false,
            ..DependencyPolicy::default()
        });
        let mut registry = LocalRegistry::empty(".");
        insert_test_skill(
            &mut registry,
            test_manifest(
                "skill.parent.v1",
                "issuer.local",
                vec![skill_dependency("skill.child.v1")],
            ),
        );
        insert_test_skill(
            &mut registry,
            test_manifest(
                "skill.child.v1",
                "issuer.local",
                vec![skill_dependency("skill.grandchild.v1")],
            ),
        );
        insert_test_skill(
            &mut registry,
            test_manifest("skill.grandchild.v1", "issuer.local", vec![]),
        );

        let result = registry
            .evaluate_dependency_trust(&policy, "skill.parent.v1")
            .unwrap();

        assert!(!result.allowed);
        assert!(
            result.dependencies[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("policy forbids"))
        );
    }

    #[test]
    fn evaluates_policy_for_indexed_skill() {
        let mut registry = LocalRegistry::empty(".");
        let manifest = SkillManifest {
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
            requirements: Requirements::default(),
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
                risk_level: RiskLevel::Low,
                forbidden_behaviors: vec![],
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
        };
        registry.skills.insert(
            manifest.id.clone(),
            RegistrySkill {
                manifest_path: PathBuf::from("skill.manifest.json"),
                manifest,
            },
        );
        let policy = SkillTrustPolicy {
            msp_version: "0.1.0".to_string(),
            policy_version: "0.1.0".to_string(),
            kind: "SkillTrustPolicy".to_string(),
            id: "trust.test.v1".to_string(),
            name: "Test".to_string(),
            default_action: msp_core::TrustAction::Allow,
            trusted_registries: vec![],
            trusted_issuers: vec![],
            signature: None,
            risk: None,
            rules: vec![],
            forbidden_behaviors: vec![],
            dependency_policy: None,
            telemetry_policy: None,
        };
        let result = registry
            .evaluate_trust_policy(&policy, "skill.test.example.v1")
            .unwrap();
        assert!(result.allowed);
    }
    #[test]
    fn real_registry_outputs_validate_against_protocol_result_schemas() {
        let registry = LocalRegistry::open(workspace_root().join("examples/registry")).unwrap();
        let policy = SkillTrustPolicy::from_path(
            workspace_root().join("examples/policies/local-reference.trust-policy.json"),
        )
        .unwrap();

        assert_schema(
            MspSchemaKind::RegistrySearchResultArray,
            registry.search(&SkillSearchQuery {
                task: Some("refactor rust module".to_string()),
                ..Default::default()
            }),
        );
        assert_schema(
            MspSchemaKind::PackSearchResultArray,
            registry.discover_packs(&msp_core::PackSearchQuery {
                task: Some("rust engineering".to_string()),
                ..Default::default()
            }),
        );
        assert_schema(
            MspSchemaKind::SkillLoadResult,
            registry
                .load_skill("skill.rust.refactor.module.v1")
                .unwrap(),
        );
        assert_schema(
            MspSchemaKind::DependencyResolutionResult,
            registry
                .resolve_dependencies("skill.rust.refactor.module.v1")
                .unwrap(),
        );
        assert_schema(
            MspSchemaKind::SkillCompatibilityResult,
            registry
                .check_skill_compatibility(
                    "skill.rust.refactor.module.v1",
                    &RuntimeCompatibilityQuery {
                        msp_version: Some("0.1.0".to_string()),
                        supported_manifest_versions: vec!["0.1.0".to_string()],
                        runtime_name: Some("msp-reference-test".to_string()),
                        supported_formats: vec!["markdown".to_string()],
                        runtime_capabilities: vec!["workspace_read".to_string()],
                        model_capabilities: vec!["code_reasoning".to_string()],
                        available_tools: vec!["read_file".to_string(), "write_file".to_string()],
                        permissions: vec!["workspace_write".to_string()],
                        context_window: Some(128_000),
                        platform: Some("linux".to_string()),
                        ..Default::default()
                    },
                )
                .unwrap(),
        );
        assert_schema(
            MspSchemaKind::TrustVerifyResult,
            registry
                .verify_trust("skill.rust.refactor.module.v1")
                .unwrap(),
        );
        assert_schema(
            MspSchemaKind::TrustPolicyEvaluation,
            registry
                .evaluate_trust_policy(&policy, "skill.rust.refactor.module.v1")
                .unwrap(),
        );
        assert_schema(
            MspSchemaKind::DependencyTrustEvaluationResult,
            registry
                .evaluate_dependency_trust(&policy, "skill.rust.refactor.module.v1")
                .unwrap(),
        );
        assert_schema(
            MspSchemaKind::PackMemberValidationResult,
            registry
                .validate_pack_members("pack.rust.engineering.v1")
                .unwrap(),
        );

        let report = ExecutionReport::from_path(
            workspace_root().join("examples/reports/rust-refactor.report.json"),
        )
        .unwrap();
        assert_schema(
            MspSchemaKind::SkillVerificationResult,
            registry.verify_execution_report(&report).unwrap(),
        );
    }

    fn assert_schema(kind: MspSchemaKind, value: impl serde::Serialize) {
        let value = serde_json::to_value(value).unwrap();
        validate_json_schema(kind, &value)
            .unwrap_or_else(|error| panic!("{kind:?} failed for {value:#}: {error}"));
    }

    fn workspace_root() -> PathBuf {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .parent()
            .and_then(Path::parent)
            .expect("crate is under workspace/crates/msp-registry")
            .to_path_buf()
    }
}
