use std::{
    env, fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileExplorerEntry {
    name: String,
    path: String,
    is_dir: bool,
    is_file: bool,
    is_symlink: bool,
    size: Option<u64>,
    modified_ms: Option<u128>,
    git_repo: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileExplorerListing {
    path: String,
    parent: Option<String>,
    home: Option<String>,
    entries: Vec<FileExplorerEntry>,
    truncated: bool,
    total_entries: usize,
    limit: usize,
}

const FILE_EXPLORER_ENTRY_LIMIT: usize = 800;

fn default_browser_path() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn modified_ms(modified: SystemTime) -> Option<u128> {
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

fn list_directory_blocking(path: Option<String>) -> Result<FileExplorerListing, String> {
    let requested = path
        .filter(|value| !value.trim().is_empty())
        .map(|value| PathBuf::from(value.trim()))
        .unwrap_or_else(default_browser_path);
    let directory = fs::canonicalize(&requested).map_err(|error| {
        format!(
            "failed to resolve directory '{}': {error}",
            requested.display()
        )
    })?;

    if !directory.is_dir() {
        return Err(format!("'{}' is not a directory", directory.display()));
    }

    let mut entries = Vec::new();
    let mut total_entries = 0usize;
    let mut truncated = false;
    for item in fs::read_dir(&directory).map_err(|error| {
        format!(
            "failed to read directory '{}': {error}",
            directory.display()
        )
    })? {
        let item = item.map_err(|error| error.to_string())?;
        total_entries += 1;
        if entries.len() >= FILE_EXPLORER_ENTRY_LIMIT {
            truncated = true;
            break;
        }
        let path = item.path();
        let file_type = item.file_type().ok();
        let is_dir = file_type.as_ref().is_some_and(|kind| kind.is_dir());
        let is_file = file_type.as_ref().is_some_and(|kind| kind.is_file());
        let is_symlink = file_type.as_ref().is_some_and(|kind| kind.is_symlink());
        let metadata = if is_file { item.metadata().ok() } else { None };
        let name = item.file_name().to_string_lossy().to_string();
        let git_repo = false;
        entries.push(FileExplorerEntry {
            name,
            path: path.display().to_string(),
            is_dir,
            is_file,
            is_symlink,
            size: metadata.as_ref().map(|metadata| metadata.len()),
            modified_ms: metadata
                .and_then(|metadata| metadata.modified().ok())
                .and_then(modified_ms),
            git_repo,
        });
    }

    entries.sort_by_cached_key(|entry| (!entry.is_dir, entry.name.to_lowercase()));

    Ok(FileExplorerListing {
        parent: directory.parent().map(|path| path.display().to_string()),
        home: env::var_os("HOME").map(|path| PathBuf::from(path).display().to_string()),
        path: directory.display().to_string(),
        entries,
        truncated,
        total_entries,
        limit: FILE_EXPLORER_ENTRY_LIMIT,
    })
}

#[tauri::command]
pub async fn fs_list_directory(path: Option<String>) -> Result<FileExplorerListing, String> {
    tauri::async_runtime::spawn_blocking(move || list_directory_blocking(path))
        .await
        .map_err(|error| error.to_string())?
}
