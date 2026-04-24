use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

use crate::audio::AudioRecorder;
use crate::cleanup::cleanup_text;
use crate::paste::paste_text;
use crate::settings::Settings;
use crate::transcribe_groq;
use crate::transcribe_local;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum RecordingState {
    Ready,
    Recording,
    Transcribing,
}

pub struct Recorder {
    state: Arc<Mutex<RecordingState>>,
    audio_recorder: Arc<Mutex<AudioRecorder>>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RecordingState::Ready)),
            audio_recorder: Arc::new(Mutex::new(AudioRecorder::new())),
        }
    }

    pub fn get_state(&self) -> RecordingState {
        self.state.lock().unwrap().clone()
    }

    fn reset_to_ready(&self, app: &AppHandle) {
        let mut state = self.state.lock().unwrap();
        *state = RecordingState::Ready;
        drop(state);

        let _ = app.emit("audio-level", 0.0_f32);
        let _ = app.emit("recording-state", RecordingState::Ready);
    }

    pub fn start_recording(&self, app: &AppHandle, mic_name: &str) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if *state != RecordingState::Ready {
            return Err("Already recording or transcribing".to_string());
        }

        let mut recorder = self.audio_recorder.lock().unwrap();
        recorder.start(app, mic_name)?;

        *state = RecordingState::Recording;
        let _ = app.emit("recording-state", RecordingState::Recording);
        Ok(())
    }

    pub async fn stop_and_transcribe(
        &self,
        app: &AppHandle,
        settings: &Settings,
        app_dir: &PathBuf,
    ) -> Result<String, String> {
        // Stop recording
        {
            let mut state = self.state.lock().unwrap();
            if *state != RecordingState::Recording {
                return Err("Not currently recording".to_string());
            }
            *state = RecordingState::Transcribing;
            let _ = app.emit("audio-level", 0.0_f32);
            let _ = app.emit("recording-state", RecordingState::Transcribing);
        }

        let temp_path = app_dir.join("temp_recording.wav");

        let result = async {
            // Save audio
            {
                let mut recorder = self.audio_recorder.lock().unwrap();
                recorder.stop_and_save(&temp_path)?;
            }

            // Transcribe
            let raw_text = match settings.engine.as_str() {
                "local" => {
                    let model_path =
                        app_dir.join(transcribe_local::model_filename(&settings.whisper_model));
                    transcribe_local::transcribe_local(
                        app,
                        &model_path,
                        &temp_path,
                        &settings.transcription_language,
                    )
                    .await?
                }
                "cloud" => {
                    transcribe_groq::transcribe_groq(
                        &settings.groq_api_key,
                        &settings.groq_model,
                        &settings.transcription_language,
                        &temp_path,
                    )
                    .await?
                }
                _ => return Err(format!("Unknown engine: {}", settings.engine)),
            };

            // Clean up text
            let cleaned = cleanup_text(&raw_text);

            // Auto-paste
            if !cleaned.is_empty() {
                paste_text(&cleaned)?;
            }

            Ok(cleaned)
        }
        .await;

        let _ = std::fs::remove_file(&temp_path);
        self.reset_to_ready(app);

        if let Err(error) = &result {
            eprintln!("[Rawi] Transcription pipeline failed: {}", error);
            let _ = app.emit("recording-error", error.clone());
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_is_ready() {
        let recorder = Recorder::new();
        assert_eq!(recorder.get_state(), RecordingState::Ready);
    }
}
