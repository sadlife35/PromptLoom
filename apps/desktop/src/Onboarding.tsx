import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

type Settings = {
  project_path: string | null;
  terms_count: number;
  onboarding_complete: boolean;
  hotkey: string;
  mic_device: string | null;
};

type ModelStatus = {
  installed: boolean;
  path: string;
  name: string;
  size_bytes: number;
  verified: boolean;
  engine_installed: boolean;
  engine_path: string | null;
};

type MicListing = {
  devices: string[];
  default: string | null;
  selected: string | null;
};

type Step =
  | "privacy"
  | "microphone"
  | "model"
  | "hotkey"
  | "vocabulary"
  | "done";

const ORDER: Step[] = [
  "privacy",
  "microphone",
  "model",
  "hotkey",
  "vocabulary",
  "done",
];

const LABELS: Record<Step, string> = {
  privacy: "Privacy",
  microphone: "Microphone",
  model: "Local model",
  hotkey: "Shortcut",
  vocabulary: "Project vocabulary (optional)",
  done: "Finish",
};

export function Onboarding(props: { onFinish: () => void }) {
  const [step, setStep] = useState<Step>("privacy");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [model, setModel] = useState<ModelStatus | null>(null);
  const [mics, setMics] = useState<MicListing | null>(null);
  const [probeLevel, setProbeLevel] = useState(0);
  const [hotkeyOptions, setHotkeyOptions] = useState<string[]>([]);
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState<{ bytes: number; total: number } | null>(
    null
  );
  const [downloadError, setDownloadError] = useState<string | null>(null);

  const idx = ORDER.indexOf(step);

  const refreshSettings = () =>
    invoke<Settings>("get_settings").then(setSettings).catch(() => void 0);
  const refreshModel = () =>
    invoke<ModelStatus>("model_status").then(setModel).catch(() => void 0);
  const refreshMics = () =>
    invoke<MicListing>("list_microphones").then(setMics).catch(() => void 0);

  useEffect(() => {
    refreshSettings();
    refreshModel();
    invoke<string[]>("allowed_hotkeys").then(setHotkeyOptions);
    const unDone = listen<boolean>("promptloom:download-done", (e) => {
      setDownloading(false);
      setProgress(null);
      if (!e.payload) setDownloadError("Download or SHA-256 check failed.");
      else setDownloadError(null);
      refreshModel();
    });
    const unProg = listen<{ bytes: number; total: number }>(
      "promptloom:download",
      (e) => setProgress(e.payload)
    );
    return () => {
      unDone.then((u) => u());
      unProg.then((u) => u());
    };
  }, []);

  useEffect(() => {
    if (step === "microphone") {
      refreshMics();
      invoke("start_mic_probe").catch(() => void 0);
      const id = window.setInterval(() => {
        invoke<number>("mic_probe_level").then(setProbeLevel).catch(() => void 0);
      }, 100);
      return () => {
        window.clearInterval(id);
        invoke("stop_mic_probe").catch(() => void 0);
      };
    }
  }, [step]);

  const next = () => {
    const n = ORDER[idx + 1];
    if (n) setStep(n);
  };
  const back = () => {
    const p = ORDER[idx - 1];
    if (p) setStep(p);
  };

  const canContinueFrom = (s: Step): boolean => {
    switch (s) {
      case "privacy":
        return true;
      case "microphone":
        return (mics?.devices.length ?? 0) > 0;
      case "model":
        return (
          model?.installed === true &&
          model?.verified === true &&
          model?.engine_installed === true
        );
      case "hotkey":
        return !!settings?.hotkey && hotkeyOptions.includes(settings.hotkey);
      case "vocabulary":
        return true;
      case "done":
        return false;
    }
  };

  const chooseVocabFolder = async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (!picked || typeof picked !== "string") return;
    const next = await invoke<Settings>("set_project_path", { path: picked });
    setSettings(next);
  };

  const clearVocab = async () => {
    const next = await invoke<Settings>("set_project_path", { path: null });
    setSettings(next);
  };

  const pickHotkey = async (hk: string) => {
    try {
      const next = await invoke<Settings>("set_hotkey", { hotkey: hk });
      setSettings(next);
    } catch (e) {
      alert(`Could not set hotkey: ${e}`);
    }
  };

  const pickMic = async (name: string | null) => {
    const next = await invoke<Settings>("set_mic_device", { device: name });
    setSettings(next);
    // Restart probe against the new device.
    await invoke("stop_mic_probe");
    await invoke("start_mic_probe").catch(() => void 0);
    refreshMics();
  };

  const finish = async () => {
    await invoke<Settings>("mark_onboarding_complete");
    props.onFinish();
  };

  const pct =
    progress && progress.total > 0
      ? Math.round((100 * progress.bytes) / progress.total)
      : 0;

  const headings: Record<Step, { eyebrow: string; title: string; sub: string }> = {
    privacy: {
      eyebrow: "Step 1 of 6",
      title: "You control your data",
      sub: "A short, one-time setup. Everything stays on this PC.",
    },
    microphone: {
      eyebrow: "Step 2 of 6",
      title: "Test your microphone",
      sub: "We recommend built-in or wired mics. Bluetooth works but can lag.",
    },
    model: {
      eyebrow: "Step 3 of 6",
      title: "Install the speech model",
      sub: "One download. After this the app works offline.",
    },
    hotkey: {
      eyebrow: "Step 4 of 6",
      title: "Pick your shortcut",
      sub: "Hold to talk, release to transcribe. Esc cancels.",
    },
    vocabulary: {
      eyebrow: "Step 5 of 6",
      title: "Teach it your words",
      sub: "Optional. Point at a project folder with a vocabulary file.",
    },
    done: {
      eyebrow: "Step 6 of 6",
      title: "Try it in Notepad",
      sub: "Thirty seconds and you're dictating.",
    },
  };

  const h = headings[step];

  return (
    <main
      style={{
        minHeight: "100vh",
        display: "flex",
        flexDirection: "column",
        padding: "28px 32px 24px",
      }}
    >
      {/* Progress bar */}
      <div
        style={{
          display: "flex",
          gap: 6,
          marginBottom: 36,
        }}
      >
        {ORDER.map((s, i) => (
          <div
            key={s}
            style={{
              flex: 1,
              height: 3,
              borderRadius: 2,
              background:
                i < idx
                  ? "var(--accent)"
                  : i === idx
                  ? "var(--accent)"
                  : "var(--border)",
              opacity: i === idx ? 1 : i < idx ? 0.5 : 1,
              transition: "background 0.2s, opacity 0.2s",
            }}
            title={LABELS[s]}
          />
        ))}
      </div>

      {/* Header */}
      <div style={{ maxWidth: 560, margin: "0 auto 28px", width: "100%" }}>
        <div className="pl-section-label" style={{ marginBottom: 8 }}>
          {h.eyebrow}
        </div>
        <h1
          style={{
            fontFamily: "var(--font-display)",
            fontStyle: "italic",
            fontWeight: 500,
            fontSize: 36,
            letterSpacing: "-0.02em",
            margin: "0 0 10px",
            color: "var(--fg)",
          }}
        >
          {h.title}
        </h1>
        <p
          style={{
            color: "var(--fg-dim)",
            fontSize: 14,
            margin: 0,
            lineHeight: 1.55,
          }}
        >
          {h.sub}
        </p>
      </div>

      {/* Step content */}
      <div
        style={{
          flex: 1,
          maxWidth: 560,
          margin: "0 auto",
          width: "100%",
        }}
        className="pl-stack"
      >
        {step === "privacy" && (
          <div className="pl-card">
            <ul style={{ margin: 0, paddingLeft: 20, lineHeight: 1.7 }}>
              <li>
                Speech is transcribed <strong style={{ color: "var(--fg)" }}>locally</strong>{" "}
                by whisper.cpp running on this PC.
              </li>
              <li>
                <strong style={{ color: "var(--fg)" }}>No account, no telemetry.</strong>
              </li>
              <li>
                <strong style={{ color: "var(--fg)" }}>Transcripts are not retained.</strong>{" "}
                Each one is shown briefly, pasted (or copied) once, then cleared.
              </li>
              <li>
                Each dictation is written to a scoped{" "}
                <code className="pl-inline">.wav</code> file in your system temp
                directory for whisper to decode, then deleted when transcription
                returns. A built-in privacy audit can verify this at any time.
              </li>
              <li>
                After the model is installed,{" "}
                <strong style={{ color: "var(--fg)" }}>no network calls</strong>{" "}
                are made.
              </li>
            </ul>
          </div>
        )}

        {step === "microphone" && (
          <div className="pl-card">
            {!mics ? (
              <div className="pl-muted">Looking for input devices…</div>
            ) : mics.devices.length === 0 ? (
              <div className="pl-notice danger">
                <strong>No input device found.</strong>
                <ol style={{ margin: "8px 0 0 20px", paddingLeft: 0 }}>
                  <li>Plug in or enable a microphone.</li>
                  <li>
                    Settings → System → Sound → confirm the device is enabled
                    and Microphone privacy is allowed for desktop apps.
                  </li>
                  <li>Click Refresh below.</li>
                </ol>
                <button
                  type="button"
                  className="pl-btn"
                  onClick={refreshMics}
                  style={{ marginTop: 12 }}
                >
                  Refresh
                </button>
              </div>
            ) : (
              <>
                <div className="pl-card-body" style={{ marginBottom: 12 }}>
                  Pick the microphone PromptLoom should listen on. The bar below
                  reacts to your voice in real time.
                </div>
                <select
                  className="pl-select"
                  value={settings?.mic_device ?? ""}
                  onChange={(e) =>
                    pickMic(e.target.value === "" ? null : e.target.value)
                  }
                  style={{ width: "100%" }}
                >
                  <option value="">
                    System default
                    {mics.default ? ` (${mics.default})` : ""}
                  </option>
                  {mics.devices.map((d) => (
                    <option key={d} value={d}>
                      {d}
                    </option>
                  ))}
                </select>

                <div
                  className="pl-section-label"
                  style={{ marginTop: 16, marginBottom: 6 }}
                >
                  Live input level
                </div>
                <div className="pl-progress">
                  <div
                    className="pl-progress-fill"
                    style={{
                      width: `${Math.min(100, probeLevel * 100)}%`,
                      background:
                        probeLevel > 0.9
                          ? "var(--danger)"
                          : probeLevel > 0.05
                          ? "var(--success)"
                          : "var(--fg-very-dim)",
                    }}
                  />
                </div>
                <div className="pl-dim" style={{ fontSize: 11, marginTop: 6 }}>
                  Speak to confirm the bar moves.
                </div>
              </>
            )}
          </div>
        )}

        {step === "model" && (
          <div className="pl-card">
            {model ? (
              <>
                <div className="pl-row between" style={{ marginBottom: 12 }}>
                  <div>
                    <div className="pl-card-head" style={{ marginBottom: 2 }}>
                      {model.name}
                    </div>
                    <div className="pl-dim" style={{ fontSize: 12 }}>
                      {(model.size_bytes / 1_000_000).toFixed(0)} MB
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
                <div className="pl-card-body" style={{ marginBottom: 12 }}>
                  License:{" "}
                  <a
                    href="https://huggingface.co/ggerganov/whisper.cpp/blob/main/LICENSE"
                    target="_blank"
                    rel="noreferrer"
                  >
                    whisper.cpp model license
                  </a>
                  . Download happens only when you click below — nothing is
                  fetched automatically.
                </div>

                {!model.installed && (
                  <button
                    type="button"
                    className="pl-btn primary"
                    disabled={downloading}
                    onClick={() => {
                      setDownloading(true);
                      setDownloadError(null);
                      setProgress({ bytes: 0, total: model.size_bytes });
                      invoke("download_model_cmd").catch((e) => {
                        setDownloadError(String(e));
                        setDownloading(false);
                      });
                    }}
                  >
                    {downloading ? `Downloading… ${pct}%` : "Download model"}
                  </button>
                )}

                {downloading && progress && (
                  <div style={{ marginTop: 12 }}>
                    <div className="pl-progress">
                      <div
                        className="pl-progress-fill"
                        style={{ width: `${pct}%` }}
                      />
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

                {downloadError && (
                  <div className="pl-notice danger" style={{ marginTop: 12 }}>
                    <strong>Download failed.</strong> {downloadError}
                    <div style={{ marginTop: 4, fontSize: 11 }}>
                      Check your network and click Download model to retry. No
                      auto-retry.
                    </div>
                  </div>
                )}

                {model.installed && model.verified && !model.engine_installed && (
                  <div className="pl-notice danger" style={{ marginTop: 12 }}>
                    <strong>Whisper engine missing.</strong> The model is
                    installed but <code className="pl-inline">whisper-server.exe</code>{" "}
                    (or <code className="pl-inline">whisper-cli.exe</code>) was
                    not found next to the model or in a <code className="pl-inline">tools/</code>{" "}
                    folder. Dictation will fail until you place one there, or
                    set <code className="pl-inline">PROMPTLOOM_WHISPER_SERVER_BIN</code>{" "}
                    to its full path.
                    <div style={{ marginTop: 8 }}>
                      <button
                        type="button"
                        className="pl-btn"
                        onClick={refreshModel}
                      >
                        Re-check
                      </button>
                    </div>
                  </div>
                )}
              </>
            ) : (
              <div className="pl-muted">Loading model status…</div>
            )}
          </div>
        )}

        {step === "hotkey" && (
          <div className="pl-card">
            <div className="pl-card-body" style={{ marginBottom: 12 }}>
              Hold to talk, release to transcribe. <kbd>Esc</kbd> cancels while
              holding. Pick the key you want to hold.
            </div>
            <div
              className="pl-btn-group"
              style={{ gap: 8, flexWrap: "wrap" }}
            >
              {hotkeyOptions.map((hk) => (
                <button
                  key={hk}
                  type="button"
                  className={`pl-btn ${settings?.hotkey === hk ? "active" : ""}`}
                  onClick={() => pickHotkey(hk)}
                >
                  {hk}
                </button>
              ))}
            </div>
            <div className="pl-dim" style={{ fontSize: 12, marginTop: 10 }}>
              Current: <kbd>{settings?.hotkey ?? "F8"}</kbd>. Takes effect
              immediately.
            </div>
          </div>
        )}

        {step === "vocabulary" && (
          <div className="pl-card">
            <div className="pl-card-body" style={{ marginBottom: 12 }}>
              Point PromptLoom at one folder that contains{" "}
              <code className="pl-inline">.promptloom/terms.yml</code>. It only
              reads that one file — it does not crawl your project. Corrections
              are applied after transcription, before paste.
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
                <div className="pl-btn-group" style={{ marginTop: 12 }}>
                  <button
                    type="button"
                    className="pl-btn"
                    onClick={chooseVocabFolder}
                  >
                    Change folder
                  </button>
                  <button
                    type="button"
                    className="pl-btn"
                    onClick={clearVocab}
                  >
                    Clear
                  </button>
                </div>
              </>
            ) : (
              <div className="pl-btn-group">
                <button
                  type="button"
                  className="pl-btn primary"
                  onClick={chooseVocabFolder}
                >
                  Choose folder…
                </button>
                <button type="button" className="pl-btn" onClick={next}>
                  Skip for now
                </button>
              </div>
            )}
          </div>
        )}

        {step === "done" && (
          <div className="pl-card">
            <ol style={{ margin: 0, paddingLeft: 20, lineHeight: 1.75 }}>
              <li>
                Open Notepad (press <kbd>Win</kbd> and type "Notepad").
              </li>
              <li>Click into the empty window so it has keyboard focus.</li>
              <li>
                Hold <kbd>{settings?.hotkey ?? "F8"}</kbd> and say{" "}
                <em style={{ color: "var(--fg)" }}>
                  "the deploy pipeline runs on Tuesday"
                </em>
                .
              </li>
              <li>Release. The text should appear in Notepad.</li>
            </ol>
            <div className="pl-notice" style={{ marginTop: 16 }}>
              When you've seen it work (or want to skip the demo), click
              Finish. You can always revisit everything in Settings.
            </div>
            <button
              type="button"
              className="pl-btn primary"
              onClick={finish}
              style={{ marginTop: 14 }}
            >
              Finish setup
            </button>
          </div>
        )}
      </div>

      {/* Footer nav */}
      <div
        style={{
          maxWidth: 560,
          margin: "32px auto 0",
          width: "100%",
          display: "flex",
          justifyContent: "space-between",
          paddingTop: 20,
          borderTop: "1px solid var(--border)",
        }}
      >
        <button
          type="button"
          className="pl-btn"
          onClick={back}
          disabled={idx === 0}
        >
          Back
        </button>
        {step !== "done" && (
          <button
            type="button"
            className="pl-btn primary"
            onClick={next}
            disabled={!canContinueFrom(step)}
            title={
              canContinueFrom(step)
                ? undefined
                : step === "model"
                ? !model?.installed
                  ? "Download the model to continue."
                  : !model?.verified
                  ? "Model hash did not match. Re-download."
                  : !model?.engine_installed
                  ? "whisper-server.exe missing — see note above."
                  : "Download and verify the model to continue."
                : step === "microphone"
                ? "At least one input device is required."
                : undefined
            }
          >
            Continue
          </button>
        )}
      </div>
    </main>
  );
}
