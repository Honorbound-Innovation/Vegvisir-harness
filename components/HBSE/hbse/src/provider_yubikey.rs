use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

pub const YUBIKEY_PIV_PROVIDER_ID: &str = "yubikey-piv";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YubikeyPivProviderStatus {
    pub available: bool,
    pub provider_id: String,
    pub ykman_available: bool,
    pub opensc_tool_available: bool,
    pub pkcs11_tool_available: bool,
    pub piv_tool_available: bool,
    pub token_present: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YubikeyPivProvider;

impl YubikeyPivProvider {
    pub fn detect() -> YubikeyPivProviderStatus {
        let ykman_available = command_exists("ykman");
        let opensc_tool_available = command_exists("opensc-tool");
        let pkcs11_tool_available = command_exists("pkcs11-tool");
        let piv_tool_available = command_exists("piv-tool");
        let token_present = token_present(
            ykman_available,
            opensc_tool_available,
            pkcs11_tool_available,
        );
        let tooling_available = ykman_available
            || (opensc_tool_available && pkcs11_tool_available)
            || piv_tool_available;
        let available = tooling_available && token_present;
        let detail = if available {
            "YubiKey/PIV-compatible token and tooling detected".to_string()
        } else if !tooling_available {
            "YubiKey/PIV tooling not found; install ykman or OpenSC tools".to_string()
        } else if !token_present {
            "YubiKey/PIV tooling found, but no token was detected".to_string()
        } else {
            "YubiKey/PIV provider unavailable".to_string()
        };
        YubikeyPivProviderStatus {
            available,
            provider_id: YUBIKEY_PIV_PROVIDER_ID.to_string(),
            ykman_available,
            opensc_tool_available,
            pkcs11_tool_available,
            piv_tool_available,
            token_present,
            detail,
        }
    }
}

fn command_exists(command: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|path| {
            let candidate = path.join(command);
            candidate.is_file()
        })
    })
}

fn token_present(
    ykman_available: bool,
    opensc_tool_available: bool,
    pkcs11_tool_available: bool,
) -> bool {
    if ykman_available && command_has_stdout("ykman", &["list"]) {
        return true;
    }
    if opensc_tool_available && command_has_stdout("opensc-tool", &["-l"]) {
        return true;
    }
    if pkcs11_tool_available && command_has_stdout("pkcs11-tool", &["-L"]) {
        return true;
    }
    false
}

fn command_has_stdout(command: &str, args: &[&str]) -> bool {
    let Ok(output) = Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    else {
        return false;
    };
    output.status.success() && !String::from_utf8_lossy(&output.stdout).trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_id_is_stable() {
        assert_eq!(YUBIKEY_PIV_PROVIDER_ID, "yubikey-piv");
    }

    #[test]
    fn detect_returns_consistent_availability() {
        let status = YubikeyPivProvider::detect();
        assert_eq!(status.provider_id, YUBIKEY_PIV_PROVIDER_ID);
        if status.available {
            assert!(status.token_present);
            assert!(
                status.ykman_available
                    || status.piv_tool_available
                    || (status.opensc_tool_available && status.pkcs11_tool_available)
            );
        }
    }
}
