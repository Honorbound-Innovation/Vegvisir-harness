use std::collections::BTreeMap;
use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DotenvError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("regex error: {0}")]
    Regex(#[from] regex::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DotenvFinding {
    pub line: usize,
    pub kind: String,
    pub key: Option<String>,
    pub detail: String,
}

pub fn scan_dotenv(path: impl AsRef<Path>) -> Result<Vec<DotenvFinding>, DotenvError> {
    let secret_ref = secret_ref_regex()?;
    let likely_secret = likely_secret_regex()?;
    let mut findings = Vec::new();
    for (index, line) in std::fs::read_to_string(path)?.lines().enumerate() {
        let line_no = index + 1;
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            continue;
        }
        for matched in secret_ref.find_iter(stripped) {
            findings.push(DotenvFinding {
                line: line_no,
                kind: "secret_ref".to_string(),
                key: dotenv_key(stripped),
                detail: matched.as_str().to_string(),
            });
        }
        if let Some(captures) = likely_secret.captures(stripped) {
            if !stripped.contains("secret://") {
                findings.push(DotenvFinding {
                    line: line_no,
                    kind: "likely_raw_secret".to_string(),
                    key: dotenv_key(stripped)
                        .or_else(|| captures.get(1).map(|value| value.as_str().to_string())),
                    detail: "dotenv value looks like raw secret material".to_string(),
                });
            }
        }
    }
    Ok(findings)
}

pub fn parse_dotenv(path: impl AsRef<Path>) -> Result<BTreeMap<String, String>, DotenvError> {
    let mut values = BTreeMap::new();
    for line in std::fs::read_to_string(path)?.lines() {
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') || !stripped.contains('=') {
            continue;
        }
        let Some((key, value)) = stripped.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        values.insert(key.to_string(), unquote(value.trim()).to_string());
    }
    Ok(values)
}

pub fn split_dotenv_values(
    values: BTreeMap<String, String>,
) -> Result<(BTreeMap<String, String>, BTreeMap<String, String>), DotenvError> {
    let secret_ref = Regex::new(r"^secret://[A-Za-z0-9_.:/@+-]+$")?;
    let mut plain = BTreeMap::new();
    let mut refs = BTreeMap::new();
    for (key, value) in values {
        if secret_ref.is_match(&value) {
            refs.insert(key, value);
        } else {
            plain.insert(key, value);
        }
    }
    Ok((plain, refs))
}

fn secret_ref_regex() -> Result<Regex, DotenvError> {
    Ok(Regex::new(r"secret://[A-Za-z0-9_.:/@+-]+")?)
}

fn likely_secret_regex() -> Result<Regex, DotenvError> {
    Ok(Regex::new(
        r#"(?i)(api[_-]?key|token|secret|password|passwd|pwd)\s*=\s*['"]?([^'"\s#]{12,})"#,
    )?)
}

fn dotenv_key(line: &str) -> Option<String> {
    line.split_once('=')
        .map(|(key, _)| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn scan_detects_secret_refs_and_raw_values() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(
            &path,
            format!(
                "APP_ENV=dev\nTOKEN=secret://default/api\nAPI_KEY={}\n",
                ["sk", "-1234567890abcdef"].concat()
            ),
        )
        .unwrap();
        let findings = scan_dotenv(&path).unwrap();
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].kind, "secret_ref");
        assert_eq!(findings[1].kind, "likely_raw_secret");
    }

    #[test]
    fn parse_splits_plain_values_and_refs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "APP_ENV=dev\nTOKEN='secret://default/api'\n").unwrap();
        let (plain, refs) = split_dotenv_values(parse_dotenv(&path).unwrap()).unwrap();
        assert_eq!(plain["APP_ENV"], "dev");
        assert_eq!(refs["TOKEN"], "secret://default/api");
    }
}
