import { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";

function Hud() {
  const [level, setLevel] = useState(0);
  const [status, setStatus] = useState<string>("Recording");
  const [transcript, setTranscript] = useState<string>("");

  useEffect(() => {
    const unlistenLevel = listen<{ level: number }>("promptloom:level", (e) => {
      setLevel(e.payload.level);
    });
    const unlistenState = listen<string>("promptloom:state", (e) => {
      setStatus(e.payload.replace(/\s*\{.*/, ""));
    });
    const unlistenTranscript = listen<string>("promptloom:transcript", (e) => {
      setTranscript(e.payload);
      // Non-persistent: clear after 4s.
      window.setTimeout(() => setTranscript(""), 4000);
    });
    return () => {
      unlistenLevel.then((u) => u());
      unlistenState.then((u) => u());
      unlistenTranscript.then((u) => u());
    };
  }, []);

  const bars = 20;
  const filled = Math.round(level * bars);
  const showingTranscript = transcript.length > 0;

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
      }}
    >
      {!showingTranscript ? (
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <div
            style={{
              width: 10,
              height: 10,
              borderRadius: "50%",
              background: "#e74c3c",
              boxShadow: "0 0 8px #e74c3c",
            }}
          />
          <div style={{ fontSize: 12, opacity: 0.85, width: 70 }}>{status}</div>
          <div style={{ display: "flex", gap: 2, flex: 1 }}>
            {Array.from({ length: bars }).map((_, i) => (
              <div
                key={i}
                style={{
                  flex: 1,
                  height: 18,
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
        </div>
      ) : (
        <div
          style={{
            fontSize: 13,
            lineHeight: 1.35,
            maxHeight: 40,
            overflow: "hidden",
            textOverflow: "ellipsis",
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
