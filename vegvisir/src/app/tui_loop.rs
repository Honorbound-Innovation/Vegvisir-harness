use std::{
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

use super::{PendingEditorKind, TuiApplication};

const MAX_QUEUED_EVENTS_PER_TICK: usize = 256;

impl TuiApplication {
    pub fn run(&mut self) -> anyhow::Result<()> {
        let _terminal = TerminalGuard::enter()?;
        let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
        let mut terminal = ratatui::Terminal::new(backend)?;
        terminal.clear()?;
        terminal.draw(|frame| crate::tui2::draw(frame, self))?;
        let mut mouse_capture_applied = true;
        let mut last_activity_pulse = Instant::now();
        while self.running {
            let thinking_trace_expiry = crate::tui2::next_thinking_trace_expiry_at(self);
            let poll_timeout = thinking_trace_expiry
                .and_then(|expires_at| {
                    expires_at
                        .signed_duration_since(chrono::Utc::now())
                        .to_std()
                        .ok()
                })
                .map(|until_expiry| until_expiry.min(Duration::from_millis(50)))
                .unwrap_or_else(|| Duration::from_millis(50));

            if event::poll(poll_timeout)? {
                // Terminals commonly queue several key events while a frame is being built.
                // Process the ready queue as one bounded batch so rapid typing does not pay for
                // a complete poll/repair/render cycle after every character.
                for _ in 0..MAX_QUEUED_EVENTS_PER_TICK {
                    self.handle_terminal_event(event::read()?);
                    if !self.running || !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
            }
            if thinking_trace_expiry.is_some_and(|expires_at| expires_at <= chrono::Utc::now()) {
                self.redraw_requested = true;
            }
            self.poll_stream_events();
            self.poll_pending_send();
            self.turn_repair(false);
            self.expire_ephemeral_notice();
            self.poll_autonomy_controller();
            self.poll_background_jobs();
            self.poll_task_runner();
            if self.pending_editor_action.is_some() {
                run_pending_editor_action(self, &mut terminal, &mut mouse_capture_applied)?;
            }
            if last_activity_pulse.elapsed() >= Duration::from_millis(150) {
                self.pulse_activity();
                last_activity_pulse = Instant::now();
            }
            if self.mouse_capture_enabled != mouse_capture_applied {
                if self.mouse_capture_enabled {
                    execute!(terminal.backend_mut(), EnableMouseCapture)?;
                } else {
                    execute!(terminal.backend_mut(), DisableMouseCapture)?;
                    self.drag_anchor = None;
                    self.drag_current = None;
                }
                mouse_capture_applied = self.mouse_capture_enabled;
                self.redraw_requested = true;
            }
            if self.clear_requested {
                terminal.clear()?;
                self.chat_scroll_offset = 0;
                self.clear_requested = false;
                self.redraw_requested = true;
            }
            if self.should_draw_frame() {
                self.redraw_requested = false;
                terminal.draw(|frame| crate::tui2::draw(frame, self))?;
            }
        }
        self.drain_before_terminal_exit();
        if self.should_draw_frame() {
            self.redraw_requested = false;
            terminal.draw(|frame| crate::tui2::draw(frame, self))?;
        }
        terminal.show_cursor()?;
        Ok(())
    }

    fn handle_terminal_event(&mut self, terminal_event: Event) {
        match terminal_event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.handle_key_event(key),
            Event::Key(_) => {}
            Event::Mouse(mouse) => self.handle_mouse_event(mouse),
            Event::Paste(text) => {
                if let Some(prompt) = self.sudo_password_prompt.as_mut() {
                    for ch in text.chars().filter(|ch| *ch != '\n' && *ch != '\r') {
                        prompt.push(ch);
                    }
                } else {
                    self.input.append_text(&text, true);
                }
                self.redraw_requested = true;
            }
            Event::Resize(_, _) => self.redraw_requested = true,
            _ => {}
        }
    }

    pub(crate) fn should_draw_frame(&self) -> bool {
        self.redraw_requested
    }

    pub(crate) fn drain_before_terminal_exit(&mut self) {
        self.poll_stream_events();
        self.poll_pending_send();
        self.poll_background_jobs();
        self.poll_task_runner();
    }
}

fn run_pending_editor_action(
    app: &mut TuiApplication,
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    mouse_capture_applied: &mut bool,
) -> anyhow::Result<()> {
    let Some(action) = app.pending_editor_action.take() else {
        return Ok(());
    };

    terminal.show_cursor()?;
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen
    )?;
    terminal.backend_mut().flush()?;
    *mouse_capture_applied = false;

    let edit_result = match action.kind {
        PendingEditorKind::KaProfile => crate::persona::run_editor_for_path(&action.path),
    };

    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )?;
    enable_raw_mode()?;
    stdout.flush()?;
    *mouse_capture_applied = true;
    app.mouse_capture_enabled = true;
    app.clear_requested = true;

    match edit_result {
        Ok(()) => match action.kind {
            PendingEditorKind::KaProfile => {
                match crate::persona::get_persona_with_root(&app.data_root, &action.id) {
                    Ok(Some(profile)) => app.push_system_message(format!(
                        "Edited ka `{}` ({}) at {}.",
                        profile.id,
                        profile.display_name,
                        action.path.display()
                    )),
                    Ok(None) => app.push_system_message(format!(
                        "Editor closed, but ka `{}` could not be loaded from {}.",
                        action.id,
                        action.path.display()
                    )),
                    Err(error) => app.push_system_message(format!(
                        "Editor closed, but ka `{}` failed validation: {error}",
                        action.id
                    )),
                }
            }
        },
        Err(error) => app.push_system_message(format!(
            "Editor failed for ka `{}` at {}: {error}",
            action.id,
            action.path.display()
        )),
    }
    app.autosave_session();
    app.redraw_requested = true;
    Ok(())
}

pub fn run_tui() -> anyhow::Result<()> {
    run_tui_with_dangerous_bypass(false)
}

pub fn run_tui_with_dangerous_bypass(
    dangerously_bypass_approvals_and_sandbox: bool,
) -> anyhow::Result<()> {
    let mut app = TuiApplication::new_with_dangerous_bypass(
        std::env::current_dir()?,
        dangerously_bypass_approvals_and_sandbox,
    )?;
    if !io::stdin().is_terminal() {
        print!("{}", app.render());
        return Ok(());
    }
    app.run()?;
    Ok(())
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> anyhow::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableMouseCapture
        )?;
        stdout.flush()?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(
            stdout,
            DisableMouseCapture,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn terminal_event_handler_ignores_key_releases() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;

        app.handle_terminal_event(Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )));
        app.handle_terminal_event(Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('b'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        )));

        assert_eq!(app.input.buffer, "a");
        assert!(app.redraw_requested);
        Ok(())
    }
}
