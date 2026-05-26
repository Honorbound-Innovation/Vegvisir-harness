use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;
use std::sync::OnceLock;

use crate::{
    app::{DiffOverlay, InfoOverlay, TuiApplication},
    core::{Attachment, ChatMessage},
    guardrails::ApprovalRequest,
    ui::input::InputState,
};

const BG: Color = Color::Rgb(8, 9, 10);
const FG: Color = Color::Rgb(220, 220, 220);
const DIM: Color = Color::Rgb(105, 105, 112);
const GREEN: Color = Color::Rgb(88, 220, 120);
const CYAN: Color = Color::Rgb(80, 190, 220);
const AMBER: Color = Color::Rgb(220, 170, 65);
const RED: Color = Color::Rgb(230, 86, 86);
const BORDER: Color = Color::Rgb(62, 66, 76);
const PANEL: Color = Color::Rgb(16, 17, 20);
const CHAT_BOTTOM_GAP: u16 = 1;
const ACTIVITY_LABEL_WIDTH: usize = 18;

const FALLBACK_SPINNER_VERBS: &[&str] = &[
    "Architecting",
    "Brewing",
    "Calculating",
    "Channeling",
    "Cogitating",
    "Computing",
    "Conjuring",
    "Crafting",
    "Crunching",
    "Deciphering",
    "Forging",
    "Hyperspacing",
    "Ideating",
    "Inferring",
    "Noodling",
    "Orchestrating",
    "Percolating",
    "Pondering",
    "Processing",
    "Reasoning",
    "Synthesizing",
    "Thinking",
    "Tinkering",
    "Wrangling",
];

pub fn draw(f: &mut Frame<'_>, app: &mut TuiApplication) {
    let area = f.area();
    f.render_widget(Clear, area);
    let pending_approvals = pending_approvals(app);
    let pending = pending_approvals.first().cloned();
    let activity_height = activity_strip_height(app, pending.as_ref());
    let input_height = input_height(&app.input, area.width).min(10);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(CHAT_BOTTOM_GAP),
            Constraint::Length(activity_height),
            Constraint::Length(input_height),
            Constraint::Length(1),
        ])
        .split(area);

    draw_header(f, app, chunks[0]);
    draw_chat(f, app, chunks[1]);
    if activity_height > 0 {
        draw_activity_strip(f, app, pending.as_ref(), chunks[3]);
    }
    draw_input(f, app, chunks[4]);
    draw_status(f, app, chunks[5]);
    draw_suggestions(f, app, chunks[4], area);
    if app.help_overlay_open {
        draw_help_overlay(f, app, centered_rect(92, 24, area));
    }
    if let Some(diff) = app.diff_overlay.as_ref() {
        draw_diff_overlay(f, app, diff, centered_rect(120, 34, area));
    }
    if let Some(info) = app.info_overlay.as_ref() {
        draw_info_overlay(f, app, info, centered_rect(110, 30, area));
    }
    if app.search_open {
        draw_search_overlay(f, app, search_rect(area));
    }
    if !pending_approvals.is_empty() {
        draw_approval_modal(
            f,
            &pending_approvals,
            app.approval_selected_index,
            centered_rect(88, 14, area),
        );
    }
    set_input_cursor(f, app, chunks[4]);
}

fn activity_strip_height(app: &TuiApplication, pending: Option<&ApprovalRequest>) -> u16 {
    let has_activity = pending.is_some()
        || app.session.status == "streaming"
        || !app.session.activity.trim().is_empty();
    let has_context = !app.session.pending_attachments.is_empty();
    match (has_activity, has_context) {
        (true, true) => 4,
        (true, false) | (false, true) => 3,
        (false, false) => 0,
    }
}

fn pending_approvals(app: &TuiApplication) -> Vec<ApprovalRequest> {
    app.tool_executor
        .guardrails
        .approvals
        .pending()
        .into_values()
        .collect()
}

fn draw_header(f: &mut Frame<'_>, app: &TuiApplication, area: Rect) {
    let title = Span::styled(
        " Vegvisir ",
        Style::default().fg(FG).bg(BG).add_modifier(Modifier::BOLD),
    );
    let meta = format!(
        " provider={} model={} workspace={} ",
        app.session.current_provider, app.session.current_model, app.session.cwd
    );
    let line = Line::from(vec![title, Span::styled(meta, Style::default().fg(DIM))]);
    let paragraph = Paragraph::new(vec![line, Line::from("")])
        .style(Style::default().fg(FG).bg(BG))
        .block(Block::default().borders(Borders::BOTTOM).border_style(DIM));
    f.render_widget(paragraph, area);
}

fn draw_chat(f: &mut Frame<'_>, app: &TuiApplication, area: Rect) {
    let content_width = area.width.saturating_sub(2) as usize;
    let lines = visual_chat_lines(app, content_width);

    let visible_height = area.height.max(1) as usize;
    let max_offset = lines.len().saturating_sub(visible_height);
    let offset = app.chat_scroll_offset.min(max_offset);
    let end = lines.len().saturating_sub(offset);
    let start = end.saturating_sub(visible_height);
    let mut visible = lines[start..end].to_vec();
    apply_scroll_indicators(&mut visible, offset, max_offset, area.width as usize);
    let paragraph = Paragraph::new(visible)
        .style(Style::default().fg(FG).bg(BG))
        // Lines are pre-wrapped before viewport slicing. Keeping Ratatui wrapping
        // enabled is harmless as a terminal safety net, but the important part is
        // that scroll math is based on visual rows, not raw markdown/logical rows.
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn visual_chat_lines(app: &TuiApplication, width: usize) -> Vec<Line<'static>> {
    chat_lines(app, width)
        .into_iter()
        .flat_map(|line| wrap_line_for_viewport(line, width))
        .collect()
}

fn wrap_line_for_viewport(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if line.spans.is_empty() {
        return vec![line];
    }
    wrap_spans(line.spans, width.max(1), "")
}

fn chat_lines(app: &TuiApplication, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if app.session.messages.is_empty() {
        lines.push(Line::from(Span::styled(
            "Ready. Type a request, paste context, or use /help.",
            Style::default().fg(DIM),
        )));
    } else {
        for (index, message) in app.session.messages.iter().enumerate() {
            if index > 0 {
                lines.push(Line::from(""));
            }
            lines.extend(message_lines(message, width, &app.search_query));
        }
        if app
            .session
            .messages
            .last()
            .is_some_and(|message| message.role == "assistant" && !message.content.is_empty())
        {
            // Keep two rendered spacer rows after completed assistant output so the
            // final line is not visually clipped by the lower TUI chrome/input area.
            // This is a presentation-only pad; stored transcript content is unchanged.
            lines.push(Line::from(""));
            lines.push(Line::from(""));
        }
    }
    if app.session.status == "streaming" && !app.session.activity.trim().is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("● ", Style::default().fg(CYAN)),
            Span::styled(app.session.activity.clone(), Style::default().fg(DIM)),
        ]));
    }
    lines
}

fn apply_scroll_indicators(
    visible: &mut [Line<'static>],
    offset: usize,
    max_offset: usize,
    width: usize,
) {
    if visible.is_empty() || max_offset == 0 {
        return;
    }
    if offset < max_offset {
        visible[0] = Line::from(Span::styled(
            truncate("↑ older messages above", width),
            Style::default().fg(DIM),
        ));
    }
    if offset > 0
        && let Some(last) = visible.last_mut()
    {
        *last = Line::from(Span::styled(
            truncate("↓ newer messages below - press End to follow", width),
            Style::default().fg(AMBER),
        ));
    }
}

fn draw_activity_strip(
    f: &mut Frame<'_>,
    app: &TuiApplication,
    pending: Option<&ApprovalRequest>,
    area: Rect,
) {
    let mut lines = Vec::new();
    if let Some(line) = activity_line(app, pending, area.width as usize) {
        lines.push(line);
    }
    if let Some(line) = attachment_line(app, area.width as usize) {
        lines.push(line);
    }
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(FG).bg(BG))
        .block(
            Block::default()
                .borders(Borders::TOP | Borders::BOTTOM)
                .border_style(Style::default().fg(BORDER)),
        );
    f.render_widget(paragraph, area);
}

fn activity_line(
    app: &TuiApplication,
    pending: Option<&ApprovalRequest>,
    width: usize,
) -> Option<Line<'static>> {
    let (indicator, label, detail, color) = if let Some(approval) = pending {
        (
            "● ".to_string(),
            "waiting approval".to_string(),
            format!(
                "{} wants {}. Press 1 approve once, 2 allow session, 3 deny.",
                approval.risk_label, approval.tool_name
            ),
            AMBER,
        )
    } else if app.session.status == "streaming" {
        (
            streaming_spinner_dot(app.session.activity_tick),
            animated_spinner_verb(app),
            if app.session.activity.trim().is_empty() {
                "model response in progress".to_string()
            } else {
                app.session.activity.clone()
            },
            CYAN,
        )
    } else if !app.session.activity.trim().is_empty() {
        ("● ".to_string(), "activity".to_string(), app.session.activity.clone(), DIM)
    } else {
        return None;
    };

    let label = fixed_width(&label, ACTIVITY_LABEL_WIDTH);
    let reserved_width = indicator.width() + ACTIVITY_LABEL_WIDTH + 1;
    let detail_budget = width.saturating_sub(reserved_width);
    Some(Line::from(vec![
        Span::styled(
            indicator,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ", Style::default().fg(DIM)),
        Span::styled(truncate(&detail, detail_budget), Style::default().fg(FG)),
    ]))
}

