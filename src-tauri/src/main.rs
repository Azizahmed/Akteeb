#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, State, WindowEvent};
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use rawi_lib::audio;
use rawi_lib::downloader;
use rawi_lib::recorder::{Recorder, RecordingState};
use rawi_lib::settings::Settings;
use rawi_lib::transcribe_groq;
use rawi_lib::transcribe_local;

struct AppState {
    recorder: Recorder,
    settings: Mutex<Settings>,
    app_dir: PathBuf,
    is_quitting: AtomicBool,
    shortcut_is_down: AtomicBool,
}

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_SHOW_ID: &str = "tray_show";
const TRAY_QUIT_ID: &str = "tray_quit";

fn get_app_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.rawi.app")
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

    if previous_settings.launch_at_startup != settings.launch_at_startup {
        if let Err(error) = set_launch_at_startup(&app, settings.launch_at_startup) {
            if previous_settings.hotkey != settings.hotkey {
                let _ = update_global_shortcut(&app, &settings.hotkey, &previous_settings.hotkey);
            }
            return Err(error);
        }
    }

    if let Err(error) = settings.save(&state.app_dir) {
        if previous_settings.hotkey != settings.hotkey {
            let _ = update_global_shortcut(&app, &settings.hotkey, &previous_settings.hotkey);
        }
        if previous_settings.launch_at_startup != settings.launch_at_startup {
            let _ = set_launch_at_startup(&app, previous_settings.launch_at_startup);
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
async fn do_toggle_recording(app: &tauri::AppHandle, state: &AppState) -> Result<String, String> {
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
        RecordingState::Transcribing => Err("Currently transcribing, please wait".to_string()),
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
            let state = app.state::<AppState>();
            let was_pressed = state.shortcut_is_down.swap(true, Ordering::AcqRel);
            if was_pressed {
                return;
            }

            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                match mode.as_str() {
                    "toggle" => match do_toggle_recording(&app, state.inner()).await {
                        Ok(result) => println!("[Rawi] Toggle result: {}", result),
                        Err(e) => eprintln!("[Rawi] Toggle error: {}", e),
                    },
                    "push-to-talk" => {
                        let current = state.recorder.get_state();
                        if current == RecordingState::Ready {
                            let mic = state.settings.lock().unwrap().microphone.clone();
                            match state.recorder.start_recording(&app, &mic) {
                                Ok(_) => println!("[Rawi] Recording started"),
                                Err(e) => eprintln!("[Rawi] Start recording error: {}", e),
                            }
                        }
                    }
                    _ => {}
                }
            });
        }
        ShortcutState::Released => {
            let state = app.state::<AppState>();
            let was_pressed = state.shortcut_is_down.swap(false, Ordering::AcqRel);
            if !was_pressed {
                return;
            }

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
                            Ok(result) => println!("[Rawi] Transcription: {}", result),
                            Err(e) => eprintln!("[Rawi] Transcription error: {}", e),
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
            println!(
                "[Rawi] Hotkey event: {:?} state={:?}",
                shortcut, event.state
            );
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

    app.global_shortcut().unregister(old_hotkey).map_err(|e| {
        format!(
            "Failed to unregister previous hotkey '{}': {}",
            old_hotkey, e
        )
    })?;

    if let Err(error) = register_global_shortcut(app, new_hotkey) {
        let _ = register_global_shortcut(app, old_hotkey);
        return Err(error);
    }

    Ok(())
}

fn set_launch_at_startup(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let autostart_manager = app.autolaunch();
    if enabled {
        autostart_manager
            .enable()
            .map_err(|e| format!("Failed to enable launch at startup: {}", e))
    } else {
        autostart_manager
            .disable()
            .map_err(|e| format!("Failed to disable launch at startup: {}", e))
    }
}

fn show_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window(MAIN_WINDOW_LABEL)
        .ok_or_else(|| "Main window not found".to_string())?;

    let _ = window.unminimize();
    window
        .show()
        .map_err(|e| format!("Failed to show main window: {}", e))?;
    window
        .set_focus()
        .map_err(|e| format!("Failed to focus main window: {}", e))?;
    Ok(())
}

fn main() {
    let app_dir = get_app_dir();
    let settings = Settings::load(&app_dir);
    let initial_hotkey = settings.hotkey.clone();
    let launch_at_startup = settings.launch_at_startup;
    let launched_at_startup = std::env::args().any(|arg| arg == "--startup");

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--startup"]),
        ))
        .manage(AppState {
            recorder: Recorder::new(),
            settings: Mutex::new(settings),
            app_dir,
            is_quitting: AtomicBool::new(false),
            shortcut_is_down: AtomicBool::new(false),
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
        .on_window_event(|window, event| {
            if window.label() != MAIN_WINDOW_LABEL {
                return;
            }

            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if state.is_quitting.load(Ordering::Relaxed) {
                    return;
                }

                api.prevent_close();

                // Closing the main window hides the app so it keeps running from the tray.
                if let Err(error) = window.hide() {
                    eprintln!("[Rawi] Failed to hide main window: {}", error);
                }
            }
        })
        .setup(move |app| {
            println!("[Rawi] Registering global shortcut: {}", initial_hotkey);

            match register_global_shortcut(&app.handle().clone(), initial_hotkey.as_str()) {
                Ok(_) => println!("[Rawi] Global shortcut registered successfully"),
                Err(e) => eprintln!("[Rawi] ERROR: Failed to register global shortcut: {}", e),
            }

            if let Err(error) = set_launch_at_startup(&app.handle().clone(), launch_at_startup) {
                eprintln!("[Rawi] Failed to apply launch at startup setting: {}", error);
            }

            let tray_menu = MenuBuilder::new(app)
                .text(TRAY_SHOW_ID, "Open Rawi")
                .separator()
                .text(TRAY_QUIT_ID, "Quit")
                .build()?;

            let mut tray_builder = TrayIconBuilder::with_id("main-tray")
                .menu(&tray_menu)
                .tooltip("Rawi")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    TRAY_SHOW_ID => {
                        if let Err(error) = show_main_window(app) {
                            eprintln!("[Rawi] Failed to open from tray: {}", error);
                        }
                    }
                    TRAY_QUIT_ID => {
                        let state = app.state::<AppState>();
                        state.is_quitting.store(true, Ordering::Relaxed);
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| match event {
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        ..
                    }
                    | TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    } => {
                        if let Err(error) = show_main_window(tray.app_handle()) {
                            eprintln!("[Rawi] Failed to show window from tray click: {}", error);
                        }
                    }
                    _ => {}
                });

            if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon);
            }

            tray_builder.build(app)?;

            if launched_at_startup {
                if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                    if let Err(error) = window.hide() {
                        eprintln!("[Rawi] Failed to hide startup window: {}", error);
                    }
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
