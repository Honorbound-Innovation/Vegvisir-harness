use crate::graph::{GraphIndex, SqliteGraphIndex};
use crate::lml::LmlParser;
use crate::sqlite::{SqliteLedger, content_hash};
use crate::vectors::{
    LEXICAL_VECTOR_PROVIDER, SqliteVectorIndex, VECTOR_CHUNKING_VERSION, VectorIndex,
    vector_index_hash,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaintenanceIssue {
    pub id: String,
    pub severity: String,
    pub message: String,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MaintenanceReport {
    pub issues: Vec<MaintenanceIssue>,
}

impl MaintenanceReport {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

pub trait MaintenanceEngine {
    fn run_full_check(&self) -> anyhow::Result<MaintenanceReport>;
    fn run_incremental_check(&self) -> anyhow::Result<MaintenanceReport>;
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RepairReport {
    pub repaired_issues: usize,
    pub reindexed_memories: Vec<String>,
    pub invalidated_prompt_cache_manifests: Vec<String>,
    pub skipped_issues: Vec<MaintenanceIssue>,
}

pub struct Reindexer<'a> {
    ledger: &'a SqliteLedger,
    vector_provider: String,
    vector_chunking: String,
}

impl<'a> Reindexer<'a> {
    pub fn new(ledger: &'a SqliteLedger) -> Self {
        Self {
            ledger,
            vector_provider: LEXICAL_VECTOR_PROVIDER.to_string(),
            vector_chunking: VECTOR_CHUNKING_VERSION.to_string(),
        }
    }

    pub fn with_vector_provider(
        ledger: &'a SqliteLedger,
        vector_provider: impl Into<String>,
        vector_chunking: impl Into<String>,
    ) -> Self {
        Self {
            ledger,
            vector_provider: vector_provider.into(),
            vector_chunking: vector_chunking.into(),
        }
    }

    pub fn reindex_memory(&self, memory_id: &str) -> anyhow::Result<bool> {
        let Some(memory) = self.ledger.get_memory(memory_id)? else {
            return Ok(false);
        };
        SqliteGraphIndex::new(self.ledger).upsert_memory(&memory)?;
        SqliteVectorIndex::with_provider(self.ledger, &self.vector_provider, &self.vector_chunking)
            .upsert_memory(&memory)?;
        Ok(true)
    }

    pub fn reindex_all(&self) -> anyhow::Result<usize> {
        self.ledger.clear_derived_indexes()?;
        let mut count = 0usize;
        for memory_id in self.ledger.all_memory_ids()? {
            if self.reindex_memory(&memory_id)? {
                count += 1;
            }
        }
        Ok(count)
    }
}

pub struct LmlMaintenanceEngine<'a> {
    ledger: &'a SqliteLedger,
    root: PathBuf,
}

impl<'a> LmlMaintenanceEngine<'a> {
    pub fn new(ledger: &'a SqliteLedger, root: impl Into<PathBuf>) -> Self {
        Self {
            ledger,
            root: root.into(),
        }
    }

    fn check_lml_tree(&self) -> anyhow::Result<MaintenanceReport> {
        let mut report = MaintenanceReport::default();
        for path in lml_paths(&self.root)? {
            match LmlParser::parse_file(&path) {
                Ok(memory) => {
                    let current_hash = content_hash(&memory);
                    match self.ledger.memory_hash(&memory.id)? {
                        Some(stored_hash) if stored_hash != current_hash => {
                            report.issues.push(issue(
                                "stale-ledger",
                                "warning",
                                format!(
                                    "memory {} has changed on disk since it was indexed",
                                    memory.id
                                ),
                                Some(path.clone()),
                            ));
                        }
                        Some(_) => {}
                        None => report.issues.push(issue(
                            "missing-ledger-record",
                            "warning",
                            format!("memory {} exists on disk but is not in SQLite", memory.id),
                            Some(path.clone()),
                        )),
                    }

                    for index_name in ["sqlite-graph", "sqlite-vector"] {
                        let expected_index_hash = if index_name == "sqlite-vector" {
                            vector_index_hash(&memory)
                        } else {
                            current_hash.clone()
                        };
                        match self.ledger.index_hash(&memory.id, index_name)? {
                            Some(index_hash) if index_hash != expected_index_hash => {
                                report.issues.push(issue(
                                    "stale-derived-index",
                                    "warning",
                                    format!(
                                        "memory {} has stale {index_name} projection",
                                        memory.id
                                    ),
                                    Some(path.clone()),
                                ));
                            }
                            Some(_) => {}
                            None if self.ledger.memory_exists(&memory.id)? => {
                                report.issues.push(issue(
                                    "missing-derived-index",
                                    "warning",
                                    format!("memory {} has no {index_name} projection", memory.id),
                                    Some(path.clone()),
                                ));
                            }
                            None => {}
                        }
                    }

                    for link in memory
                        .links
                        .iter()
                        .filter(|link| is_internal_memory_id(&link.target_id))
                    {
                        if !self.ledger.memory_exists(&link.target_id)? {
                            report.issues.push(issue(
                                "broken-memory-link",
                                "error",
                                format!(
                                    "memory {} links to missing memory {} via {}",
                                    memory.id, link.target_id, link.relation
                                ),
                                Some(path.clone()),
                            ));
                        }
                    }
                }
                Err(err) => report.issues.push(issue(
                    "invalid-lml",
                    "error",
                    format!("failed to parse .lml: {err}"),
                    Some(path),
                )),
            }
        }

        for record in self.ledger.active_memory_records()? {
            let Some(path) = record.lml_path.as_deref().map(PathBuf::from) else {
                continue;
            };
            if !is_lml_path(&path) {
                continue;
            }
            if !path.exists() {
                report.issues.push(issue(
                    "missing-lml-file",
                    "error",
                    format!(
                        "SQLite record {} points to missing .lml file {}",
                        record.id,
                        path.display()
                    ),
                    Some(path),
                ));
            }
        }

        report
            .issues
            .extend(self.detect_duplicate_memories()?.issues);
        Ok(report)
    }

    fn detect_duplicate_memories(&self) -> anyhow::Result<MaintenanceReport> {
        let mut report = MaintenanceReport::default();
        let paths_by_id = self
            .ledger
            .active_memory_records()?
            .into_iter()
            .filter_map(|record| {
                record
                    .lml_path
                    .map(PathBuf::from)
                    .map(|path| (record.id, path))
            })
            .collect::<HashMap<_, _>>();
        let mut seen_by_fingerprint: HashMap<String, String> = HashMap::new();

        for memory_id in self.ledger.all_memory_ids()? {
            let Some(memory) = self.ledger.get_memory(&memory_id)? else {
                continue;
            };
            let Some(fingerprint) = duplicate_fingerprint(&memory) else {
                continue;
            };

            if let Some(canonical_id) = seen_by_fingerprint.get(&fingerprint) {
                report.issues.push(issue(
                    "duplicate-memory",
                    "warning",
                    format!(
                        "memory {memory_id} appears duplicate of {canonical_id}; suggested merge: cms merge {memory_id} {canonical_id}"
                    ),
                    paths_by_id.get(&memory_id).cloned(),
                ));
            } else {
                seen_by_fingerprint.insert(fingerprint, memory_id);
            }
        }

        Ok(report)
    }
}

pub struct MaintenanceRepairer<'a> {
    ledger: &'a SqliteLedger,
    root: PathBuf,
    vector_provider: String,
    vector_chunking: String,
}

impl<'a> MaintenanceRepairer<'a> {
    pub fn new(ledger: &'a SqliteLedger, root: impl Into<PathBuf>) -> Self {
        Self {
            ledger,
            root: root.into(),
            vector_provider: LEXICAL_VECTOR_PROVIDER.to_string(),
            vector_chunking: VECTOR_CHUNKING_VERSION.to_string(),
        }
    }

    pub fn with_vector_provider(
        ledger: &'a SqliteLedger,
        root: impl Into<PathBuf>,
        vector_provider: impl Into<String>,
        vector_chunking: impl Into<String>,
    ) -> Self {
        Self {
            ledger,
            root: root.into(),
            vector_provider: vector_provider.into(),
            vector_chunking: vector_chunking.into(),
        }
    }

    pub fn repair_derived_indexes(&self) -> anyhow::Result<RepairReport> {
        let maintenance = LmlMaintenanceEngine::new(self.ledger, &self.root).run_full_check()?;
        let mut report = RepairReport::default();
        let reindexer = Reindexer::with_vector_provider(
            self.ledger,
            &self.vector_provider,
            &self.vector_chunking,
        );

        for issue in maintenance.issues {
            if !matches!(
                issue.id.as_str(),
                "missing-derived-index" | "stale-derived-index"
            ) {
                report.skipped_issues.push(issue);
                continue;
            }

            let Some(memory_id) = issue_memory_id(&issue) else {
                report.skipped_issues.push(issue);
                continue;
            };
            if reindexer.reindex_memory(&memory_id)? {
                report.repaired_issues += 1;
                if !report.reindexed_memories.contains(&memory_id) {
                    report.reindexed_memories.push(memory_id.clone());
                }
                self.ledger.log_audit_event(
                    Some(&memory_id),
                    "maintenance.repaired",
                    &format!("repaired {} for {memory_id}", issue.id),
                )?;
                for invalidation in self
                    .ledger
                    .invalidate_prompt_cache_by_source(&memory_id, "source-memory-reindexed")?
                {
                    if !report
                        .invalidated_prompt_cache_manifests
                        .contains(&invalidation.manifest_id)
                    {
                        report
                            .invalidated_prompt_cache_manifests
                            .push(invalidation.manifest_id);
                    }
                }
            } else {
                report.skipped_issues.push(issue);
            }
        }

        report.reindexed_memories.sort();
        report.invalidated_prompt_cache_manifests.sort();
        Ok(report)
    }
}

impl MaintenanceEngine for LmlMaintenanceEngine<'_> {
    fn run_full_check(&self) -> anyhow::Result<MaintenanceReport> {
        self.check_lml_tree()
    }

    fn run_incremental_check(&self) -> anyhow::Result<MaintenanceReport> {
        self.check_lml_tree()
    }
}

