use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::*;
use crate::run_artifacts::{RunArtifactManager, RunManifest, RunStatus};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RunListEntry {
    run_id: String,
    status: RunStatus,
    provider: String,
    model: String,
    session_id: String,
    workspace: PathBuf,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    run_dir: PathBuf,
}

impl TuiApplication {
    pub(crate) fn runs_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("list") => self.runs_list_command(args),
            Some("show") => {
                let Some(selector) = args.get(1) else {
                    return Ok("Usage: /runs show <run-id|latest>".to_string());
                };
                self.runs_show_command(selector)
            }
            Some("open") => {
                let Some(selector) = args.get(1) else {
                    return Ok("Usage: /runs open <run-id|latest>".to_string());
                };
                let Some(entry) = self.resolve_run_entry(selector)? else {
                    return Ok(format!("No run matching `{selector}`."));
                };
                Ok(format!(
                    "Run artifact directory: {}",
                    entry.run_dir.display()
                ))
            }
            Some("diff") => {
                let Some(selector) = args.get(1) else {
                    return Ok("Usage: /runs diff <run-id|latest>".to_string());
                };
                let Some(entry) = self.resolve_run_entry(selector)? else {
                    return Ok(format!("No run matching `{selector}`."));
                };
                let diff =
                    fs::read_to_string(entry.run_dir.join("diff.patch")).unwrap_or_else(|_| {
                        "No diff.patch artifact was captured for this run.".to_string()
                    });
                Ok(diff)
            }
            Some("export") => {
                let Some(selector) = args.get(1) else {
                    return Ok("Usage: /runs export <run-id|latest> [--zip]".to_string());
                };
                let Some(entry) = self.resolve_run_entry(selector)? else {
                    return Ok(format!("No run matching `{selector}`."));
                };
                if args.iter().any(|arg| arg == "--zip") {
                    return Ok("Zip export is not enabled in this build yet. Use the artifact directory path below with your archive tool.\n".to_string()
                        + &entry.run_dir.display().to_string());
                }
                Ok(format!(
                    "Run {} export source:\n{}\n\nCopy or archive this directory; artifacts are already redacted by RunArtifactManager.",
                    entry.run_id,
                    entry.run_dir.display()
                ))
            }
            Some("replay-plan") => {
                let Some(selector) = args.get(1) else {
                    return Ok("Usage: /runs replay-plan <run-id|latest>".to_string());
                };
                let Some(entry) = self.resolve_run_entry(selector)? else {
                    return Ok(format!("No run matching `{selector}`."));
                };
                self.runs_replay_plan(&entry)
            }
            Some(other) => Ok(format!(
                "Unknown /runs command: {other}\nUsage: /runs [list|show|open|export|diff|replay-plan] <run-id|latest>"
            )),
        }
    }

    fn runs_list_command(&self, args: &[String]) -> anyhow::Result<String> {
        let limit = parse_limit(args, 20).clamp(1, 200);
        let mut entries = self.list_run_entries()?;
        if entries.is_empty() {
            return Ok(format!(
                "No run artifact bundles found under {}.",
                self.runs_root().display()
            ));
        }
        entries.sort_by_key(|entry| entry.started_at);
        entries.reverse();
        entries.truncate(limit);
        let mut lines = vec![format!(
            "Recent run artifacts under {} (showing {}):",
            self.runs_root().display(),
            entries.len()
        )];
        for entry in entries {
            lines.push(format!(
                "{}  status={:?} provider={} model={} started={} finished={} dir={}",
                entry.run_id,
                entry.status,
                entry.provider,
                entry.model,
                entry.started_at.to_rfc3339(),
                entry
                    .finished_at
                    .map(|ts| ts.to_rfc3339())
                    .unwrap_or_else(|| "none".to_string()),
                entry.run_dir.display()
            ));
        }
        Ok(lines.join("\n"))
    }

    fn runs_show_command(&self, selector: &str) -> anyhow::Result<String> {
        let Some(entry) = self.resolve_run_entry(selector)? else {
            return Ok(format!("No run matching `{selector}`."));
        };
        let manifest_path = entry.run_dir.join("manifest.json");
        let manifest_text = fs::read_to_string(&manifest_path)
            .unwrap_or_else(|_| "<manifest unavailable>".to_string());
        let result = read_optional_trimmed(entry.run_dir.join("result.md"), 4_000);
        let failure = read_optional_json_summary(entry.run_dir.join("failure.json"));
        let verification = read_optional_json_summary(entry.run_dir.join("verification.json"));
        let memory_used = read_optional_json_summary(entry.run_dir.join("memory-used.json"));
        let approvals = read_optional_json_summary(entry.run_dir.join("approvals.json"));
        let subagents = read_optional_json_summary(entry.run_dir.join("subagents.json"));
        Ok(format!(
            "# Run {run_id}\n\n- dir: {dir}\n- status: {status:?}\n- provider/model: {provider}/{model}\n- session: {session}\n- started: {started}\n- finished: {finished}\n\n## Manifest\n```json\n{manifest}\n```\n\n## Result\n{result}\n\n## Failure\n{failure}\n\n## Verification\n{verification}\n\n## Memory Used\n{memory_used}\n\n## Approvals\n{approvals}\n\n## Subagents\n{subagents}",
            run_id = entry.run_id,
            dir = entry.run_dir.display(),
            status = entry.status,
            provider = entry.provider,
            model = entry.model,
            session = entry.session_id,
            started = entry.started_at.to_rfc3339(),
            finished = entry
                .finished_at
                .map(|ts| ts.to_rfc3339())
                .unwrap_or_else(|| "none".to_string()),
            manifest = manifest_text.trim(),
            result = result.unwrap_or_else(|| "No result.md captured.".to_string()),
            failure = failure.unwrap_or_else(|| "No failure.json captured.".to_string()),
            verification =
                verification.unwrap_or_else(|| "No verification.json captured.".to_string()),
            memory_used =
                memory_used.unwrap_or_else(|| "No memory-used.json captured.".to_string()),
            approvals = approvals.unwrap_or_else(|| "No approvals.json captured.".to_string()),
            subagents = subagents.unwrap_or_else(|| "No subagents.json captured.".to_string()),
        ))
    }

    fn runs_replay_plan(&self, entry: &RunListEntry) -> anyhow::Result<String> {
        let request = fs::read_to_string(entry.run_dir.join("request.json")).ok();
        let context_exists = entry.run_dir.join("context.md").exists();
        let result_exists = entry.run_dir.join("result.md").exists();
        Ok(format!(
            "Replay plan for run {run_id}\n\n1. Inspect request: {request_path}\n2. Inspect context exposure: {context}\n3. Inspect tool/provider events: {provider_events}, {tool_events}\n4. Inspect workspace diff: {diff}\n5. Re-run manually only after reviewing approvals and secrets boundary.\n\nCaptured request summary:\n```json\n{request}\n```\n\nResult captured: {result_exists}",
            run_id = entry.run_id,
            request_path = entry.run_dir.join("request.json").display(),
            context = if context_exists {
                entry.run_dir.join("context.md").display().to_string()
            } else {
                "not captured".to_string()
            },
            provider_events = entry.run_dir.join("provider-events.jsonl").display(),
            tool_events = entry.run_dir.join("tool-events.jsonl").display(),
            diff = entry.run_dir.join("diff.patch").display(),
            request = request.unwrap_or_else(|| "null".to_string()).trim(),
            result_exists = result_exists,
        ))
    }

    pub(crate) fn recover_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("turn") => Ok(self
                .turn_repair(false)
                .unwrap_or_else(|| "No stuck/dead turn detected. Use `/recover last` for latest run replay guidance.".to_string())),
            Some("force") => Ok(self.turn_repair(true).unwrap_or_else(|| "No in-flight turn existed; runtime state is ready.".to_string())),
            Some("last") | Some("replay") => {
                let Some(entry) = self.resolve_run_entry("latest")? else {
                    return Ok("No run artifacts found to recover from.".to_string());
                };
                self.runs_replay_plan(&entry)
            }
            Some(other) => Ok(format!("Unknown /recover command: {other}. Usage: /recover [turn|force|last]")),
        }
    }

    pub(crate) fn runs_root(&self) -> PathBuf {
        self.cwd.join(".vegvisir").join("runs")
    }

    pub(crate) fn latest_run_dir(&self) -> anyhow::Result<Option<PathBuf>> {
        Ok(self.resolve_run_entry("latest")?.map(|entry| entry.run_dir))
    }

    fn list_run_entries(&self) -> anyhow::Result<Vec<RunListEntry>> {
        let root = self.runs_root();
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let run_dir = entry.path();
            let manifest_path = run_dir.join("manifest.json");
            let Ok(text) = fs::read_to_string(&manifest_path) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_str::<RunManifest>(&text) else {
                continue;
            };
            entries.push(RunListEntry {
                run_id: manifest.run_id,
                status: manifest.status,
                provider: manifest.provider,
                model: manifest.model,
                session_id: manifest.session_id,
                workspace: manifest.workspace,
                started_at: manifest.started_at,
                finished_at: manifest.finished_at,
                run_dir,
            });
        }
        Ok(entries)
    }

    fn resolve_run_entry(&self, selector: &str) -> anyhow::Result<Option<RunListEntry>> {
        let mut entries = self.list_run_entries()?;
        if entries.is_empty() {
            return Ok(None);
        }
        entries.sort_by_key(|entry| entry.started_at);
        entries.reverse();
        if matches!(selector, "latest" | "last") {
            return Ok(entries.into_iter().next());
        }
        Ok(entries.into_iter().find(|entry| {
            entry.run_id == selector
                || entry.run_id.starts_with(selector)
                || entry.run_dir.ends_with(selector)
        }))
    }
}

fn read_optional_trimmed(path: impl AsRef<Path>, max_chars: usize) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut output = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        output.push_str("\n[truncated]");
    }
    Some(output)
}

fn read_optional_json_summary(path: impl AsRef<Path>) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    Some(serde_json::to_string_pretty(&value).unwrap_or(text))
}

#[allow(dead_code)]
fn _manager_from_entry(app: &TuiApplication, entry: &RunListEntry) -> RunArtifactManager {
    RunArtifactManager::from_existing(
        app.cwd.clone(),
        app.data_root.clone(),
        entry.run_id.clone(),
        entry.run_dir.clone(),
    )
}
