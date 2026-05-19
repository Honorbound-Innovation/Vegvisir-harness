use hmac::{Hmac, Mac};
use regex::Regex;
use sha2::Sha256;

use crate::serialization::b64url_no_padding;

type HmacSha256 = Hmac<Sha256>;

pub fn redaction_fingerprint(fingerprint_key: &[u8], secret: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(fingerprint_key).expect("HMAC accepts any key");
    mac.update(secret);
    let digest = mac.finalize().into_bytes();
    b64url_no_padding(&digest[..18])
}

#[derive(Debug, Clone)]
pub struct RedactionEngine {
    known_values: Vec<(String, Vec<u8>)>,
}

impl RedactionEngine {
    pub fn new() -> Self {
        Self {
            known_values: Vec::new(),
        }
    }

    pub fn learn(&mut self, fingerprint_key: &[u8], secret: &[u8]) -> String {
        let fingerprint = redaction_fingerprint(fingerprint_key, secret);
        self.known_values
            .push((fingerprint.clone(), secret.to_vec()));
        fingerprint
    }

    pub fn with_known_secret(fingerprint: impl Into<String>, secret: impl Into<Vec<u8>>) -> Self {
        Self {
            known_values: vec![(fingerprint.into(), secret.into())],
        }
    }

    pub fn redact_text(&self, text: &str) -> String {
        let mut redacted = text.to_string();
        for (fingerprint, secret) in &self.known_values {
            let marker = format!("[REDACTED:secret:{fingerprint}]");
            let mut representations = secret_representations(secret);
            representations.sort_by_key(|value| std::cmp::Reverse(value.len()));
            for representation in representations {
                redacted = redacted.replace(&representation, &marker);
            }
        }
        redact_structured_patterns(&redacted)
    }

    pub fn assert_no_known_secret(&self, text: &str) -> Result<(), RedactionLeakError> {
        if self.redact_text(text) == text {
            Ok(())
        } else {
            Err(RedactionLeakError)
        }
    }
}

impl Default for RedactionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("controlled output contains a known secret")]
pub struct RedactionLeakError;

pub fn redact_known_secret(value: &str, secret: &[u8], marker: &str) -> (String, bool) {
    let fingerprint = marker.to_string();
    let engine = RedactionEngine::with_known_secret(fingerprint, secret.to_vec());
    let redacted = engine.redact_text(value);
    let changed = redacted != value;
    (redacted, changed)
}

pub fn secret_representations(secret: &[u8]) -> Vec<String> {
    let mut values = vec![base64_encode(secret), b64url_no_padding(secret)];
    if let Ok(text) = std::str::from_utf8(secret) {
        values.push(text.to_string());
        values.push(percent_encode(text));
    }
    values.sort();
    values.dedup();
    values
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect()
}

fn redact_structured_patterns(text: &str) -> String {
    let patterns = [
        (
            r"(?i)(Authorization:\s*Bearer\s+)[^\s,;]+",
            "$1[REDACTED:bearer]",
        ),
        (
            r#"(?i)(password=)(["']?)[^;&\s"']+(["']?)"#,
            "$1$2[REDACTED]$3",
        ),
        (
            r#"(?i)(token=)(["']?)[^;&\s"']+(["']?)"#,
            "$1$2[REDACTED]$3",
        ),
        (
            r#"(?i)(api[_-]?key=)(["']?)[^;&\s"']+(["']?)"#,
            "$1$2[REDACTED]$3",
        ),
        (
            r#"(?i)("?(?:password|token|api[_-]?key)"?\s*:\s*")[^"]+(")"#,
            "$1[REDACTED]$2",
        ),
    ];
    let mut redacted = text.to_string();
    for (pattern, replacement) in patterns {
        redacted = Regex::new(pattern)
            .expect("static redaction regex compiles")
            .replace_all(&redacted, replacement)
            .to_string();
    }
    redacted
}

fn base64_encode(value: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(value)
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
    fn fingerprint_is_keyed_and_truncated() {
        let first = redaction_fingerprint(&[1u8; 32], b"secret");
        let second = redaction_fingerprint(&[2u8; 32], b"secret");
        assert_ne!(first, second);
        assert_eq!(first.len(), 24);
    }

    #[test]
    fn redacts_known_representations_and_structured_patterns() {
        let mut engine = RedactionEngine::new();
        engine.learn(&[1u8; 32], b"sk test");
        assert_eq!(engine.redact_text("token=sk test"), "token=[REDACTED]");
        assert_eq!(
            engine.redact_text("Authorization: Bearer abc123"),
            "Authorization: Bearer [REDACTED:bearer]"
        );
        assert!(engine
            .assert_no_known_secret("prefix sk test suffix")
            .is_err());
    }
}
