import { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";

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

const COLOR = {
  ready: "#9aa5b1",
  recording: "#e74c3c",
  processing: "#f39c12",
  delivered: "#2ecc71",
  copied_only: "#3498db",
  cancelled: "#9aa5b1",
  error: "#e74c3c",
} as const;

function Hud() {
  const [phase, setPhase] = useState<Phase>({
    kind: "recording",
    label: "Recording",
    detail: "Listening — release to transcribe.",
  });
  const [level, setLevel] = useState(0);
  const [transcript, setTranscript] = useState<string>("");

  useEffect(() => {
    const unLevel = listen<{ level: number }>("promptloom:level", (e) => {
      setLevel(e.payload.level);
    });
    const unPhase = listen<Phase>("promptloom:phase", (e) => {
      setPhase(e.payload);
    });
    const unTranscript = listen<string>("promptloom:transcript", (e) => {
      setTranscript(e.payload);
      window.setTimeout(() => setTranscript(""), 4000);
    });
    return () => {
      unLevel.then((u) => u());
      unPhase.then((u) => u());
      unTranscript.then((u) => u());
    };
  }, []);

  const dot = COLOR[phase.kind] ?? "#9aa5b1";

  const bars = 20;
  const filled = Math.round(level * bars);

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 4,
        padding: "10px 14px",
        background: "rgba(20,20,24,0.92)",
        borderRadius: 10,
        boxShadow: "0 4px 18px rgba(0,0,0,0.4)",
        height: "100%",
        boxSizing: "border-box",
        color: "#fff",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <div
          style={{
            width: 10,
            height: 10,
            borderRadius: "50%",
            background: dot,
            boxShadow: `0 0 8px ${dot}`,
            flexShrink: 0,
          }}
        />
        <div style={{ fontSize: 12, fontWeight: 600 }}>{phase.label}</div>
        {phase.kind === "recording" && (
          <div style={{ display: "flex", gap: 2, flex: 1, marginLeft: 6 }}>
            {Array.from({ length: bars }).map((_, i) => (
              <div
                key={i}
                style={{
                  flex: 1,
                  height: 14,
                  borderRadius: 2,
                  background:
                    i < filled
                      ? i > bars * 0.8
                        ? "#e74c3c"
                        : "#2ecc71"
                      : "rgba(255,255,255,0.12)",
                }}
              />
            ))}
          </div>
        )}
      </div>

      {phase.kind !== "recording" && (
        <div
          style={{
            fontSize: 11,
            opacity: 0.75,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
          title={phase.detail}
        >
          {phase.detail}
        </div>
      )}

      {phase.kind === "delivered" && transcript && (
        <div
          style={{
            fontSize: 11,
            opacity: 0.9,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            fontFamily: "monospace",
            borderTop: "1px solid rgba(255,255,255,0.08)",
            paddingTop: 4,
          }}
          title={transcript}
        >
          {transcript}
        </div>
      )}
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(<Hud />);
