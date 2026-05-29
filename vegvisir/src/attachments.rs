use std::path::{Path, PathBuf};

use regex::Regex;

use crate::core::Attachment;

pub fn extract_attachments(text: &str, cwd: &Path) -> (String, Vec<Attachment>) {
    let re = Regex::new(r#"['"]?(?P<path>(?:file://|/|~/?)[^\s'"]+)['"]?"#)
        .expect("valid attachment regex");
    let mut attachments = Vec::new();
    let cleaned = re
        .replace_all(text, |captures: &regex::Captures| {
            let raw = captures.name("path").map(|m| m.as_str()).unwrap_or("");
            let path = path_from_token(raw, cwd);
            if path.exists() && path.is_file() {
                if let Ok(attachment) = attachment_for(&path) {
                    attachments.push(attachment);
                }
            }
            captures
                .get(0)
                .map(|m| m.as_str())
                .unwrap_or("")
                .to_string()
        })
        .trim()
        .to_string();
    (cleaned, attachments)
}

pub fn attachment_for(path: &Path) -> anyhow::Result<Attachment> {
    let resolved = path.canonicalize()?;
    let name = resolved
        .file_name()
        .map(|n| n.to_string_lossy().to_string());
    let mime_type = guess_mime(&resolved).to_string();
    let kind = if mime_type.starts_with("image/") {
        "image"
    } else {
        "file"
    }
    .to_string();
    Ok(Attachment {
        path: resolved.display().to_string(),
        kind,
        mime_type: Some(mime_type),
        name,
        size_bytes: Some(resolved.metadata()?.len()),
    })
}

fn path_from_token(token: &str, cwd: &Path) -> PathBuf {
    let token = token.strip_prefix("file://").unwrap_or(token);
    let token = percent_decode(token);
    let candidate = if token == "~" {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(token))
    } else if let Some(rest) = token.strip_prefix("~/") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("~"))
            .join(rest)
    } else {
        PathBuf::from(token)
    };
    if candidate.is_absolute() {
        candidate
    } else {
        cwd.join(candidate)
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                out.push(hex);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn guess_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "json" => "application/json",
        "csv" => "text/csv",
        _ => "application/octet-stream",
    }
}
