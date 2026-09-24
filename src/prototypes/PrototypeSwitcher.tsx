// PROTOTYPE (throwaway): floating variant switcher shared by prototype pages.
import { useEffect } from "react";

interface PrototypeSwitcherProps {
  variants: { key: string; name: string }[];
  current: string;
  onChange: (key: string) => void;
}

export function PrototypeSwitcher({ variants, current, onChange }: PrototypeSwitcherProps) {
  const index = Math.max(
    0,
    variants.findIndex((v) => v.key === current)
  );
  const step = (delta: number) =>
    onChange(variants[(index + delta + variants.length) % variants.length].key);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, textarea, [contenteditable]")) return;
      if (event.key === "ArrowLeft") step(-1);
      if (event.key === "ArrowRight") step(1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  if (!import.meta.env.DEV) return null;
  const active = variants[index];
  return (
    <div
      className="fixed bottom-5 left-1/2 z-50 flex -translate-x-1/2 items-center gap-1 rounded-full px-1.5 py-1.5 text-[13px] font-medium shadow-[0_8px_30px_rgba(0,0,0,0.35)]"
      style={{ background: "#ffd400", color: "#111" }}
    >
      <button
        className="size-7 rounded-full hover:bg-black/10"
        onClick={() => step(-1)}
        aria-label="Previous"
      >
        ←
      </button>
      <span className="min-w-56 text-center tabular-nums">
        {active.key} ({active.name}) · {index + 1}/{variants.length}
      </span>
      <button
        className="size-7 rounded-full hover:bg-black/10"
        onClick={() => step(1)}
        aria-label="Next"
      >
        →
      </button>
    </div>
  );
}
