use crate::app::TuiApplication;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

impl TuiApplication {
    pub(crate) fn speech_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let Some(subcommand) = args.first().map(String::as_str) else {
            return Ok(speech_usage());
        };
        match subcommand {
            "status" => Ok(speech_status()),
            "transcribe" => {
                let Some(path) = args.get(1) else {
                    return Ok("Usage: /speech transcribe <audio-file>".to_string());
                };
                let audio_path = resolve_speech_audio_path(&self.cwd, path)?;
                match transcribe_audio_file(&audio_path) {
                    Ok(text) => {
                        let text = text.trim().to_string();
                        if text.is_empty() {
                            return Ok(format!(
                                "Speech transcription completed but returned no text for {}.",
                                audio_path.display()
                            ));
                        }
                        if !self.input.buffer.is_empty()
                            && !self.input.buffer.ends_with(char::is_whitespace)
                        {
                            self.input.append_text(" ", false);
                        }
                        self.input.append_text(&text, false);
                        self.input.update_suggestions(Vec::new());
                        self.redraw_requested = true;
                        Ok(format!(
                            "Transcribed {} into the input buffer using local speech-to-text. Review/edit, then press Enter to send.",
                            audio_path.display()
                        ))
                    }
                    Err(error) => Ok(format!(
                        "Speech transcription failed for {}: {error}\n\n{}",
                        audio_path.display(),
                        speech_install_help()
                    )),
                }
            }
            other => Ok(format!(
                "Unknown /speech command: {other}\n{}",
                speech_usage()
            )),
        }
    }
}

fn speech_usage() -> String {
    format!(
        "Usage:\n  /speech status\n  /speech transcribe <audio-file>\n  /stt transcribe <audio-file>\n\n{}",
        speech_install_help()
    )
}

fn speech_status() -> String {
    let backends = speech_backends()
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
    format!(
        "Speech-to-text backend status:\n{backends}\n\nVegvisir currently uses local Whisper-compatible CLI tools only. No audio or credentials are sent to a remote provider by this command.\n\n{}",
        speech_install_help()
    )
}

fn speech_install_help() -> &'static str {
    "Do not run `cargo install whisper`: the crates.io `whisper` package is an old unrelated database crate and does not provide speech-to-text.\n\nInstall one of these real Whisper STT backends instead:\n  - OpenAI Whisper Python CLI: `pipx install openai-whisper` or `python3 -m pip install --user openai-whisper`\n  - whisper.cpp: build/install whisper.cpp so `whisper-cli` is on PATH\n\nThen run `/speech transcribe path/to/audio.wav`."
}

fn resolve_speech_audio_path(cwd: &Path, value: &str) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(value);
    let resolved = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    if !resolved.exists() {
        anyhow::bail!("audio file does not exist");
    }
    if !resolved.is_file() {
        anyhow::bail!("audio path is not a file");
    }
    Ok(resolved)
}

struct SpeechBackend {
    command: &'static str,
    label: &'static str,
    kind: SpeechBackendKind,
}

enum SpeechBackendKind {
    OpenAiWhisper,
    WhisperCli,
}

fn speech_backends() -> Vec<SpeechBackend> {
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

const SPEECH_TIMEOUT: Duration = Duration::from_secs(180);

fn transcribe_audio_file(path: &Path) -> anyhow::Result<String> {
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

fn run_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> anyhow::Result<std::process::Output> {
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
    let started = std::time::Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(child.wait_with_output()?);
        }
        if started.elapsed() >= timeout {
            kill_process_tree(&mut child);
            anyhow::bail!(
                "speech transcription timed out after {} seconds",
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

fn strip_whisper_noise(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("Detecting language"))
        .filter(|line| !line.starts_with("Detected language"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn executable_in_path(command: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(command).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

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
