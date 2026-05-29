use std::{
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, DisableBracketedPaste, EnableBracketedPaste, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

// Crossterm's EnableMouseCapture enables button-event tracking (?1002), which
// captures drag selection and prevents normal terminal copy/paste selection.
// Vegvisir only needs mouse-wheel events for chat scrolling, so use normal
// tracking (?1000) plus SGR coordinates (?1006) instead. In xterm-compatible
// terminals this keeps wheel events flowing to the app without asking the
// terminal to report mouse drag motion to us.
const ENABLE_WHEEL_MOUSE_CAPTURE: &str = "\x1b[?1000h\x1b[?1006h";
const DISABLE_WHEEL_MOUSE_CAPTURE: &str = "\x1b[?1000l\x1b[?1006l";

use super::TuiApplication;

impl TuiApplication {
    pub fn run(&mut self) -> anyhow::Result<()> {
        let _terminal = TerminalGuard::enter()?;
        let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
        let mut terminal = ratatui::Terminal::new(backend)?;
        terminal.clear()?;
        terminal.draw(|frame| crate::tui2::draw(frame, self))?;
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
            if last_activity_pulse.elapsed() >= Duration::from_millis(150) {
                self.pulse_activity();
                last_activity_pulse = Instant::now();
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
        execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
        write!(stdout, "{ENABLE_WHEEL_MOUSE_CAPTURE}")?;
        stdout.flush()?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = write!(stdout, "{DISABLE_WHEEL_MOUSE_CAPTURE}");
        let _ = execute!(stdout, DisableBracketedPaste, LeaveAlternateScreen);
    }
}
