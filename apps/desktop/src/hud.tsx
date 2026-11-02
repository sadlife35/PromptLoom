import { useEffect, useMemo, useState } from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

type PhaseKind =
  | "ready"
  | "recording"
  | "processing"
  | "delivered"
  | "copied_only"
  | "cancelled"
  | "error";

type Phase = {
  kind: PhaseKind;
  label: string;
  detail: string;
};

const BAR_COUNT = 10;

function Hud() {
  const [phase, setPhase] = useState<Phase>({
    kind: "recording",
    label: "Recording",
    detail: "",
  });
  const [levels, setLevels] = useState<number[]>(() =>
    new Array(BAR_COUNT).fill(0)
  );
  const [partial, setPartial] = useState<string>("");

  useEffect(() => {
    const unLevel = listen<{ level: number }>("promptloom:level", (e) => {
      setLevels((prev) => {
        const next = prev.slice(1);
        next.push(e.payload.level);
        return next;
      });
    });
    const unPhase = listen<Phase>("promptloom:phase", (e) => {
      setPhase(e.payload);
      if (e.payload.kind !== "recording") setPartial("");
    });
    const unPartial = listen<string>("promptloom:partial", (e) => {
      setPartial(e.payload);
    });
    const unTranscript = listen<string>("promptloom:transcript", () => {
      setPartial("");
    });
    return () => {
      unLevel.then((u) => u());
      unPhase.then((u) => u());
      unPartial.then((u) => u());
      unTranscript.then((u) => u());
    };
  }, []);

  const cancel = () => {
    invoke("cancel_dictation").catch(() => void 0);
  };

  const tone = useMemo(() => {
    switch (phase.kind) {
      case "recording":
        return { accent: "#E4A72C", glow: "rgba(228,167,44,0.35)" };
      case "processing":
        return { accent: "#F4C430", glow: "rgba(244,196,48,0.30)" };
      case "delivered":
        return { accent: "#4ADE80", glow: "rgba(74,222,128,0.35)" };
      case "copied_only":
        return { accent: "#60A5FA", glow: "rgba(96,165,250,0.30)" };
      case "error":
        return { accent: "#E57373", glow: "rgba(229,115,115,0.30)" };
      default:
        return { accent: "#8A92A3", glow: "rgba(0,0,0,0)" };
    }
  }, [phase.kind]);

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        height: "100vh",
        width: "100vw",
        padding: 0,
        margin: 0,
        boxSizing: "border-box",
        gap: 4,
        background: "transparent",
      }}
    >
      {/* Partial transcript floats above the pill */}
      {phase.kind === "recording" && partial && (
        <div
          style={{
            maxWidth: 320,
            padding: "4px 10px",
            fontSize: 11,
            color: "#E6E8EC",
            background: "rgba(14,16,20,0.80)",
            backdropFilter: "blur(10px)",
            borderRadius: 6,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
            pointerEvents: "none",
            fontFamily:
              '-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif',
          }}
          title={partial}
        >
          {partial}
        </div>
      )}

      {/* The pill itself — tight, solid, single shape */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 4,
          padding: "3px 4px",
          background: "#16191F",
          borderRadius: 999,
          border: "none",
          boxShadow: `0 10px 28px rgba(0,0,0,0.6), 0 0 20px ${tone.glow}`,
          height: 34,
          color: "#fff",
          fontFamily:
            '-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif',
        }}
      >
        {/* Cancel */}
        <button
          type="button"
          onClick={cancel}
          title="Cancel (Esc)"
          style={{
            flexShrink: 0,
            width: 26,
            height: 26,
            borderRadius: "50%",
            border: "none",
            background: "transparent",
            color: "#8A92A3",
            cursor: "pointer",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            fontSize: 11,
            lineHeight: 1,
            padding: 0,
            transition: "background 0.12s, color 0.12s",
          }}
          onMouseOver={(e) => {
            e.currentTarget.style.background = "rgba(229,115,115,0.18)";
            e.currentTarget.style.color = "#FFBEBE";
          }}
          onMouseOut={(e) => {
            e.currentTarget.style.background = "transparent";
            e.currentTarget.style.color = "#8A92A3";
          }}
        >
          ✕
        </button>

        {/* Center: waveform / state glyph */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            gap: 2,
            height: 20,
            minWidth: 72,
            maxWidth: 100,
            padding: "0 4px",
          }}
        >
          {phase.kind === "recording" && (
            <>
              {levels.map((lv, i) => {
                const h = Math.max(3, Math.round(3 + lv * 16));
                return (
                  <div
                    key={i}
                    style={{
                      width: 2.5,
                      height: h,
                      borderRadius: 2,
                      background: tone.accent,
                      opacity: 0.55 + lv * 0.45,
                      transition: "height 90ms ease-out",
                    }}
                  />
                );
              })}
            </>
          )}

          {phase.kind === "processing" && (
            <>
              {Array.from({ length: BAR_COUNT }).map((_, i) => (
                <div
                  key={i}
                  style={{
                    width: 2.5,
                    height: 10,
                    borderRadius: 2,
                    background: tone.accent,
                    animation: `pl-shimmer 1.1s ease-in-out ${i * 0.06}s infinite`,
                  }}
                />
              ))}
              <style>{`
                @keyframes pl-shimmer {
                  0%, 100% { opacity: 0.25; transform: scaleY(0.5); }
                  50% { opacity: 1; transform: scaleY(1.4); }
                }
              `}</style>
            </>
          )}

          {phase.kind !== "recording" && phase.kind !== "processing" && (
            <div
              style={{
                width: 6,
                height: 6,
                borderRadius: "50%",
                background: tone.accent,
                boxShadow: `0 0 8px ${tone.glow}`,
              }}
            />
          )}
        </div>

        {/* Right status dot */}
        <div
          style={{
            flexShrink: 0,
            width: 26,
            height: 26,
            borderRadius: "50%",
            background: tone.accent,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "#0F1114",
            boxShadow: `0 0 10px ${tone.glow}`,
          }}
          title={phase.label}
        >
          {phase.kind === "recording" && (
            <div
              style={{
                width: 8,
                height: 8,
                borderRadius: "50%",
                background: "#0F1114",
                animation: "pl-pulse 1.3s ease-in-out infinite",
              }}
            />
          )}
          {phase.kind === "processing" && (
            <div
              style={{
                width: 10,
                height: 10,
                border: "2px solid rgba(15,17,20,0.3)",
                borderTopColor: "#0F1114",
                borderRadius: "50%",
                animation: "pl-spin 0.75s linear infinite",
              }}
            />
          )}
          {phase.kind === "delivered" && (
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
              <path
                d="M2.5 6.3L5 8.5L9.5 3.5"
                stroke="#0F1114"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          )}
          {phase.kind === "copied_only" && (
            <svg width="11" height="11" viewBox="0 0 12 12" fill="none">
              <rect
                x="3"
                y="3"
                width="7"
                height="8"
                rx="1"
                stroke="#0F1114"
                strokeWidth="1.6"
              />
              <path d="M2 2h6" stroke="#0F1114" strokeWidth="1.6" strokeLinecap="round" />
            </svg>
          )}
          {phase.kind === "error" && (
            <span style={{ fontSize: 11, fontWeight: 700 }}>!</span>
          )}
          {phase.kind === "cancelled" && (
            <span style={{ fontSize: 11, fontWeight: 700 }}>×</span>
          )}
          {phase.kind === "ready" && (
            <div
              style={{
                width: 6,
                height: 6,
                borderRadius: "50%",
                background: "#0F1114",
                opacity: 0.4,
              }}
            />
          )}
        </div>
      </div>

      <style>{`
        @keyframes pl-pulse {
          0%, 100% { transform: scale(1); opacity: 1; }
          50% { transform: scale(0.55); opacity: 0.55; }
        }
        @keyframes pl-spin {
          to { transform: rotate(360deg); }
        }
      `}</style>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(<Hud />);
