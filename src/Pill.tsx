// The floating recording pill: waveform + state, shown while dictating.
import { useEffect, useRef, useState } from "react";
import { onPipelineEvent } from "./lib/api";

type PillState = "recording" | "transcribing" | "cleaning" | "done" | "empty" | "error";

export default function Pill() {
  const [state, setState] = useState<PillState>("recording");
  const [elapsed, setElapsed] = useState(0);
  const [message, setMessage] = useState("");
  const levels = useRef<number[]>(new Array(24).fill(0.05));
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const un = onPipelineEvent((ev) => {
      switch (ev.type) {
        case "recording_started":
          levels.current.fill(0.05);
          setState("recording");
          setElapsed(0);
          setMessage("");
          break;
        case "level":
          levels.current.push(Math.max(0.05, ev.value));
          levels.current.shift();
          setElapsed(ev.elapsed_secs);
          break;
        case "transcribing":
          setState("transcribing");
          break;
        case "cleaning":
          setState("cleaning");
          break;
        case "done":
          setState("done");
          setMessage(
            ev.outcome === "refused_secure_field"
              ? "Password field — skipped"
              : ev.outcome === "clipboard_only"
                ? "Copied — press Ctrl+V"
                : `${ev.timings.total_ms} ms`
          );
          break;
        case "empty":
          setState("empty");
          setMessage("No speech detected");
          break;
        case "error":
          setState("error");
          setMessage(ev.message);
          break;
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  // Waveform render loop.
  useEffect(() => {
    let raf = 0;
    const draw = () => {
      const canvas = canvasRef.current;
      if (canvas) {
        const ctx = canvas.getContext("2d")!;
        const { width, height } = canvas;
        ctx.clearRect(0, 0, width, height);
        const bars = levels.current;
        const bw = width / bars.length;
        ctx.fillStyle = state === "recording" ? "#4ade80" : "#94a3b8";
        bars.forEach((v, i) => {
          const h = Math.min(1, v) * (height - 6) + 3;
          const y = (height - h) / 2;
          ctx.beginPath();
          ctx.roundRect(i * bw + 1.5, y, bw - 3, h, 2);
          ctx.fill();
        });
      }
      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(raf);
  }, [state]);

  const label =
    state === "recording"
      ? `● ${elapsed.toFixed(0)}s`
      : state === "transcribing"
        ? "Transcribing…"
        : state === "cleaning"
          ? "Polishing…"
          : message;

  return (
    <div className={`pill pill-${state}`}>
      <canvas ref={canvasRef} width={150} height={36} />
      <span className="pill-label">{label}</span>
    </div>
  );
}
