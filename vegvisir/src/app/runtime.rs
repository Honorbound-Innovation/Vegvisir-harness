use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::json;

use super::*;

const STREAM_CHANNEL_CAPACITY: usize = 1024;
const MAX_STREAM_EVENT_TEXT_BYTES: usize = 64 * 1024;
const CMS_WRITEBACK_QUEUE_CAPACITY: usize = 4;
const STEERING_CHANNEL_CAPACITY: usize = 32;

impl TuiApplication {
    pub(crate) fn show_ephemeral_notice(
        &mut self,
        text: impl Into<String>,
        kind: EphemeralNoticeKind,
        ttl: Duration,
    ) {
        self.ephemeral_notice = Some(EphemeralNotice::new(text, kind, ttl));
        self.redraw_requested = true;
    }

    pub(crate) fn show_approval_notice(&mut self, text: impl Into<String>) {
        self.show_ephemeral_notice(text, EphemeralNoticeKind::Approval, Duration::from_secs(5));
    }

    pub(crate) fn expire_ephemeral_notice(&mut self) {
        if self
            .ephemeral_notice
            .as_ref()
            .is_some_and(EphemeralNotice::is_expired)
        {
            self.ephemeral_notice = None;
            self.redraw_requested = true;
        }
    }

    pub(crate) fn start_background_send(
        &mut self,
        content: String,
        attachments: Vec<crate::core::Attachment>,
    ) {
        let display_content = if content.trim().is_empty() && !attachments.is_empty() {
            "Please review the attached file(s).".to_string()
        } else {
            content.clone()
        };
        self.start_background_send_with_display(content, display_content, attachments);
    }

