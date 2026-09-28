import { useId } from "react";
import { interpolate } from "remotion";
import { clamp, ramp } from "../../anim.ts";
import { AppWindow, WINDOW_TITLE_BAR } from "../../chrome/AppWindow.tsx";
import { useCopy } from "../../../lib/copy-context.tsx";
import { PenMark, PenTick } from "../../pen/RedPen.tsx";
import { RoughDrawing, useWobbleSeed } from "../../rough/RoughDrawing.tsx";
import { PALETTE } from "../../theme.ts";
import { NOTE, NOTE_WINDOW } from "./PinnedNote.tsx";

const SIDEBAR = 250;
/** Whisper's accent blue and success green, pencilled. */
const ACCENT = "#4f78e0";
const SUCCESS = "#4f8a4a";
const BAR_W = 620;
const BAR_H = 22;
/** The drop zone inside the window content, in content px. */
const ZONE = { top: 84, height: 400 };
const NAV_ORDER = ["home", "insights", "upload", "dictionary", "settings"] as const;

/** Where the dragged files land: the drop zone's centre in canvas px. */
export function dropZoneCentre(): [number, number] {
  const x = NOTE.x + NOTE_WINDOW.x + SIDEBAR + (NOTE_WINDOW.w - SIDEBAR) / 2;
  const y = NOTE.y + NOTE_WINDOW.y + WINDOW_TITLE_BAR + 4 + 20 + ZONE.top + ZONE.height / 2;
  return [x, y];
}

export interface UploadNoteProps {
  t: number;
  /** Window sketch-in 0..1. */
  progress: number;
  /** Film time the dragged files arrive over the drop zone (it lights up blue). */
  hoverAt: number;
  /** Film time they are dropped and the file list replaces the zone. */
  dropAt: number;
  /** Film time each file finishes; each takes `transcribe` seconds. */
  doneAt: readonly number[];
  transcribe: number;
  /** Film time the batch completes: the status is circled and the complete label shown. */
  completeAt: number;
}

/**
 * The Upload note: Whisper's main window on its Upload page. The dashed drop
 * zone lights up as files are dragged over it; once dropped, each file's
 * pencilled progress bar fills in turn and is ticked, and the "3/3
 * completed" status is circled when the batch is done.
 */
export const UploadNote: React.FC<UploadNoteProps> = ({ t, progress, hoverAt, dropAt, doneAt, transcribe, completeAt }) => {
  const copy = useCopy();
  const { upload, nav } = copy.whisper;
  const dropped = t >= dropAt + 0.1;
  const hover = ramp(t, hoverAt, hoverAt + 0.2);
  const done = doneAt.filter((at) => t >= at).length;
  const complete = ramp(t, completeAt, completeAt + 0.3);

  return (
    <AppWindow
      box={NOTE_WINDOW}
      title="Whisper"
      progress={progress}
      seed={381}
      sidebarWidth={SIDEBAR}
      sidebar={
        <div style={{ padding: "18px 10px", display: "flex", flexDirection: "column", gap: 6 }}>
          {NAV_ORDER.map((id) => (
            <div
              key={id}
              style={{
                padding: "10px 16px",
                borderRadius: 12,
                border: `2px solid ${id === "upload" ? PALETTE.pencil : "transparent"}`,
                background: id === "upload" ? PALETTE.whitePaper : undefined,
                color: id === "upload" ? PALETTE.ink : PALETTE.pencil,
                fontWeight: id === "upload" ? 600 : 400,
              }}
            >
              {nav[id]}
            </div>
          ))}
        </div>
      }
    >
      <div style={{ fontSize: 36, fontWeight: 700, marginBottom: 22 }}>{upload.title}</div>
      {!dropped ? (
        <div
          style={{
            height: ZONE.height,
            boxSizing: "border-box",
            borderRadius: 24,
            border: `3px dashed ${hover > 0 ? ACCENT : PALETTE.pencil}`,
            background: `rgba(91,134,245,${0.1 * hover})`,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            gap: 16,
            color: PALETTE.pencil,
          }}
        >
          <svg width={64} height={64} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 16V4M7 9l5-5 5 5M4 20h16" />
          </svg>
          <div style={{ fontSize: 30, color: PALETTE.ink }}>{upload.drop}</div>
          <div style={{ fontSize: 22 }}>{upload.formats}</div>
        </div>
      ) : (
        <div>
          <div style={{ display: "flex", alignItems: "center", gap: 48, padding: "4px 0 18px", fontSize: 28, color: PALETTE.pencil }}>
            <PenMark kind="circle" progress={ramp(t, completeAt, completeAt + 0.4)} seed={391} pad={14}>
              <span style={{ color: done === doneAt.length ? SUCCESS : PALETTE.pencil }}>{upload.progress(done, upload.files.length)}</span>
            </PenMark>
            {complete > 0 && (
              <span style={{ color: SUCCESS, opacity: complete }}>
                {upload.complete} · {upload.saved}
              </span>
            )}
          </div>
          {upload.files.map((name, i) => {
            const p = ramp(t, doneAt[i] - transcribe, doneAt[i]);
            const appear = interpolate(t, [dropAt + 0.1 + i * 0.08, dropAt + 0.4 + i * 0.08], [0, 1], clamp);
            return (
              <div
                key={name}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 26,
                  padding: "14px 0",
                  opacity: appear,
                  transform: `translateX(${(1 - appear) * 40}px)`,
                }}
              >
                <FileIcon name={name} size={0.62} seed={395 + i} />
                <div style={{ width: BAR_W + 20 }}>
                  <div style={{ fontSize: 28, marginBottom: 10 }}>{name}</div>
                  <ProgressBar progress={p} seed={401 + i} />
                </div>
                <span style={{ position: "relative", width: 90, height: 60, fontSize: 26, color: PALETTE.pencil }}>
                  {t < doneAt[i] ? (
                    `${Math.round(p * 100)}%`
                  ) : (
                    <svg width={90} height={60} style={{ position: "absolute", left: 0, top: 0, overflow: "visible" }}>
                      <PenTick x={34} y={30} size={48} progress={ramp(t, doneAt[i], doneAt[i] + 0.25)} seed={411 + i} />
                    </svg>
                  )}
                </span>
              </div>
            );
          })}
        </div>
      )}
    </AppWindow>
  );
};

