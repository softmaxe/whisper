// PROTOTYPE (throwaway): "What should the recording pill look like, Wispr Flow-style?"
// Four variants plus the current pill, switchable via ?variant= on a standalone
// page (the pill lives in its own transparent window, so there is no host page).
// Run: npm run prototype:voice-pill
import React, { useCallback, useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import "../../index.css";
import { VoicePill, type VoicePillState } from "../../components/dictation/VoicePill";
import { PrototypeSwitcher } from "../PrototypeSwitcher";
import { PILL_VARIANTS, toBarLevel, type PillPhase, type VariantProps } from "./variants";

type SimMode = "idle" | "speaking" | "silent" | "processing" | "auto";

const AUTO_SCRIPT: { mode: Exclude<SimMode, "auto">; ms: number }[] = [
  { mode: "idle", ms: 1400 },
  { mode: "speaking", ms: 3800 },
  { mode: "silent", ms: 1200 },
  { mode: "speaking", ms: 2200 },
  { mode: "processing", ms: 1800 },
];

/** Syllable-ish synthetic speech RMS, roughly matching real mic levels. */
function createSpeechSim() {
  let env = 0;
  let target = 0;
  let nextEdge = 0;
  let voiced = false;
  let pauseUntil = 0;
  let phraseUntil = 0;
  return (now: number, speaking: boolean) => {
    if (!speaking) {
      target = 0.002 + Math.random() * 0.002;
    } else {
      if (now > phraseUntil && now > pauseUntil) {
        phraseUntil = now + 1200 + Math.random() * 1800;
        pauseUntil = phraseUntil + 250 + Math.random() * 500;
      }
      if (now > phraseUntil) {
        target = 0.003;
      } else if (now > nextEdge) {
        voiced = !voiced;
        nextEdge = now + (voiced ? 90 + Math.random() * 90 : 40 + Math.random() * 60);
        target = voiced ? 0.03 + Math.random() * 0.12 : 0.008;
      }
    }
    env += (target - env) * (target > env ? 0.35 : 0.14);
    return env;
  };
}

function useMicRms(enabled: boolean) {
  const rmsRef = useRef<number | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let stream: MediaStream | null = null;
    let ctx: AudioContext | null = null;
    let frame = 0;
    navigator.mediaDevices.getUserMedia({ audio: true }).then((s) => {
      stream = s;
      ctx = new AudioContext();
      const analyser = ctx.createAnalyser();
      analyser.fftSize = 1024;
      ctx.createMediaStreamSource(s).connect(analyser);
      const buf = new Float32Array(analyser.fftSize);
      const loop = () => {
        analyser.getFloatTimeDomainData(buf);
        let sum = 0;
        for (const v of buf) sum += v * v;
        rmsRef.current = Math.sqrt(sum / buf.length);
        frame = requestAnimationFrame(loop);
      };
      loop();
    });
    return () => {
      cancelAnimationFrame(frame);
      stream?.getTracks().forEach((t) => t.stop());
      ctx?.close();
      rmsRef.current = null;
    };
  }, [enabled]);
  return rmsRef;
}

const ALL_VARIANTS = [
  { key: "0", name: "Current pill" },
  ...PILL_VARIANTS.map(({ key, name }) => ({ key, name })),
];

const BACKDROPS = {
  editor: { label: "Dark editor", bg: "#1b1b1d", dark: true },
  light: { label: "Light app", bg: "#f3f3f1", dark: false },
  wallpaper: {
    label: "Wallpaper",
    bg: "radial-gradient(120% 90% at 20% 10%, #6d83c9 0%, #3a4a86 40%, #1d2340 100%)",
    dark: true,
  },
} as const;

function CurrentPill({ phase, rms }: VariantProps) {
  const rmsRef = useRef(rms);
  rmsRef.current = rms;
  const getAudioLevel = useCallback(() => rmsRef.current, []);
  const state: VoicePillState =
    phase === "idle" ? "idle" : phase === "listening" ? "recording" : "processing";
  return <VoicePill variant="floating" state={state} getAudioLevel={getAudioLevel} />;
}

