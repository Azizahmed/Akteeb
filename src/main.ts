import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface Settings {
  microphone: string;
  engine: string;
  whisperModel: string;
  transcriptionLanguage: string;
  groqApiKey: string;
  groqModel: string;
  recordingMode: string;
  hotkey: string;
  launchAtStartup: boolean;
}

interface MicDevice {
  name: string;
  is_default: boolean;
}

interface DownloadProgress {
  downloaded: number;
  total: number;
  percent: number;
}

const statusDot = document.getElementById("status-dot")!;
const statusText = document.getElementById("status-text")!;
const micSelect = document.getElementById("mic-select") as HTMLSelectElement;
const engineLocal = document.getElementById("engine-local")!;
const engineCloud = document.getElementById("engine-cloud")!;
const localSettings = document.getElementById("local-settings")!;
const cloudSettings = document.getElementById("cloud-settings")!;
const modelSelect = document.getElementById("model-select") as HTMLSelectElement;
const languageSelect = document.getElementById("language-select") as HTMLSelectElement;
const downloadBtn = document.getElementById("download-btn") as HTMLButtonElement;
const downloadProgress = document.getElementById("download-progress")!;
const progressFill = document.getElementById("progress-fill")!;
const groqKey = document.getElementById("groq-key") as HTMLInputElement;
const groqModel = document.getElementById("groq-model") as HTMLInputElement;
const groqTestBtn = document.getElementById("groq-test-btn") as HTMLButtonElement;
const groqTestStatus = document.getElementById("groq-test-status")!;
const startupToggle = document.getElementById("startup-toggle") as HTMLInputElement;
const startupStatus = document.getElementById("startup-status")!;
const modeToggle = document.getElementById("mode-toggle")!;
const modePtt = document.getElementById("mode-ptt")!;
const hotkeyText = document.getElementById("hotkey-text")!;
const hotkeyChangeBtn = document.getElementById("hotkey-change-btn") as HTMLButtonElement;
const hotkeyCapturePanel = document.getElementById("hotkey-capture-panel")!;
const hotkeyPreview = document.getElementById("hotkey-preview")!;
const hotkeyCaptureHint = document.getElementById("hotkey-capture-hint")!;
const hotkeyConfirmBtn = document.getElementById("hotkey-confirm-btn") as HTMLButtonElement;
const hotkeyCancelBtn = document.getElementById("hotkey-cancel-btn") as HTMLButtonElement;

const navItems = document.querySelectorAll(".nav-item");
const sections = document.querySelectorAll(".content-section");
const titlebar = document.getElementById("titlebar")!;
const titlebarMeter = document.getElementById("titlebar-meter")!;
const sidebar = document.getElementById("sidebar")!;
const shortcutPresetButtons = document.querySelectorAll<HTMLButtonElement>(".shortcut-preset");
const appWindow = getCurrentWindow();

let currentSettings: Settings;
let pendingHotkey: string | null = null;
let isCapturingHotkey = false;
const activeModifiers = new Set<string>();
let currentRecordingState = "Ready";

const modifierCodes = new Set([
  "ShiftLeft",
  "ShiftRight",
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "MetaLeft",
  "MetaRight",
]);

function formatHotkeyLabel(hotkey: string) {
  return hotkey
    .replace(/CommandOrControl|CommandOrCtrl|CmdOrCtrl/gi, navigator.userAgent.toLowerCase().includes("mac") ? "Cmd" : "Ctrl")
    .replace(/Command/gi, "Cmd")
    .replace(/Control/gi, "Ctrl")
    .replace(/Key([A-Z])/g, "$1")
    .replace(/Digit([0-9])/g, "$1")
    .replace(/Arrow/g, "")
    .replace(/\+/g, " + ");
}

function formatRecordingError(message: string) {
  const normalized = message.replace(/^Error:\s*/, "").trim();
  if (normalized.length <= 64) {
    return normalized;
  }

  return `${normalized.slice(0, 61)}...`;
}

