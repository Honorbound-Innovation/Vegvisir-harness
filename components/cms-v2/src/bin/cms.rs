use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use cms_v2::MemoryObject;
use cms_v2::archive::{
    ArchiveExportOptions, ArchiveRedactionPolicy, ArchiveScopeFilter, export_archive_with_options,
    export_json_with_options, restore_archive, restore_json_export,
};
use cms_v2::cms_api::{
    CmsMemoryClient, Metadata, ProjectId, RetrievalMode as ApiRetrievalMode, RetrievalRequest,
};
use cms_v2::cms_runtime::LocalCmsMemoryClient;
use cms_v2::data_import::{
    ChatGptImportOptions, DocumentImportOptions, ImportPreview, ImportReport, JsonlImportOptions,
    document_paths, import_chatgpt_export, import_document_file, import_document_tree_with_report,
    import_jsonl_file, infer_document_source_kind, preview_chatgpt_export, preview_document_file,
    preview_jsonl_file, report_chatgpt_export, report_document_file, report_jsonl_file,
};
use cms_v2::diagnostics::run_diagnostics;
use cms_v2::ecm::{
    ContextBudget, ContextMode, ContextRequest, ContextSession, EterniumContextManager, UserId,
};
use cms_v2::graph::{GraphIndex, SqliteGraphIndex};
use cms_v2::lml::{LmlParser, LmlValidator, LmlWriter};
use cms_v2::maintenance::{
    LmlMaintenanceEngine, MaintenanceEngine, MaintenanceRepairer, Reindexer, lml_paths,
};
use cms_v2::prompt_cache::{
    CacheScopeIdentity, PromptCacheEngine, PromptCachePrepareRequest, PromptCacheUsage,
    prompt_cache_trace,
};
use cms_v2::rag::{HybridRagOrchestrator, RagOrchestrator, RetrievalMode};
use cms_v2::safety::detect_sensitive_content;
use cms_v2::sqlite::{LedgerStats, MemoryStatusFilter, SqliteLedger};
use cms_v2::usrl::{
    MemoryVisibility, ScopeResolution, UsrlImportOptions, UsrlValidationReport,
    import_usrl_file_with_options, usrl_paths, validate_usrl_file,
};
use cms_v2::vectors::{SqliteVectorIndex, VectorIndex};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
struct StatusOutput {
    stats: LedgerStats,
    maintenance_issues: usize,
}

#[derive(Debug, Serialize)]
struct UsrlImportOutput {
    memory: MemoryObject,
    validation: UsrlValidationReport,
}

#[derive(Debug, Serialize)]
struct UsrlDirImportOutput {
    source_root: PathBuf,
    count: usize,
    imports: Vec<UsrlImportOutput>,
}

