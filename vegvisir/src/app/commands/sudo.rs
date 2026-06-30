use super::super::*;

impl TuiApplication {
    pub(crate) fn sudo_command(&mut self, args: &[String]) -> String {
        match args.first().map(String::as_str) {
            None | Some("status" | "show") => self.sudo_status_text(),
            Some("auth" | "authenticate" | "login" | "refresh") => {
                match crate::privilege::sudo_refresh_interactive_from_tui() {
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
                }
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

    fn sudo_status_text(&self) -> String {
        let status = crate::privilege::sudo_status();
        format!(
            "Sudo workflow:\n  sudo available: {}\n  authenticated: {}\n  status: {}\n\nUsage:\n  /sudo auth   Temporarily leaves the TUI and lets sudo prompt on the controlling terminal.\n  /sudo clear  Invalidates the sudo timestamp with sudo -k.\n\nSecurity invariant: Vegvisir never asks for, reads, stores, traces, or forwards the sudo password. Privileged command execution uses sudo -n and fails closed when no sudo timestamp is active.",
            yes_no(status.sudo_available),
            yes_no(status.authenticated),
            status.message,
        )
    }
}

fn sudo_usage() -> &'static str {
    "Usage: /sudo [status|auth|clear|help]"
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
