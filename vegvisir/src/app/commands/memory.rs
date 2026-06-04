use std::{
    fs,
    path::{Path, PathBuf},
    thread,
};

use super::super::*;

impl TuiApplication {
    pub(crate) fn recall_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok("Usage: /recall [--limit N] [--global] <query>".to_string());
        }
        let mut limit = 8_usize;
        let mut global = false;
        let mut query = Vec::new();
        let mut iter = args.iter().peekable();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--global" | "--all" => global = true,
                "--project" | "--local" => global = false,
                "--limit" | "-n" => {
                    let Some(value) = iter.next() else {
                        return Ok("Usage: /recall [--limit N] [--global] <query>".to_string());
                    };
                    limit = value.parse::<usize>().unwrap_or(8).clamp(1, 50);
                }
                value if value.starts_with("--limit=") => {
                    limit = value
                        .trim_start_matches("--limit=")
                        .parse::<usize>()
                        .unwrap_or(8)
                        .clamp(1, 50);
                }
                value => query.push(value.to_string()),
            }
        }
        if query.is_empty() {
            return Ok("Usage: /recall [--limit N] [--global] <query>".to_string());
        }
        let query = query.join(" ");
        let bundle = if global {
            self.cms.retrieve_global(query, limit)?
        } else {
            self.cms.retrieve(query, limit)?
        };
        if bundle.results.is_empty() {
            return Ok("No CMS memories matched.".to_string());
        }
        Ok(bundle
            .results
            .into_iter()
            .map(|result| {
                format!(
                    "{} [{}]: {}",
                    result.memory.title, result.memory.id.0, result.memory.summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub(crate) fn memory_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("status") | Some("scope") => Ok(format!(
                "CMS-v2 memory scope\nmode={:?}\ndb={}\nuser_id={}\nproject_id={}\nactive_agent={}\nworkspace={}",
                self.cms.config.context_mode,
                self.cms.config.db_path.display(),
                self.cms.config.user_id,
                self.cms.config.project_id.as_deref().unwrap_or("none"),
                self.session.active_agent_id.as_deref().unwrap_or("default"),
                self.cwd.display()
            )),
            Some("recent") | Some("list") => {
                let (limit, global) = parse_limit_and_global(&args[1..], 8);
                let memories = self.cms.recent(limit, global)?;
                if memories.is_empty() {
                    return Ok(if global {
                        "No recent CMS memories are available for this user.".to_string()
                    } else {
                        "No recent CMS memories are available for this project scope.".to_string()
                    });
                }
                Ok(memories
                    .into_iter()
                    .map(|memory| {
                        format!(
                            "{}  {}  type={} project={} title={} summary={}",
                            memory.id,
                            memory.updated_at.format("%Y-%m-%d %H:%M:%S"),
                            memory.memory_type,
                            memory.project_id.as_deref().unwrap_or("none"),
                            memory.title,
                            memory.summary
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            Some("search-chatgpt") | Some("chatgpt-search") | Some("archive-search") => {
                let (limit, query) = parse_archive_search_args(&args[1..])?;
                let bundle = self.cms.retrieve_chatgpt_archive(query, limit)?;
                if bundle.results.is_empty() {
                    return Ok("No ChatGPT archive memories matched.".to_string());
                }
                Ok(bundle
                    .results
                    .into_iter()
                    .map(|result| {
                        let conversation = result
                            .memory
                            .metadata
                            .get("conversation_title")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(&result.memory.title);
                        let chunk = result
                            .memory
                            .metadata
                            .get("chunk_index")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("?");
                        let chunk_count = result
                            .memory
                            .metadata
                            .get("chunk_count")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("?");
                        format!(
                            "{} [{} chunk {}/{} score {:.2}]: {}",
                            conversation,
                            result.memory.id.0,
                            chunk,
                            chunk_count,
                            result.score,
                            result.memory.summary
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            Some("import-chatgpt") => {
                let (path, messages_per_memory, max_chars_per_memory) =
                    parse_chatgpt_import_args(&args[1..])?;
                if !path.exists() {
                    anyhow::bail!("ChatGPT export path does not exist: {}", path.display());
                }
                let config = self.cms.config.clone();
                let db_path = self.cms.chatgpt_archive_config().db_path.clone();
                let user_id = config.user_id.clone();
                let import_path = path.clone();
                let handle = thread::spawn(move || {
                    let mut cms = VegvisirCms::open(config)?;
                    let summary = cms.import_chatgpt(
                        &import_path,
                        messages_per_memory,
                        max_chars_per_memory,
                    )?;
                    Ok(format!(
                        "Imported {} ChatGPT archive memory object(s).\ndb={}\nuser_id={}\ncorpus={}\nretrieval_policy=explicit_only",
                        summary.imported,
                        summary.db_path.display(),
                        summary.user_id,
                        summary.corpus
                    ))
                });
                self.pending_background_jobs.push(handle);
                Ok(format!(
                    "Started ChatGPT archive import in background.\npath={}\ndb={}\nuser_id={}\ncorpus=chatgpt_archive\nretrieval_policy=explicit_only\nUse /memory search-chatgpt <query> after the completion note appears.",
                    path.display(),
                    db_path.display(),
                    user_id
                ))
            }
            Some("used-this-turn") | Some("used") => self.memory_used_this_turn(),
            Some("writes-this-session") | Some("written-this-session") | Some("writes") => {
                self.memory_writes_this_session()
            }
            Some("why") => {
                let Some(memory_id) = args.get(1) else {
                    return Ok("Usage: /memory why <memory-id>".to_string());
                };
                self.memory_why(memory_id)
            }
            Some("diff") => {
                let (Some(left), Some(right)) = (args.get(1), args.get(2)) else {
                    return Ok("Usage: /memory diff <memory-id-a> <memory-id-b>".to_string());
                };
                self.memory_diff(left, right)
            }
            Some("quarantine") => {
                let Some(memory_id) = args.get(1) else {
                    return Ok("Usage: /memory quarantine <memory-id>".to_string());
                };
                if self.cms.quarantine_memory(memory_id)? {
                    Ok(format!(
                        "Quarantined memory {memory_id}. It will be excluded from active retrieval."
                    ))
                } else {
                    Ok(format!(
                        "No active memory found for quarantine: {memory_id}"
                    ))
                }
            }
            Some("forget") | Some("delete") => {
                let Some(memory_id) = args.get(1) else {
                    return Ok("Usage: /memory forget <memory-id>".to_string());
                };
                if self.cms.forget_memory(memory_id)? {
                    Ok(format!(
                        "Forgot memory {memory_id} via CMS soft-delete. Audit history is retained."
                    ))
                } else {
                    Ok(format!("No active memory found to forget: {memory_id}"))
                }
            }
            Some("export") => {
                let global = args
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "--global" | "--user" | "--all"));
                let output = memory_export_path(&self.cwd, args);
                let count = self.cms.export_json(&output, global)?;
                Ok(format!(
                    "Exported {count} CMS memory object(s) to {}\nscope={}\nredaction=enabled",
                    output.display(),
                    if global { "user/global" } else { "project" }
                ))
            }
            Some(other) => Ok(format!("Unknown /memory command: {other}")),
        }
    }

    pub(crate) fn remember_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let global = args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--global" | "--user" | "--profile"));
        let raw = args
            .iter()
            .filter(|arg| !matches!(arg.as_str(), "--global" | "--user" | "--profile"))
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        let Some((title, content)) = raw.split_once('|') else {
            return Ok("Usage: /remember [--global] <title> | <content>".to_string());
        };
        let result = if global {
            self.cms
                .remember_global("note", title.trim(), content.trim())?
        } else {
            self.cms.remember("note", title.trim(), content.trim())?
        };
        Ok(format!(
            "Remembered {}memory {}",
            if global { "global " } else { "" },
            result.memory_id.0
        ))
    }

    pub(crate) fn context_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None => Ok("Usage: /context [explain|budget|sources] <message> | /context last | /context diff-last".to_string()),
            Some("last") => self.context_last(),
            Some("diff-last") => self.context_diff_last(),
            Some("explain") => self.context_explain(&args[1..]),
            Some("budget") => self.context_budget(&args[1..]),
            Some("sources") => self.context_sources(&args[1..]),
            Some(_) => Ok(self.cms.prepare_context(args.join(" "))?.packed_text),
        }
    }

    fn context_last(&self) -> anyhow::Result<String> {
        let Some(run_dir) = self.latest_run_dir()? else {
            return Ok("No run artifacts found; no last context is available.".to_string());
        };
        Ok(
            fs::read_to_string(run_dir.join("context.md")).unwrap_or_else(|_| {
                format!(
                    "No context.md artifact found for latest run at {}",
                    run_dir.display()
                )
            }),
        )
    }

    fn context_diff_last(&self) -> anyhow::Result<String> {
        let mut dirs = run_dirs_sorted(&self.runs_root())?;
        if dirs.len() < 2 {
            return Ok("Need at least two run artifacts to diff context.".to_string());
        }
        dirs.reverse();
        let latest = fs::read_to_string(dirs[0].join("context.md")).unwrap_or_default();
        let previous = fs::read_to_string(dirs[1].join("context.md")).unwrap_or_default();
        Ok(simple_line_diff(
            "previous context",
            &previous,
            "latest context",
            &latest,
        ))
    }

    fn context_explain(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok("Usage: /context explain <message>".to_string());
        }
        let prepared = self.cms.prepare_context(args.join(" "))?;
        Ok(format!(
            "Context explanation\nmode={:?}\nsession_id={}\ntoken_estimate={}\nsections={}\n\n{}",
            self.cms.config.context_mode,
            prepared.session_id.0,
            prepared.token_estimate,
            prepared.frames.len(),
            prepared.packed_text
        ))
    }

    fn context_budget(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok("Usage: /context budget <message>".to_string());
        }
        let prepared = self.cms.prepare_context(args.join(" "))?;
        Ok(format!(
            "Context budget\ntoken_estimate={}\ncontext_limit={}\npercent={:.2}%\nsections={}",
            prepared.token_estimate,
            self.session.context_limit,
            if self.session.context_limit == 0 {
                0.0
            } else {
                (prepared.token_estimate as f64 / self.session.context_limit as f64) * 100.0
            },
            prepared.frames.len()
        ))
    }

    fn context_sources(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            let Some(run_dir) = self.latest_run_dir()? else {
                return Ok("No run artifacts found; no context sources are available.".to_string());
            };
            return Ok(
                fs::read_to_string(run_dir.join("context-sources.json")).unwrap_or_else(|_| {
                    "No context-sources.json artifact found for latest run.".to_string()
                }),
            );
        }
        let content = args.join(" ");
        let envelope = self.cms.prepare_cached_prompt(
            content,
            self.session.current_provider.clone(),
            self.session.current_model.clone(),
        )?;
        Ok(serde_json::to_string_pretty(
            &crate::run_artifacts::RunMemoryUseEvidence::from_envelope(
                "preview".to_string(),
                &envelope,
            ),
        )?)
    }

    fn memory_used_this_turn(&self) -> anyhow::Result<String> {
        let Some(run_dir) = self.latest_run_dir()? else {
            return Ok("No run artifacts found; no memory-use evidence is available.".to_string());
        };
        Ok(fs::read_to_string(run_dir.join("memory-used.json"))
            .unwrap_or_else(|_| "No memory-used.json artifact found for latest run.".to_string()))
    }

    fn memory_writes_this_session(&self) -> anyhow::Result<String> {
        let mut lines = Vec::new();
        for run_dir in run_dirs_sorted(&self.runs_root())? {
            let path = run_dir.join("memory-written.json");
            if path.exists() {
                lines.push(format!(
                    "# {}\n{}",
                    run_dir
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("run"),
                    fs::read_to_string(path).unwrap_or_default()
                ));
            }
        }
        if lines.is_empty() {
            Ok("No memory-written.json artifacts found for this workspace.".to_string())
        } else {
            Ok(lines.join("\n\n"))
        }
    }

    fn memory_why(&self, memory_id: &str) -> anyhow::Result<String> {
        let summary = self.cms.get_memory_summary(memory_id)?;
        let mut lines = Vec::new();
        if let Some(summary) = summary {
            lines.push(format!(
                "Memory {}\ntitle={}\ntype={}\nproject={}\nsummary={}",
                summary.id,
                summary.title,
                summary.memory_type,
                summary.project_id.as_deref().unwrap_or("none"),
                summary.summary
            ));
        } else {
            lines.push(format!(
                "Memory {memory_id} was not found in the active CMS ledger."
            ));
        }
        let mut uses = Vec::new();
        for run_dir in run_dirs_sorted(&self.runs_root())? {
            let path = run_dir.join("memory-used.json");
            let text = fs::read_to_string(&path).unwrap_or_default();
            if text.contains(memory_id) {
                uses.push(
                    run_dir
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("run")
                        .to_string(),
                );
            }
        }
        if uses.is_empty() {
            lines.push("No usage found in current workspace run artifacts.".to_string());
        } else {
            lines.push(format!("Used by run artifact(s): {}", uses.join(", ")));
        }
        Ok(lines.join("\n"))
    }

    fn memory_diff(&self, left: &str, right: &str) -> anyhow::Result<String> {
        let left_text = self
            .cms
            .get_memory_summary(left)?
            .map(|m| format!("{}\n{}\n{}", m.title, m.memory_type, m.summary))
            .unwrap_or_else(|| "<missing>".to_string());
        let right_text = self
            .cms
            .get_memory_summary(right)?
            .map(|m| format!("{}\n{}\n{}", m.title, m.memory_type, m.summary))
            .unwrap_or_else(|| "<missing>".to_string());
        Ok(simple_line_diff(left, &left_text, right, &right_text))
    }
}

fn parse_archive_search_args(args: &[String]) -> anyhow::Result<(usize, String)> {
    let mut limit = 8usize;
    let mut query = Vec::new();
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--limit" | "-n" => {
                let Some(value) = iter.next() else {
                    anyhow::bail!("Usage: /memory search-chatgpt [--limit N] <query>");
                };
                limit = value.parse::<usize>().unwrap_or(8).clamp(1, 50);
            }
            value if value.starts_with("--limit=") => {
                limit = value
                    .trim_start_matches("--limit=")
                    .parse::<usize>()
                    .unwrap_or(8)
                    .clamp(1, 50);
            }
            value if value.starts_with("--") => {
                anyhow::bail!("Unknown search-chatgpt option: {value}");
            }
            value => query.push(value.to_string()),
        }
    }
    if query.is_empty() {
        anyhow::bail!("Usage: /memory search-chatgpt [--limit N] <query>");
    }
    Ok((limit, query.join(" ")))
}