#[derive(Debug, Parser)]
#[command(name = "cms")]
#[command(about = "CMS v2 hybrid memory CLI")]
struct Cli {
    #[arg(long, default_value = "cms.sqlite3")]
    db: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init,
    Validate {
        path: PathBuf,
    },
    Ingest {
        path: PathBuf,
    },
    IngestDir {
        path: PathBuf,
    },
    ImportUsrl {
        path: PathBuf,
        #[arg(long)]
        ingest: bool,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value = "public")]
        visibility: String,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        validator_root: Option<PathBuf>,
        #[arg(long)]
        require_validation: bool,
        #[arg(long)]
        json: bool,
    },
    ImportUsrlDir {
        path: PathBuf,
        #[arg(long)]
        ingest: bool,
        #[arg(long)]
        output_dir: Option<PathBuf>,
        #[arg(long, default_value = "public")]
        visibility: String,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        validator_root: Option<PathBuf>,
        #[arg(long)]
        require_validation: bool,
        #[arg(long)]
        json: bool,
    },
    ImportChatgpt {
        path: PathBuf,
        #[arg(long)]
        ingest: bool,
        #[arg(long)]
        output_dir: Option<PathBuf>,
        #[arg(long, default_value_t = 40)]
        messages_per_memory: usize,
        #[arg(long, default_value_t = 0)]
        max_chars_per_memory: usize,
        #[arg(long)]
        preview: bool,
        #[arg(long)]
        json: bool,
    },
    ImportDoc {
        path: PathBuf,
        #[arg(long)]
        ingest: bool,
        #[arg(long)]
        output_dir: Option<PathBuf>,
        #[arg(long)]
        preview: bool,
        #[arg(long, default_value_t = 8_000)]
        max_chars_per_memory: usize,
        #[arg(long)]
        source_kind: Option<String>,
        #[arg(long)]
        json: bool,
    },
    ImportDocDir {
        path: PathBuf,
        #[arg(long)]
        ingest: bool,
        #[arg(long)]
        output_dir: Option<PathBuf>,
        #[arg(long)]
        preview: bool,
        #[arg(long, default_value_t = 8_000)]
        max_chars_per_memory: usize,
        #[arg(long)]
        json: bool,
    },
    ImportJsonl {
        path: PathBuf,
        #[arg(long)]
        ingest: bool,
        #[arg(long)]
        output_dir: Option<PathBuf>,
        #[arg(long)]
        preview: bool,
        #[arg(long, default_value = "jsonl-record")]
        source_kind: String,
        #[arg(long)]
        json: bool,
    },
    Check {
        #[arg(default_value = "memories")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Status {
        #[arg(default_value = "memories")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Get {
        id: String,
        #[arg(long)]
        json: bool,
    },
    List {
        #[arg(long, default_value = "active")]
        status: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    Scope {
        #[command(subcommand)]
        command: ScopeCommand,
    },
    Delete {
        id: String,
    },
    Archive {
        id: String,
    },
    Quarantine {
        id: String,
    },
    Supersede {
        id: String,
        replacement_id: String,
    },
    Merge {
        duplicate_id: String,
        canonical_id: String,
    },
    Restore {
        id: String,
    },
    Search {
        query: String,
        #[arg(long, default_value_t = 12)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    Retrieve {
        query: String,
        #[arg(long, default_value_t = 12)]
        limit: usize,
        #[arg(long, default_value = "hybrid")]
        mode: String,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        visibility: Option<String>,
        #[arg(long)]
        correlation_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    PrepareContext {
        message: String,
        #[arg(long, default_value = "project")]
        mode: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value = "default")]
        user: String,
        #[arg(long, default_value_t = 16_000)]
        max_tokens: usize,
        #[arg(long, default_value_t = 4_000)]
        reserved_response_tokens: usize,
        #[arg(long)]
        correlation_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    PrepareModelRequest {
        message: String,
        #[arg(long, default_value = "project")]
        mode: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value = "default")]
        user: String,
        #[arg(long, default_value = "local")]
        provider: String,
        #[arg(long, default_value = "unspecified")]
        model: String,
        #[arg(long, default_value_t = 16_000)]
        max_tokens: usize,
        #[arg(long, default_value_t = 4_000)]
        reserved_response_tokens: usize,
        #[arg(long)]
        correlation_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    PromptCache {
        #[command(subcommand)]
        command: PromptCacheCommand,
    },
    CompleteTurn {
        user_message: String,
        assistant_response: String,
        #[arg(long, default_value = "default")]
        user: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        correlation_id: Option<String>,
        #[arg(long)]
        commit: bool,
        #[arg(long)]
        json: bool,
    },
    GraphRelated {
        id: String,
        #[arg(long, default_value_t = 2)]
        depth: usize,
        #[arg(long)]
        json: bool,
    },
    SemanticSearch {
        query: String,
        #[arg(long, default_value_t = 12)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    Reindex {
        #[arg(long)]
        id: Option<String>,
        #[arg(long, default_value = "cms-lexical-v1")]
        vector_provider: String,
        #[arg(long, default_value = "default-chunks-v1")]
        vector_chunking: String,
    },
    Repair {
        #[arg(default_value = "memories")]
        path: PathBuf,
        #[arg(long, default_value = "cms-lexical-v1")]
        vector_provider: String,
        #[arg(long, default_value = "default-chunks-v1")]
        vector_chunking: String,
        #[arg(long)]
        json: bool,
    },
    History {
        id: String,
        #[arg(long)]
        json: bool,
    },
    Audit {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    Diagnostics {
        #[arg(default_value = "memories")]
        path: PathBuf,
        #[arg(long, default_value_t = 20)]
        audit_limit: usize,
        #[arg(long)]
        json: bool,
    },
    RoundTrip {
        input: PathBuf,
        output: PathBuf,
    },
    ExportArchive {
        output: PathBuf,
        #[arg(long)]
        visibility: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        exclude_private: bool,
        #[arg(long)]
        redact_sensitive: bool,
        #[arg(long)]
        json: bool,
    },
    ExportJson {
        output: PathBuf,
        #[arg(long)]
        visibility: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        exclude_private: bool,
        #[arg(long)]
        redact_sensitive: bool,
        #[arg(long)]
        json: bool,
    },
    BackupDb {
        output: PathBuf,
        #[arg(long)]
        json: bool,
    },
    RestoreArchive {
        input: PathBuf,
        #[arg(long)]
        json: bool,
    },
    RestoreJson {
        input: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ScopeCommand {
    Resolve {
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        visibility: Option<String>,
        #[arg(long)]
        no_memory: bool,
        #[arg(long)]
        json: bool,
    },
    Inspect {
        id: String,
        #[arg(long)]
        json: bool,
    },
    List {
        #[arg(long, default_value = "active")]
        status: String,
        #[arg(long)]
        visibility: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum PromptCacheCommand {
    Plan {
        message: String,
        #[arg(long, default_value = "project")]
        mode: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value = "default")]
        user: String,
        #[arg(long, default_value = "local")]
        provider: String,
        #[arg(long, default_value = "unspecified")]
        model: String,
        #[arg(long, default_value_t = 16_000)]
        max_tokens: usize,
        #[arg(long, default_value_t = 4_000)]
        reserved_response_tokens: usize,
        #[arg(long)]
        json: bool,
    },
    Inspect {
        manifest_id: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    Usage {
        manifest_id: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    RecordUsage {
        manifest_id: String,
        #[arg(long, default_value_t = 0)]
        provider_cached_input_tokens: usize,
        #[arg(long, default_value_t = 0)]
        provider_cache_write_tokens: usize,
        #[arg(long, default_value_t = 0)]
        provider_cache_read_tokens: usize,
        #[arg(long, default_value_t = 0)]
        latency_ms: usize,
        #[arg(long)]
        json: bool,
    },
    Capsules {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    Invalidate {
        manifest_id: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        changed_source: Option<String>,
        #[arg(long)]
        json: bool,
    },
    InvalidateSource {
        source_memory_id: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        json: bool,
    },
    InvalidateScope {
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        shared_scope: Option<String>,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        json: bool,
    },
    ExplainMiss {
        manifest_id: String,
        #[arg(long)]
        json: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init => {
            let _ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to initialize {}", cli.db.display()))?;
            println!("initialized {}", cli.db.display());
        }
        Command::Validate { path } => {
            let memory = LmlParser::parse_file(&path)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            LmlValidator::validate(&memory)?;
            println!("valid {} ({})", memory.id, memory.title);
        }
        Command::Ingest { path } => {
            let memory = LmlParser::parse_file(&path)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            let mut ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            ledger.upsert_memory(&memory, Some(&path))?;
            SqliteGraphIndex::new(&ledger).upsert_memory(&memory)?;
            SqliteVectorIndex::new(&ledger).upsert_memory(&memory)?;
            println!("ingested {} ({})", memory.id, memory.title);
        }
        Command::IngestDir { path } => {
            let mut ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let mut count = 0usize;
            for lml_path in lml_paths(&path)? {
                let memory = LmlParser::parse_file(&lml_path)
                    .with_context(|| format!("failed to parse {}", lml_path.display()))?;
                ledger.upsert_memory(&memory, Some(&lml_path))?;
                SqliteGraphIndex::new(&ledger).upsert_memory(&memory)?;
                SqliteVectorIndex::new(&ledger).upsert_memory(&memory)?;
                println!("ingested {} ({})", memory.id, memory.title);
                count += 1;
            }
            println!("ingested {count} memories from {}", path.display());
        }
        Command::ImportUsrl {
            path,
            ingest,
            output,
            visibility,
            user,
            project,
            validator_root,
            require_validation,
            json,
        } => {
            let options = UsrlImportOptions {
                visibility,
                user_id: user,
                project_id: project,
                validator_root,
                require_authoritative_validation: require_validation,
            };
            let validation = validate_usrl_file(&path, &options)
                .with_context(|| format!("failed to validate USRL {}", path.display()))?;
            let memory = import_usrl_file_with_options(&path, &options)
                .with_context(|| format!("failed to import USRL {}", path.display()))?;
            if ingest {
                let mut ledger = SqliteLedger::open(&cli.db)
                    .with_context(|| format!("failed to open {}", cli.db.display()))?;
                ledger.upsert_memory(&memory, Some(&path))?;
                SqliteGraphIndex::new(&ledger).upsert_memory(&memory)?;
                SqliteVectorIndex::new(&ledger).upsert_memory(&memory)?;
                log_import_audit_event(
                    &ledger,
                    "usrl-source-v1",
                    &path,
                    1,
                    0,
                    usize::from(!validation.issues.is_empty()),
                    None,
                )?;
                if !json {
                    println!("ingested {} ({})", memory.id, memory.title);
                }
            }
            if let Some(output) = output {
                LmlWriter::write_file(&memory, &output)
                    .with_context(|| format!("failed to write {}", output.display()))?;
                println!("wrote {}", output.display());
            } else if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&UsrlImportOutput { memory, validation })?
                );
            } else if !ingest {
                print!("{}", LmlWriter::to_text(&memory)?);
            }
        }
        Command::ImportUsrlDir {
            path,
            ingest,
            output_dir,
            visibility,
            user,
            project,
            validator_root,
            require_validation,
            json,
        } => {
            let options = UsrlImportOptions {
                visibility,
                user_id: user,
                project_id: project,
                validator_root,
                require_authoritative_validation: require_validation,
            };
            let paths = usrl_paths(&path)?;
            let mut count = 0usize;
            let mut reports = Vec::new();
            let mut ledger = if ingest {
                Some(
                    SqliteLedger::open(&cli.db)
                        .with_context(|| format!("failed to open {}", cli.db.display()))?,
                )
            } else {
                None
            };
            if let Some(output_dir) = &output_dir {
                std::fs::create_dir_all(output_dir)
                    .with_context(|| format!("failed to create {}", output_dir.display()))?;
            }

            for usrl_path in paths {
                let validation = validate_usrl_file(&usrl_path, &options)
                    .with_context(|| format!("failed to validate USRL {}", usrl_path.display()))?;
                let memory = import_usrl_file_with_options(&usrl_path, &options)
                    .with_context(|| format!("failed to import USRL {}", usrl_path.display()))?;
                if let Some(ledger) = ledger.as_mut() {
                    ledger.upsert_memory(&memory, Some(&usrl_path))?;
                    SqliteGraphIndex::new(ledger).upsert_memory(&memory)?;
                    SqliteVectorIndex::new(ledger).upsert_memory(&memory)?;
                    log_import_audit_event(
                        ledger,
                        "usrl-source-v1",
                        &usrl_path,
                        1,
                        0,
                        usize::from(!validation.issues.is_empty()),
                        None,
                    )?;
                }
                if let Some(output_dir) = &output_dir {
                    let output = output_dir.join(format!("{}.lml", memory.id));
                    LmlWriter::write_file(&memory, &output)
                        .with_context(|| format!("failed to write {}", output.display()))?;
                }
                if json {
                    reports.push(UsrlImportOutput { memory, validation });
                } else {
                    println!("imported {} ({})", memory.id, memory.title);
                }
                count += 1;
            }
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&UsrlDirImportOutput {
                        source_root: path,
                        count,
                        imports: reports,
                    })?
                );
            } else {
                println!(
                    "imported {count} USRL source file(s) from {}",
                    path.display()
                );
            }
        }
        Command::ImportChatgpt {
            path,
            ingest,
            output_dir,
            messages_per_memory,
            max_chars_per_memory,
            preview,
            json,
        } => {
            let options = ChatGptImportOptions {
                messages_per_memory,
                max_chars_per_memory,
            };
            let import_preview = preview_chatgpt_export(&path, &options)
                .with_context(|| format!("failed to inspect ChatGPT export {}", path.display()))?;
            if preview {
                if json {
                    println!("{}", serde_json::to_string_pretty(&import_preview)?);
                } else {
                    println!("source_kind\t{}", import_preview.source_kind);
                    println!("source_reference\t{}", import_preview.source_reference);
                    println!("conversations\t{}", import_preview.conversations);
                    println!("messages\t{}", import_preview.messages);
                    println!("memories\t{}", import_preview.memories);
                }
                return Ok(());
            }

            let mut import_report = if json {
                Some(report_chatgpt_export(&path, &options).with_context(|| {
                    format!("failed to report ChatGPT export {}", path.display())
                })?)
            } else {
                None
            };
            let memories = import_chatgpt_export(&path, &options)
                .with_context(|| format!("failed to import ChatGPT export {}", path.display()))?;
            let mut ledger = if ingest {
                Some(
                    SqliteLedger::open(&cli.db)
                        .with_context(|| format!("failed to open {}", cli.db.display()))?,
                )
            } else {
                None
            };
            if let Some(output_dir) = &output_dir {
                std::fs::create_dir_all(output_dir)
                    .with_context(|| format!("failed to create {}", output_dir.display()))?;
            }

            for memory in &memories {
                if let Some(ledger) = ledger.as_mut() {
                    ingest_imported_memory(ledger, memory, Some(&path), import_report.as_mut())?;
                }
                if let Some(output_dir) = &output_dir {
                    let output = output_dir.join(format!("{}.lml", memory.id));
                    LmlWriter::write_file(memory, &output)
                        .with_context(|| format!("failed to write {}", output.display()))?;
                }
                if !json {
                    println!("imported {} ({})", memory.id, memory.title);
                }
            }
            if let Some(ledger) = ledger.as_ref() {
                log_import_report_audit_event(ledger, &path, &import_report, memories.len())?;
            }

            if json {
                println!("{}", serde_json::to_string_pretty(&import_report)?);
            } else {
                println!(
                    "imported {} ChatGPT conversation(s), {} message(s), {} memory object(s)",
                    import_preview.conversations, import_preview.messages, import_preview.memories
                );
            }
        }
        Command::ImportDoc {
            path,
            ingest,
            output_dir,
            preview,
            max_chars_per_memory,
            source_kind,
            json,
        } => {
            let options = DocumentImportOptions {
                max_chars_per_memory,
                source_kind: source_kind.unwrap_or_else(|| document_source_kind(&path)),
            };
            let import_preview = preview_document_file(&path, &options)
                .with_context(|| format!("failed to inspect document {}", path.display()))?;
            if preview {
                if json {
                    println!("{}", serde_json::to_string_pretty(&import_preview)?);
                } else {
                    println!("source_kind\t{}", import_preview.source_kind);
                    println!("source_reference\t{}", import_preview.source_reference);
                    println!("memories\t{}", import_preview.memories);
                }
                return Ok(());
            }

            let mut import_report = if json {
                Some(report_document_file(&path, &options).with_context(|| {
                    format!("failed to report document import {}", path.display())
                })?)
            } else {
                None
            };
            let memories = import_document_file(&path, &options)
                .with_context(|| format!("failed to import document {}", path.display()))?;
            let mut ledger = if ingest {
                Some(
                    SqliteLedger::open(&cli.db)
                        .with_context(|| format!("failed to open {}", cli.db.display()))?,
                )
            } else {
                None
            };
            if let Some(output_dir) = &output_dir {
                std::fs::create_dir_all(output_dir)
                    .with_context(|| format!("failed to create {}", output_dir.display()))?;
            }
            for memory in &memories {
                if let Some(ledger) = ledger.as_mut() {
                    ingest_imported_memory(ledger, memory, Some(&path), import_report.as_mut())?;
                }
                if let Some(output_dir) = &output_dir {
                    let output = output_dir.join(format!("{}.lml", memory.id));
                    LmlWriter::write_file(memory, &output)
                        .with_context(|| format!("failed to write {}", output.display()))?;
                }
                if !json {
                    println!("imported {} ({})", memory.id, memory.title);
                }
            }
            if let Some(ledger) = ledger.as_ref() {
                log_import_report_audit_event(ledger, &path, &import_report, memories.len())?;
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&import_report)?);
            } else {
                println!(
                    "imported {} document memory object(s) from {}",
                    import_preview.memories,
                    path.display()
                );
            }
        }
        Command::ImportDocDir {
            path,
            ingest,
            output_dir,
            preview,
            max_chars_per_memory,
            json,
        } => {
            let options = DocumentImportOptions {
                max_chars_per_memory,
                source_kind: "document".to_string(),
            };
            let (import_report, memories) = import_document_tree_with_report(&path, &options)
                .with_context(|| {
                    format!("failed to inspect document directory {}", path.display())
                })?;
            let import_preview = ImportPreview {
                source_kind: import_report.source_kind.clone(),
                source_reference: import_report.source_reference.clone(),
                conversations: import_report.conversations,
                messages: import_report.messages,
                memories: import_report.generated_memories,
            };
            if preview {
                if json {
                    println!("{}", serde_json::to_string_pretty(&import_preview)?);
                } else {
                    println!("source_kind\t{}", import_preview.source_kind);
                    println!("source_reference\t{}", import_preview.source_reference);
                    println!("files\t{}", document_paths(&path)?.len());
                    println!("memories\t{}", import_preview.memories);
                    for warning in &import_report.warnings {
                        println!("warning\t{warning}");
                    }
                }
                return Ok(());
            }

            let mut import_report = Some(import_report);
            let mut ledger = if ingest {
                Some(
                    SqliteLedger::open(&cli.db)
                        .with_context(|| format!("failed to open {}", cli.db.display()))?,
                )
            } else {
                None
            };
            if let Some(output_dir) = &output_dir {
                std::fs::create_dir_all(output_dir)
                    .with_context(|| format!("failed to create {}", output_dir.display()))?;
            }
            for memory in &memories {
                let source_path = memory
                    .source
                    .as_ref()
                    .map(|source| PathBuf::from(&source.reference));
                if let Some(ledger) = ledger.as_mut() {
                    ingest_imported_memory(
                        ledger,
                        memory,
                        source_path.as_deref(),
                        import_report.as_mut(),
                    )?;
                }
                if let Some(output_dir) = &output_dir {
                    let output = output_dir.join(format!("{}.lml", memory.id));
                    LmlWriter::write_file(memory, &output)
                        .with_context(|| format!("failed to write {}", output.display()))?;
                }
                if !json {
                    println!("imported {} ({})", memory.id, memory.title);
                }
            }
            if let Some(ledger) = ledger.as_ref() {
                log_import_report_audit_event(ledger, &path, &import_report, memories.len())?;
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&import_report)?);
            } else {
                println!(
                    "imported {} document memory object(s) from {}",
                    import_preview.memories,
                    path.display()
                );
                if let Some(import_report) = &import_report {
                    for warning in &import_report.warnings {
                        println!("warning\t{warning}");
                    }
                }
            }
        }
        Command::ImportJsonl {
            path,
            ingest,
            output_dir,
            preview,
            source_kind,
            json,
        } => {
            let options = JsonlImportOptions { source_kind };
            let import_preview = preview_jsonl_file(&path, &options)
                .with_context(|| format!("failed to inspect JSONL import {}", path.display()))?;
            if preview {
                if json {
                    println!("{}", serde_json::to_string_pretty(&import_preview)?);
                } else {
                    println!("source_kind\t{}", import_preview.source_kind);
                    println!("source_reference\t{}", import_preview.source_reference);
                    println!("records\t{}", import_preview.messages);
                    println!("memories\t{}", import_preview.memories);
                }
                return Ok(());
            }

            let mut import_report =
                if json {
                    Some(report_jsonl_file(&path, &options).with_context(|| {
                        format!("failed to report JSONL import {}", path.display())
                    })?)
                } else {
                    None
                };
            let memories = import_jsonl_file(&path, &options)
                .with_context(|| format!("failed to import JSONL {}", path.display()))?;
            let mut ledger = if ingest {
                Some(
                    SqliteLedger::open(&cli.db)
                        .with_context(|| format!("failed to open {}", cli.db.display()))?,
                )
            } else {
                None
            };
            if let Some(output_dir) = &output_dir {
                std::fs::create_dir_all(output_dir)
                    .with_context(|| format!("failed to create {}", output_dir.display()))?;
            }
            for memory in &memories {
                if let Some(ledger) = ledger.as_mut() {
                    ingest_imported_memory(ledger, memory, Some(&path), import_report.as_mut())?;
                }
                if let Some(output_dir) = &output_dir {
                    let output = output_dir.join(format!("{}.lml", memory.id));
                    LmlWriter::write_file(memory, &output)
                        .with_context(|| format!("failed to write {}", output.display()))?;
                }
                if !json {
                    println!("imported {} ({})", memory.id, memory.title);
                }
            }
            if let Some(ledger) = ledger.as_ref() {
                log_import_report_audit_event(ledger, &path, &import_report, memories.len())?;
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&import_report)?);
            } else {
                println!(
                    "imported {} JSONL record memory object(s) from {}",
                    import_preview.memories,
                    path.display()
                );
                if let Some(import_report) = &import_report {
                    for warning in &import_report.warnings {
                        println!("warning\t{warning}");
                    }
                }
            }
        }
        Command::Check { path, json } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let engine = LmlMaintenanceEngine::new(&ledger, &path);
            let report = engine.run_full_check()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
            if report.is_clean() {
                if !json {
                    println!("clean {}", path.display());
                }
            } else {
                if !json {
                    for issue in &report.issues {
                        if let Some(path) = &issue.path {
                            println!(
                                "{}\t{}\t{}\t{}",
                                issue.severity,
                                issue.id,
                                path.display(),
                                issue.message
                            );
                        } else {
                            println!("{}\t{}\t{}", issue.severity, issue.id, issue.message);
                        }
                    }
                }
                bail!("maintenance check found {} issue(s)", report.issues.len());
            }
        }
        Command::Status { path, json } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let stats = ledger.stats()?;
            let maintenance = LmlMaintenanceEngine::new(&ledger, &path).run_full_check()?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&StatusOutput {
                        stats,
                        maintenance_issues: maintenance.issues.len(),
                    })?
                );
            } else {
                println!("schema_version\t{}", stats.schema_version);
                println!("active_memories\t{}", stats.active_memories);
                println!("deleted_memories\t{}", stats.deleted_memories);
                println!("archived_memories\t{}", stats.archived_memories);
                println!("quarantined_memories\t{}", stats.quarantined_memories);
                println!("superseded_memories\t{}", stats.superseded_memories);
                println!("claims\t{}", stats.claims);
                println!("links\t{}", stats.links);
                println!("tags\t{}", stats.tags);
                println!("versions\t{}", stats.versions);
                println!("retrieval_logs\t{}", stats.retrieval_logs);
                println!("audit_events\t{}", stats.audit_events);
                println!("graph_nodes\t{}", stats.graph_nodes);
                println!("graph_edges\t{}", stats.graph_edges);
                println!("vector_chunks\t{}", stats.vector_chunks);
                println!("vector_terms\t{}", stats.vector_terms);
                println!("graph_indexed_memories\t{}", stats.graph_indexed_memories);
                println!("vector_indexed_memories\t{}", stats.vector_indexed_memories);
                println!("maintenance_issues\t{}", maintenance.issues.len());
            }
        }
        Command::Get { id, json } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let Some(memory) = ledger.get_memory(&id)? else {
                bail!("memory not found: {id}");
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&memory)?);
            } else {
                print!("{}", LmlWriter::to_text(&memory)?);
            }
        }
        Command::List {
            status,
            limit,
            json,
        } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let entries = ledger.list_memories(MemoryStatusFilter::parse(&status)?, limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                for entry in entries {
                    println!(
                        "{}\t{}\t{}\t{}\t{}",
                        entry.status,
                        entry.updated_at.to_rfc3339(),
                        entry.id,
                        entry.memory_type,
                        entry.title
                    );
                }
            }
        }
        Command::Scope { command } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            match command {
                ScopeCommand::Resolve {
                    user,
                    project,
                    visibility,
                    no_memory,
                    json,
                } => {
                    let visibility = visibility
                        .as_deref()
                        .map(|value| {
                            MemoryVisibility::parse(value)
                                .with_context(|| format!("unknown visibility: {value}"))
                        })
                        .transpose()?;
                    let scope = ScopeResolution {
                        user_id: user,
                        project_id: project,
                        visibility,
                        no_memory,
                    };
                    let filters = scope.to_metadata();
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "scope": scope,
                                "retrieval_filters": filters,
                            }))?
                        );
                    } else {
                        println!("user_id\t{}", scope.user_id.as_deref().unwrap_or("-"));
                        println!("project_id\t{}", scope.project_id.as_deref().unwrap_or("-"));
                        println!(
                            "visibility\t{}",
                            scope
                                .visibility
                                .map(MemoryVisibility::as_str)
                                .unwrap_or("-")
                        );
                        println!("no_memory\t{}", scope.no_memory);
                    }
                }
                ScopeCommand::Inspect { id, json } => {
                    let Some(entry) = ledger.get_memory_scope(&id)? else {
                        bail!("memory not found: {id}");
                    };
                    if json {
                        println!("{}", serde_json::to_string_pretty(&entry)?);
                    } else {
                        println!("id\t{}", entry.id);
                        println!("status\t{}", entry.status);
                        println!("visibility\t{}", entry.visibility);
                        println!(
                            "user_id\t{}",
                            entry.user_id.unwrap_or_else(|| "-".to_string())
                        );
                        println!(
                            "project_id\t{}",
                            entry.project_id.unwrap_or_else(|| "-".to_string())
                        );
                    }
                }
                ScopeCommand::List {
                    status,
                    visibility,
                    user,
                    project,
                    limit,
                    json,
                } => {
                    let entries = ledger.list_memories_by_scope(
                        MemoryStatusFilter::parse(&status)?,
                        visibility.as_deref(),
                        user.as_deref(),
                        project.as_deref(),
                        limit,
                    )?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&entries)?);
                    } else {
                        for entry in entries {
                            println!(
                                "{}\t{}\t{}\t{}\t{}\t{}",
                                entry.status,
                                entry.visibility,
                                entry.user_id.unwrap_or_else(|| "-".to_string()),
                                entry.project_id.unwrap_or_else(|| "-".to_string()),
                                entry.id,
                                entry.title
                            );
                        }
                    }
                }
            }
        }
        Command::Delete { id } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            if ledger.soft_delete_memory(&id)? {
                SqliteGraphIndex::new(&ledger).delete_memory(&id)?;
                SqliteVectorIndex::new(&ledger).delete_memory(&id)?;
                println!("deleted {id}");
            } else {
                bail!("active memory not found: {id}");
            }
        }
        Command::Archive { id } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            if ledger.archive_memory(&id)? {
                SqliteGraphIndex::new(&ledger).delete_memory(&id)?;
                SqliteVectorIndex::new(&ledger).delete_memory(&id)?;
                println!("archived {id}");
            } else {
                bail!("active memory not found: {id}");
            }
        }
        Command::Quarantine { id } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            if ledger.quarantine_memory(&id)? {
                SqliteGraphIndex::new(&ledger).delete_memory(&id)?;
                SqliteVectorIndex::new(&ledger).delete_memory(&id)?;
                println!("quarantined {id}");
            } else {
                bail!("active memory not found: {id}");
            }
        }
        Command::Supersede { id, replacement_id } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            if ledger.supersede_memory(&id, &replacement_id)? {
                SqliteGraphIndex::new(&ledger).delete_memory(&id)?;
                SqliteVectorIndex::new(&ledger).delete_memory(&id)?;
                println!("superseded {id} by {replacement_id}");
            } else {
                bail!("active memory not found: {id}");
            }
        }
        Command::Merge {
            duplicate_id,
            canonical_id,
        } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            if ledger.merge_memory(&duplicate_id, &canonical_id)? {
                SqliteGraphIndex::new(&ledger).delete_memory(&duplicate_id)?;
                SqliteVectorIndex::new(&ledger).delete_memory(&duplicate_id)?;
                println!("merged {duplicate_id} into {canonical_id}");
            } else {
                bail!("active duplicate memory not found: {duplicate_id}");
            }
        }
        Command::Restore { id } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            if ledger.restore_memory(&id)? {
                if !Reindexer::new(&ledger).reindex_memory(&id)? {
                    bail!("restored memory could not be reindexed: {id}");
                }
                println!("restored {id}");
            } else {
                bail!("inactive memory not found: {id}");
            }
        }
        Command::Search { query, limit, json } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let results = ledger.search_exact(&query, limit)?;
            ledger.log_retrieval(
                &query,
                &results
                    .iter()
                    .map(|result| result.memory.id.clone())
                    .collect::<Vec<_>>(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                for result in results {
                    println!(
                        "{}\t{:.3}\t{}",
                        result.memory.id, result.score, result.memory.title
                    );
                }
            }
        }
        Command::Retrieve {
            query,
            limit,
            mode,
            user,
            project,
            visibility,
            correlation_id,
            json,
        } => {
            let mut ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            if user.is_some()
                || project.is_some()
                || visibility.is_some()
                || correlation_id.is_some()
            {
                let mut filters = Metadata::new();
                if let Some(user) = user {
                    filters.insert("user_id".to_string(), serde_json::Value::String(user));
                }
                if let Some(visibility) = visibility {
                    filters.insert(
                        "visibility".to_string(),
                        serde_json::Value::String(visibility),
                    );
                }
                if let Some(correlation_id) = correlation_id {
                    filters.insert(
                        "correlation_id".to_string(),
                        serde_json::Value::String(correlation_id),
                    );
                }
                let project_id = project.map(ProjectId);
                let client = LocalCmsMemoryClient::new(&mut ledger);
                let bundle = client.retrieve(RetrievalRequest {
                    query,
                    project_id,
                    modes: vec![parse_api_mode(&mode)?],
                    memory_types: Vec::new(),
                    limit,
                    graph_depth: 2,
                    include_contradictions: false,
                    filters,
                })?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&bundle)?);
                } else {
                    for result in bundle.results {
                        println!(
                            "{}\t{:.3}\t{}",
                            result.memory.id.0, result.score, result.memory.title
                        );
                    }
                }
            } else {
                let orchestrator = HybridRagOrchestrator::new(&ledger);
                let bundle = orchestrator.retrieve(&query, parse_mode(&mode)?, limit)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&bundle)?);
                } else {
                    println!("{}", bundle.context);
                }
            }
        }
        Command::PrepareContext {
            message,
            mode,
            project,
            user,
            max_tokens,
            reserved_response_tokens,
            correlation_id,
            json,
        } => {
            let mut ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut ledger));
            let mut metadata = Metadata::new();
            if let Some(correlation_id) = correlation_id {
                metadata.insert(
                    "correlation_id".to_string(),
                    serde_json::Value::String(correlation_id),
                );
            }
            let prepared = ecm.prepare_context(ContextRequest {
                user_id: UserId(user),
                project_id: project.map(ProjectId),
                session: None,
                message,
                mode: parse_context_mode(&mode)?,
                budget: ContextBudget {
                    max_tokens,
                    reserved_for_response: reserved_response_tokens,
                    reserved_for_system: 1_000,
                    reserved_for_tools: 1_000,
                },
                metadata,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&prepared)?);
            } else {
                println!("{}", prepared.packed_text);
            }
        }
        Command::PrepareModelRequest {
            message,
            mode,
            project,
            user,
            provider,
            model,
            max_tokens,
            reserved_response_tokens,
            correlation_id,
            json,
        } => {
            let mut ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let project_id = project.map(ProjectId);
            let mut metadata = Metadata::new();
            if let Some(correlation_id) = correlation_id {
                metadata.insert(
                    "correlation_id".to_string(),
                    serde_json::Value::String(correlation_id),
                );
            }
            let prepared = {
                let ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut ledger));
                ecm.prepare_context(ContextRequest {
                    user_id: UserId(user.clone()),
                    project_id: project_id.clone(),
                    session: None,
                    message,
                    mode: parse_context_mode(&mode)?,
                    budget: ContextBudget {
                        max_tokens,
                        reserved_for_response: reserved_response_tokens,
                        reserved_for_system: 1_000,
                        reserved_for_tools: 1_000,
                    },
                    metadata,
                })?
            };
            let request = PromptCachePrepareRequest {
                provider,
                model,
                scope_identity: CacheScopeIdentity {
                    organization_id: None,
                    user_id: Some(user),
                    project_id: project_id.map(|id| id.0),
                    session_id: Some(prepared.session_id.0.clone()),
                    shared_scope_id: None,
                },
                ..Default::default()
            };
            let envelope = PromptCacheEngine::prepare_model_prompt(&prepared, request);
            let capsule_ids = envelope
                .capsules
                .iter()
                .map(|capsule| capsule.capsule_id.clone())
                .collect::<Vec<_>>();
            let (local_capsule_hits, local_capsule_misses) =
                ledger.prompt_cache_capsule_reuse_counts(&capsule_ids)?;
            let mut trace = prompt_cache_trace(&prepared, &envelope);
            trace.local_capsule_hits = local_capsule_hits;
            trace.local_capsule_misses = local_capsule_misses;
            ledger.put_prompt_cache_envelope(&envelope)?;
            ledger.record_prompt_cache_usage(&PromptCacheUsage {
                manifest_id: envelope.manifest.manifest_id.clone(),
                provider: envelope.manifest.provider.clone(),
                model: envelope.manifest.model.clone(),
                total_input_tokens: envelope.manifest.total_prompt_tokens,
                provider_cached_input_tokens: 0,
                provider_cache_write_tokens: 0,
                provider_cache_read_tokens: 0,
                local_capsule_hits,
                local_capsule_misses,
                latency_ms: 0,
            })?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "prepared_context": prepared,
                        "cached_prompt": envelope,
                        "prompt_cache_trace": trace,
                    }))?
                );
            } else {
                println!("{}", envelope.model_request.prompt);
                println!(
                    "\ncache_key={}\ncacheable_prefix_tokens={}\ndynamic_suffix_blocks={}",
                    envelope.manifest.prompt_cache_key,
                    envelope.manifest.cacheable_prefix_tokens,
                    envelope.cache_plan.dynamic_suffix_blocks.len()
                );
            }
        }
        Command::PromptCache { command } => {
            let mut ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            match command {
                PromptCacheCommand::Plan {
                    message,
                    mode,
                    project,
                    user,
                    provider,
                    model,
                    max_tokens,
                    reserved_response_tokens,
                    json,
                } => {
                    let project_id = project.map(ProjectId);
                    let prepared = {
                        let ecm =
                            EterniumContextManager::new(LocalCmsMemoryClient::new(&mut ledger));
                        ecm.prepare_context(ContextRequest {
                            user_id: UserId(user.clone()),
                            project_id: project_id.clone(),
                            session: None,
                            message,
                            mode: parse_context_mode(&mode)?,
                            budget: ContextBudget {
                                max_tokens,
                                reserved_for_response: reserved_response_tokens,
                                reserved_for_system: 1_000,
                                reserved_for_tools: 1_000,
                            },
                            metadata: Default::default(),
                        })?
                    };
                    let envelope = PromptCacheEngine::prepare_model_prompt(
                        &prepared,
                        PromptCachePrepareRequest {
                            provider,
                            model,
                            scope_identity: CacheScopeIdentity {
                                organization_id: None,
                                user_id: Some(user),
                                project_id: project_id.map(|id| id.0),
                                session_id: Some(prepared.session_id.0.clone()),
                                shared_scope_id: None,
                            },
                            ..Default::default()
                        },
                    );
                    let capsule_ids = envelope
                        .capsules
                        .iter()
                        .map(|capsule| capsule.capsule_id.clone())
                        .collect::<Vec<_>>();
                    let (local_capsule_hits, local_capsule_misses) =
                        ledger.prompt_cache_capsule_reuse_counts(&capsule_ids)?;
                    let mut trace = prompt_cache_trace(&prepared, &envelope);
                    trace.local_capsule_hits = local_capsule_hits;
                    trace.local_capsule_misses = local_capsule_misses;
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "prepared_context": prepared,
                                "cached_prompt": envelope,
                                "prompt_cache_trace": trace,
                                "persisted": false,
                            }))?
                        );
                    } else {
                        println!(
                            "{}\tkey={}\tprefix_tokens={}\tpersisted=false",
                            envelope.manifest.manifest_id,
                            envelope.manifest.prompt_cache_key,
                            envelope.manifest.cacheable_prefix_tokens
                        );
                    }
                }
                PromptCacheCommand::Inspect {
                    manifest_id,
                    limit,
                    json,
                } => {
                    if let Some(manifest_id) = manifest_id {
                        let Some(record) = ledger.get_prompt_cache_manifest(&manifest_id)? else {
                            bail!("unknown prompt cache manifest: {manifest_id}");
                        };
                        if json {
                            println!("{}", serde_json::to_string_pretty(&record)?);
                        } else {
                            print_prompt_cache_manifest(&record.manifest);
                        }
                    } else {
                        let records = ledger.prompt_cache_manifests(limit)?;
                        if json {
                            println!("{}", serde_json::to_string_pretty(&records)?);
                        } else {
                            for record in records {
                                print_prompt_cache_manifest(&record.manifest);
                            }
                        }
                    }
                }
                PromptCacheCommand::Usage {
                    manifest_id,
                    limit,
                    json,
                } => {
                    let records = ledger.prompt_cache_usage(manifest_id.as_deref(), limit)?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&records)?);
                    } else {
                        for record in records {
                            println!(
                                "{}\t{}\t{}/{}\ttotal={}\tcached={}",
                                record.id,
                                record.usage.manifest_id,
                                record.usage.provider,
                                record.usage.model,
                                record.usage.total_input_tokens,
                                record.usage.provider_cached_input_tokens
                            );
                        }
                    }
                }
                PromptCacheCommand::RecordUsage {
                    manifest_id,
                    provider_cached_input_tokens,
                    provider_cache_write_tokens,
                    provider_cache_read_tokens,
                    latency_ms,
                    json,
                } => {
                    let Some(record) = ledger.get_prompt_cache_manifest(&manifest_id)? else {
                        bail!("unknown prompt cache manifest: {manifest_id}");
                    };
                    let usage = PromptCacheUsage {
                        manifest_id,
                        provider: record.manifest.provider,
                        model: record.manifest.model,
                        total_input_tokens: record.manifest.total_prompt_tokens,
                        provider_cached_input_tokens,
                        provider_cache_write_tokens,
                        provider_cache_read_tokens,
                        local_capsule_hits: 0,
                        local_capsule_misses: 0,
                        latency_ms,
                    };
                    let id = ledger.record_prompt_cache_usage(&usage)?;
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "id": id,
                                "usage": usage,
                            }))?
                        );
                    } else {
                        println!("{id}\t{}", usage.manifest_id);
                    }
                }
                PromptCacheCommand::Capsules { limit, json } => {
                    let records = ledger.prompt_cache_capsules(limit)?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&records)?);
                    } else {
                        for record in records {
                            println!(
                                "{}\t{:?}\t{:?}\ttokens={}\tuses={}",
                                record.capsule.capsule_id,
                                record.capsule.capsule_type,
                                record.capsule.scope,
                                record.capsule.token_estimate,
                                record.use_count
                            );
                        }
                    }
                }
                PromptCacheCommand::Invalidate {
                    manifest_id,
                    reason,
                    changed_source,
                    json,
                } => {
                    let record = ledger.invalidate_prompt_cache_manifest(
                        &manifest_id,
                        &reason,
                        changed_source.as_deref(),
                    )?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&record)?);
                    } else {
                        println!("{}\t{}\t{}", record.id, record.manifest_id, record.reason);
                    }
                }
                PromptCacheCommand::InvalidateSource {
                    source_memory_id,
                    reason,
                    json,
                } => {
                    let records =
                        ledger.invalidate_prompt_cache_by_source(&source_memory_id, &reason)?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&records)?);
                    } else {
                        for record in records {
                            println!("{}\t{}\t{}", record.id, record.manifest_id, record.reason);
                        }
                    }
                }
                PromptCacheCommand::InvalidateScope {
                    user,
                    project,
                    session,
                    shared_scope,
                    reason,
                    json,
                } => {
                    let report = ledger.invalidate_prompt_cache_by_scope_filter(
                        user.as_deref(),
                        project.as_deref(),
                        session.as_deref(),
                        shared_scope.as_deref(),
                        &reason,
                    )?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&report)?);
                    } else {
                        println!(
                            "invalidations\t{}\nevicted_capsules\t{}",
                            report.invalidations.len(),
                            report.evicted_capsules
                        );
                    }
                }
                PromptCacheCommand::ExplainMiss { manifest_id, json } => {
                    let invalidations = ledger.prompt_cache_invalidations(&manifest_id, 1)?;
                    let explanation = if let Some(record) = invalidations.first() {
                        serde_json::json!({
                            "manifest_id": manifest_id,
                            "cache_valid": false,
                            "miss_reason": record.reason,
                            "changed_source": record.changed_source,
                            "invalidation_id": record.id,
                            "invalidated_at": record.created_at,
                        })
                    } else {
                        serde_json::json!({
                            "manifest_id": manifest_id,
                            "cache_valid": true,
                            "miss_reason": null,
                            "changed_source": null,
                        })
                    };
                    if json {
                        println!("{}", serde_json::to_string_pretty(&explanation)?);
                    } else if explanation["cache_valid"].as_bool().unwrap_or(false) {
                        println!("{manifest_id}\tvalid");
                    } else {
                        println!(
                            "{}\tmiss\t{}",
                            manifest_id,
                            explanation["miss_reason"].as_str().unwrap_or("unknown")
                        );
                    }
                }
            }
        }
        Command::CompleteTurn {
            user_message,
            assistant_response,
            user,
            project,
            correlation_id,
            commit,
            json,
        } => {
            let mut ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let project_id = project.map(ProjectId);
            let mut session = ContextSession::new(UserId(user), project_id);
            if let Some(correlation_id) = correlation_id {
                session.metadata.insert(
                    "correlation_id".to_string(),
                    serde_json::Value::String(correlation_id),
                );
            }
            let mut ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut ledger));
            if commit {
                let results = ecm.complete_turn(&session, &user_message, &assistant_response)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&results)?);
                } else {
                    for result in results {
                        println!(
                            "{}\tcreated={}\tupdated={}",
                            result.memory_id.0, result.created_new, result.updated_existing
                        );
                    }
                }
            } else {
                let candidates =
                    ecm.evaluate_writeback(&session, &user_message, &assistant_response);
                if json {
                    println!("{}", serde_json::to_string_pretty(&candidates)?);
                } else {
                    for candidate in candidates {
                        println!(
                            "{:?}\t{}\t{}",
                            candidate.candidate_type,
                            candidate.suggested_memory_type,
                            candidate.title
                        );
                    }
                }
            }
        }
        Command::GraphRelated { id, depth, json } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let graph = SqliteGraphIndex::new(&ledger);
            let hits = graph.related_memories(&id, depth)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&hits)?);
            } else {
                for hit in hits {
                    println!("{}\t{}\tdepth={}", hit.memory_id, hit.relation, hit.depth);
                }
            }
        }
        Command::SemanticSearch { query, limit, json } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let vector = SqliteVectorIndex::new(&ledger);
            let hits = vector.semantic_search(&query, limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&hits)?);
            } else {
                for hit in hits {
                    println!("{}\t{}\t{:.3}", hit.memory_id, hit.chunk_id, hit.score);
                }
            }
        }
        Command::Reindex {
            id,
            vector_provider,
            vector_chunking,
        } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let reindexer =
                Reindexer::with_vector_provider(&ledger, vector_provider, vector_chunking);
            if let Some(id) = id {
                if reindexer.reindex_memory(&id)? {
                    println!("reindexed {id}");
                } else {
                    bail!("memory not found: {id}");
                }
            } else {
                let count = reindexer.reindex_all()?;
                println!("reindexed {count} memories");
            }
        }
        Command::Repair {
            path,
            vector_provider,
            vector_chunking,
            json,
        } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let report = MaintenanceRepairer::with_vector_provider(
                &ledger,
                &path,
                vector_provider,
                vector_chunking,
            )
            .repair_derived_indexes()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("repaired_issues\t{}", report.repaired_issues);
                println!("reindexed_memories\t{}", report.reindexed_memories.len());
                println!("skipped_issues\t{}", report.skipped_issues.len());
                for memory_id in report.reindexed_memories {
                    println!("reindexed\t{memory_id}");
                }
            }
        }
        Command::History { id, json } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let versions = ledger.memory_versions(&id)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&versions)?);
            } else {
                for version in versions {
                    println!(
                        "{}\tv{}\t{}\t{}",
                        version.memory_id,
                        version.version,
                        version.content_hash,
                        version.created_at.to_rfc3339()
                    );
                }
            }
        }
        Command::Audit { limit, json } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let events = ledger.audit_events(limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&events)?);
            } else {
                for event in events {
                    println!(
                        "{}\t{}\t{}\t{}",
                        event.created_at.to_rfc3339(),
                        event.event_type,
                        event.memory_id.unwrap_or_else(|| "-".to_string()),
                        event.message
                    );
                }
            }
        }
        Command::Diagnostics {
            path,
            audit_limit,
            json,
        } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let report = run_diagnostics(&ledger, &path, audit_limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("health\t{}", report.health.status);
                println!("schema_issues\t{}", report.health.schema_issues);
                println!("maintenance_issues\t{}", report.health.maintenance_issues);
                println!("error_issues\t{}", report.health.error_issues);
                println!("warning_issues\t{}", report.health.warning_issues);
                println!("schema_version\t{}", report.stats.schema_version);
                println!(
                    "expected_schema_version\t{}",
                    report.schema.expected_version
                );
                println!("schema_current\t{}", report.schema.is_current);
                println!("active_memories\t{}", report.stats.active_memories);
                println!("deleted_memories\t{}", report.stats.deleted_memories);
                println!("prompt_cache_manifests\t{}", report.prompt_cache.manifests);
                println!("prompt_cache_capsules\t{}", report.prompt_cache.capsules);
                println!("prompt_cache_usage\t{}", report.prompt_cache.usage_records);
                println!(
                    "prompt_cache_invalidations\t{}",
                    report.prompt_cache.invalidations
                );
                println!(
                    "observability_audit_events\t{}",
                    report.observability.recent_audit_event_count
                );
                println!(
                    "observability_retrieval_events\t{}",
                    report.observability.retrieval_events
                );
                println!(
                    "observability_prompt_cache_events\t{}",
                    report.observability.prompt_cache_events
                );
                println!(
                    "observability_redacted_audit_messages\t{}",
                    report.observability.redacted_audit_messages
                );
                println!("migrations\t{}", report.migrations.len());
                println!("recent_audit_events\t{}", report.recent_audit_events.len());
            }
        }
        Command::RoundTrip { input, output } => {
            let memory = LmlParser::parse_file(&input)
                .with_context(|| format!("failed to parse {}", input.display()))?;
            LmlWriter::write_file(&memory, &output)
                .with_context(|| format!("failed to write {}", output.display()))?;
            println!("wrote {}", output.display());
        }
        Command::ExportArchive {
            output,
            visibility,
            user,
            project,
            exclude_private,
            redact_sensitive,
            json,
        } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let scope_filter = if visibility.is_some() || user.is_some() || project.is_some() {
                Some(ArchiveScopeFilter {
                    visibility,
                    user_id: user,
                    project_id: project,
                })
            } else {
                None
            };
            let manifest = export_archive_with_options(
                &ledger,
                &output,
                ArchiveExportOptions {
                    scope_filter,
                    redaction_policy: ArchiveRedactionPolicy {
                        exclude_private,
                        redact_sensitive,
                    },
                },
            )
            .with_context(|| format!("failed to export archive {}", output.display()))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&manifest)?);
            } else {
                println!(
                    "exported {} active memory object(s) to {}",
                    manifest.memory_count,
                    output.display()
                );
            }
        }
        Command::ExportJson {
            output,
            visibility,
            user,
            project,
            exclude_private,
            redact_sensitive,
            json,
        } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let scope_filter = if visibility.is_some() || user.is_some() || project.is_some() {
                Some(ArchiveScopeFilter {
                    visibility,
                    user_id: user,
                    project_id: project,
                })
            } else {
                None
            };
            let export = export_json_with_options(
                &ledger,
                &output,
                ArchiveExportOptions {
                    scope_filter,
                    redaction_policy: ArchiveRedactionPolicy {
                        exclude_private,
                        redact_sensitive,
                    },
                },
            )
            .with_context(|| format!("failed to export JSON {}", output.display()))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&export)?);
            } else {
                println!(
                    "exported {} active memory object(s) to {}",
                    export.memory_count,
                    output.display()
                );
            }
        }
        Command::BackupDb { output, json } => {
            let ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let report = ledger
                .backup_to(&output)
                .with_context(|| format!("failed to back up SQLite DB to {}", output.display()))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "backed up SQLite DB to {} ({} bytes)",
                    output.display(),
                    report.backup_size_bytes
                );
            }
        }
        Command::RestoreArchive { input, json } => {
            let mut ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let report = restore_archive(&mut ledger, &input)
                .with_context(|| format!("failed to restore archive {}", input.display()))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "restored {} memory object(s) from {}",
                    report.restored_memories,
                    input.display()
                );
            }
        }
        Command::RestoreJson { input, json } => {
            let mut ledger = SqliteLedger::open(&cli.db)
                .with_context(|| format!("failed to open {}", cli.db.display()))?;
            let report = restore_json_export(&mut ledger, &input)
                .with_context(|| format!("failed to restore JSON export {}", input.display()))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "restored {} memory object(s) from {}",
                    report.restored_memories,
                    input.display()
                );
            }
        }
    }
    Ok(())
}

