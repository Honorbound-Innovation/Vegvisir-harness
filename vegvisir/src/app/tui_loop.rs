use std::{
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

use super::{PendingEditorKind, TuiApplication};

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
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) => self.handle_key_event(key),
                    Event::Mouse(mouse) => self.handle_mouse_event(mouse),
                    Event::Paste(text) => {
                        self.input.append_text(&text, true);
                        self.redraw_requested = true;
                    }
                    Event::Resize(_, _) => {
                        self.redraw_requested = true;
                    }
                    _ => {}
                }
            }
            self.poll_stream_events();
            self.poll_pending_send();
            self.poll_background_jobs();
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
            if self.redraw_requested || !self.pending_background_jobs.is_empty() {
                self.redraw_requested = false;
                terminal.draw(|frame| crate::tui2::draw(frame, self))?;
            }
        }
        terminal.show_cursor()?;
        Ok(())
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
