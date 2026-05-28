use crate::models::*;
use anyhow::{Context, Result};
use chrono::Utc;
use pulldown_cmark::{Event, Parser, Tag};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::path::Path;
use uuid::Uuid;
use walkdir::WalkDir;

pub fn ingest_url(
    url: &str,
    max_pages: usize,
) -> Result<(Vec<SourceDocument>, Vec<DocumentSection>)> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("skiller/0.1 (+https://github.com/HonorboundInnovation/skiller)")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;
    let mut sources = Vec::new();
    let mut sections = Vec::new();
    let mut visited = std::collections::BTreeSet::new();
    let mut queue = std::collections::VecDeque::new();
    let start = url::Url::parse(url).with_context(|| format!("parse url {url}"))?;
    let start_host = start.host_str().map(str::to_string);
    queue.push_back(start);
    let max_pages = max_pages.max(1);

    while let Some(current) = queue.pop_front() {
        if visited.len() >= max_pages {
            break;
        }
        if current.scheme() != "http" && current.scheme() != "https" {
            continue;
        }
        let current_string = current.to_string();
        if !visited.insert(current_string.clone()) {
            continue;
        }
        let response = client
            .get(current.clone())
            .send()
            .with_context(|| format!("fetch {current}"))?;
        if !response.status().is_success() {
            continue;
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();
        let text = response.text()?;
        let redacted = redact_secrets(&text);
        let source_type =
            if content_type.contains("html") || redacted.to_lowercase().contains("<html") {
                SourceType::Html
            } else if content_type.contains("json") || current.path().ends_with(".json") {
                SourceType::ApiSpec
            } else if current.path().ends_with(".yaml") || current.path().ends_with(".yml") {
                SourceType::ApiSpec
            } else {
                SourceType::Url
            };
        let source_id = stable_id("src", &current_string);
        let title = title_from_url_or_html(&current, &redacted);
        let source = SourceDocument {
            source_id: source_id.clone(),
            title,
            source_type: source_type.clone(),
            origin: current_string.clone(),
            version: detect_version(&redacted),
            license: None,
            owner: None,
            visibility: Visibility::Private,
            ingested_at: Utc::now(),
            hash: hex_hash(redacted.as_bytes()),
            retention_policy: RetentionPolicy::ExcerptsOnly,
            export_policy: ExportPolicy::PrivateOnly,
            secret_scan_status: if text == redacted {
                ScanStatus::Clean
            } else {
                ScanStatus::Findings(vec!["secret-like content redacted".into()])
            },
            permission_status: PermissionStatus::Allowed,
            citation_policy: CitationPolicy::ShortExcerpts,
        };
        let mut page_sections = match source_type {
            SourceType::Html => html_sections(&source_id, &redacted),
            SourceType::OpenApi | SourceType::ApiSpec => {
                interface_sections(&source_id, &redacted, true)
            }
            _ => plain_sections(&source_id, &redacted),
        };
        if matches!(source_type, SourceType::Html) && visited.len() < max_pages {
            for link in same_host_links(&current, &redacted, start_host.as_deref()) {
                if !visited.contains(link.as_str()) && queue.len() + visited.len() < max_pages * 3 {
                    queue.push_back(link);
                }
            }
        }
        sources.push(source);
        sections.append(&mut page_sections);
    }
    Ok((sources, sections))
}

pub fn ingest_repository(path: &Path) -> Result<(Vec<SourceDocument>, Vec<DocumentSection>)> {
    let mut sources = Vec::new();
    let mut sections = Vec::new();
    let files: Vec<_> = WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| repository_source_kind(p).is_some())
        .collect();

    for file in files {
        if should_skip(&file) {
            continue;
        }
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("read repository file {}", file.display()))?;
        let redacted = redact_secrets(&text);
        let source_kind = repository_source_kind(&file).unwrap_or("repository");
        let source_id = stable_id("src", &format!("repo:{}", file.display()));
        let title = file
            .strip_prefix(path)
            .unwrap_or(&file)
            .display()
            .to_string();
        let source = SourceDocument {
            source_id: source_id.clone(),
            title: title.clone(),
            source_type: SourceType::Repository,
            origin: file.display().to_string(),
            version: detect_version(&redacted),
            license: None,
            owner: None,
            visibility: Visibility::Private,
            ingested_at: Utc::now(),
            hash: hex_hash(redacted.as_bytes()),
            retention_policy: RetentionPolicy::ExcerptsOnly,
            export_policy: ExportPolicy::PrivateOnly,
            secret_scan_status: if text == redacted {
                ScanStatus::Clean
            } else {
                ScanStatus::Findings(vec!["secret-like content redacted".into()])
            },
            permission_status: PermissionStatus::Allowed,
            citation_policy: CitationPolicy::ShortExcerpts,
        };
        let mut file_sections = repository_sections(&source_id, &redacted, source_kind, &title);
        sources.push(source);
        sections.append(&mut file_sections);
    }
    Ok((sources, sections))
}

