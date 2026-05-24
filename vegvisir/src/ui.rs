pub mod input {
    use unicode_width::UnicodeWidthChar;

    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct Suggestion {
        pub value: String,
        pub description: String,
        pub replacement: Option<String>,
    }

    impl Suggestion {
        pub fn new(
            value: impl Into<String>,
            description: impl Into<String>,
            replacement: Option<String>,
        ) -> Self {
            Self {
                value: value.into(),
                description: description.into(),
                replacement,
            }
        }
    }

    #[derive(Clone, Debug, Default)]
    pub struct InputState {
        pub buffer: String,
        pub cursor: usize,
        pub paste_char_count: usize,
        pub history: Vec<String>,
        pub history_index: Option<usize>,
        pub history_draft: Option<String>,
        pub preferred_column: Option<usize>,
        pub suggestions: Vec<Suggestion>,
        pub selected_suggestion: usize,
    }

    impl InputState {
        pub fn update_suggestions(&mut self, suggestions: Vec<Suggestion>) {
            if !self.buffer.starts_with('/') {
                self.suggestions.clear();
                self.selected_suggestion = 0;
                return;
            }
            self.suggestions = suggestions
                .into_iter()
                .filter(|suggestion| {
                    let replacement = suggestion
                        .replacement
                        .as_deref()
                        .unwrap_or(&suggestion.value);
                    let query = self
                        .buffer
                        .trim()
                        .trim_start_matches('/')
                        .to_ascii_lowercase();
                    let replacement_search =
                        replacement.trim_start_matches('/').to_ascii_lowercase();
                    let description_search = suggestion.description.to_ascii_lowercase();
                    replacement.starts_with(&self.buffer)
                        || self.buffer.ends_with(' ')
                        || self.buffer.contains(' ')
                        || (!query.is_empty()
                            && (replacement_search.contains(&query)
                                || description_search.contains(&query)))
                })
                .collect();
            self.selected_suggestion = self
                .selected_suggestion
                .min(self.suggestions.len().saturating_sub(1));
        }

        pub fn accept_suggestion(&mut self) -> bool {
            if !self.buffer.starts_with('/') || self.suggestions.is_empty() {
                return false;
            }
            let suggestion = &self.suggestions[self.selected_suggestion];
            self.buffer = format!(
                "{} ",
                suggestion
                    .replacement
                    .as_deref()
                    .unwrap_or(&suggestion.value)
            );
            self.cursor = self.buffer.chars().count();
            self.paste_char_count = 0;
            self.suggestions.clear();
            self.selected_suggestion = 0;
            true
        }

        pub fn move_selection(&mut self, delta: isize) -> bool {
            if !self.buffer.starts_with('/') || self.suggestions.is_empty() {
                return false;
            }
            let len = self.suggestions.len() as isize;
            self.selected_suggestion =
                (self.selected_suggestion as isize + delta).rem_euclid(len) as usize;
            true
        }

        pub fn move_selection_by_page(&mut self, delta: isize) -> bool {
            if !self.buffer.starts_with('/') || self.suggestions.is_empty() {
                return false;
            }
            let last = self.suggestions.len().saturating_sub(1) as isize;
            let next = (self.selected_suggestion as isize + delta).clamp(0, last);
            self.selected_suggestion = next as usize;
            true
        }

        pub fn push_history(&mut self, value: impl Into<String>) {
            let value = value.into();
            if !value.is_empty() {
                self.history.push(value);
            }
            self.history_index = None;
            self.history_draft = None;
            self.preferred_column = None;
        }

        pub fn append_text(&mut self, text: &str, pasted: bool) {
            self.history_index = None;
            self.history_draft = None;
            self.preferred_column = None;
            let byte_index = char_to_byte_index(&self.buffer, self.cursor);
            self.buffer.insert_str(byte_index, text);
            self.cursor += text.chars().count();
            if pasted {
                self.paste_char_count += text.chars().count();
            }
        }

        pub fn backspace(&mut self) {
            if self.cursor == 0 {
                return;
            }
            let end = char_to_byte_index(&self.buffer, self.cursor);
            let start = char_to_byte_index(&self.buffer, self.cursor - 1);
            self.buffer.replace_range(start..end, "");
            self.cursor -= 1;
            self.paste_char_count = self.paste_char_count.min(self.buffer.chars().count());
            self.history_index = None;
            self.history_draft = None;
            self.preferred_column = None;
        }

        pub fn move_cursor(&mut self, delta: isize) {
            let len = self.buffer.chars().count() as isize;
            self.cursor = (self.cursor as isize + delta).clamp(0, len) as usize;
            self.preferred_column = None;
        }

        pub fn visual_line_count(&self, width: usize) -> usize {
            visual_line_spans(&self.buffer, width).len()
        }

        pub fn visible_visual_cursor_position(
            &self,
            width: usize,
            max_rows: usize,
        ) -> (usize, usize) {
            let (line, col) = self.visual_cursor_position(width);
            let total = self.visual_line_count(width);
            let hidden_rows = total.saturating_sub(max_rows.max(1));
            (line.saturating_sub(hidden_rows), col)
        }

        pub fn visual_cursor_position(&self, width: usize) -> (usize, usize) {
            visual_cursor_position(&self.buffer, self.cursor, width)
        }

        pub fn move_cursor_vertical(&mut self, delta: isize, width: usize) -> bool {
            let spans = visual_line_spans(&self.buffer, width);
            if spans.len() <= 1 {
                return false;
            }
            let (line, column) =
                visual_cursor_position_from_spans(&self.buffer, &spans, self.cursor);
            let target_line = line as isize + delta;
            if target_line < 0 || target_line >= spans.len() as isize {
                return false;
            }
            let preferred = self.preferred_column.unwrap_or(column);
            self.preferred_column = Some(preferred);
            let (start, len) = spans[target_line as usize];
            self.cursor = cursor_for_display_column(&self.buffer, start, len, preferred);
            true
        }

        pub fn move_cursor_home(&mut self) {
            self.cursor = 0;
            self.preferred_column = None;
        }

        pub fn move_cursor_end(&mut self) {
            self.cursor = self.buffer.chars().count();
            self.preferred_column = None;
        }

        pub fn set_buffer(&mut self, value: impl Into<String>) {
            self.buffer = value.into();
            self.cursor = self.buffer.chars().count();
            self.paste_char_count = 0;
            self.preferred_column = None;
        }

        pub fn clear(&mut self) {
            self.buffer.clear();
            self.cursor = 0;
            self.paste_char_count = 0;
            self.history_index = None;
            self.history_draft = None;
            self.preferred_column = None;
        }

        pub fn history_move(&mut self, delta: isize) -> bool {
            if self.history.is_empty() {
                return false;
            }
            let current = match self.history_index {
                Some(index) => index as isize,
                None => {
                    self.history_draft = Some(self.buffer.clone());
                    self.history.len() as isize
                }
            };
            let next = current + delta;
            if next < 0 {
                self.history_index = Some(0);
                self.set_buffer(self.history[0].clone());
                self.cursor = 0;
                return true;
            }
            if next >= self.history.len() as isize {
                self.history_index = None;
                let draft = self.history_draft.take().unwrap_or_default();
                self.set_buffer(draft);
                self.cursor = 0;
                return true;
            }
            self.history_index = Some(next as usize);
            self.set_buffer(self.history[next as usize].clone());
            self.cursor = 0;
            true
        }
    }

    fn char_to_byte_index(value: &str, char_index: usize) -> usize {
        value
            .char_indices()
            .nth(char_index)
            .map(|(index, _)| index)
            .unwrap_or(value.len())
    }

    fn visual_cursor_position(value: &str, cursor: usize, width: usize) -> (usize, usize) {
        let spans = visual_line_spans(value, width);
        visual_cursor_position_from_spans(value, &spans, cursor)
    }

    fn visual_cursor_position_from_spans(
        value: &str,
        spans: &[(usize, usize)],
        cursor: usize,
    ) -> (usize, usize) {
        for (line, (start, len)) in spans.iter().copied().enumerate() {
            if cursor >= start && cursor <= start + len {
                return (line, display_width_between(value, start, cursor));
            }
        }
        spans
            .last()
            .map(|(start, len)| {
                (
                    spans.len().saturating_sub(1),
                    display_width_between(value, *start, cursor.min(*start + *len)),
                )
            })
            .unwrap_or((0, 0))
    }

    fn cursor_for_display_column(value: &str, start: usize, len: usize, column: usize) -> usize {
        let mut used = 0;
        for (offset, ch) in value.chars().skip(start).take(len).enumerate() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + ch_width > column {
                return start + offset;
            }
            used += ch_width;
        }
        start + len
    }

    fn display_width_between(value: &str, start: usize, end: usize) -> usize {
        value
            .chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
            .sum()
    }

    fn visual_line_spans(value: &str, width: usize) -> Vec<(usize, usize)> {
        let width = width.max(1);
        if value.is_empty() {
            return vec![(0, 0)];
        }
        let mut spans = Vec::new();
        let mut start = 0;
        let mut len = 0;
        let mut current_width = 0;
        for (index, ch) in value.chars().enumerate() {
            if ch == '\n' {
                spans.push((start, len));
                start = index + 1;
                len = 0;
                current_width = 0;
                continue;
            }
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if current_width + ch_width > width && len > 0 {
                spans.push((start, len));
                start = index;
                len = 0;
                current_width = 0;
            }
            len += 1;
            current_width += ch_width;
        }
        spans.push((start, len));
        spans
    }

    #[cfg(test)]
    mod tests {
        use super::InputState;

        #[test]
        fn visual_cursor_tracks_multiline_input_with_tail_clipping() {
            let mut input = InputState::default();
            input.append_text(
                "one
two
three
four",
                false,
            );

            assert_eq!(input.visual_line_count(20), 4);
            assert_eq!(input.visual_cursor_position(20), (3, 4));
            assert_eq!(input.visible_visual_cursor_position(20, 3), (2, 4));
        }

        #[test]
        fn visual_cursor_uses_display_width_for_wide_characters() {
            let mut input = InputState::default();
            input.append_text("a界b", false);

            assert_eq!(input.visual_cursor_position(20), (0, 4));
        }
    }
}

