#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod foreground;

use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use promptloom_asr::download::{download_model, verify_file};
use promptloom_asr::{find_model, load_all_manifest, load_manifest, WhisperTranscriber};
use promptloom_audio::{
    default_input_device_name, enumerate_input_devices, LevelMeter, MicProbe, WasapiAudioSource,
};
use promptloom_core::{
    state::ErrorKind, AudioBuffer, DeliveryOutcome, Notice, Orchestrator, OutputSink,
    PromptLoomError, State, TargetWindow, Transcriber,
};
use promptloom_commands::{apply as apply_commands, TranscriptMode};
use promptloom_llm_polish::{OllamaClient, OllamaError, OllamaModel};
use promptloom_core::TextTransformer;
use promptloom_output_windows::WindowsClipboardSink;
use promptloom_vocabulary::{Capitalizer, Polisher, Vocabulary};
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

/// Runtime LLM-polish config. Lives in a shared `Arc<Mutex>` between
/// AppState and `PolishedVocabulary`.
#[derive(Debug, Clone, Default)]
struct LlmConfig {
    enabled: bool,
    model: Option<String>,
}

/// Composite text transformer. Pipeline (order matters):
///   whisper
///   → **commands** (replace "comma" with ",", etc.)
///   → **polish** (strip fillers, collapse repeats, normalize whitespace)
///   → **capitalize** (only in Normal mode)
///   → **vocabulary** (user's project terms always win)
///
/// `TranscriptMode` gates which stages apply. Code/Terminal modes skip
/// sentence punctuation commands and skip capitalization entirely so you
/// can dictate verbatim code without silent damage.
struct PolishedVocabulary {
    mode: Arc<Mutex<TranscriptMode>>,
    polisher: Polisher,
    capitalizer: Capitalizer,
    vocab: Vocabulary,
    llm_config: Arc<Mutex<LlmConfig>>,
    ollama: OllamaClient,
    app: AppHandle,
}

impl TextTransformer for PolishedVocabulary {
    fn transform(&self, text: &str) -> String {
        let mode = *self.mode.lock();
        let after_commands = apply_commands(text, mode);
        let polished = self.polisher.polish(&after_commands);
        let after_caps = if mode.capitalize_sentences() {
            self.capitalizer.capitalize(&polished)
        } else {
            polished
        };
        let after_vocab = self.vocab.apply(&after_caps);

        // Optional Ollama cleanup. Hard timeout + raw fallback.
        let (enabled, model) = {
            let c = self.llm_config.lock();
            (c.enabled, c.model.clone())
        };
        if enabled {
            if let Some(model) = model {
                match self.ollama.clean(&model, &after_vocab) {
                    Ok(cleaned) if !cleaned.trim().is_empty() => return cleaned,
                    Ok(_) => {
                        tracing::warn!("ollama returned empty text; using raw");
                    }
                    Err(OllamaError::Timeout { ms }) => {
                        tracing::warn!(ms, "ollama timed out; using raw");
                        let _ = self
                            .app
                            .emit("promptloom:notice", "LLM polish timed out; used raw");
                    }
                    Err(e) => {
                        tracing::warn!(?e, "ollama polish failed; using raw");
                        let _ = self
                            .app
                            .emit("promptloom:notice", "LLM polish unavailable; used raw");
                    }
                }
            }
        }
        after_vocab
    }
}

type Orch =
    Orchestrator<WasapiAudioSource, ActiveTranscriber, PolishedVocabulary, ClipboardAndHudSink>;

/// Commands sent to the single-threaded orchestrator worker. All work that
/// mutates the orchestrator (press, release, cancel, vocab swap, transcriber
/// swap) goes through this channel so no UI call ever blocks on whisper.
enum WorkerCmd {
    HotkeyPressed(TargetWindow),
    HotkeyReleased,
    EscapePressed,
    ReloadVocabulary(Vocabulary),
    SwapTranscriber(ActiveTranscriber),
}

