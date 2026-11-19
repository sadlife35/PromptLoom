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
  auto_context: boolean;
  bundled_polish_enabled: boolean;
  bundled_polish_model: string | null;
  insights_enabled: boolean;
  transforms_enabled: boolean;
  auto_transform: string | null;
};

type SessionRecord = {
  ts: number;
  word_count: number;
  duration_ms: number;
  filler_count: number;
  app: string;
};

type InsightsSummary = {
  total_sessions: number;
  total_words: number;
  total_duration_ms: number;
  avg_wpm: number;
  filler_rate: number;
  top_words: [string, number][];
  recent: SessionRecord[];
};

type LlamaHealth =
  | { kind: "active"; port: number }
  | { kind: "fallback" }
  | { kind: "disabled" };

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
  in_memory_streaming?: boolean;
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
          Jot
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
        {section === "insights" && <InsightsPage />}
        {section === "dictionary" && <DictionaryPage setNotices={setNotices} />}
        {section === "snippets" && <SnippetsPage />}
        {section === "style" && <StylePage />}
        {section === "transforms" && <TransformsPage />}
        {section === "notetaker" && <NotetakerPage />}
        {section === "scratchpad" && <ScratchpadPage />}
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
          Make Jot sound like <em>you</em>
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

/* ---------------------------- Dictionary page ---------------------------- */

type PolishModelSummary = {
  name: string;
  size_bytes: number;
  note: string | null;
  installed: boolean;
  is_default: boolean;
};

function PolishModelDownloader() {
  const [models, setModels] = useState<PolishModelSummary[]>([]);
  const [downloading, setDownloading] = useState<string | null>(null);
  const [progress, setProgress] = useState<{ bytes: number; total: number } | null>(
    null
  );
  const [err, setErr] = useState<string | null>(null);

  const refresh = () =>
    invoke<PolishModelSummary[]>("list_polish_models")
      .then(setModels)
      .catch((e) => setErr(String(e)));

  useEffect(() => {
    refresh();
    const unProgress = listen<{ bytes: number; total: number }>(
      "promptloom:polish-download",
      (e) => setProgress(e.payload)
    );
    const unDone = listen<boolean>("promptloom:polish-download-done", (e) => {
      setDownloading(null);
      setProgress(null);
      if (!e.payload) setErr("Download failed. Check network and retry.");
      refresh();
    });
    return () => {
      unProgress.then((f) => f());
      unDone.then((f) => f());
    };
  }, []);

  const download = async (name: string) => {
    setErr(null);
    setDownloading(name);
    setProgress({ bytes: 0, total: 0 });
    try {
      await invoke("download_polish_model_cmd", { name });
    } catch (e) {
      setErr(String(e));
      setDownloading(null);
    }
  };

  const pct =
    progress && progress.total > 0
      ? Math.floor((progress.bytes / progress.total) * 100)
      : 0;

  if (models.length === 0) {
    return null;
  }

  return (
    <div style={{ marginTop: 16 }}>
      <div
        className="pl-section-label"
        style={{ marginBottom: 8, fontSize: 11 }}
      >
        Available polish models
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {models.map((m) => (
          <div
            key={m.name}
            className="pl-row between"
            style={{
              padding: "10px 12px",
              border: "1px solid var(--border)",
              borderRadius: 6,
            }}
          >
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 13, color: "var(--fg)" }}>
                {m.name}
                {m.is_default && (
                  <span
                    className="pl-pill"
                    style={{ marginLeft: 8, fontSize: 10 }}
                  >
                    recommended
                  </span>
                )}
              </div>
              <div
                style={{
                  fontSize: 12,
                  color: "var(--fg-dim)",
                  marginTop: 2,
                }}
              >
                {(m.size_bytes / 1_000_000_000).toFixed(2)} GB
                {m.note ? ` · ${m.note}` : ""}
              </div>
            </div>
            {m.installed ? (
              <span className="pl-pill ok">
                <span className="dot" /> installed
              </span>
            ) : downloading === m.name ? (
              <div style={{ minWidth: 140 }}>
                <div className="pl-progress">
                  <div
                    className="pl-progress-fill"
                    style={{ width: `${pct}%` }}
                  />
                </div>
                <div
                  className="pl-dim pl-mono"
                  style={{ fontSize: 10, marginTop: 2, textAlign: "right" }}
                >
                  {pct}%
                </div>
              </div>
            ) : (
              <button
                className="pl-btn"
                onClick={() => download(m.name)}
                disabled={!!downloading}
              >
                Download
              </button>
            )}
          </div>
        ))}
      </div>
      {err && (
        <div className="pl-notice danger" style={{ marginTop: 8 }}>
          {err}
        </div>
      )}
    </div>
  );
}

function StylePage() {
  return (
    <div className="pl-stack" style={{ maxWidth: 820 }}>
      <h2
        style={{
          fontFamily: "var(--font-display)",
          fontStyle: "italic",
          fontWeight: 500,
          fontSize: 32,
          margin: "0 0 6px",
          color: "var(--fg)",
        }}
      >
        Style
      </h2>
      <p style={{ color: "var(--fg-dim)", margin: "0 0 20px", fontSize: 14 }}>
        Give each app its own tone. The bundled polish model rewrites your
        transcript in that register before it's pasted. Apps detected as Code
        or Terminal keep their raw style.
      </p>
      <StyleTonePreview />
      <AppProfilesSection />
    </div>
  );
}

function StyleTonePreview() {
  const sample = "yeah so I just wanted to let you know the thing is done";
  const casual = "Yeah, so I just wanted to let you know — the thing's done.";
  const formal =
    "I wanted to let you know that the task has been completed.";
  return (
    <section className="pl-section">
      <div className="pl-section-label">How tones read</div>
      <div className="pl-card">
        <div style={{ fontSize: 13, color: "var(--fg-dim)", marginBottom: 10 }}>
          You dictate:
        </div>
        <div
          className="pl-mono"
          style={{
            fontSize: 13,
            padding: "10px 12px",
            background: "var(--bg-alt)",
            borderRadius: 6,
            marginBottom: 16,
          }}
        >
          "{sample}"
        </div>
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "1fr 1fr",
            gap: 12,
          }}
        >
          <div
            style={{
              border: "1px solid var(--border)",
              borderRadius: 6,
              padding: 12,
            }}
          >
            <div
              className="pl-section-label"
              style={{ marginBottom: 6, fontSize: 11 }}
            >
              casual
            </div>
            <div style={{ fontSize: 13 }}>{casual}</div>
          </div>
          <div
            style={{
              border: "1px solid var(--border)",
              borderRadius: 6,
              padding: 12,
            }}
          >
            <div
              className="pl-section-label"
              style={{ marginBottom: 6, fontSize: 11 }}
            >
              formal
            </div>
            <div style={{ fontSize: 13 }}>{formal}</div>
          </div>
        </div>
      </div>
    </section>
  );
}