    pub(crate) fn start_background_send_with_display(
        &mut self,
        content: String,
        display_content: String,
        attachments: Vec<crate::core::Attachment>,
    ) {
        if self.pending_send.is_some() {
            self.queue_steering_message(content, attachments);
            return;
        }
        self.session.enforce_history_limits();
        let content = crate::core::truncate_utf8_middle(
            &content,
            crate::core::MAX_SESSION_MESSAGE_BYTES,
            "user message",
        );
        let display_content = crate::core::truncate_utf8_middle(
            &display_content,
            crate::core::MAX_SESSION_MESSAGE_BYTES,
            "display message",
        );
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
        let acp_context = crate::acp::AcpSnapshot::load(&cwd)
            .ok()
            .filter(|snapshot| snapshot.initialized)
            .map(|snapshot| snapshot.render_context());
        let autonomous_mode_enabled = self.autonomous_mode_enabled;
        let autonomy_level = self.autonomous_level.min(6) as u8;
        let goal_mode_enabled = self.goal.active;
        let (stream_tx, stream_rx) = mpsc::sync_channel(STREAM_CHANNEL_CAPACITY);
        let (steering_tx, steering_rx) = mpsc::sync_channel(STEERING_CHANNEL_CAPACITY);
        let cancel_token = Arc::new(AtomicBool::new(false));
        let worker_cancel_token = Arc::clone(&cancel_token);
        self.pending_stream = Some(stream_rx);
        self.pending_steering = Some(steering_tx);
        self.start_tui_turn_artifact(&display_content);
        let turn_artifact_manager = self
            .pending_run_artifact
            .as_ref()
            .map(|(manager, _)| manager.clone());
        let now = Instant::now();
        self.pending_turn_started_at = Some(now);
        self.pending_turn_last_activity_at = Some(now);
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
                            ProviderRunEvent::Activity(activity) => {
                                StreamEvent::Activity(crate::core::truncate_utf8_middle(
                                    &activity,
                                    MAX_STREAM_EVENT_TEXT_BYTES,
                                    "stream activity",
                                ))
                            }
                            ProviderRunEvent::ApprovalRequired { request } => {
                                StreamEvent::ApprovalRequired { request }
                            }
                            ProviderRunEvent::ToolStart { name, args } => StreamEvent::ToolStart {
                                name,
                                args: crate::core::truncate_utf8_middle(
                                    &args,
                                    MAX_STREAM_EVENT_TEXT_BYTES,
                                    "tool arguments",
                                ),
                            },
                            ProviderRunEvent::ToolOutput {
                                name,
                                stream,
                                chunk,
                                truncated,
                            } => StreamEvent::ToolOutput {
                                name,
                                stream,
                                chunk: crate::core::truncate_utf8_middle(
                                    &chunk,
                                    MAX_STREAM_EVENT_TEXT_BYTES,
                                    "tool output",
                                ),
                                truncated,
                            },
                            ProviderRunEvent::ToolEnd {
                                name,
                                ok,
                                summary,
                                detail,
                            } => StreamEvent::ToolEnd {
                                name,
                                ok,
                                summary: crate::core::truncate_utf8_middle(
                                    &summary,
                                    MAX_STREAM_EVENT_TEXT_BYTES,
                                    "tool summary",
                                ),
                                detail: detail.map(|detail| {
                                    crate::core::truncate_utf8_middle(
                                        &detail,
                                        MAX_STREAM_EVENT_TEXT_BYTES,
                                        "tool detail",
                                    )
                                }),
                            },
                        };
                        let _ = stream_tx.send(event);
                    }
                })),
            };
            let (model_content, skill_trace) = prepare_lsl_augmented_content(
                &cwd,
                &data_root,
                &content,
                &worker_session,
                &lsl_config,
            )?;
            let model_content =
                apply_user_profile_context(profile_context.as_deref(), &model_content);
            let model_content = apply_subagent_delegation_context(&model_content);
            let model_content = if let Some(acp_context) = acp_context.as_deref() {
                format!("{model_content}\n\n{acp_context}")
            } else {
                model_content
            };
            let model_content = if goal_mode_enabled {
                apply_goal_mode_contract(&model_content)
            } else if autonomous_mode_enabled {
                apply_autonomous_mode_contract(&model_content, autonomy_level)
            } else {
                model_content
            };
            let envelope = cms.prepare_cached_prompt(
                &model_content,
                worker_session.current_provider.clone(),
                worker_session.current_model.clone(),
            )?;
            let _ = stream_tx.send(StreamEvent::PromptEnvelope(Box::new(envelope.clone())));
            let mut on_delta = |delta: &str| {
                if !worker_cancel_token.load(Ordering::SeqCst) {
                    let delta = crate::core::truncate_utf8_middle(
                        delta,
                        MAX_STREAM_EVENT_TEXT_BYTES,
                        "stream delta",
                    );
                    let _ = stream_tx.send(StreamEvent::Delta(delta));
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
            // made the live UI look stalled after the provider finished. Snapshot
            // the answer and persist memory asynchronously; when a run artifact
            // manager exists, the background write also records memory-written.json.
            spawn_cms_complete_turn_writeback(
                cms.config.clone(),
                display_content.clone(),
                response.clone(),
                turn_artifact_manager,
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
                session.enforce_history_limits();
                let had_tool_activity = self.completed_session_had_tool_activity(&session);
                let final_response = session
                    .messages
                    .iter()
                    .rev()
                    .find(|message| message.role == "assistant")
                    .map(|message| message.content.clone())
                    .unwrap_or_default();
                self.finish_tui_turn_artifact(RunStatus::Completed, Some(&final_response));
                self.session = session;
                self.clear_pending_turn_runtime_handles();
                self.autonomy.last_turn_had_tools = had_tool_activity;
                self.autosave_session();
            }
            Ok(Err(error)) => {
                // Preserve final tool failure/progress events before clearing
                // pending_stream. This keeps failed-tool turns from ending as a
                // silent/empty assistant message with no "what failed" context.
                self.poll_stream_events();
                self.session.enforce_history_limits();
                self.session.status = "ready".to_string();
                self.session.activity.clear();
                self.clear_pending_turn_runtime_handles();
                self.pop_empty_assistant_placeholder();
                if error.to_string() == "Cancelled" {
                    self.finish_active_tool_tasks_without_tool_end(
                        crate::tasks::TaskState::Cancelled,
                    );
                    self.finish_tui_turn_artifact(RunStatus::Cancelled, None);
                    self.pop_last_assistant_response();
                    self.push_system_message("Cancelled in-flight model response.");
                    if self.autonomy.active {
                        self.autonomy.active = false;
                        self.autonomy.enabled = false;
                        self.autonomy.last_status = "cancelled".to_string();
                    }
                    self.goal_cancelled("cancelled");
                } else {
                    self.finish_active_tool_tasks_without_tool_end(crate::tasks::TaskState::Failed);
                    let failed_run = self.fail_tui_turn_artifact(&error.to_string(), true);
                    self.push_turn_failure_summary(error.to_string());
                    self.auto_recover_failed_turn(
                        "provider worker returned an error",
                        Some(error.to_string()),
                        failed_run,
                    );
                    if self.autonomy.active {
                        self.autonomy.active = false;
                        self.autonomy.enabled = false;
                        self.autonomy.last_status = format!("failed: {error}");
                    }
                    self.goal_cancelled(&format!("failed: {error}"));
                }
                self.autosave_session();
            }
            Err(_) => {
                self.poll_stream_events();
                self.session.enforce_history_limits();
                self.session.status = "ready".to_string();
                self.session.activity.clear();
                self.clear_pending_turn_runtime_handles();
                self.pop_empty_assistant_placeholder();
                self.finish_active_tool_tasks_without_tool_end(crate::tasks::TaskState::Failed);
                let failed_run = self.fail_tui_turn_artifact(
                    "provider worker panicked before completing the turn",
                    true,
                );
                self.push_turn_failure_summary(
                    "provider worker panicked before completing the turn".to_string(),
                );
                self.auto_recover_failed_turn(
                    "provider worker panicked before completing the turn",
                    None,
                    failed_run,
                );
                if self.autonomy.active {
                    self.autonomy.active = false;
                    self.autonomy.enabled = false;
                    self.autonomy.last_status = "failed: provider worker panicked".to_string();
                }
                self.goal_cancelled("failed: provider worker panicked");
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
                    self.logger.emit(
                        "speech_ptt_finished",
                        json!({
                            "session": self.session.session_id,
                            "workspace": self.cwd.display().to_string(),
                            "audio_path": result.audio_path.display().to_string(),
                            "audio_bytes": result.audio_bytes,
                            "kept_audio": result.kept_audio,
                            "recorder": result.recorder,
                            "transcript_chars": result.transcript.chars().count(),
                        }),
                    );
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
                            "Speech push-to-talk transcript submitted. {}",
                            result.summary()
                        ));
                        self.handle_submit();
                    }
                }
                Ok(Err(error)) => {
                    self.logger.emit(
                        "speech_ptt_failed",
                        json!({
                            "session": self.session.session_id,
                            "workspace": self.cwd.display().to_string(),
                            "error": error.to_string(),
                        }),
                    );
                    self.push_system_message(format!("Speech push-to-talk failed: {error}"))
                }
                Err(_) => {
                    self.logger.emit(
                        "speech_ptt_panicked",
                        json!({
                            "session": self.session.session_id,
                            "workspace": self.cwd.display().to_string(),
                        }),
                    );
                    self.push_system_message("Speech push-to-talk job panicked.")
                }
            }
            if self.active_speech_recording.is_none() {
                self.session.activity.clear();
            }
            changed = true;
        }

        if self.poll_subagent_transcript_updates() {
            changed = true;
        }

        if changed {
            self.autosave_session();
            self.chat_scroll_offset = 0;
            self.redraw_requested = true;
        }
        changed
    }

    pub(crate) fn seed_observed_subagent_transcript_signatures(&mut self) {
        let Ok(records) = self.load_subagent_records() else {
            return;
        };
        self.observed_subagent_transcript_signatures = records
            .into_iter()
            .map(|record| (record.id.clone(), subagent_transcript_signature(&record)))
            .collect();
    }

    pub(crate) fn poll_subagent_transcript_updates(&mut self) -> bool {
        if self.observed_subagent_transcript_signatures.is_empty()
            && !self.subagent_board_path().exists()
        {
            return false;
        }
        let now = Instant::now();
        if self
            .last_subagent_board_poll
            .is_some_and(|last_poll| now.duration_since(last_poll) < Duration::from_millis(500))
        {
            return false;
        }
        self.last_subagent_board_poll = Some(now);

        let Ok(records) = self.load_subagent_records() else {
            return false;
        };
        let mut changed = false;
        for record in records {
            let signature = subagent_transcript_signature(&record);
            if self
                .observed_subagent_transcript_signatures
                .get(&record.id)
                .is_some_and(|existing| existing == &signature)
            {
                continue;
            }
            self.observed_subagent_transcript_signatures
                .insert(record.id.clone(), signature);
            changed = true;
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
        let display_content = crate::core::truncate_utf8_middle(
            &display_content,
            crate::core::MAX_SESSION_MESSAGE_BYTES,
            "steering message",
        );
        if let Some(sender) = &self.pending_steering {
            match sender.try_send(display_content.clone()) {
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
                Err(std::sync::mpsc::TrySendError::Full(_)) => self.push_system_message(
                    "Could not queue steering message because the in-flight run already has the maximum pending steering messages."
                        .to_string(),
                ),
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => self.push_system_message(
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
        self.goal_cancelled("cancelled");
        if let Some(cancel_token) = &self.pending_cancel {
            cancel_token.store(true, Ordering::SeqCst);
        }
        drop(handle);
        self.clear_pending_turn_runtime_handles();
        self.session.status = "ready".to_string();
        self.session.activity.clear();
        self.pop_last_assistant_response();
        self.finish_active_tool_tasks_without_tool_end(crate::tasks::TaskState::Cancelled);
        self.finish_tui_turn_artifact(RunStatus::Cancelled, None);
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

    pub(crate) fn turn_repair_command(&mut self, args: &[String]) -> String {
        let force = args
            .iter()
            .any(|arg| matches!(arg.trim(), "force" | "--force" | "-f"));
        self.turn_repair(force).unwrap_or_else(|| {
            if self.pending_send.is_some() {
                "Turn repair: active turn is still running and has not crossed the repair timeout. Use `/turn-repair force` to cancel and revive it if it is truly stuck.".to_string()
            } else {
                "Turn repair: no stuck/dead turn detected.".to_string()
            }
        })
    }

    pub(crate) fn turn_repair(&mut self, force: bool) -> Option<String> {
        self.poll_stream_events();

        if let Some(handle) = self.pending_send.take() {
            if handle.is_finished() {
                self.pending_send = Some(handle);
                if self.poll_pending_send() {
                    return Some(
                        "Turn repair: finalized a completed provider worker that was still marked pending."
                            .to_string(),
                    );
                }
                return Some(
                    "Turn repair: provider worker was finished but finalization made no visible change."
                        .to_string(),
                );
            }

            let now = Instant::now();
            let started_at = self.pending_turn_started_at.unwrap_or(now);
            let last_activity_at = self.pending_turn_last_activity_at.unwrap_or(started_at);
            let age = now.saturating_duration_since(started_at);
            let idle = now.saturating_duration_since(last_activity_at);
            let timeout = turn_repair_idle_timeout();
            if force || (age >= timeout && idle >= timeout) {
                if let Some(cancel_token) = &self.pending_cancel {
                    cancel_token.store(true, Ordering::SeqCst);
                }
                drop(handle);
                self.clear_pending_turn_runtime_handles();
                self.session.status = "ready".to_string();
                self.session.activity.clear();
                self.pop_empty_assistant_placeholder();
                self.finish_active_tool_tasks_without_tool_end(crate::tasks::TaskState::Cancelled);
                let reason = if force {
                    "turn_repair was forced by the operator while a provider worker was still in-flight".to_string()
                } else {
                    format!(
                        "turn_repair detected an in-flight provider worker with no stream/tool activity for {}s (turn age {}s)",
                        idle.as_secs(),
                        age.as_secs()
                    )
                };
                self.push_turn_failure_summary(format!(
                    "{reason}. The in-flight worker was detached/cancelled, partial output and recent tool context were preserved, and the UI was returned to ready so the next turn can retry or continue."
                ));
                self.finish_turn_repair_housekeeping(&reason);
                return Some(format!(
                    "Turn repair: revived stuck in-flight turn. {reason}."
                ));
            }

            self.pending_send = Some(handle);
            return None;
        }

        let mut reasons = Vec::new();
        if self.pending_stream.is_some() {
            self.pending_stream = None;
            reasons.push("removed a stranded stream receiver".to_string());
        }
        if self.pending_cancel.is_some() {
            self.pending_cancel = None;
            reasons.push("removed a stranded cancel token".to_string());
        }
        if self.pending_steering.is_some() {
            self.pending_steering = None;
            reasons.push("removed a stranded steering channel".to_string());
        }
        self.pending_turn_started_at = None;
        self.pending_turn_last_activity_at = None;

        if self.session.status == "streaming" {
            self.session.status = "ready".to_string();
            self.session.activity.clear();
            reasons.push(
                "session status was streaming without an in-flight provider worker".to_string(),
            );
        }

        if reasons.is_empty() {
            return None;
        }

        let should_summarize = self.has_repairable_turn_artifact();
        self.pop_empty_assistant_placeholder();
        let reason = reasons.join("; ");
        if should_summarize {
            self.push_turn_failure_summary(format!(
                "turn_repair detected a dead turn: {reason}. The stale runtime handles were cleared and the UI was returned to ready so the next turn can retry or continue."
            ));
        }
        self.finish_turn_repair_housekeeping(&reason);
        Some(format!("Turn repair: revived dead turn ({reason})."))
    }

    fn finish_active_tool_tasks_without_tool_end(&mut self, state: crate::tasks::TaskState) {
        let active = std::mem::take(&mut self.active_tool_tasks);
        for (tool_name, task_id) in active {
            let outcome = match state {
                crate::tasks::TaskState::Cancelled => self.task_manager.cancel(&task_id),
                crate::tasks::TaskState::TimedOut => self.task_manager.timeout(&task_id),
                _ => self.task_manager.complete(&task_id, 1),
            };
            if let Err(error) = outcome {
                self.push_system_message(format!(
                    "Warning: failed to finish task {task_id} for interrupted tool `{tool_name}`: {error}"
                ));
            }
        }
    }

    fn clear_pending_turn_runtime_handles(&mut self) {
        self.pending_stream = None;
        self.pending_cancel = None;
        self.pending_steering = None;
        self.pending_turn_started_at = None;
        self.pending_turn_last_activity_at = None;
        self.pending_assistant_paragraph_break = false;
    }

    pub(crate) fn start_tui_turn_artifact(&mut self, prompt: &str) {
        match RunArtifactManager::start_in(
            &self.cwd,
            &self.data_root,
            None::<std::path::PathBuf>,
            self.session.session_id.clone(),
            self.session.current_provider.clone(),
            self.session.current_model.clone(),
            self.session.active_agent_id.clone(),
        ) {
            Ok((manager, manifest)) => {
                if let Err(error) = manager.write_request(&json!({
                    "goal": prompt,
                    "mode": "tui_turn",
                    "autonomous_mode_enabled": self.autonomous_mode_enabled,
                    "autonomous_level": self.autonomous_level,
                    "dangerously_bypass_approvals_and_sandbox": self.dangerously_bypass_approvals_and_sandbox,
                })) {
                    self.push_system_message(format!(
                        "Warning: failed to write run artifact request: {error}"
                    ));
                }
                if let Err(error) = manager.write_approvals_from_pending(
                    &self.tool_executor.guardrails.approvals.pending(),
                ) {
                    self.push_system_message(format!(
                        "Warning: failed to write run artifact approvals: {error}"
                    ));
                }
                if let Err(error) = manager.append_runtime_event(
                    crate::events::VegvisirEvent::UserMessage(crate::events::UserMessage {
                        message_id: format!("{}:user", manager.run_id),
                        content_preview: prompt.chars().take(512).collect(),
                        attachment_count: self.session.pending_attachments.len() as u32,
                    }),
                ) {
                    self.push_system_message(format!(
                        "Warning: failed to append run artifact user event: {error}"
                    ));
                }
                self.pending_run_artifact = Some((manager, manifest));
            }
            Err(error) => self.push_system_message(format!(
                "Warning: failed to start run artifact bundle: {error}"
            )),
        }
    }

    fn write_tui_turn_context_artifacts(
        &mut self,
        envelope: &cms_v2::prompt_cache::CachedPromptEnvelope,
    ) {
        if let Some((manager, _)) = self.pending_run_artifact.as_ref()
            && let Err(error) = manager.write_context_artifacts(envelope)
        {
            self.push_system_message(format!(
                "Warning: failed to write run artifact context: {error}"
            ));
        }
    }

    pub(crate) fn register_approval_control_request(
        &mut self,
        request: crate::control_requests::ControlRequest<
            crate::control_requests::ApprovalControlPayload,
        >,
    ) {
        let json_request = match serde_json::to_value(&request.payload) {
            Ok(payload) => request.clone().map_payload(|_| payload),
            Err(error) => {
                self.push_system_message(format!(
                    "Warning: failed to serialize approval control request {}: {error}",
                    request.request_id
                ));
                return;
            }
        };
        let _ = self
            .control_requests
            .insert(json_request, chrono::Utc::now());
        self.append_tui_turn_provider_event(&ProviderRunEvent::ApprovalRequired { request });
    }

    pub(crate) fn apply_approval_control_response(
        &mut self,
        response: ControlResponse<ApprovalControlDecision>,
    ) -> ApprovalControlApplication {
        let request_id = response.request_id.clone();
        let decision_source = response.decision_source.clone();
        let decision = response.payload.decision.clone();
        let approval_id = approval_id_from_control_request_id(&request_id)
            .unwrap_or_else(|| request_id.trim_start_matches("ctrl_").to_string());
        let json_response = match serde_json::to_value(&response.payload) {
            Ok(payload) => ControlResponse {
                request_id: request_id.clone(),
                decision_source: decision_source.clone(),
                payload,
            },
            Err(error) => {
                return ApprovalControlApplication::not_applied(
                    approval_id,
                    request_id,
                    decision,
                    decision_source,
                    format!("Failed to serialize approval control response: {error}"),
                );
            }
        };

        match self
            .control_requests
            .resolve(json_response, chrono::Utc::now())
        {
            crate::control_requests::ControlResolveOutcome::Applied { .. }
            | crate::control_requests::ControlResolveOutcome::UnknownRequest { .. } => {}
            crate::control_requests::ControlResolveOutcome::DuplicateIgnored {
                existing_status,
                ..
            } => {
                return ApprovalControlApplication::not_applied(
                    approval_id,
                    request_id,
                    decision,
                    decision_source,
                    format!("Control request was already terminal: {existing_status:?}"),
                );
            }
            crate::control_requests::ControlResolveOutcome::TimedOut { .. } => {
                self.append_tui_turn_runtime_event(
                    crate::events::VegvisirEvent::ControlRequestCancelled(
                        crate::events::ControlRequestCancelled {
                            request_id: request_id.clone(),
                            subtype: crate::control_requests::CONTROL_SUBTYPE_APPROVAL.to_string(),
                            reason: "control request timed out".to_string(),
                        },
                    ),
                );
                return ApprovalControlApplication::not_applied(
                    approval_id,
                    request_id,
                    decision,
                    decision_source,
                    "Control request timed out before response could be applied.",
                );
            }
        }

        let approval_id = match response.payload.edited_args.clone() {
            Some(edited_args)
                if matches!(
                    decision,
                    ApprovalControlDecisionKind::AllowOnce
                        | ApprovalControlDecisionKind::AllowForSession
                ) =>
            {
                match self
                    .tool_executor
                    .guardrails
                    .approvals
                    .edit(&approval_id, edited_args)
                {
                    Some(edited) => edited.id,
                    None => {
                        return ApprovalControlApplication::not_applied(
                            approval_id,
                            request_id,
                            decision,
                            decision_source,
                            "Unknown pending approval for edited control response.",
                        );
                    }
                }
            }
            _ => approval_id,
        };

        match decision {
            ApprovalControlDecisionKind::AllowOnce => {
                self.apply_approval_allow_once(approval_id, request_id, decision_source)
            }
            ApprovalControlDecisionKind::AllowForSession => {
                self.apply_approval_allow_for_session(approval_id, request_id, decision_source)
            }
            ApprovalControlDecisionKind::Deny => {
                self.apply_approval_denial(approval_id, request_id, decision_source)
            }
            ApprovalControlDecisionKind::Cancel => {
                self.cancel_approval_control_request(
                    &approval_id,
                    "approval control response cancelled",
                );
                ApprovalControlApplication::not_applied(
                    approval_id,
                    request_id,
                    ApprovalControlDecisionKind::Cancel,
                    decision_source,
                    "Approval control response cancelled.",
                )
            }
        }
    }

    pub(crate) fn apply_approval_control_decision(
        &mut self,
        approval_id: &str,
        decision_source: &str,
        decision: ApprovalControlDecisionKind,
    ) -> ApprovalControlApplication {
        let response = ControlResponse {
            request_id: format!("ctrl_{approval_id}"),
            decision_source: decision_source.to_string(),
            payload: ApprovalControlDecision {
                decision,
                edited_args: None,
            },
        };
        self.apply_approval_control_response(response)
    }

    fn apply_approval_allow_once(
        &mut self,
        approval_id: String,
        request_id: String,
        decision_source: String,
    ) -> ApprovalControlApplication {
        match self
            .tool_executor
            .guardrails
            .approvals
            .approve_once_request(&approval_id)
        {
            Some(approval) => {
                self.record_approval_control_resolution(
                    &approval.id,
                    &request_id,
                    &decision_source,
                    crate::events::ApprovalDecision::Allow,
                );
                ApprovalControlApplication {
                    approval_id: approval.id.clone(),
                    request_id,
                    decision: ApprovalControlDecisionKind::AllowOnce,
                    decision_source,
                    approval: Some(approval),
                    applied: true,
                    message: "Approved once".to_string(),
                }
            }
            None => ApprovalControlApplication::not_applied(
                approval_id,
                request_id,
                ApprovalControlDecisionKind::AllowOnce,
                decision_source,
                "Unknown pending approval.",
            ),
        }
    }

    fn apply_approval_allow_for_session(
        &mut self,
        approval_id: String,
        request_id: String,
        decision_source: String,
    ) -> ApprovalControlApplication {
        match self
            .tool_executor
            .guardrails
            .approvals
            .approve_for_session(&approval_id)
        {
            Some(approval) => {
                let mut message = "Approved matching call for this running session".to_string();
                if approval.risk_label == "command-allow"
                    && let Some(command) = command_name_from_args(&approval.args)
                {
                    self.tool_executor
                        .guardrails
                        .policy
                        .allowed_commands
                        .insert(command.clone());
                    message = format!(
                        "Approved matching call and allowed shell command `{command}` for this running session"
                    );
                }
                self.record_approval_control_resolution(
                    &approval.id,
                    &request_id,
                    &decision_source,
                    crate::events::ApprovalDecision::AllowForSession,
                );
                ApprovalControlApplication {
                    approval_id: approval.id.clone(),
                    request_id,
                    decision: ApprovalControlDecisionKind::AllowForSession,
                    decision_source,
                    approval: Some(approval),
                    applied: true,
                    message,
                }
            }
            None => ApprovalControlApplication::not_applied(
                approval_id,
                request_id,
                ApprovalControlDecisionKind::AllowForSession,
                decision_source,
                "Unknown pending approval.",
            ),
        }
    }

    fn apply_approval_denial(
        &mut self,
        approval_id: String,
        request_id: String,
        decision_source: String,
    ) -> ApprovalControlApplication {
        if self.tool_executor.guardrails.approvals.deny(&approval_id) {
            self.record_approval_control_resolution(
                &approval_id,
                &request_id,
                &decision_source,
                crate::events::ApprovalDecision::Deny,
            );
            ApprovalControlApplication {
                approval_id,
                request_id,
                decision: ApprovalControlDecisionKind::Deny,
                decision_source,
                approval: None,
                applied: true,
                message: "Denied approval".to_string(),
            }
        } else {
            ApprovalControlApplication::not_applied(
                approval_id,
                request_id,
                ApprovalControlDecisionKind::Deny,
                decision_source,
                "Unknown pending approval.",
            )
        }
    }

    fn record_approval_control_resolution(
        &mut self,
        approval_id: &str,
        request_id: &str,
        decision_source: &str,
        decision: crate::events::ApprovalDecision,
    ) {
        self.append_tui_turn_runtime_event(crate::events::VegvisirEvent::ControlRequestResolved(
            crate::events::ControlRequestResolved {
                request_id: request_id.to_string(),
                subtype: crate::control_requests::CONTROL_SUBTYPE_APPROVAL.to_string(),
                decision_source: decision_source.to_string(),
            },
        ));
        self.append_tui_turn_runtime_event(crate::events::VegvisirEvent::ApprovalResolved(
            crate::events::ApprovalResolved {
                approval_id: approval_id.to_string(),
                decision,
            },
        ));
    }

    pub(crate) fn cancel_approval_control_request(&mut self, approval_id: &str, reason: &str) {
        let request_id = format!("ctrl_{approval_id}");
        let _ = self
            .control_requests
            .cancel(&request_id, reason.to_string(), chrono::Utc::now());
        self.append_tui_turn_runtime_event(crate::events::VegvisirEvent::ControlRequestCancelled(
            crate::events::ControlRequestCancelled {
                request_id,
                subtype: crate::control_requests::CONTROL_SUBTYPE_APPROVAL.to_string(),
                reason: reason.to_string(),
            },
        ));
    }

    fn append_tui_turn_provider_event(&mut self, event: &ProviderRunEvent) {
        if let Some((manager, _)) = self.pending_run_artifact.as_ref()
            && let Err(error) = manager.append_observed_provider_event(event)
        {
            self.push_system_message(format!(
                "Warning: failed to append run artifact event: {error}"
            ));
        }
    }

    fn append_tui_turn_runtime_event(&mut self, event: crate::events::VegvisirEvent) {
        if let Some((manager, _)) = self.pending_run_artifact.as_ref()
            && let Err(error) = manager.append_runtime_event(event)
        {
            self.push_system_message(format!(
                "Warning: failed to append run runtime event: {error}"
            ));
        }
    }

    fn register_task_for_tool_start(&mut self, tool_name: &str, args: &str) {
        let Some(kind) = task_kind_for_tool(tool_name) else {
            return;
        };
        if self.active_tool_tasks.contains_key(tool_name) {
            return;
        }
        let owner_run_id = self
            .pending_run_artifact
            .as_ref()
            .map(|(manager, _)| manager.run_id.clone())
            .unwrap_or_else(|| self.session.session_id.clone());
        let command = command_string_from_tool_args(args);
        let description = command
            .as_deref()
            .map(|command| format!("{tool_name}: {command}"))
            .unwrap_or_else(|| tool_name.to_string());
        let mut request = TaskSpawnRequest::new(kind, description, self.cwd.clone(), owner_run_id);
        if let Some(command) = command {
            request = request.command(command);
        }
        if let Some(agent_id) = &self.session.active_agent_id {
            request = request.owner_agent_id(agent_id.clone());
        }
        let task_id = self.task_manager.register(request);
        if let Err(error) = self.task_manager.start_foreground(&task_id) {
            self.push_system_message(format!(
                "Warning: failed to mark task {task_id} as foreground: {error}"
            ));
        }
        self.active_tool_tasks
            .insert(tool_name.to_string(), task_id.clone());
        self.push_system_message(format!("Task registered for tool `{tool_name}`: {task_id}"));
    }

    fn complete_task_for_tool_end(
        &mut self,
        tool_name: &str,
        ok: bool,
        summary: &str,
        detail: Option<&str>,
    ) {
        let Some(task_id) = self.active_tool_tasks.remove(tool_name) else {
            return;
        };
        let mut output = String::new();
        output.push_str(summary.trim_end());
        if let Some(detail) = detail.map(str::trim).filter(|detail| !detail.is_empty()) {
            if !output.is_empty() {
                output.push_str("\n\n");
            }
            output.push_str(detail);
        }
        if !output.is_empty() {
            if let Err(error) = self.persist_task_output(&task_id, &output) {
                self.push_system_message(format!(
                    "Warning: failed to persist task output for {task_id}: {error}"
                ));
            }
            if let Err(error) = self.task_manager.append_output(&task_id, &output) {
                self.push_system_message(format!(
                    "Warning: failed to append task output for {task_id}: {error}"
                ));
            }
        }
        let exit_code = if ok { 0 } else { 1 };
        if let Err(error) = self.task_manager.complete(&task_id, exit_code) {
            self.push_system_message(format!(
                "Warning: failed to complete task {task_id}: {error}"
            ));
        }
    }

    fn persist_task_output(&self, task_id: &str, output: &str) -> anyhow::Result<()> {
        let Some(record) = self.task_manager.record(task_id) else {
            return Ok(());
        };
        if let Some(parent) = record.output_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&record.output_file, output)?;
        Ok(())
    }

    pub(crate) fn poll_task_runner(&mut self) -> bool {
        let events = self.task_runner.poll();
        if events.is_empty() {
            return false;
        }
        let mut changed = false;
        for event in events {
            match event {
                TaskRunnerEvent::Output { task_id, chunk } => {
                    if let Err(error) = self.task_manager.append_output(&task_id, &chunk) {
                        self.push_system_message(format!(
                            "Warning: failed to append task runner output for {task_id}: {error}"
                        ));
                    }
                    changed = true;
                }
                TaskRunnerEvent::Completed { task_id, exit_code } => {
                    if let Err(error) = self.task_manager.complete(&task_id, exit_code) {
                        self.push_system_message(format!(
                            "Warning: failed to complete task runner task {task_id}: {error}"
                        ));
                    } else {
                        self.push_system_message(format!(
                            "Task {task_id} completed with exit_code={exit_code}. Use /tasks show {task_id} or /tasks tail {task_id}."
                        ));
                    }
                    changed = true;
                }
                TaskRunnerEvent::Cancelled { task_id } => {
                    if let Err(error) = self.task_manager.cancel(&task_id) {
                        self.push_system_message(format!(
                            "Warning: failed to cancel task runner task {task_id}: {error}"
                        ));
                    } else {
                        self.push_system_message(format!(
                            "Task {task_id} cancelled. Use /tasks show {task_id} for details."
                        ));
                    }
                    changed = true;
                }
                TaskRunnerEvent::TimedOut { task_id } => {
                    if self
                        .task_manager
                        .record(&task_id)
                        .is_some_and(|record| !record.is_terminal())
                    {
                        if let Err(error) = self.task_manager.timeout(&task_id) {
                            self.push_system_message(format!(
                                "Warning: failed to timeout task runner task {task_id}: {error}"
                            ));
                        } else {
                            self.push_system_message(format!(
                                "Task {task_id} timed out. Use /tasks show {task_id} or /tasks tail {task_id}."
                            ));
                        }
                    }
                    changed = true;
                }
                TaskRunnerEvent::Failed { task_id, error } => {
                    if let Err(transition_error) = self.task_manager.complete(&task_id, 1) {
                        self.push_system_message(format!(
                            "Warning: failed to mark task runner task {task_id} failed after {error}: {transition_error}"
                        ));
                    } else {
                        self.push_system_message(format!(
                            "Task {task_id} failed: {error}. Use /tasks show {task_id} or /tasks tail {task_id}."
                        ));
                    }
                    changed = true;
                }
            }
        }
        self.drain_task_lifecycle_events_to_run_artifact();
        self.autosave_session();
        self.redraw_requested = true;
        changed
    }

    pub(crate) fn spawn_background_shell_task(
        &mut self,
        command: Vec<String>,
        timeout_seconds: u64,
        stall_timeout_seconds: Option<u64>,
    ) -> anyhow::Result<String> {
        self.authorize_background_shell_command(&command)?;
        let owner_run_id = self
            .pending_run_artifact
            .as_ref()
            .map(|(manager, _)| manager.run_id.clone())
            .unwrap_or_else(|| self.session.session_id.clone());
        let mut request = TaskRunRequest::shell(command, self.cwd.clone(), owner_run_id)
            .timeout(Duration::from_secs(timeout_seconds.clamp(1, 86_400)))
            .stall_timeout(
                stall_timeout_seconds.map(|seconds| Duration::from_secs(seconds.max(1))),
            );
        if let Some(agent_id) = &self.session.active_agent_id {
            request = request.owner_agent_id(agent_id.clone());
        }
        let sandbox_config = crate::command_sandbox::CommandSandboxConfig::from_env(
            self.cwd.clone(),
            self.dangerously_bypass_approvals_and_sandbox,
        )?;
        let task_id =
            self.task_runner
                .spawn_background(&mut self.task_manager, request, &sandbox_config)?;
        self.push_system_message(format!(
            "Spawned background shell task {task_id}. Use /tasks show {task_id}, /tasks tail {task_id}, or /tasks cancel {task_id}."
        ));
        self.drain_task_lifecycle_events_to_run_artifact();
        self.redraw_requested = true;
        Ok(task_id)
    }

    pub(crate) fn cancel_background_task(&mut self, task_id: &str) -> anyhow::Result<()> {
        self.task_runner.cancel(task_id)?;
        self.push_system_message(format!("Cancellation requested for task {task_id}."));
        self.redraw_requested = true;
        Ok(())
    }

    fn authorize_background_shell_command(&mut self, command: &[String]) -> anyhow::Result<()> {
        if command.is_empty() {
            anyhow::bail!("Empty task command");
        }
        let args = serde_json::json!({"command": command})
            .as_object()
            .cloned()
            .expect("object literal");
        let tool = self.tool_executor.registry.get("run_command")?.clone();
        self.tool_executor.guardrails.authorize_tool(&tool, &args)?;
        if !self
            .tool_executor
            .guardrails
            .policy
            .bypass_approvals_and_sandbox
        {
            self.tool_executor
                .runtime_policy
                .authorize_tool_with_metadata(
                    "run_command",
                    &args,
                    RuntimeToolMetadata {
                        risky: tool.risky,
                        safety_labels: Vec::new(),
                    },
                    &self.logger,
                )
                .map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }

    pub(crate) fn drain_task_lifecycle_events_to_run_artifact(&mut self) {
        let Some(manager) = self
            .pending_run_artifact
            .as_ref()
            .map(|(manager, _)| manager.clone())
        else {
            return;
        };
        let events = self.task_manager.drain_events();
        for event in events {
            let task_id = event.task_id().to_string();
            let Some(record) = self.task_manager.record(&task_id) else {
                continue;
            };
            let Some(runtime_event) = event.to_vegvisir_event(record) else {
                continue;
            };
            if let Err(error) = manager.append_runtime_event(runtime_event) {
                self.push_system_message(format!(
                    "Warning: failed to append task runtime event: {error}"
                ));
            }
        }
    }

    fn finish_tui_turn_artifact(&mut self, status: RunStatus, response: Option<&str>) {
        self.drain_task_lifecycle_events_to_run_artifact();
        let Some((manager, mut manifest)) = self.pending_run_artifact.take() else {
            return;
        };
        if let Some(response) = response {
            if let Err(error) = manager.append_runtime_event(
                crate::events::VegvisirEvent::AssistantMessageCompleted(
                    crate::events::AssistantMessageCompleted {
                        message_id: format!("{}:assistant", manager.run_id),
                        output_tokens: None,
                    },
                ),
            ) {
                self.push_system_message(format!(
                    "Warning: failed to append run assistant completion event: {error}"
                ));
            }
            if let Err(error) = manager.write_result(response) {
                self.push_system_message(format!(
                    "Warning: failed to write run artifact result: {error}"
                ));
            }
        }
        if let Err(error) =
            manager.write_approvals_from_pending(&self.tool_executor.guardrails.approvals.pending())
        {
            self.push_system_message(format!(
                "Warning: failed to write run artifact approvals: {error}"
            ));
        }
        if let Err(error) = manager.write_subagents_from_board() {
            self.push_system_message(format!(
                "Warning: failed to write run artifact subagents: {error}"
            ));
        }
        if let Err(error) = manager.write_workspace_change_artifacts() {
            self.push_system_message(format!(
                "Warning: failed to write run artifact workspace changes: {error}"
            ));
        }
        if let Err(error) = manager.finish(&mut manifest, status) {
            self.push_system_message(format!(
                "Warning: failed to finalize run artifact manifest: {error}"
            ));
        }
    }

    fn fail_tui_turn_artifact(
        &mut self,
        message: &str,
        recoverable: bool,
    ) -> Option<(String, std::path::PathBuf)> {
        self.drain_task_lifecycle_events_to_run_artifact();
        let (manager, mut manifest) = self.pending_run_artifact.take()?;
        let failed_run = Some((manager.run_id.clone(), manager.run_dir.clone()));
        let failure = RunFailure {
            schema_version: crate::run_artifacts::RUN_ARTIFACT_SCHEMA_VERSION,
            run_id: manager.run_id.clone(),
            message: message.to_string(),
            recoverable,
            timestamp: chrono::Utc::now(),
        };
        if let Err(error) = manager.write_failure(&failure) {
            self.push_system_message(format!(
                "Warning: failed to write run artifact failure: {error}"
            ));
        }
        if let Err(error) = manager.write_memory_written_unavailable(
            "run failed before completion memory writeback was captured",
        ) {
            self.push_system_message(format!(
                "Warning: failed to write run artifact memory-write status: {error}"
            ));
        }
        if let Err(error) =
            manager.write_approvals_from_pending(&self.tool_executor.guardrails.approvals.pending())
        {
            self.push_system_message(format!(
                "Warning: failed to write run artifact approvals: {error}"
            ));
        }
        if let Err(error) = manager.write_subagents_from_board() {
            self.push_system_message(format!(
                "Warning: failed to write run artifact subagents: {error}"
            ));
        }
        if let Err(error) = manager.write_workspace_change_artifacts() {
            self.push_system_message(format!(
                "Warning: failed to write run artifact workspace changes: {error}"
            ));
        }
        if let Err(error) = manager.finish(&mut manifest, RunStatus::Failed) {
            self.push_system_message(format!(
                "Warning: failed to finalize failed run artifact: {error}"
            ));
        }
        failed_run
    }

    fn auto_recover_failed_turn(
        &mut self,
        reason: &str,
        error: Option<String>,
        failed_run: Option<(String, std::path::PathBuf)>,
    ) {
        let mut message = format!(
            "Automatic turn recovery fired: {reason}. Vegvisir cleared the failed worker state, preserved the exact failure context above, and returned the UI to ready so you can retry or continue."
        );
        if let Some((run_id, run_dir)) = failed_run {
            message.push_str(&format!(
                "\n\nRecoverable failed run artifact: `{run_id}` at {}. Use `/recover last` or `/runs replay-plan {run_id}` for replay guidance before retrying.",
                run_dir.display()
            ));
        } else {
            message.push_str(
                "\n\nNo run artifact was active for this failure; use the preserved transcript context above before retrying.",
            );
        }
        if let Some(error) = error.filter(|error| !error.trim().is_empty()) {
            message.push_str("\n\nRecovered error:\n```text\n");
            message.push_str(error.trim());
            message.push_str("\n```");
        }
        self.push_system_message(message);
        self.finish_turn_repair_housekeeping(reason);
    }

    fn finish_turn_repair_housekeeping(&mut self, reason: &str) {
        if self.autonomy.active {
            self.autonomy.active = false;
            self.autonomy.enabled = false;
            self.autonomy.last_status = format!("failed: turn repair: {reason}");
        }
        self.goal_cancelled(&format!("failed: turn repair: {reason}"));
        self.autosave_session();
        self.chat_scroll_offset = 0;
        self.redraw_requested = true;
        self.logger.emit(
            "turn_repaired",
            json!({
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
                "reason": reason,
            }),
        );
    }

    fn has_repairable_turn_artifact(&self) -> bool {
        let Some(last_user_index) = self
            .session
            .messages
            .iter()
            .rposition(|message| message.role == "user")
        else {
            return self
                .session
                .messages
                .last()
                .is_some_and(|message| message.role == "assistant");
        };
        self.session.messages[last_user_index + 1..]
            .iter()
            .any(|message| {
                message.role == "assistant"
                    || (message.role == "system" && is_live_tool_message(&message.content))
            })
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
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
            }
            reached_frame_budget = events.len() == MAX_STREAM_EVENTS_PER_POLL;
        }
        if events.is_empty() {
            return;
        }
        self.pending_turn_last_activity_at = Some(Instant::now());
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
                    if self.pending_assistant_paragraph_break && !delta.trim().is_empty() {
                        ensure_assistant_delta_separator(
                            &mut self.session.messages[assistant_index].content,
                            &delta,
                        );
                        self.pending_assistant_paragraph_break = false;
                    }
                    self.session.messages[assistant_index]
                        .content
                        .push_str(&delta);
                    self.append_tui_turn_runtime_event(
                        crate::events::VegvisirEvent::AssistantDelta(
                            crate::events::AssistantDelta {
                                message_id: self
                                    .pending_run_artifact
                                    .as_ref()
                                    .map(|(manager, _)| format!("{}:assistant", manager.run_id))
                                    .unwrap_or_else(|| "assistant".to_string()),
                                delta,
                            },
                        ),
                    );
                }
                StreamEvent::PromptEnvelope(envelope) => {
                    self.write_tui_turn_context_artifacts(&envelope);
                }
                StreamEvent::Activity(activity) => {
                    self.append_tui_turn_provider_event(&ProviderRunEvent::Activity(
                        activity.clone(),
                    ));
                    self.session.activity = activity;
                    self.pending_assistant_paragraph_break = true;
                }
                StreamEvent::ApprovalRequired { request } => {
                    self.register_approval_control_request(request);
                }
                StreamEvent::ToolStart { name, args } => {
                    self.append_tui_turn_provider_event(&ProviderRunEvent::ToolStart {
                        name: name.clone(),
                        args: args.clone(),
                    });
                    self.register_task_for_tool_start(&name, &args);
                    self.session.activity = format!("using tool {name}");
                    self.pending_assistant_paragraph_break = true;
                    self.push_live_tool_message(format!("Running tool: {name} {args}"));
                }
                StreamEvent::ToolOutput {
                    name,
                    stream,
                    chunk,
                    truncated,
                } => {
                    self.append_tui_turn_provider_event(&ProviderRunEvent::ToolOutput {
                        name: name.clone(),
                        stream: stream.clone(),
                        chunk: chunk.clone(),
                        truncated,
                    });
                    let suffix = if truncated { " [live truncated]" } else { "" };
                    self.session.activity = format!("{name} {stream} output{suffix}");
                    self.pending_assistant_paragraph_break = true;
                    self.push_live_tool_message(format!(
                        "Tool output: {name} {stream}{suffix}\n{chunk}"
                    ));
                }
                StreamEvent::ToolEnd {
                    name,
                    ok,
                    summary,
                    detail,
                } => {
                    self.append_tui_turn_provider_event(&ProviderRunEvent::ToolEnd {
                        name: name.clone(),
                        ok,
                        summary: summary.clone(),
                        detail: detail.clone(),
                    });
                    self.session.activity = format!("finished tool {name}");
                    self.pending_assistant_paragraph_break = true;
                    let status = if ok { "finished" } else { "failed" };
                    let mut content = format!("Tool {status}: {name} - {summary}");
                    let detail = detail.as_deref().filter(|detail| !detail.trim().is_empty());
                    if let Some(detail) = detail {
                        content.push_str("\n\n");
                        content.push_str(detail);
                    }
                    self.push_live_tool_message(content.clone());
                    self.complete_task_for_tool_end(&name, ok, &summary, detail);
                    if ok && let Some(detail) = detail {
                        self.push_assistant_tool_artifact(&name, detail);
                    }
                }
            }
        }
        self.session.enforce_history_limits();
        self.redraw_requested = true;
        if reached_frame_budget {
            // Leave remaining deltas for the next UI tick. This prevents a hot
            // streaming provider from monopolizing the TUI thread and starving
            // redraw/input/finalization work.
            self.session.activity_tick = self.session.activity_tick.saturating_add(1);
        }
    }

    pub(crate) fn push_turn_failure_summary(&mut self, error: String) {
        let exact_error = error.trim();
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
            "Turn failed before the model produced a normal final summary.

Exact error message:

```text
",
        );
        content.push_str(exact_error);
        content.push_str(
            "
```

Preserved recovery context:
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
Next step: retry or continue from the last successful step instead of leaving the turn silently truncated.",
        );

        self.info_scroll_offset = 0;
        self.info_overlay = Some(InfoOverlay {
            title: "turn failure".to_string(),
            body: content.clone(),
        });
        self.logger.emit(
            "turn_failure_visible",
            json!({
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
                "error": exact_error,
            }),
        );
        self.push_live_tool_message(content);
    }

    pub(crate) fn push_live_tool_message(&mut self, content: String) {
        let content = crate::core::truncate_utf8_middle(
            &content,
            crate::core::MAX_SESSION_MESSAGE_BYTES,
            "live tool message",
        );
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
            let completed_content = completed_message.content.clone();
            let completed_trimmed = completed_content.trim();
            let live_trimmed = live_content.trim();
            if completed_trimmed.is_empty() {
                completed_message.content = live_content;
            } else if let Some(artifact_prefix) = live_content.find("**Code/Diff update from `") {
                // Tool-observation code/diff artifacts are injected only into
                // the live TUI assistant message. A later final worker response
                // can be longer than the live partial, so preserve these
                // assistant-visible artifacts before applying length/subset
                // merge heuristics.
                let artifacts = live_content[artifact_prefix..].trim();
                if !artifacts.is_empty() && !completed_content.contains(artifacts) {
                    completed_message.content =
                        format!("{}\n\n{}", artifacts, completed_content.trim_start());
                }
            } else if live_trimmed.is_empty() || completed_trimmed == live_trimmed {
                // Keep the completed worker response. This is the only copy that
                // is guaranteed to include the provider's final tail.
            } else if whitespace_insensitive_eq(&live_content, &completed_content) {
                // The live TUI stream may insert presentation separators around
                // tool/activity boundaries so progress narration does not render
                // as `work.Now` wall text. If the completed worker response only
                // differs by whitespace, preserve the polished live transcript.
                completed_message.content = live_content;
            } else if completed_content.len() > live_content.len() {
                // Keep the completed worker response. This is the only copy that
                // is guaranteed to include the provider's final tail.
            } else if live_content.contains(completed_trimmed) {
                completed_message.content = live_content;
            } else if completed_content.contains(live_trimmed) {
                // The live content is only a prefix/subset of the final answer.
            } else {
                completed_message.content = format!(
                    "{}\n\n{}",
                    live_content.trim_end(),
                    completed_content.trim_start()
                );
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
            .map(|(_, lines)| lines.min(12))
            .or_else(|| {
                crossterm::terminal::size()
                    .ok()
                    .map(|(_, lines)| usize::from(lines.min(12)))
            })
            .unwrap_or(12)
            .max(4)
    }

    pub(crate) fn pulse_activity(&mut self) {
        let has_background_task = self
            .task_manager
            .active_records()
            .iter()
            .any(|record| record.state == crate::tasks::TaskState::RunningBackground);
        if self.session.status != "streaming" && !has_background_task {
            return;
        }
        self.session.activity_tick = self.session.activity_tick.saturating_add(1);
        if !self.session.activity.trim().is_empty() || has_background_task {
            self.redraw_requested = true;
        }
    }
}

