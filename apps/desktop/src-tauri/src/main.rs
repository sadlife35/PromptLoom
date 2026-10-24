#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod foreground;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use promptloom_asr::download::{download_model, verify_file};
use promptloom_asr::{load_manifest, WhisperTranscriber};
use promptloom_audio::{
    default_input_device_name, enumerate_input_devices, LevelMeter, MicProbe, WasapiAudioSource,
};
use promptloom_core::{
    AudioBuffer, DeliveryOutcome, Notice, Orchestrator, OutputSink, PromptLoomError, State,
    TargetWindow, Transcriber,
};
use promptloom_output_windows::WindowsClipboardSink;
use promptloom_vocabulary::Vocabulary;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};

/// Active transcriber. Starts as Placeholder (surfaces ModelUnavailable so
/// the user can see the notice); after `download_model` or boot-time load,
/// swaps to Whisper in place without rebuilding the orchestrator.
enum ActiveTranscriber {
    Whisper(WhisperTranscriber),
    Placeholder,
}

impl Transcriber for ActiveTranscriber {
    fn transcribe(&mut self, audio: &AudioBuffer) -> Result<String, PromptLoomError> {
        match self {
            Self::Whisper(w) => w.transcribe(audio),
            Self::Placeholder => Err(PromptLoomError::ModelUnavailable),
        }
    }
}

/// M3 sink: real Windows clipboard transaction + Ctrl+V, plus an echo to the
/// HUD so the user still sees what was dictated.
struct ClipboardAndHudSink {
    inner: WindowsClipboardSink,
    app: AppHandle,
}

impl OutputSink for ClipboardAndHudSink {
    fn deliver(&mut self, text: &str, target: &TargetWindow) -> DeliveryOutcome {
        tracing::info!(
            exe = target.executable.as_str(),
            pid = target.process_id,
            length = text.len(),
            "delivering transcript via clipboard + Ctrl+V"
        );
        let _ = self.app.emit("promptloom:transcript", text);
        self.inner.deliver(text, target)
    }
}

type Orch =
    Orchestrator<WasapiAudioSource, ActiveTranscriber, Vocabulary, ClipboardAndHudSink>;

struct AppState {
    orch: Mutex<Option<Orch>>,
    level: LevelMeter,
    model_dir: PathBuf,
    manifest_path: PathBuf,
    settings_path: PathBuf,
    settings: Mutex<Settings>,
    scratch_dir: PathBuf,
    mic_probe: Mutex<Option<MicProbe>>,
    probe_level: Mutex<Option<LevelMeter>>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct Settings {
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default)]
    terms_count: usize,
    #[serde(default)]
    onboarding_complete: bool,
    #[serde(default = "default_hotkey")]
    hotkey: String,
    #[serde(default)]
    mic_device: Option<String>,
}

fn default_hotkey() -> String {
    "F8".to_string()
}

/// The small safe set of hotkey options A1 allows. Keeps us well clear of
/// arbitrary shortcut capture, per the plan.
const ALLOWED_HOTKEYS: &[&str] = &["F8", "F9", "F10", "F2"];

fn hotkey_code(name: &str) -> Code {
    match name {
        "F9" => Code::F9,
        "F10" => Code::F10,
        "F2" => Code::F2,
        _ => Code::F8,
    }
}

fn load_settings(path: &std::path::Path) -> Settings {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
        .unwrap_or_default()
}

fn save_settings(path: &std::path::Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, bytes)
}

fn vocabulary_from_project(project_path: &str) -> Option<Vocabulary> {
    let terms_path = std::path::Path::new(project_path)
        .join(".promptloom")
        .join("terms.yml");
    match Vocabulary::load(&terms_path) {
        Ok(v) => {
            tracing::info!(path = %terms_path.display(), terms = v.len(), "loaded vocabulary");
            Some(v)
        }
        Err(e) => {
            tracing::warn!(path = %terms_path.display(), ?e, "vocabulary load failed");
            None
        }
    }
}

