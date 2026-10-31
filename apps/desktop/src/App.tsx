import type React from "react";
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Onboarding } from "./Onboarding";

type ModelStatus = {
  installed: boolean;
  path: string;
  name: string;
  size_bytes: number;
  verified: boolean;
  engine_installed: boolean;
  engine_path: string | null;
};

type VerifyReport = { ok: boolean; message: string };
type RemoveReport = { removed: boolean; path: string; message: string };

type Settings = {
  project_path: string | null;
  terms_count: number;
  onboarding_complete: boolean;
  hotkey: string;
  mic_device: string | null;
  dictation_mode: "hold" | "toggle";
  active_model: string;
  transcript_mode: "normal" | "code" | "terminal";
  ollama_enabled: boolean;
  ollama_model: string | null;
};

type OllamaStatus = {
  reachable: boolean;
  endpoint: string;
  models: { name: string; size: number }[];
  enabled: boolean;
  selected_model: string | null;
};

type ModelSummary = {
  name: string;
  size_bytes: number;
  note: string | null;
  installed: boolean;
  verified: boolean;
  is_active: boolean;
};

type PrivacyReport = {
  scratch_dir: string;
  residual_wavs: string[];
  residual_logs: string[];
};

type Phase = {
  kind:
    | "ready"
    | "recording"
    | "processing"
    | "delivered"
    | "copied_only"
    | "cancelled"
    | "error";
  label: string;
  detail: string;
};

type HistEntry = {
  id: number;
  text: string;
  at: Date;
};

const PHASE_COLOR: Record<Phase["kind"], string> = {
  ready: "#8A92A3",
  recording: "#E57373",
  processing: "#F4C430",
  delivered: "#4ADE80",
  copied_only: "#60A5FA",
  cancelled: "#8A92A3",
  error: "#E57373",
};

type Section =
  | "dictation"
  | "notetaker"
  | "insights"
  | "dictionary"
  | "snippets"
  | "style"
  | "transforms"
  | "scratchpad"
  | "settings";

function formatTime(d: Date): string {
  const h = d.getHours();
  const m = d.getMinutes().toString().padStart(2, "0");
  const ap = h >= 12 ? "pm" : "am";
  const hh = h % 12 === 0 ? 12 : h % 12;
  return `${hh}:${m}${ap}`;
}

function greetingLabel(): string {
  const h = new Date().getHours();
  if (h < 5) return "Still up?";
  if (h < 12) return "Good morning";
  if (h < 17) return "Hey there";
  if (h < 21) return "Good evening";
  return "Late night";
}

