import { useEffect, useState } from "react";
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
};

type Settings = {
  project_path: string | null;
  terms_count: number;
};

type PrivacyReport = {
  scratch_dir: string;
  residual_wavs: string[];
  residual_logs: string[];
};

export function App() {
  const [onboardingNeeded, setOnboardingNeeded] = useState<boolean | null>(null);
  const [state, setState] = useState<string>("");
  const [notices, setNotices] = useState<string[]>([]);
  const [model, setModel] = useState<ModelStatus | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [progress, setProgress] = useState<{ bytes: number; total: number } | null>(
    null
  );
  const [downloading, setDownloading] = useState(false);
  const [transcript, setTranscript] = useState<string>("");
  const [privacy, setPrivacy] = useState<PrivacyReport | null>(null);

  const refreshModel = () => {
    invoke<ModelStatus>("model_status")
      .then(setModel)
      .catch((e) => setNotices((n) => [`model_status error: ${e}`, ...n]));
  };

  const refreshSettings = () => {
    invoke<Settings>("get_settings").then(setSettings).catch(() => void 0);
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
    const unlistenState = listen<string>("promptloom:state", (e) =>
      setState(e.payload)
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
      setTranscript(e.payload);
      window.setTimeout(() => setTranscript(""), 6000);
    });
    invoke<string>("current_state").then(setState).catch(() => void 0);
    return () => {
      unlistenState.then((u) => u());
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

  if (onboardingNeeded === null) {
    return (
      <main style={{ fontFamily: "system-ui", padding: 24 }}>Loading…</main>
    );
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

  return (
    <main style={{ fontFamily: "system-ui", padding: 24, maxWidth: 720 }}>
      <h1 style={{ margin: 0 }}>PromptLoom</h1>
      <p style={{ color: "#555" }}>
        Local-only Windows dictation. <b>M4 Vocabulary + dogfood</b> — real
        whisper.cpp transcription, clipboard transaction + Ctrl+V to the
        captured target, project vocabulary from <code>.promptloom/terms.yml</code>,
        settings persisted, privacy audit built in.
      </p>

      <section style={{ marginTop: 16 }}>
        <h2 style={{ fontSize: 16 }}>Model</h2>
        {model ? (
          <div
            style={{
              background: "#f8f8fa",
              border: "1px solid #e4e4e8",
              padding: 12,
              borderRadius: 6,
            }}
          >
            <div>
              <b>{model.name}</b>{" "}
              <span style={{ color: "#888" }}>
                ({(model.size_bytes / 1_000_000).toFixed(1)} MB)
              </span>
            </div>
            <div style={{ fontSize: 12, color: "#666", wordBreak: "break-all" }}>
              {model.path}
            </div>
            <div style={{ marginTop: 6 }}>
              Status:{" "}
              <code>
                {model.installed
                  ? model.verified
                    ? "installed (SHA-256 verified)"
                    : "installed — hash MISMATCH"
                  : "not installed"}
              </code>
            </div>
            {!model.installed && (
              <button
                type="button"
                disabled={downloading}
                onClick={() => {
                  setDownloading(true);
                  setProgress({ bytes: 0, total: model.size_bytes });
                  invoke("download_model_cmd").catch((e) => {
                    setNotices((n) => [`download error: ${e}`, ...n]);
                    setDownloading(false);
                  });
                }}
                style={{ marginTop: 8 }}
              >
                {downloading
                  ? `Downloading… ${pct}%`
                  : "Download model (one-time)"}
              </button>
            )}
            {downloading && progress && (
              <div style={{ marginTop: 8 }}>
                <div
                  style={{
                    width: "100%",
                    height: 6,
                    background: "#eee",
                    borderRadius: 3,
                    overflow: "hidden",
                  }}
                >
                  <div
                    style={{
                      width: `${pct}%`,
                      height: "100%",
                      background: "#2ecc71",
                      transition: "width 200ms linear",
                    }}
                  />
                </div>
                <div style={{ fontSize: 11, color: "#777", marginTop: 2 }}>
                  {(progress.bytes / 1_000_000).toFixed(1)} /{" "}
                  {(progress.total / 1_000_000).toFixed(1)} MB
                </div>
              </div>
            )}
          </div>
        ) : (
          <div style={{ color: "#888" }}>loading…</div>
        )}
      </section>

      <section style={{ marginTop: 16 }}>
        <h2 style={{ fontSize: 16 }}>Project vocabulary</h2>
        <div
          style={{
            background: "#f8f8fa",
            border: "1px solid #e4e4e8",
            padding: 12,
            borderRadius: 6,
          }}
        >
          <div style={{ fontSize: 13, marginBottom: 8 }}>
            Choose a project folder containing{" "}
            <code>.promptloom/terms.yml</code>. Corrections are applied after
            transcription, before paste.
          </div>
          {settings?.project_path ? (
            <>
              <div
                style={{
                  fontFamily: "monospace",
                  fontSize: 12,
                  wordBreak: "break-all",
                }}
              >
                {settings.project_path}
              </div>
              <div style={{ fontSize: 12, marginTop: 4 }}>
                Loaded <b>{settings.terms_count}</b> term
                {settings.terms_count === 1 ? "" : "s"}.
              </div>
              <div style={{ marginTop: 8, display: "flex", gap: 8 }}>
                <button type="button" onClick={pickProject}>
                  Change folder
                </button>
                <button type="button" onClick={clearProject}>
                  Clear
                </button>
              </div>
            </>
          ) : (
            <button type="button" onClick={pickProject}>
              Choose project folder…
            </button>
          )}
        </div>
      </section>

      <section style={{ marginTop: 16 }}>
        <h2 style={{ fontSize: 16 }}>How to use</h2>
        <ol style={{ lineHeight: 1.6 }}>
          <li>
            Hold <kbd>F8</kbd> and speak. The HUD shows a live level meter.
          </li>
          <li>
            Release — transcript is corrected via your vocabulary, pasted into
            the window that was focused.
          </li>
          <li>
            Press <kbd>Esc</kbd> while holding to cancel. Audio discarded.
          </li>
        </ol>
      </section>

      {transcript && (
        <section style={{ marginTop: 16 }}>
          <h2 style={{ fontSize: 16 }}>Last transcript</h2>
          <div
            style={{
              background: "#fffbe6",
              border: "1px solid #ffe58f",
              padding: 10,
              borderRadius: 6,
              fontFamily: "monospace",
            }}
          >
            {transcript}
          </div>
          <div style={{ fontSize: 11, color: "#777", marginTop: 4 }}>
            Not persisted. Auto-clears in a few seconds.
          </div>
        </section>
      )}

      <section style={{ marginTop: 16 }}>
        <h2 style={{ fontSize: 16 }}>Privacy audit</h2>
        <div
          style={{
            background: "#f8f8fa",
            border: "1px solid #e4e4e8",
            padding: 12,
            borderRadius: 6,
          }}
        >
          <div style={{ fontSize: 13, marginBottom: 8 }}>
            Scans the scratch directory for any residual audio or logs.
            Expectation: zero after every dictation.
          </div>
          <button type="button" onClick={runPrivacyAudit}>
            Run audit now
          </button>
          {privacy && (
            <div style={{ marginTop: 8, fontSize: 12 }}>
              <div style={{ color: "#555" }}>
                scratch: <code>{privacy.scratch_dir}</code>
              </div>
              <div
                style={{
                  marginTop: 4,
                  color:
                    privacy.residual_wavs.length === 0 &&
                    privacy.residual_logs.length === 0
                      ? "#2ecc71"
                      : "crimson",
                  fontWeight: 600,
                }}
              >
                {privacy.residual_wavs.length} wav,{" "}
                {privacy.residual_logs.length} log file
                {privacy.residual_wavs.length + privacy.residual_logs.length ===
                1
                  ? ""
                  : "s"}
              </div>
              {privacy.residual_wavs.map((p) => (
                <div key={p} style={{ fontFamily: "monospace", color: "crimson" }}>
                  wav: {p}
                </div>
              ))}
              {privacy.residual_logs.map((p) => (
                <div key={p} style={{ fontFamily: "monospace", color: "crimson" }}>
                  log: {p}
                </div>
              ))}
            </div>
          )}
        </div>
      </section>

      <section style={{ marginTop: 16 }}>
        <h2 style={{ fontSize: 16 }}>Live state</h2>
        <code style={{ background: "#f4f4f4", padding: "4px 8px" }}>
          {state || "(waiting)"}
        </code>
      </section>

      <section style={{ marginTop: 16 }}>
        <h2 style={{ fontSize: 16 }}>Recent notices</h2>
        {notices.length === 0 ? (
          <p style={{ color: "#888" }}>Nothing yet.</p>
        ) : (
          <ul style={{ fontFamily: "monospace", fontSize: 12 }}>
            {notices.map((n, i) => (
              <li key={i}>{n}</li>
            ))}
          </ul>
        )}
      </section>

      <section style={{ marginTop: 24, fontSize: 12, color: "#666" }}>
        All M0–M4 milestones complete per build spec.
      </section>
    </main>
  );
}