function setStatus(state: string) {
  currentRecordingState = state;
  statusDot.className = "";
  statusText.classList.remove("status-error");
  statusText.removeAttribute("title");

  if (state === "Recording") {
    statusDot.classList.add("recording");
    statusText.textContent = "Recording...";
    titlebarMeter.classList.remove("idle", "transcribing");
    titlebarMeter.classList.add("recording");
    return;
  }

  if (state === "Transcribing") {
    statusDot.classList.add("transcribing");
    statusText.textContent = "Transcribing...";
    titlebarMeter.style.setProperty("--level", "0.08");
    titlebarMeter.classList.remove("idle", "recording", "speaking");
    titlebarMeter.classList.add("transcribing");
    return;
  }

  statusDot.classList.add("ready");
  statusText.textContent = "Ready";
  titlebarMeter.style.setProperty("--level", "0.06");
  titlebarMeter.classList.remove("recording", "transcribing", "speaking");
  titlebarMeter.classList.add("idle");
}

function showRecordingError(message: string) {
  currentRecordingState = "Ready";
  statusDot.className = "";
  statusDot.classList.add("error");
  statusText.classList.add("status-error");
  statusText.textContent = formatRecordingError(message);
  statusText.title = message;
  titlebarMeter.style.setProperty("--level", "0.06");
  titlebarMeter.classList.remove("recording", "transcribing", "speaking");
  titlebarMeter.classList.add("idle");
}

function setTitlebarMeterLevel(level: number) {
  const clampedLevel = Math.max(0, Math.min(1, level));
  const visualLevel =
    currentRecordingState === "Recording"
      ? Math.max(0.08, clampedLevel)
      : Math.max(0.06, clampedLevel * 0.35);

  titlebarMeter.style.setProperty("--level", visualLevel.toFixed(3));

  if (currentRecordingState !== "Recording") {
    titlebarMeter.classList.remove("speaking");
    return;
  }

  titlebarMeter.classList.toggle("speaking", clampedLevel > 0.12);
}

function setInlineStatus(element: HTMLElement, message: string, tone: "neutral" | "success" | "error" = "neutral") {
  element.textContent = message;
  element.classList.remove("status-success", "status-error");
  if (tone === "success") element.classList.add("status-success");
  if (tone === "error") element.classList.add("status-error");
}

function setEngine(engine: string) {
  currentSettings.engine = engine;
  engineLocal.classList.toggle("active", engine === "local");
  engineCloud.classList.toggle("active", engine === "cloud");
  localSettings.classList.toggle("hidden", engine !== "local");
  cloudSettings.classList.toggle("hidden", engine !== "cloud");
}

function setRecordingMode(mode: string) {
  currentSettings.recordingMode = mode;
  modeToggle.classList.toggle("active", mode === "toggle");
  modePtt.classList.toggle("active", mode === "push-to-talk");
}

function setLaunchAtStartup(enabled: boolean) {
  currentSettings.launchAtStartup = enabled;
  startupToggle.checked = enabled;
}

function normalizeLanguageMode(language: string) {
  if (!language || language === "auto") {
    return "mixed";
  }

  return language;
}

function setHotkeyPreview(hotkey: string | null, message?: string, tone: "neutral" | "success" | "error" = "neutral") {
  hotkeyPreview.textContent = hotkey ? formatHotkeyLabel(hotkey) : "No shortcut captured yet";
  hotkeyPreview.classList.toggle("hotkey-empty", !hotkey);
  setInlineStatus(hotkeyCaptureHint, message ?? "Press a modifier plus one key, then confirm.", tone);
}

function setHotkeyDisplay(hotkey: string) {
  hotkeyText.textContent = formatHotkeyLabel(hotkey);
  shortcutPresetButtons.forEach((button) => {
    button.classList.toggle("active", button.dataset.hotkey === hotkey);
  });
}

function beginHotkeyCapture() {
  pendingHotkey = null;
  isCapturingHotkey = true;
  activeModifiers.clear();
  hotkeyCapturePanel.classList.remove("hidden");
  hotkeyChangeBtn.classList.add("active");
  hotkeyConfirmBtn.disabled = true;
  hotkeyChangeBtn.blur();
  hotkeyCapturePanel.focus();
  setHotkeyPreview(null, "Listening for your shortcut...", "neutral");
}

function stopHotkeyCapture() {
  isCapturingHotkey = false;
  activeModifiers.clear();
  hotkeyChangeBtn.classList.remove("active");
}