#[derive(Serialize, Clone)]
struct HudPayload {
    state: String,
    level: f32,
}

#[derive(Serialize, Clone)]
struct ModelStatus {
    installed: bool,
    path: String,
    name: String,
    size_bytes: u64,
    verified: bool,
}

#[derive(Serialize, Clone)]
struct DownloadProgress {
    bytes: u64,
    total: u64,
}

#[tauri::command]
fn current_state(state: tauri::State<'_, AppState>) -> String {
    state
        .orch
        .lock()
        .as_ref()
        .map(|o| format!("{:?}", o.state()))
        .unwrap_or_else(|| "Uninitialized".into())
}

#[tauri::command]
fn hud_level(state: tauri::State<'_, AppState>) -> f32 {
    state.level.get()
}

#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> Settings {
    state.settings.lock().clone()
}

#[derive(Serialize, Clone)]
struct MicListing {
    devices: Vec<String>,
    default: Option<String>,
    selected: Option<String>,
}

#[tauri::command]
fn list_microphones(state: tauri::State<'_, AppState>) -> MicListing {
    MicListing {
        devices: enumerate_input_devices(),
        default: default_input_device_name(),
        selected: state.settings.lock().mic_device.clone(),
    }
}

#[tauri::command]
fn set_mic_device(
    state: tauri::State<'_, AppState>,
    device: Option<String>,
) -> Result<Settings, String> {
    let mut settings = state.settings.lock();
    settings.mic_device = device.filter(|s| !s.trim().is_empty());
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
fn start_mic_probe(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let selected = state.settings.lock().mic_device.clone();
    let probe = MicProbe::start(selected.as_deref()).map_err(|e| e.to_string())?;
    *state.probe_level.lock() = Some(probe.level_meter());
    *state.mic_probe.lock() = Some(probe);
    Ok(())
}

#[tauri::command]
fn stop_mic_probe(state: tauri::State<'_, AppState>) {
    *state.mic_probe.lock() = None;
    *state.probe_level.lock() = None;
}

#[tauri::command]
fn mic_probe_level(state: tauri::State<'_, AppState>) -> f32 {
    state
        .probe_level
        .lock()
        .as_ref()
        .map(|l| l.get())
        .unwrap_or(0.0)
}

#[tauri::command]
fn allowed_hotkeys() -> Vec<String> {
    ALLOWED_HOTKEYS.iter().map(|s| (*s).to_string()).collect()
}

#[tauri::command]
fn set_hotkey(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    hotkey: String,
) -> Result<Settings, String> {
    if !ALLOWED_HOTKEYS.contains(&hotkey.as_str()) {
        return Err(format!(
            "hotkey {hotkey:?} not in allowed set {:?}",
            ALLOWED_HOTKEYS
        ));
    }

    let shortcuts = app.global_shortcut();
    // Unregister the old one, register the new one.
    let old = {
        let s = state.settings.lock();
        s.hotkey.clone()
    };
    let _ = shortcuts.unregister(Shortcut::new(None, hotkey_code(&old)));
    shortcuts
        .register(Shortcut::new(None, hotkey_code(&hotkey)))
        .map_err(|e| format!("register failed: {e}"))?;

    let mut settings = state.settings.lock();
    settings.hotkey = hotkey;
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
fn mark_onboarding_complete(state: tauri::State<'_, AppState>) -> Result<Settings, String> {
    let mut settings = state.settings.lock();
    settings.onboarding_complete = true;
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    Ok(settings.clone())
}

/// Does the stored setup still look valid? Used by the first-run guard.
#[tauri::command]
fn needs_onboarding(state: tauri::State<'_, AppState>) -> bool {
    let s = state.settings.lock();
    if !s.onboarding_complete {
        return true;
    }
    // If the model vanished since last launch, send the user back to the
    // model step. ModelStatus answers "installed+verified" more precisely.
    let manifest = match load_manifest(&state.manifest_path) {
        Ok(m) => m,
        Err(_) => return true,
    };
    let model_path = state.model_dir.join(format!("{}.bin", manifest.name));
    if !model_path.exists() {
        return true;
    }
    verify_file(&manifest, &model_path).is_err()
}

#[tauri::command]
fn set_project_path(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    path: Option<String>,
) -> Result<Settings, String> {
    let new_vocab = match &path {
        Some(p) if !p.trim().is_empty() => {
            vocabulary_from_project(p).unwrap_or_else(Vocabulary::empty)
        }
        _ => Vocabulary::empty(),
    };
    let terms_count = new_vocab.len();

    {
        let mut guard = state.orch.lock();
        if let Some(orch) = guard.as_mut() {
            let (_, _, v, _) = orch.components();
            *v = new_vocab;
        }
    }

    let mut settings = state.settings.lock();
    settings.project_path = path.filter(|p| !p.trim().is_empty());
    settings.terms_count = terms_count;
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    let snapshot = settings.clone();
    drop(settings);

    let _ = app.emit(
        "promptloom:notice",
        format!("vocabulary reloaded: {terms_count} terms"),
    );
    Ok(snapshot)
}

#[derive(Serialize, Clone)]
struct PrivacyReport {
    scratch_dir: String,
    residual_wavs: Vec<String>,
    residual_logs: Vec<String>,
}

#[tauri::command]
fn audit_privacy(state: tauri::State<'_, AppState>) -> PrivacyReport {
    let dir = &state.scratch_dir;
    let mut wavs = Vec::new();
    let mut logs = Vec::new();
    if let Ok(iter) = std::fs::read_dir(dir) {
        for entry in iter.flatten() {
            let path = entry.path();
            match path.extension().and_then(|s| s.to_str()) {
                Some("wav") | Some("pcm") => {
                    wavs.push(path.to_string_lossy().into_owned());
                }
                Some("log") | Some("txt") => {
                    logs.push(path.to_string_lossy().into_owned());
                }
                _ => {}
            }
        }
    }
    PrivacyReport {
        scratch_dir: dir.to_string_lossy().into_owned(),
        residual_wavs: wavs,
        residual_logs: logs,
    }
}

#[tauri::command]
fn model_status(state: tauri::State<'_, AppState>) -> Result<ModelStatus, String> {
    let manifest = load_manifest(&state.manifest_path).map_err(|e| e.to_string())?;
    let path = state.model_dir.join(format!("{}.bin", manifest.name));
    let installed = path.exists();
    let verified = installed && verify_file(&manifest, &path).is_ok();
    Ok(ModelStatus {
        installed,
        path: path.to_string_lossy().into(),
        name: manifest.name,
        size_bytes: manifest.size_bytes,
        verified,
    })
}

#[tauri::command]
fn download_model_cmd(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let manifest = load_manifest(&state.manifest_path).map_err(|e| e.to_string())?;
    let dest = state.model_dir.join(format!("{}.bin", manifest.name));
    std::fs::create_dir_all(&state.model_dir).map_err(|e| e.to_string())?;

    let app_for_thread = app.clone();
    let state_arc = Arc::new((manifest, dest));
    let state_for_thread = state_arc.clone();

    // Run the download on a worker thread; emit progress events as it goes.
    std::thread::spawn(move || {
        let (manifest, dest) = (&state_for_thread.0, &state_for_thread.1);
        let app = app_for_thread;
        let result = download_model(manifest, dest, |bytes, total| {
            let _ = app.emit("promptloom:download", DownloadProgress { bytes, total });
        });
        match result {
            Ok(report) => {
                tracing::info!(bytes = report.bytes, "model downloaded");
                let _ = app.emit("promptloom:download-done", true);
                reload_transcriber(&app, &report.path);
            }
            Err(e) => {
                tracing::warn!(?e, "model download failed");
                let _ = app.emit(
                    "promptloom:notice",
                    format!("download failed: {e}"),
                );
                let _ = app.emit("promptloom:download-done", false);
            }
        }
    });
    Ok(())
}

fn reload_transcriber(app: &AppHandle, model_path: &std::path::Path) {
    match WhisperTranscriber::load(model_path) {
        Ok(w) => {
            let app_state = app.state::<AppState>();
            let mut guard = app_state.orch.lock();
            if let Some(orch) = guard.as_mut() {
                let (_, t, _, _) = orch.components();
                *t = ActiveTranscriber::Whisper(w);
                tracing::info!("whisper transcriber loaded and swapped in");
                let _ = app.emit("promptloom:notice", "model loaded");
            }
        }
        Err(e) => {
            tracing::warn!(?e, "whisper load failed");
            let _ = app.emit("promptloom:notice", format!("model load failed: {e}"));
        }
    }
}

fn drain_notices(app: &AppHandle, notices: Vec<Notice>) {
    for n in notices {
        let _ = app.emit("promptloom:notice", format!("{n:?}"));
    }
}

fn show_hud(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("hud") {
        let _ = w.show();
    }
}

fn hide_hud(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("hud") {
        let _ = w.hide();
    }
}

fn start_level_pump(app: AppHandle, level: LevelMeter) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(50));
        let payload = HudPayload {
            state: String::new(),
            level: level.get(),
        };
        if app.emit("promptloom:level", payload).is_err() {
            break;
        }
    });
}

