use std::{
    io::{self, IsTerminal, Read, Write},
    os::fd::{FromRawFd, RawFd},
    os::unix::process::CommandExt,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

const SUDO_PATH: &str = "/usr/bin/sudo";
const SUPERVISOR_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const SUPERVISOR_READ_CHUNK_BYTES: usize = 8 * 1024;
const SUPERVISOR_AUTH_TIMEOUT: Duration = Duration::from_secs(5);

pub const SUDO_SUPERVISOR_NOT_AUTHENTICATED: &str =
    "sudo supervisor is not authenticated; run /sudo auth";
pub const SUDO_SUPERVISOR_TIMEOUT: &str = "sudo supervisor command timed out; run /sudo auth again";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SudoStatus {
    pub authenticated: bool,
    pub sudo_available: bool,
    pub message: String,
}

#[derive(Debug)]
pub(crate) struct SupervisorOutput {
    pub(crate) content: String,
    pub(crate) status: i32,
    pub(crate) total_bytes: usize,
    pub(crate) truncated: bool,
}

struct SudoShell {
    child: Child,
    stdin: ChildStdin,
    output_rx: mpsc::Receiver<io::Result<Vec<u8>>>,
    buffered_output: Vec<u8>,
}

impl SudoShell {
    fn spawn(with_password: bool, ready_marker: Option<&str>) -> anyhow::Result<Self> {
        let (output_read, output_write) = output_pipe()?;
        let stdout_write = output_write.try_clone()?;
        let mut command = Command::new(SUDO_PATH);
        if with_password {
            let ready_marker = ready_marker
                .ok_or_else(|| anyhow::anyhow!("sudo supervisor ready marker was missing"))?;
            command.args(["-S", "-p", "", "--", "/bin/sh", "-c"]);
            command.arg(format!(
                "printf '\\n%s:0\\n' {}; exec /bin/sh -s\n",
                shell_quote(ready_marker)
            ));
        } else {
            command.args(["-n", "--", "/bin/sh", "-s"]);
        }
        command
            .env_clear()
            .env("PATH", SUPERVISOR_PATH)
            .env("HOME", "/root")
            .stdin(Stdio::piped())
            .stdout(Stdio::from(stdout_write))
            .stderr(Stdio::from(output_write));
        if let Some(term) = std::env::var_os("TERM") {
            command.env("TERM", term);
        }
        // Do not leave an authenticated root shell behind if Vegvisir exits.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command
            .spawn()
            .map_err(|error| anyhow::anyhow!("failed to start private sudo supervisor: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("private sudo supervisor stdin was unavailable"))?;
        let (output_tx, output_rx) = mpsc::channel();
        thread::spawn(move || read_supervisor_output(output_read, output_tx));
        Ok(Self {
            child,
            stdin,
            output_rx,
            buffered_output: Vec::new(),
        })
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn send(&mut self, script: &str) -> anyhow::Result<()> {
        self.stdin.write_all(script.as_bytes())?;
        self.stdin.flush()?;
        Ok(())
    }

    fn read_result(
        &mut self,
        marker: &str,
        timeout: Duration,
        max_output_bytes: usize,
    ) -> anyhow::Result<SupervisorOutput> {
        let marker_prefix = format!("\n{marker}:");
        let marker_prefix_bytes = marker_prefix.as_bytes();
        let started = Instant::now();
        let mut total_bytes = 0usize;
        let mut truncated = false;
        let retention_limit = max_output_bytes
            .saturating_add(marker_prefix_bytes.len())
            .saturating_add(64);

        loop {
            if let Some(marker_start) = find_subslice(&self.buffered_output, marker_prefix_bytes)
                && let Some(status_end_relative) = self.buffered_output
                    [marker_start + marker_prefix_bytes.len()..]
                    .iter()
                    .position(|byte| *byte == b'\n')
            {
                let status_end = marker_start + marker_prefix_bytes.len() + status_end_relative;
                let status_bytes =
                    &self.buffered_output[marker_start + marker_prefix_bytes.len()..status_end];
                let status = std::str::from_utf8(status_bytes)
                    .ok()
                    .and_then(|value| value.parse::<i32>().ok());
                if let Some(status) = status {
                    let mut output = self.buffered_output[..marker_start].to_vec();
                    self.buffered_output.drain(..status_end + 1);
                    if output.len() > max_output_bytes {
                        output.truncate(max_output_bytes);
                        truncated = true;
                    }
                    let content = String::from_utf8_lossy(&output).into_owned();
                    total_bytes = total_bytes.max(output.len());
                    output.fill(0);
                    return Ok(SupervisorOutput {
                        content,
                        status,
                        total_bytes,
                        truncated,
                    });
                }
            }

            let elapsed = started.elapsed();
            if elapsed >= timeout {
                return Err(anyhow::anyhow!("{SUDO_SUPERVISOR_TIMEOUT}"));
            }
            let remaining = timeout.saturating_sub(elapsed);
            let chunk = self
                .output_rx
                .recv_timeout(remaining)
                .map_err(|_| anyhow::anyhow!("{SUDO_SUPERVISOR_TIMEOUT}"))??;
            total_bytes = total_bytes.saturating_add(chunk.len());
            self.buffered_output.extend_from_slice(&chunk);
            if self.buffered_output.len() > retention_limit {
                let keep = marker_prefix_bytes.len().saturating_add(64);
                let remove = self.buffered_output.len().saturating_sub(keep);
                self.buffered_output.drain(..remove);
                truncated = true;
            }
        }
    }

    fn stop(&mut self) {
        #[cfg(unix)]
        unsafe {
            // The supervisor owns the process group so shutdown also terminates
            // any command children that inherited the root supervisor context.
            let _ = libc::kill(-(self.child.id() as libc::pid_t), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for SudoShell {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct SudoSupervisor {
    shell: Option<SudoShell>,
}

impl Default for SudoSupervisor {
    fn default() -> Self {
        Self { shell: None }
    }
}

impl SudoSupervisor {
    pub fn is_ready(&mut self) -> bool {
        self.shell.as_mut().is_some_and(SudoShell::alive)
    }

    pub fn authenticate_with_password(&mut self, password: &mut Vec<char>) -> anyhow::Result<()> {
        self.shutdown();
        if password.is_empty() {
            anyhow::bail!("sudo password was empty");
        }

        let password_byte_len = password.iter().map(|ch| ch.len_utf8()).sum::<usize>();
        let mut password_bytes = Vec::with_capacity(password_byte_len + 1);
        for ch in password.iter().copied() {
            let mut buf = [0u8; 4];
            let encoded = ch.encode_utf8(&mut buf);
            password_bytes.extend_from_slice(encoded.as_bytes());
            buf.fill(0);
        }
        password_bytes.push(b'\n');

        let result = self.authenticate_shell(&password_bytes);
        password_bytes.fill(0);
        result
    }

    fn authenticate_shell(&mut self, password_bytes: &[u8]) -> anyhow::Result<()> {
        let ready_marker = supervisor_marker();
        let mut shell = SudoShell::spawn(true, Some(&ready_marker))?;
        shell.stdin.write_all(password_bytes)?;
        shell.stdin.flush()?;
        match shell.read_result(&ready_marker, SUPERVISOR_AUTH_TIMEOUT, 1024) {
            Ok(output) if output.status == 0 => {}
            Ok(_) => {
                shell.stop();
                anyhow::bail!("sudo authentication failed")
            }
            Err(error) => {
                shell.stop();
                anyhow::bail!("sudo authentication failed: {error}")
            }
        }
        let marker = supervisor_marker();
        shell.send(&supervisor_marker_script(&marker, "/bin/true"))?;
        let result = shell.read_result(&marker, SUPERVISOR_AUTH_TIMEOUT, 1024);
        match result {
            Ok(output) if output.status == 0 => {
                self.shell = Some(shell);
                Ok(())
            }
            Ok(_) => {
                shell.stop();
                anyhow::bail!("sudo authentication failed")
            }
            Err(error) => {
                shell.stop();
                anyhow::bail!("sudo authentication failed: {error}")
            }
        }
    }

    pub fn authenticate_with_existing_sudo(&mut self) -> anyhow::Result<()> {
        self.shutdown();
        let mut shell = SudoShell::spawn(false, None)?;
        let marker = supervisor_marker();
        shell.send(&supervisor_marker_script(&marker, "/bin/true"))?;
        match shell.read_result(&marker, SUPERVISOR_AUTH_TIMEOUT, 1024) {
            Ok(output) if output.status == 0 => {
                self.shell = Some(shell);
                Ok(())
            }
            Ok(_) => {
                shell.stop();
                anyhow::bail!("sudo authentication did not authorize the private supervisor")
            }
            Err(error) => {
                shell.stop();
                Err(error)
            }
        }
    }

    pub(crate) fn run(
        &mut self,
        parts: &[String],
        current_dir: &std::path::Path,
        timeout: Duration,
        output_limit: usize,
    ) -> anyhow::Result<SupervisorOutput> {
        let Some(shell) = self.shell.as_mut() else {
            anyhow::bail!("{SUDO_SUPERVISOR_NOT_AUTHENTICATED}");
        };
        if !shell.alive() {
            self.shell = None;
            anyhow::bail!("{SUDO_SUPERVISOR_NOT_AUTHENTICATED}");
        }
        let marker = supervisor_marker();
        let script = supervisor_command_script(&marker, current_dir, parts)?;
        shell.send(&script)?;
        match shell.read_result(&marker, timeout, output_limit) {
            Ok(output) => Ok(output),
            Err(error) => {
                self.shutdown();
                Err(error)
            }
        }
    }

    pub fn shutdown(&mut self) {
        if let Some(mut shell) = self.shell.take() {
            shell.stop();
        }
    }
}

impl Drop for SudoSupervisor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub fn new_sudo_supervisor() -> Arc<Mutex<SudoSupervisor>> {
    Arc::new(Mutex::new(SudoSupervisor::default()))
}

pub fn sudo_status() -> SudoStatus {
    match Command::new(SUDO_PATH)
        .args(["-n", "-v"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => SudoStatus {
            authenticated: true,
            sudo_available: true,
            message: "sudo timestamp is currently valid".to_string(),
        },
        Ok(_) => SudoStatus {
            authenticated: false,
            sudo_available: true,
            message: "sudo is not currently authenticated; run /sudo auth to start the private supervisor".to_string(),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => SudoStatus {
            authenticated: false,
            sudo_available: false,
            message: "sudo executable was not found on PATH".to_string(),
        },
        Err(error) => SudoStatus {
            authenticated: false,
            sudo_available: true,
            message: format!("failed to check sudo status: {error}"),
        },
    }
}

pub fn sudo_status_for_supervisor(supervisor: &Arc<Mutex<SudoSupervisor>>) -> SudoStatus {
    let Ok(mut supervisor) = supervisor.lock() else {
        return SudoStatus {
            authenticated: false,
            sudo_available: true,
            message: "sudo supervisor state is unavailable".to_string(),
        };
    };
    if supervisor.is_ready() {
        SudoStatus {
            authenticated: true,
            sudo_available: true,
            message: "private sudo supervisor is active; model tools receive only structured command results".to_string(),
        }
    } else {
        let mut status = sudo_status();
        if status.sudo_available {
            status.authenticated = false;
            status.message =
                "sudo is available, but the private supervisor is inactive; run /sudo auth"
                    .to_string();
        }
        status
    }
}

pub fn sudo_invalidate(supervisor: &Arc<Mutex<SudoSupervisor>>) -> anyhow::Result<()> {
    let mut supervisor_guard = supervisor
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    supervisor_guard.shutdown();
    drop(supervisor_guard);
    supervisor.clear_poison();
    let status = Command::new(SUDO_PATH)
        .arg("-k")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("sudo -k failed with exit code {:?}", status.code())
    }
}

pub fn sudo_refresh_with_tui_password(
    supervisor: &Arc<Mutex<SudoSupervisor>>,
    password: &mut Vec<char>,
) -> anyhow::Result<()> {
    let mut supervisor = supervisor
        .lock()
        .map_err(|_| anyhow::anyhow!("sudo supervisor state is unavailable"))?;
    supervisor.authenticate_with_password(password)
}

pub fn sudo_refresh_interactive_from_tui(
    supervisor: &Arc<Mutex<SudoSupervisor>>,
) -> anyhow::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        anyhow::bail!("/sudo auth requires an interactive terminal");
    }

    let mut stdout = io::stdout();
    disable_raw_mode()?;
    execute!(
        stdout,
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen
    )?;
    stdout.flush()?;

    let auth_result = run_sudo_validate_on_controlling_terminal();

    let mut reenter_result = execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )
    .map_err(anyhow::Error::from)
    .and_then(|_| enable_raw_mode().map_err(anyhow::Error::from))
    .and_then(|_| stdout.flush().map_err(anyhow::Error::from));

    match (auth_result, reenter_result.as_mut()) {
        (Ok(()), Ok(())) => {
            let mut supervisor = supervisor
                .lock()
                .map_err(|_| anyhow::anyhow!("sudo supervisor state is unavailable"))?;
            supervisor.authenticate_with_existing_sudo()
        }
        (Err(auth_error), Ok(())) => Err(auth_error),
        (Ok(()), Err(reenter_error)) => Err(anyhow::anyhow!(
            "sudo authentication succeeded, but Vegvisir failed to restore the TUI: {reenter_error}"
        )),
        (Err(auth_error), Err(reenter_error)) => Err(anyhow::anyhow!(
            "sudo authentication failed ({auth_error}); additionally failed to restore the TUI: {reenter_error}"
        )),
    }
}

fn run_sudo_validate_on_controlling_terminal() -> anyhow::Result<()> {
    eprintln!(
        "Vegvisir is handing password entry directly to sudo. The password is not read by Vegvisir and is not written to chat/session/trace history."
    );
    let status = Command::new(SUDO_PATH)
        .arg("-v")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!(
            "sudo authentication failed with exit code {:?}",
            status.code()
        )
    }
}

fn supervisor_marker() -> String {
    format!("__VEGVISIR_SUDO_{}__", uuid::Uuid::new_v4().simple())
}

fn supervisor_marker_script(marker: &str, command: &str) -> String {
    format!(
        "( {} ); status=$?; printf '\\n%s:%s\\n' {} \"$status\"\n",
        shell_quote(command),
        shell_quote(marker)
    )
}

fn supervisor_command_script(
    marker: &str,
    current_dir: &std::path::Path,
    parts: &[String],
) -> anyhow::Result<String> {
    if parts.is_empty() {
        anyhow::bail!("empty privileged command");
    }
    let command = parts
        .iter()
        .map(|part| shell_quote(part))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(format!(
        "cd {} ; status=$?; if [ \"$status\" -eq 0 ]; then ( {} ); status=$?; fi; printf '\\n%s:%s\\n' {} \"$status\"\n",
        shell_quote(&current_dir.display().to_string()),
        command,
        shell_quote(marker),
    ))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn output_pipe() -> io::Result<(std::fs::File, std::fs::File)> {
    let mut fds = [0 as RawFd; 2];
    let result = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: pipe initialized both file descriptors on success and ownership is
    // transferred exactly once to these File values.
    Ok(unsafe {
        (
            std::fs::File::from_raw_fd(fds[0]),
            std::fs::File::from_raw_fd(fds[1]),
        )
    })
}

fn read_supervisor_output(mut output: std::fs::File, sender: mpsc::Sender<io::Result<Vec<u8>>>) {
    loop {
        let mut chunk = vec![0u8; SUPERVISOR_READ_CHUNK_BYTES];
        match output.read(&mut chunk) {
            Ok(0) => {
                let _ = sender.send(Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "private sudo supervisor exited",
                )));
                return;
            }
            Ok(size) => {
                chunk.truncate(size);
                if sender.send(Ok(chunk)).is_err() {
                    return;
                }
            }
            Err(error) => {
                let _ = sender.send(Err(error));
                return;
            }
        }
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_handles_command_injection_characters() {
        let quoted = shell_quote("$(touch /tmp/nope); 'quoted'\n");
        assert_eq!(quoted, "'$(touch /tmp/nope); '\\''quoted'\\''\n'");
    }

    #[test]
    fn supervisor_marker_script_reports_status_without_exposing_input() {
        let script = supervisor_command_script(
            "__marker__",
            std::path::Path::new("/workspace"),
            &["id".to_string(), "-u".to_string()],
        )
        .unwrap();
        assert!(script.contains("__marker__"));
        assert!(script.contains("'id' '-u'"));
        assert!(!script.contains("password"));
    }

    #[test]
    fn empty_tui_password_is_rejected_before_starting_supervisor() {
        let supervisor = new_sudo_supervisor();
        let mut password = Vec::new();
        let err = sudo_refresh_with_tui_password(&supervisor, &mut password)
            .expect_err("empty sudo password should be rejected");
        assert!(err.to_string().contains("sudo password was empty"));
    }
}