function cancelHotkeyCapture() {
  pendingHotkey = null;
  stopHotkeyCapture();
  hotkeyCapturePanel.classList.add("hidden");
  setHotkeyPreview(null);
}

function mapKeyboardCode(code: string) {
  if (code.startsWith("Key")) return code;
  if (code.startsWith("Digit")) return code;
  if (/^F\d{1,2}$/.test(code)) return code;

  const supported: Record<string, string> = {
    Backquote: "Backquote",
    Backslash: "Backslash",
    BracketLeft: "BracketLeft",
    BracketRight: "BracketRight",
    Comma: "Comma",
    Period: "Period",
    Minus: "Minus",
    Equal: "Equal",
    Quote: "Quote",
    Semicolon: "Semicolon",
    Slash: "Slash",
    Space: "Space",
    Enter: "Enter",
    Tab: "Tab",
    Escape: "Escape",
    Backspace: "Backspace",
    Delete: "Delete",
    Insert: "Insert",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    ArrowUp: "ArrowUp",
    ArrowDown: "ArrowDown",
    ArrowLeft: "ArrowLeft",
    ArrowRight: "ArrowRight",
    CapsLock: "CapsLock",
    Pause: "Pause",
    PrintScreen: "PrintScreen",
    ScrollLock: "ScrollLock",
    NumLock: "NumLock",
    Numpad0: "Numpad0",
    Numpad1: "Numpad1",
    Numpad2: "Numpad2",
    Numpad3: "Numpad3",
    Numpad4: "Numpad4",
    Numpad5: "Numpad5",
    Numpad6: "Numpad6",
    Numpad7: "Numpad7",
    Numpad8: "Numpad8",
    Numpad9: "Numpad9",
    NumpadAdd: "NumpadAdd",
    NumpadSubtract: "NumpadSubtract",
    NumpadMultiply: "NumpadMultiply",
    NumpadDivide: "NumpadDivide",
    NumpadDecimal: "NumpadDecimal",
    NumpadEnter: "NumpadEnter",
  };

  return supported[code] ?? null;
}

function buildShortcutFromEvent(event: KeyboardEvent) {
  const keyToken = mapKeyboardCode(event.code);
  const modifiers: string[] = [];

  if (event.metaKey) modifiers.push("Command");
  if (event.ctrlKey) modifiers.push("Ctrl");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");

  if (!keyToken || modifierCodes.has(event.code)) return null;
  if (modifiers.length === 0) return null;

  return [...modifiers, keyToken].join("+");
}

function getModifierLabel(code: string) {
  switch (code) {
    case "MetaLeft":
    case "MetaRight":
      return "Command";
    case "ControlLeft":
    case "ControlRight":
      return "Ctrl";
    case "AltLeft":
    case "AltRight":
      return "Alt";
    case "ShiftLeft":
    case "ShiftRight":
      return "Shift";
    default:
      return null;
  }
}

async function checkModelStatus() {
  const downloaded = await invoke<boolean>("check_model_downloaded", {
    modelSize: modelSelect.value,
  });
  downloadBtn.textContent = downloaded ? "\u2713" : "Download";
  downloadBtn.disabled = downloaded;
}

async function saveSettings() {
  if (!currentSettings) return;
  currentSettings.microphone = micSelect.value;
  currentSettings.whisperModel = modelSelect.value;
  currentSettings.transcriptionLanguage = languageSelect.value;
  currentSettings.groqApiKey = groqKey.value.trim();
  currentSettings.groqModel = groqModel.value.trim();
  currentSettings.launchAtStartup = startupToggle.checked;
  await invoke("save_settings", { settings: currentSettings });
}