export function App() {
  const [onboardingNeeded, setOnboardingNeeded] = useState<boolean | null>(null);
  const [section, setSection] = useState<Section>("dictation");
  const [phase, setPhase] = useState<Phase>({
    kind: "ready",
    label: "Loading…",
    detail: "",
  });
  const [notices, setNotices] = useState<string[]>([]);
  const [model, setModel] = useState<ModelStatus | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [progress, setProgress] = useState<{ bytes: number; total: number } | null>(
    null
  );
  const [downloading, setDownloading] = useState(false);
  const [history, setHistory] = useState<HistEntry[]>([]);
  const [privacy, setPrivacy] = useState<PrivacyReport | null>(null);
  const [verifyResult, setVerifyResult] = useState<VerifyReport | null>(null);
  const [removeConfirm, setRemoveConfirm] = useState(false);
  const [removeInFlight, setRemoveInFlight] = useState(false);
  const [availableModels, setAvailableModels] = useState<ModelSummary[]>([]);
  const [ollama, setOllama] = useState<OllamaStatus | null>(null);

  const refreshModel = () => {
    invoke<ModelStatus>("model_status")
      .then(setModel)
      .catch((e) => setNotices((n) => [`model_status error: ${e}`, ...n]));
    invoke<ModelSummary[]>("list_models")
      .then(setAvailableModels)
      .catch(() => void 0);
  };

  const refreshSettings = () => {
    invoke<Settings>("get_settings").then(setSettings).catch(() => void 0);
  };
  const refreshOllama = () => {
    invoke<OllamaStatus>("check_ollama").then(setOllama).catch(() => void 0);
  };

  const pickProject = async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (!picked || typeof picked !== "string") return;
    try {
      const next = await invoke<Settings>("set_project_path", { path: picked });
      setSettings(next);
    } catch (e) {
      setNotices((n) => [`set_project_path error: ${e}`, ...n]);
    }
  };

  const clearProject = async () => {
    try {
      const next = await invoke<Settings>("set_project_path", { path: null });
      setSettings(next);
    } catch (e) {
      setNotices((n) => [`set_project_path error: ${e}`, ...n]);
    }
  };

  const runPrivacyAudit = async () => {
    try {
      const r = await invoke<PrivacyReport>("audit_privacy");
      setPrivacy(r);
    } catch (e) {
      setNotices((n) => [`audit_privacy error: ${e}`, ...n]);
    }
  };

  useEffect(() => {
    invoke<boolean>("needs_onboarding")
      .then(setOnboardingNeeded)
      .catch(() => setOnboardingNeeded(false));
    refreshModel();
    refreshSettings();
    refreshOllama();
    const unlistenPhase = listen<Phase>("promptloom:phase", (e) =>
      setPhase(e.payload)
    );
    const unlistenNotice = listen<string>("promptloom:notice", (e) =>
      setNotices((prev) => [e.payload, ...prev].slice(0, 20))
    );
    const unlistenProgress = listen<{ bytes: number; total: number }>(
      "promptloom:download",
      (e) => setProgress(e.payload)
    );
    const unlistenDone = listen<boolean>("promptloom:download-done", () => {
      setDownloading(false);
      setProgress(null);
      refreshModel();
    });
    const unlistenTranscript = listen<string>("promptloom:transcript", (e) => {
      const text = e.payload.trim();
      if (!text) return;
      setHistory((prev) =>
        [{ id: Date.now(), text, at: new Date() }, ...prev].slice(0, 200)
      );
    });
    return () => {
      unlistenPhase.then((u) => u());
      unlistenNotice.then((u) => u());
      unlistenProgress.then((u) => u());
      unlistenDone.then((u) => u());
      unlistenTranscript.then((u) => u());
    };
  }, []);

  const pct =
    progress && progress.total > 0
      ? Math.round((100 * progress.bytes) / progress.total)
      : 0;

  const totalWords = useMemo(
    () =>
      history.reduce(
        (acc, h) => acc + h.text.split(/\s+/).filter(Boolean).length,
        0
      ),
    [history]
  );

  if (onboardingNeeded === null) {
    return <main className="pl-page">Loading…</main>;
  }

  if (onboardingNeeded) {
    return (
      <Onboarding
        onFinish={() => {
          setOnboardingNeeded(false);
          refreshSettings();
          refreshModel();
        }}
      />
    );
  }

  const hotkey = settings?.hotkey ?? "F8";
  const dot = PHASE_COLOR[phase.kind];

  const navItems: {
    key: Section;
    label: string;
    icon: string;
    badge?: string;
  }[] = [
    { key: "dictation", label: "Dictation", icon: "⏺" },
    { key: "notetaker", label: "Notetaker", icon: "◉", badge: "Soon" },
    { key: "insights", label: "Insights", icon: "◧" },
    { key: "dictionary", label: "Dictionary", icon: "⌁" },
    { key: "snippets", label: "Snippets", icon: "✂" },
    { key: "style", label: "Style", icon: "Tt" },
    { key: "transforms", label: "Transforms", icon: "✦" },
    { key: "scratchpad", label: "Scratchpad", icon: "☰" },
  ];

  return (
    <div className="pl-app">
      {/* Sidebar */}
      <aside className="pl-sidebar">
        <div className="pl-brand">
          <div className="pl-brand-mark">
            <span style={{ height: 10 }} />
            <span style={{ height: 16 }} />
            <span style={{ height: 20 }} />
            <span style={{ height: 14 }} />
          </div>
          PromptLoom
        </div>
        <nav className="pl-nav">
          {navItems.map((it) => (
            <button
              key={it.key}
              type="button"
              className={`pl-nav-item ${section === it.key ? "active" : ""}`}
              onClick={() => setSection(it.key)}
            >
              <span className="pl-nav-icon">{it.icon}</span>
              {it.label}
              {it.badge && <span className="pl-nav-badge">{it.badge}</span>}
            </button>
          ))}
        </nav>
        <div className="pl-sidebar-spacer" />
        <div className="pl-sidebar-foot">
          <button
            type="button"
            className={`pl-nav-item ${section === "settings" ? "active" : ""}`}
            onClick={() => setSection("settings")}
          >
            <span className="pl-nav-icon">⚙</span>
            Settings
          </button>
          <a
            className="pl-nav-item"
            href="https://github.com/sadlife35/promptloom"
            target="_blank"
            rel="noreferrer"
            style={{ textDecoration: "none" }}
          >
            <span className="pl-nav-icon">?</span>
            Help &amp; GitHub
          </a>
        </div>
      </aside>

      {/* Main pane */}
      <main className="pl-main-pane">
        {section === "dictation" && (
          <DictationPage
            greet={greetingLabel()}
            hotkey={hotkey}
            phase={phase}
            phaseDot={dot}
            history={history}
            onClear={() => setHistory([])}
          />
        )}
        {section === "settings" && (
          <SettingsPage
            model={model}
            settings={settings}
            setSettings={setSettings}
            availableModels={availableModels}
            ollama={ollama}
            refreshOllama={refreshOllama}
            refreshModel={refreshModel}
            notices={notices}
            setNotices={setNotices}
            downloading={downloading}
            setDownloading={setDownloading}
            progress={progress}
            setProgress={setProgress}
            pct={pct}
            verifyResult={verifyResult}
            setVerifyResult={setVerifyResult}
            removeConfirm={removeConfirm}
            setRemoveConfirm={setRemoveConfirm}
            removeInFlight={removeInFlight}
            setRemoveInFlight={setRemoveInFlight}
            privacy={privacy}
            runPrivacyAudit={runPrivacyAudit}
            pickProject={pickProject}
            clearProject={clearProject}
            phase={phase}
            setOnboardingNeeded={setOnboardingNeeded}
          />
        )}
        {section === "insights" && (
          <div className="pl-stub">
            <h2>Insights</h2>
            <p>
              Your voice profile, pacing, pauses, and most-used words will
              surface here. Not yet shipped. Built from local dictation only —
              nothing leaves your PC.
            </p>
          </div>
        )}
        {section === "dictionary" && (
          <div className="pl-stub">
            <h2>Dictionary</h2>
            <p>
              Teach PromptLoom your team's unusual terms. Today, this is loaded
              from <code className="pl-inline">.promptloom/terms.yml</code> in a
              project folder you choose in Settings. An in-app editor is on the
              roadmap.
            </p>
          </div>
        )}
        {section === "snippets" && (
          <div className="pl-stub">
            <h2>Snippets</h2>
            <p>
              Say "insert email" and get a saved block pasted in. Not yet
              shipped.
            </p>
          </div>
        )}
        {section === "style" && (
          <div className="pl-stub">
            <h2>Style</h2>
            <p>
              Give PromptLoom a tone target per app — formal for Gmail, casual
              for Slack, lowercase in Terminal. The three transcript modes
              (Normal / Code / Terminal) in Settings are the first step. Per-app
              style profiles are on the roadmap.
            </p>
          </div>
        )}
        {section === "transforms" && (
          <div className="pl-stub">
            <h2>Transforms</h2>
            <p>
              Mid-dictation voice commands like "make that formal" or "fix
              grammar" via your local Ollama model. Not yet shipped. Will reuse
              the LLM polish pipeline in Settings.
            </p>
          </div>
        )}
        {section === "notetaker" && (
          <div className="pl-stub">
            <h2>Notetaker</h2>
            <p>
              Transcribe meetings locally and get summaries. Not yet shipped.
            </p>
          </div>
        )}
        {section === "scratchpad" && (
          <div className="pl-stub">
            <h2>Scratchpad</h2>
            <p>
              A disposable text pane for dictating into when no other app is
              open. Not yet shipped.
            </p>
          </div>
        )}
      </main>

      {/* Right rail (only on Dictation) */}
      {section === "dictation" && (
        <aside className="pl-rail">
          <div className="pl-stat">
            <div className="pl-stat-num">{totalWords}</div>
            <div className="pl-stat-label">total words</div>
          </div>
          <div className="pl-stat">
            <div className="pl-stat-num">{history.length}</div>
            <div className="pl-stat-label">dictations</div>
          </div>
          <div className="pl-stat">
            <div className="pl-stat-num">1</div>
            <div className="pl-stat-label">day streak</div>
          </div>

          <div className="pl-rail-card">
            <h4>Your Voice Profile</h4>
            <p>
              The more you dictate, the smarter the vocabulary and style
              correction become — all learned on your PC.
            </p>
            <div className="pl-rail-progress">
              <div className="pl-rail-progress-track">
                <div
                  className="pl-rail-progress-fill"
                  style={{
                    width: `${Math.min(100, (totalWords / 2000) * 100).toFixed(1)}%`,
                  }}
                />
              </div>
              <div className="pl-rail-progress-label">
                Unlocks in {Math.max(0, 2000 - totalWords)} words
              </div>
            </div>
          </div>

          <div className="pl-rail-card">
            <h4>Live status</h4>
            <div
              className="pl-row"
              style={{ gap: 10, alignItems: "center", marginTop: 2 }}
            >
              <div
                className="pl-status-dot"
                style={{ background: dot, boxShadow: `0 0 8px ${dot}` }}
              />
              <div>
                <div style={{ fontSize: 13, fontWeight: 500 }}>
                  {phase.label}
                </div>
                {phase.detail && (
                  <div className="pl-status-detail">{phase.detail}</div>
                )}
              </div>
            </div>
          </div>
        </aside>
      )}
    </div>
  );
}

