// PROTOTYPE (throwaway): "What should the pill show while idle, warming up the
// mic, and thinking, now that listening is the black Flow bar?"
// Four variants plus the current pill, switchable via ?variant= on a standalone
// page (the pill lives in its own transparent window, so there is no host page).
// Run: npm run prototype:preload-icon
import React, { useCallback, useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import "../../index.css";
import { VoicePill, type VoicePillState } from "../../components/dictation/VoicePill";
import { PrototypeSwitcher } from "../PrototypeSwitcher";
import { VARIANTS, type Phase, type VariantProps } from "./variants";

type SimMode = "auto" | "idle" | "hover" | "preparing" | "speaking" | "silent" | "thinking";

// Warm-up and thinking are held longer than real life so they can be judged.
const AUTO_SCRIPT: { mode: Exclude<SimMode, "auto">; ms: number }[] = [
  { mode: "idle", ms: 1600 },
  { mode: "hover", ms: 900 },
  { mode: "preparing", ms: 1400 },
  { mode: "speaking", ms: 3000 },
  { mode: "silent", ms: 900 },
  { mode: "speaking", ms: 1600 },
  { mode: "thinking", ms: 2600 },
];

const toPhase = (mode: Exclude<SimMode, "auto">): Phase =>
  mode === "speaking" || mode === "silent" ? "listening" : mode;

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

const ALL_VARIANTS = [
  { key: "0", name: "Current pill" },
  ...VARIANTS.map(({ key, name }) => ({ key, name })),
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

const CURRENT_STATE: Record<Phase, VoicePillState> = {
  idle: "idle",
  hover: "hover",
  preparing: "processing",
  listening: "recording",
  thinking: "thinking",
};

function CurrentPill({ phase, rms }: VariantProps) {
  const rmsRef = useRef(rms);
  rmsRef.current = rms;
  const getAudioLevel = useCallback(() => rmsRef.current, []);
  return (
    <VoicePill variant="floating" state={CURRENT_STATE[phase]} getAudioLevel={getAudioLevel} />
  );
}

function Prototype() {
  const [variant, setVariant] = useState(
    () => new URLSearchParams(location.search).get("variant") ?? "A"
  );
  const [mode, setMode] = useState<SimMode>("auto");
  const [backdrop, setBackdrop] = useState<keyof typeof BACKDROPS>("editor");
  const [k, setK] = useState(1);
  const [frame, setFrame] = useState({ now: 0, rms: 0, effective: "idle" as SimMode });
  const sim = useRef(createSpeechSim());
  const autoStart = useRef(0);

  useEffect(() => {
    autoStart.current = performance.now();
  }, [mode, k]);

  useEffect(() => {
    let raf = 0;
    const total = AUTO_SCRIPT.reduce((s, x) => s + x.ms, 0) * k;
    const loop = (now: number) => {
      let effective: SimMode = mode;
      if (mode === "auto") {
        let t = (now - autoStart.current) % total;
        for (const step of AUTO_SCRIPT) {
          if (t < step.ms * k) {
            effective = step.mode;
            break;
          }
          t -= step.ms * k;
        }
      }
      const rms = sim.current(now / k, effective === "speaking");
      setFrame({ now, rms, effective });
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [mode, k]);

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

  const phase = toPhase(frame.effective === "auto" ? "idle" : frame.effective);
  const Variant =
    variant === "0"
      ? CurrentPill
      : (VARIANTS.find((v) => v.key === variant)?.Component ?? CurrentPill);
  // Slow motion also slows the live waveform's clock.
  const props: VariantProps = { phase, rms: frame.rms, now: frame.now, k };
  const chip = (on: boolean) =>
    `rounded-full px-2.5 py-1 text-[12px] transition-colors ${
      on ? "bg-white text-black" : "bg-white/10 text-white/80 hover:bg-white/20"
    }`;

  return (
    <div className="fixed inset-0 overflow-hidden" style={{ background: bd.bg }}>
      <div className="absolute left-4 top-4 z-10 flex max-w-[calc(100%-2rem)] flex-col gap-2 rounded-xl bg-black/70 p-3 text-white backdrop-blur">
        <div className="flex flex-wrap gap-1.5">
          {(
            ["auto", "idle", "hover", "preparing", "speaking", "silent", "thinking"] as SimMode[]
          ).map((m) => (
            <button key={m} className={chip(mode === m)} onClick={() => setMode(m)}>
              {m}
            </button>
          ))}
        </div>
        <div className="flex flex-wrap gap-1.5">
          {(Object.keys(BACKDROPS) as (keyof typeof BACKDROPS)[]).map((b) => (
            <button key={b} className={chip(backdrop === b)} onClick={() => setBackdrop(b)}>
              {BACKDROPS[b].label}
            </button>
          ))}
          <button className={chip(k !== 1)} onClick={() => setK((v) => (v === 1 ? 4 : 1))}>
            {k === 1 ? "Speed 1×" : "Slow-mo ¼×"}
          </button>
        </div>
        <pre className="font-mono text-[11px] leading-4 text-white/70">
          {`variant  ${variant} (${ALL_VARIANTS.find((v) => v.key === variant)?.name})
sim      ${frame.effective} → phase ${phase}
rms      ${frame.rms.toFixed(4)}`}
        </pre>
      </div>

      <div className="absolute inset-x-0 top-[36%] flex -translate-y-1/2 flex-col items-center">
        <div style={{ transform: "scale(3)", transformOrigin: "center" }}>
          <Variant {...props} />
        </div>
        <span className={`mt-24 text-[11px] ${bd.dark ? "text-white/40" : "text-black/40"}`}>
          3× zoom · actual size at the bottom
        </span>
      </div>

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
