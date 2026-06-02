use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use crate::command_sandbox::{CommandSandboxConfig, CommandSandboxMode, network_policy_label};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSandboxStatus {
    pub workspace_file_containment: &'static str,
    pub file_access_hardening: &'static str,
    pub command_os_sandbox: &'static str,
    pub bubblewrap_available: bool,
    pub dangerous_bypass: &'static str,
    pub command_network_policy: &'static str,
    pub writable_mount_policy: String,
    pub readonly_mount_policy: String,
}

impl CommandSandboxStatus {
    pub fn current(dangerous_bypass: bool, workspace_root: impl Into<PathBuf>) -> Self {
        let bubblewrap_available = command_exists("bwrap");
        let configured = CommandSandboxConfig::from_env(workspace_root, dangerous_bypass);
        let (
            command_os_sandbox,
            command_network_policy,
            writable_mount_policy,
            readonly_mount_policy,
        ) = match configured {
            Ok(config) if dangerous_bypass => (
                "disabled by dangerous bypass",
                network_policy_label(&config.network),
                "not applicable".to_string(),
                "not applicable".to_string(),
            ),
            Ok(config) => match config.mode {
                CommandSandboxMode::None => (
                    "disabled",
                    network_policy_label(&config.network),
                    "not applicable".to_string(),
                    "not applicable".to_string(),
                ),
                CommandSandboxMode::PathOnly => (
                    "unavailable/not configured",
                    network_policy_label(&config.network),
                    "not applicable".to_string(),
                    "not applicable".to_string(),
                ),
                CommandSandboxMode::Bubblewrap => (
                    if bubblewrap_available {
                        "configured: bubblewrap"
                    } else {
                        "configured but bubblewrap unavailable"
                    },
                    network_policy_label(&config.network),
                    mount_policy_label("workspace writable", config.writable_paths.len()),
                    mount_policy_label("system mounts read-only", config.readonly_paths.len()),
                ),
                CommandSandboxMode::StrictBubblewrap => (
                    if bubblewrap_available {
                        "configured: strict-bubblewrap"
                    } else {
                        "configured but bubblewrap unavailable"
                    },
                    network_policy_label(&config.network),
                    mount_policy_label("workspace writable", config.writable_paths.len()),
                    mount_policy_label("system mounts read-only", config.readonly_paths.len()),
                ),
            },
            Err(_) => (
                "invalid configuration",
                "unknown",
                "unknown".to_string(),
                "unknown".to_string(),
            ),
        };
        Self {
            workspace_file_containment: if dangerous_bypass {
                "disabled by dangerous bypass"
            } else {
                "active"
            },
            file_access_hardening: if dangerous_bypass {
                "disabled by dangerous bypass"
            } else {
                file_access_hardening_label()
            },
            command_os_sandbox,
            bubblewrap_available,
            dangerous_bypass: if dangerous_bypass {
                "enabled at startup"
            } else {
                "disabled"
            },
            command_network_policy,
            writable_mount_policy,
            readonly_mount_policy,
        }
    }

    pub fn tools_status_lines(&self) -> String {
        format!(
            "Workspace file containment: {}\nFile access hardening: {}\nCommand OS sandbox: {}\nBubblewrap available: {}\nDangerous bypass: {}\nCommand network policy: {}\nWritable mount policy: {}\nRead-only mount policy: {}",
            self.workspace_file_containment,
            self.file_access_hardening,
            self.command_os_sandbox,
            if self.bubblewrap_available {
                "yes"
            } else {
                "no"
            },
            self.dangerous_bypass,
            self.command_network_policy,
            self.writable_mount_policy,
            self.readonly_mount_policy
        )
    }

    pub fn verify_runtime_lines(&self) -> Vec<String> {
        vec![
            format!(
                "ok runtime/workspace_file_containment {}",
                self.workspace_file_containment.replace(' ', "_")
            ),
            format!(
                "ok runtime/file_access_hardening {}",
                self.file_access_hardening.replace(' ', "_")
            ),
            format!(
                "ok runtime/command_os_sandbox {}",
                self.command_os_sandbox.replace(' ', "_")
            ),
            format!(
                "ok runtime/bubblewrap_available {}",
                if self.bubblewrap_available {
                    "yes"
                } else {
                    "no"
                }
            ),
            format!(
                "ok runtime/command_network_policy {}",
                self.command_network_policy
            ),
            format!(
                "ok runtime/command_mount_policy writable={} readonly={}",
                self.writable_mount_policy.replace(' ', "_"),
                self.readonly_mount_policy.replace(' ', "_")
            ),
        ]
    }
}

