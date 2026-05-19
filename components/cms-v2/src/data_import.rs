use crate::core::{Claim, MemoryLink, MemoryObject, MemorySource};
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportPreview {
    pub source_kind: String,
    pub source_reference: String,
    pub conversations: usize,
    pub messages: usize,
    pub memories: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    pub source_kind: String,
    pub source_reference: String,
    pub importer_version: String,
    pub conversations: usize,
    pub messages: usize,
    pub generated_memories: usize,
    pub unique_memories: usize,
    pub duplicate_memories: usize,
    pub memory_ids: Vec<String>,
    pub source_hashes: Vec<String>,
    pub import_batch_hash: String,
    #[serde(default)]
    pub quarantined_memory_ids: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatGptImportOptions {
    pub messages_per_memory: usize,
    pub max_chars_per_memory: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentImportOptions {
    pub max_chars_per_memory: usize,
    pub source_kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonlImportOptions {
    pub source_kind: String,
}

impl Default for DocumentImportOptions {
    fn default() -> Self {
        Self {
            max_chars_per_memory: 8_000,
            source_kind: "document".to_string(),
        }
    }
}

impl Default for JsonlImportOptions {
    fn default() -> Self {
        Self {
            source_kind: "jsonl-record".to_string(),
        }
    }
}

impl Default for ChatGptImportOptions {
    fn default() -> Self {
        Self {
            messages_per_memory: 40,
            max_chars_per_memory: 0,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChatGptConversation {
    id: Option<String>,
    title: Option<String>,
    create_time: Option<f64>,
    update_time: Option<f64>,
    mapping: Option<BTreeMap<String, ChatGptNode>>,
}

#[derive(Debug, Deserialize)]
struct ChatGptNode {
    message: Option<ChatGptMessage>,
}

#[derive(Debug, Deserialize)]
struct ChatGptMessage {
    id: Option<String>,
    author: Option<ChatGptAuthor>,
    create_time: Option<f64>,
    content: Option<ChatGptContent>,
}

#[derive(Debug, Deserialize)]
struct ChatGptAuthor {
    role: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatGptContent {
    content_type: Option<String>,
    parts: Option<Vec<Value>>,
    text: Option<String>,
}

#[derive(Debug, Clone)]
struct ImportedMessage {
    id: String,
    role: String,
    created_at: Option<DateTime<Utc>>,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedDocument {
    body: String,
    frontmatter: BTreeMap<String, String>,
}

pub fn preview_chatgpt_export(
    path: impl AsRef<Path>,
    options: &ChatGptImportOptions,
) -> anyhow::Result<ImportPreview> {
    let source_path = chatgpt_conversations_path(path.as_ref())?;
    let conversations = read_chatgpt_conversations(&source_path)?;
    let messages = conversations
        .iter()
        .map(|conversation| conversation_messages(conversation).len())
        .sum::<usize>();
    Ok(ImportPreview {
        source_kind: "chatgpt-export".to_string(),
        source_reference: source_path.to_string_lossy().to_string(),
        conversations: conversations.len(),
        messages,
        memories: conversations
            .iter()
            .map(|conversation| {
                chatgpt_message_chunks(&conversation_messages(conversation), options).len()
            })
            .sum(),
    })
}

pub fn import_chatgpt_export(
    path: impl AsRef<Path>,
    options: &ChatGptImportOptions,
) -> anyhow::Result<Vec<MemoryObject>> {
    let source_path = chatgpt_conversations_path(path.as_ref())?;
    let conversations = read_chatgpt_conversations(&source_path)?;
    let mut memories = Vec::new();
    for conversation in &conversations {
        memories.extend(import_chatgpt_conversation(
            conversation,
            &source_path,
            options,
        ));
    }
    memories.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(memories)
}

pub fn report_chatgpt_export(
    path: impl AsRef<Path>,
    options: &ChatGptImportOptions,
) -> anyhow::Result<ImportReport> {
    let preview = preview_chatgpt_export(path.as_ref(), options)?;
    let memories = import_chatgpt_export(path, options)?;
    Ok(import_report_from_memories(
        preview,
        "chatgpt-export-v1",
        &memories,
    ))
}

pub fn preview_document_file(
    path: impl AsRef<Path>,
    options: &DocumentImportOptions,
) -> anyhow::Result<ImportPreview> {
    let source = fs::read_to_string(path.as_ref())?;
    let parsed = parse_document_source(&source);
    let chunks = document_chunks(&parsed.body, options.max_chars_per_memory);
    Ok(ImportPreview {
        source_kind: options.source_kind.clone(),
        source_reference: path.as_ref().to_string_lossy().to_string(),
        conversations: 0,
        messages: 0,
        memories: chunks.len(),
    })
}

pub fn document_paths(root: impl AsRef<Path>) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    collect_document_paths(root.as_ref(), &mut paths)?;
    paths.sort();
    Ok(paths)
}

pub fn preview_document_tree(
    root: impl AsRef<Path>,
    options: &DocumentImportOptions,
) -> anyhow::Result<ImportPreview> {
    let mut memories = 0usize;
    for path in document_paths(root.as_ref())? {
        let file_options = options_for_document_path(&path, options);
        memories += preview_document_file(&path, &file_options)?.memories;
    }
    Ok(ImportPreview {
        source_kind: options.source_kind.clone(),
        source_reference: root.as_ref().to_string_lossy().to_string(),
        conversations: 0,
        messages: 0,
        memories,
    })
}

pub fn import_document_file(
    path: impl AsRef<Path>,
    options: &DocumentImportOptions,
) -> anyhow::Result<Vec<MemoryObject>> {
    let path = path.as_ref();
    let source = fs::read_to_string(path)?;
    let parsed = parse_document_source(&source);
    let chunks = document_chunks(&parsed.body, options.max_chars_per_memory);
    let source_reference = path.to_string_lossy().to_string();
    let title = parsed
        .frontmatter
        .get("title")
        .cloned()
        .unwrap_or_else(|| document_title(path, &parsed.body));
    let source_kind = options.source_kind.clone();
    let mut memories = chunks
        .iter()
        .enumerate()
        .map(|(chunk_index, chunk)| {
            let mut memory = imported_document_memory(
                &source_kind,
                &source_reference,
                &title,
                chunk_index,
                chunks.len(),
                chunk,
            );
            for (key, value) in &parsed.frontmatter {
                memory
                    .metadata
                    .insert(format!("frontmatter:{key}"), value.clone());
            }
            memory
        })
        .collect::<Vec<_>>();
    memories.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(memories)
}

pub fn import_document_tree(
    root: impl AsRef<Path>,
    options: &DocumentImportOptions,
) -> anyhow::Result<Vec<MemoryObject>> {
    let mut memories = Vec::new();
    for path in document_paths(root)? {
        let file_options = options_for_document_path(&path, options);
        memories.extend(import_document_file(&path, &file_options)?);
    }
    memories.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(memories)
}

pub fn import_document_tree_with_report(
    root: impl AsRef<Path>,
    options: &DocumentImportOptions,
) -> anyhow::Result<(ImportReport, Vec<MemoryObject>)> {
    let root = root.as_ref();
    let mut memories = Vec::new();
    let mut warnings = Vec::new();

    for path in document_paths(root)? {
        let file_options = options_for_document_path(&path, options);
        match import_document_file(&path, &file_options) {
            Ok(mut file_memories) => memories.append(&mut file_memories),
            Err(error) => warnings.push(format!("skipped {}: {error}", path.display())),
        }
    }

    memories.sort_by(|left, right| left.id.cmp(&right.id));
    let preview = ImportPreview {
        source_kind: options.source_kind.clone(),
        source_reference: root.to_string_lossy().to_string(),
        conversations: 0,
        messages: 0,
        memories: memories.len(),
    };
    let mut report = import_report_from_memories(preview, "generic-document-v1", &memories);
    report.warnings = warnings;
    Ok((report, memories))
}

pub fn report_document_file(
    path: impl AsRef<Path>,
    options: &DocumentImportOptions,
) -> anyhow::Result<ImportReport> {
    let preview = preview_document_file(path.as_ref(), options)?;
    let memories = import_document_file(path, options)?;
    Ok(import_report_from_memories(
        preview,
        "generic-document-v1",
        &memories,
    ))
}

pub fn report_document_tree(
    root: impl AsRef<Path>,
    options: &DocumentImportOptions,
) -> anyhow::Result<ImportReport> {
    Ok(import_document_tree_with_report(root, options)?.0)
}

pub fn preview_jsonl_file(
    path: impl AsRef<Path>,
    options: &JsonlImportOptions,
) -> anyhow::Result<ImportPreview> {
    let report = import_jsonl_file_with_report(path, options)?.0;
    Ok(ImportPreview {
        source_kind: report.source_kind,
        source_reference: report.source_reference,
        conversations: report.conversations,
        messages: report.messages,
        memories: report.generated_memories,
    })
}

pub fn import_jsonl_file(
    path: impl AsRef<Path>,
    options: &JsonlImportOptions,
) -> anyhow::Result<Vec<MemoryObject>> {
    Ok(import_jsonl_file_with_report(path, options)?.1)
}

pub fn report_jsonl_file(
    path: impl AsRef<Path>,
    options: &JsonlImportOptions,
) -> anyhow::Result<ImportReport> {
    Ok(import_jsonl_file_with_report(path, options)?.0)
}

pub fn import_jsonl_file_with_report(
    path: impl AsRef<Path>,
    options: &JsonlImportOptions,
) -> anyhow::Result<(ImportReport, Vec<MemoryObject>)> {
    let path = path.as_ref();
    let source = fs::read_to_string(path)?;
    let source_reference = path.to_string_lossy().to_string();
    let mut memories = Vec::new();
    let mut warnings = Vec::new();

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value = match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => value,
            Err(error) => {
                warnings.push(format!("skipped line {line_number}: {error}"));
                continue;
            }
        };
        let Some(object) = value.as_object() else {
            warnings.push(format!("skipped line {line_number}: expected JSON object"));
            continue;
        };
        let Some(body) = jsonl_record_body(object) else {
            warnings.push(format!(
                "skipped line {line_number}: expected one of body, content, text, or summary"
            ));
            continue;
        };
        let title = object
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| !title.trim().is_empty())
            .map(str::trim)
            .unwrap_or("Imported JSONL record");
        memories.push(imported_jsonl_memory(
            &options.source_kind,
            &source_reference,
            line_number,
            title,
            &body,
            object,
        ));
    }

    memories.sort_by(|left, right| left.id.cmp(&right.id));
    let preview = ImportPreview {
        source_kind: options.source_kind.clone(),
        source_reference,
        conversations: 0,
        messages: memories.len(),
        memories: memories.len(),
    };
    let mut report = import_report_from_memories(preview, "jsonl-record-v1", &memories);
    report.warnings = warnings;
    Ok((report, memories))
}

pub fn infer_document_source_kind(path: &Path) -> String {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("md" | "markdown") => "markdown".to_string(),
        Some("txt" | "text") => "text".to_string(),
        Some(
            "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "c" | "cpp" | "h" | "hpp"
            | "cs" | "toml" | "json" | "yaml" | "yml",
        ) => "code".to_string(),
        _ => "document".to_string(),
    }
}

fn import_chatgpt_conversation(
    conversation: &ChatGptConversation,
    source_path: &Path,
    options: &ChatGptImportOptions,
) -> Vec<MemoryObject> {
    let messages = conversation_messages(conversation);
    if messages.is_empty() {
        return Vec::new();
    }

    let chunks = chatgpt_message_chunks(&messages, options);
    let chunk_count = chunks.len();
    let conversation_id = conversation_id(conversation);
    let title = conversation
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or("Untitled ChatGPT conversation");
    let source_reference = source_path.to_string_lossy().to_string();

    chunks
        .iter()
        .enumerate()
        .map(|(chunk_index, chunk)| {
            let source_hash = chunk_hash(&conversation_id, chunk_index, chunk);
            let mut memory = MemoryObject::new(
                "chatgpt-conversation",
                conversation_title(title, chunk_index, chunk_count),
            );
            memory.id = format!("mem_chatgpt_{}", &source_hash[..24]);
            memory.confidence = 0.85;
            memory.created_at = conversation
                .create_time
                .and_then(timestamp_to_utc)
                .or_else(|| chunk.iter().filter_map(|message| message.created_at).min())
                .unwrap_or_else(Utc::now);
            memory.updated_at = conversation
                .update_time
                .and_then(timestamp_to_utc)
                .or_else(|| chunk.iter().filter_map(|message| message.created_at).max())
                .unwrap_or(memory.created_at);
            memory.source = Some(MemorySource {
                kind: "chatgpt-export".to_string(),
                reference: source_reference.clone(),
            });
            memory.summary = format!(
                "ChatGPT conversation `{title}` chunk {} of {} with {} message(s) from role(s): {}.",
                chunk_index + 1,
                chunk_count,
                chunk.len(),
                roles_summary(chunk)
            );
            memory.body = transcript_body(chunk);
            memory.tags = vec![
                "ChatGPT".to_string(),
                "chat-history".to_string(),
                "source-import".to_string(),
            ];
            memory
                .metadata
                .insert("importer".to_string(), "chatgpt-export-v1".to_string());
            memory
                .metadata
                .insert("conversation_id".to_string(), conversation_id.clone());
            memory
                .metadata
                .insert("conversation_title".to_string(), title.to_string());
            memory
                .metadata
                .insert("chunk_index".to_string(), (chunk_index + 1).to_string());
            memory
                .metadata
                .insert("chunk_count".to_string(), chunk_count.to_string());
            memory
                .metadata
                .insert("message_count".to_string(), chunk.len().to_string());
            memory
                .metadata
                .insert("source_hash".to_string(), source_hash.clone());

            memory.claims.push(Claim {
                id: "claim_001".to_string(),
                text: format!(
                    "Conversation `{title}` contains {} imported ChatGPT message(s) in this chunk.",
                    chunk.len()
                ),
                confidence: 0.85,
                source: Some(source_reference.clone()),
            });
            memory.links.push(MemoryLink {
                source_id: memory.id.clone(),
                target_id: format!("chatgpt-conversation:{conversation_id}"),
                relation: "part_of_conversation".to_string(),
                confidence: 0.95,
            });
            memory
        })
        .collect()
}

fn imported_document_memory(
    source_kind: &str,
    source_reference: &str,
    title: &str,
    chunk_index: usize,
    chunk_count: usize,
    chunk: &str,
) -> MemoryObject {
    let source_hash = document_chunk_hash(source_reference, chunk_index, chunk);
    let memory_title = if chunk_count == 1 {
        title.to_string()
    } else {
        format!("{title} ({}/{chunk_count})", chunk_index + 1)
    };
    let memory_type = match source_kind {
        "markdown" => "markdown-document",
        "code" => "code-document",
        "text" => "text-document",
        _ => "imported-document",
    };
    let mut memory = MemoryObject::new(memory_type, memory_title);
    memory.id = format!("mem_doc_{}", &source_hash[..24]);
    memory.confidence = 0.8;
    memory.created_at = Utc::now();
    memory.updated_at = memory.created_at;
    memory.source = Some(MemorySource {
        kind: source_kind.to_string(),
        reference: source_reference.to_string(),
    });
    memory.summary = format!(
        "Imported {source_kind} document `{title}` chunk {} of {}.",
        chunk_index + 1,
        chunk_count
    );
    memory.body = chunk.trim().to_string();
    memory.tags = vec![
        "source-import".to_string(),
        source_kind.to_string(),
        "document-import".to_string(),
    ];
    memory
        .metadata
        .insert("importer".to_string(), "generic-document-v1".to_string());
    memory
        .metadata
        .insert("source_hash".to_string(), source_hash);
    memory
        .metadata
        .insert("chunk_index".to_string(), (chunk_index + 1).to_string());
    memory
        .metadata
        .insert("chunk_count".to_string(), chunk_count.to_string());
    memory.claims.push(Claim {
        id: "claim_001".to_string(),
        text: format!(
            "Document `{title}` contributes imported {source_kind} content in chunk {} of {}.",
            chunk_index + 1,
            chunk_count
        ),
        confidence: 0.8,
        source: Some(source_reference.to_string()),
    });
    memory.links.push(MemoryLink {
        source_id: memory.id.clone(),
        target_id: format!("document:{}", stable_document_id(source_reference)),
        relation: "part_of_document".to_string(),
        confidence: 0.95,
    });
    memory
}

fn imported_jsonl_memory(
    source_kind: &str,
    source_reference: &str,
    line_number: usize,
    title: &str,
    body: &str,
    object: &serde_json::Map<String, Value>,
) -> MemoryObject {
    let source_hash = jsonl_record_hash(source_reference, line_number, object);
    let mut memory = MemoryObject::new("jsonl-record", title);
    memory.id = format!("mem_jsonl_{}", &source_hash[..24]);
    memory.confidence = object
        .get("confidence")
        .and_then(Value::as_f64)
        .unwrap_or(0.75);
    memory.created_at = object
        .get("created_at")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    memory.updated_at = object
        .get("updated_at")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or(memory.created_at);
    memory.source = Some(MemorySource {
        kind: source_kind.to_string(),
        reference: source_reference.to_string(),
    });
    memory.summary = object
        .get("summary")
        .and_then(Value::as_str)
        .filter(|summary| !summary.trim().is_empty())
        .map(|summary| summary.trim().to_string())
        .unwrap_or_else(|| format!("Imported JSONL record `{title}` from line {line_number}."));
    memory.body = body.trim().to_string();
    memory.tags = object
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    memory.tags.push("source-import".to_string());
    memory.tags.push("jsonl-import".to_string());
    memory.tags.sort();
    memory.tags.dedup();

    if let Some(metadata) = object.get("metadata").and_then(Value::as_object) {
        for (key, value) in metadata {
            if let Some(value) = json_scalar_to_string(value) {
                memory.metadata.insert(key.clone(), value);
            }
        }
    }
    for key in ["visibility", "user_id", "project_id"] {
        if let Some(value) = object.get(key).and_then(Value::as_str) {
            memory.metadata.insert(key.to_string(), value.to_string());
        }
    }
    memory
        .metadata
        .insert("importer".to_string(), "jsonl-record-v1".to_string());
    memory
        .metadata
        .insert("source_hash".to_string(), source_hash);
    memory
        .metadata
        .insert("line_number".to_string(), line_number.to_string());
    memory.claims.push(Claim {
        id: "claim_001".to_string(),
        text: format!("JSONL record `{title}` was imported from line {line_number}."),
        confidence: 0.75,
        source: Some(source_reference.to_string()),
    });
    memory.links.push(MemoryLink {
        source_id: memory.id.clone(),
        target_id: format!("jsonl-file:{}", stable_document_id(source_reference)),
        relation: "part_of_import_file".to_string(),
        confidence: 0.9,
    });
    memory
}

fn jsonl_record_body(object: &serde_json::Map<String, Value>) -> Option<String> {
    ["body", "content", "text", "summary"]
        .into_iter()
        .find_map(|key| {
            object
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

fn json_scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn jsonl_record_hash(
    source_reference: &str,
    line_number: usize,
    object: &serde_json::Map<String, Value>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_reference.as_bytes());
    hasher.update(line_number.to_le_bytes());
    hasher.update(serde_json::to_string(object).unwrap_or_default().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn options_for_document_path(
    path: &Path,
    options: &DocumentImportOptions,
) -> DocumentImportOptions {
    let mut file_options = options.clone();
    if file_options.source_kind == "document" {
        file_options.source_kind = infer_document_source_kind(path);
    }
    file_options
}

fn collect_document_paths(path: &Path, paths: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if path.is_file() {
        if is_document_path(path) {
            paths.push(path.to_path_buf());
        }
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if should_skip_document_dir(&path) {
                continue;
            }
            collect_document_paths(&path, paths)?;
        } else if is_document_path(&path) {
            paths.push(path);
        }
    }
    Ok(())
}

fn is_document_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some(
            "md" | "markdown"
                | "txt"
                | "text"
                | "rs"
                | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "py"
                | "go"
                | "java"
                | "c"
                | "cpp"
                | "h"
                | "hpp"
                | "cs"
                | "toml"
                | "json"
                | "yaml"
                | "yml"
        )
    )
}

fn should_skip_document_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | "target"
            | "node_modules"
            | "dist"
            | "build"
            | ".next"
            | ".cache"
            | "vendor"
    )
}

fn parse_document_source(source: &str) -> ParsedDocument {
    let Some(rest) = source.strip_prefix("---\n") else {
        return ParsedDocument {
            body: source.to_string(),
            frontmatter: BTreeMap::new(),
        };
    };
    let Some((frontmatter_source, body)) = rest.split_once("\n---\n") else {
        return ParsedDocument {
            body: source.to_string(),
            frontmatter: BTreeMap::new(),
        };
    };

    let frontmatter = frontmatter_source
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            (!key.is_empty() && !value.is_empty()).then(|| (key.to_string(), value.to_string()))
        })
        .collect::<BTreeMap<_, _>>();

    ParsedDocument {
        body: body.to_string(),
        frontmatter,
    }
}

