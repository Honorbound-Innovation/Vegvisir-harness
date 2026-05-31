use crate::{
    app::TuiApplication,
    speech::{
        OPENAI_HBSE_SPEECH_PROVIDER, PushToTalkKey, record_and_transcribe_with_provider,
        speech_backend_status, strip_whisper_noise, transcribe_audio_file_with_provider,
    },
};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

impl TuiApplication {
    pub(crate) fn speech_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let Some(subcommand) = args.first().map(String::as_str) else {
            return Ok(speech_usage());
        };
        match subcommand {
            "status" => Ok(self.speech_status_message()),
            "transcribe" => {
                let Some(path) = args.get(1) else {
                    return Ok("Usage: /speech transcribe <audio-file>".to_string());
                };
                let audio_path = resolve_speech_audio_path(&self.cwd, path)?;
                self.transcribe_audio_into_input(&audio_path)
            }
            "ptt-status" | "push-to-talk-status" => Ok(self.speech_status_message()),
            "ptt-key" | "push-to-talk-key" => self.speech_ptt_key_command(&args[1..]),
            "ptt-seconds" | "push-to-talk-seconds" => self.speech_ptt_seconds_command(&args[1..]),
            "ptt" | "push-to-talk" => {
                self.start_push_to_talk_transcription()?;
                Ok("Push-to-talk recording started. Speak now; transcript will be inserted into the input buffer when ready.".to_string())
            }
            other => Ok(format!(
                "Unknown /speech command: {other}\n{}",
                speech_usage()
            )),
        }
    }

    pub(crate) fn speech_status_message(&self) -> String {
        let ptt = self
            .speech_ptt_key
            .as_ref()
            .map(|key| key.label())
            .unwrap_or_else(|| "off".to_string());
        format!(
            "{}\n\nPush-to-talk:\n- key: {}\n- clip length: {}s\n\n{}",
            speech_backend_status(),
            ptt,
            self.speech_ptt_seconds,
            speech_install_help()
        )
    }

    fn speech_ptt_key_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let Some(value) = args.first().map(String::as_str) else {
            let current = self
                .speech_ptt_key
                .as_ref()
                .map(|key| key.label())
                .unwrap_or_else(|| "off".to_string());
            return Ok(format!(
                "Current push-to-talk key: {current}\nUsage: /speech ptt-key <F1..F24|Ctrl+letter|off>"
            ));
        };
        if value.eq_ignore_ascii_case("off") || value.eq_ignore_ascii_case("none") {
            self.speech_ptt_key = None;
            self.save_config_defaults()?;
            return Ok("Push-to-talk key disabled.".to_string());
        }
        let Some(key) = PushToTalkKey::parse(value) else {
            return Ok("Invalid push-to-talk key. Use F1..F24, Ctrl+letter, or off.".to_string());
        };
        let label = key.label();
        self.speech_ptt_key = Some(key);
        self.save_config_defaults()?;
        Ok(format!(
            "Push-to-talk key set to {label}. Press {label} to record a {}s clip and transcribe it into the input buffer.",
            self.speech_ptt_seconds
        ))
    }

    fn speech_ptt_seconds_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let Some(value) = args.first() else {
            return Ok(format!(
                "Current push-to-talk clip length: {}s\nUsage: /speech ptt-seconds <1..30>",
                self.speech_ptt_seconds
            ));
        };
        let Ok(seconds) = value.parse::<u64>() else {
            return Ok("Invalid clip length. Use a number from 1 to 30.".to_string());
        };
        self.speech_ptt_seconds = seconds.clamp(1, 30);
        self.save_config_defaults()?;
        Ok(format!(
            "Push-to-talk clip length set to {}s.",
            self.speech_ptt_seconds
        ))
    }

    fn transcribe_audio_into_input(&mut self, audio_path: &Path) -> anyhow::Result<String> {
        let provider = self.speech_provider_config()?;
        match transcribe_audio_file_with_provider(audio_path, provider) {
            Ok(text) => {
                let text = text.trim().to_string();
                if text.is_empty() {
                    return Ok(format!(
                        "Speech transcription completed but returned no text for {}.",
                        audio_path.display()
                    ));
                }
                self.insert_speech_text(&text);
                Ok(format!(
                    "Transcribed {} into the input buffer using openai-hbse/gpt-realtime-whisper. Review/edit, then press Enter to send.",
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

    fn speech_provider_config(&self) -> anyhow::Result<&crate::core::ProviderConfig> {
        self.provider_registry
            .get(OPENAI_HBSE_SPEECH_PROVIDER)
            .ok_or_else(|| {
                anyhow::anyhow!("speech provider {OPENAI_HBSE_SPEECH_PROVIDER} is not configured")
            })
    }

    pub(crate) fn insert_speech_text(&mut self, text: &str) {
        let text = strip_whisper_noise(text).trim().to_string();
        if text.is_empty() {
            return;
        }
        if !self.input.buffer.is_empty() && !self.input.buffer.ends_with(char::is_whitespace) {
            self.input.append_text(" ", false);
        }
        self.input.append_text(&text, false);
        self.input.update_suggestions(Vec::new());
        self.redraw_requested = true;
    }

    pub(crate) fn start_push_to_talk_transcription(&mut self) -> anyhow::Result<()> {
        if !self.pending_speech_jobs.is_empty() {
            self.push_system_message("Speech transcription is already running.");
            return Ok(());
        }
        self.session.activity = "unlocking HBSE broker for speech transcription".to_string();
        self.redraw_requested = true;
        ensure_hbse_broker_unlocked_for_speech()?;

        let seconds = self.speech_ptt_seconds;
        let provider = self.speech_provider_config()?.clone();
        self.session.activity = format!("recording speech for {seconds}s");
        self.pending_speech_jobs.push(std::thread::spawn(move || {
            record_and_transcribe_with_provider(seconds, &provider)
        }));
        self.redraw_requested = true;
        Ok(())
    }
}

fn ensure_hbse_broker_unlocked_for_speech() -> anyhow::Result<()> {
    let output = Command::new("hbse")
        .args(["broker", "unlock"])
        .output()
        .map_err(|error| {
            anyhow::anyhow!(
                "failed to run `hbse broker unlock` before speech transcription: {error}"
            )
        })?;
    validate_hbse_broker_unlock_output(output.status.success(), &output.stdout, &output.stderr)
}

fn validate_hbse_broker_unlock_output(
    success: bool,
    stdout: &[u8],
    stderr: &[u8],
) -> anyhow::Result<()> {
    if success {
        return Ok(());
    }
    let mut detail = String::new();
    detail.push_str(&String::from_utf8_lossy(stdout));
    detail.push_str(&String::from_utf8_lossy(stderr));
    anyhow::bail!(
        "`hbse broker unlock` failed before speech transcription: {}",
        detail.trim().chars().take(600).collect::<String>()
    )
}

fn speech_usage() -> String {
    format!(
        "Usage:\n  /speech status\n  /speech transcribe <audio-file>\n  /speech ptt\n  /speech ptt-key <F1..F24|Ctrl+letter|off>\n  /speech ptt-seconds <1..30>\n  /stt transcribe <audio-file>\n\nPush-to-talk key binding is off by default. Use `/speech ptt` manually, or `/speech ptt-key F8` to enable a key later.\n\n{}",
        speech_install_help()
    )
}

fn speech_install_help() -> &'static str {
    "Speech-to-text uses OpenAI via HBSE (`openai-hbse` provider, `gpt-realtime-whisper` model). Before push-to-talk recording, Vegvisir runs `hbse broker unlock` so the broker can authorize the transcription request without exposing plaintext credentials.\n\nFor push-to-talk recording, install `ffmpeg` with PulseAudio support or `arecord`. Local Whisper CLI tools are retained only as diagnostic/fallback code paths, not the primary STT backend."
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_ptt_key_defaults_off() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        assert_eq!(app.speech_ptt_key, None);
        Ok(())
    }

    #[test]
    fn hbse_unlock_output_accepts_success() {
        assert!(validate_hbse_broker_unlock_output(true, b"already unlocked", b"").is_ok());
    }

    #[test]
    fn hbse_unlock_output_reports_failure_detail() {
        let err = validate_hbse_broker_unlock_output(false, b"", b"broker locked or unavailable")
            .expect_err("unlock failure should be reported");
        assert!(err.to_string().contains("hbse broker unlock"));
        assert!(err.to_string().contains("broker locked or unavailable"));
    }

    #[test]
    fn speech_ptt_key_command_persists_key() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        let response = app.speech_command(&["ptt-key".into(), "F9".into()])?;
        assert!(response.contains("F9"));
        assert_eq!(app.speech_ptt_key, Some(PushToTalkKey::F(9)));
        Ok(())
    }
}