fn command_exists(name: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|dir| dir.join(name).is_file())
}

fn mount_policy_label(base: &'static str, extra_paths: usize) -> String {
    if extra_paths == 0 {
        base.to_string()
    } else {
        format!("{base} plus {extra_paths} configured path(s)")
    }
}

fn file_access_hardening_label() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "openat2 existing paths plus symlink rejection"
    }
    #[cfg(not(target_os = "linux"))]
    {
        "symlink rejection"
    }
}

#[derive(Clone, Debug)]
pub struct WorkspaceSandbox {
    pub root: PathBuf,
    pub allow_network: bool,
    pub bypass: bool,
}

impl WorkspaceSandbox {
    pub fn new(root: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(Self {
            root: root.as_ref().canonicalize()?,
            allow_network: false,
            bypass: false,
        })
    }

    pub fn new_unrestricted(root: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(Self {
            root: root.as_ref().canonicalize()?,
            allow_network: true,
            bypass: true,
        })
    }

    pub fn resolve(&self, path: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
        let path = path.as_ref();
        let raw = path.to_string_lossy().to_string();
        if !self.bypass && (raw == "~" || raw.starts_with("~/")) {
            anyhow::bail!(
                "Home-relative paths are not supported in workspace tools; use a workspace-relative path."
            );
        }
        let candidate = if path.is_absolute() {
            path.components().collect::<PathBuf>()
        } else {
            self.root.join(path).components().collect::<PathBuf>()
        };
        let resolved = if candidate.exists() {
            candidate.canonicalize()?
        } else {
            self.resolve_missing_candidate(&candidate)?
        };
        if !self.bypass && resolved != self.root && !resolved.starts_with(&self.root) {
            anyhow::bail!("Path escapes workspace: {raw}");
        }
        Ok(resolved)
    }

    fn resolve_missing_candidate(&self, candidate: &Path) -> anyhow::Result<PathBuf> {
        let mut existing = candidate;
        let mut missing_components = Vec::new();
        while !existing.exists() {
            let Some(parent) = existing.parent() else {
                anyhow::bail!("Path has no existing ancestor: {}", candidate.display());
            };
            if let Some(name) = existing.file_name() {
                missing_components.push(name.to_os_string());
            }
            existing = parent;
        }
        let mut resolved = existing.canonicalize()?;
        for component in missing_components.into_iter().rev() {
            resolved.push(component);
        }
        Ok(resolved)
    }

    pub fn read_text(&self, path: impl AsRef<Path>) -> anyhow::Result<String> {
        let requested = path.as_ref();
        if !self.bypass {
            reject_requested_symlink_components(self.root.as_path(), requested)?;
            #[cfg(target_os = "linux")]
            if let Some(mut file) =
                openat2_read_beneath(self.root.as_path(), requested).transpose()?
            {
                let mut content = String::new();
                file.read_to_string(&mut content)?;
                return Ok(content);
            }
        }
        let target = self.resolve(requested)?;
        let mut file = open_read_no_follow(&target, self.bypass)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        Ok(content)
    }

    pub fn write_text(&self, path: impl AsRef<Path>, content: &str) -> anyhow::Result<PathBuf> {
        let requested = path.as_ref();
        if !self.bypass {
            reject_requested_symlink_components(self.root.as_path(), requested)?;
            #[cfg(target_os = "linux")]
            if let Some(mut file) =
                openat2_write_beneath(self.root.as_path(), requested).transpose()?
            {
                file.write_all(content.as_bytes())?;
                return self.resolve(requested);
            }
        }
        let target = self.resolve(requested)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = open_write_no_follow(&target, self.bypass)?;
        file.write_all(content.as_bytes())?;
        Ok(target)
    }
}

#[cfg(target_os = "linux")]
fn openat2_read_beneath(root: &Path, path: &Path) -> Option<anyhow::Result<fs::File>> {
    let relative = match workspace_relative_path(root, path) {
        Ok(relative) => relative,
        Err(error) => return Some(Err(error)),
    };
    if !root.join(&relative).exists() {
        return None;
    }
    Some(openat2_beneath(
        root,
        &relative,
        libc::O_RDONLY | libc::O_CLOEXEC,
        0,
    ))
}