async function loadSettings() {
  currentSettings = await invoke<Settings>("get_settings");
  currentSettings.transcriptionLanguage = normalizeLanguageMode(currentSettings.transcriptionLanguage);

  const mics = await invoke<MicDevice[]>("list_microphones");
  micSelect.innerHTML = "";
  mics.forEach((mic) => {
    const option = document.createElement("option");
    option.value = mic.name;
    option.textContent = mic.name + (mic.is_default ? " (default)" : "");
    micSelect.appendChild(option);
  });

  const preferredMicExists = mics.some((mic) => mic.name === currentSettings.microphone);
  if (preferredMicExists) {
    micSelect.value = currentSettings.microphone;
  } else if (mics.length > 0) {
    const fallbackMic = mics.find((mic) => mic.is_default)?.name ?? mics[0].name;
    currentSettings.microphone = fallbackMic;
    micSelect.value = fallbackMic;
    await invoke("save_settings", { settings: currentSettings });
  }

  setEngine(currentSettings.engine);
  modelSelect.value = currentSettings.whisperModel;
  languageSelect.value = currentSettings.transcriptionLanguage;
  await checkModelStatus();
  groqKey.value = currentSettings.groqApiKey;
  groqModel.value = currentSettings.groqModel;
  setRecordingMode(currentSettings.recordingMode);
  setLaunchAtStartup(currentSettings.launchAtStartup);
  setHotkeyDisplay(currentSettings.hotkey);
  setHotkeyPreview(null);
  const recordingState = await invoke<string>("get_recording_state");
  setStatus(recordingState);
}

async function testGroqConnection() {
  groqTestBtn.disabled = true;
  currentSettings.groqApiKey = groqKey.value.trim();
  currentSettings.groqModel = groqModel.value.trim();
  setInlineStatus(groqTestStatus, "Checking Groq connection...");

  try {
    await saveSettings();
    const result = await invoke<string>("test_groq_connection", {
      apiKey: currentSettings.groqApiKey,
      model: currentSettings.groqModel,
    });
    setInlineStatus(groqTestStatus, result, "success");
  } catch (error) {
    setInlineStatus(groqTestStatus, String(error), "error");
  } finally {
    groqTestBtn.disabled = false;
  }
}

async function confirmHotkeyChange() {
  if (!pendingHotkey) return;

  const previousHotkey = currentSettings.hotkey;
  currentSettings.hotkey = pendingHotkey;
  hotkeyConfirmBtn.disabled = true;
  setInlineStatus(hotkeyCaptureHint, "Saving shortcut...");

  try {
    await saveSettings();
    setHotkeyDisplay(currentSettings.hotkey);
    stopHotkeyCapture();
    hotkeyCapturePanel.classList.add("hidden");
    setHotkeyPreview(null, "Shortcut updated successfully.", "success");
  } catch (error) {
    currentSettings.hotkey = previousHotkey;
    pendingHotkey = null;
    hotkeyConfirmBtn.disabled = true;
    setHotkeyPreview(null, String(error), "error");
  }
}

navItems.forEach((item) => {
  item.addEventListener("click", () => {
    const target = item.getAttribute("data-section");
    navItems.forEach((navItem) => navItem.classList.remove("active"));
    sections.forEach((section) => section.classList.remove("active"));
    item.classList.add("active");
    document.getElementById(`section-${target}`)?.classList.add("active");
  });
});

titlebar.addEventListener("mousedown", (event) => {
  if ((event.target as HTMLElement).closest("button, select, input, a, .nav-item")) return;
  appWindow.startDragging();
});

sidebar.addEventListener("mousedown", (event) => {
  if ((event.target as HTMLElement).closest("button, select, input, a, .nav-item")) return;
  appWindow.startDragging();
});

document.addEventListener("keydown", (event) => {
  if (!isCapturingHotkey) return;

  event.preventDefault();
  event.stopPropagation();

  if (event.key === "Escape") {
    cancelHotkeyCapture();
    return;
  }

  if (modifierCodes.has(event.code)) {
    const modifierLabel = getModifierLabel(event.code);
    if (modifierLabel) activeModifiers.add(modifierLabel);
    const preview = [...activeModifiers].join(" + ");
    hotkeyPreview.textContent = preview || "No shortcut captured yet";
    hotkeyPreview.classList.toggle("hotkey-empty", preview.length === 0);
    setInlineStatus(hotkeyCaptureHint, "Keep holding the modifier and press one more key.");
    return;
  }

  const shortcut = buildShortcutFromEvent(event);
  if (!shortcut) {
    setHotkeyPreview(null, "Use at least one modifier key with your shortcut.", "error");
    return;
  }

  pendingHotkey = shortcut;
  stopHotkeyCapture();
  hotkeyConfirmBtn.disabled = false;
  setHotkeyPreview(shortcut, "Shortcut captured. Click Confirm to apply it.", "success");
});

