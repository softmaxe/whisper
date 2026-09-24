// PROTOTYPE — throwaway floating bar that cycles ?variant= on the current route. Dev builds only.
import { useCallback, useEffect, useState } from "react";
import { ChevronRight } from "../icons";

interface Variant {
  key: string;
  name: string;
}

function readVariant(variants: readonly Variant[]) {
  const key = new URLSearchParams(window.location.search).get("variant");
  return variants.find((variant) => variant.key === key)?.key ?? variants[0].key;
}

export function usePrototypeVariant<T extends Variant>(variants: readonly T[]) {
  const [current, setCurrent] = useState(() => readVariant(variants));

  const select = useCallback((key: string) => {
    const url = new URL(window.location.href);
    url.searchParams.set("variant", key);
    window.history.replaceState(window.history.state, "", url);
    setCurrent(key);
  }, []);

  return [current as T["key"], select] as const;
}

export function PrototypeSwitcher({
  variants,
  current,
  onSelect,
}: {
  variants: readonly Variant[];
  current: string;
  onSelect: (key: string) => void;
}) {
  const index = Math.max(
    0,
    variants.findIndex((variant) => variant.key === current)
  );
  const step = useCallback(
    (delta: number) => onSelect(variants[(index + delta + variants.length) % variants.length].key),
    [index, onSelect, variants]
  );

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, textarea, [contenteditable]")) return;
      if (event.key === "ArrowLeft") step(-1);
      if (event.key === "ArrowRight") step(1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [step]);

  if (!import.meta.env.DEV) return null;

  const variant = variants[index];
  return (
    <div className="fixed bottom-5 left-1/2 z-50 flex -translate-x-1/2 items-center gap-1 rounded-full bg-[#111] px-1.5 py-1.5 font-sans text-xs text-white shadow-[0_8px_30px_rgba(0,0,0,0.35)] ring-1 ring-white/15">
      <button
        onClick={() => step(-1)}
        aria-label="Previous variant"
        className="grid h-7 w-7 place-items-center rounded-full hover:bg-white/10"
      >
        <ChevronRight size={14} className="rotate-180" />
      </button>
      <span className="min-w-36 text-center tabular-nums">
        {variant.key === "current" ? "" : `${variant.key} · `}
        {variant.name}
        <span className="ms-2 text-white/40">
          {index + 1}/{variants.length}
        </span>
      </span>
      <button
        onClick={() => step(1)}
        aria-label="Next variant"
        className="grid h-7 w-7 place-items-center rounded-full hover:bg-white/10"
      >
        <ChevronRight size={14} />
      </button>
    </div>
  );
}