fn fixed_width(text: &str, width: usize) -> String {
    let mut out = truncate(text, width);
    let current = out.width();
    if current < width {
        out.push_str(&" ".repeat(width - current));
    }
    out
}

fn streaming_spinner_dot(tick: u64) -> String {
    let frames = ["⠋ ", "⠙ ", "⠹ ", "⠸ ", "⠼ ", "⠴ ", "⠦ ", "⠧ ", "⠇ ", "⠏ "];
    frames[(tick as usize / 2) % frames.len()].to_string()
}

fn animated_spinner_verb(app: &TuiApplication) -> String {
    let verb = selected_spinner_verb(app);
    let suffix_frames = ["", ".", "..", "...", "..", "."];
    let suffix = suffix_frames[(app.session.activity_tick as usize / 6) % suffix_frames.len()];
    let shimmer = match (app.session.activity_tick / 4) % 4 {
        0 => "",
        1 => " ✦",
        2 => "",
        _ => " ✧",
    };
    format!("{verb}{suffix}{shimmer}")
}

fn selected_spinner_verb(app: &TuiApplication) -> String {
    let verbs = spinner_verbs();
    if verbs.is_empty() {
        return "Thinking".to_string();
    }
    let seed = if app.session.spinner_verb_seed == 0 {
        stable_hash(&app.session.session_id)
    } else {
        app.session.spinner_verb_seed
    };
    verbs[(seed as usize) % verbs.len()].clone()
}

fn spinner_verbs() -> &'static Vec<String> {
    static VERBS: OnceLock<Vec<String>> = OnceLock::new();
    VERBS.get_or_init(|| {
        let from_file = std::fs::read_to_string("spinner_verbs.md")
            .ok()
            .map(|content| parse_spinner_verbs(&content))
            .unwrap_or_default();
        if from_file.is_empty() {
            FALLBACK_SPINNER_VERBS
                .iter()
                .map(|verb| (*verb).to_string())
                .collect()
        } else {
            from_file
        }
    })
}

fn parse_spinner_verbs(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

fn stable_hash(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn attachment_line(app: &TuiApplication, width: usize) -> Option<Line<'static>> {
    if app.session.pending_attachments.is_empty() {
        return None;
    }
    let summary = pending_attachment_summary(&app.session.pending_attachments, width);
    Some(Line::from(vec![
        Span::styled("◇ ", Style::default().fg(CYAN)),
        Span::styled(
            "context",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ", Style::default().fg(DIM)),
        Span::styled(summary, Style::default().fg(FG)),
    ]))
}

fn pending_attachment_summary(attachments: &[Attachment], width: usize) -> String {
    let labels = attachments
        .iter()
        .take(4)
        .map(attachment_label)
        .collect::<Vec<_>>();
    let more = attachments.len().saturating_sub(labels.len());
    let mut summary = format!(
        "{} pending attachment{}: {}",
        attachments.len(),
        if attachments.len() == 1 { "" } else { "s" },
        labels.join(", ")
    );
    if more > 0 {
        summary.push_str(&format!(", +{more} more"));
    }
    truncate(&summary, width.saturating_sub(14))
}

fn attachment_label(attachment: &Attachment) -> String {
    let name = attachment.name.as_deref().unwrap_or_else(|| {
        std::path::Path::new(&attachment.path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&attachment.path)
    });
    match attachment.size_bytes {
        Some(bytes) => format!("{} {} ({})", attachment.kind, name, format_bytes(bytes)),
        None => format!("{} {}", attachment.kind, name),
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
fn work_log_lines(app: &TuiApplication, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if app.pending_send.is_some() {
        lines.push(work_log_line(
            "running",
            "model response in progress",
            CYAN,
            width,
        ));
    }
    if !app.session.activity.trim().is_empty() {
        lines.push(work_log_line(
            "activity",
            &app.session.activity,
            CYAN,
            width,
        ));
    }
    let pending = app.tool_executor.guardrails.approvals.pending();
    for approval in pending.values().take(3) {
        lines.push(work_log_line(
            "approval",
            &format!("{} {}", approval.tool_name, approval.risk_label),
            AMBER,
            width,
        ));
    }
    for message in app.session.messages.iter().rev().take(8).rev() {
        match message.role.as_str() {
            "system" => {
                let label = if message.content.to_ascii_lowercase().contains("error") {
                    "error"
                } else {
                    "note"
                };
                let color = if label == "error" { RED } else { AMBER };
                lines.push(work_log_line(label, &message.content, color, width));
            }
            "user" => lines.push(work_log_line("user", &message.content, CYAN, width)),
            "assistant" => {
                if message.content.trim().is_empty() {
                    lines.push(work_log_line("agent", "stream opened", GREEN, width));
                }
            }
            _ => {}
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No recent work log entries.",
            Style::default().fg(DIM),
        )));
    }
    lines
}

#[cfg(test)]
fn work_log_line(label: &str, detail: &str, color: Color, width: usize) -> Line<'static> {
    let detail_width = width.saturating_sub(label.len() + 5).max(8);
    Line::from(vec![
        Span::styled("• ", Style::default().fg(color)),
        Span::styled(
            format!("{label:<8}"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            truncate(&detail.replace('\n', " "), detail_width),
            Style::default().fg(DIM),
        ),
    ])
}

fn message_lines(message: &ChatMessage, width: usize, search_query: &str) -> Vec<Line<'static>> {
    let system_kind = if message.role == "system" {
        Some(classify_system_message(&message.content))
    } else {
        None
    };
    let (marker, style) = match (message.role.as_str(), system_kind) {
        ("user", _) => ("❯", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        ("assistant", _) => ("●", Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
        ("system", Some(SystemMessageKind::Error)) => {
            ("!", Style::default().fg(RED).add_modifier(Modifier::BOLD))
        }
        ("system", Some(SystemMessageKind::Approval)) => {
            ("?", Style::default().fg(AMBER).add_modifier(Modifier::BOLD))
        }
        ("system", Some(SystemMessageKind::Tool)) => {
            ("›", Style::default().fg(CYAN).add_modifier(Modifier::BOLD))
        }
        ("system", _) => ("·", Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
        _ => ("·", Style::default().fg(DIM)),
    };
    let content_style = match message.role.as_str() {
        "system" => system_kind
            .map(SystemMessageKind::content_style)
            .unwrap_or_else(|| Style::default().fg(AMBER)),
        _ => Style::default().fg(FG),
    };
    let role_label = message_label(message.role.as_str(), system_kind);
    let timestamp = message.created_at.format("%H:%M:%S").to_string();
    let is_match = message_matches_search(message, search_query);

    if message.role == "system"
        && matches!(
            system_kind,
            Some(SystemMessageKind::Tool | SystemMessageKind::Note)
        )
    {
        return vec![compact_system_message_line(
            marker,
            role_label,
            &timestamp,
            &message.content,
            style,
            content_style,
            is_match,
            width,
        )];
    }

    let mut header_spans = vec![
        Span::styled(format!("{marker} "), style),
        Span::styled(role_label.to_string(), style),
        Span::styled(" ", Style::default().fg(DIM)),
        Span::styled(timestamp, Style::default().fg(DIM)),
    ];
    if is_match {
        header_spans.push(Span::styled("  match", Style::default().fg(AMBER)));
    }
    let header = Line::from(header_spans);
    let rendered = render_markdown(
        &message.content,
        width.saturating_sub(2).max(10),
        content_style,
    );
    let mut out = vec![header];
    for line in rendered {
        let mut spans = vec![Span::raw("  ".to_string())];
        spans.extend(line.spans);
        out.push(Line::from(spans));
    }
    out
}

fn compact_system_message_line(
    marker: &str,
    role_label: &str,
    timestamp: &str,
    content: &str,
    marker_style: Style,
    content_style: Style,
    is_match: bool,
    width: usize,
) -> Line<'static> {
    let prefix_width = marker.width() + 1 + role_label.width() + 2 + timestamp.width() + 2;
    let match_width = if is_match { " match".width() } else { 0 };
    let detail_width = width.saturating_sub(prefix_width + match_width).max(12);
    let detail = summarize_system_inline(content, detail_width);
    let mut spans = vec![
        Span::styled(format!("{marker} "), marker_style),
        Span::styled(role_label.to_string(), marker_style),
        Span::styled(" ", Style::default().fg(DIM)),
        Span::styled(timestamp.to_string(), Style::default().fg(DIM)),
        Span::styled(" ", Style::default().fg(DIM)),
        Span::styled(detail, content_style),
    ];
    if is_match {
        spans.push(Span::styled(" match", Style::default().fg(AMBER)));
    }
    Line::from(spans)
}

fn summarize_system_inline(content: &str, width: usize) -> String {
    let normalized = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("  ");
    truncate(&normalized, width)
}

fn message_matches_search(message: &ChatMessage, search_query: &str) -> bool {
    let query = search_query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return false;
    }
    message.role.to_ascii_lowercase().contains(&query)
        || message.content.to_ascii_lowercase().contains(&query)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SystemMessageKind {
    Note,
    Error,
    Approval,
    Tool,
}

impl SystemMessageKind {
    fn content_style(self) -> Style {
        match self {
            Self::Error => Style::default().fg(RED),
            Self::Approval => Style::default().fg(AMBER),
            Self::Tool => Style::default().fg(CYAN),
            Self::Note => Style::default().fg(AMBER),
        }
    }
}

fn message_label(role: &str, system_kind: Option<SystemMessageKind>) -> &str {
    match role {
        "user" => "you",
        "assistant" => "agent",
        "system" => match system_kind.unwrap_or(SystemMessageKind::Note) {
            SystemMessageKind::Error => "error",
            SystemMessageKind::Approval => "approval",
            SystemMessageKind::Tool => "tool",
            SystemMessageKind::Note => "note",
        },
        other => other,
    }
}

fn classify_system_message(content: &str) -> SystemMessageKind {
    let lower = content.to_ascii_lowercase();
    if lower.starts_with("error:")
        || lower.contains(" failed")
        || lower.contains("denied")
        || lower.contains("not found")
        || lower.contains("exceeded")
    {
        return SystemMessageKind::Error;
    }
    if lower.contains("approval")
        || lower.contains("approve")
        || lower.contains("risky tool")
        || lower.contains("approval_id=")
    {
        return SystemMessageKind::Approval;
    }
    if lower.starts_with("tool ")
        || lower.starts_with("command ")
        || lower.contains("tool call")
        || lower.contains("running:")
        || lower.contains("exit code")
    {
        return SystemMessageKind::Tool;
    }
    SystemMessageKind::Note
}

fn render_markdown(text: &str, width: usize, base_style: Style) -> Vec<Line<'static>> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(text, options);
    let mut out = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut strong = false;
    let mut emphasis = false;
    let mut list_depth = 0usize;
    let mut heading_level = 0usize;
    let mut in_code = false;
    let mut code_language = String::new();
    let mut code = String::new();
    let mut table = TableRenderState::default();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    heading_level = level as usize;
                }
                Tag::Strong => strong = true,
                Tag::Emphasis => emphasis = true,
                Tag::List(_) => list_depth += 1,
                Tag::Item => {
                    flush_current(&mut out, &mut current, width, "");
                    current.push(Span::styled(
                        format!("{}- ", "  ".repeat(list_depth.saturating_sub(1))),
                        Style::default().fg(CYAN),
                    ));
                }
                Tag::CodeBlock(kind) => {
                    flush_current(&mut out, &mut current, width, "");
                    in_code = true;
                    code_language = match kind {
                        CodeBlockKind::Fenced(language) => language.to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    code.clear();
                }
                Tag::BlockQuote(_) => {
                    flush_current(&mut out, &mut current, width, "");
                    current.push(Span::styled("> ", Style::default().fg(DIM)));
                }
                Tag::Table(_) => {
                    flush_current(&mut out, &mut current, width, "");
                    table = TableRenderState {
                        in_table: true,
                        ..Default::default()
                    };
                }
                Tag::TableHead => table.in_header = true,
                Tag::TableRow => table.current_row.clear(),
                Tag::TableCell => table.current_cell.clear(),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => {
                    let prefix = match heading_level {
                        1 => "# ",
                        2 => "## ",
                        3 => "### ",
                        _ => "",
                    };
                    if !prefix.is_empty() {
                        current.insert(0, Span::styled(prefix, Style::default().fg(CYAN)));
                    }
                    for span in &mut current {
                        span.style = span.style.add_modifier(Modifier::BOLD);
                    }
                    flush_current(&mut out, &mut current, width, "");
                    out.push(Line::from(""));
                    heading_level = 0;
                }
                TagEnd::Strong => strong = false,
                TagEnd::Emphasis => emphasis = false,
                TagEnd::Paragraph | TagEnd::Item => {
                    flush_current(&mut out, &mut current, width, "")
                }
                TagEnd::List(_) => {
                    list_depth = list_depth.saturating_sub(1);
                    if list_depth == 0 {
                        out.push(Line::from(""));
                    }
                }
                TagEnd::CodeBlock => {
                    render_code_block(&mut out, &code_language, &code, width);
                    in_code = false;
                    code_language.clear();
                    code.clear();
                }
                TagEnd::BlockQuote(_) => {
                    flush_current(&mut out, &mut current, width, "");
                    out.push(Line::from(""));
                }
                TagEnd::TableCell => table
                    .current_row
                    .push(std::mem::take(&mut table.current_cell)),
                TagEnd::TableHead => {
                    table.headers = std::mem::take(&mut table.current_row);
                    table.in_header = false;
                }
                TagEnd::TableRow => {
                    if table.in_header {
                        table.headers = std::mem::take(&mut table.current_row);
                    } else if !table.current_row.is_empty() {
                        table.rows.push(std::mem::take(&mut table.current_row));
                    }
                }
                TagEnd::Table => {
                    render_table(&mut out, &table, width);
                    table = TableRenderState::default();
                    out.push(Line::from(""));
                }
                _ => {}
            },
            Event::Text(value) => {
                let value = value.to_string();
                if in_code {
                    code.push_str(&value);
                } else if table.in_table {
                    table.current_cell.push_str(&value);
                } else {
                    let mut style = base_style;
                    if strong {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    if emphasis {
                        style = style.add_modifier(Modifier::ITALIC);
                    }
                    current.push(Span::styled(value, style));
                }
            }
            Event::Code(value) => current.push(Span::styled(
                value.to_string(),
                Style::default().fg(CYAN).bg(PANEL),
            )),
            Event::SoftBreak | Event::HardBreak => flush_current(&mut out, &mut current, width, ""),
            Event::Rule => out.push(Line::from(Span::styled(
                "─".repeat(width),
                Style::default().fg(BORDER),
            ))),
            _ => {}
        }
    }
    flush_current(&mut out, &mut current, width, "");
    if out.is_empty() {
        out.push(Line::from(""));
    }
    while out.last().is_some_and(|line| {
        line.spans.is_empty() || line.spans.iter().all(|span| span.content.is_empty())
    }) {
        out.pop();
    }
    if out.is_empty() {
        out.push(Line::from(""));
    }
    out
}