fn repository_source_kind(path: &Path) -> Option<&'static str> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if matches!(ext.as_str(), "md" | "markdown" | "txt" | "rst" | "adoc") {
        return Some("docs");
    }
    if name.contains("readme") || name.contains("runbook") || name.contains("troubleshoot") {
        return Some("docs");
    }
    if name.contains("openapi")
        || name.contains("swagger")
        || name.contains("api") && matches!(ext.as_str(), "yaml" | "yml" | "json")
    {
        return Some("api");
    }
    if name.contains("cli") && matches!(ext.as_str(), "yaml" | "yml" | "json" | "txt") {
        return Some("cli");
    }
    if matches!(
        ext.as_str(),
        "rs" | "py" | "ts" | "js" | "go" | "java" | "cs" | "cpp" | "hpp" | "c" | "h"
    ) {
        return Some("code");
    }
    if matches!(ext.as_str(), "toml" | "yaml" | "yml" | "json")
        && (name.contains("config") || name == "cargo.toml" || name == "package.json")
    {
        return Some("config");
    }
    None
}

fn repository_sections(
    source_id: &str,
    text: &str,
    kind: &str,
    title: &str,
) -> Vec<DocumentSection> {
    match kind {
        "docs" => {
            if title.ends_with(".md") || title.ends_with(".markdown") {
                markdown_sections(source_id, text)
            } else {
                plain_sections(source_id, text)
            }
        }
        "api" => interface_sections(source_id, text, true),
        "cli" => interface_sections(source_id, text, false),
        "code" => code_comment_sections(source_id, text, title),
        "config" => config_sections(source_id, text, title),
        _ => plain_sections(source_id, text),
    }
}

fn code_comment_sections(source_id: &str, text: &str, title: &str) -> Vec<DocumentSection> {
    let mut collected = Vec::new();
    let mut start = 1usize;
    let mut current = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let is_comment = trimmed.starts_with("//")
            || trimmed.starts_with("#")
            || trimmed.starts_with("*")
            || trimmed.starts_with("///")
            || trimmed.starts_with("/**")
            || trimmed.starts_with("<!--");
        let is_signature = trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("def ")
            || trimmed.starts_with("function ")
            || trimmed.contains(" endpoint")
            || trimmed.contains(" route");
        if is_comment || is_signature {
            if current.is_empty() {
                start = idx + 1;
            }
            current.push(line);
        } else if !current.is_empty() {
            collected.push(section_from_lines(
                source_id,
                &format!("Repository evidence from {title}"),
                start,
                idx,
                &current,
            ));
            current.clear();
        }
    }
    if !current.is_empty() {
        let end = start + current.len() - 1;
        collected.push(section_from_lines(
            source_id,
            &format!("Repository evidence from {title}"),
            start,
            end,
            &current,
        ));
    }
    collected
        .into_iter()
        .filter(|s| !s.text_excerpt.trim().is_empty())
        .collect()
}

fn config_sections(source_id: &str, text: &str, title: &str) -> Vec<DocumentSection> {
    let mut sections = plain_sections(source_id, text);
    for section in &mut sections {
        section.heading = format!("Repository configuration from {title}");
        section.detected_normative_language.push("Configuration may define operational defaults, dependencies, or toolchain expectations.".into());
    }
    sections
}

