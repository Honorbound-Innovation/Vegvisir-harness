use std::{
    env,
    path::{Path, PathBuf},
};

pub const COMMAND_SANDBOX_ENV: &str = "VEGVISIR_COMMAND_SANDBOX";
pub const COMMAND_SANDBOX_NETWORK_ENV: &str = "VEGVISIR_COMMAND_NETWORK";
pub const COMMAND_SANDBOX_WRITABLE_PATHS_ENV: &str = "VEGVISIR_COMMAND_WRITABLE_PATHS";
pub const COMMAND_SANDBOX_READONLY_PATHS_ENV: &str = "VEGVISIR_COMMAND_READONLY_PATHS";
pub const COMMAND_SANDBOX_HIDDEN_PATHS_ENV: &str = "VEGVISIR_COMMAND_HIDDEN_PATHS";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandSandboxMode {
    None,
    PathOnly,
    Bubblewrap,
    StrictBubblewrap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxNetworkPolicy {
    Inherit,
    Disable,
    RequireApproval,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSandboxConfig {
    pub mode: CommandSandboxMode,
    pub bwrap_path: Option<PathBuf>,
    pub workspace_root: PathBuf,
    pub network: SandboxNetworkPolicy,
    pub writable_paths: Vec<PathBuf>,
    pub readonly_paths: Vec<PathBuf>,
    pub hidden_paths: Vec<PathBuf>,
    pub clear_env: bool,
    pub private_tmp: bool,
    pub private_home: bool,
    pub allow_dangerous_bypass: bool,
}

impl CommandSandboxConfig {
    pub fn from_env(
        workspace_root: impl Into<PathBuf>,
        allow_dangerous_bypass: bool,
    ) -> anyhow::Result<Self> {
        let workspace_root = workspace_root.into();
        let raw = env::var(COMMAND_SANDBOX_ENV).unwrap_or_else(|_| "path".to_string());
        let normalized = raw.trim().to_ascii_lowercase();
        let mut config = match normalized.as_str() {
            "" | "path" | "path-only" | "pathonly" => Self::path_only(workspace_root),
            "none" | "off" | "disabled" => {
                let mut config = Self::path_only(workspace_root);
                config.mode = CommandSandboxMode::None;
                config
            }
            "bwrap" | "bubblewrap" => Self::bubblewrap(workspace_root),
            "strict-bwrap" | "strict_bwrap" | "strict-bubblewrap" | "strict_bubblewrap" => {
                Self::strict_bubblewrap(workspace_root)
            }
            other => anyhow::bail!(
                "Unsupported {COMMAND_SANDBOX_ENV} value: {other}. Use path, none, bwrap, or strict-bwrap."
            ),
        };
        if !matches!(config.mode, CommandSandboxMode::StrictBubblewrap) {
            config.network = configured_network_policy(config.network.clone())?;
        }
        apply_mount_path_env(&mut config)?;
        config.allow_dangerous_bypass = allow_dangerous_bypass;
        Ok(config)
    }

    pub fn path_only(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            mode: CommandSandboxMode::PathOnly,
            bwrap_path: None,
            workspace_root: workspace_root.into(),
            network: SandboxNetworkPolicy::Inherit,
            writable_paths: Vec::new(),
            readonly_paths: Vec::new(),
            hidden_paths: Vec::new(),
            clear_env: false,
            private_tmp: false,
            private_home: false,
            allow_dangerous_bypass: false,
        }
    }

    pub fn bubblewrap(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            mode: CommandSandboxMode::Bubblewrap,
            bwrap_path: None,
            workspace_root: workspace_root.into(),
            network: SandboxNetworkPolicy::Inherit,
            writable_paths: Vec::new(),
            readonly_paths: default_readonly_mounts(),
            hidden_paths: Vec::new(),
            clear_env: false,
            private_tmp: true,
            private_home: true,
            allow_dangerous_bypass: false,
        }
    }

    pub fn strict_bubblewrap(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            mode: CommandSandboxMode::StrictBubblewrap,
            bwrap_path: None,
            workspace_root: workspace_root.into(),
            network: SandboxNetworkPolicy::Disable,
            writable_paths: Vec::new(),
            readonly_paths: default_readonly_mounts(),
            hidden_paths: Vec::new(),
            clear_env: true,
            private_tmp: true,
            private_home: true,
            allow_dangerous_bypass: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxedCommand {
    pub program: String,
    pub args: Vec<String>,
    pub current_dir: PathBuf,
    pub sandboxed: bool,
    pub sandbox_kind: String,
    pub network_policy: String,
}

pub fn build_sandboxed_command(
    parts: &[&str],
    config: &CommandSandboxConfig,
) -> anyhow::Result<SandboxedCommand> {
    if parts.is_empty() {
        anyhow::bail!("Empty command");
    }
    if config.allow_dangerous_bypass
        || matches!(
            config.mode,
            CommandSandboxMode::None | CommandSandboxMode::PathOnly
        )
    {
        return Ok(SandboxedCommand {
            program: parts[0].to_string(),
            args: parts[1..].iter().map(|part| part.to_string()).collect(),
            current_dir: config.workspace_root.clone(),
            sandboxed: false,
            sandbox_kind: match config.mode {
                CommandSandboxMode::None => "none",
                CommandSandboxMode::PathOnly => "path-only",
                CommandSandboxMode::Bubblewrap => "disabled-by-dangerous-bypass",
                CommandSandboxMode::StrictBubblewrap => "disabled-by-dangerous-bypass",
            }
            .to_string(),
            network_policy: network_policy_label(&config.network).to_string(),
        });
    }
    if !config.workspace_root.is_dir() {
        anyhow::bail!(
            "Workspace root does not exist or is not a directory: {}",
            config.workspace_root.display()
        );
    }
    let bwrap = resolve_bwrap_path(config.bwrap_path.as_deref())?;
    let mut args = vec![
        "--die-with-parent".to_string(),
        "--unshare-pid".to_string(),
        "--unshare-ipc".to_string(),
        "--unshare-uts".to_string(),
        "--proc".to_string(),
        "/proc".to_string(),
        "--dev".to_string(),
        "/dev".to_string(),
    ];
    if config.clear_env {
        args.push("--clearenv".to_string());
        args.push("--setenv".to_string());
        args.push("PATH".to_string());
        args.push(default_sandbox_path().to_string());
    } else {
        for key in sensitive_environment_names() {
            args.push("--unsetenv".to_string());
            args.push(key);
        }
    }
    if matches!(config.network, SandboxNetworkPolicy::Disable) {
        args.push("--unshare-net".to_string());
    }
    if config.private_tmp {
        args.push("--tmpfs".to_string());
        args.push("/tmp".to_string());
    }
    if config.private_home {
        args.push("--tmpfs".to_string());
        args.push("/home".to_string());
        args.push("--dir".to_string());
        args.push("/home/vegvisir".to_string());
        args.push("--setenv".to_string());
        args.push("HOME".to_string());
        args.push("/home/vegvisir".to_string());
    }
    push_existing_readonly_mounts(&mut args, &config.readonly_paths);
    for path in &config.writable_paths {
        validate_mount_path(path)?;
        push_mount(&mut args, "--bind", path, path);
    }
    push_mount(
        &mut args,
        "--bind",
        &config.workspace_root,
        Path::new("/workspace"),
    );
    push_hidden_mounts(&mut args, &config.hidden_paths)?;
    args.push("--chdir".to_string());
    args.push("/workspace".to_string());
    args.push("--".to_string());
    args.extend(parts.iter().map(|part| part.to_string()));
    Ok(SandboxedCommand {
        program: bwrap.display().to_string(),
        args,
        current_dir: config.workspace_root.clone(),
        sandboxed: true,
        sandbox_kind: match config.mode {
            CommandSandboxMode::Bubblewrap => "bubblewrap",
            CommandSandboxMode::StrictBubblewrap => "strict-bubblewrap",
            CommandSandboxMode::None | CommandSandboxMode::PathOnly => unreachable!(),
        }
        .to_string(),
        network_policy: network_policy_label(&config.network).to_string(),
    })
}

fn default_readonly_mounts() -> Vec<PathBuf> {
    ["/usr", "/bin", "/sbin", "/lib", "/lib64", "/etc/ssl/certs"]
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

fn default_sandbox_path() -> &'static str {
    "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
}

fn configured_network_policy(
    default: SandboxNetworkPolicy,
) -> anyhow::Result<SandboxNetworkPolicy> {
    let Ok(raw) = env::var(COMMAND_SANDBOX_NETWORK_ENV) else {
        return Ok(default);
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "" => Ok(default),
        "inherit" | "host" => Ok(SandboxNetworkPolicy::Inherit),
        "disable" | "disabled" | "none" | "off" => Ok(SandboxNetworkPolicy::Disable),
        "approval" | "require-approval" | "require_approval" => {
            Ok(SandboxNetworkPolicy::RequireApproval)
        }
        other => anyhow::bail!(
            "Unsupported {COMMAND_SANDBOX_NETWORK_ENV} value: {other}. Use inherit, disable, or require-approval."
        ),
    }
}

fn apply_mount_path_env(config: &mut CommandSandboxConfig) -> anyhow::Result<()> {
    if let Some(paths) = configured_path_list(COMMAND_SANDBOX_WRITABLE_PATHS_ENV)? {
        config.writable_paths = paths;
    }
    if let Some(paths) = configured_path_list(COMMAND_SANDBOX_READONLY_PATHS_ENV)? {
        config.readonly_paths = paths;
    }
    if let Some(paths) = configured_path_list(COMMAND_SANDBOX_HIDDEN_PATHS_ENV)? {
        config.hidden_paths.extend(paths);
        config.hidden_paths.sort();
        config.hidden_paths.dedup();
    }
    Ok(())
}

fn configured_path_list(key: &str) -> anyhow::Result<Option<Vec<PathBuf>>> {
    let Some(raw) = env::var_os(key) else {
        return Ok(None);
    };
    let mut paths = Vec::new();
    for path in env::split_paths(&raw) {
        if path.as_os_str().is_empty() {
            continue;
        }
        if !path.is_absolute() {
            anyhow::bail!("{key} entries must be absolute paths: {}", path.display());
        }
        paths.push(path);
    }
    paths.sort();
    paths.dedup();
    Ok(Some(paths))
}

pub fn network_policy_label(policy: &SandboxNetworkPolicy) -> &'static str {
    match policy {
        SandboxNetworkPolicy::Inherit => "inherit",
        SandboxNetworkPolicy::Disable => "disabled",
        SandboxNetworkPolicy::RequireApproval => "approval",
    }
}

pub fn configured_command_network_policy() -> anyhow::Result<SandboxNetworkPolicy> {
    let raw_mode = env::var(COMMAND_SANDBOX_ENV).unwrap_or_else(|_| "path".to_string());
    let normalized_mode = raw_mode.trim().to_ascii_lowercase();
    if matches!(
        normalized_mode.as_str(),
        "strict-bwrap" | "strict_bwrap" | "strict-bubblewrap" | "strict_bubblewrap"
    ) {
        return Ok(SandboxNetworkPolicy::Disable);
    }
    configured_network_policy(SandboxNetworkPolicy::Inherit)
}

pub fn command_requires_network_approval(parts: &[&str]) -> anyhow::Result<bool> {
    Ok(matches!(
        configured_command_network_policy()?,
        SandboxNetworkPolicy::RequireApproval
    ) && is_likely_network_command(parts))
}

fn is_likely_network_command(parts: &[&str]) -> bool {
    let Some(program) = parts.first().map(|part| part.trim()) else {
        return false;
    };
    let subcommand = parts
        .iter()
        .skip(1)
        .find(|part| !part.trim().starts_with('-'))
        .map(|part| part.trim());
    match program {
        "curl" | "wget" | "ssh" | "scp" | "sftp" => true,
        "gh" => matches!(
            subcommand,
            Some(
                "api"
                    | "auth"
                    | "browse"
                    | "codespace"
                    | "cs"
                    | "gist"
                    | "issue"
                    | "pr"
                    | "release"
                    | "repo"
            )
        ),
        "git" => matches!(
            subcommand,
            Some("clone" | "fetch" | "pull" | "push" | "submodule" | "ls-remote")
        ),
        "cargo" => matches!(subcommand, Some("fetch" | "install" | "publish" | "search")),
        "npm" => matches!(
            subcommand,
            Some("install" | "i" | "ci" | "publish" | "update" | "audit" | "view" | "info")
        ),
        "pip" | "pip3" => matches!(
            subcommand,
            Some("install" | "download" | "wheel" | "search")
        ),
        "python" | "python3" => parts.windows(3).any(|window| {
            window[0].trim() == "-m"
                && window[1].trim() == "pip"
                && matches!(
                    window[2].trim(),
                    "install" | "download" | "wheel" | "search"
                )
        }),
        _ => false,
    }
}

fn sensitive_environment_names() -> Vec<String> {
    let mut keys = env::vars()
        .map(|(key, _)| key)
        .filter(|key| is_sensitive_environment_name(key))
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    keys
}

fn is_sensitive_environment_name(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    upper.contains("API_KEY")
        || upper.contains("ACCESS_TOKEN")
        || upper.contains("AUTH_TOKEN")
        || upper.contains("BEARER_TOKEN")
        || upper.contains("ID_TOKEN")
        || upper.contains("REFRESH_TOKEN")
        || upper.contains("SECRET")
        || upper.contains("PASSWORD")
        || upper.contains("PRIVATE_KEY")
        || upper.starts_with("HBSE_")
        || upper == "TOKEN"
}

fn push_existing_readonly_mounts(args: &mut Vec<String>, paths: &[PathBuf]) {
    for path in paths {
        if path.exists() {
            push_mount(args, "--ro-bind", path, path);
        }
    }
}

fn push_mount(args: &mut Vec<String>, flag: &str, source: &Path, destination: &Path) {
    args.push(flag.to_string());
    args.push(source.display().to_string());
    args.push(destination.display().to_string());
}

fn push_hidden_mounts(args: &mut Vec<String>, paths: &[PathBuf]) -> anyhow::Result<()> {
    for path in paths {
        validate_mount_path(path)?;
        if path.is_dir() {
            args.push("--tmpfs".to_string());
            args.push(path.display().to_string());
        } else {
            push_mount(args, "--bind", Path::new("/dev/null"), path);
        }
    }
    Ok(())
}

fn validate_mount_path(path: &Path) -> anyhow::Result<()> {
    if !path.is_absolute() {
        anyhow::bail!("Sandbox mount path must be absolute: {}", path.display());
    }
    if !path.exists() {
        anyhow::bail!("Sandbox mount path does not exist: {}", path.display());
    }
    Ok(())
}

fn resolve_bwrap_path(configured: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(path) = configured {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        anyhow::bail!(
            "Configured Bubblewrap executable does not exist: {}",
            path.display()
        );
    }
    let Some(path) = env::var_os("PATH") else {
        anyhow::bail!(
            "Bubblewrap command not found in PATH; install bwrap or use {COMMAND_SANDBOX_ENV}=path"
        );
    };
    for dir in env::split_paths(&path) {
        let candidate = dir.join("bwrap");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "Bubblewrap command not found in PATH; install bwrap or use {COMMAND_SANDBOX_ENV}=path"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn path_only_returns_original_command() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let config = CommandSandboxConfig::path_only(dir.path());
        let command = build_sandboxed_command(&["cargo", "test"], &config)?;
        assert_eq!(command.program, "cargo");
        assert_eq!(command.args, vec!["test"]);
        assert_eq!(command.current_dir, dir.path());
        assert!(!command.sandboxed);
        assert_eq!(command.sandbox_kind, "path-only");
        Ok(())
    }

    #[test]
    fn bubblewrap_mounts_workspace_and_appends_command() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let config = CommandSandboxConfig {
            bwrap_path: Some(PathBuf::from("/usr/bin/bwrap")),
            readonly_paths: Vec::new(),
            ..CommandSandboxConfig::bubblewrap(dir.path())
        };
        let command = build_sandboxed_command(&["cargo", "test"], &config)?;
        assert_eq!(command.program, "/usr/bin/bwrap");
        assert!(command.sandboxed);
        assert_eq!(command.sandbox_kind, "bubblewrap");
        assert_arg_sequence(
            &command.args,
            &[
                "--bind",
                &dir.path().display().to_string(),
                "/workspace",
                "--chdir",
                "/workspace",
                "--",
                "cargo",
                "test",
            ],
        );
        Ok(())
    }

    #[test]
    fn strict_bubblewrap_disables_network_and_uses_private_tmp_home() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let config = CommandSandboxConfig {
            readonly_paths: Vec::new(),
            ..CommandSandboxConfig::strict_bubblewrap(dir.path())
        };
        let command = build_sandboxed_command(&["printf", "ok"], &config)?;
        assert_arg_sequence(
            &command.args,
            &["--clearenv", "--setenv", "PATH", default_sandbox_path()],
        );
        assert!(command.args.iter().any(|arg| arg == "--unshare-net"));
        assert_arg_sequence(&command.args, &["--tmpfs", "/tmp"]);
        assert_arg_sequence(&command.args, &["--tmpfs", "/home"]);
        assert_arg_sequence(&command.args, &["--setenv", "HOME", "/home/vegvisir"]);
        Ok(())
    }

    #[test]
    fn bubblewrap_generates_hidden_path_mounts() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let hidden_dir = dir.path().join("hidden-dir");
        let hidden_file = dir.path().join("hidden-file");
        std::fs::create_dir(&hidden_dir)?;
        std::fs::write(&hidden_file, "secret")?;
        let config = CommandSandboxConfig {
            bwrap_path: Some(PathBuf::from("/usr/bin/bwrap")),
            readonly_paths: Vec::new(),
            hidden_paths: vec![hidden_dir.clone(), hidden_file.clone()],
            ..CommandSandboxConfig::bubblewrap(dir.path())
        };
        let command = build_sandboxed_command(&["printf", "ok"], &config)?;
        assert_arg_sequence(
            &command.args,
            &["--tmpfs", &hidden_dir.display().to_string()],
        );
        assert_arg_sequence(
            &command.args,
            &["--bind", "/dev/null", &hidden_file.display().to_string()],
        );
        Ok(())
    }

    #[test]
    fn bubblewrap_network_disable_env_adds_unshare_net() -> anyhow::Result<()> {
        let _guard = TEST_ENV_LOCK
            .get_or_init(std::sync::Mutex::default)
            .lock()
            .unwrap();
        let _sandbox_env = TestEnvGuard::set(COMMAND_SANDBOX_ENV, "bwrap");
        let _network_env = TestEnvGuard::set(COMMAND_SANDBOX_NETWORK_ENV, "disable");
        let dir = tempdir()?;
        let config = CommandSandboxConfig {
            bwrap_path: Some(PathBuf::from("/usr/bin/bwrap")),
            readonly_paths: Vec::new(),
            ..CommandSandboxConfig::from_env(dir.path(), false)?
        };
        let command = build_sandboxed_command(&["printf", "ok"], &config)?;
        assert_eq!(command.network_policy, "disabled");
        assert!(command.args.iter().any(|arg| arg == "--unshare-net"));
        Ok(())
    }

    #[test]
    fn strict_bubblewrap_ignores_network_inherit_override() -> anyhow::Result<()> {
        let _guard = TEST_ENV_LOCK
            .get_or_init(std::sync::Mutex::default)
            .lock()
            .unwrap();
        let _sandbox_env = TestEnvGuard::set(COMMAND_SANDBOX_ENV, "strict-bwrap");
        let _network_env = TestEnvGuard::set(COMMAND_SANDBOX_NETWORK_ENV, "inherit");
        let dir = tempdir()?;
        let config = CommandSandboxConfig {
            bwrap_path: Some(PathBuf::from("/usr/bin/bwrap")),
            readonly_paths: Vec::new(),
            ..CommandSandboxConfig::from_env(dir.path(), false)?
        };
        let command = build_sandboxed_command(&["printf", "ok"], &config)?;
        assert_eq!(command.network_policy, "disabled");
        assert!(command.args.iter().any(|arg| arg == "--unshare-net"));
        Ok(())
    }

    #[test]
    fn network_approval_policy_identifies_likely_network_commands() -> anyhow::Result<()> {
        let _guard = TEST_ENV_LOCK
            .get_or_init(std::sync::Mutex::default)
            .lock()
            .unwrap();
        let _network_env = TestEnvGuard::set(COMMAND_SANDBOX_NETWORK_ENV, "require-approval");
        assert!(command_requires_network_approval(&["git", "fetch"])?);
        assert!(command_requires_network_approval(&["npm", "install"])?);
        assert!(command_requires_network_approval(&[
            "python", "-m", "pip", "install", "requests"
        ])?);
        assert!(!command_requires_network_approval(&["git", "status"])?);
        assert!(!command_requires_network_approval(&["cargo", "test"])?);
        Ok(())
    }

    #[test]
    fn strict_mode_does_not_request_network_approval() -> anyhow::Result<()> {
        let _guard = TEST_ENV_LOCK
            .get_or_init(std::sync::Mutex::default)
            .lock()
            .unwrap();
        let _sandbox_env = TestEnvGuard::set(COMMAND_SANDBOX_ENV, "strict-bwrap");
        let _network_env = TestEnvGuard::set(COMMAND_SANDBOX_NETWORK_ENV, "require-approval");
        assert!(!command_requires_network_approval(&["git", "fetch"])?);
        Ok(())
    }

    #[test]
    fn mount_path_env_configures_bubblewrap_mounts() -> anyhow::Result<()> {
        let _guard = TEST_ENV_LOCK
            .get_or_init(std::sync::Mutex::default)
            .lock()
            .unwrap();
        let dir = tempdir()?;
        let writable = dir.path().join("writable");
        let readonly = dir.path().join("readonly");
        let hidden = dir.path().join("hidden");
        std::fs::create_dir_all(&writable)?;
        std::fs::create_dir_all(&readonly)?;
        std::fs::create_dir_all(&hidden)?;
        let _sandbox_env = TestEnvGuard::set(COMMAND_SANDBOX_ENV, "bwrap");
        let _writable_env = TestEnvGuard::set_os(COMMAND_SANDBOX_WRITABLE_PATHS_ENV, &writable);
        let _readonly_env = TestEnvGuard::set_os(COMMAND_SANDBOX_READONLY_PATHS_ENV, &readonly);
        let _hidden_env = TestEnvGuard::set_os(COMMAND_SANDBOX_HIDDEN_PATHS_ENV, &hidden);

        let config = CommandSandboxConfig {
            bwrap_path: Some(PathBuf::from("/usr/bin/bwrap")),
            ..CommandSandboxConfig::from_env(dir.path(), false)?
        };
        let command = build_sandboxed_command(&["printf", "ok"], &config)?;
        assert_arg_sequence(
            &command.args,
            &[
                "--bind",
                &writable.display().to_string(),
                &writable.display().to_string(),
            ],
        );
        assert_arg_sequence(
            &command.args,
            &[
                "--ro-bind",
                &readonly.display().to_string(),
                &readonly.display().to_string(),
            ],
        );
        assert_arg_sequence(&command.args, &["--tmpfs", &hidden.display().to_string()]);
        Ok(())
    }

    #[test]
    fn mount_path_env_rejects_relative_paths() -> anyhow::Result<()> {
        let _guard = TEST_ENV_LOCK
            .get_or_init(std::sync::Mutex::default)
            .lock()
            .unwrap();
        let _sandbox_env = TestEnvGuard::set(COMMAND_SANDBOX_ENV, "bwrap");
        let _hidden_env = TestEnvGuard::set(COMMAND_SANDBOX_HIDDEN_PATHS_ENV, "relative");
        let dir = tempdir()?;
        let err = CommandSandboxConfig::from_env(dir.path(), false).unwrap_err();
        assert!(
            err.to_string()
                .contains("VEGVISIR_COMMAND_HIDDEN_PATHS entries must be absolute paths")
        );
        Ok(())
    }

    #[test]
    fn dangerous_bypass_disables_bubblewrap() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let mut config = CommandSandboxConfig::bubblewrap(dir.path());
        config.allow_dangerous_bypass = true;
        let command = build_sandboxed_command(&["sh", "-c", "true"], &config)?;
        assert_eq!(command.program, "sh");
        assert_eq!(command.args, vec!["-c", "true"]);
        assert!(!command.sandboxed);
        assert_eq!(command.sandbox_kind, "disabled-by-dangerous-bypass");
        Ok(())
    }

    #[test]
    fn missing_configured_bwrap_path_gives_clear_error() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let missing = dir.path().join("missing-bwrap");
        let config = CommandSandboxConfig {
            bwrap_path: Some(missing),
            readonly_paths: Vec::new(),
            ..CommandSandboxConfig::bubblewrap(dir.path())
        };
        let err = build_sandboxed_command(&["printf", "ok"], &config).unwrap_err();
        assert!(
            err.to_string()
                .contains("Configured Bubblewrap executable does not exist")
        );
        Ok(())
    }

    #[test]
    fn bubblewrap_unsets_secret_like_environment_names() -> anyhow::Result<()> {
        let _guard = TEST_ENV_LOCK
            .get_or_init(std::sync::Mutex::default)
            .lock()
            .unwrap();
        let _visible_env = TestEnvGuard::set("VEGVISIR_TEST_VISIBLE_SETTING", "keep");
        let _api_key_env = TestEnvGuard::set("VEGVISIR_TEST_API_KEY", "secret");
        let _hbse_env = TestEnvGuard::set("HBSE_BROKER_SOCKET", "/tmp/hbse.sock");
        let dir = tempdir()?;
        let config = CommandSandboxConfig {
            bwrap_path: Some(PathBuf::from("/usr/bin/bwrap")),
            readonly_paths: Vec::new(),
            ..CommandSandboxConfig::bubblewrap(dir.path())
        };
        let command = build_sandboxed_command(&["env"], &config)?;
        assert_arg_sequence(&command.args, &["--unsetenv", "HBSE_BROKER_SOCKET"]);
        assert_arg_sequence(&command.args, &["--unsetenv", "VEGVISIR_TEST_API_KEY"]);
        assert!(
            !command
                .args
                .windows(2)
                .any(|window| window == ["--unsetenv", "VEGVISIR_TEST_VISIBLE_SETTING"])
        );
        Ok(())
    }

    fn assert_arg_sequence(args: &[String], expected: &[&str]) {
        assert!(
            args.windows(expected.len())
                .any(|window| window == expected),
            "missing sequence {expected:?} in {args:?}"
        );
    }

    static TEST_ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

    struct TestEnvGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl TestEnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let old = env::var_os(key);
            unsafe {
                env::set_var(key, value);
            }
            Self { key, old }
        }

        fn set_os(key: &'static str, value: &Path) -> Self {
            let old = env::var_os(key);
            unsafe {
                env::set_var(key, value);
            }
            Self { key, old }
        }
    }

    impl Drop for TestEnvGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(old) = &self.old {
                    env::set_var(self.key, old);
                } else {
                    env::remove_var(self.key);
                }
            }
        }
    }
}
