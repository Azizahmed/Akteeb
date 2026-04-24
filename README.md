# Rawi (راوي)

Rawi is a Windows desktop dictation app built with Tauri, Rust, TypeScript, and Vite. It records from your microphone, transcribes speech with either local Whisper or Groq Cloud, and pastes the result into the active application.

## Features

- Global push-to-talk dictation from the system tray.
- Local Whisper transcription through the bundled `whisper.cpp` sidecar.
- Groq Cloud transcription with a locally stored API key and model id.
- Mixed Arabic and English speech mode for same-sentence dictation without translation.
- Configurable global hotkey with presets for `Ctrl + Alt + Space` and `Ctrl + Shift + Space`.
- Launch-at-startup support on Windows, enabled by default and controlled from Settings.
- Microphone, paste delay, audio device, and transcription engine settings.

## Requirements

- Windows 10 or later.
- Node.js and npm.
- Rust and Cargo.
- Tauri v2 prerequisites.
- A microphone.
- A Groq API key if you use Groq Cloud transcription.

## Development

Install dependencies:

```powershell
npm install
```

Run the frontend dev server:

```powershell
npm run dev
```

Run the Tauri app in development:

```powershell
npm run tauri dev
```

Build the frontend:

```powershell
npm run build
```

Build the desktop app:

```powershell
npm run tauri build
```

For a debug installer build:

```powershell
npm run tauri build -- --debug
```

## Settings

Rawi stores settings locally on the machine. The settings screen includes:

- General: microphone selection, launch-at-startup toggle, and paste delay.
- Engine: Local Whisper or Groq Cloud, speech language mode, Groq API key, Groq model id, and connection testing.
- Recording: hotkey presets, custom hotkey capture, and recording behavior.
- About: app information.

The default hotkey is `Ctrl + Shift + Space`. Modifier-only shortcuts such as only `Ctrl + Alt` or only `Ctrl + Shift` are not valid global shortcuts, so Rawi uses those modifier combinations with `Space`.

## Startup Behavior

Launch at startup is enabled by default. When Windows starts Rawi automatically, the app opens in the background and is available from the tray. You can turn this off in Settings under General.

## Project Structure

- `src/` contains the TypeScript UI logic.
- `src/style.css` contains the app styling.
- `index.html` contains the settings and app shell markup.
- `src-tauri/` contains the Rust backend, Tauri configuration, tray integration, recording pipeline, transcription logic, settings, and installer configuration.
- `src-tauri/binaries/` contains sidecar binaries used by the app.

## Verification

Useful local checks before shipping changes:

```powershell
npm run build
cd src-tauri
cargo test
```