fn ingest_imported_memory(
    ledger: &mut SqliteLedger,
    memory: &MemoryObject,
    source_path: Option<&std::path::Path>,
    mut report: Option<&mut ImportReport>,
) -> anyhow::Result<()> {
    ledger.upsert_memory(memory, source_path)?;
    let sensitive_findings = imported_memory_sensitive_findings(memory);
    if sensitive_findings.is_empty() {
        SqliteGraphIndex::new(ledger).upsert_memory(memory)?;
        SqliteVectorIndex::new(ledger).upsert_memory(memory)?;
        return Ok(());
    }

    ledger.quarantine_memory(&memory.id)?;
    SqliteGraphIndex::new(ledger).delete_memory(&memory.id)?;
    SqliteVectorIndex::new(ledger).delete_memory(&memory.id)?;
    if let Some(report) = report.as_mut() {
        if !report.quarantined_memory_ids.contains(&memory.id) {
            report.quarantined_memory_ids.push(memory.id.clone());
        }
        report.warnings.push(format!(
            "quarantined {} due to {} sensitive finding(s)",
            memory.id,
            sensitive_findings.len()
        ));
    }
    Ok(())
}

fn log_import_report_audit_event(
    ledger: &SqliteLedger,
    source_path: &std::path::Path,
    report: &Option<ImportReport>,
    fallback_generated: usize,
) -> anyhow::Result<()> {
    if let Some(report) = report {
        log_import_audit_event(
            ledger,
            &report.importer_version,
            source_path,
            report.generated_memories,
            report.quarantined_memory_ids.len(),
            report.warnings.len(),
            Some(&report.import_batch_hash),
        )
    } else {
        log_import_audit_event(
            ledger,
            "importer-unreported",
            source_path,
            fallback_generated,
            0,
            0,
            None,
        )
    }
}