pub mod theme {
    #[derive(Clone, Debug)]
    pub struct UiTheme {
        pub name: String,
        pub border: String,
        pub heading: String,
        pub normal: String,
        pub dim: String,
        pub active_bg: String,
        pub active_fg: String,
        pub success: String,
        pub warning: String,
        pub error: String,
        pub info: String,
        pub color_enabled: bool,
    }

    impl Default for UiTheme {
        fn default() -> Self {
            Self {
                name: "dark-cobalt".to_string(),
                border: "38;5;33".to_string(),
                heading: "38;5;39".to_string(),
                normal: "38;5;250".to_string(),
                dim: "38;5;241".to_string(),
                active_bg: "48;5;24".to_string(),
                active_fg: "38;5;231".to_string(),
                success: "38;5;114".to_string(),
                warning: "38;5;179".to_string(),
                error: "38;5;203".to_string(),
                info: "38;5;81".to_string(),
                color_enabled: supports_color(),
            }
        }
    }

    #[derive(Clone, Debug)]
    pub struct ThemeRenderer {
        pub theme: UiTheme,
    }

    impl Default for ThemeRenderer {
        fn default() -> Self {
            Self {
                theme: UiTheme::default(),
            }
        }
    }

    impl ThemeRenderer {
        pub fn paint(&self, text: impl AsRef<str>, style: &str) -> String {
            let text = text.as_ref();
            if !self.theme.color_enabled {
                return text.to_string();
            }
            let code = match style {
                "border" => &self.theme.border,
                "heading" => &self.theme.heading,
                "title" => "1;38;5;231",
                "dim" => &self.theme.dim,
                "code" => "38;5;151",
                "code_keyword" => "38;5;81",
                "code_string" => "38;5;222",
                "code_comment" => "38;5;244",
                "diff_add" => "48;5;22;38;5;151",
                "diff_del" => "48;5;52;38;5;224",
                "diff_hunk" => "38;5;105",
                "diff_header" => "38;5;75",
                "diff_lineno" => "38;5;34",
                "table" => "38;5;117",
                "success" => &self.theme.success,
                "warning" => &self.theme.warning,
                "error" => &self.theme.error,
                "info" => &self.theme.info,
                "status" => "7",
                _ => &self.theme.normal,
            };
            format!("\x1b[{code}m{text}\x1b[0m")
        }

        pub fn active(&self, text: impl AsRef<str>) -> String {
            let text = text.as_ref();
            if !self.theme.color_enabled {
                format!("> {text}")
            } else {
                format!(
                    "\x1b[{};{}m{text}\x1b[0m",
                    self.theme.active_bg, self.theme.active_fg
                )
            }
        }
    }

    fn supports_color() -> bool {
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }
        !matches!(
            std::env::var("TERM").as_deref(),
            Ok("") | Ok("dumb") | Err(_)
        )
    }
}

pub mod layout {
    use std::collections::BTreeMap;

    use crate::{
        command_registry::CommandRegistry,
        core::{SessionState, SkillDefinition, ToolDefinition},
        guardrails::ApprovalRequest,
        ui::{
            input::{InputState, Suggestion},
            theme::ThemeRenderer,
        },
    };