/* ----------------------------- Dictation page ---------------------------- */

function DictationPage({
  greet,
  hotkey,
  phase,
  phaseDot,
  history,
  onClear,
}: {
  greet: string;
  hotkey: string;
  phase: Phase;
  phaseDot: string;
  history: HistEntry[];
  onClear: () => void;
}) {
  const copy = (text: string) => {
    navigator.clipboard?.writeText(text).catch(() => void 0);
  };

  return (
    <>
      <h1 className="pl-greet">
        {greet}, get back into the flow with <kbd>{hotkey}</kbd>
      </h1>

      <div className="pl-promo">
        <h3 className="pl-promo-eyebrow">
          Make PromptLoom sound like <em>you</em>
        </h3>
        <p className="pl-promo-sub">
          Teach your vocabulary, polish filler, and keep every byte on your
          PC. Nothing uploads to a cloud server.
        </p>
        <button className="pl-btn primary" type="button" onClick={() => void 0}>
          Set up vocabulary
        </button>
      </div>

      <div
        className="pl-eyebrow"
        style={{ justifyContent: "space-between" }}
      >
        <span style={{ display: "flex", alignItems: "center", gap: 10 }}>
          Today
          <span
            className="pl-status-dot"
            style={{ background: phaseDot, width: 7, height: 7 }}
            title={phase.label}
          />
        </span>
        {history.length > 0 && (
          <button
            type="button"
            className="pl-btn"
            style={{ padding: "4px 10px", fontSize: 11 }}
            onClick={onClear}
          >
            Clear session
          </button>
        )}
      </div>

      <div className="pl-history">
        {history.length === 0 ? (
          <div className="pl-hist-empty">
            Hold <kbd>{hotkey}</kbd> and speak. Your dictations will appear
            here (session only — nothing is written to disk).
          </div>
        ) : (
          history.map((h) => (
            <div key={h.id} className="pl-hist-row">
              <div className="pl-hist-time">{formatTime(h.at)}</div>
              <div className="pl-hist-body">{h.text}</div>
              <div className="pl-hist-actions">
                <button
                  type="button"
                  className="pl-hist-icon-btn"
                  title="Copy"
                  onClick={() => copy(h.text)}
                >
                  ⎘
                </button>
              </div>
            </div>
          ))
        )}
      </div>
    </>
  );
}