pub fn lml_paths(root: impl AsRef<Path>) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    collect_lml_paths(root.as_ref(), &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_lml_paths(path: &Path, paths: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if path.is_file() {
        if path.extension().is_some_and(|extension| extension == "lml") {
            paths.push(path.to_path_buf());
        }
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            collect_lml_paths(&entry_path, paths)?;
        } else if entry_path
            .extension()
            .is_some_and(|extension| extension == "lml")
        {
            paths.push(entry_path);
        }
    }
    Ok(())
}

fn is_internal_memory_id(id: &str) -> bool {
    id.starts_with("mem_")
}

fn is_lml_path(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "lml")
}

fn issue_memory_id(issue: &MaintenanceIssue) -> Option<String> {
    let marker = "memory ";
    let start = issue.message.find(marker)? + marker.len();
    let rest = &issue.message[start..];
    let memory_id = rest
        .split_whitespace()
        .next()
        .map(|value| value.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_'))?;
    if memory_id.starts_with("mem_") {
        Some(memory_id.to_string())
    } else {
        None
    }
}

fn duplicate_fingerprint(memory: &crate::core::MemoryObject) -> Option<String> {
    let mut claim_texts = memory
        .claims
        .iter()
        .map(|claim| normalize_for_duplicate_detection(&claim.text))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>();
    claim_texts.sort();

    let mut parts = vec![
        normalize_for_duplicate_detection(&memory.memory_type),
        normalize_for_duplicate_detection(&memory.summary),
        normalize_for_duplicate_detection(&memory.body),
    ];
    parts.extend(claim_texts);

    let fingerprint = parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if fingerprint.len() >= 32 {
        Some(fingerprint)
    } else {
        None
    }
}

fn normalize_for_duplicate_detection(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn issue(
    id: impl Into<String>,
    severity: impl Into<String>,
    message: impl Into<String>,
    path: Option<PathBuf>,
) -> MaintenanceIssue {
    MaintenanceIssue {
        id: id.into(),
        severity: severity.into(),
        message: message.into(),
        path,
    }
}