struct AppState {
    /// Send into the orchestrator worker. `try_send` so UI calls never block
    /// if the worker is mid-transcription.
    worker_tx: SyncSender<WorkerCmd>,
    /// Last observed State. Updated by the worker after every event so UI
    /// commands can answer "are we Idle?" without acquiring the orch lock.
    state_snapshot: Arc<Mutex<State>>,
    /// Current transcript mode. Shared with `PolishedVocabulary` so a
    /// mode change takes effect on the next dictation without touching
    /// the worker.
    transcript_mode: Arc<Mutex<TranscriptMode>>,
    /// Shared Ollama config: whether polish is enabled + which model to use.
    /// Changes take effect on the next dictation.
    llm_config: Arc<Mutex<LlmConfig>>,
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
    #[serde(default = "default_dictation_mode")]
    dictation_mode: String,
    #[serde(default = "default_active_model")]
    active_model: String,
    #[serde(default = "default_transcript_mode")]
    transcript_mode: String,
    #[serde(default)]
    ollama_enabled: bool,
    #[serde(default)]
    ollama_model: Option<String>,
}

fn default_transcript_mode() -> String {
    "normal".to_string()
}

const ALLOWED_TRANSCRIPT_MODES: &[&str] = &["normal", "code", "terminal"];

fn parse_transcript_mode(s: &str) -> TranscriptMode {
    match s {
        "code" => TranscriptMode::Code,
        "terminal" => TranscriptMode::Terminal,
        _ => TranscriptMode::Normal,
    }
}

fn default_active_model() -> String {
    "ggml-base.en".to_string()
}

fn default_hotkey() -> String {
    "F8".to_string()
}

fn default_dictation_mode() -> String {
    "hold".to_string()
}

/// The small safe set of hotkey options A1 allows. Keeps us well clear of
/// arbitrary shortcut capture, per the plan.
const ALLOWED_HOTKEYS: &[&str] = &["F8", "F9", "F10", "F2"];

/// Dictation modes offered in A3. Keep this set small on purpose.
const ALLOWED_MODES: &[&str] = &["hold", "toggle"];

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
    engine_installed: bool,
    engine_path: Option<String>,
}

#[derive(Serialize, Clone)]
struct DownloadProgress {
    bytes: u64,
    total: u64,
}