fn log_import_audit_event(
    ledger: &SqliteLedger,
    importer: &str,
    source_path: &std::path::Path,
    generated_memories: usize,
    quarantined_memories: usize,
    warnings: usize,
    import_batch_hash: Option<&str>,
) -> anyhow::Result<()> {
    let mut message = format!(
        "import.completed importer={importer} source={} generated_memories={generated_memories} quarantined_memories={quarantined_memories} warnings={warnings}",
        source_path.display()
    );
    if let Some(import_batch_hash) = import_batch_hash {
        message.push_str(&format!(" import_batch_hash={import_batch_hash}"));
    }
    ledger.log_audit_event(None, "import.completed", &message)
}

fn imported_memory_sensitive_findings(
    memory: &MemoryObject,
) -> Vec<cms_v2::safety::SensitiveFinding> {
    let mut findings = Vec::new();
    findings.extend(detect_sensitive_content(&memory.title));
    findings.extend(detect_sensitive_content(&memory.summary));
    findings.extend(detect_sensitive_content(&memory.body));
    for claim in &memory.claims {
        findings.extend(detect_sensitive_content(&claim.text));
    }
    findings
        .sort_by(|left, right| (&left.kind, &left.evidence).cmp(&(&right.kind, &right.evidence)));
    findings.dedup();
    findings
}