fn init_orchestrator(
    app: &AppHandle,
    model_path: Option<&std::path::Path>,
    vocab: Vocabulary,
    mic_device: Option<&str>,
) -> Result<(Orch, LevelMeter), PromptLoomError> {
    let audio = WasapiAudioSource::new_with_device(mic_device)?;
    let level = audio.level_meter();
    let transcriber = match model_path {
        Some(p) if p.exists() => match WhisperTranscriber::load(p) {
            Ok(w) => {
                tracing::info!(path = %p.display(), "whisper model loaded at startup");
                ActiveTranscriber::Whisper(w)
            }
            Err(e) => {
                tracing::warn!(?e, "whisper load failed at startup; using placeholder");
                ActiveTranscriber::Placeholder
            }
        },
        _ => {
            tracing::info!("no model installed; placeholder transcriber active");
            ActiveTranscriber::Placeholder
        }
    };
    let sink = ClipboardAndHudSink {
        inner: WindowsClipboardSink::default(),
        app: app.clone(),
    };
    let orch = Orchestrator::new(audio, transcriber, vocab, sink);
    Ok((orch, level))
}

fn on_hotkey(app: &AppHandle, state: ShortcutState) {
    let app_state = app.state::<AppState>();
    let mut guard = app_state.orch.lock();
    let Some(orch) = guard.as_mut() else {
        return;
    };

    match state {
        ShortcutState::Pressed => {
            let Some(target) = foreground::capture_foreground() else {
                tracing::warn!("no foreground window at hotkey press");
                return;
            };
            if matches!(orch.state(), State::Idle) {
                orch.press(target);
                show_hud(app);
            }
        }
        ShortcutState::Released => {
            if !matches!(orch.state(), State::Idle) {
                orch.release();
            }
            hide_hud(app);
        }
    }

    drain_notices(app, orch.take_notices());
    let _ = app.emit("promptloom:state", format!("{:?}", orch.state()));
}