#[derive(Default)]
struct TableRenderState {
    in_table: bool,
    in_header: bool,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
}

fn flush_current(
    out: &mut Vec<Line<'static>>,
    current: &mut Vec<Span<'static>>,
    width: usize,
    continuation_prefix: &str,
) {
    if current.is_empty() {
        return;
    }
    out.extend(wrap_spans(
        std::mem::take(current),
        width,
        continuation_prefix,
    ));
}

fn wrap_spans(
    spans: Vec<Span<'static>>,
    width: usize,
    continuation_prefix: &str,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;
    let continuation_width = continuation_prefix.width();
    for span in spans {
        let style = span.style;
        for piece in split_preserving_spaces(span.content.as_ref()) {
            let piece_width = piece.width();
            if current_width > 0 && current_width + piece_width > width {
                rows.push(Line::from(std::mem::take(&mut current)));
                current_width = 0;
                if !continuation_prefix.is_empty() {
                    current.push(Span::raw(continuation_prefix.to_string()));
                    current_width = continuation_width;
                }
            }
            if piece_width > width && current_width == 0 {
                for chunk in wrap_preserve(&piece, width) {
                    rows.push(Line::from(Span::styled(chunk, style)));
                }
                continue;
            }
            current.push(Span::styled(piece, style));
            current_width += piece_width;
        }
    }
    if !current.is_empty() {
        rows.push(Line::from(current));
    }
    rows
}

fn split_preserving_spaces(text: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut was_space = None::<bool>;
    for ch in text.chars() {
        let is_space = ch.is_whitespace();
        if let Some(previous) = was_space
            && previous != is_space
            && !current.is_empty()
        {
            pieces.push(std::mem::take(&mut current));
        }
        current.push(ch);
        was_space = Some(is_space);
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    pieces
}

fn render_code_block(out: &mut Vec<Line<'static>>, language: &str, code: &str, width: usize) {
    let language = language.trim();
    let title = if language.is_empty() {
        " code ".to_string()
    } else {
        format!(" {language} ")
    };
    out.push(Line::from(vec![
        Span::styled("╭─", Style::default().fg(BORDER)),
        Span::styled(
            title,
            Style::default()
                .fg(CYAN)
                .bg(PANEL)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "─".repeat(width.saturating_sub(language.len() + 4)),
            Style::default().fg(BORDER),
        ),
    ]));
    let is_diff = matches!(language, "diff" | "patch");
    for (index, raw) in code.lines().enumerate() {
        let line_style = if is_diff {
            diff_line_style(raw)
        } else {
            code_line_style(language, raw)
        };
        let gutter = format!("{:>4} │ ", index + 1);
        for (wrap_index, piece) in wrap_preserve(raw, width.saturating_sub(7).max(1))
            .into_iter()
            .enumerate()
        {
            let prefix = if wrap_index == 0 {
                gutter.clone()
            } else {
                "     │ ".to_string()
            };
            let mut spans = vec![Span::styled(prefix, Style::default().fg(DIM).bg(PANEL))];
            spans.extend(highlight_code_piece(language, &piece, line_style));
            out.push(Line::from(spans));
        }
    }
    out.push(Line::from(Span::styled(
        "╰────",
        Style::default().fg(BORDER),
    )));
    out.push(Line::from(""));
}

fn diff_line_style(line: &str) -> Style {
    if line.starts_with('+') && !line.starts_with("+++") {
        Style::default().fg(GREEN)
    } else if line.starts_with('-') && !line.starts_with("---") {
        Style::default().fg(RED)
    } else if line.starts_with("@@") {
        Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(185, 190, 200))
    }
}