const TRANSFORMS: { phrase: string; name: string; sample: [string, string] }[] = [
  {
    phrase: "make that formal",
    name: "Make formal",
    sample: [
      "yeah we're gonna ship it tomorrow",
      "We plan to ship it tomorrow.",
    ],
  },
  {
    phrase: "make that casual",
    name: "Make casual",
    sample: [
      "I would like to inform you that the task is complete",
      "Just letting you know — it's done.",
    ],
  },
  {
    phrase: "make that shorter",
    name: "Make shorter",
    sample: [
      "I was just thinking that we should probably go ahead and try it",
      "Let's try it.",
    ],
  },
  {
    phrase: "fix grammar",
    name: "Fix grammar",
    sample: [
      "me and him was going to the store",
      "He and I were going to the store.",
    ],
  },
];

type NotetakerStatus = {
  recording: boolean;
  elapsed_secs: number;
  path: string | null;
  chars_written: number;
};

type MeetingSummary = {
  path: string;
  name: string;
  bytes: number;
  modified_secs: number;
};

function NotetakerPage() {
  const [status, setStatus] = useState<NotetakerStatus | null>(null);
  const [liveTranscript, setLiveTranscript] = useState("");
  const [meetings, setMeetings] = useState<MeetingSummary[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [selectedBody, setSelectedBody] = useState<string>("");
  const [summary, setSummary] = useState<string | null>(null);
  const [summarizing, setSummarizing] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const refreshStatus = () =>
    invoke<NotetakerStatus>("notetaker_status").then(setStatus);
  const refreshMeetings = () =>
    invoke<MeetingSummary[]>("notetaker_list_meetings").then(setMeetings);

  useEffect(() => {
    refreshStatus();
    refreshMeetings();
    const id = setInterval(refreshStatus, 2000);
    const unTx = listen<string>("promptloom:notetaker-transcript", (e) => {
      setLiveTranscript((prev) => (prev ? prev + "\n\n" + e.payload : e.payload));
    });
    const unState = listen<string>("promptloom:notetaker-state", (e) => {
      refreshStatus();
      if (e.payload === "idle") {
        refreshMeetings();
      } else if (e.payload === "recording") {
        setLiveTranscript("");
      }
    });
    return () => {
      clearInterval(id);
      unTx.then((f) => f());
      unState.then((f) => f());
    };
  }, []);

  const start = async () => {
    setErr(null);
    try {
      await invoke<string>("notetaker_start");
      refreshStatus();
    } catch (e) {
      setErr(String(e));
    }
  };
  const stop = async () => {
    try {
      await invoke("notetaker_stop");
      refreshStatus();
    } catch (e) {
      setErr(String(e));
    }
  };

  const open = async (m: MeetingSummary) => {
    setSelected(m.path);
    setSummary(null);
    try {
      const body = await invoke<string>("notetaker_read_meeting", {
        path: m.path,
      });
      setSelectedBody(body);
    } catch (e) {
      setErr(String(e));
    }
  };

  const summarize = async () => {
    if (!selected) return;
    setSummarizing(true);
    setSummary(null);
    try {
      const s = await invoke<string>("notetaker_summarize_meeting", {
        path: selected,
      });
      setSummary(s);
    } catch (e) {
      setErr(String(e));
    } finally {
      setSummarizing(false);
    }
  };

  const fmtDuration = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}:${String(s).padStart(2, "0")}`;
  };
  const fmtTs = (secs: number) => {
    if (!secs) return "—";
    return new Date(secs * 1000).toLocaleString();
  };

  return (
    <div className="pl-stack" style={{ maxWidth: 1040 }}>
      <h2
        style={{
          fontFamily: "var(--font-display)",
          fontStyle: "italic",
          fontWeight: 500,
          fontSize: 32,
          margin: "0 0 6px",
          color: "var(--fg)",
        }}
      >
        Notetaker
      </h2>
      <p style={{ color: "var(--fg-dim)", margin: "0 0 20px", fontSize: 14 }}>
        Long-form meeting transcription. Captures from your microphone only —
        the other side of the call is captured only to the extent your mic
        hears it through speakers. 15-second chunks, appends to a markdown file
        in <code className="pl-inline">%APPDATA%/com.promptloom.desktop/meetings/</code>.
        No system audio loopback, no speaker labels.
      </p>

      <section className="pl-section">
        <div className="pl-card">
          <div className="pl-row between" style={{ marginBottom: 12 }}>
            <div>
              <div className="pl-section-label" style={{ marginBottom: 4 }}>
                {status?.recording ? "Recording" : "Idle"}
              </div>
              <div
                style={{
                  fontFamily: "var(--font-display)",
                  fontSize: 28,
                  color: "var(--fg)",
                }}
              >
                {status?.recording ? fmtDuration(status.elapsed_secs) : "0:00"}
              </div>
            </div>
            <div style={{ display: "flex", gap: 8 }}>
              {status?.recording ? (
                <button className="pl-btn danger" onClick={stop}>
                  Stop
                </button>
              ) : (
                <button className="pl-btn primary" onClick={start}>
                  Start recording
                </button>
              )}
            </div>
          </div>
          {err && <div className="pl-notice danger">{err}</div>}
          {status?.recording && (
            <div
              style={{
                marginTop: 10,
                maxHeight: 220,
                overflowY: "auto",
                padding: 12,
                background: "var(--bg)",
                border: "1px solid var(--border)",
                borderRadius: 6,
                fontSize: 13,
                lineHeight: 1.55,
                whiteSpace: "pre-wrap",
              }}
            >
              {liveTranscript ||
                "Listening. New text will appear every ~15 seconds."}
            </div>
          )}
        </div>
      </section>

      <section className="pl-section">
        <div className="pl-section-label">Saved meetings ({meetings.length})</div>
        {meetings.length === 0 ? (
          <div className="pl-muted" style={{ fontSize: 13 }}>
            No meetings yet. Press Start recording above to capture one.
          </div>
        ) : (
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "280px 1fr",
              gap: 16,
            }}
          >
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              {meetings.map((m) => (
                <button
                  key={m.path}
                  onClick={() => open(m)}
                  className={`pl-btn ${selected === m.path ? "active" : ""}`}
                  style={{
                    textAlign: "left",
                    padding: "10px 12px",
                    display: "flex",
                    flexDirection: "column",
                    alignItems: "flex-start",
                    gap: 2,
                  }}
                >
                  <span className="pl-mono" style={{ fontSize: 12 }}>
                    {m.name}
                  </span>
                  <span style={{ fontSize: 11, color: "var(--fg-dim)" }}>
                    {(m.bytes / 1000).toFixed(1)} KB · {fmtTs(m.modified_secs)}
                  </span>
                </button>
              ))}
            </div>
            <div
              style={{
                border: "1px solid var(--border)",
                borderRadius: 6,
                padding: 14,
                minHeight: 260,
                background: "var(--bg)",
              }}
            >
              {selected ? (
                <>
                  <div
                    className="pl-row between"
                    style={{ marginBottom: 10 }}
                  >
                    <div
                      className="pl-mono"
                      style={{ fontSize: 12, color: "var(--fg-dim)" }}
                    >
                      {selected}
                    </div>
                    <button
                      className="pl-btn"
                      onClick={summarize}
                      disabled={summarizing}
                    >
                      {summarizing ? "Summarizing…" : "Summarize"}
                    </button>
                  </div>
                  {summary && (
                    <div
                      style={{
                        marginBottom: 12,
                        padding: 10,
                        background: "var(--bg-alt)",
                        borderRadius: 4,
                        borderLeft: "3px solid var(--accent)",
                        fontSize: 13,
                        lineHeight: 1.55,
                        whiteSpace: "pre-wrap",
                      }}
                    >
                      <div
                        className="pl-section-label"
                        style={{ marginBottom: 6, fontSize: 10 }}
                      >
                        Summary
                      </div>
                      {summary}
                    </div>
                  )}
                  <pre
                    style={{
                      fontSize: 13,
                      lineHeight: 1.55,
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-word",
                      margin: 0,
                      fontFamily: "var(--font-body)",
                      maxHeight: 420,
                      overflowY: "auto",
                    }}
                  >
                    {selectedBody}
                  </pre>
                </>
              ) : (
                <div className="pl-muted" style={{ fontSize: 13 }}>
                  Select a meeting on the left to read it.
                </div>
              )}
            </div>
          </div>
        )}
      </section>

      <div className="pl-notice">
        <strong>Known limitations:</strong> mic only (no system audio), no
        speaker labels, 15-second chunks mean sentences occasionally split
        across boundaries. Summary uses the bundled polish model — install one
        in Settings to enable it.
      </div>
    </div>
  );
}

const TRANSFORM_ACTIONS = [
  { key: "make_formal", label: "Formal" },
  { key: "make_casual", label: "Casual" },
  { key: "make_shorter", label: "Shorter" },
  { key: "fix_grammar", label: "Fix grammar" },
];

function TransformsPage() {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [auto, setAuto] = useState<string | null>(null);

  useEffect(() => {
    invoke<Settings>("get_settings").then((s) => {
      setEnabled(s.transforms_enabled);
      setAuto(s.auto_transform ?? null);
    });
  }, []);

  const toggle = async (next: boolean) => {
    try {
      const updated = await invoke<Settings>("set_transforms_enabled", {
        enabled: next,
      });
      setEnabled(updated.transforms_enabled);
    } catch (e) {
      console.error(e);
    }
  };
  const chooseAuto = async (next: string | null) => {
    try {
      const updated = await invoke<Settings>("set_auto_transform", {
        action: next,
      });
      setAuto(updated.auto_transform ?? null);
    } catch (e) {
      console.error(e);
    }
  };

  return (
    <div className="pl-stack" style={{ maxWidth: 980 }}>
      <div className="pl-row between" style={{ marginBottom: 20 }}>
        <h2
          style={{
            fontFamily: "var(--font-display)",
            fontStyle: "italic",
            fontWeight: 500,
            fontSize: 34,
            margin: 0,
            color: "var(--fg)",
          }}
        >
          Transforms
          <span className="pl-beta">Beta</span>
        </h2>
        <div style={{ display: "flex", gap: 14, alignItems: "center" }}>
          <label
            style={{
              display: "flex",
              gap: 8,
              alignItems: "center",
              cursor: "pointer",
            }}
          >
            <span style={{ fontSize: 13, color: "var(--fg-dim)" }}>Opt in</span>
            <input
              type="checkbox"
              checked={enabled ?? true}
              onChange={(e) => toggle(e.target.checked)}
            />
          </label>
        </div>
      </div>

      <div className="pl-hero">
        <h3>
          Transforms work <em>wherever</em> you write.
        </h3>
        <p>
          After a dictation lands, say one of these phrases on its own to
          rewrite what you just pasted — the bundled polish model does the
          rewrite, keystrokes replace the text in place. Works in any window.
        </p>
      </div>

      <section className="pl-section">
        <div
          className="pl-card"
          style={{ display: "flex", alignItems: "center", gap: 14 }}
        >
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 14, color: "var(--fg)" }}>
              Auto apply after dictation
            </div>
            <div
              style={{ fontSize: 12, color: "var(--fg-dim)", marginTop: 4 }}
            >
              Run a transform on everything you dictate, automatically.
            </div>
          </div>
          <select
            className="pl-input"
            value={auto ?? ""}
            onChange={(e) =>
              chooseAuto(e.target.value === "" ? null : e.target.value)
            }
            style={{ minWidth: 180 }}
          >
            <option value="">Off</option>
            {TRANSFORM_ACTIONS.map((a) => (
              <option key={a.key} value={a.key}>
                {a.label}
              </option>
            ))}
          </select>
        </div>
      </section>

      <section className="pl-section">
        <div className="pl-row between" style={{ marginBottom: 14 }}>
          <div
            style={{
              fontFamily: "var(--font-display)",
              fontStyle: "italic",
              fontSize: 22,
              color: "var(--fg)",
            }}
          >
            My Transforms
          </div>
        </div>
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(3, 1fr)",
            gap: 14,
          }}
        >
          {TRANSFORMS.map((t, i) => (
            <div key={t.phrase} className="pl-tile">
              <div className="pl-kbd">
                <span>Say</span>
                <span className="pl-mono">"{t.phrase.split(" ")[0]}…"</span>
              </div>
              <div
                style={{
                  fontFamily: "var(--font-display)",
                  fontSize: 18,
                  color: "var(--fg)",
                }}
              >
                {t.name}
              </div>
              <div style={{ fontSize: 12, color: "var(--fg-dim)" }}>
                {i === 0 && "Professional register, no contractions."}
                {i === 1 && "Conversational, uses contractions."}
                {i === 2 && "Cut redundancy, keep meaning."}
                {i === 3 && "Fix spelling and grammar only."}
              </div>
            </div>
          ))}
          <div className="pl-tile create">
            <div style={{ fontSize: 24 }}>+</div>
            <div style={{ fontSize: 13 }}>Create your own</div>
            <div style={{ fontSize: 11 }}>
              Coming soon — custom prompts you define.
            </div>
          </div>
        </div>
      </section>

      <div className="pl-notice">
        <strong>Caveats:</strong> if you've typed or clicked anything else
        after the dictation, the keystroke-select will grab the wrong text.
        If the previous paste is longer than ~1200 characters, the transform
        skips with a notice.
      </div>
    </div>
  );
}

function InsightsPage() {
  const [summary, setSummary] = useState<InsightsSummary | null>(null);
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [tab, setTab] = useState<"usage" | "voice">("usage");
  const [confirmingClear, setConfirmingClear] = useState(false);

  const refresh = () => {
    invoke<Settings>("get_settings").then((s) => setEnabled(s.insights_enabled));
    invoke<InsightsSummary>("insights_summary").then(setSummary);
  };

  useEffect(() => {
    refresh();
  }, []);

  const toggle = async (next: boolean) => {
    try {
      const updated = await invoke<Settings>("set_insights_enabled", {
        enabled: next,
      });
      setEnabled(updated.insights_enabled);
    } catch (e) {
      console.error(e);
    }
  };

  const clearAll = async () => {
    try {
      await invoke("clear_insights");
      setConfirmingClear(false);
      refresh();
    } catch (e) {
      console.error(e);
    }
  };

  const appUsage = aggregateAppUsage(summary?.recent ?? []);
  const streak = computeStreak(summary?.recent ?? []);

  return (
    <div className="pl-stack" style={{ maxWidth: 1040 }}>
      <div className="pl-row between" style={{ marginBottom: 18 }}>
        <h2
          style={{
            fontFamily: "var(--font-display)",
            fontStyle: "italic",
            fontWeight: 500,
            fontSize: 34,
            margin: 0,
            color: "var(--fg)",
          }}
        >
          Insights
        </h2>
      </div>

      <div
        style={{
          display: "flex",
          gap: 24,
          borderBottom: "1px solid var(--border)",
          marginBottom: 24,
        }}
      >
        {(["usage", "voice"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            style={{
              background: "transparent",
              border: 0,
              padding: "8px 0",
              marginBottom: -1,
              borderBottom:
                tab === t ? "2px solid var(--accent)" : "2px solid transparent",
              color: tab === t ? "var(--fg)" : "var(--fg-dim)",
              fontSize: 13,
              cursor: "pointer",
            }}
          >
            {t === "usage" ? "Your usage" : "Your voice"}
          </button>
        ))}
      </div>

      {enabled === false && (
        <div className="pl-notice" style={{ marginBottom: 20 }}>
          <strong>Insights are off.</strong>{" "}
          <button
            className="pl-btn"
            onClick={() => toggle(true)}
            style={{ marginLeft: 10 }}
          >
            Turn on
          </button>{" "}
          <span style={{ fontSize: 12, color: "var(--fg-dim)" }}>
            Off by default — only aggregate counts are stored, never raw text.
          </span>
        </div>
      )}

      {tab === "usage" && (
        <>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(3, 1fr)",
              gap: 16,
              marginBottom: 20,
            }}
          >
            <div className="pl-bigstat">
              <div className="pl-bigstat-num">
                {summary ? summary.avg_wpm.toFixed(0) : "—"}
              </div>
              <div className="pl-bigstat-label" style={{ marginTop: 8 }}>
                Words per minute
              </div>
            </div>
            <div className="pl-bigstat">
              <div className="pl-bigstat-num">
                {summary ? summary.top_words.length : "—"}
              </div>
              <div className="pl-bigstat-label" style={{ marginTop: 8 }}>
                Tracked terms
              </div>
              <div className="pl-bigstat-sub">
                Words of length ≥4, filtered against common stopwords.
              </div>
            </div>
            <div className="pl-bigstat">
              <div className="pl-bigstat-num">
                {summary ? summary.total_words.toLocaleString() : "—"}
              </div>
              <div className="pl-bigstat-label" style={{ marginTop: 8 }}>
                Total words dictated
              </div>
              <div className="pl-bigstat-sub">
                {summary?.total_sessions ?? 0} sessions.
              </div>
            </div>
          </div>

          <div
            style={{
              display: "grid",
              gridTemplateColumns: "1fr 1fr",
              gap: 16,
              marginBottom: 20,
            }}
          >
            <div className="pl-card">
              <div className="pl-row between" style={{ marginBottom: 14 }}>
                <div
                  style={{
                    fontFamily: "var(--font-display)",
                    fontSize: 22,
                    color: "var(--fg)",
                  }}
                >
                  Desktop usage
                </div>
                <div
                  className="pl-section-label"
                  style={{ fontSize: 11, margin: 0 }}
                >
                  Total apps used · {appUsage.length}
                </div>
              </div>
              {appUsage.length === 0 ? (
                <div className="pl-muted" style={{ fontSize: 13 }}>
                  No session data yet.
                </div>
              ) : (
                appUsage.slice(0, 6).map((u) => (
                  <div className="pl-usage-row" key={u.app}>
                    <span style={{ fontSize: 14 }}>▣</span>
                    <div className="pl-usage-bar">
                      <div
                        className="pl-usage-bar-fill"
                        style={{ width: `${u.pct}%` }}
                      >
                        {u.pct >= 20 ? `${u.pct}%` : ""}
                      </div>
                    </div>
                    <div
                      style={{
                        fontSize: 12,
                        color: "var(--fg-dim)",
                        textTransform: "uppercase",
                        letterSpacing: "0.06em",
                      }}
                    >
                      {u.sessions} · {u.app || "unknown"}
                    </div>
                  </div>
                ))
              )}
            </div>

            <div className="pl-card">
              <div className="pl-row between" style={{ marginBottom: 14 }}>
                <div
                  style={{
                    fontFamily: "var(--font-display)",
                    fontSize: 22,
                    color: "var(--fg)",
                  }}
                >
                  {streak.current} day streak
                </div>
                <div
                  className="pl-section-label"
                  style={{ fontSize: 11, margin: 0 }}
                >
                  Longest · {streak.longest} day{streak.longest === 1 ? "" : "s"}
                </div>
              </div>
              <ActivityHeatmap sessions={summary?.recent ?? []} />
              <div
                style={{
                  marginTop: 10,
                  fontSize: 11,
                  color: "var(--fg-dim)",
                  display: "flex",
                  gap: 8,
                  alignItems: "center",
                }}
              >
                Less
                {[0, 1, 2, 3, 4].map((l) => (
                  <span
                    key={l}
                    className={`pl-heatmap-cell${l > 0 ? ` l${l}` : ""}`}
                  />
                ))}
                More
              </div>
            </div>
          </div>
        </>
      )}

      {tab === "voice" && (
        <section className="pl-section">
          <div
            style={{
              fontFamily: "var(--font-display)",
              fontSize: 22,
              color: "var(--fg)",
              marginBottom: 14,
            }}
          >
            Most-used words
          </div>
          <div className="pl-card">
            {!summary || summary.top_words.length === 0 ? (
              <div className="pl-muted" style={{ fontSize: 13 }}>
                Not enough data yet.
              </div>
            ) : (
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "repeat(2, 1fr)",
                  gap: 6,
                }}
              >
                {summary.top_words.map(([w, c]) => (
                  <div key={w} className="pl-row between" style={{ fontSize: 13 }}>
                    <span className="pl-mono">{w}</span>
                    <span style={{ color: "var(--fg-dim)" }}>{c}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        </section>
      )}

      <section className="pl-section" style={{ marginTop: 20 }}>
        <div className="pl-card">
          {confirmingClear ? (
            <div className="pl-row between">
              <span style={{ fontSize: 13 }}>
                Delete every session and word count? Cannot be undone.
              </span>
              <div style={{ display: "flex", gap: 8 }}>
                <button className="pl-btn" onClick={() => setConfirmingClear(false)}>
                  Cancel
                </button>
                <button className="pl-btn danger" onClick={clearAll}>
                  Delete all
                </button>
              </div>
            </div>
          ) : (
            <div className="pl-row between">
              <span style={{ fontSize: 12, color: "var(--fg-dim)" }}>
                Stored locally in{" "}
                <code className="pl-inline">
                  %APPDATA%/com.promptloom.desktop/insights/
                </code>
              </span>
              <button className="pl-btn" onClick={() => setConfirmingClear(true)}>
                Clear all stats
              </button>
            </div>
          )}
        </div>
      </section>
    </div>
  );
}

function aggregateAppUsage(sessions: SessionRecord[]) {
  const counts = new Map<string, number>();
  for (const s of sessions) {
    const k = s.app || "unknown";
    counts.set(k, (counts.get(k) ?? 0) + 1);
  }
  const total = sessions.length || 1;
  return [...counts.entries()]
    .map(([app, n]) => ({
      app,
      sessions: n,
      pct: Math.round((n / total) * 100),
    }))
    .sort((a, b) => b.sessions - a.sessions);
}

function computeStreak(sessions: SessionRecord[]) {
  if (sessions.length === 0) return { current: 0, longest: 0 };
  const days = new Set<string>();
  for (const s of sessions) {
    const d = new Date(s.ts * 1000);
    days.add(d.toISOString().slice(0, 10));
  }
  const sortedDays = [...days].sort();
  let longest = 1;
  let run = 1;
  for (let i = 1; i < sortedDays.length; i++) {
    const prev = new Date(sortedDays[i - 1] + "T00:00:00Z").getTime();
    const cur = new Date(sortedDays[i] + "T00:00:00Z").getTime();
    if (cur - prev === 86_400_000) {
      run++;
      longest = Math.max(longest, run);
    } else {
      run = 1;
    }
  }
  // Current streak ending today
  const today = new Date().toISOString().slice(0, 10);
  let current = 0;
  const walk = new Date(today + "T00:00:00Z");
  while (days.has(walk.toISOString().slice(0, 10))) {
    current++;
    walk.setUTCDate(walk.getUTCDate() - 1);
  }
  return { current, longest };
}

function ActivityHeatmap({ sessions }: { sessions: SessionRecord[] }) {
  // Show the last ~17 weeks (119 days), 7-row column grid.
  const DAYS = 17 * 7;
  const counts = new Map<string, number>();
  for (const s of sessions) {
    const d = new Date(s.ts * 1000).toISOString().slice(0, 10);
    counts.set(d, (counts.get(d) ?? 0) + 1);
  }
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const cells: { date: string; level: number; count: number }[] = [];
  for (let i = DAYS - 1; i >= 0; i--) {
    const d = new Date(today.getTime() - i * 86_400_000)
      .toISOString()
      .slice(0, 10);
    const c = counts.get(d) ?? 0;
    const level = c === 0 ? 0 : c < 2 ? 1 : c < 5 ? 2 : c < 10 ? 3 : 4;
    cells.push({ date: d, level, count: c });
  }
  return (
    <div className="pl-heatmap">
      {cells.map((c) => (
        <span
          key={c.date}
          title={`${c.date} · ${c.count} sessions`}
          className={`pl-heatmap-cell${c.level > 0 ? ` l${c.level}` : ""}`}
        />
      ))}
    </div>
  );
}

function StatTile({ label, value }: { label: string; value: string }) {
  return (
    <div
      style={{
        padding: "14px 16px",
        border: "1px solid var(--border)",
        borderRadius: 6,
        background: "var(--bg)",
      }}
    >
      <div
        style={{
          fontSize: 11,
          color: "var(--fg-dim)",
          textTransform: "uppercase",
          letterSpacing: "0.08em",
          marginBottom: 6,
        }}
      >
        {label}
      </div>
      <div
        style={{
          fontSize: 24,
          fontFamily: "var(--font-display)",
          color: "var(--fg)",
        }}
      >
        {value}
      </div>
    </div>
  );
}

function SessionSparkline({ sessions }: { sessions: SessionRecord[] }) {
  if (sessions.length === 0) {
    return <div className="pl-muted">No sessions yet.</div>;
  }
  const max = Math.max(...sessions.map((s) => s.word_count), 1);
  // Oldest on left, newest on right (reverse the recent[] which is newest-first)
  const ordered = [...sessions].reverse();
  return (
    <div
      style={{
        display: "flex",
        alignItems: "flex-end",
        gap: 4,
        height: 80,
      }}
    >
      {ordered.map((s, i) => {
        const h = Math.max(4, (s.word_count / max) * 76);
        return (
          <div
            key={i}
            title={`${s.word_count} words · ${(s.duration_ms / 1000).toFixed(1)}s${s.app ? ` · ${s.app}` : ""}`}
            style={{
              flex: 1,
              height: h,
              background: "var(--accent)",
              borderRadius: 2,
              opacity: 0.75,
            }}
          />
        );
      })}
    </div>
  );
}

function SnippetsPage() {
  const [snippets, setSnippets] = useState<[string, string][]>([]);
  const [name, setName] = useState("");
  const [body, setBody] = useState("");
  const [filter, setFilter] = useState("");
  const [err, setErr] = useState<string | null>(null);

  const refresh = () =>
    invoke<[string, string][]>("list_snippets")
      .then(setSnippets)
      .catch((e) => setErr(String(e)));

  useEffect(() => {
    refresh();
  }, []);

  const add = async () => {
    setErr(null);
    try {
      const next = await invoke<[string, string][]>("add_snippet", {
        name,
        body,
      });
      setSnippets(next);
      setName("");
      setBody("");
    } catch (e) {
      setErr(String(e));
    }
  };

  const remove = async (n: string) => {
    try {
      const next = await invoke<[string, string][]>("remove_snippet", {
        name: n,
      });
      setSnippets(next);
    } catch (e) {
      setErr(String(e));
    }
  };

  const filtered = snippets.filter(
    ([n, b]) =>
      !filter ||
      n.toLowerCase().includes(filter.toLowerCase()) ||
      b.toLowerCase().includes(filter.toLowerCase())
  );

  const recentSnips = snippets.slice(0, 4);
  return (
    <div className="pl-stack" style={{ maxWidth: 940 }}>
      <div className="pl-row between" style={{ marginBottom: 20 }}>
        <h2
          style={{
            fontFamily: "var(--font-display)",
            fontStyle: "italic",
            fontWeight: 500,
            fontSize: 34,
            margin: 0,
            color: "var(--fg)",
          }}
        >
          Snippets
        </h2>
        <button
          className="pl-btn primary"
          onClick={() => {
            const el = document.getElementById("snippet-name-input");
            if (el) (el as HTMLInputElement).focus();
          }}
        >
          Add new
        </button>
      </div>

      <div className="pl-hero">
        <h3>
          The stuff <em>you</em> shouldn't have to re-type.
        </h3>
        <p>
          Save text you dictate often — an email intro, a prompt template, a
          code block — then say "insert &lt;name&gt;" to drop it in instantly.
        </p>
        <div
          className="pl-chip-row"
          style={{ flexDirection: "column", alignItems: "flex-start" }}
        >
          {recentSnips.length > 0 ? (
            recentSnips.map(([n, b]) => (
              <div
                key={n}
                style={{ display: "flex", alignItems: "center", gap: 8 }}
              >
                <span className="pl-chip pl-mono">"insert {n}"</span>
                <span style={{ color: "var(--fg-dim)" }}>→</span>
                <span
                  className="pl-chip"
                  style={{
                    maxWidth: 360,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {b}
                </span>
              </div>
            ))
          ) : (
            <>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span className="pl-chip pl-mono">"insert linkedin"</span>
                <span style={{ color: "var(--fg-dim)" }}>→</span>
                <span className="pl-chip">https://linkedin.com/in/you</span>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span className="pl-chip pl-mono">"insert signoff"</span>
                <span style={{ color: "var(--fg-dim)" }}>→</span>
                <span className="pl-chip">
                  Thanks,{" "}— you
                </span>
              </div>
            </>
          )}
          <span
            className="pl-chip btn"
            onClick={() => {
              const el = document.getElementById("snippet-name-input");
              if (el) (el as HTMLInputElement).focus();
            }}
            style={{ marginTop: 6 }}
          >
            + Add new snippet
          </span>
        </div>
      </div>

      <section className="pl-section">
        <div className="pl-section-label">Add a snippet</div>
        <div className="pl-card">
          <div style={{ display: "flex", gap: 8, marginBottom: 8 }}>
            <input
              id="snippet-name-input"
              className="pl-input pl-mono"
              style={{ flex: 1 }}
              placeholder="name (one word)"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <textarea
            value={body}
            onChange={(e) => setBody(e.target.value)}
            placeholder="Body — multiple lines OK."
            style={{
              width: "100%",
              minHeight: 100,
              background: "var(--bg)",
              color: "var(--fg)",
              border: "1px solid var(--border)",
              borderRadius: 6,
              padding: 10,
              fontSize: 13,
              fontFamily: "var(--font-body)",
              lineHeight: 1.55,
              resize: "vertical",
              outline: "none",
              marginBottom: 10,
            }}
          />
          <button
            className="pl-btn primary"
            onClick={add}
            disabled={!name.trim() || !body.trim()}
          >
            Add snippet
          </button>
          {err && (
            <div className="pl-notice danger" style={{ marginTop: 10 }}>
              {err}
            </div>
          )}
        </div>
      </section>

      <section className="pl-section">
        <div className="pl-row between" style={{ marginBottom: 10 }}>
          <div className="pl-section-label">Saved ({snippets.length})</div>
          <input
            className="pl-input"
            placeholder="Filter…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            style={{ width: 180 }}
          />
        </div>
        {filtered.length === 0 ? (
          <div className="pl-muted" style={{ fontSize: 13 }}>
            {snippets.length === 0
              ? "No snippets yet. Add one above."
              : "No snippets match that filter."}
          </div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            {filtered.map(([n, b]) => (
              <div
                key={n}
                style={{
                  padding: "10px 12px",
                  border: "1px solid var(--border)",
                  borderRadius: 6,
                }}
              >
                <div
                  className="pl-row between"
                  style={{ alignItems: "flex-start" }}
                >
                  <span className="pl-mono" style={{ fontSize: 13 }}>
                    insert {n}
                  </span>
                  <button className="pl-btn" onClick={() => remove(n)}>
                    Remove
                  </button>
                </div>
                <pre
                  className="pl-mono"
                  style={{
                    fontSize: 12,
                    color: "var(--fg-dim)",
                    marginTop: 6,
                    marginBottom: 0,
                    whiteSpace: "pre-wrap",
                    wordBreak: "break-word",
                  }}
                >
                  {b}
                </pre>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function ScratchpadPage() {
  const [text, setText] = useState("");
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const unlisten = listen<string>("promptloom:transcript", (e) => {
      if (!e.payload) return;
      setText((prev) => (prev ? prev + " " + e.payload : e.payload));
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  };

  return (
    <div className="pl-stack" style={{ maxWidth: 940 }}>
      <div className="pl-row between" style={{ marginBottom: 20 }}>
        <h2
          style={{
            fontFamily: "var(--font-display)",
            fontStyle: "italic",
            fontWeight: 500,
            fontSize: 34,
            margin: 0,
            color: "var(--fg)",
          }}
        >
          Scratchpad
          <span className="pl-beta">Beta</span>
        </h2>
      </div>

      <div className="pl-hero">
        <h3>
          For quick thoughts <em>you</em> want to come back to.
        </h3>
        <p>
          Drop a to-do, polish a message before you send it, brain-dump an
          idea. The pad only lives for this session — close the app and it's
          gone. Copy the result when you're done.
        </p>
      </div>

      <div className="pl-card">
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Hold your hotkey and speak. Text lands here."
          style={{
            width: "100%",
            minHeight: 320,
            background: "var(--bg)",
            color: "var(--fg)",
            border: "1px solid var(--border)",
            borderRadius: 6,
            padding: 12,
            fontSize: 14,
            fontFamily: "var(--font-body)",
            lineHeight: 1.6,
            resize: "vertical",
            outline: "none",
          }}
        />
        <div
          style={{
            display: "flex",
            gap: 8,
            marginTop: 12,
            alignItems: "center",
          }}
        >
          <button className="pl-btn primary" onClick={copy} disabled={!text}>
            {copied ? "Copied" : "Copy all"}
          </button>
          <button
            className="pl-btn"
            onClick={() => setText("")}
            disabled={!text}
          >
            Clear
          </button>
          <span
            style={{
              marginLeft: "auto",
              fontSize: 12,
              color: "var(--fg-dim)",
            }}
          >
            {text.length} chars · {text.trim() ? text.trim().split(/\s+/).length : 0}{" "}
            words
          </span>
        </div>
      </div>
    </div>
  );
}

function AppProfilesSection() {
  const [profiles, setProfiles] = useState<[string, string][]>([]);
  const [exe, setExe] = useState("");
  const [tone, setTone] = useState<"casual" | "formal">("casual");
  const [err, setErr] = useState<string | null>(null);

  const refresh = () =>
    invoke<[string, string][]>("list_app_profiles")
      .then(setProfiles)
      .catch((e) => setErr(String(e)));

  useEffect(() => {
    refresh();
  }, []);

  const add = async () => {
    if (!exe.trim()) return;
    setErr(null);
    try {
      const next = await invoke<[string, string][]>("set_app_profile", {
        exe: exe.trim(),
        tone,
      });
      setProfiles(next);
      setExe("");
    } catch (e) {
      setErr(String(e));
    }
  };

  const remove = async (e: string) => {
    try {
      const next = await invoke<[string, string][]>("remove_app_profile", {
        exe: e,
      });
      setProfiles(next);
    } catch (err) {
      setErr(String(err));
    }
  };

  return (
    <section className="pl-section">
      <div className="pl-section-label">Per-app style (optional)</div>
      <div className="pl-card">
        <div className="pl-card-body" style={{ marginBottom: 12 }}>
          Give specific apps a tone. The bundled polish model rewrites your
          transcript in that register before it's pasted. Only applies when
          the app isn't already detected as Code or Terminal. Example:{" "}
          <code className="pl-inline">slack.exe</code> → casual,{" "}
          <code className="pl-inline">outlook.exe</code> → formal.
        </div>

        <div style={{ display: "flex", gap: 8, marginBottom: 12 }}>
          <input
            className="pl-input pl-mono"
            style={{ flex: 1 }}
            placeholder="slack.exe"
            value={exe}
            onChange={(ev) => setExe(ev.target.value)}
            onKeyDown={(ev) => {
              if (ev.key === "Enter") add();
            }}
          />
          <select
            className="pl-input"
            value={tone}
            onChange={(ev) => setTone(ev.target.value as "casual" | "formal")}
          >
            <option value="casual">casual</option>
            <option value="formal">formal</option>
          </select>
          <button className="pl-btn primary" onClick={add}>
            Add
          </button>
        </div>

        {err && (
          <div className="pl-notice danger" style={{ marginBottom: 12 }}>
            {err}
          </div>
        )}

        {profiles.length === 0 ? (
          <div className="pl-muted" style={{ fontSize: 13 }}>
            No per-app profiles yet. Unlisted apps use the neutral default.
          </div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            {profiles.map(([e, t]) => (
              <div
                key={e}
                className="pl-row between"
                style={{
                  padding: "8px 12px",
                  border: "1px solid var(--border)",
                  borderRadius: 6,
                }}
              >
                <div>
                  <span className="pl-mono" style={{ fontSize: 13 }}>
                    {e}
                  </span>
                  <span
                    style={{
                      marginLeft: 10,
                      color: "var(--fg-dim)",
                      fontSize: 12,
                    }}
                  >
                    → {t}
                  </span>
                </div>
                <button className="pl-btn" onClick={() => remove(e)}>
                  Remove
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}

function DictionaryPage({
  setNotices,
}: {
  setNotices: React.Dispatch<React.SetStateAction<string[]>>;
}) {
  const [terms, setTerms] = useState<[string, string][]>([]);
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [filter, setFilter] = useState("");
  const [saving, setSaving] = useState(false);

  const refresh = () =>
    invoke<[string, string][]>("list_vocab_terms")
      .then((t) => setTerms(t))
      .catch((e) => setNotices((n) => [`list_vocab_terms: ${e}`, ...n]));

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const add = async () => {
    if (!from.trim() || !to.trim()) return;
    setSaving(true);
    try {
      const next = await invoke<[string, string][]>("add_vocab_term", {
        from: from.trim(),
        to: to.trim(),
      });
      setTerms(next);
      setFrom("");
      setTo("");
    } catch (e) {
      setNotices((n) => [`add_vocab_term: ${e}`, ...n]);
    } finally {
      setSaving(false);
    }
  };

  const remove = async (f: string) => {
    try {
      const next = await invoke<[string, string][]>("remove_vocab_term", {
        from: f,
      });
      setTerms(next);
    } catch (e) {
      setNotices((n) => [`remove_vocab_term: ${e}`, ...n]);
    }
  };

  const visible = filter.trim()
    ? terms.filter(
        ([f, t]) =>
          f.toLowerCase().includes(filter.toLowerCase()) ||
          t.toLowerCase().includes(filter.toLowerCase())
      )
    : terms;

  const recentChips = terms.slice(0, 5);

  return (
    <div style={{ maxWidth: 940, margin: "0 auto" }}>
      <div className="pl-row between" style={{ marginBottom: 20 }}>
        <h1
          style={{
            fontFamily: "var(--font-display)",
            fontStyle: "italic",
            fontWeight: 500,
            fontSize: 34,
            margin: 0,
            letterSpacing: "-0.02em",
          }}
        >
          Dictionary
        </h1>
        <button
          className="pl-btn primary"
          onClick={() => {
            const el = document.getElementById("vocab-from-input");
            if (el) (el as HTMLInputElement).focus();
          }}
        >
          Add new
        </button>
      </div>

      <div className="pl-hero">
        <h3>
          Jot spells the way <em>you</em> do.
        </h3>
        <p>
          Correct a spelling once or add it here manually, so your{" "}
          <strong style={{ color: "var(--fg)" }}>
            personal terms, company jargon, or uncommon names
          </strong>{" "}
          are spelled right in both dictations and meeting notes.
        </p>
        <div className="pl-chip-row">
          <span
            className="pl-chip btn"
            onClick={() => {
              const el = document.getElementById("vocab-from-input");
              if (el) (el as HTMLInputElement).focus();
            }}
          >
            + Add new word
          </span>
          {recentChips.length > 0 ? (
            recentChips.map(([f, t]) => (
              <span key={f} className="pl-chip" title={`→ ${t}`}>
                {t}
              </span>
            ))
          ) : (
            <>
              <span className="pl-chip">Supabase</span>
              <span className="pl-chip">Jot</span>
              <span className="pl-chip">Qwen</span>
            </>
          )}
        </div>
      </div>

      <div className="pl-card" style={{ marginBottom: 16 }}>
        <div className="pl-card-head" style={{ marginBottom: 12 }}>
          Add a term
        </div>
        <div
          className="pl-row"
          style={{ gap: 10, alignItems: "flex-end", flexWrap: "wrap" }}
        >
          <div style={{ flex: 1, minWidth: 180 }}>
            <div className="pl-section-label" style={{ marginBottom: 4 }}>
              You say
            </div>
            <input
              id="vocab-from-input"
              className="pl-input"
              style={{ width: "100%", minWidth: 0 }}
              value={from}
              placeholder="super base"
              onChange={(e) => setFrom(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") add();
              }}
            />
          </div>
          <div
            style={{
              fontSize: 18,
              color: "var(--fg-very-dim)",
              paddingBottom: 10,
            }}
          >
            →
          </div>
          <div style={{ flex: 1, minWidth: 180 }}>
            <div className="pl-section-label" style={{ marginBottom: 4 }}>
              We type
            </div>
            <input
              className="pl-input"
              style={{ width: "100%", minWidth: 0 }}
              value={to}
              placeholder="Supabase"
              onChange={(e) => setTo(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") add();
              }}
            />
          </div>
          <button
            type="button"
            className="pl-btn primary"
            onClick={add}
            disabled={saving || !from.trim() || !to.trim()}
          >
            {saving ? "Adding…" : "Add"}
          </button>
        </div>
      </div>

      <div
        className="pl-row between"
        style={{ marginBottom: 10, alignItems: "baseline" }}
      >
        <div className="pl-section-label">
          {visible.length} of {terms.length} term
          {terms.length === 1 ? "" : "s"}
        </div>
        {terms.length > 0 && (
          <input
            className="pl-input"
            style={{ minWidth: 220, fontSize: 12.5 }}
            placeholder="Filter…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
        )}
      </div>

      {terms.length === 0 ? (
        <div className="pl-hist-empty">
          Your dictionary is empty. Add a term above to start.
        </div>
      ) : (
        <div className="pl-card" style={{ padding: 0 }}>
          {visible.map(([f, t]) => (
            <div
              key={f}
              style={{
                display: "grid",
                gridTemplateColumns: "1fr auto 1fr auto",
                alignItems: "center",
                gap: 12,
                padding: "10px 16px",
                borderBottom: "1px solid var(--border)",
              }}
            >
              <div className="pl-mono" style={{ fontSize: 13 }}>
                {f}
              </div>
              <div
                style={{ color: "var(--fg-very-dim)", fontSize: 14 }}
              >
                →
              </div>
              <div className="pl-mono" style={{ fontSize: 13 }}>
                {t}
              </div>
              <button
                type="button"
                className="pl-btn danger"
                onClick={() => remove(f)}
                style={{ padding: "4px 10px", fontSize: 11 }}
              >
                Remove
              </button>
            </div>
          ))}
          {visible.length === 0 && (
            <div className="pl-hist-empty" style={{ padding: "24px 16px" }}>
              No matches for "{filter}".
            </div>
          )}
        </div>
      )}
    </div>
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
          <label
            style={{
              display: "flex",
              gap: 10,
              alignItems: "flex-start",
              cursor: "pointer",
              marginBottom: 14,
            }}
          >
            <input
              type="checkbox"
              checked={settings?.auto_context ?? true}
              onChange={async (e) => {
                try {
                  const next = await invoke<Settings>("set_auto_context", {
                    enabled: e.target.checked,
                  });
                  setSettings(next);
                } catch (err) {
                  setNotices((n) => [`set_auto_context error: ${err}`, ...n]);
                }
              }}
            />
            <div>
              <div style={{ fontWeight: 600 }}>
                Auto-detect from active app
              </div>
              <div className="pl-card-body" style={{ marginTop: 2 }}>
                When on, Jot picks Code or Terminal mode when it
                recognizes editors (VS Code, Cursor, JetBrains…) or shells
                (Windows Terminal, PowerShell, WezTerm…). For unknown apps,
                falls back to your manual pick below.
              </div>
            </div>
          </label>

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

      {/* Bundled polish (primary) */}
      <section className="pl-section">
        <div className="pl-section-label">Transcript polish</div>
        <div className="pl-card">
          <div className="pl-card-body" style={{ marginBottom: 12 }}>
            After the normal pipeline runs, Jot cleans the transcript
            with a <strong style={{ color: "var(--fg)" }}>local LLM</strong>{" "}
            bundled with the app. Fillers get stripped, awkward phrasing
            gets smoothed, and your meaning stays intact. Hard 2-second
            timeout — if the model is slow, you get the raw transcript. All
            inference stays on your PC.
          </div>

          <label
            style={{
              display: "flex",
              gap: 10,
              alignItems: "center",
              cursor: "pointer",
            }}
          >
            <input
              type="checkbox"
              checked={settings?.bundled_polish_enabled ?? true}
              onChange={async (e) => {
                try {
                  const next = await invoke<Settings>(
                    "set_bundled_polish_enabled",
                    { enabled: e.target.checked }
                  );
                  setSettings(next);
                } catch (err) {
                  setNotices((n) => [
                    `set_bundled_polish_enabled error: ${err}`,
                    ...n,
                  ]);
                }
              }}
            />
            <span style={{ fontSize: 13 }}>
              Clean transcripts with the local polish model
            </span>
          </label>

          <PolishModelDownloader />

          <div className="pl-notice" style={{ marginTop: 12 }}>
            <code className="pl-inline">llama-server.exe</code> must be
            reachable (next to the model, in <code className="pl-inline">tools/</code>,
            or via <code className="pl-inline">PROMPTLOOM_LLAMA_SERVER_BIN</code>).
            Model downloads are SHA-256 verified against{" "}
            <code className="pl-inline">polish-manifest.json</code>.
          </div>
        </div>
      </section>

      {/* Advanced: Ollama fallthrough */}
      <section className="pl-section">
        <div className="pl-section-label">Advanced · Custom LLM via Ollama</div>
        <div className="pl-card">
          <div className="pl-card-body" style={{ marginBottom: 12 }}>
            After the normal pipeline runs, Jot can send the transcript
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
                  0 wav · 0 log · in-memory streaming (0 disk writes) · clean
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
