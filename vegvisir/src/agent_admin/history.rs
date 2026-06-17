use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::core::{AgentProfile, normalize_agent_id};

use super::{HistoryEvent, metadata_json};

pub fn history_path(data_root: &Path) -> PathBuf {
    data_root
        .join("agents")
        .join("history")
        .join("events.jsonl")
}

pub fn load_history(data_root: &Path) -> anyhow::Result<Vec<HistoryEvent>> {
    let path = history_path(data_root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut events = Vec::new();
    for (index, line) in fs::read_to_string(&path)?.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<HistoryEvent>(line) {
            Ok(event) => events.push(event),
            Err(error) => events.push(HistoryEvent {
                agent_id: "-".to_string(),
                action: "invalid-history-record".to_string(),
                summary: format!("{}:{}: {error}", path.display(), index + 1),
                metadata: BTreeMap::new(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            }),
        }
    }
    Ok(events)
}

pub fn append_history(
    data_root: &Path,
    profile: &AgentProfile,
    action: &str,
    path: &Path,
) -> anyhow::Result<()> {
    let history_path = history_path(data_root);
    if let Some(parent) = history_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "path".to_string(),
        Value::String(path.display().to_string()),
    );
    metadata.insert("mode".to_string(), Value::String(profile.mode.clone()));
    metadata.insert("status".to_string(), metadata_json(profile, "status"));
    let event = HistoryEvent {
        agent_id: profile.id.clone(),
        action: action.to_string(),
        summary: format!(
            "{} ({}, mode={})",
            profile.display_name, profile.id, profile.mode
        ),
        metadata,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&history_path)?;
    writeln!(file, "{}", serde_json::to_string(&event)?)?;
    Ok(())
}

pub fn history_agent_id(id: &str) -> String {
    normalize_agent_id(id)
}
