//! Producer-side publication helpers for writing canonical MSP registry artifacts.
//!
//! The crate intentionally consumes producer output formats at artifact boundaries
//! instead of linking MSP core to any specific generator runtime.

use anyhow::{Context, Result, bail};
use msp_core::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const MSP_VERSION: &str = "0.1.0";
const MANIFEST_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishOptions {
    pub registry: PathBuf,
    pub issuer: String,
    pub force: bool,
    /// Explicitly allow replacing an existing same-id publication with different bytes.
    ///
    /// Normal `--force` is intentionally limited to idempotent regeneration of identical
    /// artifacts so published versions remain immutable by default.
    #[serde(default)]
    pub allow_mutable_version: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_key: Option<PathBuf>,
    #[serde(default)]
    pub deprecation: PublicationDeprecation,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PublicationDeprecation {
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_replacement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_replacement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sunset_at: Option<String>,
}

impl PublicationDeprecation {
    fn skill_deprecation(&self) -> Deprecation {
        Deprecation {
            deprecated: self.deprecated,
            reason: self.reason.clone(),
            replacement: self.skill_replacement.clone(),
            sunset_at: self.sunset_at.clone(),
        }
    }

    fn pack_deprecation(&self) -> Deprecation {
        Deprecation {
            deprecated: self.deprecated,
            reason: self.reason.clone(),
            replacement: self.pack_replacement.clone(),
            sunset_at: self.sunset_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishReport {
    pub registry: PathBuf,
    pub pack_id: Option<String>,
    pub skills_published: Vec<String>,
    pub files_written: Vec<PathBuf>,
    pub warnings: Vec<String>,
    pub signed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key_sha256: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct SkillerPackage {
    bundle_id: String,
    name: String,
    version: String,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    source_corpus: Vec<String>,
    #[serde(default)]
    review_status: Option<String>,
    #[serde(default)]
    publish_status: Option<String>,
    #[serde(default)]
    compatibility: BTreeMap<String, String>,
    #[serde(default)]
    created_at: Option<serde_json::Value>,
    #[serde(default, flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
struct SkillerSourceDocument {
    source_id: String,
    title: String,
    origin: String,
    #[serde(default)]
    source_type: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    visibility: Option<String>,
    #[serde(default)]
    retention_policy: Option<String>,
    #[serde(default)]
    export_policy: Option<String>,
    #[serde(default)]
    secret_scan_status: Option<serde_json::Value>,
    #[serde(default)]
    permission_status: Option<serde_json::Value>,
    #[serde(default)]
    citation_policy: Option<String>,
    #[serde(default, flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
struct SkillerSkill {
    id: String,
    title: String,
    summary: String,
    #[serde(default)]
    skill_type: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    maturity: Option<String>,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    source_section_ids: Vec<String>,
    #[serde(default)]
    procedure: Vec<String>,
    #[serde(default)]
    inputs: Vec<String>,
    #[serde(default)]
    outputs: Vec<String>,
    #[serde(default)]
    guardrails: Vec<String>,
    #[serde(default)]
    anti_patterns: Vec<String>,
    #[serde(default)]
    evals: Vec<SkillerEvalCase>,
    #[serde(default)]
    scripts: Vec<SkillerScript>,
    #[serde(default)]
    citations: Vec<SkillerCitation>,
    #[serde(default)]
    confidence: Option<serde_json::Value>,
    #[serde(default)]
    evidence_breakdown: Option<serde_json::Value>,
    #[serde(default)]
    inference_records: Vec<serde_json::Value>,
    #[serde(default)]
    tool_requirements: Vec<SkillerToolRequirement>,
    #[serde(default)]
    runtime_policy: SkillerRuntimePolicy,
    #[serde(default)]
    role_suitability: Vec<SkillerRoleSuitability>,
    #[serde(default)]
    version_applicability: Option<serde_json::Value>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
    #[serde(default, flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
struct SkillerScript {
    id: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    script_type: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    entrypoint: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    inputs: Vec<String>,
    #[serde(default)]
    outputs: Vec<String>,
    #[serde(default)]
    deterministic: bool,
    #[serde(default)]
    idempotent: bool,
    #[serde(default)]
    requires_approval: bool,
    #[serde(default)]
    permission_level: Option<String>,
    #[serde(default)]
    when_to_use: Vec<String>,
    #[serde(default)]
    guardrails: Vec<String>,
    #[serde(default)]
    generated_by: String,
    #[serde(default)]
    source_section_ids: Vec<String>,
    #[serde(default, flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
struct SkillerEvalCase {
    id: String,
    prompt: String,
    expected_behavior: String,
    #[serde(default)]
    eval_type: Option<String>,
    #[serde(default)]
    safety_notes: Vec<String>,
    #[serde(default, flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
struct SkillerCitation {
    citation_id: String,
    source_id: String,
    section_id: String,
    excerpt: String,
    #[serde(default, flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
struct SkillerToolRequirement {
    name: String,
    #[serde(default)]
    requirement_type: Option<String>,
    #[serde(default)]
    permission_level: Option<String>,
    #[serde(default)]
    dry_run_available: Option<bool>,
    #[serde(default)]
    rollback_required: bool,
    #[serde(default, flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
struct SkillerRuntimePolicy {
    #[serde(default)]
    conceptual_answer: bool,
    #[serde(default)]
    recommend_commands: bool,
    #[serde(default)]
    run_read_only_commands: bool,
    #[serde(default)]
    modify_files: bool,
    #[serde(default)]
    modify_external_systems: bool,
    #[serde(default)]
    requires_user_approval: bool,
    #[serde(default)]
    requires_backup_or_rollback: bool,
    #[serde(default)]
    handles_secrets: bool,
    #[serde(default)]
    handles_licensed_source: bool,
    #[serde(default, flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
struct SkillerRoleSuitability {
    role: String,
    suitability: f64,
    rationale: String,
    #[serde(default, flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
struct SkillerDependency {
    from_skill: String,
    to_skill: String,
    #[serde(default)]
    dependency_type: Option<String>,
    #[serde(default, flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[allow(dead_code)]
struct SkillerBundle {
    package: SkillerPackage,
    sources: Vec<SkillerSourceDocument>,
    sections: Vec<serde_json::Value>,
    skills: Vec<SkillerSkill>,
    dependencies: Vec<SkillerDependency>,
    related: Vec<serde_json::Value>,
    concepts: Vec<serde_json::Value>,
    candidates: Vec<serde_json::Value>,
    forge_requests: Vec<serde_json::Value>,
    forge_responses: Vec<serde_json::Value>,
    manifest_sha256: Option<String>,
    provenance: Option<serde_json::Value>,
}

pub fn import_skiller_bundle_as_draft(
    bundle_path: impl AsRef<Path>,
    issuer: impl Into<String>,
) -> Result<MspSkillPublicationDraft> {
    let bundle = read_skiller_bundle(bundle_path.as_ref())?;
    let ids = normalized_skill_ids(&bundle);
    let skills = bundle
        .skills
        .iter()
        .map(|skill| PublicationDraftSkill {
            id: ids
                .get(&skill.id)
                .cloned()
                .unwrap_or_else(|| normalize_skill_id(&bundle, skill)),
            source_id: Some(skill.id.clone()),
            name: bounded(skill.title.clone(), 160),
            version: semver_or_default(&bundle.package.version),
            category: category_for(&bundle.package, skill),
            summary: bounded(non_empty(&skill.summary, &skill.title), 500),
            body: render_skill_body(&bundle.package, skill),
            task_patterns: task_patterns(skill),
            required_checks: required_check_ids(skill),
            source_documents: source_documents_for(&bundle, skill),
        })
        .collect();

    let draft = MspSkillPublicationDraft {
        kind: "MspSkillPublicationDraft".to_string(),
        draft_version: MSP_VERSION.to_string(),
        publisher: PublicationDraftPublisher {
            issuer: issuer.into(),
            source: "skiller-bundle".to_string(),
            unsigned: true,
        },
        pack: Some(PublicationDraftPack {
            id: normalize_pack_id(&bundle.package),
            name: bounded(bundle.package.name.clone(), 160),
            version: semver_or_default(&bundle.package.version),
            category: package_category(&bundle.package),
            summary: bounded(
                format!("MSP import of Skiller bundle {}", bundle.package.name),
                500,
            ),
        }),
        skills,
    };
    draft.validate()?;
    Ok(draft)
}

pub fn publish_skiller_bundle(
    bundle_path: impl AsRef<Path>,
    options: PublishOptions,
) -> Result<PublishReport> {
    let bundle = read_skiller_bundle(bundle_path.as_ref())?;
    fs::create_dir_all(&options.registry)
        .with_context(|| format!("create registry {}", options.registry.display()))?;

    let signing_seed = options
        .signing_key
        .as_ref()
        .map(|path| read_signing_seed(path))
        .transpose()?;
    let signing_public_key = signing_seed
        .as_deref()
        .map(public_key_refs_for_seed)
        .transpose()?;

    let skill_ids = normalized_skill_ids(&bundle);
    let mut files_written = Vec::new();
    let mut warnings = Vec::new();
    let mut skill_refs = Vec::new();
    let version = semver_or_default(&bundle.package.version);

    for skill in &bundle.skills {
        let skill_id = skill_ids
            .get(&skill.id)
            .cloned()
            .unwrap_or_else(|| normalize_skill_id(&bundle, skill));
        let dir = options.registry.join("skills").join(&skill_id);
        if dir.exists() && !options.force {
            bail!(
                "MSP skill publication target already exists: {} (use --force to regenerate identical artifacts, or publish a new version)",
                dir.display()
            );
        }
        fs::create_dir_all(&dir)?;

        let body = render_skill_body(&bundle.package, skill);
        let body_bytes = body.as_bytes().to_vec();
        let body_hash = HashDigest::from_bytes(HashAlgorithm::Sha256, &body_bytes);
        let body_size = body_bytes.len() as u64;
        let body_signature = signing_seed
            .as_deref()
            .map(|seed| sign_ed25519_bytes(&body_bytes, seed))
            .transpose()?;

        let contract = verification_contract(&skill_id, &version, skill);
        let verify_bytes = serde_json::to_vec_pretty(&contract)?;
        let verify_hash = HashDigest::from_bytes(HashAlgorithm::Sha256, &verify_bytes);
        let verify_size = verify_bytes.len() as u64;

        let dependencies = dependencies_for(&bundle, &skill_ids, skill);
        let manifest = skill_manifest(SkillManifestBuild {
            bundle: &bundle,
            skill,
            skill_id: &skill_id,
            version: &version,
            issuer: &options.issuer,
            deprecation: options.deprecation.skill_deprecation(),
            body_hash,
            body_size,
            body_signature,
            verify_hash,
            verify_size,
            dependencies,
        });
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;

        let body_path = dir.join("skill.md");
        write_publication_file(&body_path, &body_bytes, &options)?;
        files_written.push(body_path.clone());

        let verify_path = dir.join("verify.json");
        write_publication_file(&verify_path, &verify_bytes, &options)?;
        VerificationContract::from_path(&verify_path)?;
        files_written.push(verify_path.clone());

        let manifest_path = dir.join("skill.manifest.json");
        write_publication_file(&manifest_path, &manifest_bytes, &options)?;
        SkillManifest::from_path(&manifest_path)?;
        files_written.push(manifest_path.clone());

        skill_refs.push(PackSkillRef {
            id: skill_id,
            version: version.clone(),
            manifest_uri: manifest_path
                .strip_prefix(&options.registry)
                .unwrap_or(&manifest_path)
                .to_string_lossy()
                .replace('\\', "/"),
            required: true,
            role: skill.role_suitability.first().map(|role| {
                let rationale = if role.rationale.trim().is_empty() {
                    "skiller-role".to_string()
                } else {
                    format!("{} score {:.2}", role.rationale, role.suitability)
                };
                format!("{} ({})", role.role, bounded(rationale, 120))
            }),
        });
    }

    let pack_id = normalize_pack_id(&bundle.package);
    let mut pack = SkillPackManifest {
        msp_version: MSP_VERSION.to_string(),
        manifest_version: MANIFEST_VERSION.to_string(),
        kind: "SkillPackManifest".to_string(),
        id: pack_id.clone(),
        name: bounded(bundle.package.name.clone(), 160),
        version: version.clone(),
        category: package_category(&bundle.package),
        summary: bounded(
            format!("MSP import of Skiller bundle {}", bundle.package.name),
            500,
        ),
        description: Some(format!(
            "Generated from Skiller bundle {} version {}.",
            bundle.package.bundle_id, bundle.package.version
        )),
        skills: skill_refs,
        dependencies: Vec::new(),
        trust: TrustMetadata {
            hash: HashDigest::from_bytes(HashAlgorithm::Sha256, b""),
            signed: signing_seed.is_some(),
            signature: None,
            issuer: options.issuer.clone(),
            license: None,
            review_status: package_review_status(&bundle.package),
            risk_level: package_risk(&bundle),
            forbidden_behaviors: Vec::new(),
            sandbox_profile: None,
        },
        provenance: Some(Provenance {
            author: None,
            generator: Some("skiller".to_string()),
            source_documents: package_source_documents(&bundle),
            generated_at: None,
            published_at: skiller_published_at(&bundle),
        }),
        deprecation: Some(options.deprecation.pack_deprecation()),
        extensions: BTreeMap::from([(
            "msp:skiller".to_string(),
            serde_json::json!({
                "bundle_id": bundle.package.bundle_id,
                "source_corpus": bundle.package.source_corpus,
                "publish_status": bundle.package.publish_status,
                "compatibility": bundle.package.compatibility,
                "created_at": skiller_created_at(&bundle.package),
                "source_rights": bundle.sources.iter().map(source_rights_json).collect::<Vec<_>>(),
                "section_count": bundle.sections.len(),
                "candidate_count": bundle.candidates.len(),
                "concept_count": bundle.concepts.len(),
                "related_count": bundle.related.len(),
                "dependency_count": bundle.dependencies.len(),
                "forge_request_count": bundle.forge_requests.len(),
                "forge_response_count": bundle.forge_responses.len(),
                "manifest_sha256_present": bundle.manifest_sha256.is_some(),
                "provenance_present": bundle.provenance.is_some(),
            }),
        )]),
    };
    let pack_canonical_bytes = pack_trust_bytes_for_proof(&pack)?;
    pack.trust.hash = HashDigest::from_bytes(HashAlgorithm::Sha256, &pack_canonical_bytes);
    if let Some(seed) = signing_seed.as_deref() {
        pack.trust.signature = Some(sign_ed25519_bytes(&pack_canonical_bytes, seed)?);
    }

    let pack_dir = options.registry.join("packs").join(&pack.id);
    if pack_dir.exists() && !options.force {
        bail!(
            "MSP pack publication target already exists: {} (use --force to regenerate identical artifacts, or publish a new version)",
            pack_dir.display()
        );
    }
    fs::create_dir_all(&pack_dir)?;
    let pack_path = pack_dir.join("pack.manifest.json");
    let pack_bytes = serde_json::to_vec_pretty(&pack_manifest_json(&pack)?)?;
    write_publication_file(&pack_path, &pack_bytes, &options)?;
    SkillPackManifest::from_path(&pack_path)?;
    files_written.push(pack_path);

    if bundle.package.version != version {
        warnings.push(format!(
            "Skiller bundle version {} was normalized to MSP semver {}",
            bundle.package.version, version
        ));
    }

    Ok(PublishReport {
        registry: options.registry,
        pack_id: Some(pack_id),
        skills_published: pack.skills.into_iter().map(|skill| skill.id).collect(),
        files_written,
        warnings,
        signed: signing_public_key.is_some(),
        public_key_ref: signing_public_key.as_ref().map(|key| key.0.clone()),
        public_key_sha256: signing_public_key.map(|key| key.1),
    })
}

fn write_publication_file(path: &Path, bytes: &[u8], options: &PublishOptions) -> Result<()> {
    if path.exists() {
        if !options.force {
            bail!(
                "MSP publication target already exists: {} (use --force to regenerate identical artifacts, or publish a new version)",
                path.display()
            );
        }
        let existing = fs::read(path)
            .with_context(|| format!("read existing publication artifact {}", path.display()))?;
        if existing != bytes && !options.allow_mutable_version {
            bail!(
                "MSP published version is immutable by default: {} would change existing bytes (publish a new version, or use --force --allow-mutable-version for an explicit local override)",
                path.display()
            );
        }
    }
    fs::write(path, bytes).with_context(|| format!("write publication artifact {}", path.display()))
}

fn read_skiller_bundle(path: &Path) -> Result<SkillerBundle> {
    let package: SkillerPackage = read_yaml(&path.join("package.yaml"))?;
    let sources: Vec<SkillerSourceDocument> =
        read_yaml(&path.join("sources/index.yaml")).unwrap_or_default();
    let sections: Vec<serde_json::Value> =
        read_yaml(&path.join("sources/sections.yaml")).unwrap_or_default();
    let dependencies: Vec<SkillerDependency> =
        read_yaml(&path.join("graph/dependencies.yaml")).unwrap_or_default();
    let related: Vec<serde_json::Value> =
        read_yaml(&path.join("graph/related.yaml")).unwrap_or_default();
    let concepts: Vec<serde_json::Value> =
        read_yaml(&path.join("graph/concepts.yaml")).unwrap_or_default();
    let candidates: Vec<serde_json::Value> =
        read_yaml(&path.join("candidates.yaml")).unwrap_or_default();
    let forge_requests: Vec<serde_json::Value> =
        read_yaml(&path.join("forge_requests.yaml")).unwrap_or_default();
    let forge_responses: Vec<serde_json::Value> =
        read_yaml(&path.join("forge_responses.yaml")).unwrap_or_default();
    let manifest_sha256 = fs::read_to_string(path.join("MANIFEST.sha256")).ok();
    let provenance = fs::read_to_string(path.join("PROVENANCE.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    let skills_dir = path.join("skills");
    let mut skills = Vec::new();
    for entry in
        fs::read_dir(&skills_dir).with_context(|| format!("read {}", skills_dir.display()))?
    {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("yaml") {
            skills.push(read_yaml(&p).with_context(|| format!("read skill {}", p.display()))?);
        }
    }
    skills.sort_by(|a: &SkillerSkill, b: &SkillerSkill| a.id.cmp(&b.id));
    if skills.is_empty() {
        bail!("Skiller bundle contains no skills: {}", path.display());
    }
    Ok(SkillerBundle {
        package,
        sources,
        sections,
        skills,
        dependencies,
        related,
        concepts,
        candidates,
        forge_requests,
        forge_responses,
        manifest_sha256,
        provenance,
    })
}

fn read_yaml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_yaml::from_str(&text)?)
}

struct SkillManifestBuild<'a> {
    bundle: &'a SkillerBundle,
    skill: &'a SkillerSkill,
    skill_id: &'a str,
    version: &'a str,
    issuer: &'a str,
    deprecation: Deprecation,
    body_hash: HashDigest,
    body_size: u64,
    body_signature: Option<String>,
    verify_hash: HashDigest,
    verify_size: u64,
    dependencies: Vec<SkillDependency>,
}

fn skill_manifest(build: SkillManifestBuild<'_>) -> SkillManifest {
    let SkillManifestBuild {
        bundle,
        skill,
        skill_id,
        version,
        issuer,
        deprecation,
        body_hash,
        body_size,
        body_signature,
        verify_hash,
        verify_size,
        dependencies,
    } = build;
    SkillManifest {
        msp_version: MSP_VERSION.to_string(),
        manifest_version: MANIFEST_VERSION.to_string(),
        kind: "SkillManifest".to_string(),
        id: skill_id.to_string(),
        name: bounded(skill.title.clone(), 160),
        version: version.to_string(),
        category: category_for(&bundle.package, skill),
        summary: bounded(non_empty(&skill.summary, &skill.title), 500),
        description: Some(format!(
            "Imported from Skiller skill {} in bundle {}.",
            skill.id, bundle.package.bundle_id
        )),
        keywords: keywords(bundle, skill),
        formats: vec!["markdown".to_string()],
        primary_format: "markdown".to_string(),
        activation: Activation {
            task_patterns: task_patterns(skill),
            negative_patterns: skill.anti_patterns.iter().take(8).cloned().collect(),
            required_context: Vec::new(),
            optional_context: vec!["workspace".to_string()],
            domains: bundle
                .package
                .domain
                .as_deref()
                .map(tokenize)
                .filter(|s| !s.is_empty())
                .into_iter()
                .collect(),
            languages: languages(skill),
        },
        requirements: requirements(skill),
        schemas: SchemaRefs {
            input: "msp://schemas/generic_skill_request.v1".to_string(),
            output: "msp://schemas/skill_execution_report.v1".to_string(),
            context: None,
        },
        verification: VerificationSummary {
            contract: Some("verify.json".to_string()),
            required_checks: required_check_ids(skill),
            optional_checks: Vec::new(),
            minimum_evidence: vec!["runtime_notes".to_string()],
        },
        trust: TrustMetadata {
            hash: body_hash.clone(),
            signed: body_signature.is_some(),
            signature: body_signature,
            issuer: issuer.to_string(),
            license: license_for(bundle),
            review_status: review_status(skill.status.as_deref()),
            risk_level: risk_level(skill),
            forbidden_behaviors: forbidden_behaviors(skill),
            sandbox_profile: sandbox_profile(skill),
        },
        dependencies,
        artifacts: SkillArtifacts {
            body: ArtifactRef {
                uri: "skill.md".to_string(),
                media_type: "text/markdown".to_string(),
                hash: body_hash,
                size_bytes: Some(body_size),
            },
            verification_contract: Some(ArtifactRef {
                uri: "verify.json".to_string(),
                media_type: "application/json".to_string(),
                hash: verify_hash,
                size_bytes: Some(verify_size),
            }),
            examples: Vec::new(),
            schemas: Vec::new(),
        },
        provenance: Some(Provenance {
            author: None,
            generator: Some("skiller".to_string()),
            source_documents: source_documents_for(bundle, skill),
            generated_at: skiller_created_at(&bundle.package),
            published_at: skiller_published_at(bundle),
        }),
        compatibility: Some(Compatibility {
            min_msp_version: Some(MSP_VERSION.to_string()),
            max_msp_version: None,
            known_runtimes: vec!["vegvisir".to_string(), "msp-reference".to_string()],
        }),
        deprecation: Some(deprecation),
        extensions: BTreeMap::from([(
            "msp:skiller".to_string(),
            serde_json::json!({
                "bundle_id": bundle.package.bundle_id,
                "skill_id": skill.id,
                "skill_type": skill.skill_type,
                "scope": skill.scope,
                "status": skill.status,
                "maturity": skill.maturity,
                "metadata": skill.metadata,
                "source_section_ids": skill.source_section_ids,
                "confidence": skill.confidence,
                "evidence_breakdown": skill.evidence_breakdown,
                "inference_record_count": skill.inference_records.len(),
                "version_applicability": skill.version_applicability,
                "script_count": skill.scripts.len(),
                "scripts": skill.scripts.iter().map(script_extension_json).collect::<Vec<_>>(),
            }),
        )]),
    }
}

fn verification_contract(
    skill_id: &str,
    version: &str,
    skill: &SkillerSkill,
) -> VerificationContract {
    let checks = verification_checks(skill);
    VerificationContract {
        msp_version: MSP_VERSION.to_string(),
        contract_version: MANIFEST_VERSION.to_string(),
        kind: "SkillVerificationContract".to_string(),
        id: format!("verify{}", skill_id.trim_start_matches("skill")),
        skill_id: skill_id.to_string(),
        skill_version: Some(version.to_string()),
        checks,
        success_criteria: SuccessCriteria {
            required_checks_pass: true,
            minimum_score: Some(1.0),
            minimum_confidence: Some(1.0),
            allowed_warnings: Some(1),
        },
        evidence_requirements: vec![EvidenceRequirement {
            key: "runtime_notes".to_string(),
            evidence_type: "runtime_note".to_string(),
            required: true,
            description: Some("Runtime notes describing how the Skiller-derived MSP skill was applied.".to_string()),
        }],
        failure_taxonomy: vec![
            FailureTaxonomyEntry {
                code: "missing_evidence".to_string(),
                description: "Required evidence was not provided.".to_string(),
                severity: Some("error".to_string()),
            },
            FailureTaxonomyEntry {
                code: "manual_review".to_string(),
                description: "Manual review did not pass.".to_string(),
                severity: Some("error".to_string()),
            },
        ],
        notes: Some("Generated from Skiller eval scaffolding. Runtimes should report each check id in execution results.".to_string()),
    }
}

fn verification_checks(skill: &SkillerSkill) -> Vec<VerificationCheck> {
    let mut seen = BTreeSet::new();
    let mut checks = Vec::new();
    for eval in &skill.evals {
        let id = check_id(eval);
        if !seen.insert(id.clone()) {
            continue;
        }
        checks.push(VerificationCheck {
            id,
            check_type: check_type(eval).to_string(),
            required: true,
            description: bounded(
                non_empty(
                    &eval.expected_behavior,
                    &format!("Skiller eval {} must pass", eval.id),
                ),
                1000,
            ),
            expected: Some(serde_json::json!({
                "prompt": eval.prompt,
                "expected_behavior": eval.expected_behavior,
                "safety_notes": eval.safety_notes,
                "skiller_eval_type": eval.eval_type,
            })),
            evidence_keys: vec!["runtime_notes".to_string()],
            weight: 1.0,
        });
    }
    if checks.is_empty() {
        checks.push(VerificationCheck {
            id: "manual_review".to_string(),
            check_type: "manual_review".to_string(),
            required: true,
            description: "Reviewer confirms the skill was applied according to its body, guardrails, and runtime policy.".to_string(),
            expected: None,
            evidence_keys: vec!["runtime_notes".to_string()],
            weight: 1.0,
        });
    }
    let weight = 1.0 / checks.len() as f64;
    for check in &mut checks {
        check.weight = weight;
    }
    checks
}

fn check_id(eval: &SkillerEvalCase) -> String {
    let id = tokenize(&eval.id);
    if id.is_empty() {
        "manual_review".to_string()
    } else {
        id.chars().take(128).collect()
    }
}

fn check_type(eval: &SkillerEvalCase) -> &'static str {
    match eval.eval_type.as_deref().unwrap_or_default() {
        "Safety" => "policy_check",
        "ToolUsePlanning" => "runtime_assertion",
        "SourceGrounding" => "manual_review",
        "Routing" => "manual_review",
        _ => "manual_review",
    }
}

fn requirements(skill: &SkillerSkill) -> Requirements {
    let mut runtime_capabilities = Vec::new();
    let mut permissions = Vec::new();
    if skill.runtime_policy.conceptual_answer {
        runtime_capabilities.push("reasoning".to_string());
    }
    if skill.runtime_policy.recommend_commands {
        runtime_capabilities.push("command_recommendation".to_string());
    }
    if skill.runtime_policy.run_read_only_commands {
        runtime_capabilities.push("command_execution".to_string());
        permissions.push("read_only_command".to_string());
    }
    if skill.runtime_policy.modify_files {
        runtime_capabilities.push("workspace_write".to_string());
        permissions.push("workspace_write".to_string());
    }
    if skill.runtime_policy.modify_external_systems {
        runtime_capabilities.push("external_mutation".to_string());
        permissions.push("external_mutation".to_string());
    }
    if skill.runtime_policy.requires_user_approval {
        permissions.push("user_approval".to_string());
    }
    if skill.runtime_policy.requires_backup_or_rollback {
        permissions.push("backup_or_rollback".to_string());
    }
    for script in &skill.scripts {
        if !script.content.trim().is_empty() {
            runtime_capabilities.push("script_review".to_string());
        }
        if script.requires_approval {
            permissions.push("user_approval".to_string());
        }
        match script.permission_level.as_deref() {
            Some("ReadOnly") => permissions.push("read_only_command".to_string()),
            Some("FileMutation") => {
                runtime_capabilities.push("workspace_write".to_string());
                permissions.push("workspace_write".to_string());
            }
            Some("ExternalMutation") => {
                runtime_capabilities.push("external_mutation".to_string());
                permissions.push("external_mutation".to_string());
            }
            Some("Dangerous") => {
                runtime_capabilities.push("dangerous_operation_review".to_string());
                permissions.push("dangerous_operation".to_string());
            }
            _ => {}
        }
    }

    Requirements {
        model_capabilities: vec!["instruction_following".to_string()],
        runtime_capabilities: unique(runtime_capabilities),
        tools: skill
            .tool_requirements
            .iter()
            .filter_map(|tool| {
                let name = tokenize(&tool.name);
                (!name.is_empty()).then(|| ToolRequirement {
                    name,
                    required: !matches!(tool.requirement_type.as_deref(), Some("Optional")),
                    purpose: Some(format!(
                        "Skiller {:?} requirement with {:?} permission; dry_run={:?}; rollback_required={}",
                        tool.requirement_type,
                        tool.permission_level,
                        tool.dry_run_available,
                        tool.rollback_required
                    )),
                    minimum_version: None,
                })
            })
            .collect(),
        permissions: unique(permissions),
        min_context_window: Some(8192),
        supported_platforms: vec!["linux".to_string(), "macos".to_string(), "windows".to_string()],
    }
}

fn dependencies_for(
    bundle: &SkillerBundle,
    ids: &BTreeMap<String, String>,
    skill: &SkillerSkill,
) -> Vec<SkillDependency> {
    bundle
        .dependencies
        .iter()
        .filter(|dep| dep.from_skill == skill.id)
        .filter_map(|dep| {
            ids.get(&dep.to_skill).map(|to| SkillDependency {
                dependency_type: DependencyType::Skill,
                id: to.clone(),
                requirement: semver_or_default(&bundle.package.version),
                required: true,
                purpose: Some(format!(
                    "Imported Skiller dependency {:?} from {} to {}",
                    dep.dependency_type, dep.from_skill, dep.to_skill
                )),
                trust: None,
                resolution: Some(DependencyResolution {
                    strategy: ResolutionStrategy::Compatible,
                    allow_prerelease: false,
                }),
            })
        })
        .collect()
}

fn render_skill_body(package: &SkillerPackage, skill: &SkillerSkill) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# {}\n\n{}\n\n",
        skill.title,
        non_empty(&skill.summary, &skill.title)
    ));
    out.push_str(&format!(
        "Skiller bundle: `{}` `{}`.\n\n",
        package.bundle_id, package.version
    ));
    out.push_str(&format!(
        "Status: `{}`  \\nMaturity: `{}`\n\n",
        skill.status.as_deref().unwrap_or("unknown"),
        skill.maturity.as_deref().unwrap_or("unknown")
    ));
    write_list(&mut out, "Inputs", &skill.inputs);
    write_list(&mut out, "Outputs", &skill.outputs);
    write_list(&mut out, "Procedure", &skill.procedure);
    write_list(&mut out, "Guardrails", &skill.guardrails);
    write_list(&mut out, "Anti-patterns", &skill.anti_patterns);
    if !skill.tool_requirements.is_empty() {
        out.push_str("## Tool Requirements\n");
        for tool in &skill.tool_requirements {
            out.push_str(&format!(
                "- `{}`: {:?} / {:?}; rollback_required={}\n",
                tool.name, tool.requirement_type, tool.permission_level, tool.rollback_required
            ));
        }
        out.push('\n');
    }
    if !skill.scripts.is_empty() {
        out.push_str("## Generated Scripts\n");
        for script in &skill.scripts {
            out.push_str(&format!(
                "### {}\n\n- id: `{}`\n- type: `{}`\n- language: `{}`\n- entrypoint: `{}`\n- deterministic: {}\n- idempotent: {}\n- requires_approval: {}\n- permission_level: `{}`\n\n{}\n\n",
                non_empty(&script.title, &script.id),
                script.id,
                script.script_type.as_deref().unwrap_or("unknown"),
                script.language.as_deref().unwrap_or("unknown"),
                script.entrypoint,
                script.deterministic,
                script.idempotent,
                script.requires_approval,
                script.permission_level.as_deref().unwrap_or("unknown"),
                script.description,
            ));
            write_list(&mut out, "When to use", &script.when_to_use);
            write_list(&mut out, "Script guardrails", &script.guardrails);
            if !script.content.trim().is_empty() {
                out.push_str("Script content is preserved in manifest extension metadata for host review; runtimes must not execute it without host policy approval.\n\n");
            }
        }
    }
    out.push_str("## Runtime Policy\n");
    out.push_str(&format!(
        "- conceptual_answer: {}\n- recommend_commands: {}\n- run_read_only_commands: {}\n- modify_files: {}\n- modify_external_systems: {}\n- requires_user_approval: {}\n- requires_backup_or_rollback: {}\n- handles_secrets: {}\n- handles_licensed_source: {}\n\n",
        skill.runtime_policy.conceptual_answer,
        skill.runtime_policy.recommend_commands,
        skill.runtime_policy.run_read_only_commands,
        skill.runtime_policy.modify_files,
        skill.runtime_policy.modify_external_systems,
        skill.runtime_policy.requires_user_approval,
        skill.runtime_policy.requires_backup_or_rollback,
        skill.runtime_policy.handles_secrets,
        skill.runtime_policy.handles_licensed_source,
    ));
    if !skill.citations.is_empty() {
        out.push_str("## Citations\n");
        for citation in &skill.citations {
            out.push_str(&format!(
                "- `{}` source `{}` section `{}`: {}\n",
                citation.citation_id,
                citation.source_id,
                citation.section_id,
                citation.excerpt.replace('\n', " ")
            ));
        }
        out.push('\n');
    }
    if !skill.evals.is_empty() {
        out.push_str("## Eval Scaffolding\n");
        for eval in &skill.evals {
            out.push_str(&format!(
                "- `{}` ({:?}): {} Expected: {}\n",
                eval.id, eval.eval_type, eval.prompt, eval.expected_behavior
            ));
        }
        out.push('\n');
    }
    out
}

fn write_list(out: &mut String, title: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    out.push_str(&format!("## {title}\n"));
    for value in values {
        out.push_str(&format!("- {value}\n"));
    }
    out.push('\n');
}

fn normalized_skill_ids(bundle: &SkillerBundle) -> BTreeMap<String, String> {
    let mut used = BTreeSet::new();
    bundle
        .skills
        .iter()
        .map(|skill| {
            let mut id = normalize_skill_id(bundle, skill);
            let (base_prefix, version_suffix) = id
                .rsplit_once(".v")
                .map(|(prefix, suffix)| (prefix.to_string(), suffix.to_string()))
                .unwrap_or_else(|| (id.clone(), "1".to_string()));
            let mut i = 2;
            while !used.insert(id.clone()) {
                id = format!("{base_prefix}_{i}.v{version_suffix}");
                i += 1;
            }
            (skill.id.clone(), id)
        })
        .collect()
}

fn normalize_skill_id(bundle: &SkillerBundle, skill: &SkillerSkill) -> String {
    if is_skill_id(&skill.id) {
        return skill.id.clone();
    }
    let domain = bundle
        .package
        .domain
        .as_deref()
        .unwrap_or(&bundle.package.bundle_id);
    format!(
        "skill.{}.{}.v1",
        dotted_segments(domain),
        dotted_segments(&skill.id)
    )
}

fn normalize_pack_id(package: &SkillerPackage) -> String {
    let raw = format!("pack.{}.v1", dotted_segments(&package.bundle_id));
    if raw == "pack.v1" {
        "pack.skiller_bundle.v1".to_string()
    } else {
        raw
    }
}

fn is_skill_id(id: &str) -> bool {
    id.starts_with("skill.") && id.contains(".v") && id.split('.').all(valid_segment_or_version)
}

fn valid_segment_or_version(s: &str) -> bool {
    if let Some(rest) = s.strip_prefix('v') {
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
    }
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn dotted_segments(value: &str) -> String {
    let parts: Vec<_> = value
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .map(tokenize)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        "skiller".to_string()
    } else {
        parts.join(".")
    }
}

fn tokenize(value: &str) -> String {
    let mut out = String::new();
    let mut last_sep = false;
    for c in value.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
            out.push(c);
            last_sep = false;
        } else if !last_sep {
            out.push('_');
            last_sep = true;
        }
    }
    let trimmed = out
        .trim_matches(|c| c == '_' || c == '-' || c == '.')
        .to_string();
    if trimmed
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        trimmed
    } else if trimmed.is_empty() {
        String::new()
    } else {
        format!("x{trimmed}")
    }
}

fn package_category(package: &SkillerPackage) -> String {
    package
        .domain
        .as_deref()
        .map(|d| tokenize(&d.replace('.', "/")))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "skiller/imported".to_string())
        .replace('.', "/")
}

fn category_for(package: &SkillerPackage, skill: &SkillerSkill) -> String {
    let domain = skill.domain.as_deref().or(package.domain.as_deref());
    let mut cat = domain
        .map(|d| tokenize(&d.replace('.', "/")))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "skiller/imported".to_string())
        .replace('.', "/");
    if let Some(kind) = skill.skill_type.as_deref() {
        let kind = tokenize(kind);
        if !kind.is_empty() {
            cat = format!("{cat}/{kind}");
        }
    }
    cat
}

fn semver_or_default(value: &str) -> String {
    let core = value.split(['-', '+']).next().unwrap_or(value);
    let parts: Vec<_> = core.split('.').collect();
    if parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    {
        value.to_string()
    } else {
        "0.1.0".to_string()
    }
}

fn task_patterns(skill: &SkillerSkill) -> Vec<String> {
    let mut patterns = vec![skill.title.clone(), skill.summary.clone()];
    patterns.extend(skill.procedure.iter().take(4).cloned());
    unique(
        patterns
            .into_iter()
            .filter(|p| !p.trim().is_empty())
            .map(|p| bounded(p, 300))
            .collect(),
    )
}

fn required_check_ids(skill: &SkillerSkill) -> Vec<String> {
    let ids: Vec<_> = verification_checks(skill)
        .into_iter()
        .map(|check| check.id)
        .collect();
    unique(ids)
}

fn source_documents_for(bundle: &SkillerBundle, skill: &SkillerSkill) -> Vec<String> {
    let cited: BTreeSet<_> = skill
        .citations
        .iter()
        .map(|c| c.source_id.as_str())
        .collect();
    let mut docs: Vec<_> = bundle
        .sources
        .iter()
        .filter(|source| cited.is_empty() || cited.contains(source.source_id.as_str()))
        .map(|source| {
            if source.origin.trim().is_empty() {
                source.source_id.clone()
            } else {
                source.origin.clone()
            }
        })
        .collect();
    if docs.is_empty() {
        docs.extend(bundle.package.source_corpus.clone());
    }
    unique(docs)
}

fn package_source_documents(bundle: &SkillerBundle) -> Vec<String> {
    let mut docs: Vec<_> = bundle
        .sources
        .iter()
        .map(|source| {
            if source.origin.trim().is_empty() {
                source.source_id.clone()
            } else {
                source.origin.clone()
            }
        })
        .collect();
    docs.extend(bundle.package.source_corpus.clone());
    unique(docs)
}

fn keywords(bundle: &SkillerBundle, skill: &SkillerSkill) -> Vec<String> {
    let mut values = vec![bundle.package.name.clone(), skill.title.clone()];
    if let Some(domain) = &bundle.package.domain {
        values.push(domain.clone());
    }
    if let Some(kind) = &skill.skill_type {
        values.push(kind.clone());
    }
    unique(
        values
            .into_iter()
            .filter_map(|v| {
                let token = tokenize(&v);
                (!token.is_empty()).then(|| token.chars().take(128).collect())
            })
            .collect(),
    )
}

fn languages(skill: &SkillerSkill) -> Vec<String> {
    let mut langs = Vec::new();
    let text = format!("{} {}", skill.title, skill.summary).to_ascii_lowercase();
    for lang in [
        "rust",
        "python",
        "javascript",
        "typescript",
        "java",
        "csharp",
        "cpp",
        "go",
    ] {
        if text.contains(lang) {
            langs.push(lang.to_string());
        }
    }
    langs
}

fn license_for(bundle: &SkillerBundle) -> Option<String> {
    let mut licenses: Vec<_> = bundle
        .sources
        .iter()
        .filter_map(|s| s.license.clone())
        .collect();
    licenses.sort();
    licenses.dedup();
    (licenses.len() == 1).then(|| licenses.remove(0))
}

fn review_status(status: Option<&str>) -> ReviewStatus {
    match status.unwrap_or_default() {
        "Reviewed" => ReviewStatus::PeerReviewed,
        "Approved" | "Published" => ReviewStatus::SecurityReviewed,
        "Deprecated" => ReviewStatus::Deprecated,
        "Unsafe" | "Archived" => ReviewStatus::Revoked,
        _ => ReviewStatus::Unreviewed,
    }
}

fn package_review_status(package: &SkillerPackage) -> ReviewStatus {
    review_status(package.review_status.as_deref())
}

fn risk_level(skill: &SkillerSkill) -> RiskLevel {
    if skill.runtime_policy.modify_external_systems
        || skill.tool_requirements.iter().any(|tool| {
            matches!(
                tool.permission_level.as_deref(),
                Some("Dangerous") | Some("ExternalMutation")
            )
        })
        || skill.scripts.iter().any(|script| {
            matches!(
                script.permission_level.as_deref(),
                Some("Dangerous") | Some("ExternalMutation")
            )
        })
    {
        RiskLevel::High
    } else if skill.runtime_policy.modify_files
        || skill
            .tool_requirements
            .iter()
            .any(|tool| matches!(tool.permission_level.as_deref(), Some("FileMutation")))
        || skill
            .scripts
            .iter()
            .any(|script| matches!(script.permission_level.as_deref(), Some("FileMutation")))
    {
        RiskLevel::Medium
    } else if skill.runtime_policy.handles_secrets {
        RiskLevel::High
    } else {
        RiskLevel::Low
    }
}

fn package_risk(bundle: &SkillerBundle) -> RiskLevel {
    if bundle
        .skills
        .iter()
        .any(|s| risk_level(s) >= RiskLevel::High)
    {
        RiskLevel::High
    } else if bundle
        .skills
        .iter()
        .any(|s| risk_level(s) >= RiskLevel::Medium)
    {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    }
}

fn forbidden_behaviors(skill: &SkillerSkill) -> Vec<String> {
    let mut values = vec![
        "override_host_policy".to_string(),
        "override_user_policy".to_string(),
        "grant_tool_permissions".to_string(),
        "silently_load_dependencies".to_string(),
    ];
    if skill.runtime_policy.handles_secrets {
        values.push("expose_plaintext_secrets".to_string());
    }
    unique(values)
}

fn sandbox_profile(skill: &SkillerSkill) -> Option<String> {
    if skill.runtime_policy.modify_external_systems
        || skill.scripts.iter().any(|script| {
            matches!(
                script.permission_level.as_deref(),
                Some("ExternalMutation") | Some("Dangerous")
            )
        })
    {
        Some("approval_required_external".to_string())
    } else if skill.runtime_policy.modify_files
        || skill
            .scripts
            .iter()
            .any(|script| matches!(script.permission_level.as_deref(), Some("FileMutation")))
    {
        Some("workspace_edit".to_string())
    } else if skill.runtime_policy.run_read_only_commands {
        Some("read_only_commands".to_string())
    } else {
        Some("conceptual".to_string())
    }
}

fn skiller_created_at(package: &SkillerPackage) -> Option<String> {
    package.created_at.as_ref().and_then(json_scalar_to_string)
}

fn skiller_published_at(bundle: &SkillerBundle) -> Option<String> {
    bundle
        .provenance
        .as_ref()
        .and_then(|value| value.get("published_at"))
        .and_then(json_scalar_to_string)
}

fn json_scalar_to_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn source_rights_json(source: &SkillerSourceDocument) -> serde_json::Value {
    serde_json::json!({
        "source_id": source.source_id,
        "title": source.title,
        "origin": source.origin,
        "source_type": source.source_type,
        "version": source.version,
        "license": source.license,
        "owner": source.owner,
        "visibility": source.visibility,
        "retention_policy": source.retention_policy,
        "export_policy": source.export_policy,
        "secret_scan_status": source.secret_scan_status,
        "permission_status": source.permission_status,
        "citation_policy": source.citation_policy,
    })
}

fn script_extension_json(script: &SkillerScript) -> serde_json::Value {
    serde_json::json!({
        "id": script.id,
        "title": script.title,
        "description": script.description,
        "script_type": script.script_type,
        "language": script.language,
        "entrypoint": script.entrypoint,
        "inputs": script.inputs,
        "outputs": script.outputs,
        "deterministic": script.deterministic,
        "idempotent": script.idempotent,
        "requires_approval": script.requires_approval,
        "permission_level": script.permission_level,
        "when_to_use": script.when_to_use,
        "guardrails": script.guardrails,
        "generated_by": script.generated_by,
        "source_section_ids": script.source_section_ids,
        "content": script.content,
    })
}

fn pack_manifest_json(pack: &SkillPackManifest) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(pack)?;
    if let Some(trust) = value
        .get_mut("trust")
        .and_then(|trust| trust.as_object_mut())
    {
        // The Rust type is shared with skill manifests, but the v0.1 pack
        // schema intentionally keeps pack trust metadata narrower. Missing
        // shared fields deserialize back to Rust defaults for canonical hash
        // verification.
        trust.remove("license");
        trust.remove("forbidden_behaviors");
        trust.remove("sandbox_profile");
    }
    Ok(value)
}

fn pack_trust_bytes_for_proof(pack: &SkillPackManifest) -> Result<Vec<u8>> {
    let mut canonical = pack.clone();
    canonical.trust.hash = HashDigest::from_bytes(canonical.trust.hash.algorithm, b"");
    canonical.trust.signed = false;
    canonical.trust.signature = None;
    Ok(serde_json::to_vec(&canonical)?)
}

fn read_signing_seed(path: &Path) -> Result<Vec<u8>> {
    let bytes = fs::read(path).with_context(|| format!("read signing key {}", path.display()))?;
    parse_signing_seed(&bytes).with_context(|| format!("parse signing key {}", path.display()))
}

fn parse_signing_seed(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() == 32 {
        return Ok(bytes.to_vec());
    }
    let text = std::str::from_utf8(bytes)
        .ok()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    if let Some(text) = text {
        let value = text.strip_prefix("ed25519-seed:").unwrap_or(text);
        let decoded = hex::decode(value).with_context(
            || "expected a raw 32-byte seed or hex text optionally prefixed by ed25519-seed:",
        )?;
        if decoded.len() == 32 {
            return Ok(decoded);
        }
        bail!(
            "ed25519 signing seed must be 32 bytes, got {}",
            decoded.len()
        );
    }
    bail!("ed25519 signing seed must be 32 bytes, got {}", bytes.len())
}

fn public_key_refs_for_seed(seed: &[u8]) -> Result<(String, String)> {
    let probe = sign_ed25519_bytes(b"msp-public-key-probe", seed)?;
    let parsed = ParsedSignature::parse(&probe)?;
    Ok((parsed.public_key_ref(), parsed.public_key_sha256_ref()))
}

fn unique(mut values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    values.retain(|v| seen.insert(v.clone()));
    values
}

fn bounded(value: String, max: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_string()
    } else {
        trimmed.chars().take(max).collect()
    }
}

fn non_empty(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use msp_registry::LocalRegistry;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("msp-publisher-{name}-{nonce}"))
    }

    fn write_sample_skiller_bundle(root: &Path) {
        fs::create_dir_all(root.join("skills")).unwrap();
        fs::create_dir_all(root.join("sources")).unwrap();
        fs::create_dir_all(root.join("graph")).unwrap();
        fs::write(
            root.join("package.yaml"),
            r#"bundle_id: sample-skiller-bundle
name: Sample Skiller Bundle
version: 0.1.0
domain: software_engineering/rust
source_corpus:
  - docs/sample.md
review_status: Reviewed
publish_status: Staged
compatibility: {}
created_at: "2026-01-01T00:00:00Z"
"#,
        )
        .unwrap();
        fs::write(
            root.join("sources/index.yaml"),
            r#"- source_id: src-1
  title: Sample Docs
  source_type: Markdown
  origin: docs/sample.md
  version: null
  license: MIT
  owner: null
  visibility: Public
  ingested_at: "2026-01-01T00:00:00Z"
  hash: sha256:abcd
  retention_policy: ExcerptsOnly
  export_policy: PublicAllowed
  secret_scan_status: Clean
  permission_status: Allowed
  citation_policy: ShortExcerpts
"#,
        )
        .unwrap();
        fs::write(
            root.join("sources/sections.yaml"),
            r#"- section_id: sec-1
  source_id: src-1
  heading: Sample Docs
  breadcrumbs: []
  line_start: 1
  line_end: 3
  text_excerpt: Refactor in small steps.
  code_blocks: []
  links: []
  detected_commands: []
  detected_api_operations: []
  detected_warnings: []
  detected_examples: []
  detected_normative_language: []
"#,
        )
        .unwrap();
        fs::write(root.join("candidates.yaml"), "[]\n").unwrap();
        fs::write(root.join("graph/concepts.yaml"), "[]\n").unwrap();
        fs::write(root.join("graph/dependencies.yaml"), "[]\n").unwrap();
        fs::write(root.join("graph/related.yaml"), "[]\n").unwrap();
        fs::write(root.join("forge_requests.yaml"), "[]\n").unwrap();
        fs::write(root.join("forge_responses.yaml"), "[]\n").unwrap();
        fs::write(root.join("MANIFEST.sha256"), "abcd  package.yaml\n").unwrap();
        fs::write(
            root.join("PROVENANCE.json"),
            r#"{
  "bundle_id": "sample-skiller-bundle",
  "published_at": "2026-01-02T00:00:00Z",
  "content_manifest_hash": "abcd"
}
"#,
        )
        .unwrap();
        fs::write(
            root.join("skills/refactor.yaml"),
            r#"id: refactor-module
title: Refactor Rust Module
summary: Refactor a Rust module safely with checks.
skill_type: Procedure
scope: TaskLevel
status: Reviewed
maturity: Level3Verified
domain: software_engineering/rust
source_section_ids:
  - sec-1
procedure:
  - Inspect current module boundaries.
  - Make one behavior-preserving change at a time.
inputs:
  - module path
outputs:
  - changed files and verification notes
guardrails:
  - Preserve public API unless explicitly requested.
anti_patterns:
  - rewrite from scratch
evals:
  - id: routing_eval
    prompt: User asks to refactor a Rust module.
    expected_behavior: Select this skill and ask for module path if missing.
    eval_type: Routing
    safety_notes: []
scripts:
  - id: preflight
    title: Refactor Preflight
    description: Check the target module before editing.
    script_type: PreflightCheck
    language: Pseudocode
    entrypoint: preflight
    content: inspect module path and planned edits
    inputs:
      - module path
    outputs:
      - preflight notes
    deterministic: true
    idempotent: true
    requires_approval: true
    permission_level: FileMutation
    when_to_use:
      - before editing files
    guardrails:
      - do not execute without host approval
    generated_by: skiller-test
    source_section_ids:
      - sec-1
confidence:
  raw: 0.62
  extraction: 0.8
  inference: 0.1
  procedure: 0.72
  guardrail: 0.55
  eval: 0.68
  routing: 0.68
  source_quality: 0.72
  human_review: 0.0
  runtime: 0.0
evidence_breakdown:
  direct_extraction: 1.0
  supporting_inference: 0.0
  operational_synthesis: 0.0
  speculative_candidate: 0.0
  community_derived: 0.0
  internal_policy_derived: 0.0
inference_records:
  - inference_id: inf-1
    required_review: false
version_applicability:
  supported_versions: []
  unsupported_versions: []
  version_source_refs: []
  version_confidence: 0.0
  migration_notes: []
  deprecated_flags: []
citations:
  - citation_id: cite-1
    source_id: src-1
    section_id: sec-1
    excerpt: Refactor in small steps.
tool_requirements:
  - name: read_file
    requirement_type: Required
    permission_level: ReadOnly
    dry_run_available: true
    rollback_required: false
  - name: write_file
    requirement_type: Required
    permission_level: FileMutation
    dry_run_available: false
    rollback_required: true
runtime_policy:
  conceptual_answer: true
  recommend_commands: true
  run_read_only_commands: true
  modify_files: true
  modify_external_systems: false
  requires_user_approval: true
  requires_backup_or_rollback: true
  handles_secrets: false
  handles_licensed_source: false
role_suitability:
  - role: rust_engineer
    suitability: 0.9
    rationale: Good fit for Rust refactors.
metadata: {}
"#,
        )
        .unwrap();
    }

    #[test]
    fn imports_skiller_bundle_as_valid_draft() {
        let root = temp_path("draft");
        write_sample_skiller_bundle(&root);
        let draft = import_skiller_bundle_as_draft(&root, "issuer.local").unwrap();
        draft.validate().unwrap();
        assert_eq!(draft.skills.len(), 1);
        assert_eq!(
            draft.skills[0].id,
            "skill.software_engineering.rust.refactor-module.v1"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn publishes_skiller_bundle_to_loadable_msp_registry() {
        let bundle = temp_path("bundle");
        let registry_root = temp_path("registry");
        write_sample_skiller_bundle(&bundle);

        let report = publish_skiller_bundle(
            &bundle,
            PublishOptions {
                registry: registry_root.clone(),
                issuer: "issuer.local".to_string(),
                force: false,
                allow_mutable_version: false,
                signing_key: None,
                deprecation: PublicationDeprecation::default(),
            },
        )
        .unwrap();

        assert_eq!(report.skills_published.len(), 1);
        let registry = LocalRegistry::open(&registry_root).unwrap();
        assert_eq!(registry.skill_count(), 1);
        assert_eq!(registry.pack_count(), 1);
        let loaded = registry.load_skill(&report.skills_published[0]).unwrap();
        assert!(loaded.body_hash_valid);
        assert!(loaded.verification_contract.is_some());
        assert!(loaded.body.contains("## Generated Scripts"));
        assert_eq!(
            loaded
                .manifest
                .provenance
                .as_ref()
                .and_then(|p| p.generated_at.as_deref()),
            Some("2026-01-01T00:00:00Z")
        );
        assert_eq!(
            loaded
                .manifest
                .provenance
                .as_ref()
                .and_then(|p| p.published_at.as_deref()),
            Some("2026-01-02T00:00:00Z")
        );
        assert_eq!(loaded.manifest.trust.risk_level, RiskLevel::Medium);
        assert!(
            loaded
                .manifest
                .requirements
                .runtime_capabilities
                .contains(&"script_review".to_string())
        );
        let skiller_ext = loaded
            .manifest
            .extensions
            .get("msp:skiller")
            .expect("skiller extension metadata");
        assert_eq!(skiller_ext["script_count"], 1);
        assert_eq!(skiller_ext["inference_record_count"], 1);
        assert_eq!(skiller_ext["scripts"][0]["id"], "preflight");
        let member_validation = registry
            .validate_pack_members(report.pack_id.as_deref().unwrap())
            .unwrap();
        assert!(member_validation.valid, "{member_validation:?}");

        let _ = fs::remove_dir_all(bundle);
        let _ = fs::remove_dir_all(registry_root);
    }
}
