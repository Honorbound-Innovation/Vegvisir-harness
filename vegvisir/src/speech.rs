use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub const DEFAULT_PTT_SECONDS: u64 = 8;
const SPEECH_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushToTalkKey {
    F(u8),
    Ctrl(char),
}

impl PushToTalkKey {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("off") || value.eq_ignore_ascii_case("none") {
            return None;
        }
        let upper = value.to_ascii_uppercase();
        if let Some(number) = upper.strip_prefix('F') {
            let n = number.parse::<u8>().ok()?;
            if (1..=24).contains(&n) {
                return Some(Self::F(n));
            }
        }
        if let Some(ch) = value
            .strip_prefix("Ctrl+")
            .or_else(|| value.strip_prefix("ctrl+"))
            .and_then(|rest| rest.chars().next())
        {
            return Some(Self::Ctrl(ch.to_ascii_lowercase()));
        }
        None
    }

    pub fn to_config_string(&self) -> String {
        match self {
            Self::F(n) => format!("F{n}"),
            Self::Ctrl(ch) => format!("Ctrl+{}", ch.to_ascii_uppercase()),
        }
    }

    pub fn label(&self) -> String {
        self.to_config_string()
    }

    pub fn matches(&self, key: &KeyEvent) -> bool {
        match self {
            Self::F(n) => key.modifiers.is_empty() && key.code == KeyCode::F(*n),
            Self::Ctrl(ch) => {
                key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char(*ch)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct SpeechBackend {
    pub command: &'static str,
    pub label: &'static str,
    kind: SpeechBackendKind,
}

#[derive(Clone, Debug)]
enum SpeechBackendKind {
    OpenAiWhisper,
    WhisperCli,
}

pub fn speech_backends() -> Vec<SpeechBackend> {
    vec![
        SpeechBackend {
            command: "whisper",
            label: "OpenAI Whisper Python CLI, not the crates.io Rust crate",
            kind: SpeechBackendKind::OpenAiWhisper,
        },
        SpeechBackend {
            command: "whisper-cli",
            label: "whisper.cpp CLI",
            kind: SpeechBackendKind::WhisperCli,
        },
        SpeechBackend {
            command: "whisper.cpp",
            label: "whisper.cpp CLI compatibility name",
            kind: SpeechBackendKind::WhisperCli,
        },
    ]
}

#[derive(Clone, Debug)]
struct RecorderBackend {
    command: &'static str,
    label: &'static str,
    kind: RecorderBackendKind,
}

#[derive(Clone, Debug)]
enum RecorderBackendKind {
    FfmpegPulse,
    Arecord,
}

fn recorder_backends() -> Vec<RecorderBackend> {
    vec![
        RecorderBackend {
            command: "ffmpeg",
            label: "ffmpeg PulseAudio default input",
            kind: RecorderBackendKind::FfmpegPulse,
        },
        RecorderBackend {
            command: "arecord",
            label: "ALSA arecord default input",
            kind: RecorderBackendKind::Arecord,
        },
    ]
}

pub fn speech_backend_status() -> String {
    let stt = speech_backends()
        .into_iter()
        .map(|backend| {
            let status = if executable_in_path(backend.command) {
                "available"
            } else {
                "missing"
            };
            format!("- {} ({}): {}", backend.command, backend.label, status)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let recorders = recorder_backends()
        .into_iter()
        .map(|backend| {
            let status = if executable_in_path(backend.command) {
                "available"
            } else {
                "missing"
            };
            format!("- {} ({}): {}", backend.command, backend.label, status)
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("Speech-to-text backends:\n{stt}\n\nPush-to-talk recorders:\n{recorders}")
}

pub fn transcribe_audio_file(path: &Path) -> anyhow::Result<String> {
    for backend in speech_backends() {
        if !executable_in_path(backend.command) {
            continue;
        }
        let result = match backend.kind {
            SpeechBackendKind::OpenAiWhisper => run_openai_whisper(backend.command, path),
            SpeechBackendKind::WhisperCli => run_whisper_cli(backend.command, path),
        };
        match result {
            Ok(text) if !text.trim().is_empty() => return Ok(text),
            Ok(_) => continue,
            Err(_) => continue,
        }
    }
    anyhow::bail!("no usable local Whisper speech-to-text backend found")
}

pub fn record_and_transcribe(seconds: u64) -> anyhow::Result<String> {
    let seconds = seconds.clamp(1, 30);
    let audio_path = std::env::temp_dir().join(format!(
        "vegvisir-ptt-{}-{}.wav",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let record_result = record_audio_clip(&audio_path, seconds);
    if let Err(error) = record_result {
        let _ = std::fs::remove_file(&audio_path);
        return Err(error);
    }
    let transcript = transcribe_audio_file(&audio_path);
    let _ = std::fs::remove_file(&audio_path);
    transcript
}

fn record_audio_clip(path: &Path, seconds: u64) -> anyhow::Result<()> {
    for backend in recorder_backends() {
        if !executable_in_path(backend.command) {
            continue;
        }
        let timeout = Duration::from_secs(seconds.saturating_add(5));
        let result = match backend.kind {
            RecorderBackendKind::FfmpegPulse => run_command_with_timeout(
                Command::new(backend.command)
                    .args(["-hide_banner", "-loglevel", "error", "-y"])
                    .args(["-f", "pulse", "-i", "default"])
                    .args(["-t", &seconds.to_string(), "-ac", "1", "-ar", "16000"])
                    .arg(path),
                timeout,
            ),
            RecorderBackendKind::Arecord => run_command_with_timeout(
                Command::new(backend.command)
                    .args(["-q", "-f", "S16_LE", "-r", "16000", "-c", "1"])
                    .args(["-d", &seconds.to_string()])
                    .arg(path),
                timeout,
            ),
        };
        match result {
            Ok(output) if output.status.success() && path.is_file() => return Ok(()),
            Ok(_) | Err(_) => continue,
        }
    }
    anyhow::bail!(
        "no usable local audio recorder found; install ffmpeg with PulseAudio support or arecord"
    )
}

fn run_openai_whisper(command: &str, path: &Path) -> anyhow::Result<String> {
    let out_dir = std::env::temp_dir().join(format!(
        "vegvisir-whisper-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir_all(&out_dir)?;
    let output = run_command_with_timeout(
        Command::new(command)
            .arg(path)
            .args(["--output_format", "txt", "--output_dir"])
            .arg(&out_dir)
            .args(["--fp16", "False"]),
        SPEECH_TIMEOUT,
    )?;
    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&out_dir);
        anyhow::bail!(
            "{command} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let transcript = find_first_txt_file(&out_dir)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| strip_whisper_noise(&text))
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| strip_whisper_noise(&String::from_utf8_lossy(&output.stdout)));
    let _ = std::fs::remove_dir_all(&out_dir);
    Ok(transcript)
}

fn run_whisper_cli(command: &str, path: &Path) -> anyhow::Result<String> {
    let output = run_command_with_timeout(
        Command::new(command)
            .arg("-f")
            .arg(path)
            .arg("--no-timestamps"),
        SPEECH_TIMEOUT,
    )?;
    if !output.status.success() {
        anyhow::bail!(
            "{command} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(strip_whisper_noise(&stdout))
}

fn find_first_txt_file(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txt"))
}

pub fn run_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> anyhow::Result<Output> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(child.wait_with_output()?);
        }
        if started.elapsed() >= timeout {
            kill_process_tree(&mut child);
            anyhow::bail!(
                "speech command timed out after {} seconds",
                timeout.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub fn strip_whisper_noise(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("Detecting language"))
        .filter(|line| !line.starts_with("Detected language"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn executable_in_path(command: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(command).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_to_talk_key_parses_function_keys_and_ctrl_keys() {
        assert_eq!(PushToTalkKey::parse("F8"), Some(PushToTalkKey::F(8)));
        assert_eq!(
            PushToTalkKey::parse("ctrl+v"),
            Some(PushToTalkKey::Ctrl('v'))
        );
        assert_eq!(PushToTalkKey::parse("off"), None);
        assert_eq!(PushToTalkKey::F(8).to_config_string(), "F8");
    }

    #[test]
    fn strip_whisper_noise_removes_language_lines() {
        assert_eq!(
            strip_whisper_noise(
                "Detecting language using up to the first 30 seconds.\nDetected language: English\nhello world"
            ),
            "hello world"
        );
    }
}