#[cfg(target_os = "linux")]
fn openat2_write_beneath(root: &Path, path: &Path) -> Option<anyhow::Result<fs::File>> {
    let relative = match workspace_relative_path(root, path) {
        Ok(relative) => relative,
        Err(error) => return Some(Err(error)),
    };
    if root.join(&relative).exists() {
        return Some(openat2_beneath(
            root,
            &relative,
            libc::O_WRONLY | libc::O_TRUNC | libc::O_CLOEXEC,
            0,
        ));
    }

    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let parent_on_host = if parent.as_os_str().is_empty() {
        root.to_path_buf()
    } else {
        root.join(parent)
    };
    if !parent_on_host.exists() {
        return None;
    }

    Some(openat2_create_beneath(
        root,
        parent,
        relative
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Path has no file name: {}", path.display())),
    ))
}

#[cfg(target_os = "linux")]
fn workspace_relative_path(root: &Path, path: &Path) -> anyhow::Result<PathBuf> {
    let relative = if path.is_absolute() {
        path.strip_prefix(root)
            .map_err(|_| anyhow::anyhow!("Path escapes workspace: {}", path.display()))?
            .to_path_buf()
    } else {
        path.components().collect::<PathBuf>()
    };
    for component in relative.components() {
        if matches!(component, std::path::Component::ParentDir) {
            anyhow::bail!("Path escapes workspace: {}", path.display());
        }
    }
    Ok(relative)
}

#[cfg(target_os = "linux")]
fn openat2_beneath(
    root: &Path,
    relative: &Path,
    flags: i32,
    mode: libc::mode_t,
) -> anyhow::Result<fs::File> {
    use std::os::fd::AsRawFd;

    let root_dir = fs::File::open(root)?;
    openat2_at(
        root_dir.as_raw_fd(),
        relative,
        flags,
        mode,
        openat2_beneath_resolve(),
    )
}

#[cfg(target_os = "linux")]
fn openat2_create_beneath(
    root: &Path,
    parent: &Path,
    file_name: anyhow::Result<&std::ffi::OsStr>,
) -> anyhow::Result<fs::File> {
    use std::os::fd::AsRawFd;

    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let parent_dir = openat2_beneath(
        root,
        parent,
        libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
        0,
    )?;
    openat2_at(
        parent_dir.as_raw_fd(),
        Path::new(file_name?),
        libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC,
        0o644,
        openat2_local_resolve(),
    )
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

#[cfg(target_os = "linux")]
const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
#[cfg(target_os = "linux")]
const RESOLVE_NO_SYMLINKS: u64 = 0x04;
#[cfg(target_os = "linux")]
const RESOLVE_BENEATH: u64 = 0x08;

#[cfg(target_os = "linux")]
fn openat2_beneath_resolve() -> u64 {
    RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS
}

#[cfg(target_os = "linux")]
fn openat2_local_resolve() -> u64 {
    RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS
}

#[cfg(target_os = "linux")]
fn openat2_at(
    dirfd: std::os::fd::RawFd,
    path: &Path,
    flags: i32,
    mode: libc::mode_t,
    resolve: u64,
) -> anyhow::Result<fs::File> {
    use std::{ffi::CString, os::fd::FromRawFd, os::unix::ffi::OsStrExt};

    let path = CString::new(path.as_os_str().as_bytes())?;
    let how = OpenHow {
        flags: flags as u64,
        mode: mode as u64,
        resolve,
    };
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            dirfd,
            path.as_ptr(),
            &how,
            std::mem::size_of::<OpenHow>(),
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if matches!(
            error.raw_os_error(),
            Some(libc::ENOSYS | libc::EINVAL | libc::E2BIG)
        ) {
            anyhow::bail!("Linux openat2 is unavailable or unsupported on this system: {error}");
        }
        return Err(error.into());
    }
    Ok(unsafe { fs::File::from_raw_fd(fd as i32) })
}

fn open_read_no_follow(path: &Path, bypass: bool) -> anyhow::Result<fs::File> {
    if !bypass {
        reject_symlink(path)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        return Ok(fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?);
    }
    #[cfg(not(unix))]
    {
        Ok(fs::OpenOptions::new().read(true).open(path)?)
    }
}

fn open_write_no_follow(path: &Path, bypass: bool) -> anyhow::Result<fs::File> {
    if !bypass {
        reject_symlink(path)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        return Ok(fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?);
    }
    #[cfg(not(unix))]
    {
        Ok(fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?)
    }
}

fn reject_symlink(path: &Path) -> anyhow::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        anyhow::bail!("Refusing to follow symlink path: {}", path.display());
    }
    Ok(())
}

