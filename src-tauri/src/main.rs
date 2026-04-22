#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use typr_lib::audio;
use typr_lib::downloader;
use typr_lib::recorder::{Recorder, RecordingState};
use typr_lib::settings::Settings;
use typr_lib::transcribe_groq;
use typr_lib::transcribe_local;

struct AppState {
    recorder: Recorder,
    settings: Mutex<Settings>,
    app_dir: PathBuf,
}

fn get_app_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.typr.app")
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    state: State<AppState>,
    settings: Settings,
) -> Result<(), String> {
    let previous_settings = state.settings.lock().unwrap().clone();

    if previous_settings.hotkey != settings.hotkey {
        update_global_shortcut(&app, &previous_settings.hotkey, &settings.hotkey)?;
    }

    if let Err(error) = settings.save(&state.app_dir) {
        if previous_settings.hotkey != settings.hotkey {
            let _ = update_global_shortcut(&app, &settings.hotkey, &previous_settings.hotkey);
        }
        return Err(error);
    }

    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
fn list_microphones() -> Vec<audio::MicDevice> {
    audio::list_microphones()
}

#[tauri::command]
fn get_recording_state(state: State<AppState>) -> RecordingState {
    state.recorder.get_state()
}

#[tauri::command]
fn check_model_downloaded(state: State<AppState>, model_size: String) -> bool {
    let model_file = transcribe_local::model_filename(&model_size);
    state.app_dir.join(&model_file).exists()
}

#[tauri::command]
async fn download_model(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    model_size: String,
) -> Result<(), String> {
    let url = transcribe_local::model_download_url(&model_size);
    let model_file = transcribe_local::model_filename(&model_size);
    let dest = state.app_dir.join(&model_file);
    downloader::download_model(app, &url, &dest).await
}

#[tauri::command]
async fn toggle_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    do_toggle_recording(&app, &state).await
}

#[tauri::command]
async fn test_groq_connection(api_key: String, model: String) -> Result<String, String> {
    transcribe_groq::test_groq_connection(&api_key, &model).await
}

/// Shared logic for toggle recording, used by both the Tauri command and hotkey handler.
async fn do_toggle_recording(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<String, String> {
    let current_state = state.recorder.get_state();
    match current_state {
        RecordingState::Ready => {
            let mic = state.settings.lock().unwrap().microphone.clone();
            state.recorder.start_recording(app, &mic)?;
            Ok("recording".to_string())
        }
        RecordingState::Recording => {
            let settings = state.settings.lock().unwrap().clone();
            let result = state
                .recorder
                .stop_and_transcribe(app, &settings, &state.app_dir)
                .await?;
            Ok(result)
        }
        RecordingState::Transcribing => {
            Err("Currently transcribing, please wait".to_string())
        }
    }
}

fn handle_shortcut_event(app: tauri::AppHandle, event_state: ShortcutState) {
    let mode = {
        let state = app.state::<AppState>();
        let mode = state.settings.lock().unwrap().recording_mode.clone();
        mode
    };

    match event_state {
        ShortcutState::Pressed => {
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                match mode.as_str() {
                    "toggle" => match do_toggle_recording(&app, state.inner()).await {
                        Ok(result) => println!("[Typr] Toggle result: {}", result),
                        Err(e) => eprintln!("[Typr] Toggle error: {}", e),
                    },
                    "push-to-talk" => {
                        let current = state.recorder.get_state();
                        if current == RecordingState::Ready {
                            let mic = state.settings.lock().unwrap().microphone.clone();
                            match state.recorder.start_recording(&app, &mic) {
                                Ok(_) => println!("[Typr] Recording started"),
                                Err(e) => eprintln!("[Typr] Start recording error: {}", e),
                            }
                        }
                    }
                    _ => {}
                }
            });
        }
        ShortcutState::Released => {
            if mode == "push-to-talk" {
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    let current = state.recorder.get_state();
                    if current == RecordingState::Recording {
                        let settings = state.settings.lock().unwrap().clone();
                        match state
                            .recorder
                            .stop_and_transcribe(&app, &settings, &state.app_dir)
                            .await
                        {
                            Ok(result) => println!("[Typr] Transcription: {}", result),
                            Err(e) => eprintln!("[Typr] Transcription error: {}", e),
                        }
                    }
                });
            }
        }
    }
}

fn register_global_shortcut(app: &tauri::AppHandle, hotkey: &str) -> Result<(), String> {
    let handle = app.clone();
    app.global_shortcut()
        .on_shortcut(hotkey, move |_app, shortcut, event| {
            println!("[Typr] Hotkey event: {:?} state={:?}", shortcut, event.state);
            handle_shortcut_event(handle.clone(), event.state);
        })
        .map_err(|e| format!("Failed to register global shortcut '{}': {}", hotkey, e))
}

fn update_global_shortcut(
    app: &tauri::AppHandle,
    old_hotkey: &str,
    new_hotkey: &str,
) -> Result<(), String> {
    if old_hotkey == new_hotkey {
        return Ok(());
    }

    app.global_shortcut()
        .unregister(old_hotkey)
        .map_err(|e| format!("Failed to unregister previous hotkey '{}': {}", old_hotkey, e))?;

    if let Err(error) = register_global_shortcut(app, new_hotkey) {
        let _ = register_global_shortcut(app, old_hotkey);
        return Err(error);
    }

    Ok(())
}

fn main() {
    let app_dir = get_app_dir();
    let settings = Settings::load(&app_dir);
    let initial_hotkey = settings.hotkey.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            recorder: Recorder::new(),
            settings: Mutex::new(settings),
            app_dir,
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            list_microphones,
            get_recording_state,
            check_model_downloaded,
            download_model,
            toggle_recording,
            test_groq_connection,
        ])
        .setup(move |app| {
            println!("[Typr] Registering global shortcut: {}", initial_hotkey);

            match register_global_shortcut(&app.handle().clone(), initial_hotkey.as_str()) {
                Ok(_) => println!("[Typr] Global shortcut registered successfully"),
                Err(e) => eprintln!("[Typr] ERROR: Failed to register global shortcut: {}", e),
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
