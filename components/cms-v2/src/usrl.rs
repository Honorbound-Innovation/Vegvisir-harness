use crate::cms_api::{Metadata, ProjectId, RetrievalRequest};
use crate::core::{Claim, MemoryLink, MemoryObject, MemorySource};
use anyhow::{Context, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryVisibility {
    Public,
    Shared,
    Project,
    Private,
}

impl MemoryVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Shared => "shared",
            Self::Project => "project",
            Self::Private => "private",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "public" => Some(Self::Public),
            "shared" => Some(Self::Shared),
            "project" => Some(Self::Project),
            "private" => Some(Self::Private),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ScopeResolution {
    pub user_id: Option<String>,
    pub project_id: Option<String>,
    pub visibility: Option<MemoryVisibility>,
    pub no_memory: bool,
}

impl ScopeResolution {
    pub fn public() -> Self {
        Self {
            visibility: Some(MemoryVisibility::Public),
            ..Self::default()
        }
    }

    pub fn for_user_project(
        user_id: impl Into<String>,
        project_id: impl Into<String>,
        visibility: MemoryVisibility,
    ) -> Self {
        Self {
            user_id: Some(user_id.into()),
            project_id: Some(project_id.into()),
            visibility: Some(visibility),
            no_memory: false,
        }
    }

    pub fn no_memory(user_id: impl Into<String>, project_id: Option<String>) -> Self {
        Self {
            user_id: Some(user_id.into()),
            project_id,
            visibility: None,
            no_memory: true,
        }
    }

    pub fn from_retrieval_request(request: &RetrievalRequest) -> Self {
        Self {
            user_id: metadata_string(&request.filters, "user_id").map(ToOwned::to_owned),
            project_id: request
                .project_id
                .as_ref()
                .map(|project_id| project_id.0.clone())
                .or_else(|| metadata_string(&request.filters, "project_id").map(ToOwned::to_owned)),
            visibility: metadata_string(&request.filters, "visibility")
                .and_then(MemoryVisibility::parse),
            no_memory: metadata_string(&request.filters, "memory_mode")
                .or_else(|| metadata_string(&request.filters, "memory"))
                .is_some_and(is_no_memory_value),
        }
    }

    pub fn to_metadata(&self) -> Metadata {
        let mut metadata = Metadata::new();
        if let Some(user_id) = &self.user_id {
            metadata.insert("user_id".to_string(), Value::String(user_id.clone()));
        }
        if let Some(project_id) = &self.project_id {
            metadata.insert("project_id".to_string(), Value::String(project_id.clone()));
        }
        if let Some(visibility) = self.visibility {
            metadata.insert(
                "visibility".to_string(),
                Value::String(visibility.as_str().to_string()),
            );
        }
        if self.no_memory {
            metadata.insert("memory_mode".to_string(), Value::String("none".to_string()));
        }
        metadata
    }

    pub fn apply_to_retrieval_request(&self, request: &mut RetrievalRequest) {
        for (key, value) in self.to_metadata() {
            request.filters.insert(key, value);
        }
        if let Some(project_id) = &self.project_id {
            request.project_id = Some(ProjectId(project_id.clone()));
        }
    }
}

pub fn memory_visible_to_scope(metadata: &Metadata, scope: &ScopeResolution) -> bool {
    if scope.no_memory {
        return false;
    }

    let memory_user_id = metadata_string(metadata, "user_id");
    let memory_project_id = metadata_string(metadata, "project_id");
    let memory_visibility = metadata_string(metadata, "visibility")
        .and_then(MemoryVisibility::parse)
        .unwrap_or(MemoryVisibility::Public);

    if let Some(project_id) = memory_project_id
        && Some(project_id) != scope.project_id.as_deref()
        && !matches!(
            memory_visibility,
            MemoryVisibility::Public | MemoryVisibility::Shared
        )
    {
        return false;
    }

    match memory_visibility {
        MemoryVisibility::Private => {
            if memory_user_id.is_none() || memory_user_id != scope.user_id.as_deref() {
                return false;
            }
        }
        MemoryVisibility::Project => {
            if memory_project_id.is_none() || memory_project_id != scope.project_id.as_deref() {
                return false;
            }
        }
        MemoryVisibility::Shared | MemoryVisibility::Public => {}
    }

    match scope.visibility {
        Some(MemoryVisibility::Public) => {
            matches!(
                memory_visibility,
                MemoryVisibility::Public | MemoryVisibility::Shared
            )
        }
        Some(MemoryVisibility::Shared) => {
            matches!(
                memory_visibility,
                MemoryVisibility::Shared | MemoryVisibility::Public
            )
        }
        Some(MemoryVisibility::Project) => !matches!(memory_visibility, MemoryVisibility::Private),
        Some(MemoryVisibility::Private) | None => true,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopePolicyDecision {
    pub allowed: bool,
    pub reason: String,
    pub scope: ScopeResolution,
    pub metadata: Metadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UsrlScopePolicy;

impl UsrlScopePolicy {
    pub fn retrieval_scope(
        &self,
        user_id: impl Into<String>,
        project_id: Option<String>,
        metadata: &Metadata,
    ) -> ScopeResolution {
        ScopeResolution {
            user_id: Some(user_id.into()),
            project_id,
            visibility: metadata_string(metadata, "visibility").and_then(MemoryVisibility::parse),
            no_memory: metadata_string(metadata, "memory_mode")
                .or_else(|| metadata_string(metadata, "memory"))
                .is_some_and(is_no_memory_value),
        }
    }

    pub fn writeback_decision(
        &self,
        user_id: impl Into<String>,
        project_id: Option<String>,
        metadata: &Metadata,
    ) -> ScopePolicyDecision {
        let user_id = user_id.into();
        let no_memory = metadata_string(metadata, "memory_mode")
            .or_else(|| metadata_string(metadata, "memory"))
            .is_some_and(is_no_memory_value);
        if no_memory {
            return ScopePolicyDecision {
                allowed: false,
                reason: "no-memory mode disables USRL writeback".to_string(),
                scope: ScopeResolution::no_memory(user_id, project_id),
                metadata: Metadata::new(),
            };
        }

        let requested_visibility = metadata_string(metadata, "writeback_visibility")
            .or_else(|| metadata_string(metadata, "visibility"))
            .and_then(MemoryVisibility::parse)
            .unwrap_or(MemoryVisibility::Private);
        let visibility =
            if requested_visibility == MemoryVisibility::Project && project_id.is_none() {
                MemoryVisibility::Private
            } else {
                requested_visibility
            };
        let scope = ScopeResolution {
            user_id: Some(user_id.clone()),
            project_id: project_id.clone(),
            visibility: Some(visibility),
            no_memory: false,
        };
        let mut writeback_metadata = Metadata::new();
        writeback_metadata.insert("user_id".to_string(), Value::String(user_id));
        writeback_metadata.insert(
            "visibility".to_string(),
            Value::String(visibility.as_str().to_string()),
        );
        if let Some(project_id) = project_id {
            writeback_metadata.insert("project_id".to_string(), Value::String(project_id));
        }
        writeback_metadata.insert(
            "usrl_scope_policy".to_string(),
            Value::String("default-v1".to_string()),
        );
        writeback_metadata.insert(
            "usrl_scope_decision".to_string(),
            Value::String("writeback_allowed".to_string()),
        );

        ScopePolicyDecision {
            allowed: true,
            reason: format!("writeback allowed as {} memory", visibility.as_str()),
            scope,
            metadata: writeback_metadata,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsrlImportOptions {
    pub visibility: String,
    pub user_id: Option<String>,
    pub project_id: Option<String>,
    pub validator_root: Option<PathBuf>,
    pub require_authoritative_validation: bool,
}

impl Default for UsrlImportOptions {
    fn default() -> Self {
        Self {
            visibility: "public".to_string(),
            user_id: None,
            project_id: None,
            validator_root: None,
            require_authoritative_validation: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsrlValidationStatus {
    NotRequested,
    Valid,
    Invalid,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsrlValidationReport {
    pub validator: String,
    pub status: UsrlValidationStatus,
    pub exit_code: Option<i32>,
    pub issues: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub module_count: Option<usize>,
    pub symbol_count: Option<usize>,
    pub reference_count: Option<usize>,
    pub graph_node_count: Option<usize>,
    pub graph_edge_count: Option<usize>,
}

impl UsrlValidationReport {
    pub fn not_requested() -> Self {
        Self {
            validator: "cms-usrl-lightweight-importer".to_string(),
            status: UsrlValidationStatus::NotRequested,
            exit_code: None,
            issues: Vec::new(),
            stdout: String::new(),
            stderr: String::new(),
            module_count: None,
            symbol_count: None,
            reference_count: None,
            graph_node_count: None,
            graph_edge_count: None,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.status == UsrlValidationStatus::Valid
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsrlSummary {
    pub contracts: Vec<String>,
    pub sections: Vec<String>,
    pub facts: Vec<String>,
    pub rules: Vec<String>,
    pub constraints: Vec<String>,
    pub stages: Vec<String>,
    pub triggers: Vec<String>,
}

pub fn import_usrl_file(path: impl AsRef<Path>) -> anyhow::Result<MemoryObject> {
    let path = path.as_ref();
    import_usrl_file_with_options(path, &UsrlImportOptions::default())
}

pub fn import_usrl_file_with_options(
    path: impl AsRef<Path>,
    options: &UsrlImportOptions,
) -> anyhow::Result<MemoryObject> {
    let path = path.as_ref();
    let source = fs::read_to_string(path)?;
    let validation = validate_usrl_file(path, options)?;
    if options.require_authoritative_validation && !validation.is_valid() {
        bail!(
            "authoritative USRL validation failed for {}: {:?}",
            path.display(),
            validation.status
        );
    }
    let mut memory = import_usrl_text_with_options(
        &source,
        &path.to_string_lossy(),
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("usrl-source"),
        options,
    );
    apply_validation_metadata(&mut memory, &validation);
    Ok(memory)
}

pub fn validate_usrl_file(
    path: impl AsRef<Path>,
    options: &UsrlImportOptions,
) -> anyhow::Result<UsrlValidationReport> {
    let Some(validator_root) = &options.validator_root else {
        return Ok(UsrlValidationReport::not_requested());
    };
    validate_usrl_file_with_authoritative_cli(path, validator_root)
}

pub fn validate_usrl_file_with_authoritative_cli(
    path: impl AsRef<Path>,
    validator_root: impl AsRef<Path>,
) -> anyhow::Result<UsrlValidationReport> {
    let path = path.as_ref();
    let validator_root = validator_root.as_ref();
    let cli = validator_root.join("dist/src/cli.js");
    if !cli.exists() {
        return Ok(UsrlValidationReport {
            validator: cli.to_string_lossy().to_string(),
            status: UsrlValidationStatus::Unavailable,
            exit_code: None,
            issues: vec![format!("validator CLI not found at {}", cli.display())],
            stdout: String::new(),
            stderr: String::new(),
            module_count: None,
            symbol_count: None,
            reference_count: None,
            graph_node_count: None,
            graph_edge_count: None,
        });
    }

    let validate_output = Command::new("node")
        .arg(&cli)
        .arg("validate")
        .arg(path)
        .output()
        .with_context(|| format!("failed to run USRL validator {}", cli.display()))?;
    let stdout = String::from_utf8_lossy(&validate_output.stdout)
        .trim()
        .to_string();
    let stderr = String::from_utf8_lossy(&validate_output.stderr)
        .trim()
        .to_string();
    if !validate_output.status.success() {
        return Ok(UsrlValidationReport {
            validator: cli.to_string_lossy().to_string(),
            status: UsrlValidationStatus::Invalid,
            exit_code: validate_output.status.code(),
            issues: stderr_lines(&stderr),
            stdout,
            stderr,
            module_count: None,
            symbol_count: None,
            reference_count: None,
            graph_node_count: None,
            graph_edge_count: None,
        });
    }

    let resolve_output = Command::new("node")
        .arg(&cli)
        .arg("resolve")
        .arg(path)
        .output()
        .with_context(|| format!("failed to run USRL resolver {}", cli.display()))?;
    let resolve_stdout = String::from_utf8_lossy(&resolve_output.stdout)
        .trim()
        .to_string();
    let resolve_stderr = String::from_utf8_lossy(&resolve_output.stderr)
        .trim()
        .to_string();
    if !resolve_output.status.success() {
        return Ok(UsrlValidationReport {
            validator: cli.to_string_lossy().to_string(),
            status: UsrlValidationStatus::Invalid,
            exit_code: resolve_output.status.code(),
            issues: stderr_lines(&resolve_stderr),
            stdout: resolve_stdout,
            stderr: resolve_stderr,
            module_count: None,
            symbol_count: None,
            reference_count: None,
            graph_node_count: None,
            graph_edge_count: None,
        });
    }

    let resolved: Value = serde_json::from_str(&resolve_stdout)
        .with_context(|| format!("failed to parse USRL resolver JSON for {}", path.display()))?;
    Ok(UsrlValidationReport {
        validator: cli.to_string_lossy().to_string(),
        status: UsrlValidationStatus::Valid,
        exit_code: Some(0),
        issues: Vec::new(),
        stdout,
        stderr,
        module_count: json_usize(&resolved, "module_count"),
        symbol_count: json_usize(&resolved, "symbol_count"),
        reference_count: json_usize(&resolved, "reference_count"),
        graph_node_count: json_usize(&resolved, "graph_node_count"),
        graph_edge_count: json_usize(&resolved, "graph_edge_count"),
    })
}

pub fn usrl_paths(root: impl AsRef<Path>) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    collect_usrl_paths(root.as_ref(), &mut paths)?;
    paths.sort();
    Ok(paths)
}

pub fn import_usrl_text(source: &str, reference: &str, title_hint: &str) -> MemoryObject {
    import_usrl_text_with_options(source, reference, title_hint, &UsrlImportOptions::default())
}

pub fn import_usrl_text_with_options(
    source: &str,
    reference: &str,
    title_hint: &str,
    options: &UsrlImportOptions,
) -> MemoryObject {
    let summary = summarize_usrl(source);
    let source_hash = source_hash(source);
    let title = if summary.contracts.is_empty() {
        format!("USRL source {title_hint}")
    } else {
        format!("USRL contract {}", summary.contracts.join(", "))
    };

    let mut memory = MemoryObject::new("usrl-source", title);
    memory.id = format!("mem_usrl_{}", &source_hash[..24]);
    memory.confidence = 0.9;
    memory.created_at = Utc::now();
    memory.updated_at = memory.created_at;
    memory.source = Some(MemorySource {
        kind: "usrl-source".to_string(),
        reference: reference.to_string(),
    });
    memory.summary = format!(
        "USRL source with {} contract(s), {} section(s), {} fact(s), {} rule(s), {} constraint(s), {} stage(s), and {} trigger(s).",
        summary.contracts.len(),
        summary.sections.len(),
        summary.facts.len(),
        summary.rules.len(),
        summary.constraints.len(),
        summary.stages.len(),
        summary.triggers.len(),
    );
    memory.body = source.to_string();
    memory.tags = vec![
        "USRL".to_string(),
        "contract-dsl".to_string(),
        "source-import".to_string(),
    ];
    memory
        .metadata
        .insert("source_hash".to_string(), source_hash.clone());
    memory.metadata.insert(
        "parser".to_string(),
        "cms-usrl-lightweight-importer".to_string(),
    );
    memory
        .metadata
        .insert("visibility".to_string(), options.visibility.clone());
    if let Some(user_id) = &options.user_id {
        memory
            .metadata
            .insert("user_id".to_string(), user_id.clone());
    }
    if let Some(project_id) = &options.project_id {
        memory
            .metadata
            .insert("project_id".to_string(), project_id.clone());
    }

    let mut claim_id = 1usize;
    for contract in &summary.contracts {
        memory.claims.push(claim(
            claim_id,
            format!("USRL contract `{contract}` is declared."),
            reference,
        ));
        claim_id += 1;
        memory.links.push(link(
            &memory.id,
            format!("usrl-contract:{contract}"),
            "declares_contract",
        ));
    }

    for section in &summary.sections {
        memory.claims.push(claim(
            claim_id,
            format!("USRL section `{section}` is declared."),
            reference,
        ));
        claim_id += 1;
        memory.links.push(link(
            &memory.id,
            format!("usrl-section:{section}"),
            "has_section",
        ));
    }

    for rule in &summary.rules {
        memory
            .links
            .push(link(&memory.id, format!("usrl-rule:{rule}"), "has_rule"));
    }
    for fact in &summary.facts {
        memory
            .links
            .push(link(&memory.id, format!("usrl-fact:{fact}"), "has_fact"));
    }
    for constraint in &summary.constraints {
        memory.links.push(link(
            &memory.id,
            format!("usrl-constraint:{constraint}"),
            "has_constraint",
        ));
    }
    for stage in &summary.stages {
        memory
            .links
            .push(link(&memory.id, format!("usrl-stage:{stage}"), "has_stage"));
    }
    for trigger in &summary.triggers {
        memory.links.push(link(
            &memory.id,
            format!("usrl-trigger:{trigger}"),
            "has_trigger",
        ));
    }

    memory
}

pub fn summarize_usrl(source: &str) -> UsrlSummary {
    let mut summary = UsrlSummary {
        contracts: Vec::new(),
        sections: Vec::new(),
        facts: Vec::new(),
        rules: Vec::new(),
        constraints: Vec::new(),
        stages: Vec::new(),
        triggers: Vec::new(),
    };

    for statement in source.lines().map(strip_line_comment) {
        let statement = statement.trim();
        capture_named_decl(statement, "contract", &mut summary.contracts);
        capture_named_decl(statement, "section", &mut summary.sections);
        capture_named_decl(statement, "fact", &mut summary.facts);
        capture_named_decl(statement, "rule", &mut summary.rules);
        capture_named_decl(statement, "constraint", &mut summary.constraints);
        capture_named_decl(statement, "stage", &mut summary.stages);
        capture_named_decl(statement, "trigger", &mut summary.triggers);
    }

    summary.contracts.sort();
    summary.contracts.dedup();
    summary.sections.sort();
    summary.sections.dedup();
    summary.facts.sort();
    summary.facts.dedup();
    summary.rules.sort();
    summary.rules.dedup();
    summary.constraints.sort();
    summary.constraints.dedup();
    summary.stages.sort();
    summary.stages.dedup();
    summary.triggers.sort();
    summary.triggers.dedup();
    summary
}

fn capture_named_decl(line: &str, keyword: &str, out: &mut Vec<String>) {
    let Some(rest) = line.strip_prefix(keyword) else {
        return;
    };
    if !rest.starts_with(char::is_whitespace) {
        return;
    }
    if let Some(name) = rest.split_whitespace().next() {
        let name = name
            .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '.')
            .to_string();
        if !name.is_empty() {
            out.push(name);
        }
    }
}

fn strip_line_comment(line: &str) -> &str {
    line.split_once("//")
        .map(|(before_comment, _)| before_comment)
        .unwrap_or(line)
}

fn source_hash(source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn apply_validation_metadata(memory: &mut MemoryObject, validation: &UsrlValidationReport) {
    memory
        .metadata
        .insert("usrl_validator".to_string(), validation.validator.clone());
    memory.metadata.insert(
        "usrl_validation_status".to_string(),
        format!("{:?}", validation.status),
    );
    memory.metadata.insert(
        "usrl_validation_issue_count".to_string(),
        validation.issues.len().to_string(),
    );
    if let Some(count) = validation.module_count {
        memory
            .metadata
            .insert("usrl_module_count".to_string(), count.to_string());
    }
    if let Some(count) = validation.symbol_count {
        memory
            .metadata
            .insert("usrl_symbol_count".to_string(), count.to_string());
    }
    if let Some(count) = validation.reference_count {
        memory
            .metadata
            .insert("usrl_reference_count".to_string(), count.to_string());
    }
    if let Some(count) = validation.graph_node_count {
        memory
            .metadata
            .insert("usrl_graph_node_count".to_string(), count.to_string());
    }
    if let Some(count) = validation.graph_edge_count {
        memory
            .metadata
            .insert("usrl_graph_edge_count".to_string(), count.to_string());
    }
}

fn stderr_lines(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn json_usize(value: &Value, key: &str) -> Option<usize> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
}

fn metadata_string<'a>(metadata: &'a Metadata, key: &str) -> Option<&'a str> {
    metadata.get(key).and_then(Value::as_str)
}

fn is_no_memory_value(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "none" | "no-memory" | "no_memory" | "disabled" | "off"
    )
}

fn claim(id: usize, text: String, source: &str) -> Claim {
    Claim {
        id: format!("claim_{id:03}"),
        text,
        confidence: 0.9,
        source: Some(source.to_string()),
    }
}

fn link(source_id: &str, target_id: String, relation: &str) -> MemoryLink {
    MemoryLink {
        source_id: source_id.to_string(),
        target_id,
        relation: relation.to_string(),
        confidence: 0.9,
    }
}

fn collect_usrl_paths(path: &Path, paths: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if path.is_file() {
        if path
            .extension()
            .is_some_and(|extension| extension == "usrl")
        {
            paths.push(path.to_path_buf());
        }
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if should_skip_dir(&path) {
            continue;
        }
        if path.is_dir() {
            collect_usrl_paths(&path, paths)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "usrl")
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn should_skip_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    matches!(
        name,
        ".git" | ".claude" | ".codex" | "node_modules" | "dist" | "target"
    )
}
