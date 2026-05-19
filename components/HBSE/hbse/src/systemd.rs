use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SystemdError {
    #[error("scope must be user or system")]
    InvalidScope,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("systemctl failed: {0:?}")]
    SystemctlFailed(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemdInstallResult {
    pub scope: String,
    pub unit_dir: String,
    pub service_path: String,
    pub socket_path: String,
    pub service_name: String,
    pub socket_name: String,
    pub enabled: bool,
    pub started: bool,
    pub commands: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BrokerServiceInstallOptions {
    pub scope: String,
    pub unit_dir: Option<PathBuf>,
    pub broker_executable: Option<String>,
    pub vault_path: String,
    pub socket_path: Option<String>,
    pub idle_timeout_seconds: f64,
    pub service_user: Option<String>,
    pub enable: bool,
    pub start: bool,
    pub dry_run: bool,
}

pub fn default_unit_dir(scope: &str) -> Result<PathBuf, SystemdError> {
    match scope {
        "user" => Ok(env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".config"))
            .join("systemd/user")),
        "system" => Ok(PathBuf::from("/etc/systemd/system")),
        _ => Err(SystemdError::InvalidScope),
    }
}

pub fn default_socket_path(scope: &str) -> Result<String, SystemdError> {
    match scope {
        "user" => Ok("%t/hbse/broker.sock".to_string()),
        "system" => Ok("/run/hbse/broker.sock".to_string()),
        _ => Err(SystemdError::InvalidScope),
    }
}

pub fn default_broker_executable() -> String {
    if let Ok(current) = env::current_exe() {
        if let Some(parent) = current.parent() {
            let sibling = parent.join("hbse-broker");
            if sibling.exists() {
                return sibling.to_string_lossy().to_string();
            }
        }
    }
    "hbse-broker".to_string()
}

pub fn render_broker_service(
    scope: &str,
    broker_executable: &str,
    vault_path: &str,
    socket_path: &str,
    idle_timeout_seconds: f64,
    service_user: Option<&str>,
) -> Result<String, SystemdError> {
    if scope != "user" && scope != "system" {
        return Err(SystemdError::InvalidScope);
    }
    let mut lines = vec![
        "[Unit]".to_string(),
        "Description=HBSE broker".to_string(),
        "After=network-online.target".to_string(),
        String::new(),
        "[Service]".to_string(),
        "Type=simple".to_string(),
        format!("Environment=HBSE_VAULT_PATH={}", escape_env(vault_path)),
        format!(
            "ExecStart={} --vault {} --socket {} --idle-timeout-seconds {}",
            broker_executable,
            vault_path,
            socket_path,
            format_seconds(idle_timeout_seconds)
        ),
        "Restart=on-failure".to_string(),
        "RestartSec=2".to_string(),
        "NoNewPrivileges=true".to_string(),
        "PrivateTmp=true".to_string(),
        "ProtectSystem=strict".to_string(),
    ];
    if scope == "user" {
        let mut write_paths = vec![
            parent_path(vault_path).unwrap_or_else(|| ".".to_string()),
            parent_path(socket_path).unwrap_or_else(|| "%t/hbse".to_string()),
        ];
        if socket_path.starts_with("%t/") {
            write_paths.push("%t/hbse".to_string());
        }
        write_paths.sort();
        write_paths.dedup();
        lines.extend([
            "ProtectHome=read-only".to_string(),
            "RuntimeDirectory=hbse".to_string(),
            "RuntimeDirectoryMode=0700".to_string(),
            format!("ReadWritePaths={}", write_paths.join(" ")),
        ]);
    } else {
        if let Some(service_user) = service_user {
            lines.push(format!("User={service_user}"));
        }
        lines.extend([
            "ProtectHome=true".to_string(),
            "StateDirectory=hbse".to_string(),
            "RuntimeDirectory=hbse".to_string(),
            "RuntimeDirectoryMode=0700".to_string(),
        ]);
    }
    lines.extend([
        "RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6".to_string(),
        String::new(),
        "[Install]".to_string(),
        if scope == "user" {
            "WantedBy=default.target".to_string()
        } else {
            "WantedBy=multi-user.target".to_string()
        },
        String::new(),
    ]);
    Ok(lines.join("\n"))
}