fn code_line_style(language: &str, line: &str) -> Style {
    let trimmed = line.trim_start();
    if matches!(
        language,
        "rust"
            | "rs"
            | "python"
            | "py"
            | "typescript"
            | "ts"
            | "javascript"
            | "js"
            | "java"
            | "csharp"
            | "cs"
            | "cpp"
            | "c++"
            | "c"
    ) {
        if trimmed.starts_with("//") || trimmed.starts_with('#') {
            return Style::default().fg(DIM).add_modifier(Modifier::ITALIC);
        }
        if trimmed.starts_with("fn ")
            || trimmed.starts_with("def ")
            || trimmed.starts_with("class ")
            || trimmed.starts_with("function ")
            || trimmed.starts_with("public ")
            || trimmed.starts_with("private ")
        {
            return Style::default().fg(CYAN);
        }
        if trimmed.starts_with("use ")
            || trimmed.starts_with("import ")
            || trimmed.starts_with("from ")
            || trimmed.starts_with("#include")
            || trimmed.starts_with("using ")
        {
            return Style::default().fg(AMBER);
        }
    }
    Style::default().fg(Color::Rgb(185, 190, 200))
}

fn highlight_code_piece(language: &str, piece: &str, fallback: Style) -> Vec<Span<'static>> {
    let fallback = fallback.bg(PANEL);
    if matches!(language, "diff" | "patch") {
        return vec![Span::styled(piece.to_string(), fallback)];
    }
    if matches!(language, "json" | "jsonc") {
        return highlight_json_piece(piece, fallback);
    }
    if !is_code_language(language) {
        return vec![Span::styled(piece.to_string(), fallback)];
    }
    let trimmed = piece.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with('#') {
        return vec![Span::styled(
            piece.to_string(),
            Style::default()
                .fg(DIM)
                .bg(PANEL)
                .add_modifier(Modifier::ITALIC),
        )];
    }
    highlight_language_piece(piece, fallback)
}

fn is_code_language(language: &str) -> bool {
    matches!(
        language,
        "rust"
            | "rs"
            | "python"
            | "py"
            | "typescript"
            | "ts"
            | "tsx"
            | "javascript"
            | "js"
            | "jsx"
            | "java"
            | "csharp"
            | "cs"
            | "cpp"
            | "c++"
            | "c"
            | "go"
            | "html"
            | "css"
            | "toml"
            | "yaml"
            | "yml"
            | "bash"
            | "sh"
            | "shell"
    )
}

fn highlight_language_piece(piece: &str, fallback: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut chars = piece.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if ch == '"' || ch == '\'' || ch == '`' {
            let quote = ch;
            let mut end = start + ch.len_utf8();
            let mut escaped = false;
            for (idx, next) in chars.by_ref() {
                end = idx + next.len_utf8();
                if escaped {
                    escaped = false;
                } else if next == '\\' {
                    escaped = true;
                } else if next == quote {
                    break;
                }
            }
            spans.push(Span::styled(
                piece[start..end].to_string(),
                Style::default().fg(GREEN).bg(PANEL),
            ));
        } else if ch.is_ascii_digit() {
            let mut end = start + ch.len_utf8();
            while let Some((idx, next)) = chars.peek().copied() {
                if next.is_ascii_digit() || next == '.' || next == '_' {
                    chars.next();
                    end = idx + next.len_utf8();
                } else {
                    break;
                }
            }
            spans.push(Span::styled(
                piece[start..end].to_string(),
                Style::default().fg(AMBER).bg(PANEL),
            ));
        } else if is_ident_start(ch) {
            let mut end = start + ch.len_utf8();
            while let Some((idx, next)) = chars.peek().copied() {
                if is_ident_continue(next) {
                    chars.next();
                    end = idx + next.len_utf8();
                } else {
                    break;
                }
            }
            let token = &piece[start..end];
            let style = if is_keyword(token) {
                Style::default()
                    .fg(CYAN)
                    .bg(PANEL)
                    .add_modifier(Modifier::BOLD)
            } else if is_literal(token) {
                Style::default().fg(AMBER).bg(PANEL)
            } else {
                fallback
            };
            spans.push(Span::styled(token.to_string(), style));
        } else {
            spans.push(Span::styled(ch.to_string(), fallback));
        }
    }
    spans
}

fn highlight_json_piece(piece: &str, fallback: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut chars = piece.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if ch == '"' {
            let mut end = start + 1;
            let mut escaped = false;
            for (idx, next) in chars.by_ref() {
                end = idx + next.len_utf8();
                if escaped {
                    escaped = false;
                } else if next == '\\' {
                    escaped = true;
                } else if next == '"' {
                    break;
                }
            }
            let rest = piece[end..].trim_start();
            let style = if rest.starts_with(':') {
                Style::default().fg(CYAN).bg(PANEL)
            } else {
                Style::default().fg(GREEN).bg(PANEL)
            };
            spans.push(Span::styled(piece[start..end].to_string(), style));
        } else if ch.is_ascii_digit() || ch == '-' {
            let mut end = start + ch.len_utf8();
            while let Some((idx, next)) = chars.peek().copied() {
                if next.is_ascii_digit() || matches!(next, '.' | 'e' | 'E' | '+' | '-') {
                    chars.next();
                    end = idx + next.len_utf8();
                } else {
                    break;
                }
            }
            spans.push(Span::styled(
                piece[start..end].to_string(),
                Style::default().fg(AMBER).bg(PANEL),
            ));
        } else if is_ident_start(ch) {
            let mut end = start + ch.len_utf8();
            while let Some((idx, next)) = chars.peek().copied() {
                if is_ident_continue(next) {
                    chars.next();
                    end = idx + next.len_utf8();
                } else {
                    break;
                }
            }
            let token = &piece[start..end];
            let style = if matches!(token, "true" | "false" | "null") {
                Style::default().fg(AMBER).bg(PANEL)
            } else {
                fallback
            };
            spans.push(Span::styled(token.to_string(), style));
        } else {
            spans.push(Span::styled(ch.to_string(), fallback));
        }
    }
    spans
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn is_keyword(token: &str) -> bool {
    matches!(
        token,
        "as" | "async"
            | "await"
            | "break"
            | "case"
            | "class"
            | "const"
            | "continue"
            | "def"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "fn"
            | "for"
            | "from"
            | "function"
            | "if"
            | "impl"
            | "import"
            | "in"
            | "interface"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "mut"
            | "new"
            | "private"
            | "pub"
            | "public"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "switch"
            | "this"
            | "trait"
            | "true"
            | "type"
            | "use"
            | "var"
            | "while"
    )
}

fn is_literal(token: &str) -> bool {
    matches!(
        token,
        "true" | "false" | "null" | "None" | "Some" | "Ok" | "Err"
    )
}

fn render_table(out: &mut Vec<Line<'static>>, table: &TableRenderState, width: usize) {
    if table.headers.is_empty() && table.rows.is_empty() {
        return;
    }
    let columns = table
        .headers
        .len()
        .max(table.rows.iter().map(Vec::len).max().unwrap_or(0));
    if columns == 0 {
        return;
    }
    let max_col_width = (width / columns).saturating_sub(3).clamp(8, 28);
    let header = (0..columns)
        .map(|index| {
            truncate(
                table.headers.get(index).map(String::as_str).unwrap_or(""),
                max_col_width,
            )
        })
        .collect::<Vec<_>>();
    out.push(table_line(
        &header,
        max_col_width,
        Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
    ));
    out.push(Line::from(Span::styled(
        "─".repeat(width.min(columns * (max_col_width + 3))),
        Style::default().fg(BORDER),
    )));
    for row in &table.rows {
        let cells = (0..columns)
            .map(|index| {
                truncate(
                    row.get(index).map(String::as_str).unwrap_or(""),
                    max_col_width,
                )
            })
            .collect::<Vec<_>>();
        out.push(table_line(&cells, max_col_width, Style::default().fg(FG)));
    }
}

fn table_line(cells: &[String], width: usize, style: Style) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(BORDER)));
        }
        spans.push(Span::styled(format!("{cell:<width$}"), style));
    }
    Line::from(spans)
}

