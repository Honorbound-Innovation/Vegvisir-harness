use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use regex::Regex;

pub fn get_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            load_environment_d(None)
                .ok()
                .and_then(|values| values.get(name).cloned())
        })
}

pub fn load_environment_d(root: Option<&Path>) -> anyhow::Result<BTreeMap<String, String>> {
    let config_root = root
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/environment.d"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    let mut values = BTreeMap::new();
    if !config_root.is_dir() {
        return Ok(values);
    }
    let mut paths = fs::read_dir(config_root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("conf"))
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        let text = fs::read_to_string(path)?;
        for line in text.lines() {
            if let Some((key, value)) = parse_environment_line(line) {
                values.insert(key, value);
            }
        }
    }
    Ok(values)
}

pub fn parse_environment_line(raw_line: &str) -> Option<(String, String)> {
    let mut line = raw_line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    if let Some(rest) = line.strip_prefix("export ") {
        line = rest.trim();
    }
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    let name_re = Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").ok()?;
    if !name_re.is_match(key) {
        return None;
    }
    Some((key.to_string(), parse_environment_value(value.trim())))
}

fn parse_environment_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.as_bytes()[0];
        let last = trimmed.as_bytes()[trimmed.len() - 1];
        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}
