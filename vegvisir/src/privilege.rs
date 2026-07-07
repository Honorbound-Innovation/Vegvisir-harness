use std::process::{Command, Stdio};

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
            message: "sudo timestamp is not currently valid; run /sudo auth to authenticate through Vegvisir's HBSE-backed broker flow".to_string(),
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

/// Refresh the sudo timestamp via HBSE broker authorization.
///
/// Security invariants:
/// - Vegvisir never reads, stores, or logs the sudo password.
/// - Password entry happens in HBSE, not in the harness.
/// - This function only consumes the resulting authorization by checking whether
///   `sudo -n -v` succeeds after the broker unlock.
/// - stdout/stderr from the broker are not captured into chat/session history.
///
/// This replaces the old in-app password prompt path.
pub fn sudo_refresh_via_hbse_broker() -> anyhow::Result<()> {
    let output = Command::new("hbse")
        .args(["broker", "unlock"])
        .output()
        .map_err(|error| {
            anyhow::anyhow!(
                "failed to run `hbse broker unlock` before sudo authentication: {error}"
            )
        })?;
    validate_hbse_broker_unlock_output(output.status.success(), &output.stdout, &output.stderr)?;

    let status = Command::new("sudo")
        .args(["-n", "-v"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!(
            "sudo timestamp did not become active after HBSE broker unlock; exit code {:?}",
            status.code()
        )
    }
}

pub fn sudo_refresh_interactive_from_tui() -> anyhow::Result<()> {
    sudo_refresh_via_hbse_broker()
}

pub fn sudo_refresh_with_tui_password(_password: &mut Vec<char>) -> anyhow::Result<()> {
    sudo_refresh_via_hbse_broker()
}

fn validate_hbse_broker_unlock_output(
    success: bool,
    stdout: &[u8],
    stderr: &[u8],
) -> anyhow::Result<()> {
    if success {
        return Ok(());
    }
    let mut detail = String::new();
    detail.push_str(&String::from_utf8_lossy(stdout));
    detail.push_str(&String::from_utf8_lossy(stderr));
    anyhow::bail!(
        "`hbse broker unlock` failed before sudo authentication: {}",
        detail.trim().chars().take(600).collect::<String>()
    )
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

    #[test]
    fn hbse_unlock_output_accepts_success() {
        assert!(validate_hbse_broker_unlock_output(true, b"already unlocked", b"").is_ok());
    }

    #[test]
    fn hbse_unlock_output_reports_failure_detail() {
        let err = validate_hbse_broker_unlock_output(false, b"", b"broker locked or unavailable")
            .expect_err("unlock failure should be reported");
        assert!(err.to_string().contains("hbse broker unlock"));
        assert!(err.to_string().contains("broker locked or unavailable"));
    }
}
