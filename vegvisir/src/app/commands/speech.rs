use crate::{
    app::TuiApplication,
    speech::{
        OPENAI_HBSE_SPEECH_PROVIDER, PushToTalkKey, speech_backend_status,
        start_push_to_talk_recording, stop_recording_and_transcribe_with_provider,
        strip_whisper_noise, synthesize_text_to_speech_with_provider,
        transcribe_audio_file_with_provider,
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
            "ptt" | "push-to-talk" => match self.toggle_push_to_talk_transcription() {
                Ok(message) => Ok(message),
                Err(error) => Ok(format!("Speech push-to-talk failed: {error}")),
            },
            other => Ok(format!(
                "Unknown /speech command: {other}\n{}",
                speech_usage()
            )),
        }
    }

    pub(crate) fn tts_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty()
            || matches!(
                args.first().map(String::as_str),
                Some("help" | "--help" | "-h")
            )
        {
            return Ok(tts_usage().to_string());
        }
        let mut voice: Option<String> = None;
        let mut out: Option<PathBuf> = None;
        let mut play = true;
        let mut text_parts = Vec::new();
        let mut index = 0usize;
        while index < args.len() {
            match args[index].as_str() {
                "--voice" | "-v" => {
                    let Some(value) = args.get(index + 1) else {
                        return Ok(
                            "Usage: /tts [--voice <voice>] [--out <path>] [--no-play] <text>"
                                .to_string(),
                        );
                    };
                    voice = Some(value.clone());
                    index += 2;
                }
                "--out" | "-o" => {
                    let Some(value) = args.get(index + 1) else {
                        return Ok(
                            "Usage: /tts [--voice <voice>] [--out <path>] [--no-play] <text>"
                                .to_string(),
                        );
                    };
                    out = Some(resolve_tts_output_path(&self.cwd, value));
                    index += 2;
                }
                "--no-play" | "--save-only" => {
                    play = false;
                    index += 1;
                }
                value => {
                    text_parts.push(value.to_string());
                    index += 1;
                }
            }
        }
        let text = text_parts.join(" ");
        if text.trim().is_empty() {
            return Ok(tts_usage().to_string());
        }
        self.start_text_to_speech(text, voice, out, play)
    }

    fn start_text_to_speech(
        &mut self,
        text: String,
        voice: Option<String>,
        out: Option<PathBuf>,
        play: bool,
    ) -> anyhow::Result<String> {
        self.session.activity = "unlocking HBSE broker for text-to-speech".to_string();
        self.logger.emit(
            "tts_unlock_start",
            serde_json::json!({
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
                "chars": text.chars().count(),
                "play": play,
            }),
        );
        self.redraw_requested = true;
        if let Err(error) = ensure_hbse_broker_unlocked_for_speech() {
            self.session.activity.clear();
            self.logger.emit(
                "tts_unlock_failed",
                serde_json::json!({
                    "session": self.session.session_id,
                    "workspace": self.cwd.display().to_string(),
                    "error": error.to_string(),
                }),
            );
            return Ok(format!(
                "Text-to-speech failed before synthesis started: {error}"
            ));
        }
        let provider = self.speech_provider_config()?.clone();
        let logger = self.logger.clone();
        let session_id = self.session.session_id.clone();
        let workspace = self.cwd.display().to_string();
        self.session.activity = "generating text-to-speech audio".to_string();
        self.pending_background_jobs
            .push(std::thread::spawn(move || {
                logger.emit(
                    "tts_started",
                    serde_json::json!({
                        "session": session_id,
                        "workspace": workspace,
                        "provider": provider.name,
                        "chars": text.chars().count(),
                        "voice": voice.as_deref(),
                        "play": play,
                    }),
                );
                match synthesize_text_to_speech_with_provider(
                    &text,
                    &provider,
                    voice.as_deref(),
                    out.as_deref(),
                    play,
                ) {
                    Ok(result) => {
                        logger.emit(
                            "tts_finished",
                            serde_json::json!({
                                "audio_path": result.audio_path.display().to_string(),
                                "audio_bytes": result.audio_bytes,
                                "model": result.model,
                                "voice": result.voice,
                                "playback": result.playback,
                            }),
                        );
                        Ok(format!("Text-to-speech complete. {}", result.summary()))
                    }
                    Err(error) => {
                        logger.emit(
                            "tts_failed",
                            serde_json::json!({
                                "error": error.to_string(),
                            }),
                        );
                        Err(anyhow::anyhow!("Text-to-speech failed: {error}"))
                    }
                }
            }));
        Ok("Text-to-speech started. Audio will be saved and played when ready.".to_string())
    }

    pub(crate) fn speech_status_message(&self) -> String {
        let ptt = self
            .speech_ptt_key
            .as_ref()
            .map(|key| key.label())
            .unwrap_or_else(|| "off".to_string());
        format!(
            "{}\n\nPush-to-talk:\n- key: {}\n- mode: toggle; press once to start recording and again to stop/transcribe/submit\n- recording: {}\n\n{}",
            speech_backend_status(),
            ptt,
            if self.active_speech_recording.is_some() {
                "active"
            } else {
                "idle"
            },
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
            "Push-to-talk key set to {label}. Press {label} once to start recording, then press {label} again to stop, transcribe, and submit."
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
                    "Transcribed {} into the input buffer using openai-hbse/gpt-4o-mini-transcribe. Review/edit, then press Enter to send.",
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

    pub(crate) fn toggle_push_to_talk_transcription(&mut self) -> anyhow::Result<String> {
        if self.active_speech_recording.is_some() {
            return self.stop_push_to_talk_transcription();
        }
        self.start_push_to_talk_transcription()
    }

    pub(crate) fn start_push_to_talk_transcription(&mut self) -> anyhow::Result<String> {
        if !self.pending_speech_jobs.is_empty() {
            return Ok("Speech transcription is already running; wait for it to finish before starting a new recording.".to_string());
        }
        self.session.activity = "unlocking HBSE broker for speech transcription".to_string();
        self.logger.emit(
            "speech_ptt_unlock_start",
            serde_json::json!({
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
            }),
        );
        self.redraw_requested = true;
        if let Err(error) = ensure_hbse_broker_unlocked_for_speech() {
            self.session.activity.clear();
            self.logger.emit(
                "speech_ptt_unlock_failed",
                serde_json::json!({
                    "session": self.session.session_id,
                    "workspace": self.cwd.display().to_string(),
                    "error": error.to_string(),
                }),
            );
            anyhow::bail!(error);
        }
        self.logger.emit(
            "speech_ptt_unlock_finished",
            serde_json::json!({
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
            }),
        );

        let provider = self.speech_provider_config()?.clone();
        let recording = start_push_to_talk_recording()?;
        self.session.activity = "recording speech until push-to-talk is pressed again".to_string();
        self.logger.emit(
            "speech_ptt_recording_started",
            serde_json::json!({
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
                "mode": "toggle",
                "provider": provider.name,
                "audio_path": recording.audio_path.display().to_string(),
                "recorder": recording.recorder,
            }),
        );
        self.active_speech_recording = Some(recording);
        self.redraw_requested = true;
        Ok("Push-to-talk recording started. Press the push-to-talk key again to stop, transcribe, and submit.".to_string())
    }

    pub(crate) fn stop_push_to_talk_transcription(&mut self) -> anyhow::Result<String> {
        let Some(recording) = self.active_speech_recording.take() else {
            return Ok("No active push-to-talk recording to stop.".to_string());
        };
        let provider = self.speech_provider_config()?.clone();
        let elapsed_ms = recording.elapsed().as_millis();
        self.session.activity = "stopping speech recording and transcribing".to_string();
        self.logger.emit(
            "speech_ptt_recording_stopping",
            serde_json::json!({
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
                "elapsed_ms": elapsed_ms,
                "audio_path": recording.audio_path.display().to_string(),
                "recorder": recording.recorder,
            }),
        );
        self.pending_speech_jobs.push(std::thread::spawn(move || {
            stop_recording_and_transcribe_with_provider(recording, &provider)
        }));
        self.redraw_requested = true;
        Ok("Push-to-talk recording stopped. Transcribing now; transcript will be submitted when ready.".to_string())
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

fn tts_usage() -> &'static str {
    "Usage:
  /tts [--voice <voice>] [--out <path>] [--no-play] <text>
  /speak [--voice <voice>] <text>

Text-to-speech uses OpenAI via HBSE (`openai-hbse`, `/v1/audio/speech`) and writes MP3 audio. Playback tries ffplay, mpv, paplay, then aplay. Use `--no-play` to save only."
}

fn speech_usage() -> String {
    format!(
        "Usage:\n  /speech status\n  /speech transcribe <audio-file>\n  /speech ptt\n  /speech ptt-key <F1..F24|Ctrl+letter|off>\n  /speech ptt-seconds <1..30>\n  /stt transcribe <audio-file>\n\nPush-to-talk key binding defaults to F9. Use `/speech ptt` manually or press the PTT key once to start recording and again to stop/transcribe/submit. Use `/speech ptt-key <key>` to change it, or `/speech ptt-key off` to disable it.\n\n{}",
        speech_install_help()
    )
}

fn speech_install_help() -> &'static str {
    "Speech-to-text uses OpenAI via HBSE (`openai-hbse` provider, `gpt-4o-mini-transcribe` model). Before push-to-talk recording, Vegvisir runs `hbse broker unlock` so the broker can authorize the transcription request without exposing plaintext credentials.\n\nFor push-to-talk recording, install `ffmpeg` with PulseAudio support or `arecord`. PTT is toggle-based: recording continues until you press the PTT key again. Local Whisper CLI tools are retained only as diagnostic/fallback code paths, not the primary STT backend."
}

fn resolve_tts_output_path(cwd: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
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
    fn speech_ptt_key_defaults_to_f9() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        assert_eq!(app.speech_ptt_key, Some(PushToTalkKey::F(9)));
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

    #[test]
    fn speech_status_reports_toggle_ptt_mode() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        let status = app.speech_status_message();
        assert!(status.contains("mode: toggle"));
        assert!(status.contains("press once to start recording and again to stop"));
        assert!(!status.contains("clip length:"));
        Ok(())
    }

    #[test]
    fn speech_usage_describes_toggle_ptt() {
        let usage = speech_usage();
        assert!(usage.contains("once to start recording and again to stop"));
        assert!(usage.contains("recording continues until you press the PTT key again"));
    }

    #[test]
    fn tts_command_without_text_shows_usage() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        let response = app.tts_command(&[])?;
        assert!(response.contains("/tts"));
        assert!(response.contains("--voice"));
        Ok(())
    }
}
