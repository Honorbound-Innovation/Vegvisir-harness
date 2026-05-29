use super::*;

impl TuiApplication {
    pub fn handle_submit(&mut self) {
        let raw = self.input.buffer.trim().to_string();
        if raw.is_empty() {
            self.input.clear();
            self.input.update_suggestions(Vec::new());
            return;
        }
        if !raw.starts_with('/') {
            self.input.push_history(raw.clone());
            self.session.input_history = self.input.history.clone();
        } else {
            self.input.reset_history_navigation();
        }
        self.input.clear();
        self.input.update_suggestions(Vec::new());

        if raw.starts_with('/') {
            match self.execute_command(&raw) {
                Ok(Some(response)) if !response.is_empty() => {
                    self.push_system_message(response);
                    self.autosave_session();
                }
                Ok(_) => {
                    self.autosave_session();
                }
                Err(error) => {
                    self.session.status = "ready".to_string();
                    self.session.activity.clear();
                    self.push_system_message(format!("Command failed: {error}"));
                    self.autosave_session();
                }
            }
            return;
        }

        let (mut content, mut attachments) = extract_attachments(&raw, &self.cwd);
        let pending = std::mem::take(&mut self.session.pending_attachments);
        attachments = pending.into_iter().chain(attachments).collect();
        if content.trim().is_empty() && !attachments.is_empty() {
            content = "Please review the attached file(s).".to_string();
        }

        if attachments.is_empty()
            && let Some(response) = self.try_handle_natural_agent_template_request(&content)
        {
            self.session.messages.push(ChatMessage {
                role: "user".to_string(),
                content,
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            });
            self.push_system_message(response);
            self.autosave_session();
            self.chat_scroll_offset = 0;
            self.redraw_requested = true;
            return;
        }

        self.start_background_send(content, attachments);
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) {
        if should_refresh_suggestions_before_key(&key) {
            let suggestions = self.build_suggestions();
            self.input.update_suggestions(suggestions);
        }
        if self.handle_search_key(key) {
            self.redraw_requested = true;
            return;
        }
        if self.handle_pending_approval_key(key) {
            self.redraw_requested = true;
            return;
        }
        if self.help_overlay_open {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => self.help_overlay_open = false,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.handle_ctrl_c();
                }
                _ => {}
            }
            self.redraw_requested = true;
            return;
        }
        if self.diff_overlay.is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.diff_overlay = None;
                    self.diff_scroll_offset = 0;
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.handle_ctrl_c();
                }
                KeyCode::PageUp => {
                    self.diff_scroll_offset = self
                        .diff_scroll_offset
                        .saturating_add(self.chat_page_size());
                }
                KeyCode::PageDown => {
                    self.diff_scroll_offset = self
                        .diff_scroll_offset
                        .saturating_sub(self.chat_page_size());
                }
                KeyCode::Home => self.diff_scroll_offset = usize::MAX / 2,
                KeyCode::End => self.diff_scroll_offset = 0,
                _ => {}
            }
            self.redraw_requested = true;
            return;
        }
        if self.info_overlay.is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.info_overlay = None;
                    self.info_scroll_offset = 0;
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.handle_ctrl_c();
                }
                KeyCode::PageUp => {
                    self.info_scroll_offset = self
                        .info_scroll_offset
                        .saturating_add(self.chat_page_size());
                }
                KeyCode::PageDown => {
                    self.info_scroll_offset = self
                        .info_scroll_offset
                        .saturating_sub(self.chat_page_size());
                }
                KeyCode::Home => self.info_scroll_offset = usize::MAX / 2,
                KeyCode::End => self.info_scroll_offset = 0,
                _ => {}
            }
            self.redraw_requested = true;
            return;
        }
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.handle_ctrl_c();
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.open_command_palette();
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.open_search();
            }
            KeyCode::Char('?') if key.modifiers.is_empty() && self.input.buffer.is_empty() => {
                self.help_overlay_open = true;
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.input.append_text("\n", false);
            }
            KeyCode::Enter => {
                if self.command_palette_open {
                    self.accept_palette_selection_for_execution();
                    self.command_palette_open = false;
                    self.handle_submit();
                } else if self.should_execute_selected_slash_suggestion() {
                    self.accept_palette_selection_for_execution();
                    self.handle_submit();
                } else {
                    self.handle_submit();
                }
            }
            KeyCode::Tab => {
                self.input.accept_suggestion();
            }
            KeyCode::Esc => {
                if self.command_palette_open || self.input.buffer == "/" {
                    self.input.clear();
                }
                self.command_palette_open = false;
                self.input.update_suggestions(Vec::new());
            }
            KeyCode::Backspace => {
                self.input.backspace();
            }
            KeyCode::Up => {
                if !self.input.move_selection(-1) {
                    let input_width = self.input_edit_width();
                    if self.input.cursor == 0 {
                        self.input.history_move(-1);
                    } else if self.input.visual_line_count(input_width) > 1 {
                        self.input.move_cursor_vertical(-1, input_width);
                    }
                }
            }
            KeyCode::Down => {
                if !self.input.move_selection(1) {
                    let input_width = self.input_edit_width();
                    if self.input.cursor == 0 {
                        self.input.history_move(1);
                    } else if self.input.visual_line_count(input_width) > 1 {
                        self.input.move_cursor_vertical(1, input_width);
                    }
                }
            }
            KeyCode::Left => {
                self.input.move_cursor(-1);
            }
            KeyCode::Right => {
                self.input.move_cursor(1);
            }
            KeyCode::PageUp => {
                if self.command_palette_open {
                    self.input
                        .move_selection_by_page(-(self.command_palette_page_size() as isize));
                } else {
                    self.chat_scroll_offset = self
                        .chat_scroll_offset
                        .saturating_add(self.chat_page_size());
                }
            }
            KeyCode::PageDown => {
                if self.command_palette_open {
                    self.input
                        .move_selection_by_page(self.command_palette_page_size() as isize);
                } else {
                    self.chat_scroll_offset = self
                        .chat_scroll_offset
                        .saturating_sub(self.chat_page_size());
                }
            }
            KeyCode::Home => {
                if self.command_palette_open {
                    self.input.selected_suggestion = 0;
                } else if self.input.buffer.is_empty() {
                    self.chat_scroll_offset = usize::MAX / 2;
                } else {
                    self.input.move_cursor_home();
                }
            }
            KeyCode::End => {
                if self.command_palette_open {
                    self.input.selected_suggestion = self.input.suggestions.len().saturating_sub(1);
                } else if self.input.buffer.is_empty() {
                    self.chat_scroll_offset = 0;
                } else {
                    self.input.move_cursor_end();
                }
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if ch == '/' && self.input.buffer.is_empty() && key.modifiers.is_empty() {
                    self.open_command_palette();
                    self.chat_scroll_offset = 0;
                    self.redraw_requested = true;
                    return;
                }
                self.input.append_text(&ch.to_string(), false);
                self.chat_scroll_offset = 0;
            }
            _ => {}
        }
        let suggestions = self.build_suggestions();
        self.input.update_suggestions(suggestions);
        self.redraw_requested = true;
    }

    pub(crate) fn open_command_palette(&mut self) {
        self.input.set_buffer("/");
        self.input.selected_suggestion = 0;
        self.command_palette_open = true;
        let suggestions = self.build_suggestions();
        self.input.update_suggestions(suggestions);
    }

    pub(crate) fn accept_palette_selection_for_execution(&mut self) {
        let replacement = self
            .input
            .suggestions
            .get(self.input.selected_suggestion)
            .map(|suggestion| {
                suggestion
                    .replacement
                    .as_deref()
                    .unwrap_or(&suggestion.value)
                    .to_string()
            });
        if let Some(replacement) = replacement {
            self.input.set_buffer(replacement);
        }
        self.input.suggestions.clear();
        self.input.selected_suggestion = 0;
    }

    pub(crate) fn should_execute_selected_slash_suggestion(&self) -> bool {
        let raw = self.input.buffer.trim();
        if !raw.starts_with('/')
            || raw.contains(char::is_whitespace)
            || self.input.suggestions.is_empty()
        {
            return false;
        }
        let Some((command, _)) = self.commands.parse_with_aliases(raw) else {
            return true;
        };
        self.commands.get(&command).is_none()
    }

    pub(crate) fn open_search(&mut self) {
        self.search_open = true;
        self.command_palette_open = false;
        self.input.update_suggestions(Vec::new());
        self.search_match_index = self
            .search_match_index
            .min(self.search_matches().len().saturating_sub(1));
    }

    pub(crate) fn handle_search_key(&mut self, key: KeyEvent) -> bool {
        if !self.search_open {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.search_open = false;
                true
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.handle_ctrl_c();
                true
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.search_match_index = 0;
                self.jump_to_search_match(0);
                true
            }
            KeyCode::Enter | KeyCode::Down => {
                self.jump_to_search_match(1);
                true
            }
            KeyCode::Up => {
                self.jump_to_search_match(-1);
                true
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.search_query.push(ch);
                self.search_match_index = 0;
                self.jump_to_search_match(0);
                true
            }
            _ => true,
        }
    }

    pub fn search_matches(&self) -> Vec<usize> {
        let query = self.search_query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        self.session
            .messages
            .iter()
            .enumerate()
            .filter_map(|(index, message)| {
                let role_matches = message.role.to_ascii_lowercase().contains(&query);
                let content_matches = message.content.to_ascii_lowercase().contains(&query);
                (role_matches || content_matches).then_some(index)
            })
            .collect()
    }

    pub(crate) fn jump_to_search_match(&mut self, delta: isize) {
        let matches = self.search_matches();
        if matches.is_empty() {
            self.search_match_index = 0;
            return;
        }
        let len = matches.len() as isize;
        self.search_match_index =
            (self.search_match_index as isize + delta).rem_euclid(len) as usize;
        let message_index = matches[self.search_match_index];
        self.chat_scroll_offset = self.estimated_chat_scroll_offset_for_message(message_index);
    }

    pub(crate) fn estimated_chat_scroll_offset_for_message(&self, message_index: usize) -> usize {
        self.session
            .messages
            .iter()
            .skip(message_index + 1)
            .map(estimated_message_line_count)
            .sum()
    }

    pub(crate) fn handle_pending_approval_key(&mut self, key: KeyEvent) -> bool {
        if !key.modifiers.is_empty() {
            return false;
        }
        let pending_ids = self.tool_executor.guardrails.approvals.pending_ids();
        if pending_ids.is_empty() {
            return false;
        }
        self.approval_selected_index = self
            .approval_selected_index
            .min(pending_ids.len().saturating_sub(1));
        let id = pending_ids[self.approval_selected_index].clone();
        let message = match key.code {
            KeyCode::Up => {
                self.approval_selected_index = self.approval_selected_index.saturating_sub(1);
                return true;
            }
            KeyCode::Down => {
                self.approval_selected_index =
                    (self.approval_selected_index + 1).min(pending_ids.len().saturating_sub(1));
                return true;
            }
            KeyCode::Esc => {
                self.approval_selected_index = 0;
                return true;
            }
            KeyCode::Char('1') | KeyCode::Enter | KeyCode::Char('a') | KeyCode::Char('A') => {
                match self
                    .tool_executor
                    .guardrails
                    .approvals
                    .approve_once_request(&id)
                {
                    Some(request) if self.pending_send.is_some() => {
                        format!(
                            "Approved once: {}. In-flight model run will resume.",
                            request.tool_name
                        )
                    }
                    Some(request) => self.execute_approved_request("Approved once", request),
                    None => format!("Unknown pending approval: {id}"),
                }
            }
            KeyCode::Char('2') | KeyCode::Char('s') | KeyCode::Char('S') => {
                match self
                    .tool_executor
                    .guardrails
                    .approvals
                    .approve_for_session(&id)
                {
                    Some(request) if self.pending_send.is_some() => format!(
                        "Approved matching call for this running session: {}. In-flight model run will resume.",
                        request.tool_name
                    ),
                    Some(request) => self.execute_approved_request(
                        "Approved matching call for this running session",
                        request,
                    ),
                    None => format!("Unknown pending approval: {id}"),
                }
            }
            KeyCode::Char('3') | KeyCode::Char('d') | KeyCode::Char('D') => {
                if self.tool_executor.guardrails.approvals.deny(&id) {
                    format!("Denied approval {id}.")
                } else {
                    format!("Unknown pending approval: {id}")
                }
            }
            _ => return false,
        };
        let remaining = self.tool_executor.guardrails.approvals.pending_len();
        if remaining == 0 {
            self.approval_selected_index = 0;
        } else {
            self.approval_selected_index = self.approval_selected_index.min(remaining - 1);
        }
        self.session.status = "ready".to_string();
        self.session.activity.clear();
        self.push_system_message(message);
        self.autosave_session();
        self.chat_scroll_offset = 0;
        true
    }

    pub fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        let delta = match mouse.kind {
            MouseEventKind::ScrollUp => 3isize,
            MouseEventKind::ScrollDown => -3isize,
            _ => return,
        };
        let pending_approvals = self.tool_executor.guardrails.approvals.pending_len();
        if pending_approvals > 0 {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.approval_selected_index = self.approval_selected_index.saturating_sub(1);
                }
                MouseEventKind::ScrollDown => {
                    self.approval_selected_index =
                        (self.approval_selected_index + 1).min(pending_approvals - 1);
                }
                _ => return,
            }
            self.redraw_requested = true;
            return;
        }
        if self.command_palette_open
            && self.input.buffer.starts_with('/')
            && self.input.buffer.chars().count() > 1
        {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.input.move_selection(-1);
                }
                MouseEventKind::ScrollDown => {
                    self.input.move_selection(1);
                }
                _ => return,
            }
            self.redraw_requested = true;
            return;
        }
        if self.diff_overlay.is_some() {
            self.diff_scroll_offset = apply_scroll_delta(self.diff_scroll_offset, delta);
            self.redraw_requested = true;
            return;
        }
        if self.info_overlay.is_some() {
            self.info_scroll_offset = apply_scroll_delta(self.info_scroll_offset, delta);
            self.redraw_requested = true;
            return;
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.chat_scroll_offset = self.chat_scroll_offset.saturating_add(3);
            }
            MouseEventKind::ScrollDown => {
                self.chat_scroll_offset = self.chat_scroll_offset.saturating_sub(3);
            }
            _ => return,
        }
        self.redraw_requested = true;
    }
}