/** A pencilled progress bar: a wobbly outline, filled with blue hatching up to `progress`, green when done. */
const ProgressBar: React.FC<{ progress: number; seed: number }> = ({ progress, seed }) => {
  const clipId = useId();
  const boil = useWobbleSeed(seed);
  const fill = progress >= 1 ? SUCCESS : ACCENT;
  return (
    <svg width={BAR_W + 8} height={BAR_H + 8} style={{ overflow: "visible", display: "block" }}>
      <clipPath id={clipId}>
        <rect x={0} y={0} width={4 + BAR_W * progress} height={BAR_H + 8} />
      </clipPath>
      <g clipPath={`url(#${clipId})`}>
        <RoughDrawing
          seed={seed}
          options={{ stroke: "none", fill, fillStyle: "hachure", hachureGap: 5, hachureAngle: -40, fillWeight: 2.2, roughness: 1 }}
          build={(g, o) => [g.rectangle(4, 4, BAR_W, BAR_H, o)]}
        />
      </g>
      <RoughDrawing
        seed={boil}
        options={{ stroke: PALETTE.graphite, strokeWidth: 2.2, roughness: 0.9, bowing: 0.6 }}
        build={(g, o) => [g.rectangle(4, 4, BAR_W, BAR_H, o)]}
      />
    </svg>
  );
};

export interface FileIconProps {
  name: string;
  /** 1 = 110 px tall sheet. */
  size?: number;
  seed: number;
}

/** A hand-drawn audio file: a sheet with a folded corner, a little waveform and its extension. */
export const FileIcon: React.FC<FileIconProps> = ({ name, size = 1, seed }) => {
  const boil = useWobbleSeed(seed);
  const ext = name.split(".").pop()?.toUpperCase() ?? "";
  const w = 90;
  const h = 110;
  const fold = 24;
  const wave = [10, 22, 14, 30, 18, 26, 12];
  return (
    <svg width={w * size + 8} height={h * size + 8} viewBox={`-4 -4 ${w + 8} ${h + 8}`} style={{ overflow: "visible", flex: "none" }}>
      <path d={`M0 0 H${w - fold} L${w} ${fold} V${h} H0 Z`} fill={PALETTE.whitePaper} />
      <RoughDrawing
        seed={boil}
        options={{ stroke: PALETTE.graphite, strokeWidth: 2.6, roughness: 0.9, bowing: 0.6 }}
        build={(g, o) => [
          g.linearPath(
            [
              [0, 0],
              [w - fold, 0],
              [w, fold],
              [w, h],
              [0, h],
              [0, 0],
            ],
            o,
          ),
          g.linearPath(
            [
              [w - fold, 0],
              [w - fold, fold],
              [w, fold],
            ],
            o,
          ),
        ]}
      />
      {wave.map((bh, i) => (
        <rect key={i} x={16 + i * 9} y={52 - bh / 2} width={5} height={bh} rx={2.5} fill={ACCENT} />
      ))}
      <text x={w / 2} y={h - 14} textAnchor="middle" fontFamily="-apple-system, Helvetica, sans-serif" fontWeight={700} fontSize={18} fill={PALETTE.pencil}>
        {ext}
      </text>
    </svg>
  );
};