fn task_kind_for_tool(tool_name: &str) -> Option<TaskKind> {
    match tool_name {
        "run_tests" => Some(TaskKind::Test),
        "run_command" => Some(TaskKind::Shell),
        _ => None,
    }
}

fn command_string_from_tool_args(args: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(args).ok()?;
    let command = value.get("command")?.as_array()?;
    let parts = command
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn subagent_transcript_signature(record: &SubAgentTaskRecord) -> String {
    format!(
        "{:?}|started={:?}|finished={:?}|checkpoint={:?}|final_len={}|error={}|observability={}",
        record.status,
        record.started_at,
        record.finished_at,
        record.checkpoint,
        record.final_answer.as_deref().map(str::len).unwrap_or(0),
        record.error.as_deref().unwrap_or(""),
        serde_json::to_string(&record.observability).unwrap_or_default()
    )
}

fn approval_id_from_control_request_id(request_id: &str) -> Option<String> {
    request_id
        .strip_prefix("ctrl_")
        .map(str::to_string)
        .filter(|id| !id.is_empty())
}

fn whitespace_insensitive_eq(left: &str, right: &str) -> bool {
    left.chars()
        .filter(|ch| !ch.is_whitespace())
        .eq(right.chars().filter(|ch| !ch.is_whitespace()))
}

fn ensure_assistant_delta_separator(content: &mut String, next_delta: &str) {
    if content.trim().is_empty() || next_delta.starts_with(char::is_whitespace) {
        return;
    }
    if content.ends_with("\n\n") {
        return;
    }
    if content.ends_with('\n') {
        content.push('\n');
    } else {
        content.push_str("\n\n");
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

fn turn_repair_idle_timeout() -> Duration {
    Duration::from_secs(
        std::env::var("VEGVISIR_TURN_REPAIR_IDLE_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(600)
            .clamp(30, 86_400),
    )
}

struct CmsWritebackJob {
    config: crate::memory::VegvisirCmsConfig,
    user_content: String,
    assistant_response: String,
    artifact_manager: Option<RunArtifactManager>,
}

fn cms_writeback_sender() -> &'static std::sync::mpsc::SyncSender<CmsWritebackJob> {
    static SENDER: OnceLock<std::sync::mpsc::SyncSender<CmsWritebackJob>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel(CMS_WRITEBACK_QUEUE_CAPACITY);
        thread::Builder::new()
            .name("vegvisir-cms-writeback".to_string())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    process_cms_complete_turn_writeback(job);
                }
            })
            .expect("failed to start CMS writeback worker");
        sender
    })
}

