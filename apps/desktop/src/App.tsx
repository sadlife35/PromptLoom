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

const PHASE_COLOR: Record<Phase["kind"], string> = {
  ready: "#9aa5b1",
  recording: "#e74c3c",
  processing: "#f39c12",
  delivered: "#2ecc71",
  copied_only: "#3498db",
  cancelled: "#9aa5b1",
  error: "#e74c3c",
};

export function App() {
  const [onboardingNeeded, setOnboardingNeeded] = useState<boolean | null>(null);
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
  const [transcript, setTranscript] = useState<string>("");
  const [privacy, setPrivacy] = useState<PrivacyReport | null>(null);
  const [verifyResult, setVerifyResult] = useState<VerifyReport | null>(null);
  const [removeConfirm, setRemoveConfirm] = useState(false);
  const [removeInFlight, setRemoveInFlight] = useState(false);
  const [availableModels, setAvailableModels] = useState<ModelSummary[]>([]);

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
      setTranscript(e.payload);
      window.setTimeout(() => setTranscript(""), 6000);
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
        <h2 style={{ fontSize: 16 }}>Models</h2>
        {availableModels.length > 1 && (
          <div
            style={{
              background: "#f8f8fa",
              border: "1px solid #e4e4e8",
              padding: 12,
              borderRadius: 6,
              marginBottom: 10,
            }}
          >
            <div style={{ fontSize: 13, marginBottom: 8 }}>
              Speech model (used for all dictations):
            </div>
            <select
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
              style={{ padding: 6, minWidth: 320 }}
            >
              {availableModels.map((m) => (
                <option key={m.name} value={m.name}>
                  {m.name} — {(m.size_bytes / 1_000_000).toFixed(0)} MB
                  {m.installed ? " · installed" : " · not installed"}
                </option>
              ))}
            </select>
            {availableModels.find((m) => m.is_active)?.note && (
              <div style={{ fontSize: 12, color: "#666", marginTop: 6 }}>
                {availableModels.find((m) => m.is_active)?.note}
              </div>
            )}
          </div>
        )}
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
                ({(model.size_bytes / 1_000_000).toFixed(1)} MB · active)
              </span>
            </div>
            <div style={{ fontSize: 12, color: "#666", wordBreak: "break-all" }}>
              {model.path}
            </div>
            <div style={{ marginTop: 8, fontSize: 13 }}>
              <div>
                Weights:{" "}
                <code>
                  {model.installed
                    ? model.verified
                      ? "installed (SHA-256 verified)"
                      : "installed — hash MISMATCH"
                    : "not installed"}
                </code>
              </div>
              <div>
                Engine:{" "}
                <code>
                  {model.engine_installed
                    ? "whisper.cpp binary present"
                    : "whisper-cli.exe missing"}
                </code>
                {model.engine_path && (
                  <div
                    style={{
                      fontSize: 11,
                      color: "#888",
                      wordBreak: "break-all",
                    }}
                  >
                    {model.engine_path}
                  </div>
                )}
              </div>
            </div>

            {!model.installed && (
              <div style={{ marginTop: 10 }}>
                <div style={{ fontSize: 12, color: "#555", marginBottom: 6 }}>
                  Model downloads happen only after you click below. Nothing is
                  fetched automatically.
                </div>
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
                >
                  {downloading
                    ? `Downloading… ${pct}%`
                    : "Download model (one-time)"}
                </button>
              </div>
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

            {model.installed && (
              <div style={{ marginTop: 12, display: "flex", gap: 8 }}>
                <button
                  type="button"
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
                  onClick={() => setRemoveConfirm(true)}
                  style={{ color: "#a40000" }}
                >
                  Remove downloaded model
                </button>
              </div>
            )}

            {verifyResult && (
              <div
                style={{
                  marginTop: 8,
                  padding: 8,
                  background: verifyResult.ok ? "#f0fff4" : "#fff1f0",
                  border:
                    "1px solid " + (verifyResult.ok ? "#95de64" : "#ffa39e"),
                  borderRadius: 4,
                  fontSize: 12,
                }}
              >
                {verifyResult.message}
                <div style={{ color: "#888", marginTop: 4, fontSize: 11 }}>
                  Verification is a local SHA-256 check — no network calls.
                </div>
              </div>
            )}

            {removeConfirm && (
              <div
                style={{
                  marginTop: 8,
                  padding: 10,
                  background: "#fff8e6",
                  border: "1px solid #ffd666",
                  borderRadius: 4,
                  fontSize: 13,
                }}
              >
                <div style={{ marginBottom: 6 }}>
                  <b>Remove the downloaded model?</b> This deletes only the
                  file inside PromptLoom's managed model directory. The
                  whisper engine binary and your settings are not touched.
                  You'll need to re-download the model (~
                  {(model.size_bytes / 1_000_000).toFixed(0)} MB) to dictate
                  again.
                </div>
                <div style={{ display: "flex", gap: 8 }}>
                  <button
                    type="button"
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
                        // Removal flips onboarding_complete=false, so route
                        // back to the wizard.
                        invoke<boolean>("needs_onboarding").then(
                          setOnboardingNeeded
                        );
                      } catch (e) {
                        setNotices((n) => [`remove error: ${e}`, ...n]);
                      } finally {
                        setRemoveInFlight(false);
                      }
                    }}
                    style={{ color: "#a40000" }}
                  >
                    {removeInFlight ? "Removing…" : "Yes, remove"}
                  </button>
                  <button
                    type="button"
                    onClick={() => setRemoveConfirm(false)}
                    disabled={removeInFlight}
                  >
                    Cancel
                  </button>
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
        <h2 style={{ fontSize: 16 }}>Dictation controls</h2>
        <div
          style={{
            background: "#f8f8fa",
            border: "1px solid #e4e4e8",
            padding: 12,
            borderRadius: 6,
          }}
        >
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <label style={{ display: "flex", gap: 8, alignItems: "flex-start" }}>
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
                <b>Hold-to-talk</b>{" "}
                <span style={{ color: "#888" }}>(recommended)</span>
                <div style={{ fontSize: 12, color: "#666" }}>
                  Hold <kbd>{settings?.hotkey ?? "F8"}</kbd> while speaking,
                  release to transcribe. Simplest and least likely to leave a
                  session hanging.
                </div>
              </div>
            </label>
            <label style={{ display: "flex", gap: 8, alignItems: "flex-start" }}>
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
                <b>Toggle</b>
                <div style={{ fontSize: 12, color: "#666" }}>
                  Tap <kbd>{settings?.hotkey ?? "F8"}</kbd> to start recording,
                  tap again to stop. Good for longer dictations where holding
                  gets tiring.
                </div>
              </div>
            </label>
            {phase.kind !== "ready" && (
              <div style={{ fontSize: 11, color: "#a76d00" }}>
                Mode changes are disabled while a session is in progress.
              </div>
            )}
          </div>
          <div style={{ fontSize: 11, color: "#888", marginTop: 10 }}>
            <kbd>Esc</kbd> always cancels — in both modes.
          </div>
        </div>
      </section>

      <section style={{ marginTop: 16 }}>
        <h2 style={{ fontSize: 16 }}>How to use</h2>
        <ol style={{ lineHeight: 1.6 }}>
          {settings?.dictation_mode === "toggle" ? (
            <>
              <li>
                Tap <kbd>{settings?.hotkey ?? "F8"}</kbd> to start recording.
                The HUD shows a live level meter.
              </li>
              <li>
                Tap <kbd>{settings?.hotkey ?? "F8"}</kbd> again to stop and
                transcribe. The result is pasted into the window that was
                focused when you started.
              </li>
              <li>
                Press <kbd>Esc</kbd> to cancel. Audio discarded.
              </li>
            </>
          ) : (
            <>
              <li>
                Hold <kbd>{settings?.hotkey ?? "F8"}</kbd> and speak. The HUD
                shows a live level meter.
              </li>
              <li>
                Release — transcript is corrected via your vocabulary and
                pasted into the window that was focused.
              </li>
              <li>
                Press <kbd>Esc</kbd> while holding to cancel. Audio discarded.
              </li>
            </>
          )}
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
        <h2 style={{ fontSize: 16 }}>Status</h2>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            background: "#f4f4f4",
            padding: "8px 12px",
            borderRadius: 6,
          }}
        >
          <div
            style={{
              width: 10,
              height: 10,
              borderRadius: "50%",
              background: PHASE_COLOR[phase.kind],
              boxShadow: `0 0 6px ${PHASE_COLOR[phase.kind]}`,
              flexShrink: 0,
            }}
          />
          <div>
            <div style={{ fontWeight: 600 }}>{phase.label}</div>
            {phase.detail && (
              <div style={{ fontSize: 12, color: "#666" }}>{phase.detail}</div>
            )}
          </div>
        </div>
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
