use hmac::{Hmac, Mac};
use rand::random;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use thiserror::Error;

pub const TOTP_SECRET_REF: &str = "secret://hbse/internal/mfa/totp";
pub const DEFAULT_TOTP_PERIOD_SECONDS: u64 = 30;
pub const DEFAULT_TOTP_DIGITS: u32 = 6;

type HmacSha1 = Hmac<Sha1>;

#[derive(Debug, Error)]
pub enum MfaError {
    #[error("TOTP seed decoding failed")]
    InvalidSeed,
    #[error("TOTP code must contain digits")]
    InvalidCode,
    #[error("unsupported TOTP algorithm: {0}")]
    UnsupportedAlgorithm(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TotpConfig {
    pub issuer: String,
    pub account: String,
    pub algorithm: String,
    pub digits: u32,
    pub period_seconds: u64,
    pub secret_base32: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TotpEnrollment {
    pub issuer: String,
    pub account: String,
    pub algorithm: String,
    pub digits: u32,
    pub period_seconds: u64,
    pub secret_base32: String,
    pub otpauth_uri: String,
}

impl TotpConfig {
    pub fn enrollment(&self) -> TotpEnrollment {
        TotpEnrollment {
            issuer: self.issuer.clone(),
            account: self.account.clone(),
            algorithm: self.algorithm.clone(),
            digits: self.digits,
            period_seconds: self.period_seconds,
            secret_base32: self.secret_base32.clone(),
            otpauth_uri: otpauth_uri(self),
        }
    }
}

pub fn new_totp_config(issuer: impl Into<String>, account: impl Into<String>) -> TotpConfig {
    let secret: [u8; 20] = random();
    TotpConfig {
        issuer: issuer.into(),
        account: account.into(),
        algorithm: "SHA1".to_string(),
        digits: DEFAULT_TOTP_DIGITS,
        period_seconds: DEFAULT_TOTP_PERIOD_SECONDS,
        secret_base32: base32_encode_no_padding(&secret),
    }
}

pub fn verify_totp_code(
    config: &TotpConfig,
    code: &str,
    unix_time_seconds: u64,
    allowed_drift_steps: i64,
) -> Result<bool, MfaError> {
    if !config.algorithm.eq_ignore_ascii_case("SHA1") {
        return Err(MfaError::UnsupportedAlgorithm(config.algorithm.clone()));
    }
    let code = normalize_code(code)?;
    let secret = base32_decode_no_padding(&config.secret_base32)?;
    let period = config.period_seconds.max(1);
    let step = (unix_time_seconds / period) as i64;
    for offset in -allowed_drift_steps..=allowed_drift_steps {
        let candidate_step = step + offset;
        if candidate_step < 0 {
            continue;
        }
        let candidate = totp_at_step(&secret, candidate_step as u64, config.digits)?;
        if candidate == code {
            return Ok(true);
        }
    }
    Ok(false)
}

fn totp_at_step(secret: &[u8], step: u64, digits: u32) -> Result<String, MfaError> {
    let mut mac = HmacSha1::new_from_slice(secret).map_err(|_| MfaError::InvalidSeed)?;
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0f) as usize;
    let binary = (((digest[offset] & 0x7f) as u32) << 24)
        | ((digest[offset + 1] as u32) << 16)
        | ((digest[offset + 2] as u32) << 8)
        | digest[offset + 3] as u32;
    let modulo = 10u32.checked_pow(digits).ok_or(MfaError::InvalidCode)?;
    Ok(format!(
        "{:0width$}",
        binary % modulo,
        width = digits as usize
    ))
}

fn normalize_code(code: &str) -> Result<String, MfaError> {
    let normalized: String = code.chars().filter(|ch| !ch.is_whitespace()).collect();
    if normalized.is_empty() || !normalized.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(MfaError::InvalidCode);
    }
    Ok(normalized)
}

fn otpauth_uri(config: &TotpConfig) -> String {
    format!(
        "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm={}&digits={}&period={}",
        percent_encode(&config.issuer),
        percent_encode(&config.account),
        config.secret_base32,
        percent_encode(&config.issuer),
        config.algorithm,
        config.digits,
        config.period_seconds,
    )
}

fn base32_encode_no_padding(value: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut output = String::new();
    let mut buffer = 0u32;
    let mut bits = 0u8;
    for byte in value {
        buffer = (buffer << 8) | (*byte as u32);
        bits += 8;
        while bits >= 5 {
            let index = ((buffer >> (bits - 5)) & 0x1f) as usize;
            output.push(ALPHABET[index] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        let index = ((buffer << (5 - bits)) & 0x1f) as usize;
        output.push(ALPHABET[index] as char);
    }
    output
}

fn base32_decode_no_padding(value: &str) -> Result<Vec<u8>, MfaError> {
    let mut buffer = 0u32;
    let mut bits = 0u8;
    let mut output = Vec::new();
    for ch in value.chars().filter(|ch| !ch.is_whitespace() && *ch != '=') {
        let index = match ch.to_ascii_uppercase() {
            'A'..='Z' => ch.to_ascii_uppercase() as u8 - b'A',
            '2'..='7' => ch as u8 - b'2' + 26,
            _ => return Err(MfaError::InvalidSeed),
        };
        buffer = (buffer << 5) | index as u32;
        bits += 5;
        if bits >= 8 {
            output.push(((buffer >> (bits - 8)) & 0xff) as u8);
            bits -= 8;
        }
    }
    if output.is_empty() {
        return Err(MfaError::InvalidSeed);
    }
    Ok(output)
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_rfc_6238_sha1_vector() {
        let config = TotpConfig {
            issuer: "HBSE".to_string(),
            account: "test".to_string(),
            algorithm: "SHA1".to_string(),
            digits: 8,
            period_seconds: 30,
            secret_base32: base32_encode_no_padding(b"12345678901234567890"),
        };
        assert!(verify_totp_code(&config, "94287082", 59, 0).unwrap());
        assert!(verify_totp_code(&config, "07081804", 1111111109, 0).unwrap());
    }

    #[test]
    fn enrollment_uri_contains_expected_fields() {
        let config = TotpConfig {
            issuer: "HBSE".to_string(),
            account: "local vault".to_string(),
            algorithm: "SHA1".to_string(),
            digits: 6,
            period_seconds: 30,
            secret_base32: "JBSWY3DPEHPK3PXP".to_string(),
        };
        assert_eq!(
            config.enrollment().otpauth_uri,
            "otpauth://totp/HBSE:local%20vault?secret=JBSWY3DPEHPK3PXP&issuer=HBSE&algorithm=SHA1&digits=6&period=30"
        );
    }
}