fn draw_input(f: &mut Frame<'_>, app: &TuiApplication, area: Rect) {
    let focused = app.session.status != "streaming";
    let title = if app.input.buffer.starts_with('/') {
        " command "
    } else {
        " message "
    };
    let border_style = if focused { CYAN } else { BORDER };
    let max_rows = area.height.saturating_sub(2).max(1) as usize;
    let lines = input_lines(&app.input, area.width.saturating_sub(4) as usize, max_rows);
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(FG).bg(PANEL))
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_style))
                .padding(Padding::horizontal(1)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn draw_suggestions(f: &mut Frame<'_>, app: &TuiApplication, input_area: Rect, screen: Rect) {
    if !app.input.buffer.starts_with('/') || app.input.suggestions.is_empty() {
        return;
    }
    if app.command_palette_open {
        draw_command_palette(f, app, screen);
        return;
    }
    let count = app.input.suggestions.len().min(8) as u16;
    let width = input_area.width.min(92).max(30);
    let height = count + 2;
    let x = input_area.x;
    let y = input_area.y.saturating_sub(height);
    let area = Rect {
        x,
        y,
        width,
        height: height.min(input_area.y.saturating_add(input_area.height)),
    };
    f.render_widget(Clear, area);
    let lines = app
        .input
        .suggestions
        .iter()
        .take(count as usize)
        .enumerate()
        .map(|(index, suggestion)| {
            let selected = index == app.input.selected_suggestion;
            let style = if selected {
                Style::default().fg(Color::Black).bg(CYAN)
            } else {
                Style::default().fg(FG).bg(PANEL)
            };
            let value = suggestion
                .replacement
                .as_deref()
                .unwrap_or(&suggestion.value);
            Line::from(vec![
                Span::styled(format!(" {:<22}", truncate(value, 22)), style),
                Span::styled(
                    truncate(&suggestion.description, width.saturating_sub(26) as usize),
                    style.fg(if selected { Color::Black } else { DIM }),
                ),
            ])
        })
        .collect::<Vec<_>>();
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .title(" commands ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER)),
    );
    f.render_widget(paragraph, area);
}

fn draw_help_overlay(f: &mut Frame<'_>, app: &TuiApplication, area: Rect) {
    f.render_widget(Clear, area);
    let lines = help_overlay_lines(
        app,
        area.width.saturating_sub(4) as usize,
        area.height as usize,
    );
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(FG).bg(PANEL))
        .block(
            Block::default()
                .title(" help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(CYAN))
                .padding(Padding::horizontal(1)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn help_overlay_lines(app: &TuiApplication, width: usize, height: usize) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "Vegvisir controls",
                Style::default().fg(FG).add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            Span::styled("Esc or ? closes", Style::default().fg(DIM)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Enter", Style::default().fg(CYAN)),
            Span::styled(" send/run   ", Style::default().fg(DIM)),
            Span::styled("Shift+Enter", Style::default().fg(CYAN)),
            Span::styled(" newline   ", Style::default().fg(DIM)),
            Span::styled("Ctrl+P", Style::default().fg(CYAN)),
            Span::styled(" commands", Style::default().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled("PgUp/PgDn", Style::default().fg(CYAN)),
            Span::styled(" scroll chat   ", Style::default().fg(DIM)),
            Span::styled("End", Style::default().fg(CYAN)),
            Span::styled(" follow live output   ", Style::default().fg(DIM)),
            Span::styled("Ctrl+F", Style::default().fg(CYAN)),
            Span::styled(" search   ", Style::default().fg(DIM)),
            Span::styled("Ctrl+C", Style::default().fg(CYAN)),
            Span::styled(" quit", Style::default().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled("1", Style::default().fg(GREEN)),
            Span::styled(" approve once   ", Style::default().fg(DIM)),
            Span::styled("2", Style::default().fg(CYAN)),
            Span::styled(" allow session   ", Style::default().fg(DIM)),
            Span::styled("3", Style::default().fg(RED)),
            Span::styled(" deny pending tool", Style::default().fg(DIM)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Common commands",
            Style::default().fg(FG).add_modifier(Modifier::BOLD),
        )),
    ];
    let common = [
        ("/models", "list or refresh models for the active provider"),
        ("/provider", "switch provider or inspect current provider"),
        ("/workspace", "switch project workspace and session context"),
        ("/approvals", "inspect or resolve pending tool approvals"),
        ("/work", "open recent work, command, and tool activity"),
        ("/context", "inspect current context and memory use"),
        ("/tools", "show or change available tool permissions"),
    ];
    for (name, description) in common {
        push_help_command_line(&mut lines, name, description, width);
    }
    if lines.len() < height.saturating_sub(1) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Command inventory",
            Style::default().fg(FG).add_modifier(Modifier::BOLD),
        )));
    }
    let remaining = height.saturating_sub(lines.len() + 1);
    for command in app.commands.all().into_iter().take(remaining) {
        push_help_command_line(&mut lines, &command.name, &command.description, width);
    }
    lines
}

fn push_help_command_line(
    lines: &mut Vec<Line<'static>>,
    name: &str,
    description: &str,
    width: usize,
) {
    let name_width = 18usize;
    let desc_width = width.saturating_sub(name_width + 3).max(8);
    lines.push(Line::from(vec![
        Span::styled(
            format!("{:<name_width$}", truncate(name, name_width)),
            Style::default().fg(CYAN),
        ),
        Span::styled(" ", Style::default().fg(DIM)),
        Span::styled(truncate(description, desc_width), Style::default().fg(DIM)),
    ]));
}

fn draw_diff_overlay(f: &mut Frame<'_>, app: &TuiApplication, diff: &DiffOverlay, area: Rect) {
    f.render_widget(Clear, area);
    let visible_height = area.height.saturating_sub(6).max(1) as usize;
    let content_width = area.width.saturating_sub(4) as usize;
    let all_lines = diff_overlay_lines(diff, content_width);
    let max_offset = all_lines.len().saturating_sub(visible_height);
    let offset = app.diff_scroll_offset.min(max_offset);
    let end = all_lines.len().saturating_sub(offset);
    let start = end.saturating_sub(visible_height);
    let mut visible = all_lines[start..end].to_vec();
    apply_scroll_indicators(&mut visible, offset, max_offset, content_width);

    let summary = format!(
        " {} file{}  +{} -{}   PgUp/PgDn scroll   End follow   Esc close ",
        diff.files_changed,
        if diff.files_changed == 1 { "" } else { "s" },
        diff.added_lines,
        diff.removed_lines
    );
    let paragraph = Paragraph::new(visible)
        .style(Style::default().fg(FG).bg(PANEL))
        .block(
            Block::default()
                .title(format!(" review: {} ", diff.title))
                .title_bottom(summary)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(CYAN))
                .padding(Padding::horizontal(1)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn diff_overlay_lines(diff: &DiffOverlay, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut current_file: Option<String> = None;
    for raw in diff.diff.lines() {
        if let Some(file) = parse_diff_file(raw) {
            current_file = Some(file.clone());
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("file ", Style::default().fg(DIM)),
                Span::styled(file, Style::default().fg(FG).add_modifier(Modifier::BOLD)),
            ]));
            continue;
        }
        if raw.starts_with("@@") {
            lines.push(Line::from(Span::styled(
                truncate(raw, width),
                Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if raw.starts_with("+++ ") || raw.starts_with("--- ") {
            continue;
        }
        if raw.starts_with('+') {
            push_wrapped_diff_line(&mut lines, raw, width, "+", GREEN);
        } else if raw.starts_with('-') {
            push_wrapped_diff_line(&mut lines, raw, width, "-", RED);
        } else if raw.starts_with("diff --git ") {
            if current_file.is_none() {
                lines.push(Line::from(Span::styled(
                    truncate(raw, width),
                    Style::default().fg(DIM),
                )));
            }
        } else {
            push_wrapped_diff_line(&mut lines, raw, width, " ", Color::Rgb(185, 190, 200));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No diff content.",
            Style::default().fg(DIM),
        )));
    }
    lines
}

fn parse_diff_file(line: &str) -> Option<String> {
    let rest = line.strip_prefix("diff --git ")?;
    let mut parts = rest.split_whitespace();
    let _left = parts.next()?;
    let right = parts.next()?;
    Some(right.trim_start_matches("b/").to_string())
}

fn push_wrapped_diff_line(
    lines: &mut Vec<Line<'static>>,
    raw: &str,
    width: usize,
    marker: &str,
    color: Color,
) {
    let text = raw.strip_prefix(marker).unwrap_or(raw);
    let body_width = width.saturating_sub(4).max(1);
    for (index, chunk) in wrap_preserve(text, body_width).into_iter().enumerate() {
        let prefix = if index == 0 {
            format!("{marker} ")
        } else {
            "  ".to_string()
        };
        lines.push(Line::from(vec![
            Span::styled(
                prefix,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(chunk, Style::default().fg(color)),
        ]));
    }
}

fn draw_info_overlay(f: &mut Frame<'_>, app: &TuiApplication, info: &InfoOverlay, area: Rect) {
    f.render_widget(Clear, area);
    let visible_height = area.height.saturating_sub(4).max(1) as usize;
    let content_width = area.width.saturating_sub(4) as usize;
    let all_lines = info_overlay_lines(info, content_width);
    let max_offset = all_lines.len().saturating_sub(visible_height);
    let offset = app.info_scroll_offset.min(max_offset);
    let end = all_lines.len().saturating_sub(offset);
    let start = end.saturating_sub(visible_height);
    let mut visible = all_lines[start..end].to_vec();
    apply_scroll_indicators(&mut visible, offset, max_offset, content_width);
    let paragraph = Paragraph::new(visible)
        .style(Style::default().fg(FG).bg(PANEL))
        .block(
            Block::default()
                .title(format!(" inspect: {} ", info.title))
                .title_bottom(" PgUp/PgDn scroll   End follow   Esc close ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(CYAN))
                .padding(Padding::horizontal(1)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn info_overlay_lines(info: &InfoOverlay, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for raw in info.body.lines() {
        if raw.trim().is_empty() {
            lines.push(Line::from(""));
            continue;
        }
        if raw.starts_with('#') {
            lines.push(Line::from(Span::styled(
                truncate(raw, width),
                Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if raw.starts_with("```") {
            lines.push(Line::from(Span::styled(
                truncate(raw, width),
                Style::default().fg(DIM),
            )));
            continue;
        }
        let style = if raw.to_ascii_lowercase().contains("error") {
            Style::default().fg(RED)
        } else if raw.starts_with('/') || raw.contains(" = ") || raw.contains(':') {
            Style::default().fg(CYAN)
        } else {
            Style::default().fg(FG)
        };
        for piece in wrap_preserve(raw, width) {
            lines.push(Line::from(Span::styled(piece, style)));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No content.",
            Style::default().fg(DIM),
        )));
    }
    lines
}

fn draw_search_overlay(f: &mut Frame<'_>, app: &TuiApplication, area: Rect) {
    f.render_widget(Clear, area);
    let matches = app.search_matches();
    let count = matches.len();
    let current = if count == 0 {
        "0/0".to_string()
    } else {
        format!("{}/{}", app.search_match_index.min(count - 1) + 1, count)
    };
    let query = if app.search_query.is_empty() {
        "type to search".to_string()
    } else {
        app.search_query.clone()
    };
    let lines = vec![Line::from(vec![
        Span::styled(
            "Search ",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            truncate(&query, area.width.saturating_sub(28) as usize),
            Style::default().fg(FG),
        ),
        Span::styled(" ", Style::default().fg(DIM)),
        Span::styled(current, Style::default().fg(AMBER)),
        Span::styled(
            "  Enter/↓ next  ↑ previous  Esc close",
            Style::default().fg(DIM),
        ),
    ])];
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(FG).bg(PANEL))
        .block(
            Block::default()
                .title(" search ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(CYAN))
                .padding(Padding::horizontal(1)),
        );
    f.render_widget(paragraph, area);
}

fn draw_command_palette(f: &mut Frame<'_>, app: &TuiApplication, screen: Rect) {
    let count = app.input.suggestions.len().min(12);
    let area = command_palette_rect(screen, count);
    f.render_widget(Clear, area);
    let lines = command_palette_lines(app, area.width.saturating_sub(4) as usize, count);
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(FG).bg(PANEL))
        .block(
            Block::default()
                .title(" command palette ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(CYAN))
                .padding(Padding::horizontal(1)),
        );
    f.render_widget(paragraph, area);
}

fn command_palette_rect(screen: Rect, visible_items: usize) -> Rect {
    let width = screen.width.min(92).max(34);
    let height = (visible_items as u16 + 4).min(screen.height.saturating_sub(4).max(8));
    Rect {
        x: screen.x + screen.width.saturating_sub(width) / 2,
        y: screen.y + screen.height.saturating_sub(height) / 3,
        width,
        height,
    }
}

fn command_palette_lines(
    app: &TuiApplication,
    width: usize,
    visible_items: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Type to filter", Style::default().fg(DIM)),
        Span::raw("   "),
        Span::styled("Enter", Style::default().fg(CYAN)),
        Span::styled(" run  ", Style::default().fg(DIM)),
        Span::styled("Esc", Style::default().fg(CYAN)),
        Span::styled(" close", Style::default().fg(DIM)),
    ]));
    lines.push(Line::from(""));

    let value_width = (width / 3).clamp(16, 30);
    let desc_width = width.saturating_sub(value_width + 3).max(8);
    let total = app.input.suggestions.len();
    let selected_index = app.input.selected_suggestion.min(total.saturating_sub(1));
    let start = if total <= visible_items {
        0
    } else {
        selected_index
            .saturating_sub(visible_items / 2)
            .min(total - visible_items)
    };
    for (index, suggestion) in app
        .input
        .suggestions
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_items)
    {
        let selected = index == app.input.selected_suggestion;
        let row_style = if selected {
            Style::default().fg(Color::Black).bg(CYAN)
        } else {
            Style::default().fg(FG).bg(PANEL)
        };
        let dim_style = if selected {
            Style::default().fg(Color::Black).bg(CYAN)
        } else {
            Style::default().fg(DIM).bg(PANEL)
        };
        let value = suggestion
            .replacement
            .as_deref()
            .unwrap_or(&suggestion.value);
        lines.push(Line::from(vec![
            Span::styled(if selected { ">" } else { " " }, row_style),
            Span::styled(" ", row_style),
            Span::styled(
                format!("{:<value_width$}", truncate(value, value_width)),
                row_style.add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            ),
            Span::styled(" ", row_style),
            Span::styled(truncate(&suggestion.description, desc_width), dim_style),
        ]));
    }
    if total > visible_items {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                "showing {}-{} of {} commands",
                start + 1,
                (start + visible_items).min(total),
                total
            ),
            Style::default().fg(DIM),
        )));
    }
    lines
}

