use super::super::*;

impl TuiApplication {
    pub(crate) fn sudo_command(&mut self, args: &[String]) -> String {
        match args.first().map(String::as_str) {
            None | Some("status" | "show") => self.sudo_status_text(),
            Some("auth" | "authenticate" | "login" | "refresh") => {
                if args
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "--terminal" | "terminal" | "os-prompt"))
                {
                    return match crate::privilege::sudo_refresh_interactive_from_tui() {
                        Ok(()) => {
                            self.clear_requested = true;
                            self.redraw_requested = true;
                            self.logger.emit(
                                "sudo_auth_refreshed",
                                json!({
                                    "session": self.session.session_id,
                                    "workspace": self.cwd.display().to_string(),
                                    "password_visibility": "not-collected; OS sudo prompt only",
                                }),
                            );
                            "Sudo authentication refreshed through the OS prompt. Vegvisir did not read, store, or log the password. Privileged commands can now use the cached sudo timestamp with sudo -n.".to_string()
                        }
                        Err(error) => {
                            self.clear_requested = true;
                            self.redraw_requested = true;
                            format!("Sudo authentication failed: {error}")
                        }
                    };
                }

                self.open_sudo_password_prompt();
                "Opened secure sudo password prompt inside Vegvisir. Type the password in the modal only; it is masked and is not written to chat, command history, provider context, tools, traces, or run artifacts. Press Esc to cancel.".to_string()
            }
            Some("clear" | "invalidate" | "logout" | "forget") => {
                match crate::privilege::sudo_invalidate() {
                    Ok(()) => "Sudo timestamp invalidated with sudo -k.".to_string(),
                    Err(error) => format!("Failed to invalidate sudo timestamp: {error}"),
                }
            }
            Some("help") => sudo_usage().to_string(),
            Some(other) => format!("Unknown /sudo command: {other}\n{}", sudo_usage()),
        }
    }

    pub(crate) fn open_sudo_password_prompt(&mut self) {
        self.sudo_password_prompt = Some(SudoPasswordPrompt::new());
        self.command_palette_open = false;
        self.info_overlay = None;
        self.diff_overlay = None;
        self.sessions_overlay = None;
        self.profile_overlay = None;
        self.input.clear();
        self.input.update_suggestions(Vec::new());
        self.redraw_requested = true;
    }

    fn sudo_status_text(&self) -> String {
        let status = crate::privilege::sudo_status();
        format!(
            "Sudo workflow:\n  sudo available: {}\n  authenticated: {}\n  status: {}\n\nUsage:\n  /sudo auth              Opens Vegvisir's local secure masked password prompt.\n  /sudo auth --terminal   Fallback: temporarily leaves the TUI and lets sudo prompt on the controlling terminal.\n  /sudo clear             Invalidates the sudo timestamp with sudo -k.\n\nSecurity invariant: Vegvisir never sends the sudo password to chat/model/tools/logs/traces/run artifacts. The in-app prompt stores it only in local transient UI memory, masks it on screen, writes it only to sudo stdin for `sudo -S -p '' -v`, then clears it. Privileged command execution uses sudo -n and fails closed when no sudo timestamp is active.",
            yes_no(status.sudo_available),
            yes_no(status.authenticated),
            status.message,
        )
    }
}

fn sudo_usage() -> &'static str {
    "Usage: /sudo [status|auth [--terminal]|clear|help]"
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
