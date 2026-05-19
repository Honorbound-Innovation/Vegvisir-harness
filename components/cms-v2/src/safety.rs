use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SensitiveFinding {
    pub kind: String,
    pub evidence: String,
}

pub fn detect_sensitive_content(text: &str) -> Vec<SensitiveFinding> {
    let mut findings = Vec::new();
    for token in text.split(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | '`' | ',')) {
        let token = token
            .trim_matches(|ch: char| matches!(ch, ':' | ';' | ')' | '(' | ']' | '[' | '{' | '}'));
        if is_openai_key(token) {
            findings.push(finding("openai-api-key", token));
        } else if is_github_token(token) {
            findings.push(finding("github-token", token));
        } else if is_aws_access_key(token) {
            findings.push(finding("aws-access-key", token));
        }
    }

    let lower = text.to_ascii_lowercase();
    for key in [
        "api_key",
        "apikey",
        "access_token",
        "secret_key",
        "private_key",
        "password",
    ] {
        if lower.contains(key) {
            findings.push(SensitiveFinding {
                kind: "secret-like-assignment".to_string(),
                evidence: key.to_string(),
            });
        }
    }

    findings
        .sort_by(|left, right| (&left.kind, &left.evidence).cmp(&(&right.kind, &right.evidence)));
    findings.dedup();
    findings
}

pub fn contains_sensitive_content(text: &str) -> bool {
    !detect_sensitive_content(text).is_empty()
}

pub fn redact_sensitive_text(text: &str) -> String {
    let mut redacted = text.to_string();
    for finding in detect_sensitive_content(text) {
        if finding.kind == "secret-like-assignment" {
            redacted = redact_assignment_keywords(&redacted);
        } else {
            redacted = redacted.replace(&finding.evidence, "[REDACTED]");
        }
    }

    redacted
        .split_whitespace()
        .map(|token| {
            let trimmed = token.trim_matches(|ch: char| {
                matches!(
                    ch,
                    ':' | ';' | ')' | '(' | ']' | '[' | '{' | '}' | ',' | '"' | '\''
                )
            });
            if is_openai_key(trimmed) || is_github_token(trimmed) || is_aws_access_key(trimmed) {
                token.replace(trimmed, "[REDACTED]")
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn finding(kind: &str, token: &str) -> SensitiveFinding {
    SensitiveFinding {
        kind: kind.to_string(),
        evidence: redact_token(token),
    }
}

fn is_openai_key(token: &str) -> bool {
    token.starts_with("sk-") && token.len() >= 24
}

fn is_github_token(token: &str) -> bool {
    ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"]
        .iter()
        .any(|prefix| token.starts_with(prefix))
        && token.len() >= 24
}

fn is_aws_access_key(token: &str) -> bool {
    (token.starts_with("AKIA") || token.starts_with("ASIA")) && token.len() == 20
}

fn redact_token(token: &str) -> String {
    let prefix = token.chars().take(4).collect::<String>();
    let suffix = token
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{prefix}...{suffix}")
}

fn redact_assignment_keywords(text: &str) -> String {
    let mut out = text.to_string();
    for key in [
        "api_key",
        "apikey",
        "access_token",
        "secret_key",
        "private_key",
        "password",
    ] {
        out = redact_after_keyword(&out, key);
    }
    out
}

fn redact_after_keyword(text: &str, key: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let Some(start) = lower.find(key) else {
        return text.to_string();
    };
    let mut end = start + key.len();
    let chars = text.char_indices().collect::<Vec<_>>();
    while end < text.len()
        && text[end..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_whitespace() || matches!(ch, ':' | '='))
    {
        end += text[end..].chars().next().unwrap().len_utf8();
    }
    let value_start = end;
    while end < text.len()
        && text[end..]
            .chars()
            .next()
            .is_some_and(|ch| !ch.is_whitespace() && !matches!(ch, ',' | ';'))
    {
        end += text[end..].chars().next().unwrap().len_utf8();
    }
    if value_start == end || !chars.iter().any(|(index, _)| *index == start) {
        return text.to_string();
    }
    format!("{}{}[REDACTED]{}", &text[..value_start], "", &text[end..])
}