fn reject_requested_symlink_components(root: &Path, path: &Path) -> anyhow::Result<()> {
    let candidate = if path.is_absolute() {
        path.components().collect::<PathBuf>()
    } else {
        root.join(path).components().collect::<PathBuf>()
    };
    let mut current = PathBuf::new();
    for component in candidate.components() {
        current.push(component.as_os_str());
        reject_symlink(&current)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_new_nested_paths_inside_workspace() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let sandbox = WorkspaceSandbox::new(dir.path())?;
        let written = sandbox.write_text("new/dir/file.txt", "ok")?;
        assert!(written.starts_with(dir.path().canonicalize()?));
        assert_eq!(
            fs::read_to_string(dir.path().join("new/dir/file.txt"))?,
            "ok"
        );
        Ok(())
    }

    #[test]
    fn rejects_missing_paths_that_escape_workspace() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let sandbox = WorkspaceSandbox::new(dir.path())?;
        let err = sandbox
            .write_text("../escape/file.txt", "nope")
            .unwrap_err();
        assert!(err.to_string().contains("escapes workspace"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn read_text_rejects_final_symlink() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let dir = tempdir()?;
        fs::write(dir.path().join("real.txt"), "secret")?;
        symlink(dir.path().join("real.txt"), dir.path().join("link.txt"))?;
        let sandbox = WorkspaceSandbox::new(dir.path())?;
        let err = sandbox.read_text("link.txt").unwrap_err();
        assert!(err.to_string().contains("Refusing to follow symlink path"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn write_text_rejects_final_symlink() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let dir = tempdir()?;
        fs::write(dir.path().join("real.txt"), "original")?;
        symlink(dir.path().join("real.txt"), dir.path().join("link.txt"))?;
        let sandbox = WorkspaceSandbox::new(dir.path())?;
        let err = sandbox.write_text("link.txt", "updated").unwrap_err();
        assert!(err.to_string().contains("Refusing to follow symlink path"));
        assert_eq!(fs::read_to_string(dir.path().join("real.txt"))?, "original");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn read_text_rejects_symlink_parent_component() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let dir = tempdir()?;
        fs::create_dir(dir.path().join("real"))?;
        fs::write(dir.path().join("real").join("file.txt"), "secret")?;
        symlink(dir.path().join("real"), dir.path().join("link-dir"))?;
        let sandbox = WorkspaceSandbox::new(dir.path())?;
        let err = sandbox.read_text("link-dir/file.txt").unwrap_err();
        assert!(err.to_string().contains("Refusing to follow symlink path"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn write_text_rejects_symlink_parent_component() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let dir = tempdir()?;
        fs::create_dir(dir.path().join("real"))?;
        fs::write(dir.path().join("real").join("file.txt"), "original")?;
        symlink(dir.path().join("real"), dir.path().join("link-dir"))?;
        let sandbox = WorkspaceSandbox::new(dir.path())?;
        let err = sandbox
            .write_text("link-dir/file.txt", "updated")
            .unwrap_err();
        assert!(err.to_string().contains("Refusing to follow symlink path"));
        assert_eq!(
            fs::read_to_string(dir.path().join("real").join("file.txt"))?,
            "original"
        );
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn read_text_uses_openat2_for_existing_file() -> anyhow::Result<()> {
        let dir = tempdir()?;
        fs::write(dir.path().join("file.txt"), "openat2-read")?;
        let sandbox = WorkspaceSandbox::new(dir.path())?;
        assert_eq!(sandbox.read_text("file.txt")?, "openat2-read");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn write_text_uses_openat2_for_existing_file() -> anyhow::Result<()> {
        let dir = tempdir()?;
        fs::write(dir.path().join("file.txt"), "original")?;
        let sandbox = WorkspaceSandbox::new(dir.path())?;
        let written = sandbox.write_text("file.txt", "openat2-write")?;
        assert_eq!(written, dir.path().join("file.txt").canonicalize()?);
        assert_eq!(
            fs::read_to_string(dir.path().join("file.txt"))?,
            "openat2-write"
        );
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn write_text_uses_openat2_for_new_file_with_existing_parent() -> anyhow::Result<()> {
        let dir = tempdir()?;
        fs::create_dir(dir.path().join("existing"))?;
        let sandbox = WorkspaceSandbox::new(dir.path())?;
        let written = sandbox.write_text("existing/new.txt", "openat2-create")?;
        assert_eq!(
            written,
            dir.path().join("existing").join("new.txt").canonicalize()?
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("existing").join("new.txt"))?,
            "openat2-create"
        );
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn read_text_rejects_parent_escape_before_openat2() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let sandbox = WorkspaceSandbox::new(dir.path())?;
        let err = sandbox.read_text("../outside.txt").unwrap_err();
        assert!(err.to_string().contains("Path escapes workspace"));
        Ok(())
    }
}
