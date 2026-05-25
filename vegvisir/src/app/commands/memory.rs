use std::thread;

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
            Some("import-chatgpt") => {
                let (path, messages_per_memory, max_chars_per_memory) =
                    parse_chatgpt_import_args(&args[1..])?;
                if !path.exists() {
                    anyhow::bail!("ChatGPT export path does not exist: {}", path.display());
                }
                let config = self.cms.config.clone();
                let db_path = config.db_path.clone();
                let user_id = config.user_id.clone();
                let project_id = config.project_id.clone();
                let import_path = path.clone();
                let handle = thread::spawn(move || {
                    let mut cms = VegvisirCms::open(config)?;
                    let summary = cms.import_chatgpt(
                        &import_path,
                        messages_per_memory,
                        max_chars_per_memory,
                    )?;
                    Ok(format!(
                        "Imported {} ChatGPT memory object(s) into active CMS scope.\ndb={}\nuser_id={}\nproject_id={}",
                        summary.imported,
                        summary.db_path.display(),
                        summary.user_id,
                        summary.project_id.as_deref().unwrap_or("none")
                    ))
                });
                self.pending_background_jobs.push(handle);
                Ok(format!(
                    "Started ChatGPT import in background.\npath={}\ndb={}\nuser_id={}\nproject_id={}\nUse /memory recent after the completion note appears.",
                    path.display(),
                    db_path.display(),
                    user_id,
                    project_id.as_deref().unwrap_or("none")
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
        if args.is_empty() {
            return Ok("Usage: /context <message>".to_string());
        }
        Ok(self.cms.prepare_context(args.join(" "))?.packed_text)
    }
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