fn parse_chatgpt_import_args(args: &[String]) -> anyhow::Result<(PathBuf, usize, usize)> {
    let mut path = None;
    let mut messages_per_memory = 40usize;
    let mut max_chars_per_memory = 0usize;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--messages-per-memory" => {
                let Some(value) = args.get(index + 1) else {
                    anyhow::bail!("Missing value for --messages-per-memory");
                };
                messages_per_memory = value
                    .parse::<usize>()
                    .map_err(|_| anyhow::anyhow!("Invalid --messages-per-memory value: {value}"))?
                    .max(1);
                index += 2;
            }
            "--max-chars-per-memory" => {
                let Some(value) = args.get(index + 1) else {
                    anyhow::bail!("Missing value for --max-chars-per-memory");
                };
                max_chars_per_memory = value.parse::<usize>().map_err(|_| {
                    anyhow::anyhow!("Invalid --max-chars-per-memory value: {value}")
                })?;
                index += 2;
            }
            value if value.starts_with("--") => {
                anyhow::bail!("Unknown import-chatgpt option: {value}");
            }
            value => {
                if path.is_some() {
                    anyhow::bail!(
                        "Usage: /memory import-chatgpt <export-dir-or-conversations.json> [--messages-per-memory N] [--max-chars-per-memory N]"
                    );
                }
                path = Some(expand_workspace_path(value));
                index += 1;
            }
        }
    }
    let Some(path) = path else {
        anyhow::bail!(
            "Usage: /memory import-chatgpt <export-dir-or-conversations.json> [--messages-per-memory N] [--max-chars-per-memory N]"
        );
    };
    Ok((path, messages_per_memory, max_chars_per_memory))
}