fn spawn_cms_complete_turn_writeback(
    config: crate::memory::VegvisirCmsConfig,
    user_content: String,
    assistant_response: String,
    artifact_manager: Option<RunArtifactManager>,
) {
    let job = CmsWritebackJob {
        config,
        user_content,
        assistant_response,
        artifact_manager,
    };
    if let Err(error) = cms_writeback_sender().try_send(job) {
        if let mpsc::TrySendError::Full(job) = error {
            if let Some(manager) = job.artifact_manager {
                let _ = manager.write_memory_written_unavailable(
                    "CMS writeback queue is full; durable writeback was skipped to keep process memory bounded",
                );
            }
        }
    }
}

fn process_cms_complete_turn_writeback(job: CmsWritebackJob) {
    let CmsWritebackJob {
        config,
        user_content,
        assistant_response,
        artifact_manager,
    } = job;
    let mut config = config;
    config.commit_writebacks = true;
    let outcome = match VegvisirCms::open(config) {
        Ok(mut cms) => cms
            .complete_turn(&user_content, &assistant_response)
            .map(|results| (results, None))
            .unwrap_or_else(|error| (Vec::new(), Some(error.to_string()))),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    if let Some(manager) = artifact_manager {
        let (results, error) = outcome;
        let _ = manager.write_memory_written_from_outcome(&results, error.as_deref());
    }
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
- For complex, multi-part, evidence-seeking work, proactively delegate bounded independent tasks with the `spawn_subagent` tool. Vegvisir defaults to three active subagents, but the operator may raise or lower the session limit with `/agents max=<n>` or `/subagents max <n>`.
- Good subagent tasks include codebase reconnaissance, focused test investigation, documentation review, compatibility checks, security review, design critique, and migration impact analysis.
- Do not spawn subagents for trivial single-step tasks where delegation would add overhead.

How to spawn subagents:
- Give each child a narrow goal, explicit workspace, low `max_steps` by default, current provider/model when useful, explicit non-overlapping `file_scope`, and a `work_budget` for non-trivial review/bug-hunting/recon tasks. Respect the active subagent session limit; default is three unless the operator changes it.
- Work budgets should specify max_steps, max_tool_calls, max_read_bytes, max_output_bytes, allowed_tools, and notes such as avoiding huge raw file reads.
- Prefer read-only/review/test-planning goals unless the user explicitly asks for parallel implementation. Parallel implementation must be partitioned by non-overlapping file scopes so agents never edit or reason as owners of the same files at the same time.
- Continue useful main-thread work while subagents run; do not idle solely because a child is running.
- Check `/subagents list` or `/subagents show <id>` before final summary when subagents were spawned.

Subagent final report contract:
- Task understood
- Files inspected
- Tools used
- Findings
- Changes made
- Verification run
- Risks/blockers

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

pub(crate) fn apply_goal_mode_contract(content: &str) -> String {
    format!(
        r#"[Vegvisir goal mode is ENABLED]

You are executing an unattended, specification-driven implementation goal.

Runtime contract:
- Read and follow the complete specification supplied by the user; do not reduce it to a fixed number of steps or a single response.
- Plan the entire specification before implementation, then execute the plan end to end.
- Continue inspecting, implementing, testing, fixing, and verifying until every explicit exit/acceptance criterion is satisfied.
- Use the goal controller's Markdown plan, checklist, and evidence requirements as the completion gate. Never claim completion early.
- Continue automatically between model turns. A normal turn ending is not goal completion.
- Preserve unrelated work and stay within the active workspace, tool, approval, sandbox, secret, and user-authority boundaries.
- Pause for required approvals or blockers, never request plaintext secrets, and honor cancellation immediately.
- When all criteria are truly met, provide a concise final report with changed files, verification, and remaining risks.

Goal-mode turn:
{content}"#
    )
}

pub(crate) fn apply_autonomous_mode_contract(content: &str, level: u8) -> String {
    let level = level.min(6);
    let description = autonomy_level_runtime_description(level);
    format!(
        r#"[Vegvisir autonomous working mode is ENABLED]
Autonomy level: {level} - {description}

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
- End with a concise completion report: changed files, tests/checks run, unresolved risks, and exact next steps if blocked.

User task:
{content}"#
    )
}

fn autonomy_level_runtime_description(level: u8) -> &'static str {
    match level {
        0 => "off; interactive only",
        1 => "assist; plan and ask before execution-heavy work",
        2 => "supervised execution; run safe reads/edits/checks inside scope",
        3 => {
            "bounded implementation; pursue requested change until blocked or verification complete"
        }
        4 => "extended implementation; use subagents and broader verification when useful",
        5 => "high-autonomy project work; aggressive but approval-bound execution",
        6 => "maximum local autonomy; still approval-, secret-, sandbox-, and workspace-bound",
        _ => "maximum local autonomy; still approval-, secret-, sandbox-, and workspace-bound",
    }
}