pub fn ingest_path(path: &Path) -> Result<(Vec<SourceDocument>, Vec<DocumentSection>)> {
    ingest_path_with_forced_type(path, None)
}

pub fn ingest_path_as(
    path: &Path,
    forced_type: SourceType,
) -> Result<(Vec<SourceDocument>, Vec<DocumentSection>)> {
    ingest_path_with_forced_type(path, Some(forced_type))
}

fn ingest_path_with_forced_type(
    path: &Path,
    forced_type: Option<SourceType>,
) -> Result<(Vec<SourceDocument>, Vec<DocumentSection>)> {
    let mut sources = Vec::new();
    let mut sections = Vec::new();
    let files: Vec<_> = if path.is_dir() {
        WalkDir::new(path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .collect()
    } else {
        vec![path.to_path_buf()]
    };
    for file in files {
        if should_skip(&file) {
            continue;
        }
        let text =
            std::fs::read_to_string(&file).with_context(|| format!("read {}", file.display()))?;
        let source_type = forced_type
            .clone()
            .unwrap_or_else(|| source_type_for(&file, &text));
        let source_id = stable_id("src", &file.display().to_string());
        let hash = hex_hash(text.as_bytes());
        let redacted = redact_secrets(&text);
        let source = SourceDocument {
            source_id: source_id.clone(),
            title: file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("source")
                .replace(['_', '-'], " "),
            source_type: source_type.clone(),
            origin: file.display().to_string(),
            version: detect_version(&redacted),
            license: None,
            owner: None,
            visibility: Visibility::Private,
            ingested_at: Utc::now(),
            hash,
            retention_policy: RetentionPolicy::ExcerptsOnly,
            export_policy: ExportPolicy::PrivateOnly,
            secret_scan_status: if text == redacted {
                ScanStatus::Clean
            } else {
                ScanStatus::Findings(vec!["secret-like content redacted".into()])
            },
            permission_status: PermissionStatus::Allowed,
            citation_policy: CitationPolicy::ShortExcerpts,
        };
        let mut file_sections = match source_type {
            SourceType::Markdown => markdown_sections(&source_id, &redacted),
            SourceType::OpenApi | SourceType::ApiSpec => {
                interface_sections(&source_id, &redacted, true)
            }
            SourceType::CliSpec | SourceType::CliHelp => {
                interface_sections(&source_id, &redacted, false)
            }
            SourceType::Html => html_sections(&source_id, &redacted),
            _ => plain_sections(&source_id, &redacted),
        };
        sources.push(source);
        sections.append(&mut file_sections);
    }
    Ok((sources, sections))
}

fn html_sections(source_id: &str, text: &str) -> Vec<DocumentSection> {
    let stripped = html_to_text(text);
    let mut sections = markdown_like_heading_sections(source_id, &stripped);
    if sections.is_empty() {
        sections = plain_sections(source_id, &stripped);
    }
    sections
}

fn markdown_like_heading_sections(source_id: &str, text: &str) -> Vec<DocumentSection> {
    let lines: Vec<&str> = text.lines().collect();
    let heading_re = Regex::new(r"(?m)^#{1,6}\s+(.+)$").unwrap();
    if !heading_re.is_match(text) {
        return vec![];
    }
    let mut result = Vec::new();
    let mut current_heading = "Overview".to_string();
    let mut start = 1usize;
    for (idx, raw) in lines.iter().enumerate() {
        if raw.trim_start().starts_with('#') {
            if idx + 1 > start {
                result.push(section_from_lines(
                    source_id,
                    &current_heading,
                    start,
                    idx,
                    &lines[start - 1..idx],
                ));
            }
            current_heading = raw.trim_start_matches('#').trim().to_string();
            start = idx + 1;
        }
    }
    if start <= lines.len() {
        result.push(section_from_lines(
            source_id,
            &current_heading,
            start,
            lines.len(),
            &lines[start - 1..],
        ));
    }
    result
}

fn html_to_text(text: &str) -> String {
    let mut out = text.to_string();
    out = Regex::new(r"(?is)<(script|style)[^>]*>.*?</\1>")
        .unwrap_or_else(|_| Regex::new(r"$^").unwrap())
        .replace_all(&out, "")
        .to_string();
    out = Regex::new(r"(?i)</h[1-6]>")
        .unwrap()
        .replace_all(&out, "\n")
        .to_string();
    out = Regex::new(r"(?i)<h([1-6])[^>]*>")
        .unwrap()
        .replace_all(&out, "\n# ")
        .to_string();
    out = Regex::new(r"(?i)<br\s*/?>")
        .unwrap()
        .replace_all(&out, "\n")
        .to_string();
    out = Regex::new(r"(?i)</(p|li|tr|div|section|article)>")
        .unwrap()
        .replace_all(&out, "\n")
        .to_string();
    out = Regex::new(r"(?is)<[^>]+>")
        .unwrap()
        .replace_all(&out, " ")
        .to_string();
    html_unescape(&out)
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn html_unescape(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn title_from_url_or_html(url: &url::Url, text: &str) -> String {
    if let Some(caps) = Regex::new(r"(?is)<title[^>]*>(.*?)</title>")
        .unwrap()
        .captures(text)
    {
        let title = html_unescape(&caps[1]).trim().to_string();
        if !title.is_empty() {
            return title;
        }
    }
    url.path_segments()
        .and_then(|mut s| s.next_back())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| url.host_str().unwrap_or("url-source"))
        .replace(['_', '-', '.'], " ")
}

fn same_host_links(base: &url::Url, html: &str, host: Option<&str>) -> Vec<url::Url> {
    let mut out = Vec::new();
    let href_re = Regex::new(r#"(?is)href\s*=\s*[\"']([^\"'#]+)[\"']"#).unwrap();
    for caps in href_re.captures_iter(html) {
        if let Ok(link) = base.join(caps[1].trim()) {
            if link.scheme() == "http" || link.scheme() == "https" {
                if host.is_none() || link.host_str() == host {
                    out.push(link);
                }
            }
        }
    }
    out.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    out.dedup_by(|a, b| a.as_str() == b.as_str());
    out
}

fn should_skip(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("/.git/")
        || s.contains("target/")
        || s.contains("legacy/python/skiller/__pycache__")
        || s.ends_with(".pyc")
}

fn source_type_for(path: &Path, text: &str) -> SourceType {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext == "md" || ext == "markdown" {
        SourceType::Markdown
    } else if ext == "html" || ext == "htm" {
        SourceType::Html
    } else if name.contains("openapi") || text.contains("openapi:") || text.contains("swagger:") {
        SourceType::OpenApi
    } else if name.contains("cli") && (ext == "yaml" || ext == "yml" || ext == "json") {
        SourceType::CliSpec
    } else if name.contains("help") || text.contains("USAGE") || text.contains("Usage:") {
        SourceType::CliHelp
    } else if ext == "yaml" || ext == "yml" || ext == "json" {
        SourceType::ApiSpec
    } else {
        SourceType::Text
    }
}

fn markdown_sections(source_id: &str, text: &str) -> Vec<DocumentSection> {
    let mut headings = Vec::<(usize, String)>::new();
    let mut line = 1usize;
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Heading { .. }) => {}
            Event::Text(t) => headings.push((line, t.to_string())),
            Event::SoftBreak | Event::HardBreak => line += 1,
            _ => {}
        }
    }
    if headings.is_empty() {
        return plain_sections(source_id, text);
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut result = Vec::new();
    let mut current_heading = "Overview".to_string();
    let mut start = 1usize;
    for (idx, raw) in lines.iter().enumerate() {
        if raw.trim_start().starts_with('#') {
            if idx + 1 > start {
                result.push(section_from_lines(
                    source_id,
                    &current_heading,
                    start,
                    idx,
                    &lines[start - 1..idx],
                ));
            }
            current_heading = raw.trim_start_matches('#').trim().to_string();
            start = idx + 1;
        }
    }
    if start <= lines.len() {
        result.push(section_from_lines(
            source_id,
            &current_heading,
            start,
            lines.len(),
            &lines[start - 1..],
        ));
    }
    result
}

fn plain_sections(source_id: &str, text: &str) -> Vec<DocumentSection> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    for (i, chunk) in lines.chunks(80).enumerate() {
        let start = i * 80 + 1;
        let end = start + chunk.len() - 1;
        out.push(section_from_lines(
            source_id,
            if i == 0 { "Overview" } else { "Continuation" },
            start,
            end,
            chunk,
        ));
    }
    out
}