fn memory_export_path(workspace: &Path, args: &[String]) -> PathBuf {
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--out" | "--file" => {
                if let Some(path) = iter.next() {
                    return expand_workspace_path_for(workspace, path);
                }
            }
            value if value.starts_with("--out=") => {
                return expand_workspace_path_for(workspace, value.trim_start_matches("--out="));
            }
            value if value.ends_with(".json") => {
                return expand_workspace_path_for(workspace, value);
            }
            _ => {}
        }
    }
    workspace.join(".vegvisir").join("memory-export.json")
}

fn expand_workspace_path_for(workspace: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        workspace.join(path)
    }
}

fn run_dirs_sorted(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            entries.push(entry.path());
        }
    }
    entries.sort_by_key(|path| {
        path.join("manifest.json")
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    Ok(entries)
}

fn simple_line_diff(left_label: &str, left: &str, right_label: &str, right: &str) -> String {
    let left_lines = left.lines().collect::<Vec<_>>();
    let right_lines = right.lines().collect::<Vec<_>>();
    let mut out = format!("--- {left_label}\n+++ {right_label}\n");
    let max = left_lines.len().max(right_lines.len());
    for index in 0..max {
        match (left_lines.get(index), right_lines.get(index)) {
            (Some(left), Some(right)) if left == right => {
                out.push(' ');
                out.push_str(left);
                out.push('\n');
            }
            (Some(left), Some(right)) => {
                out.push('-');
                out.push_str(left);
                out.push('\n');
                out.push('+');
                out.push_str(right);
                out.push('\n');
            }
            (Some(left), None) => {
                out.push('-');
                out.push_str(left);
                out.push('\n');
            }
            (None, Some(right)) => {
                out.push('+');
                out.push_str(right);
                out.push('\n');
            }
            (None, None) => {}
        }
    }
    out
}