document.addEventListener("keyup", (event) => {
  if (!isCapturingHotkey) return;

  if (modifierCodes.has(event.code)) {
    const modifierLabel = getModifierLabel(event.code);
    if (modifierLabel) activeModifiers.delete(modifierLabel);

    if (!pendingHotkey) {
      const preview = [...activeModifiers].join(" + ");
      hotkeyPreview.textContent = preview || "No shortcut captured yet";
      hotkeyPreview.classList.toggle("hotkey-empty", preview.length === 0);
    }
  }
});

engineLocal.addEventListener("click", () => {
  setEngine("local");
  void saveSettings();
});

engineCloud.addEventListener("click", () => {
  setEngine("cloud");
  void saveSettings();
});

micSelect.addEventListener("change", () => {
  void saveSettings();
});

modelSelect.addEventListener("change", async () => {
  await checkModelStatus();
  await saveSettings();
});

languageSelect.addEventListener("change", () => {
  void saveSettings();
});

downloadBtn.addEventListener("click", async () => {
  downloadBtn.disabled = true;
  downloadProgress.classList.remove("hidden");
  progressFill.style.width = "0%";

  try {
    await invoke("download_model", { modelSize: modelSelect.value });
    downloadBtn.textContent = "\u2713";
  } catch (error) {
    downloadBtn.textContent = "Retry";
    downloadBtn.disabled = false;
    console.error("Download failed:", error);
  }

  downloadProgress.classList.add("hidden");
});

groqKey.addEventListener("change", () => {
  setInlineStatus(groqTestStatus, "Saved API key locally. Use Test Connection to verify it.");
  void saveSettings();
});

groqModel.addEventListener("change", () => {
  setInlineStatus(groqTestStatus, "Saved model id locally. Use Test Connection to verify it.");
  void saveSettings();
});

groqTestBtn.addEventListener("click", () => {
  void testGroqConnection();
});

modeToggle.addEventListener("click", () => {
  setRecordingMode("toggle");
  void saveSettings();
});

modePtt.addEventListener("click", () => {
  setRecordingMode("push-to-talk");
  void saveSettings();
});

startupToggle.addEventListener("change", async () => {
  const previousValue = currentSettings.launchAtStartup;
  setLaunchAtStartup(startupToggle.checked);
  setInlineStatus(startupStatus, startupToggle.checked ? "Startup enabled." : "Startup disabled.");

  try {
    await saveSettings();
  } catch (error) {
    setLaunchAtStartup(previousValue);
    setInlineStatus(startupStatus, String(error), "error");
  }
});

shortcutPresetButtons.forEach((button) => {
  button.addEventListener("click", async () => {
    const hotkey = button.dataset.hotkey;
    if (!hotkey || hotkey === currentSettings.hotkey) return;

    const previousHotkey = currentSettings.hotkey;
    currentSettings.hotkey = hotkey;
    setHotkeyDisplay(hotkey);
    setHotkeyPreview(null, "Saving shortcut...");

    try {
      await saveSettings();
      setHotkeyPreview(null, "Shortcut updated successfully.", "success");
    } catch (error) {
      currentSettings.hotkey = previousHotkey;
      setHotkeyDisplay(previousHotkey);
      setHotkeyPreview(null, String(error), "error");
    }
  });
});

hotkeyChangeBtn.addEventListener("click", () => {
  beginHotkeyCapture();
});

hotkeyConfirmBtn.addEventListener("click", () => {
  void confirmHotkeyChange();
});

hotkeyCancelBtn.addEventListener("click", () => {
  cancelHotkeyCapture();
});

listen<string>("recording-state", (event) => {
  setStatus(event.payload);
});

listen<string>("recording-error", (event) => {
  console.error("Recording error:", event.payload);
  showRecordingError(event.payload);
});

listen<number>("audio-level", (event) => {
  setTitlebarMeterLevel(event.payload);
});

listen<DownloadProgress>("download-progress", (event) => {
  const { percent } = event.payload;
  progressFill.style.width = `${percent}%`;
});

loadSettings().catch((error) => {
  console.error("Failed to initialize settings UI:", error);
  setStatus("Ready");
});