fn input_lines(input: &InputState, width: usize, max_rows: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let max_rows = max_rows.max(1);
    if input.buffer.is_empty() {
        return vec![Line::from(Span::styled(
            "Ask anything, @tag files/folders, use skills, or / for commands",
            Style::default().fg(DIM),
        ))];
    }
    if should_collapse_paste(input, width) {
        let marker = format!("[Pasted {} characters]", input.paste_char_count);
        let mut lines = vec![Line::from(Span::styled(
            truncate(&marker, width),
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        ))];
        let preview_width = width.saturating_sub(2).max(1);
        let preview = paste_tail_preview(&input.buffer, preview_width, max_rows.saturating_sub(1));
        for line in preview {
            lines.push(Line::from(vec![
                Span::styled(" ", Style::default().fg(DIM)),
                Span::styled(line, Style::default().fg(DIM)),
            ]));
        }
        return tail_lines(lines, max_rows);
    }
    let mut lines = Vec::new();
    for raw in input.buffer.split('\n') {
        let wrapped = wrap_preserve(raw, width);
        if wrapped.is_empty() {
            lines.push(Line::from(""));
        } else {
            for piece in wrapped {
                lines.push(Line::from(Span::styled(piece, Style::default().fg(FG))));
            }
        }
    }
    tail_lines(lines, max_rows)
}

fn draw_status(f: &mut Frame<'_>, app: &TuiApplication, area: Rect) {
    let pending = app.tool_executor.guardrails.approvals.pending_len();
    let approval = if pending > 0 {
        format!(" approvals:{pending}")
    } else {
        String::new()
    };
    let controls = if pending > 0 {
        "Enter approve | S allow session | D deny | /approvals show"
    } else if app.search_open {
        "search: type query | Enter/Down next | Up previous | Esc close"
    } else if app.input.buffer.starts_with('/') {
        "Enter run command | Tab complete | Esc close"
    } else if app.session.status == "streaming" {
        "Ctrl+C cancel/quit | PgUp read history | End follow"
    } else {
        "Enter send | Ctrl+P commands | ? help | PgUp/PgDn scroll | Ctrl+C quit"
    };
    let text = format!(
        " {} | session {} | ctx {}/{} | tools {} | skills {}{} | {} ",
        app.session.status,
        app.session.session_id,
        app.session.tokens_used,
        app.session.context_limit,
        app.session.enabled_tools.len(),
        app.session.enabled_skills.len(),
        approval,
        controls
    );
    let paragraph = Paragraph::new(Line::from(Span::styled(
        truncate(&text, area.width as usize),
        Style::default().fg(CYAN).bg(BG),
    )));
    f.render_widget(paragraph, area);
}

