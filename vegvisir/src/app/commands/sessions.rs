use super::super::*;

impl TuiApplication {
    pub(crate) fn new_session(&mut self, args: &[String]) -> String {
        self.autosave_session();
        let title = join_or(args, "untitled");
        let mut next = self.sessions.create(
            title,
            self.session.current_provider.clone(),
            self.session.current_model.clone(),
            self.session.enabled_tools.clone(),
            self.session.enabled_skills.clone(),
        );
        next.system_prompt = self.session.system_prompt.clone();
        next.active_agent_id = self.session.active_agent_id.clone();
        next.active_agent_name = self.session.active_agent_name.clone();
        next.cwd = self.cwd.display().to_string();
        next.context_limit = self.session.context_limit;
        self.session = next;
        let _ = self.apply_session_workspace_state();
        format!("Started new session {}", self.session.session_id)
    }

    pub(crate) fn sessions_command(&self) -> anyhow::Result<String> {
        let sessions = self.sessions.list()?;
        if sessions.is_empty() {
            return Ok("No saved sessions.".to_string());
        }
        Ok(sessions
            .iter()
            .map(|session| {
                format!(
                    "{}  {}  messages={}  title={}  cwd={}",
                    session.session_id,
                    session.created_at.format("%Y-%m-%d %H:%M:%S"),
                    session.messages.len(),
                    session.title,
                    session.cwd
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub(crate) fn load_session_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let Some(session_id) = args.first() else {
            return Ok("Usage: /load <session-id>".to_string());
        };
        self.autosave_session();
        let loaded = self.sessions.load(session_id)?;
        let loaded_cwd = PathBuf::from(&loaded.cwd);
        self.session = loaded;
        if loaded_cwd.exists() && loaded_cwd.is_dir() {
            self.set_workspace_root(loaded_cwd)?;
        }
        self.apply_session_workspace_state()?;
        self.input.history = self.session.input_history.clone();
        self.chat_scroll_offset = 0;
        self.redraw_requested = true;
        Ok(format!(
            "Loaded session {} with {} message(s).",
            self.session.session_id,
            self.session.messages.len()
        ))
    }

    pub(crate) fn projects_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("list") => self.projects_list_command(),
            Some("name") | Some("alias") => self.projects_name_command(args),
            Some("forget") | Some("remove") => self.projects_forget_command(args),
            Some("use") | Some("switch") => {
                let Some(path) = args.get(1..) else {
                    return Ok("Usage: /projects use <path>".to_string());
                };
                if path.is_empty() {
                    return Ok("Usage: /projects use <path>".to_string());
                }
                self.switch_workspace_or_alias(&path.join(" "))
            }
            Some(_) => self.switch_workspace_or_alias(&args.join(" ")),
        }
    }

    fn projects_list_command(&self) -> anyhow::Result<String> {
        let mut projects = BTreeMap::<String, ProjectListEntry>::new();
        for session in self.sessions.list()? {
            let entry = projects
                .entry(session.cwd.clone())
                .or_insert_with(|| ProjectListEntry {
                    cwd: session.cwd.clone(),
                    latest_session_id: session.session_id.clone(),
                    latest_created_at: session.created_at,
                    session_count: 0,
                    message_count: 0,
                });
            entry.session_count += 1;
            entry.message_count += session.messages.len();
            if session.created_at > entry.latest_created_at {
                entry.latest_created_at = session.created_at;
                entry.latest_session_id = session.session_id.clone();
            }
        }
        let active = self.cwd.display().to_string();
        projects
            .entry(active.clone())
            .and_modify(|entry| {
                entry.latest_session_id = self.session.session_id.clone();
                entry.latest_created_at = self.session.created_at;
            })
            .or_insert_with(|| ProjectListEntry {
                cwd: active.clone(),
                latest_session_id: self.session.session_id.clone(),
                latest_created_at: self.session.created_at,
                session_count: 1,
                message_count: self.session.messages.len(),
            });
        if projects.is_empty() {
            return Ok("No project workspaces are known yet.".to_string());
        }
        let index = self.load_workspace_index();
        let alias_by_path = index
            .aliases
            .iter()
            .map(|(alias, path)| (path.clone(), alias.clone()))
            .collect::<BTreeMap<_, _>>();
        Ok(projects
            .values()
            .map(|project| {
                let marker = if project.cwd == active { "*" } else { " " };
                let remembered = if project.cwd == active {
                    self.session.session_id.clone()
                } else {
                    index
                        .active_sessions
                        .get(&project.cwd)
                        .cloned()
                        .unwrap_or_else(|| project.latest_session_id.clone())
                };
                let alias = alias_by_path
                    .get(&project.cwd)
                    .map(|alias| format!(" alias={alias}"))
                    .unwrap_or_default();
                format!(
                    "{marker} {}{}  sessions={} messages={} last_session={} path={}",
                    workspace_title(Path::new(&project.cwd)),
                    alias,
                    project.session_count,
                    project.message_count,
                    remembered,
                    project.cwd
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn projects_name_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let Some(alias) = args.get(1) else {
            return Ok("Usage: /projects name <alias> [path]".to_string());
        };
        let alias = crate::core::normalize_agent_id(alias);
        if alias.is_empty() {
            return Ok("Project alias must contain at least one letter or number.".to_string());
        }
        let requested = if args.len() > 2 {
            args[2..].join(" ")
        } else {
            self.cwd.display().to_string()
        };
        let path = self.resolve_project_path(&requested)?;
        let mut index = self.load_workspace_index();
        index
            .aliases
            .insert(alias.clone(), path.display().to_string());
        self.save_workspace_index(&index)?;
        Ok(format!("Project alias {alias} -> {}.", path.display()))
    }

    fn projects_forget_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let Some(alias) = args.get(1) else {
            return Ok("Usage: /projects forget <alias>".to_string());
        };
        let alias = crate::core::normalize_agent_id(alias);
        let mut index = self.load_workspace_index();
        if index.aliases.remove(&alias).is_none() {
            return Ok(format!("Unknown project alias: {alias}"));
        }
        self.save_workspace_index(&index)?;
        Ok(format!("Forgot project alias {alias}."))
    }

    pub(crate) fn workspace_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok(format!("Workspace: {}", self.cwd.display()));
        }
        let path = self.resolve_project_path(&args.join(" "))?;
        self.switch_workspace(path)
    }

    fn switch_workspace_or_alias(&mut self, raw: &str) -> anyhow::Result<String> {
        let alias = crate::core::normalize_agent_id(raw);
        if let Some(path) = self.load_workspace_index().aliases.get(&alias) {
            return self.switch_workspace(path);
        }
        let path = self.resolve_project_path(raw)?;
        self.switch_workspace(path)
    }

    fn resolve_project_path(&self, raw: &str) -> anyhow::Result<PathBuf> {
        let requested = expand_workspace_path(raw);
        let path = if requested.is_absolute() {
            requested
        } else {
            self.cwd.join(requested)
        };
        canonical_workspace(&path)
    }

    fn switch_workspace(&mut self, path: impl AsRef<Path>) -> anyhow::Result<String> {
        self.autosave_session();
        let target = canonical_workspace(path.as_ref())?;
        let restored = self.session_for_workspace(&target)?;
        let restored_id = restored.as_ref().map(|session| session.session_id.clone());
        self.set_workspace_root(&target)?;
        if let Some(session) = restored {
            self.session = session;
        } else {
            self.session = self.sessions.create(
                workspace_title(&target),
                self.provider_selection_for_workspace(&target).provider,
                self.provider_selection_for_workspace(&target).model,
                self.session.enabled_tools.clone(),
                Vec::new(),
            );
            self.session.system_prompt = self
                .config
                .load()
                .ok()
                .and_then(|defaults| {
                    defaults
                        .get("system_prompt")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(default_system_prompt);
        }
        self.session.cwd = self.cwd.display().to_string();
        if self.session.active_agent_id.is_none() {
            self.apply_provider_selection_for_workspace();
        }
        self.apply_session_workspace_state()?;
        self.input.history = self.session.input_history.clone();
        self.chat_scroll_offset = 0;
        self.redraw_requested = true;
        self.autosave_session();
        Ok(match restored_id {
            Some(session_id) => format!(
                "Workspace set to {}. Restored session {}.",
                self.cwd.display(),
                session_id
            ),
            None => format!(
                "Workspace set to {}. Started new project session {}.",
                self.cwd.display(),
                self.session.session_id
            ),
        })
    }

    fn set_workspace_root(&mut self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = canonical_workspace(path.as_ref())?;
        self.cwd = path.clone();
        self.sessions.cwd = path;
        Ok(())
    }

    fn apply_session_workspace_state(&mut self) -> anyhow::Result<()> {
        self.session.cwd = self.cwd.display().to_string();
        self.session.enabled_skills = load_skill_definitions(&self.cwd, &self.data_root)?;
        if let Some(agent_id) = self.session.active_agent_id.clone()
            && let Ok(profile) = self.agents.load(&agent_id)
        {
            self.apply_agent_profile(&profile)?;
            return Ok(());
        }
        self.apply_provider_selection_for_workspace();
        let mut config = self.cms.config.clone();
        config.user_id = self.default_user_id();
        config.project_id = Some(workspace_project_id(&self.cwd));
        self.cms = VegvisirCms::open(config)?;
        self.tool_executor.runtime_policy = RuntimePolicy::default();
        self.rebuild_tooling_for_cms()?;
        Ok(())
    }

    fn latest_session_for_workspace(
        &self,
        workspace: &Path,
    ) -> anyhow::Result<Option<SessionState>> {
        let target = workspace.display().to_string();
        Ok(self
            .sessions
            .list()?
            .into_iter()
            .find(|session| session.cwd == target))
    }

    pub(crate) fn session_for_workspace(
        &self,
        workspace: &Path,
    ) -> anyhow::Result<Option<SessionState>> {
        let target = workspace.display().to_string();
        if let Some(session_id) = self.load_workspace_session_index().get(&target)
            && let Ok(session) = self.sessions.load(session_id)
            && session.cwd == target
        {
            return Ok(Some(session));
        }
        self.latest_session_for_workspace(workspace)
    }

    pub(crate) fn history(&self) -> String {
        self.transcript_text()
    }

    fn transcript_text(&self) -> String {
        if self.session.messages.is_empty() {
            return "No conversation history.".to_string();
        }
        self.session
            .messages
            .iter()
            .map(|message| format!("{}: {}", message.role, message.content))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) fn retry(&mut self) -> anyhow::Result<String> {
        let Some(last_user) = self
            .session
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content.clone())
        else {
            return Ok("No user message to retry.".to_string());
        };
        self.send_demo(&last_user)
    }

    pub(crate) fn branch(&mut self, args: &[String]) -> String {
        let old_id = self.session.session_id.clone();
        let mut next = self.sessions.create(
            join_or(args, &format!("{}-branch", self.session.title)),
            self.session.current_provider.clone(),
            self.session.current_model.clone(),
            self.session.enabled_tools.clone(),
            self.session.enabled_skills.clone(),
        );
        next.messages = self.session.messages.clone();
        next.system_prompt = self.session.system_prompt.clone();
        next.active_agent_id = self.session.active_agent_id.clone();
        next.active_agent_name = self.session.active_agent_name.clone();
        next.context_limit = self.session.context_limit;
        self.session = next;
        format!("Branched {old_id} into {}", self.session.session_id)
    }

    pub(crate) fn compress(&mut self, args: &[String]) -> String {
        let topic = join_or(args, "conversation");
        let original_count = self.session.messages.len();
        if original_count == 0 {
            return "No conversation history to compress.".to_string();
        }

        let capsule = self.context_capsule(&topic, original_count);
        let keep_recent = 6.min(original_count);
        let recent_messages = self
            .session
            .messages
            .iter()
            .skip(original_count.saturating_sub(keep_recent))
            .cloned()
            .collect::<Vec<_>>();

        self.session.messages = vec![ChatMessage {
            role: "system".to_string(),
            content: capsule.clone(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        }];
        self.session.messages.extend(recent_messages);

        capsule
    }

    fn context_capsule(&self, topic: &str, original_count: usize) -> String {
        let recent_window = self
            .session
            .messages
            .iter()
            .rev()
            .take(18)
            .collect::<Vec<_>>();
        let current_objective = recent_window
            .iter()
            .find(|message| message.role == "user")
            .map(|message| compact_context_line(&message.content, 700))
            .unwrap_or_else(|| "Not stated in recent context.".to_string());
        let git_status = git_status_summary(&self.cwd);
        let recent_actions = summarize_recent_actions(&self.session.messages, 10);
        let decisions =
            summarize_context_signals(&self.session.messages, 8, CONTEXT_DECISION_MARKERS);
        let open_issues =
            summarize_context_signals(&self.session.messages, 8, CONTEXT_OPEN_ISSUE_MARKERS);
        let retained = 6.min(original_count);

        let mut lines = vec![
            format!("Context Capsule: {topic}"),
            "Generated by /compress. This is a structured handoff, not a full transcript."
                .to_string(),
            String::new(),
            "Current Objective:".to_string(),
            format!("- {current_objective}"),
            String::new(),
            "Session State:".to_string(),
            format!("- Session: {}", self.session.session_id),
            format!("- Workspace: {}", self.cwd.display()),
            format!(
                "- Provider/model: {}/{}",
                self.session.current_provider, self.session.current_model
            ),
            format!("- Original message count: {original_count}"),
            format!("- Recent messages retained verbatim after this capsule: {retained}"),
            format!("- Git status: {git_status}"),
            String::new(),
            "Recent Actions / Evidence:".to_string(),
        ];
        append_bullets(&mut lines, recent_actions, "No recent actions detected.");
        lines.push(String::new());
        lines.push("Decisions / Constraints / Preferences:".to_string());
        append_bullets(
            &mut lines,
            decisions,
            "No explicit recent decisions detected.",
        );
        lines.push(String::new());
        lines.push("Open Issues / Follow-ups:".to_string());
        append_bullets(
            &mut lines,
            open_issues,
            "No explicit unresolved issue detected.",
        );
        lines.push(String::new());
        lines.push("Continuity Instructions:".to_string());
        lines.push("- Treat this capsule plus the retained recent messages as the active conversation state.".to_string());
        lines.push("- Do not assume details omitted by compression; inspect files, git state, traces, or CMS memory before making project-specific claims.".to_string());
        lines.push("- Preserve user work and keep future changes scoped and verified.".to_string());
        lines.join("\n")
    }

    pub(crate) fn system_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() || matches!(args[0].as_str(), "show" | "print" | "view") {
            return Ok(if self.session.system_prompt.is_empty() {
                "No harness system prompt is set.".to_string()
            } else {
                self.session.system_prompt.clone()
            });
        }
        match args[0].as_str() {
            "default" | "reset-default" => {
                self.session.system_prompt = default_system_prompt();
                self.save_config_defaults()?;
                return Ok("Harness system prompt reset to the Vegvisir default.".to_string());
            }
            "clear" => {
                self.session.system_prompt.clear();
                self.save_config_defaults()?;
                return Ok("Harness system prompt cleared.".to_string());
            }
            "append" => {
                let content = args.get(1).cloned().unwrap_or_default();
                if content.is_empty() {
                    return Ok("Usage: /system append <text>".to_string());
                }
                if !self.session.system_prompt.is_empty() {
                    self.session.system_prompt.push('\n');
                }
                self.session.system_prompt.push_str(&content);
                self.save_config_defaults()?;
                return Ok("Appended to harness system prompt.".to_string());
            }
            "set" => {
                let content = args.get(1).cloned().unwrap_or_default();
                if content.is_empty() {
                    return Ok("Usage: /system set <text>".to_string());
                }
                self.session.system_prompt = content;
            }
            other => self.session.system_prompt = other.to_string(),
        }
        self.save_config_defaults()?;
        Ok("Harness system prompt updated.".to_string())
    }
}