/* ----------------------------- Settings page ----------------------------- */

type SettingsPageProps = {
  model: ModelStatus | null;
  settings: Settings | null;
  setSettings: React.Dispatch<React.SetStateAction<Settings | null>>;
  availableModels: ModelSummary[];
  ollama: OllamaStatus | null;
  refreshOllama: () => void;
  refreshModel: () => void;
  notices: string[];
  setNotices: React.Dispatch<React.SetStateAction<string[]>>;
  downloading: boolean;
  setDownloading: React.Dispatch<React.SetStateAction<boolean>>;
  progress: { bytes: number; total: number } | null;
  setProgress: React.Dispatch<
    React.SetStateAction<{ bytes: number; total: number } | null>
  >;
  pct: number;
  verifyResult: VerifyReport | null;
  setVerifyResult: React.Dispatch<React.SetStateAction<VerifyReport | null>>;
  removeConfirm: boolean;
  setRemoveConfirm: React.Dispatch<React.SetStateAction<boolean>>;
  removeInFlight: boolean;
  setRemoveInFlight: React.Dispatch<React.SetStateAction<boolean>>;
  privacy: PrivacyReport | null;
  runPrivacyAudit: () => void;
  pickProject: () => void;
  clearProject: () => void;
  phase: Phase;
  setOnboardingNeeded: React.Dispatch<React.SetStateAction<boolean | null>>;
};