#[tauri::command]
fn current_state(state: tauri::State<'_, AppState>) -> String {
    format!("{:?}", *state.state_snapshot.lock())
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

#[derive(Serialize, Clone)]
struct OllamaStatus {
    reachable: bool,
    endpoint: String,
    models: Vec<OllamaModel>,
    enabled: bool,
    selected_model: Option<String>,
}

#[tauri::command]
fn check_ollama(state: tauri::State<'_, AppState>) -> OllamaStatus {
    let client = OllamaClient::default();
    let models = client.list_models().unwrap_or_default();
    let (enabled, selected_model) = {
        let c = state.llm_config.lock();
        (c.enabled, c.model.clone())
    };
    OllamaStatus {
        reachable: !models.is_empty(),
        endpoint: client.endpoint,
        models,
        enabled,
        selected_model,
    }
}

#[tauri::command]
fn set_ollama_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<Settings, String> {
    {
        let mut c = state.llm_config.lock();
        c.enabled = enabled;
    }
    let mut settings = state.settings.lock();
    settings.ollama_enabled = enabled;
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
fn set_ollama_model(
    state: tauri::State<'_, AppState>,
    model: Option<String>,
) -> Result<Settings, String> {
    let normalized = model.filter(|s| !s.trim().is_empty());
    {
        let mut c = state.llm_config.lock();
        c.model = normalized.clone();
    }
    let mut settings = state.settings.lock();
    settings.ollama_model = normalized;
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
fn set_transcript_mode(
    state: tauri::State<'_, AppState>,
    mode: String,
) -> Result<Settings, String> {
    if !ALLOWED_TRANSCRIPT_MODES.contains(&mode.as_str()) {
        return Err(format!(
            "mode {mode:?} not in allowed set {:?}",
            ALLOWED_TRANSCRIPT_MODES
        ));
    }
    *state.transcript_mode.lock() = parse_transcript_mode(&mode);
    let mut settings = state.settings.lock();
    settings.transcript_mode = mode;
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
fn set_dictation_mode(
    state: tauri::State<'_, AppState>,
    mode: String,
) -> Result<Settings, String> {
    if !ALLOWED_MODES.contains(&mode.as_str()) {
        return Err(format!(
            "mode {mode:?} not in allowed set {:?}",
            ALLOWED_MODES
        ));
    }
    // Reject if a session is in flight. Prevents dropping the user into a
    // half-and-half mode where Press/Release wiring disagrees with state.
    {
        let current = state.state_snapshot.lock().clone();
        if !matches!(current, State::Idle) {
            return Err("Cannot change dictation mode while a session is in progress.".into());
        }
    }
    let mut settings = state.settings.lock();
    settings.dictation_mode = mode;
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
    // If the active model vanished since last launch, send the user back to
    // the model step. ModelStatus answers "installed+verified" more precisely.
    let manifest = match find_model(&state.manifest_path, &s.active_model) {
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

    let _ = state.worker_tx.try_send(WorkerCmd::ReloadVocabulary(new_vocab));

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

fn resolve_engine_path(model_dir: &std::path::Path) -> Option<PathBuf> {
    if let Ok(env) = std::env::var("PROMPTLOOM_WHISPER_BIN") {
        let p = PathBuf::from(env);
        if p.exists() {
            return Some(p);
        }
    }
    for name in &["whisper-cli.exe", "main.exe", "whisper-cli", "main"] {
        let p = model_dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

#[tauri::command]
fn model_status(state: tauri::State<'_, AppState>) -> Result<ModelStatus, String> {
    // Backward-compatible: reports the currently selected model.
    let active = state.settings.lock().active_model.clone();
    let manifest = find_model(&state.manifest_path, &active).map_err(|e| e.to_string())?;
    let path = state.model_dir.join(format!("{}.bin", manifest.name));
    let installed = path.exists();
    let verified = installed && verify_file(&manifest, &path).is_ok();
    let engine_path = resolve_engine_path(&state.model_dir);
    let engine_installed = engine_path.is_some();
    Ok(ModelStatus {
        installed,
        path: path.to_string_lossy().into(),
        name: manifest.name,
        size_bytes: manifest.size_bytes,
        verified,
        engine_installed,
        engine_path: engine_path.map(|p| p.to_string_lossy().into()),
    })
}

#[derive(Serialize, Clone)]
struct ModelSummary {
    name: String,
    size_bytes: u64,
    note: Option<String>,
    installed: bool,
    verified: bool,
    is_active: bool,
}

#[tauri::command]
fn list_models(state: tauri::State<'_, AppState>) -> Result<Vec<ModelSummary>, String> {
    let active = state.settings.lock().active_model.clone();
    let manifests = load_all_manifest(&state.manifest_path).map_err(|e| e.to_string())?;
    Ok(manifests
        .into_iter()
        .map(|m| {
            let path = state.model_dir.join(format!("{}.bin", m.name));
            let installed = path.exists();
            let verified = installed && verify_file(&m, &path).is_ok();
            ModelSummary {
                is_active: m.name == active,
                installed,
                verified,
                name: m.name,
                size_bytes: m.size_bytes,
                note: m.note,
            }
        })
        .collect())
}

#[tauri::command]
fn set_active_model(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<Settings, String> {
    let manifest = find_model(&state.manifest_path, &name).map_err(|e| e.to_string())?;
    let path = state.model_dir.join(format!("{}.bin", manifest.name));

    // Persist the choice regardless of install state so the user can select
    // a not-yet-downloaded model and trigger the download next.
    {
        let mut settings = state.settings.lock();
        settings.active_model = manifest.name.clone();
        save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    }

    if path.exists() && verify_file(&manifest, &path).is_ok() {
        // Model is on disk and valid — reload the transcriber immediately
        // so the next dictation uses the newly selected weights.
        reload_transcriber(&app, &path);
    } else {
        // Not installed. Swap to Placeholder so the next dictation surfaces
        // a clear "model not installed" error rather than silently falling
        // back to whatever was previously loaded.
        let _ = state
            .worker_tx
            .try_send(WorkerCmd::SwapTranscriber(ActiveTranscriber::Placeholder));
        let _ = app.emit(
            "promptloom:notice",
            format!("selected {name}: not installed yet — click Download to fetch it"),
        );
    }

    Ok(state.settings.lock().clone())
}

#[derive(Serialize, Clone)]
struct VerifyReport {
    ok: bool,
    message: String,
}

#[tauri::command]
fn verify_model_cmd(state: tauri::State<'_, AppState>) -> Result<VerifyReport, String> {
    let manifest = load_manifest(&state.manifest_path).map_err(|e| e.to_string())?;
    let path = state.model_dir.join(format!("{}.bin", manifest.name));
    if !path.exists() {
        return Ok(VerifyReport {
            ok: false,
            message: "Model not installed.".into(),
        });
    }
    // Purely local: hashes bytes on disk. No network.
    match verify_file(&manifest, &path) {
        Ok(()) => Ok(VerifyReport {
            ok: true,
            message: "SHA-256 matches manifest. Model is intact.".into(),
        }),
        Err(e) => Ok(VerifyReport {
            ok: false,
            message: format!("Verification failed: {e}"),
        }),
    }
}

#[derive(Serialize, Clone)]
struct RemoveReport {
    removed: bool,
    path: String,
    message: String,
}

#[tauri::command]
fn remove_model_cmd(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    confirm: bool,
) -> Result<RemoveReport, String> {
    if !confirm {
        return Err("Confirmation required to remove the model.".into());
    }

    let manifest = load_manifest(&state.manifest_path).map_err(|e| e.to_string())?;
    let model_dir = &state.model_dir;
    let path = model_dir.join(format!("{}.bin", manifest.name));

    // Safety: both paths must be canonicalizable and the model file must be
    // strictly inside the managed model directory. Rejects any symlink or
    // walk-out attempt even though the UI never passes a path.
    let dir_canon = std::fs::canonicalize(model_dir).map_err(|e| {
        format!("Model directory not accessible: {e}")
    })?;
    if !path.exists() {
        return Ok(RemoveReport {
            removed: false,
            path: path.to_string_lossy().into(),
            message: "Model file not present; nothing to remove.".into(),
        });
    }
    let path_canon =
        std::fs::canonicalize(&path).map_err(|e| format!("Model path invalid: {e}"))?;
    if !path_canon.starts_with(&dir_canon) {
        return Err(format!(
            "Refused to remove: {path_canon:?} is not inside managed model directory {dir_canon:?}"
        ));
    }
    if path_canon
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("bin"))
        != Some(true)
    {
        return Err("Refused to remove: only .bin model files may be removed.".into());
    }

    // Release any lingering reference to the model BEFORE deleting the file
    // on Windows. The worker thread swaps and takes ownership of the drop.
    let _ = state
        .worker_tx
        .try_send(WorkerCmd::SwapTranscriber(ActiveTranscriber::Placeholder));
    tracing::info!("transcriber swap-to-placeholder dispatched for model removal");

    std::fs::remove_file(&path_canon).map_err(|e| format!("Delete failed: {e}"))?;

    // Also remove any lingering .part partial download, if present.
    let part = path.with_extension("part");
    let _ = std::fs::remove_file(&part);

    // Flip onboarding_complete back to false so the user is routed to the
    // Model setup step on next launch, matching the plan's "removing a model
    // returns the app to the model setup state."
    let mut settings = state.settings.lock();
    settings.onboarding_complete = false;
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    drop(settings);

    let _ = app.emit("promptloom:notice", "model removed");

    Ok(RemoveReport {
        removed: true,
        path: path.to_string_lossy().into(),
        message: "Model removed. Reinstall from the Model section when ready.".into(),
    })
}

#[tauri::command]
fn download_model_cmd(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let active = state.settings.lock().active_model.clone();
    let manifest = find_model(&state.manifest_path, &active).map_err(|e| e.to_string())?;
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
            let _ = app_state
                .worker_tx
                .try_send(WorkerCmd::SwapTranscriber(ActiveTranscriber::Whisper(w)));
            tracing::info!("whisper transcriber load dispatched to worker");
            let _ = app.emit("promptloom:notice", "model loaded");
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

/// User-visible phase that the UI renders. Shape kept flat so the TS side
/// can read `phase.kind` and `phase.detail` without deep pattern matching.
#[derive(Serialize, Clone)]
struct Phase {
    kind: &'static str,
    label: String,
    detail: String,
}

fn phase_for(state: &State, notices: &[Notice]) -> Phase {
    // Terminal notices take precedence over the mechanical state.
    if let Some(n) = notices.last() {
        match n {
            Notice::Delivered(DeliveryOutcome::Pasted) => {
                return Phase {
                    kind: "delivered",
                    label: "Delivered".into(),
                    detail: "Pasted into the focused window.".into(),
                }
            }
            Notice::Delivered(DeliveryOutcome::CopyOnly { reason }) => {
                return Phase {
                    kind: "copied_only",
                    label: "Copied only".into(),
                    detail: format!("{reason}. Text is on the clipboard."),
                }
            }
            Notice::Delivered(DeliveryOutcome::Failed) => {
                return Phase {
                    kind: "error",
                    label: "Could not deliver".into(),
                    detail: "Try again, or paste the clipboard with Ctrl+V.".into(),
                };
            }
            Notice::Cancelled | Notice::TooShort => {
                return Phase {
                    kind: "cancelled",
                    label: "Cancelled".into(),
                    detail: "Audio discarded. Nothing delivered.".into(),
                };
            }
            Notice::Error(kind) => return error_phase(*kind),
        }
    }
    match state {
        State::Idle => Phase {
            kind: "ready",
            label: "Ready".into(),
            detail: "Hold the hotkey to dictate.".into(),
        },
        State::Recording { .. } => Phase {
            kind: "recording",
            label: "Recording".into(),
            detail: "Listening — release to transcribe.".into(),
        },
        State::Finalizing { .. } | State::Transcribing { .. } => Phase {
            kind: "processing",
            label: "Processing locally".into(),
            detail: "Transcribing on this PC.".into(),
        },
        State::Correcting { .. } | State::Delivering { .. } => Phase {
            kind: "processing",
            label: "Delivering".into(),
            detail: "Pasting into the focused window.".into(),
        },
    }
}

fn error_phase(kind: ErrorKind) -> Phase {
    let (label, detail) = match kind {
        ErrorKind::NoMicrophone => (
            "No microphone",
            "Plug one in or check Windows Sound settings, then try again.",
        ),
        ErrorKind::ModelMissing => (
            "Model not installed",
            "Open Settings and reinstall the local speech model.",
        ),
        ErrorKind::TranscriptionFailed => (
            "Transcription failed",
            "Try again. Speak closer to the mic and give it 1–2 seconds.",
        ),
        ErrorKind::TargetUnavailable => (
            "No window to paste into",
            "Click back into the app you want to dictate into, then try again.",
        ),
        ErrorKind::DeliveryFailed => (
            "Could not paste",
            "The text is on your clipboard — paste it manually with Ctrl+V.",
        ),
    };
    Phase {
        kind: "error",
        label: label.into(),
        detail: detail.into(),
    }
}

fn emit_phase(app: &AppHandle, state: &State, notices: &[Notice]) {
    let phase = phase_for(state, notices);
    update_tray_tooltip(app, &phase);
    let _ = app.emit("promptloom:phase", phase);
}

/// Best-effort tray-tooltip update. Silent on platforms where tray isn't
/// managed (release builds without the tray-icon feature, headless CI).
fn update_tray_tooltip(app: &AppHandle, phase: &Phase) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(format!("PromptLoom — {}", phase.label)));
    }
}

/// Schedule the HUD to hide after `delay`. Used for terminal phases so the
/// user actually sees the confirmation before it disappears.
fn schedule_hide_hud(app: AppHandle, delay: Duration) {
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        hide_hud(&app);
    });
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
    mode: Arc<Mutex<TranscriptMode>>,
    llm_config: Arc<Mutex<LlmConfig>>,
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
    let text_transformer = PolishedVocabulary {
        mode,
        polisher: Polisher::default(),
        capitalizer: Capitalizer::default(),
        vocab,
        llm_config,
        ollama: OllamaClient::default(),
        app: app.clone(),
    };
    let orch = Orchestrator::new(audio, transcriber, text_transformer, sink);
    Ok((orch, level))
}

fn on_hotkey(app: &AppHandle, state: ShortcutState) {
    let app_state = app.state::<AppState>();
    let mode = app_state.settings.lock().dictation_mode.clone();
    let is_toggle = mode == "toggle";
    let current = app_state.state_snapshot.lock().clone();

    match (is_toggle, state) {
        // Hold-to-talk
        (false, ShortcutState::Pressed) => {
            let Some(target) = foreground::capture_foreground() else {
                tracing::warn!("no foreground window at hotkey press");
                return;
            };
            if matches!(current, State::Idle) {
                let _ = app_state
                    .worker_tx
                    .try_send(WorkerCmd::HotkeyPressed(target));
                show_hud(app);
            }
        }
        (false, ShortcutState::Released) => {
            if !matches!(current, State::Idle) {
                let _ = app_state.worker_tx.try_send(WorkerCmd::HotkeyReleased);
            }
        }

        // Toggle
        (true, ShortcutState::Pressed) => match current {
            State::Idle => {
                let Some(target) = foreground::capture_foreground() else {
                    tracing::warn!("no foreground window at hotkey press");
                    return;
                };
                let _ = app_state
                    .worker_tx
                    .try_send(WorkerCmd::HotkeyPressed(target));
                show_hud(app);
            }
            State::Recording { .. } => {
                let _ = app_state.worker_tx.try_send(WorkerCmd::HotkeyReleased);
            }
            _ => {
                tracing::debug!(?current, "toggle press ignored: session mid-processing");
            }
        },
        (true, ShortcutState::Released) => { /* ignored in toggle mode */ }
    }
}

fn on_escape(app: &AppHandle) {
    let app_state = app.state::<AppState>();
    let current = app_state.state_snapshot.lock().clone();
    if !matches!(current, State::Idle) {
        let _ = app_state.worker_tx.try_send(WorkerCmd::EscapePressed);
    }
}

/// The orchestrator worker. Owns `Orch` exclusively; receives commands via
/// an mpsc channel. All blocking work (whisper subprocess, clipboard) runs
/// here, off the Tauri command threads, so the UI stays responsive.
fn spawn_orchestrator_worker(
    mut orch: Orch,
    rx: std::sync::mpsc::Receiver<WorkerCmd>,
    app: AppHandle,
    state_snapshot: Arc<Mutex<State>>,
) {
    std::thread::Builder::new()
        .name("promptloom-orch".into())
        .spawn(move || {
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    WorkerCmd::HotkeyPressed(target) => orch.press(target),
                    WorkerCmd::HotkeyReleased => {
                        // Flash "Processing locally" before whisper blocks.
                        let processing = Phase {
                            kind: "processing",
                            label: "Processing locally".into(),
                            detail: "Transcribing on this PC.".into(),
                        };
                        update_tray_tooltip(&app, &processing);
                        let _ = app.emit("promptloom:phase", processing);
                        orch.release();
                    }
                    WorkerCmd::EscapePressed => orch.cancel(),
                    WorkerCmd::ReloadVocabulary(v) => {
                        let (_, _, text_slot, _) = orch.components();
                        text_slot.vocab = v;
                        tracing::info!("vocabulary swapped in by worker");
                        continue; // No state change; skip emit path.
                    }
                    WorkerCmd::SwapTranscriber(t) => {
                        let (_, trans_slot, _, _) = orch.components();
                        *trans_slot = t;
                        tracing::info!("transcriber swapped in by worker");
                        continue;
                    }
                }

                let notices = orch.take_notices();
                let final_state = orch.state().clone();
                *state_snapshot.lock() = final_state.clone();

                drain_notices(&app, notices.clone());
                emit_phase(&app, &final_state, &notices);
                let _ = app.emit("promptloom:state", format!("{:?}", final_state));

                if has_terminal_notice(&notices) {
                    let dwell = terminal_dwell(&notices);
                    schedule_hide_hud(app.clone(), dwell);
                }
            }
        })
        .expect("failed to spawn orchestrator worker");
}

fn has_terminal_notice(notices: &[Notice]) -> bool {
    notices
        .iter()
        .any(|n| matches!(n, Notice::Delivered(_) | Notice::Cancelled | Notice::TooShort | Notice::Error(_)))
}

fn terminal_dwell(notices: &[Notice]) -> Duration {
    for n in notices {
        match n {
            Notice::Delivered(DeliveryOutcome::Pasted) => return Duration::from_millis(900),
            Notice::Delivered(DeliveryOutcome::CopyOnly { .. }) => {
                return Duration::from_millis(1600);
            }
            Notice::Error(_) | Notice::Delivered(DeliveryOutcome::Failed) => {
                return Duration::from_millis(2400);
            }
            Notice::Cancelled | Notice::TooShort => return Duration::from_millis(600),
        }
    }
    Duration::from_millis(900)
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

fn resolve_model_path(
    model_dir: &std::path::Path,
    manifest_path: &std::path::Path,
    active_name: &str,
) -> Option<PathBuf> {
    if let Ok(env_path) = std::env::var("PROMPTLOOM_MODEL_PATH") {
        let p = PathBuf::from(env_path);
        if p.exists() {
            return Some(p);
        }
    }
    // Prefer the user's selected model; fall back to the manifest default.
    let path = model_dir.join(format!("{active_name}.bin"));
    if path.exists() {
        return Some(path);
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
            set_dictation_mode,
            mark_onboarding_complete,
            needs_onboarding,
            verify_model_cmd,
            remove_model_cmd,
            list_models,
            set_active_model,
            set_transcript_mode,
            check_ollama,
            set_ollama_enabled,
            set_ollama_model,
        ])
        .setup(|app| {
            let (model_dir, manifest_path) = resolve_paths(app.handle());
            let settings_path = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("PromptLoom"))
                .join("settings.json");
            let mut settings = load_settings(&settings_path);

            let model_path = resolve_model_path(&model_dir, &manifest_path, &settings.active_model);

            let vocab = settings
                .project_path
                .as_deref()
                .and_then(vocabulary_from_project)
                .unwrap_or_else(Vocabulary::empty);
            settings.terms_count = vocab.len();

            let mic_device = settings.mic_device.clone();
            let transcript_mode = Arc::new(Mutex::new(parse_transcript_mode(
                &settings.transcript_mode,
            )));
            let llm_config = Arc::new(Mutex::new(LlmConfig {
                enabled: settings.ollama_enabled,
                model: settings.ollama_model.clone(),
            }));
            let (orch_opt, level) = match init_orchestrator(
                app.handle(),
                model_path.as_deref(),
                vocab,
                mic_device.as_deref(),
                transcript_mode.clone(),
                llm_config.clone(),
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

            // Build the worker channel. If audio is unavailable the worker
            // never spawns, and command sends silently drop at the receiver's
            // disconnect — safe degraded mode.
            let (worker_tx, worker_rx) = sync_channel::<WorkerCmd>(32);
            let state_snapshot = Arc::new(Mutex::new(State::Idle));
            if let Some(orch) = orch_opt {
                spawn_orchestrator_worker(
                    orch,
                    worker_rx,
                    app.handle().clone(),
                    state_snapshot.clone(),
                );
            }

            app.manage(AppState {
                worker_tx,
                state_snapshot,
                transcript_mode,
                llm_config,
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

            // Initial status so the UI shows "Ready" immediately.
            emit_phase(app.handle(), &State::Idle, &[]);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

