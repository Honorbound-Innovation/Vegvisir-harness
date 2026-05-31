use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use serde_json::json;

use super::*;

impl TuiApplication {
    pub(crate) fn start_background_send(
        &mut self,
        content: String,
        attachments: Vec<crate::core::Attachment>,
    ) {
        if self.pending_send.is_some() {
            self.queue_steering_message(content, attachments);
            return;
        }
        let display_content = if content.trim().is_empty() && !attachments.is_empty() {
            "Please review the attached file(s).".to_string()
        } else {
            content.clone()
        };
        self.session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: display_content.clone(),
            attachments: attachments.clone(),
            created_at: chrono::Utc::now(),
        });
        self.session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: String::new(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        self.session.status = "streaming".to_string();
        self.session.activity = "using CMS-v2 prepared model request".to_string();
        self.session.activity_tick = 0;
        self.session.spinner_verb_seed = new_spinner_verb_seed(&self.session.session_id);
        self.chat_scroll_offset = 0;
        self.redraw_requested = true;

        let profile_context = self.user_profile.compact_prompt_context();
        let mut worker_session = self.session.clone();
        worker_session.messages.pop();
        worker_session.messages.pop();
        worker_session.pending_attachments = attachments;
        let provider_registry = self.provider_registry.clone();
        let models = self.models.clone();
        let tool_registry = self.tool_registry.clone();
        let tool_executor = self.tool_executor.clone();
        let mut cms_config = self.cms.config.clone();
        let cwd = self.cwd.clone();
        let data_root = self.data_root.clone();
        let lsl_config = self.lsl_runtime_config();
        let autonomous_mode_enabled = self.autonomous_mode_enabled;
        let (stream_tx, stream_rx) = mpsc::channel();
        let (steering_tx, steering_rx) = mpsc::channel();
        let cancel_token = Arc::new(AtomicBool::new(false));
        let worker_cancel_token = Arc::clone(&cancel_token);
        self.pending_stream = Some(stream_rx);
        self.pending_steering = Some(steering_tx);
        let handle = thread::spawn(move || -> anyhow::Result<SessionState> {
            let mut cms = VegvisirCms::open({
                cms_config.commit_writebacks = true;
                cms_config
            })?;
            let mut runner = ConversationRunner {
                provider: ProviderRouter::from_registry(&provider_registry)
                    .get(&worker_session.current_provider)
                    .cloned()
                    .ok_or_else(|| {
                        anyhow::anyhow!("Unknown provider: {}", worker_session.current_provider)
                    })?,
                models,
                tools: Some(tool_registry),
                tool_executor: Some(tool_executor),
                cancel_token: Some(Arc::clone(&worker_cancel_token)),
                steering_rx: Some(steering_rx),
                event_sink: Some(Arc::new({
                    let stream_tx = stream_tx.clone();
                    move |event| {
                        let event = match event {
                            ProviderRunEvent::Activity(activity) => StreamEvent::Activity(activity),
                            ProviderRunEvent::ToolStart { name, args } => {
                                StreamEvent::ToolStart { name, args }
                            }
                            ProviderRunEvent::ToolEnd {
                                name,
                                ok,
                                summary,
                                detail,
                            } => StreamEvent::ToolEnd {
                                name,
                                ok,
                                summary,
                                detail,
                            },
                        };
                        let _ = stream_tx.send(event);
                    }
                })),
            };
            let (model_content, skill_trace) = prepare_lsl_augmented_content(
                &cwd,
                &data_root,
                &display_content,
                &worker_session,
                &lsl_config,
            )?;
            let model_content =
                apply_user_profile_context(profile_context.as_deref(), &model_content);
            let model_content = apply_subagent_delegation_context(&model_content);
            let model_content = if autonomous_mode_enabled {
                apply_autonomous_mode_contract(&model_content)
            } else {
                model_content
            };
            let envelope = cms.prepare_cached_prompt(
                &model_content,
                worker_session.current_provider.clone(),
                worker_session.current_model.clone(),
            )?;
            let mut on_delta = |delta: &str| {
                if !worker_cancel_token.load(Ordering::SeqCst) {
                    let _ = stream_tx.send(StreamEvent::Delta(delta.to_string()));
                }
            };
            let response = runner.send_with_envelope_streaming(
                &mut worker_session,
                &model_content,
                envelope,
                &mut on_delta,
            )?;
            if worker_cancel_token.load(Ordering::SeqCst) {
                anyhow::bail!("Cancelled");
            }
            if skill_trace
                .as_ref()
                .is_some_and(|trace| trace.event == "auto_load")
            {
                let _ = update_skill_metrics_for_load(
                    &cwd.join("skills"),
                    &compiled_lsl_selected_from_trace(
                        &cwd,
                        &data_root,
                        &display_content,
                        &lsl_config,
                    ),
                    Some(true),
                );
            }
            if let Some(trace) = skill_trace {
                let _ = append_skill_trace(
                    &cwd.join(".vegvisir")
                        .join("compiled")
                        .join("skill_traces.json"),
                    trace,
                );
            }
            // Do not run CMS writeback on the foreground TUI worker. Completion
            // writeback can involve SQLite/vectors/graph work and has previously
            // made the live UI look stalled after the provider finished: status
            // stayed "streaming" and the context counter did not advance because
            // the JoinHandle could not complete. Snapshot the answer and persist
            // memory asynchronously instead.
            spawn_cms_complete_turn_writeback(
                cms.config.clone(),
                display_content.clone(),
                response.clone(),
            );
            Ok(worker_session)
        });
        self.pending_send = Some(handle);
        self.pending_cancel = Some(cancel_token);
    }

    pub fn poll_pending_send(&mut self) -> bool {
        let Some(handle) = self.pending_send.take() else {
            return false;
        };
        if !handle.is_finished() {
            self.pending_send = Some(handle);
            return false;
        }
        match handle.join() {
            Ok(Ok(mut session)) => {
                // Drain any final streamed tool/activity events before replacing
                // the live session. A worker can finish between the regular
                // poll_stream_events() call and this join path; without this
                // drain, final ToolEnd/error observations can be lost and the
                // turn appears to stop without explaining what happened.
                self.poll_stream_events();
                self.merge_live_tool_messages(&mut session);
                self.merge_live_reasoning_trace(&mut session);
                self.restore_latest_visible_user_message(&mut session);
                let had_tool_activity = self.completed_session_had_tool_activity(&session);
                self.session = session;
                self.pending_stream = None;
                self.pending_cancel = None;
                self.pending_steering = None;
                self.autonomy.last_turn_had_tools = had_tool_activity;
                self.autosave_session();
            }
            Ok(Err(error)) => {
                // Preserve final tool failure/progress events before clearing
                // pending_stream. This keeps failed-tool turns from ending as a
                // silent/empty assistant message with no "what failed" context.
                self.poll_stream_events();
                self.session.status = "ready".to_string();
                self.session.activity.clear();
                self.pending_stream = None;
                self.pending_cancel = None;
                self.pending_steering = None;
                self.pop_empty_assistant_placeholder();
                if error.to_string() == "Cancelled" {
                    self.pop_last_assistant_response();
                    self.push_system_message("Cancelled in-flight model response.");
                    if self.autonomy.active {
                        self.autonomy.active = false;
                        self.autonomy.enabled = false;
                        self.autonomy.last_status = "cancelled".to_string();
                    }
                } else {
                    self.push_turn_failure_summary(error.to_string());
                    if self.autonomy.active {
                        self.autonomy.active = false;
                        self.autonomy.enabled = false;
                        self.autonomy.last_status = format!("failed: {error}");
                    }
                }
                self.autosave_session();
            }
            Err(_) => {
                self.poll_stream_events();
                self.session.status = "ready".to_string();
                self.session.activity.clear();
                self.pending_stream = None;
                self.pending_cancel = None;
                self.pending_steering = None;
                self.pop_empty_assistant_placeholder();
                self.push_turn_failure_summary(
                    "provider worker panicked before completing the turn".to_string(),
                );
                if self.autonomy.active {
                    self.autonomy.active = false;
                    self.autonomy.enabled = false;
                    self.autonomy.last_status = "failed: provider worker panicked".to_string();
                }
                self.autosave_session();
            }
        }
        self.chat_scroll_offset = 0;
        self.redraw_requested = true;
        true
    }

    fn completed_session_had_tool_activity(&self, completed: &SessionState) -> bool {
        completed
            .messages
            .iter()
            .any(|message| message.role == "system" && is_live_tool_message(&message.content))
    }

    pub fn poll_background_jobs(&mut self) -> bool {
        let mut changed = false;
        let mut index = 0usize;
        while index < self.pending_background_jobs.len() {
            if !self.pending_background_jobs[index].is_finished() {
                index += 1;
                continue;
            }
            let handle = self.pending_background_jobs.remove(index);
            match handle.join() {
                Ok(Ok(message)) => self.push_system_message(message),
                Ok(Err(error)) => self.push_system_message(format!("Error: {error}")),
                Err(_) => self.push_system_message("Error: background job panicked."),
            }
            changed = true;
        }

        let mut speech_index = 0usize;
        while speech_index < self.pending_speech_jobs.len() {
            if !self.pending_speech_jobs[speech_index].is_finished() {
                speech_index += 1;
                continue;
            }
            let handle = self.pending_speech_jobs.remove(speech_index);
            match handle.join() {
                Ok(Ok(result)) => {
                    let text = result.transcript.trim().to_string();
                    if text.is_empty() {
                        self.push_system_message(format!(
                            "Speech push-to-talk completed but returned no text. {}; audio kept at {} for inspection.",
                            result.summary(),
                            result.audio_path.display()
                        ));
                    } else {
                        self.insert_speech_text(&text);
                        self.push_system_message(format!(
                            "Speech push-to-talk transcript inserted into the input buffer. Review/edit, then press Enter to send. {}",
                            result.summary()
                        ));
                    }
                }
                Ok(Err(error)) => {
                    self.push_system_message(format!("Speech push-to-talk failed: {error}"))
                }
                Err(_) => self.push_system_message("Speech push-to-talk job panicked."),
            }
            self.session.activity.clear();
            changed = true;
        }

        if changed {
            self.autosave_session();
            self.chat_scroll_offset = 0;
            self.redraw_requested = true;
        }
        changed
    }

    pub(crate) fn queue_steering_message(
        &mut self,
        content: String,
        attachments: Vec<crate::core::Attachment>,
    ) {
        let display_content = if content.trim().is_empty() && !attachments.is_empty() {
            "Please review the attached file(s).".to_string()
        } else {
            content.trim().to_string()
        };
        if display_content.trim().is_empty() {
            return;
        }
        if let Some(sender) = &self.pending_steering {
            match sender.send(display_content.clone()) {
                Ok(()) => {
                    let attachment_note = if attachments.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "

Note: {} attachment(s) were not injected into the in-flight run; send them after the run or cancel/retry if the model needs the files.",
                            attachments.len()
                        )
                    };
                    self.push_system_message(format!(
                        "Queued steering message for the in-flight model run. It will be injected after the next completed tool call, or before the final save if the run ends first.

Steering: {display_content}{attachment_note}"
                    ));
                }
                Err(_) => self.push_system_message(
                    "Could not queue steering message because the in-flight run is closing."
                        .to_string(),
                ),
            }
        } else {
            self.push_system_message("A model response is already in progress.".to_string());
        }
        self.autosave_session();
        self.chat_scroll_offset = 0;
        self.redraw_requested = true;
    }

    pub(crate) fn cancel_pending_response(&mut self) -> String {
        let Some(handle) = self.pending_send.take() else {
            return "No in-flight model response to cancel.".to_string();
        };
        self.autonomy.active = false;
        self.autonomy.enabled = false;
        self.autonomy.last_status = "cancelled".to_string();
        if let Some(cancel_token) = &self.pending_cancel {
            cancel_token.store(true, Ordering::SeqCst);
        }
        drop(handle);
        self.pending_stream = None;
        self.pending_cancel = None;
        self.pending_steering = None;
        self.session.status = "ready".to_string();
        self.session.activity.clear();
        self.pop_last_assistant_response();
        self.push_system_message("Cancelled in-flight model response.");
        self.autosave_session();
        self.chat_scroll_offset = 0;
        self.redraw_requested = true;
        self.logger.emit(
            "provider_cancelled",
            json!({
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
            }),
        );
        "Cancelled in-flight model response.".to_string()
    }

    pub(crate) fn handle_ctrl_c(&mut self) {
        if self.pending_send.is_some() {
            let _ = self.cancel_pending_response();
        } else {
            self.running = false;
        }
    }

    pub(crate) fn poll_stream_events(&mut self) {
        const MAX_STREAM_EVENTS_PER_POLL: usize = 256;

        let mut events = Vec::new();
        let mut reached_frame_budget = false;
        if let Some(receiver) = &self.pending_stream {
            for _ in 0..MAX_STREAM_EVENTS_PER_POLL {
                match receiver.try_recv() {
                    Ok(event) => events.push(event),
                    Err(_) => break,
                }
            }
            reached_frame_budget = events.len() == MAX_STREAM_EVENTS_PER_POLL;
        }
        if events.is_empty() {
            return;
        }
        for event in events {
            match event {
                StreamEvent::Delta(delta) => {
                    let assistant_index = self
                        .session
                        .messages
                        .iter()
                        .rposition(|message| message.role == "assistant")
                        .unwrap_or_else(|| {
                            self.session.messages.push(ChatMessage {
                                role: "assistant".to_string(),
                                content: String::new(),
                                attachments: Vec::new(),
                                created_at: chrono::Utc::now(),
                            });
                            self.session.messages.len() - 1
                        });
                    self.session.messages[assistant_index]
                        .content
                        .push_str(&delta);
                }
                StreamEvent::Activity(activity) => {
                    self.session.activity = activity;
                }
                StreamEvent::ToolStart { name, args } => {
                    self.session.activity = format!("using tool {name}");
                    self.push_live_tool_message(format!("Running tool: {name} {args}"));
                }
                StreamEvent::ToolEnd {
                    name,
                    ok,
                    summary,
                    detail,
                } => {
                    self.session.activity = format!("finished tool {name}");
                    let status = if ok { "finished" } else { "failed" };
                    let mut content = format!("Tool {status}: {name} - {summary}");
                    let detail = detail.as_deref().filter(|detail| !detail.trim().is_empty());
                    if let Some(detail) = detail {
                        content.push_str("\n\n");
                        content.push_str(detail);
                    }
                    self.push_live_tool_message(content);
                    if ok {
                        if let Some(detail) = detail {
                            self.push_assistant_tool_artifact(&name, detail);
                        }
                    }
                }
            }
        }
        self.redraw_requested = true;
        if reached_frame_budget {
            // Leave remaining deltas for the next UI tick. This prevents a hot
            // streaming provider from monopolizing the TUI thread and starving
            // redraw/input/finalization work.
            self.session.activity_tick = self.session.activity_tick.saturating_add(1);
        }
    }

    pub(crate) fn push_turn_failure_summary(&mut self, error: String) {
        let recent_tool_messages = self
            .session
            .messages
            .iter()
            .rev()
            .take(12)
            .filter(|message| message.role == "system" && is_live_tool_message(&message.content))
            .take(4)
            .map(|message| first_nonempty_line(&message.content).to_string())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>();

        let mut content = String::from(
            "Turn failed before the model produced a normal final summary. Preserved recovery context follows.

",
        );
        if recent_tool_messages.is_empty() {
            content.push_str(
                "What happened: no final tool/progress event was available before the turn failed.
",
            );
        } else {
            content.push_str(
                "Recent tool/progress events:
",
            );
            for line in recent_tool_messages {
                content.push_str("- ");
                content.push_str(&line);
                content.push('\n');
            }
        }
        content.push_str(
            "
Failure:
",
        );
        content.push_str(error.trim());
        content.push_str(
            "

Next step: I should retry or continue from the last successful step instead of leaving the turn silently truncated.",
        );

        self.push_live_tool_message(content);
    }

    pub(crate) fn push_live_tool_message(&mut self, content: String) {
        if self
            .session
            .messages
            .last()
            .map(|message| message.role == "system" && message.content == content)
            .unwrap_or(false)
        {
            return;
        }
        self.session.messages.push(ChatMessage {
            role: "system".to_string(),
            content,
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
    }

    pub(crate) fn push_assistant_tool_artifact(&mut self, tool_name: &str, detail: &str) {
        let detail = detail.trim();
        if detail.is_empty() || !is_chat_visible_tool_artifact(detail) {
            return;
        }
        let artifact = format!("\n\n**Code/Diff update from `{tool_name}`**\n\n{detail}\n");
        let assistant_index = self
            .session
            .messages
            .iter()
            .rposition(|message| message.role == "assistant")
            .unwrap_or_else(|| {
                self.session.messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: String::new(),
                    attachments: Vec::new(),
                    created_at: chrono::Utc::now(),
                });
                self.session.messages.len() - 1
            });
        if self.session.messages[assistant_index]
            .content
            .contains(&artifact)
        {
            return;
        }
        self.session.messages[assistant_index]
            .content
            .push_str(&artifact);
    }

    pub(crate) fn restore_latest_visible_user_message(&self, completed: &mut SessionState) {
        let Some(live_user) = self
            .session
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .cloned()
        else {
            return;
        };
        let Some(completed_user) = completed
            .messages
            .iter_mut()
            .rev()
            .find(|message| message.role == "user")
        else {
            return;
        };

        // The worker session stores the provider-facing content, which can
        // include hidden user-context wrappers such as the local profile block
        // or autonomy contracts. The live TUI session stores the actual text the
        // user typed. Preserve that visible text in chat/history while still
        // letting the provider receive the augmented model request.
        completed_user.content = live_user.content;
        completed_user.attachments = live_user.attachments;
    }

    pub(crate) fn merge_live_tool_messages(&self, completed: &mut SessionState) {
        let live_messages = self
            .session
            .messages
            .iter()
            .filter(|message| message.role == "system" && is_live_tool_message(&message.content))
            .filter(|message| {
                !completed.messages.iter().any(|existing| {
                    existing.role == message.role && existing.content == message.content
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        if live_messages.is_empty() {
            return;
        }
        let insert_at = completed
            .messages
            .iter()
            .rposition(|message| message.role == "assistant")
            .unwrap_or(completed.messages.len());
        completed
            .messages
            .splice(insert_at..insert_at, live_messages);
    }

    pub(crate) fn merge_live_reasoning_trace(&self, completed: &mut SessionState) {
        let Some(live_content) = self
            .session
            .messages
            .iter()
            .rposition(|message| message.role == "user")
            .and_then(|last_user_index| {
                self.session.messages[last_user_index + 1..]
                    .iter()
                    .rev()
                    .find(|message| message.role == "assistant" && !message.content.is_empty())
            })
            .map(|message| message.content.clone())
        else {
            return;
        };
        if let Some(completed_message) = completed
            .messages
            .iter_mut()
            .rev()
            .find(|message| message.role == "assistant")
        {
            // Preserve live assistant-visible artifacts (thinking traces,
            // code/diff updates emitted after tool completion) across the final
            // session swap. The worker's completed response does not include
            // UI-only artifacts injected from tool observations, so merge them
            // instead of dropping them when the provider turn finishes.
            if completed_message.content.trim().is_empty()
                || live_content.len() >= completed_message.content.len()
            {
                if live_content.contains("**Code/Diff update from `")
                    && !completed_message
                        .content
                        .contains("**Code/Diff update from `")
                    && !completed_message.content.trim().is_empty()
                {
                    completed_message.content = format!(
                        "{}\n\n{}",
                        live_content.trim_end(),
                        completed_message.content.trim_start()
                    );
                } else {
                    completed_message.content = live_content;
                }
            }
        }
    }

    pub(crate) fn pop_empty_assistant_placeholder(&mut self) {
        if self
            .session
            .messages
            .last()
            .map(|message| message.role == "assistant" && message.content.is_empty())
            .unwrap_or(false)
        {
            self.session.messages.pop();
        }
    }

    pub(crate) fn pop_last_assistant_response(&mut self) {
        if self
            .session
            .messages
            .last()
            .map(|message| message.role == "assistant")
            .unwrap_or(false)
        {
            self.session.messages.pop();
        }
    }

    pub(crate) fn chat_page_size(&self) -> usize {
        self.renderer
            .viewport
            .map(|(_, lines)| lines / 2)
            .or_else(|| {
                crossterm::terminal::size()
                    .ok()
                    .map(|(_, lines)| usize::from(lines) / 2)
            })
            .unwrap_or(16)
            .max(5)
    }

    pub(crate) fn command_palette_page_size(&self) -> usize {
        self.renderer
            .viewport
            .map(|(_, lines)| usize::from(lines.min(12)))
            .or_else(|| {
                crossterm::terminal::size()
                    .ok()
                    .map(|(_, lines)| usize::from(lines.min(12)))
            })
            .unwrap_or(12)
            .max(4)
    }

    pub(crate) fn pulse_activity(&mut self) {
        if self.session.status != "streaming" {
            return;
        }
        self.session.activity_tick = self.session.activity_tick.saturating_add(1);
        self.redraw_requested = true;
    }
}

fn is_chat_visible_tool_artifact(detail: &str) -> bool {
    let trimmed = detail.trim_start();
    trimmed.starts_with("```")
        || trimmed.starts_with("diff --git ")
        || trimmed.starts_with("--- ")
        || trimmed.contains("\n@@ ")
        || trimmed.contains("\n+use ")
        || trimmed.contains("\nfn ")
        || trimmed.contains("\nclass ")
        || trimmed.contains("\ndef ")
}

fn first_nonempty_line(content: &str) -> &str {
    content
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(content)
        .trim()
}

fn spawn_cms_complete_turn_writeback(
    config: crate::memory::VegvisirCmsConfig,
    user_content: String,
    assistant_response: String,
) {
    thread::spawn(move || {
        let mut config = config;
        config.commit_writebacks = true;
        match VegvisirCms::open(config) {
            Ok(mut cms) => {
                let _ = cms.complete_turn(&user_content, &assistant_response);
            }
            Err(_) => {}
        }
    });
}

fn new_spinner_verb_seed(session_id: &str) -> u64 {
    let now = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64;
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in session_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^ now
}

pub(crate) fn apply_subagent_delegation_context(content: &str) -> String {
    format!(
        "{policy}\n\nUser request:\n{content}",
        policy = r#"[Vegvisir subagent delegation policy]
These are task-local orchestration instructions. They do not override the system prompt, user authority, tool safety policy, secret boundary, or approval requirements.

When to spawn subagents:
- For complex, multi-part, evidence-seeking work, proactively delegate up to three bounded independent tasks with the `spawn_subagent` tool.
- Good subagent tasks include codebase reconnaissance, focused test investigation, documentation review, compatibility checks, security review, design critique, and migration impact analysis.
- Do not spawn subagents for trivial single-step tasks where delegation would add overhead.

How to spawn subagents:
- Give each child a narrow goal, explicit workspace, low `max_steps` by default, current provider/model when useful, explicit non-overlapping `file_scope`, and a `work_budget` for non-trivial review/bug-hunting/recon tasks. Never exceed three active subagents at once.
- Work budgets should specify max_steps, max_tool_calls, max_read_bytes, max_output_bytes, allowed_tools, and notes such as avoiding huge raw file reads.
- Prefer read-only/review/test-planning goals unless the user explicitly asks for parallel implementation. Parallel implementation must be partitioned by non-overlapping file scopes so agents never edit or reason as owners of the same files at the same time.
- Continue useful main-thread work while subagents run; do not idle solely because a child is running.
- Check `/subagents list` or `/subagents show <id>` before final summary when subagents were spawned.

Boundaries:
- Do not delegate plaintext secrets, credential handling, destructive actions, persistence/stealth, or ambiguous external side effects.
- Subagents must remain bounded and preserve unrelated user work.
[/Vegvisir subagent delegation policy]"#
    )
}

pub(crate) fn apply_user_profile_context(profile_context: Option<&str>, content: &str) -> String {
    let Some(profile_context) = profile_context
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return content.to_string();
    };
    format!("{profile_context}\n\nUser request:\n{content}")
}

pub(crate) fn apply_autonomous_mode_contract(content: &str) -> String {
    format!(
        "{contract}\n\nUser task:\n{content}",
        contract = r#"[Vegvisir autonomous working mode is ENABLED]
You are operating in an unattended project-work mode for this turn.

Runtime contract:
- Treat the user task as permission to complete the whole coherent workflow, not merely the next small step.
- Orient, plan, inspect evidence, implement, verify, and summarize without waiting for unnecessary chat confirmation.
- Use available tools proactively and keep visible progress through tool/activity events.
- Prefer reversible, scoped edits; preserve unrelated user work.
- Run focused tests/builds/checks when practical, and report verification clearly.
- Continue through routine fix/test iterations until the workflow is complete, blocked, cancelled, or requires user authority.
- Stop and request approval for destructive operations, privileged actions, secret use, external side effects, ambiguous scope, or policy-required approvals.
- Never ask for plaintext secrets; use HBSE secret refs when credentials are required.
- End with a concise completion report: changed files, tests/checks run, unresolved risks, and exact next steps if blocked."#,
        content = content
    )
}
