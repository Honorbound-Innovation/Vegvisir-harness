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
        if key.code == KeyCode::F(12) {
            self.toggle_mouse_capture_mode();
            self.redraw_requested = true;
            return;
        }
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
        match mouse.kind {
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                if self.can_start_chat_drag(mouse.column, mouse.row) {
                    self.drag_anchor = Some((mouse.column, mouse.row));
                    self.drag_current = Some((mouse.column, mouse.row));
                    self.redraw_requested = true;
                } else {
                    self.drag_anchor = None;
                    self.drag_current = None;
                }
                return;
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
                if self.drag_anchor.is_some() {
                    self.drag_current = Some((mouse.column, mouse.row));
                    self.redraw_requested = true;
                }
                return;
            }
            MouseEventKind::Up(crossterm::event::MouseButton::Left) => {
                self.handle_chat_mouse_up(mouse.column, mouse.row);
                self.redraw_requested = true;
                return;
            }
            _ => {}
        }
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

impl TuiApplication {
    pub(crate) fn toggle_mouse_capture_mode(&mut self) {
        self.mouse_capture_enabled = !self.mouse_capture_enabled;
        self.drag_anchor = None;
        self.drag_current = None;
        let message = if self.mouse_capture_enabled {
            "Mouse capture ON: wheel scroll and in-app drag copy enabled (F12 releases native selection)."
        } else {
            "Mouse capture OFF: terminal-native text selection enabled (F12 restores wheel/in-app copy)."
        };
        self.push_system_message(message);
    }

    fn can_start_chat_drag(&self, col: u16, row: u16) -> bool {
        self.mouse_capture_enabled
            && !self.help_overlay_open
            && self.diff_overlay.is_none()
            && self.info_overlay.is_none()
            && !self.search_open
            && !self.command_palette_open
            && self.tool_executor.guardrails.approvals.pending_len() == 0
            && self.point_in_chat_area(col, row)
    }

    fn point_in_chat_area(&self, col: u16, row: u16) -> bool {
        let Some(x_end) = self.chat_area_x.checked_add(self.chat_area_width) else {
            return false;
        };
        let Some(y_end) = self.chat_area_y.checked_add(self.chat_area_height) else {
            return false;
        };
        col >= self.chat_area_x && col < x_end && row >= self.chat_area_y && row < y_end
    }

    pub(crate) fn handle_chat_mouse_up(&mut self, col: u16, row: u16) {
        let Some(anchor) = self.drag_anchor.take() else {
            self.drag_current = None;
            return;
        };
        self.drag_current = None;
        let text = self.extract_chat_drag_selection(anchor, (col, row));
        if text.trim().is_empty() {
            return;
        }
        if Self::copy_to_clipboard(&text) {
            self.push_system_message("Copied selected chat text to clipboard.");
        } else {
            self.push_system_message(
                "Could not copy selection: install pbcopy, wl-copy, xclip, or xsel.",
            );
        }
    }

    pub(crate) fn extract_chat_drag_selection(
        &self,
        anchor: (u16, u16),
        end: (u16, u16),
    ) -> String {
        if self.chat_rendered_lines.is_empty() || self.chat_area_height == 0 {
            return String::new();
        }
        let Some((start_line, start_col, end_line, end_col)) = self.drag_bounds(anchor, end) else {
            return String::new();
        };
        let mut out = Vec::new();
        for line_index in start_line..=end_line {
            let Some(line) = self.chat_rendered_lines.get(line_index) else {
                continue;
            };
            let from = if line_index == start_line {
                start_col
            } else {
                0
            };
            let to = if line_index == end_line {
                end_col
            } else {
                usize::MAX
            };
            let selected = slice_display_columns(line, from, to);
            out.push(selected.trim_end().to_string());
        }
        out.join("\n")
    }

    fn drag_bounds(
        &self,
        anchor: (u16, u16),
        end: (u16, u16),
    ) -> Option<(usize, usize, usize, usize)> {
        let point = |(col, row): (u16, u16)| -> Option<(usize, usize)> {
            let row_in_chat = row.checked_sub(self.chat_area_y)? as usize;
            if row_in_chat >= self.chat_area_height as usize {
                return None;
            }
            let col_in_chat = col.saturating_sub(self.chat_area_x) as usize;
            Some((self.chat_render_scroll + row_in_chat, col_in_chat))
        };
        let a = point(anchor)?;
        let b = point(end)?;
        if a <= b {
            Some((a.0, a.1, b.0, b.1))
        } else {
            Some((b.0, b.1, a.0, a.1))
        }
    }

    fn copy_to_clipboard(text: &str) -> bool {
        #[cfg(target_os = "macos")]
        let candidates: &[(&str, &[&str])] = &[("pbcopy", &[])];
        #[cfg(not(target_os = "macos"))]
        let candidates: &[(&str, &[&str])] = &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ];
        for (program, args) in candidates {
            let Ok(mut child) = Command::new(program)
                .args(*args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            else {
                continue;
            };
            if let Some(stdin) = child.stdin.as_mut() {
                use std::io::Write;
                if stdin.write_all(text.as_bytes()).is_err() {
                    continue;
                }
            }
            if child.wait().is_ok_and(|status| status.success()) {
                return true;
            }
        }
        false
    }
}

fn slice_display_columns(text: &str, start_col: usize, end_col: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    if end_col <= start_col {
        return String::new();
    }
    let mut out = String::new();
    let mut col = 0usize;
    for ch in text.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        let next = col + width;
        if next > start_col && col < end_col {
            out.push(ch);
        }
        col = next;
        if col >= end_col {
            break;
        }
    }
    out
}
