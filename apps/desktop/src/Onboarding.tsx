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
        return model?.installed === true && model?.verified === true;
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

  return (
    <main style={{ fontFamily: "system-ui", padding: 24, maxWidth: 720 }}>
      <h1 style={{ margin: 0 }}>Welcome to PromptLoom</h1>
      <p style={{ color: "#555", marginTop: 4 }}>
        A short, one-time setup. You can change any of this later in Settings.
      </p>

      <nav
        style={{
          display: "flex",
          gap: 6,
          margin: "16px 0 24px",
          flexWrap: "wrap",
          fontSize: 12,
        }}
      >
        {ORDER.map((s, i) => (
          <div
            key={s}
            style={{
              padding: "4px 8px",
              borderRadius: 4,
              background: s === step ? "#2d6cdf" : "#eef2f7",
              color: s === step ? "#fff" : "#555",
              border: "1px solid " + (s === step ? "#2d6cdf" : "#ddd"),
            }}
          >
            {i + 1}. {LABELS[s]}
          </div>
        ))}
      </nav>

      {step === "privacy" && (
        <section>
          <h2>Privacy</h2>
          <ul style={{ lineHeight: 1.7 }}>
            <li>Speech is transcribed <b>locally</b> by whisper.cpp running on this PC.</li>
            <li>PromptLoom <b>does not require an account</b> and sends no telemetry.</li>
            <li>
              <b>Transcripts are not retained</b>. Each dictation is shown
              briefly in the HUD, pasted (or copied) once, then cleared.
            </li>
            <li>
              <b>Audio behavior, stated precisely:</b> each dictation is
              written to a scoped <code>.wav</code> file in your system temp
              directory for whisper to decode, then deleted when
              transcription returns. Nothing is written on cancel. A built-in
              privacy audit under Settings can verify this at any time.
            </li>
            <li>
              After the model is installed, <b>no network calls</b> are made.
              You can disable your network and the app still works.
            </li>
          </ul>
        </section>
      )}

      {step === "microphone" && (
        <section>
          <h2>Microphone</h2>
          {!mics ? (
            <p>Looking for input devices…</p>
          ) : mics.devices.length === 0 ? (
            <div
              style={{
                background: "#fff1f0",
                border: "1px solid #ffa39e",
                padding: 12,
                borderRadius: 6,
              }}
            >
              <b>No input device found.</b>
              <ol>
                <li>Plug in or enable a microphone.</li>
                <li>
                  Settings → System → Sound → confirm the device is enabled
                  and Microphone privacy is allowed for desktop apps.
                </li>
                <li>Click Refresh.</li>
              </ol>
              <button type="button" onClick={refreshMics}>
                Refresh
              </button>
            </div>
          ) : (
            <>
              <p>
                Pick the microphone PromptLoom should listen on. The level
                below reacts to your voice in real time.
              </p>
              <select
                value={settings?.mic_device ?? ""}
                onChange={(e) =>
                  pickMic(e.target.value === "" ? null : e.target.value)
                }
                style={{ padding: 6, minWidth: 320 }}
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
              <div style={{ marginTop: 16 }}>
                <div style={{ fontSize: 12, color: "#666", marginBottom: 4 }}>
                  Live input level
                </div>
                <div
                  style={{
                    width: "100%",
                    height: 20,
                    background: "#eee",
                    borderRadius: 4,
                    overflow: "hidden",
                  }}
                >
                  <div
                    style={{
                      width: `${Math.min(100, probeLevel * 100)}%`,
                      height: "100%",
                      background:
                        probeLevel > 0.9
                          ? "#e74c3c"
                          : probeLevel > 0.05
                          ? "#2ecc71"
                          : "#bbb",
                      transition: "width 60ms linear",
                    }}
                  />
                </div>
                <div style={{ fontSize: 11, color: "#888", marginTop: 4 }}>
                  Speak now to confirm the bar moves. If it stays flat with
                  everything plugged in, this device isn't picking you up.
                </div>
              </div>
            </>
          )}
        </section>
      )}

      {step === "model" && (
        <section>
          <h2>Local speech model</h2>
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
              <div style={{ fontSize: 12, color: "#666", marginTop: 4 }}>
                License notice:{" "}
                <a
                  href="https://huggingface.co/ggerganov/whisper.cpp/blob/main/LICENSE"
                  target="_blank"
                  rel="noreferrer"
                >
                  whisper.cpp model license
                </a>
              </div>
              <div style={{ marginTop: 8 }}>
                Status:{" "}
                <code>
                  {model.installed
                    ? model.verified
                      ? "installed (SHA-256 verified)"
                      : "installed — hash MISMATCH; re-download recommended"
                    : "not installed"}
                </code>
              </div>
              {!model.installed && (
                <button
                  type="button"
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
                  style={{ marginTop: 12 }}
                >
                  {downloading ? `Downloading… ${pct}%` : "Download now"}
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
                      }}
                    />
                  </div>
                  <div style={{ fontSize: 11, color: "#777", marginTop: 2 }}>
                    {(progress.bytes / 1_000_000).toFixed(1)} /{" "}
                    {(progress.total / 1_000_000).toFixed(1)} MB
                  </div>
                </div>
              )}
              {downloadError && (
                <div
                  style={{
                    marginTop: 8,
                    padding: 8,
                    background: "#fff1f0",
                    border: "1px solid #ffa39e",
                    borderRadius: 4,
                    fontSize: 12,
                  }}
                >
                  <b>Download failed.</b> {downloadError}
                  <div style={{ marginTop: 4, color: "#666" }}>
                    Check your network and click Download now to retry. We do
                    not auto-retry.
                  </div>
                </div>
              )}
            </div>
          ) : (
            <p>Loading model status…</p>
          )}
        </section>
      )}

      {step === "hotkey" && (
        <section>
          <h2>Shortcut</h2>
          <p>
            <b>Hold to talk, release to transcribe.</b> Press <kbd>Esc</kbd>{" "}
            while holding to cancel. Pick the key you want to hold.
          </p>
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            {hotkeyOptions.map((h) => (
              <button
                key={h}
                type="button"
                onClick={() => pickHotkey(h)}
                style={{
                  padding: "8px 16px",
                  background: settings?.hotkey === h ? "#2d6cdf" : "#fff",
                  color: settings?.hotkey === h ? "#fff" : "#333",
                  border: "1px solid #ccc",
                  borderRadius: 4,
                  cursor: "pointer",
                }}
              >
                {h}
              </button>
            ))}
          </div>
          <div style={{ fontSize: 12, color: "#777", marginTop: 8 }}>
            Current: <code>{settings?.hotkey ?? "F8"}</code>. The change
            applies immediately.
          </div>
        </section>
      )}

      {step === "vocabulary" && (
        <section>
          <h2>Project vocabulary (optional)</h2>
          <p>
            Point PromptLoom at one folder that contains{" "}
            <code>.promptloom/terms.yml</code>. PromptLoom <b>only</b> reads
            that one file — it does not crawl your project. Corrections are
            applied after transcription, before paste.
          </p>
          {settings?.project_path ? (
            <div
              style={{
                background: "#f8f8fa",
                border: "1px solid #e4e4e8",
                padding: 12,
                borderRadius: 6,
              }}
            >
              <div
                style={{
                  fontFamily: "monospace",
                  fontSize: 12,
                  wordBreak: "break-all",
                }}
              >
                {settings.project_path}
              </div>
              <div style={{ marginTop: 4, fontSize: 13 }}>
                Loaded <b>{settings.terms_count}</b> term
                {settings.terms_count === 1 ? "" : "s"}.
              </div>
              <div style={{ marginTop: 8, display: "flex", gap: 8 }}>
                <button type="button" onClick={chooseVocabFolder}>
                  Change folder
                </button>
                <button type="button" onClick={clearVocab}>
                  Skip
                </button>
              </div>
            </div>
          ) : (
            <div style={{ display: "flex", gap: 8 }}>
              <button type="button" onClick={chooseVocabFolder}>
                Choose folder…
              </button>
              <button type="button" onClick={next}>
                Skip for now
              </button>
            </div>
          )}
        </section>
      )}

      {step === "done" && (
        <section>
          <h2>Try it in Notepad</h2>
          <ol style={{ lineHeight: 1.7 }}>
            <li>
              Open Notepad (press <kbd>Win</kbd> and type "Notepad").
            </li>
            <li>Click into the empty window so it has keyboard focus.</li>
            <li>
              Hold <kbd>{settings?.hotkey ?? "F8"}</kbd> and speak clearly:{" "}
              <i>"the deploy pipeline runs on Tuesday"</i>.
            </li>
            <li>Release. The text should appear in Notepad.</li>
          </ol>
          <p style={{ color: "#555" }}>
            When you've seen it work (or if you want to skip and finish
            anyway), click <b>Finish setup</b>.
          </p>
          <button type="button" onClick={finish} style={{ padding: "8px 16px" }}>
            Finish setup
          </button>
        </section>
      )}

      <nav
        style={{
          marginTop: 32,
          display: "flex",
          justifyContent: "space-between",
          borderTop: "1px solid #eee",
          paddingTop: 16,
        }}
      >
        <button type="button" onClick={back} disabled={idx === 0}>
          Back
        </button>
        {step !== "done" && (
          <button
            type="button"
            onClick={next}
            disabled={!canContinueFrom(step)}
            title={
              canContinueFrom(step)
                ? undefined
                : step === "model"
                ? "Download and verify the model to continue."
                : step === "microphone"
                ? "At least one input device is required."
                : undefined
            }
          >
            Next
          </button>
        )}
      </nav>
    </main>
  );
}