function Prototype() {
  const [variant, setVariant] = useState(
    () => new URLSearchParams(location.search).get("variant") ?? "A"
  );
  const [mode, setMode] = useState<SimMode>("auto");
  const [backdrop, setBackdrop] = useState<keyof typeof BACKDROPS>("editor");
  const [micOn, setMicOn] = useState(false);
  const micRms = useMicRms(micOn);
  const [frame, setFrame] = useState({ now: 0, rms: 0, effective: "idle" as SimMode });
  const sim = useRef(createSpeechSim());
  const autoStart = useRef(performance.now());

  useEffect(() => {
    autoStart.current = performance.now();
  }, [mode]);

  useEffect(() => {
    let raf = 0;
    const total = AUTO_SCRIPT.reduce((s, x) => s + x.ms, 0);
    const loop = (now: number) => {
      let effective: SimMode = mode;
      if (mode === "auto") {
        let t = (now - autoStart.current) % total;
        for (const step of AUTO_SCRIPT) {
          if (t < step.ms) {
            effective = step.mode;
            break;
          }
          t -= step.ms;
        }
      }
      const simRms = sim.current(now, effective === "speaking");
      const rms = micRms.current ?? simRms;
      setFrame({ now, rms, effective });
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [mode, micRms]);

  const changeVariant = (key: string) => {
    setVariant(key);
    const url = new URL(location.href);
    url.searchParams.set("variant", key);
    history.replaceState(null, "", url);
  };

  const bd = BACKDROPS[backdrop];
  useEffect(() => {
    document.documentElement.classList.toggle("dark", bd.dark);
  }, [bd.dark]);

  const phase: PillPhase =
    frame.effective === "idle"
      ? "idle"
      : frame.effective === "processing"
        ? "processing"
        : "listening";
  const Variant =
    variant === "0"
      ? CurrentPill
      : (PILL_VARIANTS.find((v) => v.key === variant)?.Component ?? CurrentPill);
  const props: VariantProps = { phase, rms: frame.rms, now: frame.now };
  const chip = (on: boolean) =>
    `rounded-full px-2.5 py-1 text-[12px] transition-colors ${
      on ? "bg-white text-black" : "bg-white/10 text-white/80 hover:bg-white/20"
    }`;

  return (
    <div className="fixed inset-0 overflow-hidden" style={{ background: bd.bg }}>
      {/* Controls + surfaced state */}
      <div className="absolute left-4 top-4 z-10 flex max-w-[calc(100%-2rem)] flex-col gap-2 rounded-xl bg-black/70 p-3 text-white backdrop-blur">
        <div className="flex flex-wrap gap-1.5">
          {(["auto", "idle", "speaking", "silent", "processing"] as SimMode[]).map((m) => (
            <button key={m} className={chip(mode === m)} onClick={() => setMode(m)}>
              {m}
            </button>
          ))}
        </div>
        <div className="flex flex-wrap gap-1.5">
          {(Object.keys(BACKDROPS) as (keyof typeof BACKDROPS)[]).map((k) => (
            <button key={k} className={chip(backdrop === k)} onClick={() => setBackdrop(k)}>
              {BACKDROPS[k].label}
            </button>
          ))}
          <button className={chip(micOn)} onClick={() => setMicOn((v) => !v)}>
            {micOn ? "Mic: live" : "Mic: simulated"}
          </button>
        </div>
        <pre className="font-mono text-[11px] leading-4 text-white/70">
          {`variant  ${variant} (${ALL_VARIANTS.find((v) => v.key === variant)?.name})
state    ${frame.effective} → phase ${phase}
rms      ${frame.rms.toFixed(4)}   bar ${toBarLevel(frame.rms).toFixed(2)}`}
        </pre>
      </div>

      {/* Zoomed view */}
      <div className="absolute inset-x-0 top-[34%] flex -translate-y-1/2 flex-col items-center gap-10">
        <div style={{ transform: "scale(3)", transformOrigin: "center" }}>
          <Variant {...props} />
        </div>
        <span className={`mt-10 text-[11px] ${bd.dark ? "text-white/40" : "text-black/40"}`}>
          3× zoom · actual size at the bottom
        </span>
      </div>

      {/* Actual size, docked like the real pill */}
      <div className="absolute inset-x-0 bottom-24 flex justify-center">
        <Variant {...props} />
      </div>

      <PrototypeSwitcher variants={ALL_VARIANTS} current={variant} onChange={changeVariant} />
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Prototype />
  </React.StrictMode>
);