fn parse_context_mode(mode: &str) -> anyhow::Result<ContextMode> {
    match mode {
        "minimal" => Ok(ContextMode::Minimal),
        "session" => Ok(ContextMode::Session),
        "project" => Ok(ContextMode::Project),
        "deep-project" | "deep_project" => Ok(ContextMode::DeepProject),
        "research" => Ok(ContextMode::Research),
        "coding" => Ok(ContextMode::Coding),
        "debugging" => Ok(ContextMode::Debugging),
        "architecture" => Ok(ContextMode::Architecture),
        "memory-recall" | "memory_recall" => Ok(ContextMode::MemoryRecall),
        "decision-review" | "decision_review" => Ok(ContextMode::DecisionReview),
        _ => bail!("unknown context mode: {mode}"),
    }
}

fn print_prompt_cache_manifest(manifest: &cms_v2::prompt_cache::PromptCacheManifest) {
    println!(
        "{}\t{}/{}\tprefix_tokens={}\ttotal_tokens={}\tkey={}",
        manifest.manifest_id,
        manifest.provider,
        manifest.model,
        manifest.cacheable_prefix_tokens,
        manifest.total_prompt_tokens,
        manifest.prompt_cache_key
    );
}

fn parse_mode(mode: &str) -> anyhow::Result<RetrievalMode> {
    match mode {
        "exact" => Ok(RetrievalMode::Exact),
        "semantic" => Ok(RetrievalMode::Semantic),
        "graph" => Ok(RetrievalMode::Graph),
        "hybrid" => Ok(RetrievalMode::Hybrid),
        "recent" => Ok(RetrievalMode::Recent),
        "project" => Ok(RetrievalMode::Project),
        "contradiction" => Ok(RetrievalMode::Contradiction),
        "decision-history" | "decision_history" => Ok(RetrievalMode::DecisionHistory),
        _ => bail!("unknown retrieval mode: {mode}"),
    }
}

fn parse_api_mode(mode: &str) -> anyhow::Result<ApiRetrievalMode> {
    match mode {
        "exact" => Ok(ApiRetrievalMode::Exact),
        "semantic" => Ok(ApiRetrievalMode::Semantic),
        "graph" => Ok(ApiRetrievalMode::Graph),
        "hybrid" => Ok(ApiRetrievalMode::Hybrid),
        "recent" => Ok(ApiRetrievalMode::Recent),
        "project" => Ok(ApiRetrievalMode::Project),
        "contradiction" => Ok(ApiRetrievalMode::Contradiction),
        "decision-history" | "decision_history" => Ok(ApiRetrievalMode::DecisionHistory),
        _ => bail!("unknown retrieval mode: {mode}"),
    }
}

fn document_source_kind(path: &std::path::Path) -> String {
    infer_document_source_kind(path)
}