    pub fn visible_width(text: &str) -> usize {
        let mut width = 0;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' && chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next == 'm' {
                        break;
                    }
                }
            } else {
                width += 1;
            }
        }
        width
    }

    pub fn truncate(text: &str, width: usize) -> String {
        if width <= 1 {
            return String::new();
        }
        if visible_width(text) <= width {
            return text.to_string();
        }
        truncate_ansi(text, width)
    }

    fn truncate_ansi(text: &str, width: usize) -> String {
        let target = width.saturating_sub(1);
        let mut out = String::new();
        let mut visible = 0;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' && chars.peek() == Some(&'[') {
                out.push(ch);
                out.push(chars.next().unwrap_or('['));
                for next in chars.by_ref() {
                    out.push(next);
                    if next == 'm' {
                        break;
                    }
                }
                continue;
            }
            if visible >= target {
                break;
            }
            out.push(ch);
            visible += 1;
        }
        out.push('…');
        if text.contains("\x1b[") {
            out.push_str("\x1b[0m");
        }
        out
    }

    fn pad_to_width(text: &str, width: usize) -> String {
        format!(
            "{text}{}",
            " ".repeat(width.saturating_sub(visible_width(text)))
        )
    }

    fn border_line(width: usize, left: &str, fill: &str, right: &str) -> String {
        format!("{left}{}{right}", fill.repeat(width.saturating_sub(2)))
    }

    fn boxed(
        lines: &[String],
        width: usize,
        theme: &ThemeRenderer,
        title: Option<&str>,
    ) -> Vec<String> {
        let mut top = border_line(width, "┌", "─", "┐");
        if let Some(title) = title {
            let label = format!(" {title} ");
            if label.chars().count() < width.saturating_sub(2) {
                top = format!(
                    "┌{}{}┐",
                    label,
                    "─".repeat(width - label.chars().count() - 2)
                );
            }
        }
        let mut rendered = vec![theme.paint(top, "border")];
        for line in lines {
            let body = truncate(line, width.saturating_sub(4));
            rendered.push(format!(
                "{} {} {}",
                theme.paint("│", "border"),
                pad_to_width(&body, width.saturating_sub(4)),
                theme.paint("│", "border")
            ));
        }
        rendered.push(theme.paint(border_line(width, "└", "─", "┘"), "border"));
        rendered
    }

    #[derive(Clone, Debug)]
    pub struct LayoutRenderer {
        pub theme: ThemeRenderer,
        pub viewport: Option<(usize, usize)>,
    }

    impl Default for LayoutRenderer {
        fn default() -> Self {
            Self {
                theme: ThemeRenderer::default(),
                viewport: None,
            }
        }
    }

    impl LayoutRenderer {
        pub fn render_startup(
            &self,
            session: &SessionState,
            commands: &CommandRegistry,
            input: &InputState,
            suggestions: &[Suggestion],
            selected_suggestion: usize,
            chat_scroll_offset: usize,
            pending_approvals: &[ApprovalRequest],
        ) -> String {
            let (width, height) = self
                .viewport
                .unwrap_or_else(|| (terminal_columns(), terminal_lines()));
            if width < 50 || height < 18 {
                return self.too_small(width, height);
            }
            let mut rows = Vec::new();
            rows.push(self.header_bar(session, width));
            if session.messages.is_empty() {
                if width >= 104 {
                    let left_w = (width / 4).clamp(24, 30);
                    let main_w = width - left_w - 1;
                    let left = self.identity_panel(session, left_w);
                    let main = self.dashboard_panel(session, main_w);
                    for index in 0..left.len().max(main.len()) {
                        rows.push(format!(
                            "{} {}",
                            pad_to_width(left.get(index).map(String::as_str).unwrap_or(""), left_w),
                            main.get(index).map(String::as_str).unwrap_or("")
                        ));
                    }
                } else {
                    rows.extend(self.identity_panel(session, width));
                    rows.extend(self.dashboard_panel(session, width));
                }
            } else {
                rows.push(self.session_strip(session, width));
            }
            rows.extend(self.pending_attachments(session, width));
            rows.extend(self.pending_approvals(pending_approvals, width));
            let autocomplete_height = if input.buffer.starts_with('/') && !suggestions.is_empty() {
                suggestions.len().min(8) + 2
            } else {
                0
            };
            let input_lines = self.input_lines(input, width.saturating_sub(2));
            let fixed_tail = 2 + input_lines.len() + autocomplete_height;
            let chat_height = height.saturating_sub(rows.len() + fixed_tail).max(6);
            rows.extend(self.chat(session, width, chat_height, chat_scroll_offset));
            rows.push(self.status_bar(session, width));
            rows.push(self.key_hint_bar(width));
            for (index, line) in input_lines.into_iter().enumerate() {
                let prompt = if index == 0 { "› " } else { "  " };
                rows.push(format!("{}{}", self.theme.paint(prompt, "heading"), line));
            }
            if input.buffer.starts_with('/') && !suggestions.is_empty() {
                rows.extend(self.autocomplete(suggestions, commands, selected_suggestion, width));
            }
            rows.join("\n")
        }

        fn too_small(&self, width: usize, height: usize) -> String {
            let required = "Vegvisir needs at least 50x18.";
            [
                self.theme.paint("Vegvisir", "title"),
                format!("terminal: {width}x{height}"),
                required.to_string(),
                "Resize the terminal to continue.".to_string(),
            ]
            .join("\n")
        }

        fn header_bar(&self, session: &SessionState, width: usize) -> String {
            let title = self.theme.paint(" Vegvisir ", "title");
            let cwd = truncate(&session.cwd, width / 3);
            let meta = format!(
                " provider={} model={} workspace={} ",
                session.current_provider, session.current_model, cwd
            );
            let line = format!("{title}{}", self.theme.paint(truncate(&meta, width), "dim"));
            truncate(&line, width)
        }

        fn session_strip(&self, session: &SessionState, width: usize) -> String {
            let state_style = match session.status.as_str() {
                "ready" => "success",
                "streaming" => "info",
                "error" => "error",
                _ => "warning",
            };
            let tools = session
                .enabled_tools
                .iter()
                .filter(|tool| tool.enabled)
                .count();
            let skills = session
                .enabled_skills
                .iter()
                .filter(|skill| skill.enabled)
                .count();
            truncate(
                &format!(
                    "{}  {}  {}  {}  {}",
                    self.theme.paint(format!("{} tools", tools), "dim"),
                    self.theme.paint(format!("{} skills", skills), "dim"),
                    self.theme
                        .paint(format!("state {}", session.status), state_style),
                    self.theme
                        .paint(format!("session {}", session.session_id), "dim"),
                    self.theme.paint("/status /tools /skills /history", "dim"),
                ),
                width,
            )
        }

        fn input_lines(&self, input: &InputState, width: usize) -> Vec<String> {
            if input.paste_char_count > width {
                let marker = format!("[Pasted {} characters]", input.paste_char_count);
                let prefix_width = width.saturating_sub(marker.chars().count() + 1);
                return vec![truncate(
                    &format!(
                        "{} {}",
                        truncate(&input.buffer, prefix_width).trim(),
                        marker
                    ),
                    width,
                )];
            }
            let max_lines = 6;
            let mut lines = wrap_input_text(&input.buffer, width);
            if lines.len() > max_lines {
                let hidden = lines.len() - max_lines;
                lines = lines[hidden..].to_vec();
                if let Some(first) = lines.first_mut() {
                    *first = truncate(&format!("… {first}"), width);
                }
            }
            if lines.is_empty() {
                vec![String::new()]
            } else {
                lines
            }
        }

        fn identity_panel(&self, session: &SessionState, width: usize) -> Vec<String> {
            let state_style = match session.status.as_str() {
                "ready" => "success",
                "streaming" => "info",
                "error" => "error",
                _ => "warning",
            };
            [
                self.theme.paint("status", "heading"),
                format!(
                    "provider {}",
                    self.theme.paint(&session.current_provider, "info")
                ),
                format!(
                    "model    {}",
                    self.theme.paint(&session.current_model, "info")
                ),
                format!(
                    "cwd      {}",
                    self.theme
                        .paint(truncate(&session.cwd, width.saturating_sub(9)), "dim")
                ),
                format!("id       {}", session.session_id),
                format!(
                    "state    {}",
                    self.theme.paint(&session.status, state_style)
                ),
            ]
            .into_iter()
            .map(|line| truncate(&line, width))
            .collect()
        }

        fn dashboard_panel(&self, session: &SessionState, width: usize) -> Vec<String> {
            [
                self.theme.paint("dashboard", "heading"),
                self.theme.paint("Vegvisir Console 0.1.0", "title"),
                format!(
                    "tools  {} enabled: {}",
                    self.theme
                        .paint(session.enabled_tools.len().to_string(), "success"),
                    name_preview_tools(&session.enabled_tools, width.saturating_sub(24))
                ),
                format!(
                    "skills {} enabled: {}",
                    self.theme
                        .paint(session.enabled_skills.len().to_string(), "success"),
                    name_preview_skills(&session.enabled_skills, width.saturating_sub(24))
                ),
                self.theme.paint(
                    "/help commands | /tools inventory | /skills inventory",
                    "dim",
                ),
            ]
            .into_iter()
            .map(|line| truncate(&line, width))
            .collect()
        }

        fn chat(
            &self,
            session: &SessionState,
            width: usize,
            height: usize,
            scroll_offset: usize,
        ) -> Vec<String> {
            let inner_height = height.max(1);
            let mut lines = Vec::new();
            if session.messages.is_empty() {
                lines.push(self.theme.paint("No messages yet.", "dim"));
            }
            for (message_index, message) in session.messages.iter().enumerate() {
                if message_index > 0 {
                    lines.push(String::new());
                }
                let label = match message.role.as_str() {
                    "user" => "you",
                    "assistant" => "agent",
                    "system" => "note",
                    other => other,
                };
                let suffix = if message.attachments.is_empty() {
                    String::new()
                } else {
                    format!(" [{} attachment(s)]", message.attachments.len())
                };
                lines.extend(self.message_lines(
                    label,
                    &message.content,
                    &suffix,
                    width.saturating_sub(12).max(20),
                ));
            }
            if session
                .messages
                .last()
                .is_some_and(|message| message.role == "assistant" && !message.content.is_empty())
            {
                lines.push(String::new());
            }
            if session.status == "streaming" {
                if let Some(activity) = self.activity_label(session) {
                    lines.push(format!(
                        "{:<6} {}",
                        "agent",
                        truncate(&activity, width.saturating_sub(12).max(20))
                    ));
                }
            }
            let max_offset = lines.len().saturating_sub(inner_height);
            let offset = scroll_offset.min(max_offset);
            let end = lines.len().saturating_sub(offset);
            let start = end.saturating_sub(inner_height);
            let mut visible = lines[start..end].to_vec();
            while visible.len() < inner_height {
                visible.push(String::new());
            }
            let title = if max_offset == 0 {
                "chat".to_string()
            } else {
                format!("chat {}-{}/{}", start + 1, end, lines.len())
            };
            let mut out = vec![self.theme.paint(format!(" {title} "), "heading")];
            out.extend(
                visible
                    .into_iter()
                    .take(inner_height.saturating_sub(1))
                    .map(|line| truncate(&line, width)),
            );
            while out.len() < height {
                out.push(String::new());
            }
            out
        }

        fn message_lines(
            &self,
            label: &str,
            content: &str,
            suffix: &str,
            width: usize,
        ) -> Vec<String> {
            let rendered = render_markdown(content, width, &self.theme);
            let rendered = if rendered.is_empty() {
                vec![String::new()]
            } else {
                rendered
            };
            rendered
                .into_iter()
                .enumerate()
                .map(|(index, line)| {
                    let prefix = if index == 0 { label } else { "" };
                    let end = if index == 0 { suffix } else { "" };
                    let style = match label {
                        "you" => "info",
                        "agent" => "success",
                        "note" => "warning",
                        _ => "dim",
                    };
                    format!(
                        "{} {line}{end}",
                        self.theme.paint(format!("{prefix:<6}"), style)
                    )
                })
                .collect()
        }

        fn pending_attachments(&self, session: &SessionState, width: usize) -> Vec<String> {
            if session.pending_attachments.is_empty() {
                return Vec::new();
            }
            let lines = session
                .pending_attachments
                .iter()
                .rev()
                .take(5)
                .rev()
                .map(|item| {
                    format!(
                        "{}: {} ({})",
                        item.kind,
                        item.name.as_deref().unwrap_or(&item.path),
                        item.mime_type.as_deref().unwrap_or("unknown")
                    )
                })
                .collect::<Vec<_>>();
            boxed(&lines, width, &self.theme, Some("attachments"))
        }

        fn pending_approvals(&self, approvals: &[ApprovalRequest], width: usize) -> Vec<String> {
            let Some(request) = approvals.first() else {
                return Vec::new();
            };
            let extra = approvals.len().saturating_sub(1);
            let mut lines =
                vec![
                self.theme.paint(" approval required ", "warning"),
                self.theme.paint(
                    truncate(
                        &format!(
                            "? {} risk={} id={}",
                            request.reason, request.risk_label, request.id
                        ),
                        width,
                    ),
                    "warning",
                ),
                truncate(
                    &format!(
                        "  tool={} args={}",
                        request.tool_name,
                        serde_json::to_string(&request.args).unwrap_or_default()
                    ),
                    width,
                ),
                self.theme.paint(
                    truncate(
                        "  [1] approve once and run    [2] allow for session and run    [3] deny",
                        width,
                    ),
                    "heading",
                ),
                self.theme.paint(
                    truncate(&format!("  inspect details: /approvals show {}", request.id), width),
                    "dim",
                ),
            ];
            if extra > 0 {
                lines.push(self.theme.paint(
                    truncate(
                        &format!("  {extra} more pending approval(s). Use /approvals list."),
                        width,
                    ),
                    "dim",
                ));
            }
            lines
        }

        fn status_bar(&self, session: &SessionState, width: usize) -> String {
            let mut segments = vec![
                format!(
                    "{} {}",
                    self.theme.paint("provider", "dim"),
                    session.current_provider
                ),
                format!(
                    "{} {}",
                    self.theme.paint("model", "dim"),
                    session.current_model
                ),
                format!("ctx {}/{}", session.tokens_used, session.context_limit),
                format!("worked {}", format_duration(session.last_latency_ms)),
                format!("session {}", session.session_id),
            ];
            if let Some(activity) = self.activity_label(session) {
                segments.insert(2, activity);
            }
            if let Some(cache_key) = &session.last_prompt_cache_key {
                segments.insert(3, format!("cache {}", truncate(cache_key, 18)));
            }
            self.theme.paint(
                truncate(
                    &segments
                        .into_iter()
                        .map(|s| format!("[{s}]"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    width,
                ),
                "heading",
            )
        }

        fn key_hint_bar(&self, width: usize) -> String {
            self.theme.paint(
                truncate(
                    "[Enter] send  [PgUp/PgDn] chat  [/] commands  [drag] select text  [?] help  [Ctrl+C] quit",
                    width,
                ),
                "dim",
            )
        }

        fn activity_label(&self, session: &SessionState) -> Option<String> {
            if session.status != "streaming" {
                return None;
            }
            let spinner = ["◐", "◓", "◑", "◒"][session.activity_tick as usize % 4];
            let text = if session.activity.is_empty() {
                "thinking through the request"
            } else {
                &session.activity
            };
            Some(format!("{spinner} {text}"))
        }

        fn autocomplete(
            &self,
            suggestions: &[Suggestion],
            _commands: &CommandRegistry,
            selected: usize,
            width: usize,
        ) -> Vec<String> {
            let panel_width = width.min(78);
            let max_visible = 8;
            let selected = selected.min(suggestions.len().saturating_sub(1));
            let start = if suggestions.len() <= max_visible {
                0
            } else {
                selected
                    .saturating_sub(max_visible - 1)
                    .min(suggestions.len() - max_visible)
            };
            let visible = &suggestions[start..(start + max_visible).min(suggestions.len())];
            let title = if suggestions.len() <= max_visible {
                "select".to_string()
            } else {
                format!(
                    "select {}-{}/{}",
                    start + 1,
                    start + visible.len(),
                    suggestions.len()
                )
            };
            let mut rendered = boxed(&[], panel_width, &self.theme, Some(&title));
            rendered.pop();
            for (relative, suggestion) in visible.iter().enumerate() {
                let index = start + relative;
                let line = truncate(
                    &format!("{:<24} {}", suggestion.value, suggestion.description),
                    panel_width.saturating_sub(4),
                );
                let mut padded =
                    format!(" {} ", pad_to_width(&line, panel_width.saturating_sub(4)));
                if index == selected {
                    padded = self.theme.active(padded);
                }
                rendered.push(format!(
                    "{}{}{}",
                    self.theme.paint("│", "border"),
                    padded,
                    self.theme.paint("│", "border")
                ));
            }
            rendered.push(
                self.theme
                    .paint(border_line(panel_width, "└", "─", "┘"), "border"),
            );
            rendered
        }
    }

    fn terminal_columns() -> usize {
        crossterm::terminal::size()
            .map(|(columns, _)| usize::from(columns))
            .ok()
            .or_else(|| std::env::var("COLUMNS").ok().and_then(|v| v.parse().ok()))
            .unwrap_or(100)
    }

    fn terminal_lines() -> usize {
        crossterm::terminal::size()
            .map(|(_, lines)| usize::from(lines))
            .ok()
            .or_else(|| std::env::var("LINES").ok().and_then(|v| v.parse().ok()))
            .unwrap_or(32)
    }

    fn name_preview_tools(items: &[ToolDefinition], width: usize) -> String {
        truncate(
            &items
                .iter()
                .filter(|item| item.enabled)
                .map(|item| item.name.as_str())
                .take(8)
                .collect::<Vec<_>>()
                .join(", "),
            width.max(10),
        )
    }

    fn name_preview_skills(items: &[SkillDefinition], width: usize) -> String {
        truncate(
            &items
                .iter()
                .filter(|item| item.enabled)
                .map(|item| item.name.as_str())
                .take(8)
                .collect::<Vec<_>>()
                .join(", "),
            width.max(10),
        )
    }

    fn wrap_text(text: &str, width: usize) -> Vec<String> {
        if text.is_empty() {
            return vec![String::new()];
        }
        let mut out = Vec::new();
        let mut line = String::new();
        for word in text.split_whitespace() {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
                out.push(line);
                line = String::new();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            if word.chars().count() > width {
                let chars = word.chars().collect::<Vec<_>>();
                for chunk in chars.chunks(width) {
                    if !line.is_empty() {
                        out.push(line);
                        line = String::new();
                    }
                    out.push(chunk.iter().collect());
                }
            } else {
                line.push_str(word);
            }
        }
        if !line.is_empty() {
            out.push(line);
        }
        out
    }

    fn wrap_input_text(text: &str, width: usize) -> Vec<String> {
        let width = width.max(1);
        if text.is_empty() {
            return vec![String::new()];
        }
        let mut out = Vec::new();
        for logical_line in text.split('\n') {
            if logical_line.is_empty() {
                out.push(String::new());
                continue;
            }
            let mut current = String::new();
            for ch in logical_line.chars() {
                if current.chars().count() >= width {
                    out.push(current);
                    current = String::new();
                }
                current.push(ch);
            }
            out.push(current);
        }
        out
    }

    fn render_markdown(content: &str, width: usize, theme: &ThemeRenderer) -> Vec<String> {
        let mut out = Vec::new();
        let mut lines = content.lines().peekable();
        while let Some(line) = lines.next() {
            if let Some(language) = line.trim_start().strip_prefix("```") {
                let language = language.trim();
                out.extend(render_code_block(&mut lines, language, width, theme));
                continue;
            }
            if is_table_start(line, lines.peek().copied()) {
                let mut table = vec![line.to_string()];
                while let Some(next) = lines.peek().copied() {
                    if !next.contains('|') || next.trim().is_empty() {
                        break;
                    }
                    table.push(lines.next().unwrap_or_default().to_string());
                }
                out.extend(render_table(&table, width, theme));
                continue;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                out.push(String::new());
            } else if let Some(heading) = trimmed.strip_prefix("### ") {
                out.push(theme.paint(format!("▸ {heading}"), "heading"));
            } else if let Some(heading) = trimmed.strip_prefix("## ") {
                out.push(theme.paint(format!("◆ {heading}"), "heading"));
            } else if let Some(heading) = trimmed.strip_prefix("# ") {
                out.push(theme.paint(format!("■ {heading}"), "heading"));
            } else if let Some(item) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                for (index, visual) in wrap_text(item, width.saturating_sub(2))
                    .into_iter()
                    .enumerate()
                {
                    out.push(format!("{} {}", if index == 0 { "•" } else { " " }, visual));
                }
            } else {
                out.extend(wrap_text(trimmed, width));
            }
        }
        out
    }

    fn render_code_block<'a, I>(
        lines: &mut std::iter::Peekable<I>,
        language: &str,
        width: usize,
        theme: &ThemeRenderer,
    ) -> Vec<String>
    where
        I: Iterator<Item = &'a str>,
    {
        if matches!(language.trim(), "diff" | "patch") {
            return render_diff_block(lines, width, theme);
        }
        let label = if language.is_empty() {
            "code"
        } else {
            language
        };
        let inner = width.saturating_sub(4).max(8);
        let title = format!(" {label} ");
        let mut out = vec![theme.paint(
            format!(
                "┌{}{}",
                title,
                "─".repeat(inner.saturating_sub(title.chars().count()).max(1))
            ),
            "border",
        )];
        for line in lines.by_ref() {
            if line.trim_start().starts_with("```") {
                break;
            }
            if line.is_empty() {
                out.push(format!("{} ", theme.paint("│", "border")));
                continue;
            }
            for visual in wrap_code_line(line, inner) {
                out.push(format!(
                    "{} {}",
                    theme.paint("│", "border"),
                    syntax_highlight(&visual, language, theme)
                ));
            }
        }
        out.push(theme.paint(format!("└{}", "─".repeat(inner + 1)), "border"));
        out
    }

    fn render_diff_block<'a, I>(
        lines: &mut std::iter::Peekable<I>,
        width: usize,
        theme: &ThemeRenderer,
    ) -> Vec<String>
    where
        I: Iterator<Item = &'a str>,
    {
        let mut out = Vec::new();
        let mut old_line = None::<usize>;
        let mut new_line = None::<usize>;
        for line in lines.by_ref() {
            if line.trim_start().starts_with("```") {
                break;
            }
            if line.starts_with("@@") {
                let (old_start, new_start) = parse_hunk_header(line);
                old_line = old_start;
                new_line = new_start;
                out.push(theme.paint(truncate(line, width), "diff_hunk"));
                continue;
            }
            if line.starts_with("diff --git")
                || line.starts_with("index ")
                || line.starts_with("--- ")
                || line.starts_with("+++ ")
            {
                out.push(theme.paint(truncate(line, width), "diff_header"));
                continue;
            }
            let (number, marker, style) = if line.starts_with('-') {
                let number = old_line;
                old_line = old_line.map(|value| value + 1);
                (number, "-", Some("diff_del"))
            } else if line.starts_with('+') {
                let number = new_line;
                new_line = new_line.map(|value| value + 1);
                (number, "+", Some("diff_add"))
            } else {
                let number = new_line.or(old_line);
                old_line = old_line.map(|value| value + 1);
                new_line = new_line.map(|value| value + 1);
                (number, " ", None)
            };
            let content = line.strip_prefix(['-', '+', ' ']).unwrap_or(line);
            for (index, visual) in wrap_code_line(content, width.saturating_sub(9).max(8))
                .into_iter()
                .enumerate()
            {
                let gutter = if index == 0 {
                    format!(
                        "{:>5} {marker} ",
                        number.map_or(String::new(), |n| n.to_string())
                    )
                } else {
                    "      | ".to_string()
                };
                if let Some(style) = style {
                    let rendered = truncate(&format!("{gutter}{visual}"), width);
                    out.push(theme.paint(pad_to_width(&rendered, width), style));
                } else {
                    let rendered = format!(
                        "{}{}",
                        theme.paint(gutter, "diff_lineno"),
                        syntax_highlight(&visual, "rust", theme)
                    );
                    let rendered = truncate(&rendered, width);
                    out.push(rendered);
                }
            }
        }
        out
    }

    fn parse_hunk_header(line: &str) -> (Option<usize>, Option<usize>) {
        let mut old_start = None;
        let mut new_start = None;
        for part in line.split_whitespace() {
            if let Some(rest) = part.strip_prefix('-') {
                old_start = rest
                    .split(',')
                    .next()
                    .and_then(|value| value.parse::<usize>().ok());
            } else if let Some(rest) = part.strip_prefix('+') {
                new_start = rest
                    .split(',')
                    .next()
                    .and_then(|value| value.parse::<usize>().ok());
            }
        }
        (old_start, new_start)
    }

    fn wrap_code_line(text: &str, width: usize) -> Vec<String> {
        if text.chars().count() <= width {
            return vec![text.to_string()];
        }
        let chars = text.chars().collect::<Vec<_>>();
        chars
            .chunks(width)
            .map(|chunk| chunk.iter().collect::<String>())
            .collect()
    }

    fn syntax_highlight(line: &str, language: &str, theme: &ThemeRenderer) -> String {
        if !theme.theme.color_enabled {
            return line.to_string();
        }
        let language = normalize_language(language);
        let trimmed = line.trim_start();
        if is_comment_line(trimmed, language) {
            return theme.paint(line, "code_comment");
        }
        if matches!(language, "json" | "yaml" | "toml") {
            return highlight_json(line, theme);
        }
        highlight_keywords_and_strings(line, keywords_for_language(language), theme)
    }

    fn normalize_language(language: &str) -> &str {
        match language.trim().to_ascii_lowercase().as_str() {
            "rs" | "rust" => "rust",
            "csharp" | "cs" | "c#" => "csharp",
            "cpp" | "c++" | "cc" | "cxx" | "hpp" | "h++" => "cpp",
            "c" | "h" => "c",
            "java" => "java",
            "javascript" | "js" | "jsx" | "mjs" | "cjs" => "javascript",
            "typescript" | "ts" | "tsx" => "typescript",
            "python" | "py" | "python3" => "python",
            "go" | "golang" => "go",
            "kotlin" | "kt" | "kts" => "kotlin",
            "swift" => "swift",
            "php" => "php",
            "ruby" | "rb" => "ruby",
            "bash" | "sh" | "shell" | "zsh" => "shell",
            "sql" => "sql",
            "json" => "json",
            "yaml" | "yml" => "yaml",
            "toml" => "toml",
            "html" | "xml" | "tsx-html" => "html",
            "css" | "scss" | "sass" => "css",
            _ => "plain",
        }
    }

    fn is_comment_line(trimmed: &str, language: &str) -> bool {
        match language {
            "python" | "ruby" | "shell" | "yaml" | "toml" => trimmed.starts_with('#'),
            "sql" => trimmed.starts_with("--"),
            "html" => trimmed.starts_with("<!--"),
            "css" => trimmed.starts_with("/*"),
            _ => {
                trimmed.starts_with("//")
                    || trimmed.starts_with("/*")
                    || (language == "plain" && trimmed.starts_with('#'))
            }
        }
    }

    fn highlight_json(line: &str, theme: &ThemeRenderer) -> String {
        if let Some((key, rest)) = line.split_once(':') {
            if key.trim_start().starts_with('"') {
                return format!(
                    "{}:{}",
                    theme.paint(key, "code_keyword"),
                    theme.paint(rest, "code_string")
                );
            }
        }
        theme.paint(line, "code")
    }

    fn highlight_keywords_and_strings(
        line: &str,
        keywords: &[&str],
        theme: &ThemeRenderer,
    ) -> String {
        let mut out = String::new();
        let mut token = String::new();
        let mut string_quote: Option<char> = None;
        let mut escaped = false;
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            if string_quote.is_none() && ch == '/' && chars.peek() == Some(&'/') {
                if !token.is_empty() {
                    out.push_str(&paint_token(&token, keywords, theme));
                    token.clear();
                }
                let rest = format!("/{}", chars.collect::<String>());
                out.push_str(&theme.paint(rest, "code_comment"));
                break;
            }
            if string_quote.is_none() && ch == '#' && out.trim().is_empty() {
                if !token.is_empty() {
                    out.push_str(&paint_token(&token, keywords, theme));
                    token.clear();
                }
                out.push_str(&theme.paint(
                    format!("#{rest}", rest = chars.collect::<String>()),
                    "code_comment",
                ));
                break;
            }
            if ch == '"' || ch == '\'' {
                if !token.is_empty() {
                    out.push_str(&paint_token(&token, keywords, theme));
                    token.clear();
                }
                if string_quote == Some(ch) && !escaped {
                    string_quote = None;
                } else if string_quote.is_none() {
                    string_quote = Some(ch);
                }
                escaped = false;
                out.push_str(&theme.paint(ch.to_string(), "code_string"));
            } else if string_quote.is_some() {
                escaped = ch == '\\' && !escaped;
                out.push_str(&theme.paint(ch.to_string(), "code_string"));
            } else if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                token.push(ch);
            } else {
                if !token.is_empty() {
                    out.push_str(&paint_token(&token, keywords, theme));
                    token.clear();
                }
                out.push(ch);
            }
        }
        if !token.is_empty() {
            out.push_str(&paint_token(&token, &keywords, theme));
        }
        out
    }

    fn keywords_for_language(language: &str) -> &'static [&'static str] {
        match language {
            "rust" => &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
                "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
                "super", "trait", "true", "type", "unsafe", "use", "where", "while",
            ],
            "csharp" => &[
                "abstract",
                "as",
                "async",
                "await",
                "base",
                "bool",
                "break",
                "case",
                "catch",
                "class",
                "const",
                "continue",
                "decimal",
                "default",
                "delegate",
                "do",
                "double",
                "else",
                "enum",
                "event",
                "false",
                "finally",
                "fixed",
                "float",
                "for",
                "foreach",
                "if",
                "in",
                "int",
                "interface",
                "internal",
                "is",
                "lock",
                "namespace",
                "new",
                "null",
                "object",
                "out",
                "override",
                "private",
                "protected",
                "public",
                "readonly",
                "record",
                "ref",
                "return",
                "sealed",
                "static",
                "string",
                "struct",
                "switch",
                "this",
                "throw",
                "true",
                "try",
                "using",
                "var",
                "virtual",
                "void",
                "while",
                "yield",
            ],
            "cpp" | "c" => &[
                "alignas",
                "auto",
                "bool",
                "break",
                "case",
                "catch",
                "char",
                "class",
                "const",
                "constexpr",
                "continue",
                "delete",
                "do",
                "double",
                "else",
                "enum",
                "explicit",
                "extern",
                "false",
                "float",
                "for",
                "friend",
                "goto",
                "if",
                "inline",
                "int",
                "long",
                "namespace",
                "new",
                "nullptr",
                "private",
                "protected",
                "public",
                "return",
                "short",
                "signed",
                "sizeof",
                "static",
                "struct",
                "switch",
                "template",
                "this",
                "throw",
                "true",
                "try",
                "typedef",
                "typename",
                "union",
                "unsigned",
                "using",
                "virtual",
                "void",
                "volatile",
                "while",
            ],
            "java" => &[
                "abstract",
                "assert",
                "boolean",
                "break",
                "byte",
                "case",
                "catch",
                "char",
                "class",
                "const",
                "continue",
                "default",
                "do",
                "double",
                "else",
                "enum",
                "extends",
                "false",
                "final",
                "finally",
                "float",
                "for",
                "if",
                "implements",
                "import",
                "instanceof",
                "int",
                "interface",
                "long",
                "native",
                "new",
                "null",
                "package",
                "private",
                "protected",
                "public",
                "record",
                "return",
                "short",
                "static",
                "strictfp",
                "super",
                "switch",
                "synchronized",
                "this",
                "throw",
                "throws",
                "transient",
                "true",
                "try",
                "var",
                "void",
                "volatile",
                "while",
            ],
            "javascript" | "typescript" => &[
                "async",
                "await",
                "break",
                "case",
                "catch",
                "class",
                "const",
                "continue",
                "debugger",
                "default",
                "delete",
                "do",
                "else",
                "enum",
                "export",
                "extends",
                "false",
                "finally",
                "for",
                "from",
                "function",
                "get",
                "if",
                "implements",
                "import",
                "in",
                "instanceof",
                "interface",
                "let",
                "new",
                "null",
                "of",
                "private",
                "protected",
                "public",
                "readonly",
                "return",
                "set",
                "static",
                "super",
                "switch",
                "this",
                "throw",
                "true",
                "try",
                "type",
                "typeof",
                "undefined",
                "var",
                "void",
                "while",
                "yield",
            ],
            "python" => &[
                "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
                "del", "elif", "else", "except", "False", "finally", "for", "from", "global", "if",
                "import", "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise",
                "return", "True", "try", "while", "with", "yield",
            ],
            "go" => &[
                "break",
                "case",
                "chan",
                "const",
                "continue",
                "default",
                "defer",
                "else",
                "fallthrough",
                "for",
                "func",
                "go",
                "goto",
                "if",
                "import",
                "interface",
                "map",
                "package",
                "range",
                "return",
                "select",
                "struct",
                "switch",
                "type",
                "var",
            ],
            "kotlin" => &[
                "as",
                "break",
                "class",
                "continue",
                "data",
                "do",
                "else",
                "false",
                "for",
                "fun",
                "if",
                "in",
                "interface",
                "is",
                "null",
                "object",
                "package",
                "return",
                "super",
                "this",
                "throw",
                "true",
                "try",
                "typealias",
                "val",
                "var",
                "when",
                "while",
            ],
            "swift" => &[
                "as",
                "associatedtype",
                "break",
                "case",
                "catch",
                "class",
                "continue",
                "defer",
                "do",
                "else",
                "enum",
                "extension",
                "false",
                "for",
                "func",
                "guard",
                "if",
                "import",
                "in",
                "init",
                "let",
                "nil",
                "protocol",
                "return",
                "self",
                "Self",
                "static",
                "struct",
                "super",
                "switch",
                "throw",
                "true",
                "try",
                "typealias",
                "var",
                "where",
                "while",
            ],
            "php" => &[
                "abstract",
                "and",
                "array",
                "as",
                "break",
                "case",
                "catch",
                "class",
                "clone",
                "const",
                "continue",
                "declare",
                "default",
                "do",
                "echo",
                "else",
                "elseif",
                "extends",
                "false",
                "final",
                "finally",
                "fn",
                "for",
                "foreach",
                "function",
                "global",
                "if",
                "implements",
                "interface",
                "match",
                "namespace",
                "new",
                "null",
                "or",
                "private",
                "protected",
                "public",
                "return",
                "static",
                "switch",
                "throw",
                "trait",
                "true",
                "try",
                "use",
                "var",
                "while",
                "yield",
            ],
            "ruby" => &[
                "BEGIN", "END", "alias", "and", "begin", "break", "case", "class", "def",
                "defined?", "do", "else", "elsif", "end", "ensure", "false", "for", "if", "in",
                "module", "next", "nil", "not", "or", "redo", "rescue", "retry", "return", "self",
                "super", "then", "true", "undef", "unless", "until", "when", "while", "yield",
            ],
            "shell" => &[
                "case", "do", "done", "elif", "else", "esac", "export", "fi", "for", "function",
                "if", "in", "local", "readonly", "return", "select", "then", "until", "while",
            ],
            "sql" => &[
                "ALTER", "AND", "AS", "BEGIN", "BY", "CASE", "CREATE", "DELETE", "DROP", "ELSE",
                "END", "FROM", "GROUP", "HAVING", "IN", "INSERT", "INTO", "IS", "JOIN", "LEFT",
                "LIKE", "LIMIT", "NOT", "NULL", "ON", "OR", "ORDER", "RIGHT", "SELECT", "SET",
                "TABLE", "THEN", "UPDATE", "VALUES", "WHEN", "WHERE",
            ],
            "html" => &["html", "head", "body", "script", "style", "div", "span"],
            "css" => &["@media", "@keyframes", "display", "position"],
            _ => &[
                "async", "await", "class", "const", "def", "else", "false", "fn", "for", "from",
                "function", "if", "import", "let", "return", "true", "var", "while",
            ],
        }
    }

    fn paint_token(token: &str, keywords: &[&str], theme: &ThemeRenderer) -> String {
        if keywords.contains(&token) {
            theme.paint(token, "code_keyword")
        } else {
            theme.paint(token, "code")
        }
    }

    fn is_table_start(line: &str, next: Option<&str>) -> bool {
        line.contains('|') && next.map(is_table_separator).unwrap_or(false)
    }

    fn is_table_separator(line: &str) -> bool {
        let trimmed = line.trim().trim_matches('|').trim();
        !trimmed.is_empty()
            && trimmed
                .split('|')
                .all(|cell| cell.trim().chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
    }

    fn render_table(lines: &[String], width: usize, theme: &ThemeRenderer) -> Vec<String> {
        let rows = lines
            .iter()
            .filter(|line| !is_table_separator(line))
            .map(|line| {
                line.trim()
                    .trim_matches('|')
                    .split('|')
                    .map(|cell| cell.trim().to_string())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return Vec::new();
        }
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut widths = vec![0; columns];
        for row in &rows {
            for (index, cell) in row.iter().enumerate() {
                widths[index] = widths[index].max(cell.chars().count());
            }
        }
        let max_cell = width.saturating_sub(columns.saturating_mul(3)).max(6) / columns.max(1);
        for item in &mut widths {
            *item = (*item).min(max_cell.max(6));
        }
        rows.into_iter()
            .enumerate()
            .flat_map(|(row_index, row)| {
                let mut rendered = vec![format_table_row(&row, &widths, theme)];
                if row_index == 0 {
                    rendered.push(theme.paint(
                        format!(
                            "{}",
                            widths
                                .iter()
                                .map(|cell_width| "─".repeat(cell_width + 2))
                                .collect::<Vec<_>>()
                                .join("┼")
                        ),
                        "table",
                    ));
                }
                rendered
            })
            .collect()
    }

    fn format_table_row(row: &[String], widths: &[usize], theme: &ThemeRenderer) -> String {
        let cells = widths
            .iter()
            .enumerate()
            .map(|(index, width)| {
                let cell = row.get(index).map(String::as_str).unwrap_or("");
                format!(" {} ", pad_to_width(&truncate(cell, *width), *width))
            })
            .collect::<Vec<_>>();
        theme.paint(cells.join("│"), "table")
    }

    fn format_duration(milliseconds: u64) -> String {
        let total_seconds = ((milliseconds as f64) / 1000.0).round().max(0.0) as u64;
        format!("{:02}:{:02}", total_seconds / 60, total_seconds % 60)
    }

    #[allow(dead_code)]
    fn _category_preview_tools(items: &[ToolDefinition], width: usize) -> Vec<String> {
        let mut grouped: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for item in items.iter().filter(|item| item.enabled) {
            grouped.entry(&item.category).or_default().push(&item.name);
        }
        grouped
            .into_iter()
            .map(|(category, names)| {
                truncate(
                    &format!("  {category}: {}", names.join(", ")),
                    width.saturating_sub(6),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use crate::guardrails::ApprovalRequest;

    use super::layout::LayoutRenderer;

    #[test]
    fn layout_renderer_can_use_explicit_viewport() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let app = crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        let mut renderer = LayoutRenderer::default();
        renderer.viewport = Some((72, 24));

        let output = renderer.render_startup(
            &app.session,
            &app.commands,
            &app.input,
            &[],
            0,
            app.chat_scroll_offset,
            &[],
        );

        assert!(
            output
                .lines()
                .all(|line| super::layout::visible_width(line) <= 72)
        );
        Ok(())
    }

    #[test]
    fn layout_renderer_surfaces_pending_approval() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let app = crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        let mut renderer = LayoutRenderer::default();
        renderer.viewport = Some((96, 30));
        let approval = ApprovalRequest {
            id: "apr_test".to_string(),
            reason: "Risky tool requires human approval: write_file".to_string(),
            tool_name: "write_file".to_string(),
            args: Map::new(),
            risk_label: "write".to_string(),
        };

        let output = renderer.render_startup(
            &app.session,
            &app.commands,
            &app.input,
            &[],
            0,
            app.chat_scroll_offset,
            &[approval],
        );

        assert!(output.contains("approval required"));
        assert!(output.contains("[1] approve once and run"));
        assert!(output.contains("[2] allow for session and run"));
        assert!(output.contains("[3] deny"));
        assert!(output.contains("/approvals show apr_test"));
        Ok(())
    }
}