fn interface_sections(source_id: &str, text: &str, api: bool) -> Vec<DocumentSection> {
    let mut sections = plain_sections(source_id, text);
    for section in &mut sections {
        section.detected_api_operations = if api {
            detect_api_ops(&section.text_excerpt)
        } else {
            vec![]
        };
        section.detected_commands = if api {
            vec![]
        } else {
            detect_commands(&section.text_excerpt)
        };
    }
    sections
}

fn section_from_lines(
    source_id: &str,
    heading: &str,
    start: usize,
    end: usize,
    lines: &[&str],
) -> DocumentSection {
    let text = lines.join("\n");
    DocumentSection {
        section_id: stable_id("sec", &format!("{source_id}:{start}:{heading}")),
        source_id: source_id.to_string(),
        heading: heading.to_string(),
        breadcrumbs: vec![heading.to_string()],
        line_start: start,
        line_end: end,
        text_excerpt: truncate(&text, 1400),
        code_blocks: detect_code_blocks(&text),
        links: detect_links(&text),
        detected_commands: detect_commands(&text),
        detected_api_operations: detect_api_ops(&text),
        detected_warnings: detect_warnings(&text),
        detected_examples: detect_examples(&text),
        detected_normative_language: detect_normative(&text),
    }
}

fn detect_code_blocks(text: &str) -> Vec<String> {
    text.split("```")
        .skip(1)
        .step_by(2)
        .map(|s| truncate(s.trim(), 600))
        .collect()
}
fn detect_links(text: &str) -> Vec<String> {
    Regex::new(r"https?://\S+")
        .unwrap()
        .find_iter(text)
        .map(|m| m.as_str().trim_end_matches(')').to_string())
        .collect()
}
fn detect_commands(text: &str) -> Vec<String> {
    Regex::new(r"(?m)^\s*(?:\$\s*)?([a-zA-Z][\w.-]+(?:\s+[-\w./:=]+){1,8})\s*$")
        .unwrap()
        .captures_iter(text)
        .map(|c| c[1].to_string())
        .take(20)
        .collect()
}
fn detect_api_ops(text: &str) -> Vec<String> {
    Regex::new(r"(?i)\b(GET|POST|PUT|PATCH|DELETE)\s+(/[A-Za-z0-9_./{}-]+)")
        .unwrap()
        .captures_iter(text)
        .map(|c| format!("{} {}", &c[1].to_uppercase(), &c[2]))
        .take(20)
        .collect()
}
fn detect_warnings(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| {
            l.to_lowercase().contains("warning")
                || l.to_lowercase().contains("danger")
                || l.to_lowercase().contains("caution")
        })
        .map(|l| truncate(l.trim(), 240))
        .collect()
}
fn detect_examples(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.to_lowercase().contains("example") || l.trim_start().starts_with('$'))
        .map(|l| truncate(l.trim(), 240))
        .collect()
}
fn detect_normative(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| {
            ["must", "should", "required", "never", "always"]
                .iter()
                .any(|w| l.to_lowercase().contains(w))
        })
        .map(|l| truncate(l.trim(), 240))
        .collect()
}
fn detect_version(text: &str) -> Option<String> {
    Regex::new(r"(?i)\b(?:version|v)\s*([0-9]+\.[0-9]+(?:\.[0-9]+)?)")
        .unwrap()
        .captures(text)
        .map(|c| c[1].to_string())
}
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}
fn redact_secrets(text: &str) -> String {
    Regex::new(r"(?i)(api[_-]?key|token|password|secret)\s*[:=]\s*\S+")
        .unwrap()
        .replace_all(text, "$1=<REDACTED>")
        .to_string()
}
pub fn stable_id(prefix: &str, input: &str) -> String {
    let h = hex_hash(input.as_bytes());
    format!("{}-{}", prefix, &h[..12])
}
pub fn hex_hash(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}
fn _uuid() -> String {
    Uuid::new_v4().to_string()
}
