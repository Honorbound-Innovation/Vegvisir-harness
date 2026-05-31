use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};

use crate::core::ProviderConfig;

pub const DEFAULT_PTT_SECONDS: u64 = 8;
pub const OPENAI_HBSE_SPEECH_PROVIDER: &str = "openai-hbse";
pub const OPENAI_HBSE_SPEECH_MODEL: &str = "gpt-realtime-whisper";
const SPEECH_TIMEOUT: Duration = Duration::from_secs(180);
const SPEECH_HBSE_TIMEOUT_SECONDS: u64 = 180;
const SPEECH_HBSE_MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct SpeechTranscriptionResult {
    pub transcript: String,
    pub recorder: String,
    pub audio_path: PathBuf,
    pub audio_bytes: u64,
    pub kept_audio: bool,
}

impl SpeechTranscriptionResult {
    pub fn summary(&self) -> String {
        let preview = self.transcript.trim().chars().take(120).collect::<String>();
        format!(
            "Speech push-to-talk used {recorder}; captured {bytes} bytes; transcript preview: {preview}",
            recorder = self.recorder,
            bytes = self.audio_bytes,
            preview = if preview.is_empty() {
                "<empty>".to_string()
            } else {
                preview
            }
        )
    }
}

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
    let stt = format!(
        "- provider: {OPENAI_HBSE_SPEECH_PROVIDER} model: {OPENAI_HBSE_SPEECH_MODEL} (OpenAI transcription via HBSE): configured"
    );
    let local_fallbacks = speech_backends()
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
    format!(
        "Speech-to-text backends:\n{stt}\n\nLocal fallback/diagnostic backends:\n{local_fallbacks}\n\nPush-to-talk recorders:\n{recorders}"
    )
}

pub fn transcribe_audio_file_with_provider(
    path: &Path,
    provider: &ProviderConfig,
) -> anyhow::Result<String> {
    if provider.name == OPENAI_HBSE_SPEECH_PROVIDER || provider.kind == "hbse_openai_compatible" {
        return transcribe_audio_file_openai_hbse(path, provider);
    }
    transcribe_audio_file(path)
}

pub fn transcribe_audio_file(path: &Path) -> anyhow::Result<String> {
    let mut attempted = Vec::new();
    let mut errors = Vec::new();
    for backend in speech_backends() {
        if !executable_in_path(backend.command) {
            continue;
        }
        attempted.push(backend.command);
        let result = match backend.kind {
            SpeechBackendKind::OpenAiWhisper => run_openai_whisper(backend.command, path),
            SpeechBackendKind::WhisperCli => run_whisper_cli(backend.command, path),
        };
        match result {
            Ok(text) if !text.trim().is_empty() => return Ok(text),
            Ok(_) => errors.push(format!("{} returned an empty transcript", backend.command)),
            Err(error) => errors.push(format!("{} failed: {error}", backend.command)),
        }
    }
    if attempted.is_empty() {
        anyhow::bail!("no local Whisper speech-to-text backend found on PATH")
    }
    anyhow::bail!(
        "no usable local Whisper speech-to-text backend produced text: {}",
        errors.join("; ")
    )
}

