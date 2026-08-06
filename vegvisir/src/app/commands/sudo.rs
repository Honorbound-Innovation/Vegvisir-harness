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
                    let supervisor = self.tool_registry.sudo_supervisor();
                    return match crate::privilege::sudo_refresh_interactive_from_tui(&supervisor) {
                        Ok(()) => {
                            self.clear_requested = true;
                            self.redraw_requested = true;
                            self.logger.emit(
                                "sudo_auth_refreshed",
                                json!({
                                    "session": self.session.session_id,
                                    "workspace": self.cwd.display().to_string(),
                                    "auth_path": "os-sudo-prompt",
                                }),
                            );
                            "Sudo authentication refreshed through the OS prompt. Vegvisir did not read, store, or log the password. Privileged commands can now use the private local supervisor.".to_string()
                        }
                        Err(error) => {
                            self.clear_requested = true;
                            self.redraw_requested = true;
                            format!("Sudo authentication failed: {error}")
                        }
                    };
                }

                self.open_sudo_password_prompt();
                String::new()
            }
            Some("clear" | "invalidate" | "logout" | "forget") => {
                let supervisor = self.tool_registry.sudo_supervisor();
                match crate::privilege::sudo_invalidate(&supervisor) {
                    Ok(()) => "Private sudo supervisor stopped and sudo timestamp invalidated with sudo -k.".to_string(),
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
        self.clear_requested = true;
        self.redraw_requested = true;
        self.session.status = "sudo auth prompt open".to_string();
        self.session.activity = "Secure sudo prompt is open; type the password locally, Enter hands it only to sudo, Esc cancels.".to_string();
        self.ephemeral_notice = Some(EphemeralNotice::new(
            "Secure sudo prompt opened. Type only in the masked modal; Esc cancels.",
            EphemeralNoticeKind::Info,
            std::time::Duration::from_secs(8),
        ));
    }

    fn sudo_status_text(&self) -> String {
        let supervisor = self.tool_registry.sudo_supervisor();
        let status = crate::privilege::sudo_status_for_supervisor(&supervisor);
        format!(
            "Sudo workflow:\n  sudo available: {}\n  authenticated: {}\n  status: {}\n\nUsage:\n  /sudo auth              Opens Vegvisir's local secure prompt and authenticates sudo.\n  /sudo auth --terminal   Fallback: temporarily leaves the TUI and lets the OS sudo prompt handle authentication.\n  /sudo clear             Stops the private supervisor and invalidates the sudo timestamp.\n\nSecurity invariant: Vegvisir never sends the sudo password to chat/model/tools/logs/traces/run artifacts. Authentication is handled locally by sudo; the prompt buffer and supervisor stdin/PTY remain local, masked or inaccessible to the model, and are cleared or stopped after use. Privileged commands fail closed when no supervisor is active.",
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