fn on_escape(app: &AppHandle) {
    let app_state = app.state::<AppState>();
    let mut guard = app_state.orch.lock();
    if let Some(orch) = guard.as_mut() {
        if !matches!(orch.state(), State::Idle) {
            orch.cancel();
            hide_hud(app);
            drain_notices(app, orch.take_notices());
        }
    }
}

fn resolve_paths(app: &AppHandle) -> (PathBuf, PathBuf) {
    let data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("PromptLoom"));
    let model_dir = data_dir.join("models");
    // Prefer the repo-local manifest in dev; fall back to resource dir in release.
    let manifest_path = std::env::current_dir()
        .ok()
        .and_then(|cwd| {
            // apps/desktop/src-tauri -> repo root
            let candidate = cwd.join("../../../models/manifest.json");
            if candidate.exists() {
                Some(candidate)
            } else {
                None
            }
        })
        .unwrap_or_else(|| data_dir.join("manifest.json"));
    (model_dir, manifest_path)
}

fn resolve_model_path(model_dir: &std::path::Path, manifest_path: &std::path::Path) -> Option<PathBuf> {
    if let Ok(env_path) = std::env::var("PROMPTLOOM_MODEL_PATH") {
        let p = PathBuf::from(env_path);
        if p.exists() {
            return Some(p);
        }
    }
    load_manifest(manifest_path)
        .ok()
        .map(|m| model_dir.join(format!("{}.bin", m.name)))
        .filter(|p| p.exists())
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "promptloom=info,warn".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    // Resolve the configured hotkey at every event so a live
                    // change via `set_hotkey` takes effect immediately.
                    let app_state = app.state::<AppState>();
                    let configured = app_state.settings.lock().hotkey.clone();
                    let hotkey = Shortcut::new(None, hotkey_code(&configured));
                    let escape = Shortcut::new(None, Code::Escape);
                    if shortcut == &hotkey {
                        on_hotkey(app, event.state());
                    } else if shortcut == &escape && event.state() == ShortcutState::Pressed {
                        on_escape(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            current_state,
            hud_level,
            model_status,
            download_model_cmd,
            get_settings,
            set_project_path,
            audit_privacy,
            list_microphones,
            set_mic_device,
            start_mic_probe,
            stop_mic_probe,
            mic_probe_level,
            allowed_hotkeys,
            set_hotkey,
            mark_onboarding_complete,
            needs_onboarding,
        ])
        .setup(|app| {
            let (model_dir, manifest_path) = resolve_paths(app.handle());
            let settings_path = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("PromptLoom"))
                .join("settings.json");
            let mut settings = load_settings(&settings_path);

            let model_path = resolve_model_path(&model_dir, &manifest_path);

            let vocab = settings
                .project_path
                .as_deref()
                .and_then(vocabulary_from_project)
                .unwrap_or_else(Vocabulary::empty);
            settings.terms_count = vocab.len();

            let mic_device = settings.mic_device.clone();
            let (orch, level) = match init_orchestrator(
                app.handle(),
                model_path.as_deref(),
                vocab,
                mic_device.as_deref(),
            ) {
                Ok(ol) => (Some(ol.0), Some(ol.1)),
                Err(e) => {
                    tracing::warn!(?e, "audio unavailable; app runs in degraded mode");
                    (None, None)
                }
            };
            let level = level.unwrap_or_default();

            let scratch_dir = std::env::temp_dir().join("promptloom-scratch");
            let _ = save_settings(&settings_path, &settings);

            let configured_hotkey = settings.hotkey.clone();
            app.manage(AppState {
                orch: Mutex::new(orch),
                level: level.clone(),
                model_dir,
                manifest_path,
                settings_path,
                settings: Mutex::new(settings),
                scratch_dir,
                mic_probe: Mutex::new(None),
                probe_level: Mutex::new(None),
            });

            let shortcuts = app.global_shortcut();
            let hotkey = Shortcut::new(None, hotkey_code(&configured_hotkey));
            let escape = Shortcut::new(None, Code::Escape);
            if let Err(e) = shortcuts.register(hotkey) {
                tracing::warn!(?e, "failed to register hold-to-talk");
            }
            if let Err(e) = shortcuts.register(escape) {
                tracing::warn!(?e, "failed to register escape");
            }

            start_level_pump(app.handle().clone(), level);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