fn format_seconds(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

pub fn render_broker_socket(scope: &str, socket_path: &str) -> Result<String, SystemdError> {
    if scope != "user" && scope != "system" {
        return Err(SystemdError::InvalidScope);
    }
    Ok([
        "[Unit]",
        "Description=HBSE broker socket",
        "",
        "[Socket]",
        &format!("ListenStream={socket_path}"),
        "SocketMode=0600",
        "DirectoryMode=0700",
        "",
        "[Install]",
        "WantedBy=sockets.target",
        "",
    ]
    .join("\n"))
}

pub fn install_broker_service(
    options: BrokerServiceInstallOptions,
) -> Result<SystemdInstallResult, SystemdError> {
    let unit_dir = options
        .unit_dir
        .clone()
        .unwrap_or(default_unit_dir(&options.scope)?);
    let service_name = "hbse-broker.service".to_string();
    let socket_name = "hbse-broker.socket".to_string();
    let broker_executable = options
        .broker_executable
        .clone()
        .unwrap_or_else(default_broker_executable);
    let socket_path = options
        .socket_path
        .clone()
        .unwrap_or(default_socket_path(&options.scope)?);
    let service_text = render_broker_service(
        &options.scope,
        &broker_executable,
        &options.vault_path,
        &socket_path,
        options.idle_timeout_seconds,
        options.service_user.as_deref(),
    )?;
    let socket_text = render_broker_socket(&options.scope, &socket_path)?;
    let service_path = unit_dir.join(&service_name);
    let socket_unit_path = unit_dir.join(&socket_name);
    let mut commands = Vec::new();
    if !options.dry_run {
        std::fs::create_dir_all(&unit_dir)?;
        std::fs::write(&service_path, service_text)?;
        std::fs::write(&socket_unit_path, socket_text)?;
        run_systemctl(&options.scope, &["daemon-reload"], &mut commands)?;
        if options.enable {
            run_systemctl(
                &options.scope,
                &["enable", &socket_name, &service_name],
                &mut commands,
            )?;
        }
        if options.start {
            run_systemctl(&options.scope, &["start", &service_name], &mut commands)?;
        }
    }
    Ok(SystemdInstallResult {
        scope: options.scope,
        unit_dir: unit_dir.to_string_lossy().to_string(),
        service_path: service_path.to_string_lossy().to_string(),
        socket_path: socket_unit_path.to_string_lossy().to_string(),
        service_name,
        socket_name,
        enabled: options.enable && !options.dry_run,
        started: options.start && !options.dry_run,
        commands,
    })
}

fn run_systemctl(
    scope: &str,
    args: &[&str],
    commands: &mut Vec<Vec<String>>,
) -> Result<(), SystemdError> {
    let mut command = vec!["systemctl".to_string()];
    if scope == "user" {
        command.push("--user".to_string());
    }
    command.extend(args.iter().map(|value| value.to_string()));
    let status = Command::new(&command[0]).args(&command[1..]).status()?;
    commands.push(command.clone());
    if status.success() {
        Ok(())
    } else {
        Err(SystemdError::SystemctlFailed(command))
    }
}

fn escape_env(value: &str) -> String {
    value.replace('%', "%%")
}

fn parent_path(path: &str) -> Option<String> {
    if path.starts_with("%t/") {
        let mut parts = path.rsplit_once('/')?.0.to_string();
        if parts.is_empty() {
            parts = "%t".to_string();
        }
        return Some(parts);
    }
    Path::new(path)
        .parent()
        .map(|path| path.to_string_lossy().to_string())
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_user_service_uses_hbse_broker_binary() {
        let service = render_broker_service(
            "user",
            "/opt/hbse/bin/hbse-broker",
            "/home/alice/.local/share/hbse/vault.db",
            "%t/hbse/broker.sock",
            900.0,
            None,
        )
        .unwrap();
        assert!(service.contains("ExecStart=/opt/hbse/bin/hbse-broker --vault /home/alice/.local/share/hbse/vault.db --socket %t/hbse/broker.sock --idle-timeout-seconds 900"));
        assert!(service.contains("ReadWritePaths="));
    }

    #[test]
    fn dry_run_reports_unit_paths_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let result = install_broker_service(BrokerServiceInstallOptions {
            scope: "user".to_string(),
            unit_dir: Some(dir.path().join("systemd/user")),
            broker_executable: Some("/opt/hbse/bin/hbse-broker".to_string()),
            vault_path: "/tmp/vault.db".to_string(),
            socket_path: Some("/tmp/hbse.sock".to_string()),
            idle_timeout_seconds: 1.0,
            service_user: None,
            enable: true,
            start: true,
            dry_run: true,
        })
        .unwrap();
        assert!(result.service_path.ends_with("hbse-broker.service"));
        assert!(!Path::new(&result.service_path).exists());
        assert!(!result.enabled);
        assert!(!result.started);
    }
}