fn document_chunks(source: &str, max_chars_per_memory: usize) -> Vec<String> {
    let max_chars = max_chars_per_memory.max(256);
    let sections = markdown_sections(source);
    if !sections.is_empty() {
        return sections
            .into_iter()
            .flat_map(|section| {
                if section.len() > max_chars {
                    split_large_unit(&section, max_chars)
                } else {
                    vec![section]
                }
            })
            .collect();
    }
    let units = if sections.is_empty() {
        source
            .split("\n\n")
            .map(str::trim)
            .filter(|paragraph| !paragraph.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    } else {
        sections
    };

    let mut chunks = Vec::new();
    let mut current = String::new();
    for unit in units {
        if !current.is_empty() && current.len() + unit.len() + 2 > max_chars {
            chunks.push(current.trim().to_string());
            current.clear();
        }
        if unit.len() > max_chars {
            if !current.trim().is_empty() {
                chunks.push(current.trim().to_string());
                current.clear();
            }
            chunks.extend(split_large_unit(&unit, max_chars));
            continue;
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(&unit);
    }
    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }
    chunks
}

fn markdown_sections(source: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current = String::new();
    for line in source.lines() {
        let is_heading = line.trim_start().starts_with('#');
        if is_heading && !current.trim().is_empty() {
            sections.push(current.trim().to_string());
            current.clear();
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        sections.push(current.trim().to_string());
    }
    if sections.len() <= 1 {
        Vec::new()
    } else {
        sections
    }
}

fn split_large_unit(unit: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for line in unit.lines() {
        if !current.is_empty() && current.len() + line.len() + 1 > max_chars {
            chunks.push(current.trim().to_string());
            current.clear();
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }
    chunks
}

fn document_title(path: &Path, source: &str) -> String {
    source
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("# ")
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "Imported document".to_string())
}

fn document_chunk_hash(source_reference: &str, chunk_index: usize, chunk: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_reference.as_bytes());
    hasher.update(chunk_index.to_le_bytes());
    hasher.update(chunk.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn stable_document_id(source_reference: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_reference.as_bytes());
    format!("{:x}", hasher.finalize())[..24].to_string()
}

fn chatgpt_conversations_path(path: &Path) -> anyhow::Result<PathBuf> {
    if path.is_dir() {
        let candidate = path.join("conversations.json");
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    if path.is_file()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "conversations.json")
    {
        return Ok(path.to_path_buf());
    }
    anyhow::bail!(
        "expected a ChatGPT export directory or conversations.json file: {}",
        path.display()
    )
}

fn read_chatgpt_conversations(path: &Path) -> anyhow::Result<Vec<ChatGptConversation>> {
    let source = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&source)?)
}