pub fn record_and_transcribe(seconds: u64) -> anyhow::Result<SpeechTranscriptionResult> {
    let seconds = seconds.clamp(1, 30);
    let audio_path = std::env::temp_dir().join(format!(
        "vegvisir-ptt-{}-{}.wav",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let recorder = match record_audio_clip(&audio_path, seconds) {
        Ok(recorder) => recorder,
        Err(error) => {
            let _ = std::fs::remove_file(&audio_path);
            return Err(error);
        }
    };
    let audio_bytes = std::fs::metadata(&audio_path)
        .map(|meta| meta.len())
        .unwrap_or(0);
    if audio_bytes < 1024 {
        anyhow::bail!(
            "push-to-talk recorder {recorder} produced an unexpectedly small audio file ({audio_bytes} bytes) at {}",
            audio_path.display()
        );
    }
    match transcribe_audio_file(&audio_path) {
        Ok(transcript) => {
            let transcript = transcript.trim().to_string();
            let kept_audio = transcript.is_empty();
            if !kept_audio {
                let _ = std::fs::remove_file(&audio_path);
            }
            Ok(SpeechTranscriptionResult {
                transcript,
                recorder,
                audio_path,
                audio_bytes,
                kept_audio,
            })
        }
        Err(error) => Err(anyhow::anyhow!(
            "speech transcription failed after recording {audio_bytes} bytes with {recorder}; kept audio at {} for inspection: {error}",
            audio_path.display()
        )),
    }
}

pub fn record_and_transcribe_with_provider(
    seconds: u64,
    provider: &ProviderConfig,
) -> anyhow::Result<SpeechTranscriptionResult> {
    let seconds = seconds.clamp(1, 30);
    let audio_path = std::env::temp_dir().join(format!(
        "vegvisir-ptt-{}-{}.wav",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let recorder = match record_audio_clip(&audio_path, seconds) {
        Ok(recorder) => recorder,
        Err(error) => {
            let _ = std::fs::remove_file(&audio_path);
            return Err(error);
        }
    };
    let audio_bytes = std::fs::metadata(&audio_path)
        .map(|meta| meta.len())
        .unwrap_or(0);
    if audio_bytes < 1024 {
        anyhow::bail!(
            "push-to-talk recorder {recorder} produced an unexpectedly small audio file ({audio_bytes} bytes) at {}",
            audio_path.display()
        );
    }
    match transcribe_audio_file_with_provider(&audio_path, provider) {
        Ok(transcript) => {
            let transcript = transcript.trim().to_string();
            let kept_audio = transcript.is_empty();
            if !kept_audio {
                let _ = std::fs::remove_file(&audio_path);
            }
            Ok(SpeechTranscriptionResult {
                transcript,
                recorder: format!(
                    "{recorder}; STT {OPENAI_HBSE_SPEECH_PROVIDER}/{OPENAI_HBSE_SPEECH_MODEL}"
                ),
                audio_path,
                audio_bytes,
                kept_audio,
            })
        }
        Err(error) => Err(anyhow::anyhow!(
            "speech transcription failed after recording {audio_bytes} bytes with {recorder}; kept audio at {} for inspection: {error}",
            audio_path.display()
        )),
    }
}

fn record_audio_clip(path: &Path, seconds: u64) -> anyhow::Result<String> {
    let mut attempted = Vec::new();
    for backend in recorder_backends() {
        if !executable_in_path(backend.command) {
            continue;
        }
        attempted.push(backend.command);
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
            Ok(output) if output.status.success() && path.is_file() => {
                let bytes = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
                if bytes >= 1024 {
                    return Ok(format!("{} ({})", backend.command, backend.label));
                }
            }
            Ok(_) | Err(_) => continue,
        }
    }
    if attempted.is_empty() {
        anyhow::bail!(
            "no local audio recorder found on PATH; install ffmpeg with PulseAudio support or arecord"
        )
    }
    anyhow::bail!(
        "local audio recorders were found ({}) but none produced a usable audio file",
        attempted.join(", ")
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

fn transcribe_audio_file_openai_hbse(
    path: &Path,
    provider: &ProviderConfig,
) -> anyhow::Result<String> {
    let boundary = format!(
        "----vegvisir-speech-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let body = openai_transcription_multipart_body(path, OPENAI_HBSE_SPEECH_MODEL, &boundary)?;
    let base_url = provider
        .base_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("provider {} has no base_url", provider.name))?;
    let response = hbse_provider_http_binary(
        provider,
        &format!("{}/audio/transcriptions", base_url.trim_end_matches('/')),
        &format!("multipart/form-data; boundary={boundary}"),
        body,
    )?;
    let status = response
        .get("status_code")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let body = response
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if status >= 400 {
        anyhow::bail!(
            "{} speech transcription failed through HBSE: {} {}",
            provider.name,
            status,
            body.chars().take(600).collect::<String>()
        );
    }
    let value: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({"text": body}));
    value
        .get("text")
        .and_then(Value::as_str)
        .map(strip_whisper_noise)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{} speech response did not include transcript text",
                provider.name
            )
        })
}

fn openai_transcription_multipart_body(
    path: &Path,
    model: &str,
    boundary: &str,
) -> anyhow::Result<Vec<u8>> {
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("audio.wav");
    let audio = std::fs::read(path)?;
    let mut body = Vec::new();
    write_multipart_text(&mut body, boundary, "model", model)?;
    write_multipart_text(&mut body, boundary, "response_format", "json")?;
    write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
        sanitize_multipart_filename(filename),
        audio_mime_type(path)
    )?;
    body.extend_from_slice(&audio);
    write!(body, "\r\n--{boundary}--\r\n")?;
    Ok(body)
}

fn write_multipart_text(
    body: &mut Vec<u8>,
    boundary: &str,
    name: &str,
    value: &str,
) -> anyhow::Result<()> {
    write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
    )?;
    Ok(())
}

fn sanitize_multipart_filename(value: &str) -> String {
    value.replace(['\r', '\n', '"'], "_")
}

fn audio_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "webm" => "audio/webm",
        _ => "audio/wav",
    }
}

fn hbse_provider_http_binary(
    provider: &ProviderConfig,
    url: &str,
    content_type: &str,
    body: Vec<u8>,
) -> anyhow::Result<Value> {
    let socket_path = crate::provider::hbse_default_or_configured_socket(provider);
    let secret_ref = provider
        .metadata
        .get("hbse_secret_ref")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| std::env::var("HBSE_PROVIDER_SECRET_REF").ok())
        .ok_or_else(|| anyhow::anyhow!("Set HBSE_PROVIDER_SECRET_REF or provider metadata hbse_secret_ref to use HBSE-routed speech transcription."))?;
    let consumer = provider
        .metadata
        .get("hbse_consumer")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("vegvisir.provider.{}", provider.name));
    let purpose = provider
        .metadata
        .get("hbse_speech_purpose")
        .and_then(Value::as_str)
        .or_else(|| {
            provider
                .metadata
                .get("hbse_purpose")
                .and_then(Value::as_str)
        })
        .unwrap_or("model.speech.transcription");
    let payload = json!({
        "command": "provider_http",
        "secret_ref": secret_ref,
        "consumer": consumer,
        "purpose": purpose,
        "method": "POST",
        "url": url,
        "headers": {
            "Content-Type": content_type,
            "Accept": "application/json"
        },
        "body_base64": STANDARD.encode(body),
        "credential_header": provider.metadata.get("credential_header").and_then(Value::as_str).unwrap_or("Authorization"),
        "credential_prefix": provider.metadata.get("credential_prefix").and_then(Value::as_str).unwrap_or("Bearer "),
        "timeout_seconds": SPEECH_HBSE_TIMEOUT_SECONDS,
        "max_response_bytes": SPEECH_HBSE_MAX_RESPONSE_BYTES,
    });
    let mut stream = UnixStream::connect(&socket_path).map_err(|error| {
        anyhow::anyhow!("HBSE broker unavailable for speech transcription: {error}")
    })?;
    stream.write_all(serde_json::to_string(&payload)?.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    let response: Value = serde_json::from_str(&line)?;
    if !response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        let message = response
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| response.get("error").map(Value::to_string))
            .unwrap_or_else(|| "unknown HBSE broker error".to_string());
        anyhow::bail!("HBSE broker denied speech transcription request: {message}");
    }
    Ok(response)
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
