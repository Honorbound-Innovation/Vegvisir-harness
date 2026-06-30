use std::{
    io::{self, IsTerminal, Write},
    process::{Command, Stdio},
};

use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SudoStatus {
    pub authenticated: bool,
    pub sudo_available: bool,
    pub message: String,
}

pub fn sudo_status() -> SudoStatus {
    match Command::new("sudo")
        .args(["-n", "-v"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => SudoStatus {
            authenticated: true,
            sudo_available: true,
            message: "sudo timestamp is currently valid".to_string(),
        },
        Ok(_) => SudoStatus {
            authenticated: false,
            sudo_available: true,
            message: "sudo timestamp is not currently valid; run /sudo auth to authenticate via the OS prompt".to_string(),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => SudoStatus {
            authenticated: false,
            sudo_available: false,
            message: "sudo executable was not found on PATH".to_string(),
        },
        Err(error) => SudoStatus {
            authenticated: false,
            sudo_available: true,
            message: format!("failed to check sudo status: {error}"),
        },
    }
}

pub fn sudo_invalidate() -> anyhow::Result<()> {
    let status = Command::new("sudo")
        .arg("-k")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("sudo -k failed with exit code {:?}", status.code())
    }
}

pub fn sudo_refresh_interactive_from_tui() -> anyhow::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        anyhow::bail!("/sudo auth requires an interactive terminal");
    }

    let mut stdout = io::stdout();
    disable_raw_mode()?;
    execute!(
        stdout,
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen
    )?;
    stdout.flush()?;

    let auth_result = run_sudo_validate_on_controlling_terminal();

    let mut reenter_result = execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )
    .map_err(anyhow::Error::from)
    .and_then(|_| enable_raw_mode().map_err(anyhow::Error::from))
    .and_then(|_| stdout.flush().map_err(anyhow::Error::from));

    match (auth_result, reenter_result.as_mut()) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(auth_error), Ok(())) => Err(auth_error),
        (Ok(()), Err(reenter_error)) => Err(anyhow::anyhow!(
            "sudo authentication succeeded, but Vegvisir failed to restore the TUI: {reenter_error}"
        )),
        (Err(auth_error), Err(reenter_error)) => Err(anyhow::anyhow!(
            "sudo authentication failed ({auth_error}); additionally failed to restore the TUI: {reenter_error}"
        )),
    }
}

fn run_sudo_validate_on_controlling_terminal() -> anyhow::Result<()> {
    eprintln!(
        "Vegvisir is handing password entry directly to sudo. The password is not read by Vegvisir and is not written to chat/session/trace history."
    );
    let status = Command::new("sudo")
        .arg("-v")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!(
            "sudo authentication failed with exit code {:?}",
            status.code()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sudo_status_never_reports_password_material() {
        let status = sudo_status();
        assert!(!status.message.to_ascii_lowercase().contains("password="));
        assert!(!status.message.to_ascii_lowercase().contains("token="));
    }
}