fn conversation_messages(conversation: &ChatGptConversation) -> Vec<ImportedMessage> {
    let mut messages = conversation
        .mapping
        .as_ref()
        .into_iter()
        .flat_map(|mapping| mapping.iter())
        .filter_map(|(node_id, node)| {
            let message = node.message.as_ref()?;
            let role = message
                .author
                .as_ref()
                .and_then(|author| author.role.as_deref())
                .unwrap_or("unknown");
            if role == "system" || role == "tool" {
                return None;
            }
            let text = message_text(message.content.as_ref()?)?;
            if text.trim().is_empty() {
                return None;
            }
            Some(ImportedMessage {
                id: message.id.clone().unwrap_or_else(|| node_id.clone()),
                role: role.to_string(),
                created_at: message.create_time.and_then(timestamp_to_utc),
                text,
            })
        })
        .collect::<Vec<_>>();
    messages.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    messages
}

fn message_text(content: &ChatGptContent) -> Option<String> {
    if let Some(text) = &content.text {
        return Some(text.trim().to_string());
    }

    let parts = content.parts.as_ref()?;
    let text = parts
        .iter()
        .filter_map(|part| match part {
            Value::String(text) => Some(text.clone()),
            Value::Object(object) => object
                .get("text")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    if text.trim().is_empty() {
        None
    } else if content.content_type.as_deref() == Some("code") {
        Some(format!("```\n{}\n```", text.trim()))
    } else {
        Some(text.trim().to_string())
    }
}

fn transcript_body(messages: &[ImportedMessage]) -> String {
    messages
        .iter()
        .map(|message| {
            let created_at = message
                .created_at
                .map(|created_at| created_at.to_rfc3339())
                .unwrap_or_else(|| "unknown-time".to_string());
            format!("## {} {}\n\n{}", message.role, created_at, message.text)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn roles_summary(messages: &[ImportedMessage]) -> String {
    messages
        .iter()
        .map(|message| message.role.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ")
}

fn conversation_id(conversation: &ChatGptConversation) -> String {
    conversation.id.clone().unwrap_or_else(|| {
        let mut hasher = Sha256::new();
        hasher.update(
            conversation
                .title
                .as_deref()
                .unwrap_or("untitled")
                .as_bytes(),
        );
        format!("generated-{}", &format!("{:x}", hasher.finalize())[..16])
    })
}

fn conversation_title(title: &str, chunk_index: usize, chunk_count: usize) -> String {
    if chunk_count == 1 {
        format!("ChatGPT: {title}")
    } else {
        format!("ChatGPT: {title} ({}/{chunk_count})", chunk_index + 1)
    }
}

fn chunk_hash(conversation_id: &str, chunk_index: usize, messages: &[ImportedMessage]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(conversation_id.as_bytes());
    hasher.update(chunk_index.to_le_bytes());
    for message in messages {
        hasher.update(message.id.as_bytes());
        hasher.update(message.role.as_bytes());
        hasher.update(message.text.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn chatgpt_message_chunks(
    messages: &[ImportedMessage],
    options: &ChatGptImportOptions,
) -> Vec<Vec<ImportedMessage>> {
    if messages.is_empty() {
        return Vec::new();
    }

    let max_messages = options.messages_per_memory.max(1);
    let max_chars = options.max_chars_per_memory;
    let mut chunks = Vec::new();
    let mut current = Vec::new();
    let mut current_chars = 0usize;

    for message in messages {
        let message_chars = chatgpt_message_budget_chars(message);
        let exceeds_message_count = current.len() >= max_messages;
        let exceeds_char_budget =
            max_chars > 0 && !current.is_empty() && current_chars + message_chars > max_chars;
        if exceeds_message_count || exceeds_char_budget {
            chunks.push(current);
            current = Vec::new();
            current_chars = 0;
        }
        current.push(message.clone());
        current_chars += message_chars;
    }

    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn chatgpt_message_budget_chars(message: &ImportedMessage) -> usize {
    message.role.len() + message.text.len() + "##  unknown-time\n\n".len()
}

fn import_report_from_memories(
    preview: ImportPreview,
    importer_version: &str,
    memories: &[MemoryObject],
) -> ImportReport {
    let memory_ids = memories
        .iter()
        .map(|memory| memory.id.clone())
        .collect::<Vec<_>>();
    let unique_memory_ids = memory_ids.iter().collect::<BTreeSet<_>>();
    let source_hashes = memories
        .iter()
        .filter_map(|memory| memory.metadata.get("source_hash").cloned())
        .collect::<Vec<_>>();

    ImportReport {
        source_kind: preview.source_kind,
        source_reference: preview.source_reference,
        importer_version: importer_version.to_string(),
        conversations: preview.conversations,
        messages: preview.messages,
        generated_memories: memories.len(),
        unique_memories: unique_memory_ids.len(),
        duplicate_memories: memories.len().saturating_sub(unique_memory_ids.len()),
        memory_ids,
        import_batch_hash: import_batch_hash(&source_hashes),
        source_hashes,
        quarantined_memory_ids: Vec::new(),
        warnings: Vec::new(),
    }
}

fn import_batch_hash(source_hashes: &[String]) -> String {
    let mut hasher = Sha256::new();
    for source_hash in source_hashes {
        hasher.update(source_hash.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn timestamp_to_utc(timestamp: f64) -> Option<DateTime<Utc>> {
    if !timestamp.is_finite() {
        return None;
    }
    let seconds = timestamp.trunc() as i64;
    let nanos = ((timestamp.fract().abs()) * 1_000_000_000.0).round() as u32;
    Utc.timestamp_opt(seconds, nanos).single()
}
