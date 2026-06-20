use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::bail;
use serde_json::Value;

use crate::core::AgentProfile;

use super::models::{FieldDifference, SkillerAgentArtifact, ValidationIssue};

pub fn clean_list(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut cleaned = Vec::new();
    for value in values {
        for item in value.split(',') {
            let item = item.trim();
            if !item.is_empty() && seen.insert(item.to_string()) {
                cleaned.push(item.to_string());
            }
        }
    }
    cleaned
}

pub fn append_unique(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target.contains(&value) {
            target.push(value);
        }
    }
}

pub fn remove_all(target: &mut Vec<String>, values: &[String]) {
    target.retain(|item| !values.contains(item));
}

pub fn list_or_dash(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(",")
    }
}

pub fn dash_if_empty(value: &str) -> &str {
    if value.trim().is_empty() { "-" } else { value }
}

pub fn none_marker(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "-" || trimmed.eq_ignore_ascii_case("clear") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn join_required(label: &str, values: Vec<String>) -> anyhow::Result<String> {
    let joined = values.join(" ").trim().to_string();
    if joined.is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(joined)
}

pub fn normalized_or_default(value: &str, default: &str) -> String {
    let normalized = crate::core::normalize_agent_id(value);
    if normalized.is_empty() {
        default.to_string()
    } else {
        normalized
    }
}

pub fn admin_metadata(action: &str) -> BTreeMap<String, Value> {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "managed_by".to_string(),
        Value::String("vegvisir-agent-admin".to_string()),
    );
    metadata.insert(
        "last_admin_action".to_string(),
        Value::String(action.to_string()),
    );
    metadata
}

pub fn print_json_or_text<F>(json_output: bool, value: &Value, text: F) -> anyhow::Result<()>
where
    F: FnOnce() -> anyhow::Result<()>,
{
    if json_output {
        println!("{}", serde_json::to_string_pretty(value)?);
        Ok(())
    } else {
        text()
    }
}

pub fn ratio(part: u64, total: u64) -> Option<f64> {
    if total == 0 {
        None
    } else {
        Some(part as f64 / total as f64)
    }
}

pub fn percent_or_dash(value: Option<f64>) -> String {
    value
        .map(|value| format!("{:.1}%", value * 100.0))
        .unwrap_or_else(|| "-".to_string())
}

pub fn push_diff(differences: &mut Vec<FieldDifference>, field: &str, left: Value, right: Value) {
    if left != right {
        differences.push(FieldDifference {
            field: field.to_string(),
            left,
            right,
        });
    }
}

pub fn metadata_json(profile: &AgentProfile, key: &str) -> Value {
    profile.metadata.get(key).cloned().unwrap_or(Value::Null)
}

pub fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unprintable>".to_string())
}

pub fn issue(severity: &str, field: &str, message: impl Into<String>) -> ValidationIssue {
    issue_with_details(severity, field, message, std::iter::empty::<String>())
}

pub fn issue_with_details<I, S>(
    severity: &str,
    field: &str,
    message: impl Into<String>,
    details: I,
) -> ValidationIssue
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    ValidationIssue {
        severity: severity.to_string(),
        field: field.to_string(),
        message: message.into(),
        details: details.into_iter().map(Into::into).collect(),
    }
}

pub fn secret_like(value: &str) -> bool {
    secret_like_pattern(value).is_some()
}

pub fn secret_like_pattern(value: &str) -> Option<&'static str> {
    let lower = value.to_ascii_lowercase();
    secret_like_patterns()
        .iter()
        .copied()
        .find(|pattern| lower.contains(pattern))
}

pub fn secret_like_patterns() -> [&'static str; 9] {
    [
        "api_key",
        "apikey",
        "secret_key",
        "access_token",
        "refresh_token",
        "private key",
        "-----begin",
        "password=",
        "authorization: bearer",
    ]
}

pub fn find_skiller_agent_artifacts(
    cwd: &Path,
    data_root: &Path,
) -> Vec<anyhow::Result<SkillerAgentArtifact>> {
    let roots = [
        cwd.join(".vegvisir").join("agent-packs"),
        cwd.join(".vegvisir").join("skiller"),
        cwd.join(".vegvisir").join("skiller-agent-packs"),
        data_root.join("agent-packs"),
        data_root.join("skiller"),
        data_root.join("skiller-agent-packs"),
    ];
    let mut artifacts = Vec::new();
    let mut seen = BTreeSet::new();
    for root in roots {
        collect_skiller_agent_artifacts(&root, 6, &mut seen, &mut artifacts);
    }
    artifacts
}

fn collect_skiller_agent_artifacts(
    path: &Path,
    remaining_depth: usize,
    seen: &mut BTreeSet<std::path::PathBuf>,
    artifacts: &mut Vec<anyhow::Result<SkillerAgentArtifact>>,
) {
    if remaining_depth == 0 || !path.exists() {
        return;
    }
    let Ok(metadata) = fs::metadata(path) else {
        artifacts.push(Err(anyhow::anyhow!("could not inspect {}", path.display())));
        return;
    };
    if metadata.is_file() {
        match path.file_name().and_then(|name| name.to_str()) {
            Some("agent-pack.yaml") if seen.insert(path.to_path_buf()) => {
                artifacts.push(Ok(SkillerAgentArtifact::Pack(path.to_path_buf())))
            }
            Some("agent-proposals-index.yaml") if seen.insert(path.to_path_buf()) => {
                artifacts.push(Ok(SkillerAgentArtifact::ProposalIndex(path.to_path_buf())))
            }
            _ => {}
        }
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        artifacts.push(Err(anyhow::anyhow!("could not list {}", path.display())));
        return;
    };
    for entry in entries.flatten() {
        collect_skiller_agent_artifacts(&entry.path(), remaining_depth - 1, seen, artifacts);
    }
}
