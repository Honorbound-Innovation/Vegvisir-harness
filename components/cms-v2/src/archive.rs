use crate::lml::{CURRENT_LML_SCHEMA_VERSION, LmlParser, LmlWriter};
use crate::safety::contains_sensitive_content;
use crate::sqlite::{CURRENT_SCHEMA_VERSION, MemoryStatusFilter, SqliteLedger};
use crate::{
    core::MemoryObject,
    graph::{GraphIndex, SqliteGraphIndex},
    vectors::{SqliteVectorIndex, VectorIndex},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveManifest {
    pub archive_version: i64,
    pub created_at: DateTime<Utc>,
    pub cms_schema_version: i64,
    pub lml_schema_version: i64,
    #[serde(default)]
    pub scope_filter: Option<ArchiveScopeFilter>,
    #[serde(default)]
    pub redaction_policy: Option<ArchiveRedactionPolicy>,
    pub memory_count: usize,
    pub memories: Vec<ArchiveMemoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveScopeFilter {
    pub visibility: Option<String>,
    pub user_id: Option<String>,
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ArchiveRedactionPolicy {
    #[serde(default)]
    pub exclude_private: bool,
    #[serde(default)]
    pub redact_sensitive: bool,
}

impl ArchiveRedactionPolicy {
    pub fn is_empty(self) -> bool {
        !self.exclude_private && !self.redact_sensitive
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ArchiveExportOptions {
    #[serde(default)]
    pub scope_filter: Option<ArchiveScopeFilter>,
    #[serde(default)]
    pub redaction_policy: ArchiveRedactionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveMemoryEntry {
    pub id: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonMemoryExport {
    pub export_version: i64,
    pub created_at: DateTime<Utc>,
    pub cms_schema_version: i64,
    pub lml_schema_version: i64,
    #[serde(default)]
    pub scope_filter: Option<ArchiveScopeFilter>,
    #[serde(default)]
    pub redaction_policy: Option<ArchiveRedactionPolicy>,
    pub memory_count: usize,
    pub memories: Vec<MemoryObject>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveRestoreReport {
    pub archive_version: i64,
    pub restored_memories: usize,
    pub memory_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRestoreReport {
    pub export_version: i64,
    pub restored_memories: usize,
    pub memory_ids: Vec<String>,
}

pub fn export_archive(
    ledger: &SqliteLedger,
    output_dir: impl AsRef<Path>,
) -> anyhow::Result<ArchiveManifest> {
    export_archive_scoped(ledger, output_dir, None)
}

pub fn export_archive_scoped(
    ledger: &SqliteLedger,
    output_dir: impl AsRef<Path>,
    scope_filter: Option<ArchiveScopeFilter>,
) -> anyhow::Result<ArchiveManifest> {
    export_archive_with_options(
        ledger,
        output_dir,
        ArchiveExportOptions {
            scope_filter,
            redaction_policy: ArchiveRedactionPolicy::default(),
        },
    )
}

pub fn export_archive_with_options(
    ledger: &SqliteLedger,
    output_dir: impl AsRef<Path>,
    options: ArchiveExportOptions,
) -> anyhow::Result<ArchiveManifest> {
    let output_dir = output_dir.as_ref();
    let memories_dir = output_dir.join("memories");
    fs::create_dir_all(&memories_dir)?;

    let mut entries = Vec::new();
    let memory_ids = if let Some(scope_filter) = &options.scope_filter {
        ledger
            .list_memories_by_scope(
                MemoryStatusFilter::Active,
                scope_filter.visibility.as_deref(),
                scope_filter.user_id.as_deref(),
                scope_filter.project_id.as_deref(),
                1_000_000,
            )?
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>()
    } else {
        ledger.all_memory_ids()?
    };

    for memory_id in memory_ids {
        let Some(memory) = ledger.get_memory(&memory_id)? else {
            continue;
        };
        if options.redaction_policy.exclude_private && is_private_memory(&memory) {
            continue;
        }
        let exported_memory = if options.redaction_policy.redact_sensitive {
            redact_sensitive_memory(memory)
        } else {
            memory
        };
        let relative_path = PathBuf::from("memories").join(format!("{memory_id}.lml"));
        let output_path = output_dir.join(&relative_path);
        LmlWriter::write_file(&exported_memory, &output_path)?;
        entries.push(ArchiveMemoryEntry {
            id: memory_id,
            path: relative_path.to_string_lossy().to_string(),
        });
    }

    entries.sort_by(|left, right| left.id.cmp(&right.id));
    let manifest = ArchiveManifest {
        archive_version: 1,
        created_at: Utc::now(),
        cms_schema_version: CURRENT_SCHEMA_VERSION,
        lml_schema_version: CURRENT_LML_SCHEMA_VERSION,
        scope_filter: options.scope_filter,
        redaction_policy: (!options.redaction_policy.is_empty())
            .then_some(options.redaction_policy),
        memory_count: entries.len(),
        memories: entries,
    };
    fs::write(
        output_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

pub fn export_json_with_options(
    ledger: &SqliteLedger,
    output_file: impl AsRef<Path>,
    options: ArchiveExportOptions,
) -> anyhow::Result<JsonMemoryExport> {
    let export = build_json_export(ledger, options)?;
    if let Some(parent) = output_file
        .as_ref()
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_file, serde_json::to_string_pretty(&export)?)?;
    Ok(export)
}

pub fn build_json_export(
    ledger: &SqliteLedger,
    options: ArchiveExportOptions,
) -> anyhow::Result<JsonMemoryExport> {
    let scope_filter = options.scope_filter.clone();
    let memories = export_memories(ledger, &options)?;
    Ok(JsonMemoryExport {
        export_version: 1,
        created_at: Utc::now(),
        cms_schema_version: CURRENT_SCHEMA_VERSION,
        lml_schema_version: CURRENT_LML_SCHEMA_VERSION,
        scope_filter,
        redaction_policy: (!options.redaction_policy.is_empty())
            .then_some(options.redaction_policy),
        memory_count: memories.len(),
        memories,
    })
}

fn export_memories(
    ledger: &SqliteLedger,
    options: &ArchiveExportOptions,
) -> anyhow::Result<Vec<MemoryObject>> {
    let memory_ids = if let Some(scope_filter) = &options.scope_filter {
        ledger
            .list_memories_by_scope(
                MemoryStatusFilter::Active,
                scope_filter.visibility.as_deref(),
                scope_filter.user_id.as_deref(),
                scope_filter.project_id.as_deref(),
                1_000_000,
            )?
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>()
    } else {
        ledger.all_memory_ids()?
    };

    let mut memories = Vec::new();
    for memory_id in memory_ids {
        let Some(memory) = ledger.get_memory(&memory_id)? else {
            continue;
        };
        if options.redaction_policy.exclude_private && is_private_memory(&memory) {
            continue;
        }
        memories.push(if options.redaction_policy.redact_sensitive {
            redact_sensitive_memory(memory)
        } else {
            memory
        });
    }
    memories.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(memories)
}

fn is_private_memory(memory: &MemoryObject) -> bool {
    memory
        .metadata
        .get("visibility")
        .is_some_and(|visibility| visibility.eq_ignore_ascii_case("private"))
}

fn redact_sensitive_memory(mut memory: MemoryObject) -> MemoryObject {
    let mut redacted = false;
    redact_text_field(&mut memory.title, &mut redacted);
    redact_text_field(&mut memory.summary, &mut redacted);
    redact_text_field(&mut memory.body, &mut redacted);
    for claim in &mut memory.claims {
        redact_text_field(&mut claim.text, &mut redacted);
        if let Some(source) = &mut claim.source {
            redact_text_field(source, &mut redacted);
        }
    }
    if let Some(source) = &mut memory.source {
        redact_text_field(&mut source.reference, &mut redacted);
    }
    for (key, value) in &mut memory.metadata {
        if is_sensitive_metadata_pair(key, value) || contains_sensitive_content(value) {
            *value = "[redacted sensitive content]".to_string();
            redacted = true;
        }
    }
    if redacted {
        memory
            .metadata
            .insert("archive_redacted".to_string(), "true".to_string());
    }
    memory
}

fn redact_text_field(field: &mut String, redacted: &mut bool) {
    if contains_sensitive_content(field) {
        *field = "[redacted sensitive content]".to_string();
        *redacted = true;
    }
}

fn is_sensitive_metadata_pair(key: &str, value: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "api_key" | "apikey" | "access_token" | "secret_key" | "private_key" | "password" | "token"
    ) || (matches!(
        normalized.as_str(),
        "prompt_cache_sensitivity" | "sensitivity"
    ) && matches!(
        value.to_ascii_lowercase().as_str(),
        "secret" | "sensitive" | "private" | "restricted"
    ))
}

pub fn restore_archive(
    ledger: &mut SqliteLedger,
    input_dir: impl AsRef<Path>,
) -> anyhow::Result<ArchiveRestoreReport> {
    let input_dir = input_dir.as_ref();
    let manifest = read_manifest(input_dir)?;
    let mut restored = Vec::new();

    for entry in &manifest.memories {
        let memory_path = input_dir.join(&entry.path);
        let memory = LmlParser::parse_file(&memory_path)?;
        ledger.upsert_memory(&memory, Some(&memory_path))?;
        SqliteGraphIndex::new(ledger).upsert_memory(&memory)?;
        SqliteVectorIndex::new(ledger).upsert_memory(&memory)?;
        restored.push(memory.id);
    }

    restored.sort();
    Ok(ArchiveRestoreReport {
        archive_version: manifest.archive_version,
        restored_memories: restored.len(),
        memory_ids: restored,
    })
}

pub fn read_json_export(input_file: impl AsRef<Path>) -> anyhow::Result<JsonMemoryExport> {
    let source = fs::read_to_string(input_file)?;
    Ok(serde_json::from_str(&source)?)
}

pub fn restore_json_export(
    ledger: &mut SqliteLedger,
    input_file: impl AsRef<Path>,
) -> anyhow::Result<JsonRestoreReport> {
    let input_file = input_file.as_ref();
    let export = read_json_export(input_file)?;
    let mut restored = Vec::new();

    for memory in &export.memories {
        ledger.upsert_memory(memory, Some(input_file))?;
        SqliteGraphIndex::new(ledger).upsert_memory(memory)?;
        SqliteVectorIndex::new(ledger).upsert_memory(memory)?;
        restored.push(memory.id.clone());
    }

    restored.sort();
    Ok(JsonRestoreReport {
        export_version: export.export_version,
        restored_memories: restored.len(),
        memory_ids: restored,
    })
}

pub fn read_manifest(input_dir: impl AsRef<Path>) -> anyhow::Result<ArchiveManifest> {
    let source = fs::read_to_string(input_dir.as_ref().join("manifest.json"))?;
    Ok(serde_json::from_str(&source)?)
}