function SettingsPage(p: SettingsPageProps) {
  const {
    model,
    settings,
    setSettings,
    availableModels,
    ollama,
    refreshOllama,
    refreshModel,
    setNotices,
    downloading,
    setDownloading,
    progress,
    setProgress,
    pct,
    verifyResult,
    setVerifyResult,
    removeConfirm,
    setRemoveConfirm,
    removeInFlight,
    setRemoveInFlight,
    privacy,
    runPrivacyAudit,
    pickProject,
    clearProject,
    phase,
    setOnboardingNeeded,
  } = p;

  return (
    <div className="pl-page" style={{ padding: 0, maxWidth: 720 }}>
      <h1
        style={{
          fontFamily: "var(--font-display)",
          fontStyle: "italic",
          fontWeight: 500,
          fontSize: 32,
          margin: "0 0 24px",
          letterSpacing: "-0.02em",
        }}
      >
        Settings
      </h1>

      {/* Models */}
      <section className="pl-section">
        <div className="pl-section-label">Models</div>
        {availableModels.length > 1 && (
          <div className="pl-card">
            <div className="pl-card-head">Speech model</div>
            <div className="pl-card-body" style={{ marginBottom: 10 }}>
              Used for all dictations. Switching takes effect on the next
              recording.
            </div>
            <select
              className="pl-select"
              value={settings?.active_model ?? ""}
              onChange={async (e) => {
                try {
                  const next = await invoke<Settings>("set_active_model", {
                    name: e.target.value,
                  });
                  setSettings(next);
                  refreshModel();
                } catch (err) {
                  setNotices((n) => [`set_active_model error: ${err}`, ...n]);
                }
              }}
            >
              {availableModels.map((m) => (
                <option key={m.name} value={m.name}>
                  {m.name} — {(m.size_bytes / 1_000_000).toFixed(0)} MB
                  {m.installed ? " · installed" : " · not installed"}
                </option>
              ))}
            </select>
          </div>
        )}

        {model && (
          <div className="pl-card">
            <div className="pl-row between" style={{ marginBottom: 10 }}>
              <div>
                <div className="pl-card-head" style={{ marginBottom: 2 }}>
                  {model.name}
                </div>
                <div className="pl-dim" style={{ fontSize: 12 }}>
                  {(model.size_bytes / 1_000_000).toFixed(1)} MB · active
                </div>
              </div>
              {model.installed ? (
                model.verified ? (
                  <span className="pl-pill ok">
                    <span className="dot" /> SHA-256 verified
                  </span>
                ) : (
                  <span className="pl-pill err">
                    <span className="dot" /> hash mismatch
                  </span>
                )
              ) : (
                <span className="pl-pill warn">
                  <span className="dot" /> not installed
                </span>
              )}
            </div>

            <dl className="pl-dl">
              <dt>Weights</dt>
              <dd>{model.path}</dd>
              <dt>Engine</dt>
              <dd>
                {model.engine_installed
                  ? model.engine_path ?? "whisper-cli.exe present"
                  : "whisper-cli.exe missing"}
              </dd>
            </dl>

            {!model.installed && (
              <>
                <div className="pl-notice" style={{ marginTop: 10 }}>
                  Model downloads happen only after you click below. Nothing is
                  fetched automatically.
                </div>
                <div style={{ marginTop: 10 }}>
                  <button
                    type="button"
                    className="pl-btn primary"
                    disabled={downloading}
                    onClick={() => {
                      setDownloading(true);
                      setProgress({ bytes: 0, total: model.size_bytes });
                      invoke("download_model_cmd").catch((e) => {
                        setNotices((n) => [`download error: ${e}`, ...n]);
                        setDownloading(false);
                      });
                    }}
                  >
                    {downloading ? `Downloading… ${pct}%` : "Download model"}
                  </button>
                </div>
              </>
            )}

            {downloading && progress && (
              <div style={{ marginTop: 12 }}>
                <div className="pl-progress">
                  <div className="pl-progress-fill" style={{ width: `${pct}%` }} />
                </div>
                <div
                  className="pl-dim pl-mono"
                  style={{ fontSize: 11, marginTop: 4 }}
                >
                  {(progress.bytes / 1_000_000).toFixed(1)} /{" "}
                  {(progress.total / 1_000_000).toFixed(1)} MB
                </div>
              </div>
            )}

            {model.installed && (
              <div className="pl-btn-group" style={{ marginTop: 14 }}>
                <button
                  type="button"
                  className="pl-btn"
                  onClick={async () => {
                    setVerifyResult(null);
                    try {
                      const r = await invoke<VerifyReport>("verify_model_cmd");
                      setVerifyResult(r);
                      refreshModel();
                    } catch (e) {
                      setVerifyResult({ ok: false, message: String(e) });
                    }
                  }}
                >
                  Verify installed model
                </button>
                <button
                  type="button"
                  className="pl-btn danger"
                  onClick={() => setRemoveConfirm(true)}
                >
                  Remove downloaded model
                </button>
              </div>
            )}

            {verifyResult && (
              <div
                className={`pl-notice ${verifyResult.ok ? "ok" : "danger"}`}
                style={{ marginTop: 10 }}
              >
                <strong>{verifyResult.message}</strong>
              </div>
            )}

            {removeConfirm && (
              <div className="pl-notice warn" style={{ marginTop: 10 }}>
                <strong>Remove the downloaded model?</strong> You'll need to
                re-download it to dictate again.
                <div className="pl-btn-group" style={{ marginTop: 10 }}>
                  <button
                    type="button"
                    className="pl-btn danger"
                    disabled={removeInFlight}
                    onClick={async () => {
                      setRemoveInFlight(true);
                      try {
                        const r = await invoke<RemoveReport>("remove_model_cmd", {
                          confirm: true,
                        });
                        setNotices((n) => [r.message, ...n]);
                        setRemoveConfirm(false);
                        setVerifyResult(null);
                        refreshModel();
                        invoke<boolean>("needs_onboarding").then(
                          setOnboardingNeeded
                        );
                      } catch (e) {
                        setNotices((n) => [`remove error: ${e}`, ...n]);
                      } finally {
                        setRemoveInFlight(false);
                      }
                    }}
                  >
                    {removeInFlight ? "Removing…" : "Yes, remove"}
                  </button>
                  <button
                    type="button"
                    className="pl-btn"
                    onClick={() => setRemoveConfirm(false)}
                    disabled={removeInFlight}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            )}
          </div>
        )}
      </section>

      {/* Transcript mode */}
      <section className="pl-section">
        <div className="pl-section-label">Transcript mode</div>
        <div className="pl-card">
          <div className="pl-btn-group" style={{ marginBottom: 10 }}>
            {(["normal", "code", "terminal"] as const).map((m) => (
              <button
                key={m}
                type="button"
                className={`pl-btn ${
                  settings?.transcript_mode === m ? "active" : ""
                }`}
                onClick={async () => {
                  try {
                    const next = await invoke<Settings>("set_transcript_mode", {
                      mode: m,
                    });
                    setSettings(next);
                  } catch (err) {
                    setNotices((n) => [
                      `set_transcript_mode error: ${err}`,
                      ...n,
                    ]);
                  }
                }}
                style={{ textTransform: "capitalize" }}
              >
                {m}
              </button>
            ))}
          </div>
          <div className="pl-card-body">
            {settings?.transcript_mode === "code" && (
              <>
                <strong style={{ color: "var(--fg)" }}>Code</strong>: structural
                commands active, sentence punctuation words preserved,
                auto-capitalization off.
              </>
            )}
            {settings?.transcript_mode === "terminal" && (
              <>
                <strong style={{ color: "var(--fg)" }}>Terminal</strong>: like
                Code mode, for shell prompts.
              </>
            )}
            {(!settings || settings.transcript_mode === "normal") && (
              <>
                <strong style={{ color: "var(--fg)" }}>Normal</strong>: all
                dictation commands, filler stripping, sentence capitalization.
              </>
            )}
          </div>
        </div>
      </section>

      {/* LLM polish */}
      <section className="pl-section">
        <div className="pl-section-label">LLM polish (optional)</div>
        <div className="pl-card">
          <div className="pl-card-body" style={{ marginBottom: 12 }}>
            After the normal pipeline runs, PromptLoom can send the transcript
            to your{" "}
            <strong style={{ color: "var(--fg)" }}>own local Ollama server</strong>{" "}
            for one additional cleanup pass. Hard 2-second timeout. Raw-text
            fallback.
          </div>

          <label
            style={{
              display: "flex",
              gap: 10,
              alignItems: "center",
              cursor: ollama?.reachable ? "pointer" : "not-allowed",
              opacity: ollama?.reachable ? 1 : 0.5,
            }}
          >
            <input
              type="checkbox"
              checked={settings?.ollama_enabled ?? false}
              disabled={!ollama?.reachable}
              onChange={async (e) => {
                try {
                  const next = await invoke<Settings>("set_ollama_enabled", {
                    enabled: e.target.checked,
                  });
                  setSettings(next);
                  refreshOllama();
                } catch (err) {
                  setNotices((n) => [`set_ollama_enabled error: ${err}`, ...n]);
                }
              }}
            />
            <span style={{ fontSize: 13 }}>
              Clean transcripts with Ollama{" "}
              <code className="pl-inline">
                {ollama?.endpoint ?? "http://localhost:11434"}
              </code>
            </span>
          </label>

          {!ollama?.reachable && (
            <div className="pl-notice warn" style={{ marginTop: 10 }}>
              Ollama is not responding. Install from{" "}
              <code className="pl-inline">https://ollama.com</code> and pull a
              small model, e.g.{" "}
              <code className="pl-inline">ollama pull llama3.2:3b</code>.
            </div>
          )}

          {ollama?.reachable && (
            <div style={{ marginTop: 12 }} className="pl-stack-sm">
              <div className="pl-card-body" style={{ fontSize: 12 }}>
                Model
              </div>
              <select
                className="pl-select"
                value={settings?.ollama_model ?? ""}
                onChange={async (e) => {
                  try {
                    const next = await invoke<Settings>("set_ollama_model", {
                      model: e.target.value || null,
                    });
                    setSettings(next);
                  } catch (err) {
                    setNotices((n) => [`set_ollama_model error: ${err}`, ...n]);
                  }
                }}
              >
                <option value="">(none selected)</option>
                {ollama.models.map((m) => (
                  <option key={m.name} value={m.name}>
                    {m.name}
                    {m.size
                      ? ` — ${(m.size / 1_000_000_000).toFixed(1)} GB`
                      : ""}
                  </option>
                ))}
              </select>
            </div>
          )}

          <div style={{ marginTop: 12 }}>
            <button type="button" className="pl-btn" onClick={refreshOllama}>
              Refresh Ollama status
            </button>
          </div>
        </div>
      </section>

      {/* Dictation controls */}
      <section className="pl-section">
        <div className="pl-section-label">Dictation controls</div>
        <div className="pl-card">
          <div className="pl-stack">
            <label
              style={{
                display: "flex",
                gap: 10,
                alignItems: "flex-start",
                cursor: phase.kind === "ready" ? "pointer" : "not-allowed",
                opacity: phase.kind === "ready" ? 1 : 0.5,
              }}
            >
              <input
                type="radio"
                name="mode"
                value="hold"
                checked={settings?.dictation_mode === "hold"}
                disabled={phase.kind !== "ready" || !settings}
                onChange={async () => {
                  try {
                    const next = await invoke<Settings>("set_dictation_mode", {
                      mode: "hold",
                    });
                    setSettings(next);
                  } catch (e) {
                    setNotices((n) => [`set_dictation_mode error: ${e}`, ...n]);
                  }
                }}
              />
              <div>
                <div style={{ fontWeight: 600 }}>
                  Hold-to-talk{" "}
                  <span className="pl-muted" style={{ fontWeight: 400 }}>
                    (recommended)
                  </span>
                </div>
                <div className="pl-card-body" style={{ marginTop: 2 }}>
                  Hold <kbd>{settings?.hotkey ?? "F8"}</kbd> while speaking,
                  release to transcribe.
                </div>
              </div>
            </label>
            <label
              style={{
                display: "flex",
                gap: 10,
                alignItems: "flex-start",
                cursor: phase.kind === "ready" ? "pointer" : "not-allowed",
                opacity: phase.kind === "ready" ? 1 : 0.5,
              }}
            >
              <input
                type="radio"
                name="mode"
                value="toggle"
                checked={settings?.dictation_mode === "toggle"}
                disabled={phase.kind !== "ready" || !settings}
                onChange={async () => {
                  try {
                    const next = await invoke<Settings>("set_dictation_mode", {
                      mode: "toggle",
                    });
                    setSettings(next);
                  } catch (e) {
                    setNotices((n) => [`set_dictation_mode error: ${e}`, ...n]);
                  }
                }}
              />
              <div>
                <div style={{ fontWeight: 600 }}>Toggle</div>
                <div className="pl-card-body" style={{ marginTop: 2 }}>
                  Tap <kbd>{settings?.hotkey ?? "F8"}</kbd> to start recording,
                  tap again to stop.
                </div>
              </div>
            </label>
          </div>
          <div className="pl-dim" style={{ fontSize: 11, marginTop: 10 }}>
            <kbd>Esc</kbd> always cancels — in both modes.
          </div>
        </div>
      </section>

      {/* Project vocabulary */}
      <section className="pl-section">
        <div className="pl-section-label">Project vocabulary</div>
        <div className="pl-card">
          <div className="pl-card-body" style={{ marginBottom: 10 }}>
            Choose a project folder containing{" "}
            <code className="pl-inline">.promptloom/terms.yml</code>.
          </div>
          {settings?.project_path ? (
            <>
              <div
                className="pl-mono"
                style={{
                  fontSize: 12,
                  color: "var(--fg)",
                  wordBreak: "break-all",
                  padding: "8px 10px",
                  background: "var(--bg-sunken)",
                  border: "1px solid var(--border)",
                  borderRadius: 4,
                }}
              >
                {settings.project_path}
              </div>
              <div className="pl-card-body" style={{ marginTop: 8 }}>
                Loaded{" "}
                <strong style={{ color: "var(--fg)" }}>
                  {settings.terms_count}
                </strong>{" "}
                term{settings.terms_count === 1 ? "" : "s"}.
              </div>
              <div className="pl-btn-group" style={{ marginTop: 10 }}>
                <button type="button" className="pl-btn" onClick={pickProject}>
                  Change folder
                </button>
                <button type="button" className="pl-btn" onClick={clearProject}>
                  Clear
                </button>
              </div>
            </>
          ) : (
            <button type="button" className="pl-btn" onClick={pickProject}>
              Choose project folder…
            </button>
          )}
        </div>
      </section>

      {/* Privacy audit */}
      <section className="pl-section">
        <div className="pl-section-label">Privacy audit</div>
        <div className="pl-card">
          <div className="pl-card-body" style={{ marginBottom: 10 }}>
            Scans the scratch directory for any residual audio or logs.
          </div>
          <button type="button" className="pl-btn" onClick={runPrivacyAudit}>
            Run audit now
          </button>
          {privacy && (
            <div style={{ marginTop: 12 }}>
              <div className="pl-dim" style={{ fontSize: 11 }}>
                scratch <code className="pl-inline">{privacy.scratch_dir}</code>
              </div>
              {privacy.residual_wavs.length === 0 &&
              privacy.residual_logs.length === 0 ? (
                <div className="pl-pill ok" style={{ marginTop: 8 }}>
                  <span className="dot" />
                  0 wav · 0 log · clean
                </div>
              ) : (
                <div className="pl-pill err" style={{ marginTop: 8 }}>
                  <span className="dot" />
                  {privacy.residual_wavs.length} wav ·{" "}
                  {privacy.residual_logs.length} log
                </div>
              )}
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