fn draw_approval_modal(
    f: &mut Frame<'_>,
    approvals: &[ApprovalRequest],
    selected_index: usize,
    area: Rect,
) {
    let selected_index = selected_index.min(approvals.len().saturating_sub(1));
    let approval = &approvals[selected_index];
    f.render_widget(Clear, area);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "? ",
                Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Approval Required",
                Style::default().fg(FG).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {} pending", approvals.len()),
                Style::default().fg(DIM),
            ),
        ]),
        Line::from(""),
    ];
    lines.extend(approval_queue_lines(
        approvals,
        selected_index,
        area.width.saturating_sub(4) as usize,
        4,
    ));
    lines.extend([
        Line::from(""),
        Line::from(vec![
            Span::styled("Tool: ", Style::default().fg(DIM)),
            Span::styled(approval.tool_name.clone(), Style::default().fg(CYAN)),
            Span::styled("  Risk: ", Style::default().fg(DIM)),
            Span::styled(approval.risk_label.clone(), Style::default().fg(AMBER)),
        ]),
    ]);
    lines.extend(approval_arg_preview(
        approval,
        area.width.saturating_sub(4) as usize,
        area.height.saturating_sub(11) as usize,
    ));
    lines.extend([
        Line::from(""),
        Line::from(vec![
            Span::styled("[Enter/A] Approve once", Style::default().fg(GREEN)),
            Span::raw("   "),
            Span::styled(
                if approval.risk_label == "command-allow" {
                    "[S] Allow command this session"
                } else {
                    "[S] Allow session"
                },
                Style::default().fg(CYAN),
            ),
            Span::raw("   "),
            Span::styled("[D] Deny", Style::default().fg(RED)),
            Span::raw("   "),
            Span::styled("[1/2/3] also work   [↑/↓] Select", Style::default().fg(DIM)),
        ]),
    ]);
    let paragraph = Paragraph::new(lines)
        .alignment(Alignment::Left)
        .style(Style::default().fg(FG).bg(PANEL))
        .block(
            Block::default()
                .title(" pending tool authorization ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(AMBER))
                .padding(Padding::horizontal(1)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn approval_queue_lines(
    approvals: &[ApprovalRequest],
    selected_index: usize,
    width: usize,
    max_rows: usize,
) -> Vec<Line<'static>> {
    let start = if approvals.len() <= max_rows {
        0
    } else {
        selected_index
            .saturating_sub(max_rows / 2)
            .min(approvals.len() - max_rows)
    };
    approvals
        .iter()
        .enumerate()
        .skip(start)
        .take(max_rows)
        .map(|(index, approval)| {
            let selected = index == selected_index;
            let style = if selected {
                Style::default().fg(Color::Black).bg(AMBER)
            } else {
                Style::default().fg(FG).bg(PANEL)
            };
            let muted = if selected {
                Style::default().fg(Color::Black).bg(AMBER)
            } else {
                Style::default().fg(DIM).bg(PANEL)
            };
            let text = format!(
                "{} {} {}",
                approval.risk_label, approval.tool_name, approval.id
            );
            Line::from(vec![
                Span::styled(if selected { "> " } else { "  " }, style),
                Span::styled(format!("{}/{} ", index + 1, approvals.len()), muted),
                Span::styled(truncate(&text, width.saturating_sub(8)), style),
            ])
        })
        .collect()
}

fn approval_arg_preview(
    approval: &ApprovalRequest,
    width: usize,
    max_lines: usize,
) -> Vec<Line<'static>> {
    let raw = serde_json::to_string_pretty(&approval.args).unwrap_or_else(|_| "{}".to_string());
    let mut lines = Vec::new();
    for line in raw.lines().take(max_lines.max(1)) {
        lines.push(Line::from(Span::styled(
            truncate(line, width),
            Style::default().fg(DIM),
        )));
    }
    if raw.lines().count() > max_lines.max(1) {
        lines.push(Line::from(Span::styled(
            truncate(
                "... more args hidden; use /approvals show for full details",
                width,
            ),
            Style::default().fg(AMBER),
        )));
    }
    lines
}

fn set_input_cursor(f: &mut Frame<'_>, app: &TuiApplication, area: Rect) {
    let width = area.width.saturating_sub(4).max(1) as usize;
    if should_collapse_paste(&app.input, width) {
        let marker = format!("[Pasted {} characters]", app.input.paste_char_count);
        let x = area.x + 2 + truncate(&marker, width).width().min(width) as u16;
        let y = area.y + 1;
        f.set_cursor_position(Position::new(x, y));
        return;
    }
    let max_content_rows = area.height.saturating_sub(2).max(1) as usize;
    let (line, col) = app
        .input
        .visible_visual_cursor_position(width, max_content_rows);
    let row = line.min(max_content_rows.saturating_sub(1)) as u16;
    let x = area.x + 2 + (col.min(width) as u16);
    let y = area.y + 1 + row;
    f.set_cursor_position(Position::new(x, y));
}

fn input_height(input: &InputState, terminal_width: u16) -> u16 {
    let width = terminal_width.saturating_sub(4).max(1) as usize;
    if should_collapse_paste(input, width) {
        return 4;
    }
    let rows = input.visual_line_count(width).min(8).max(1) as u16;
    rows + 2
}

fn should_collapse_paste(input: &InputState, width: usize) -> bool {
    input.paste_char_count > width.max(80)
}

fn paste_tail_preview(text: &str, width: usize, max_rows: usize) -> Vec<String> {
    if max_rows == 0 {
        return Vec::new();
    }
    let mut pieces = Vec::new();
    for raw in text.split('\n') {
        let wrapped = wrap_preserve(raw, width);
        if wrapped.is_empty() {
            pieces.push(String::new());
        } else {
            pieces.extend(wrapped);
        }
    }
    tail_strings(pieces, max_rows)
}

fn tail_lines(mut lines: Vec<Line<'static>>, max_rows: usize) -> Vec<Line<'static>> {
    if lines.len() > max_rows {
        lines.drain(0..lines.len() - max_rows);
    }
    lines
}

fn tail_strings(mut lines: Vec<String>, max_rows: usize) -> Vec<String> {
    if lines.len() > max_rows {
        lines.drain(0..lines.len() - max_rows);
    }
    lines
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width.saturating_sub(4)).max(20);
    let height = height.min(area.height.saturating_sub(4)).max(6);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn search_rect(area: Rect) -> Rect {
    let width = area.width.min(92).max(32);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area
            .y
            .saturating_add(3)
            .min(area.bottom().saturating_sub(4)),
        width,
        height: 3,
    }
}

fn wrap_preserve(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    for ch in text.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + ch_width > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width >= width.saturating_sub(1) {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratatui_markdown_renderer_handles_code_diff_and_tables() {
        let markdown = [
            "## Result",
            "",
            "| file | status |",
            "| --- | --- |",
            "| src/lib.rs | changed |",
            "",
            "```diff",
            "-old",
            "+new",
            "```",
        ]
        .join("\n");

        let lines = render_markdown(&markdown, 80, Style::default().fg(FG));
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Result"));
        assert!(rendered.contains("file"));
        assert!(rendered.contains("src/lib.rs"));
        assert!(rendered.contains("diff"));
        assert!(rendered.contains("-old"));
        assert!(rendered.contains("+new"));
    }

    #[test]
    fn ratatui_code_blocks_apply_token_level_highlighting() {
        let markdown = [
            "```rust",
            "fn main() {",
            r#"    let value = "hello";"#,
            "}",
            "```",
        ]
        .join("\n");

        let lines = render_markdown(&markdown, 80, Style::default().fg(FG));
        let fn_span = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content.as_ref() == "fn")
            .expect("keyword span");
        let string_span = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content.as_ref() == r#""hello""#)
            .expect("string span");

        assert_eq!(fn_span.style.fg, Some(CYAN));
        assert_eq!(string_span.style.fg, Some(GREEN));
    }

    #[test]
    fn ratatui_tool_and_note_messages_render_compactly() {
        let message = ChatMessage {
            role: "system".to_string(),
            content: "Tool finished: read_file - ok: read 20 bytes\n\nfull detail that should be summarized".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        };
        let lines = message_lines(&message, 72, "");
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>();

        assert_eq!(lines.len(), 1);
        assert!(rendered.contains("tool"));
        assert!(rendered.contains("Tool finished: read_file"));
    }

    #[test]
    fn ratatui_input_collapses_large_paste_marker() {
        let mut input = InputState::default();
        input.append_text(&"x".repeat(240), true);

        let lines = input_lines(&input, 40, 4);
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("[Pasted 240 characters]"));
        assert!(!rendered.contains(&"x".repeat(80)));
        assert!(lines.len() <= 4);
    }

    #[test]
    fn ratatui_input_preserves_multiline_spacing() {
        let mut input = InputState::default();
        input.append_text(
            "hello   world
second line
",
            false,
        );

        let lines = input_lines(&input, 40, 8);
        let rendered = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert_eq!(rendered, vec!["hello   world", "second line", ""]);
    }

    #[test]
    fn ratatui_input_cursor_matches_tail_clipped_rendered_row() {
        let mut input = InputState::default();
        input.append_text(
            "one
two
three
four",
            false,
        );

        let rendered = input_lines(&input, 20, 3);
        let rendered_text = rendered
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let (row, col) = input.visible_visual_cursor_position(20, 3);

        assert_eq!(rendered_text, vec!["two", "three", "four"]);
        assert_eq!((row, col), (2, 4));
    }

    #[test]
    fn ratatui_wraps_unformatted_paste_without_overflow() {
        let width = 24;
        let wrapped = wrap_preserve(&"a".repeat(97), width);

        assert!(wrapped.len() > 1);
        assert!(wrapped.iter().all(|line| line.width() <= width));

        let markdown = render_markdown(&"b".repeat(97), width, Style::default().fg(FG));
        for line in markdown {
            for span in line.spans {
                assert!(span.content.width() <= width + 2, "{}", span.content);
            }
        }
    }

    #[test]
    fn ratatui_completed_assistant_message_has_two_trailing_spacer_lines() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app =
            crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: "final visible line".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });

        let lines = chat_lines(&app, 80);
        assert!(lines.len() >= 3);
        let last_two = &lines[lines.len() - 2..];
        assert!(last_two.iter().all(|line| line.spans.is_empty()));
        Ok(())
    }

    #[test]
    fn ratatui_chat_area_ends_above_message_input_box() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app =
            crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))
                .unwrap();
        app.session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: "final visible line above input".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 22)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut final_line_y = None;
        let mut input_top_y = None;
        for y in 0..buffer.area.height {
            let row = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>();
            if row.contains("final visible line above input") {
                final_line_y = Some(y);
            }
            if row.contains(" message ") {
                input_top_y = Some(y);
            }
        }

        let final_line_y = final_line_y.expect("assistant content should render");
        let input_top_y = input_top_y.expect("input box title should render");
        assert!(
            final_line_y + CHAT_BOTTOM_GAP < input_top_y,
            "assistant line at y={final_line_y} should end above input top y={input_top_y}"
        );
    }

    #[test]
    fn ratatui_bottom_viewport_uses_visual_rows_so_summary_is_visible() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app =
            crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))
                .unwrap();
        let long_line = "x".repeat(420);
        app.session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: format!("Here is a long line that wraps heavily:\n\n{long_line}\n\nSummary:\n- final summary visible"),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(72, 18)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            rendered.contains("Summary:"),
            "bottom-follow view should include the response summary, not stop mid wrapped content:\n{rendered}"
        );
        assert!(rendered.contains("final summary visible"));
    }

    #[test]
    fn ratatui_layout_keeps_chat_body_full_width_for_native_selection() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app =
            crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))
                .unwrap();
        app.session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: "copyable chat body".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 32)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<Vec<_>>()
            .join("");

        assert!(rendered.contains("copyable chat body"));
        assert!(!rendered.contains("state ready"));
        assert!(!rendered.contains("work log"));
    }

    #[test]
    fn ratatui_activity_strip_tracks_running_or_approval_state() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app =
            crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        assert_eq!(activity_strip_height(&app, None), 0);

        app.session.status = "streaming".to_string();
        app.session.spinner_verb_seed = 0;
        app.session.session_id = "activity-strip-test".to_string();
        assert_eq!(activity_strip_height(&app, None), 3);
        let running_line = activity_line(&app, None, 80).expect("streaming activity line");
        let running_text = running_line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(!running_text.contains("running"));
        assert!(running_text.contains("model response in progress"));
        assert!(
            spinner_verbs()
                .iter()
                .any(|verb| running_text.contains(verb.trim()))
        );

        app.session.status = "ready".to_string();
        let approval = ApprovalRequest {
            id: "apr_test".to_string(),
            reason: "Risky tool requires human approval: write_file".to_string(),
            tool_name: "write_file".to_string(),
            args: serde_json::Map::new(),
            risk_label: "write".to_string(),
        };
        assert_eq!(activity_strip_height(&app, Some(&approval)), 3);
        Ok(())
    }

    #[test]
    fn ratatui_spinner_dot_and_verb_animate() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app =
            crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.session.status = "streaming".to_string();
        app.session.session_id = "spinner-animation-test".to_string();
        app.session.spinner_verb_seed = 42;

        app.session.activity_tick = 0;
        let first = activity_line(&app, None, 100).expect("first line");
        let first_text = first
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        app.session.activity_tick = 12;
        let second = activity_line(&app, None, 100).expect("second line");
        let second_text = second
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_ne!(first_text, second_text);
        assert!(first_text.starts_with('⠋'));
        assert!(second_text.starts_with('⠦'));
        Ok(())
    }

    #[test]
    fn ratatui_streaming_activity_detail_stays_anchored_while_spinner_animates() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app =
            crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.session.status = "streaming".to_string();
        app.session.activity = "finished tool run_command".to_string();
        app.session.session_id = "spinner-anchor-test".to_string();
        app.session.spinner_verb_seed = 42;

        app.session.activity_tick = 0;
        let first = activity_line(&app, None, 100).expect("first line");
        let first_text = first
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        app.session.activity_tick = 12;
        let second = activity_line(&app, None, 100).expect("second line");
        let second_text = second
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_ne!(first_text, second_text);
        fn visual_column_of(haystack: &str, needle: &str) -> Option<usize> {
            let idx = haystack.find(needle)?;
            Some(haystack[..idx].width())
        }

        let first_detail_col = visual_column_of(&first_text, "finished tool run_command");
        let second_detail_col = visual_column_of(&second_text, "finished tool run_command");
        assert_eq!(first_detail_col, second_detail_col);
        assert_eq!(first_detail_col, Some(21));
        Ok(())
    }

    #[test]
    fn ratatui_activity_strip_surfaces_pending_attachments() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app =
            crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.session.pending_attachments.push(Attachment {
            path: "/tmp/example.rs".to_string(),
            kind: "file".to_string(),
            mime_type: None,
            name: None,
            size_bytes: Some(2048),
        });

        assert_eq!(activity_strip_height(&app, None), 3);
        let summary = pending_attachment_summary(&app.session.pending_attachments, 80);
        assert!(summary.contains("1 pending attachment"));
        assert!(summary.contains("example.rs"));
        assert!(summary.contains("2.0 KB"));

        app.session.status = "streaming".to_string();
        assert_eq!(activity_strip_height(&app, None), 4);
        Ok(())
    }

    #[test]
    fn ratatui_command_palette_centers_full_command_list() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app =
            crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.command_palette_open = true;
        app.input.set_buffer("/");
        app.input.update_suggestions(vec![
            crate::ui::input::Suggestion::new(
                "/workspace",
                "switch workspace and project session",
                Some("/workspace".to_string()),
            ),
            crate::ui::input::Suggestion::new(
                "/models",
                "list or refresh provider models",
                Some("/models".to_string()),
            ),
        ]);

        let area = command_palette_rect(Rect::new(0, 0, 120, 40), app.input.suggestions.len());
        assert!(area.x > 0);
        assert!(area.y > 0);
        assert!(area.width <= 92);

        let lines = command_palette_lines(&app, 80, 12);
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>();
        assert!(rendered.contains("Type to filter"));
        assert!(rendered.contains("/workspace"));
        assert!(rendered.contains("provider models"));
        Ok(())
    }

    #[test]
    fn ratatui_command_palette_window_follows_selected_row() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app =
            crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.command_palette_open = true;
        app.input.set_buffer("/");
        let suggestions = (0..20)
            .map(|index| {
                crate::ui::input::Suggestion::new(
                    format!("/cmd-{index:02}"),
                    format!("Command {index}"),
                    Some(format!("/cmd-{index:02}")),
                )
            })
            .collect::<Vec<_>>();
        app.input.update_suggestions(suggestions);
        app.input.selected_suggestion = 17;

        let lines = command_palette_lines(&app, 80, 8);
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>();
        assert!(rendered.contains("/cmd-17"));
        assert!(!rendered.contains("/cmd-00"));
        assert!(rendered.contains("showing"));
        Ok(())
    }

    #[test]
    fn ratatui_help_overlay_lists_controls_without_chat_pollution() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let app = crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        let lines = help_overlay_lines(&app, 88, 22);
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>();
        assert!(rendered.contains("Vegvisir controls"));
        assert!(rendered.contains("Ctrl+P"));
        assert!(rendered.contains("/models"));
        assert!(rendered.contains("Esc or ? closes"));
        Ok(())
    }

    #[test]
    fn ratatui_diff_overlay_summarizes_patch_for_review() {
        let overlay = DiffOverlay {
            title: "Git diff".to_string(),
            diff: [
                "diff --git a/src/lib.rs b/src/lib.rs",
                "@@ -1,2 +1,3 @@",
                "-old",
                "+new",
                "+more",
            ]
            .join("\n"),
            files_changed: 1,
            added_lines: 2,
            removed_lines: 1,
        };
        let lines = diff_overlay_lines(&overlay, 80);
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>();
        assert!(rendered.contains("src/lib.rs"));
        assert!(rendered.contains("@@ -1,2 +1,3 @@"));
        assert!(rendered.contains("old"));
        assert!(rendered.contains("new"));
    }

    #[test]
    fn ratatui_info_overlay_renders_command_output() {
        let overlay = InfoOverlay {
            title: "models".to_string(),
            body: "Models for provider openai\n/model gpt-5.5\ncontext: 400000".to_string(),
        };
        let lines = info_overlay_lines(&overlay, 80);
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>();
        assert!(rendered.contains("Models for provider openai"));
        assert!(rendered.contains("/model gpt-5.5"));
        assert!(rendered.contains("context: 400000"));
    }

    #[test]
    fn ratatui_help_overlay_lists_work_activity_command() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let app = crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        let rendered = help_overlay_lines(&app, 88, 24)
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>();
        assert!(rendered.contains("/work"));
        assert!(rendered.contains("recent work"));
        Ok(())
    }

    #[test]
    fn ratatui_classifies_system_messages_for_scanability() {
        assert_eq!(
            classify_system_message("Error: model exceeded Vegvisir tool-call round limit."),
            SystemMessageKind::Error
        );
        assert_eq!(
            classify_system_message(
                "Risky tool requires human approval: write_file; approval_id=apr_123"
            ),
            SystemMessageKind::Approval
        );
        assert_eq!(
            classify_system_message("Tool call read_file completed with exit code 0"),
            SystemMessageKind::Tool
        );
        assert_eq!(
            classify_system_message("Workspace set to /tmp/project"),
            SystemMessageKind::Note
        );

        let message = ChatMessage {
            role: "system".to_string(),
            content: "Error: provider failed".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        };
        let rendered = message_lines(&message, 80, "")
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>();
        assert!(rendered.contains("error"));
        assert!(!rendered.contains("note"));
    }

    #[test]
    fn ratatui_scroll_indicators_mark_history_position() {
        let mut visible = vec![Line::from("one"), Line::from("two"), Line::from("three")];
        apply_scroll_indicators(&mut visible, 2, 5, 80);
        assert!(
            visible[0]
                .spans
                .iter()
                .any(|span| span.content.contains("older messages"))
        );
        assert!(
            visible[2]
                .spans
                .iter()
                .any(|span| span.content.contains("newer messages"))
        );
    }

    #[test]
    fn ratatui_work_log_summarizes_recent_system_events() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app =
            crate::app::TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.session.messages.push(ChatMessage {
            role: "system".to_string(),
            content: "Error: something failed".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        let lines = work_log_lines(&app, 80);
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(rendered.contains("error"));
        assert!(rendered.contains("something failed"));
        Ok(())
    }

    #[test]
    fn ratatui_approval_preview_truncates_large_args() {
        let mut args = serde_json::Map::new();
        args.insert("path".to_string(), serde_json::json!("/tmp/example"));
        args.insert(
            "content".to_string(),
            serde_json::json!("line1\nline2\nline3"),
        );
        let approval = ApprovalRequest {
            id: "apr_test".to_string(),
            reason: "Risky tool requires human approval: write_file".to_string(),
            tool_name: "write_file".to_string(),
            args,
            risk_label: "write".to_string(),
        };
        let preview = approval_arg_preview(&approval, 80, 2);
        let rendered = preview
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("path") || rendered.contains("content"));
        assert!(rendered.contains("more args hidden"));
    }

    #[test]
    fn ratatui_approval_queue_highlights_selected_request() {
        let approvals = vec![
            ApprovalRequest {
                id: "apr_a".to_string(),
                reason: "first".to_string(),
                tool_name: "write_file".to_string(),
                args: serde_json::Map::new(),
                risk_label: "write".to_string(),
            },
            ApprovalRequest {
                id: "apr_b".to_string(),
                reason: "second".to_string(),
                tool_name: "run_command".to_string(),
                args: serde_json::Map::new(),
                risk_label: "command".to_string(),
            },
        ];
        let lines = approval_queue_lines(&approvals, 1, 80, 4);
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>();
        assert!(rendered.contains("1/2"));
        assert!(rendered.contains("2/2"));
        assert!(rendered.contains("apr_b"));
        assert!(
            lines[1]
                .spans
                .first()
                .is_some_and(|span| span.content.as_ref() == "> ")
        );
    }
}
