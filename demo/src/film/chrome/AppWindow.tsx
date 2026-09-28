import { interpolate } from "remotion";
import { clamp } from "../anim.ts";
import { useCopy } from "../../lib/copy-context.tsx";
import { RoughDrawing, useWobbleSeed } from "../rough/RoughDrawing.tsx";
import { PALETTE } from "../theme.ts";

/** A rectangle in canvas px. */
export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Height of the title bar in canvas px. */
export const WINDOW_TITLE_BAR = 54;
/** Offset of the hatched pencil shadow to the right and below. */
const SHADOW = 12;
/** Muted pencil versions of the three traffic-light dots. */
const DOTS = ["#e08a74", "#e7c26a", "#9bbf7a"] as const;

/**
 * Where a window's content goes, in canvas px (below the title bar, right of
 * the sidebar if any). Use it to place canvas-space annotations such as
 * PenArrow tips over the window's content.
 */
export function windowContentBox(box: Box, sidebarWidth = 0): Box {
  return { x: box.x + sidebarWidth, y: box.y + WINDOW_TITLE_BAR, w: box.w - sidebarWidth, h: box.h - WINDOW_TITLE_BAR };
}

export interface AppWindowProps {
  /** Outer frame in canvas px. */
  box: Box;
  /** App name in the title bar (e.g. `copy.mail.app`, so both cuts use the app's own label). */
  title: string;
  /**
   * 0..1 sketch-in: the frame, title bar and dots are drawn stroke by stroke
   * (0–0.7), then the surface, title and content fade in (0.55–1). 0 renders nothing.
   */
  progress?: number;
  /** Base wobble seed. Use a distinct one per window. */
  seed?: number;
  /** Width of a sidebar on the left in px (e.g. the Whisper main window's nav, Mail's list). */
  sidebarWidth?: number;
  /** HTML for the sidebar, laid out in its own box. */
  sidebar?: React.ReactNode;
  /** Content surface colour (default whitePaper; cream for a quieter window). */
  surface?: string;
  /** Extra styles for the content box (padding, font size...). */
  contentStyle?: React.CSSProperties;
  /** Content, in the clear typeface of the cut's language (copy.font), ink coloured, 30 px. */
  children?: React.ReactNode;
}

/**
 * A hand-drawn app window on the paper: a wobbly pencil frame with a hatched
 * shadow, a title bar with three pencilled dots and the app's name, and a
 * paper-coloured surface. The content is plain HTML in the cut's clear system
 * typeface, so app text stays crisp and recognisable against the sketch.
 *
 * It is an absolutely positioned HTML element: put it in an AbsoluteFill
 * (not inside an <svg>). Draw annotations over it with a full-canvas <svg>
 * placed after it, using windowContentBox() for coordinates, or with PenMark
 * inline in the content. Content is clipped to the window.
 *
 *   <AppWindow box={{ x: 820, y: 80, w: 960, h: 420 }} title={copy.mail.app} progress={p} seed={31}>
 *     <div>Hi Maya,</div>
 *   </AppWindow>
 */
export const AppWindow: React.FC<AppWindowProps> = ({
  box,
  title,
  progress = 1,
  seed = 31,
  sidebarWidth = 0,
  sidebar,
  surface = PALETTE.whitePaper,
  contentStyle,
  children,
}) => {
  const copy = useCopy();
  const drawing = progress < 1;
  const boil = useWobbleSeed(seed, 4, drawing);
  if (progress <= 0) return null;
  const frameP = interpolate(progress, [0, 0.7], [0, 1], clamp);
  const fillP = interpolate(progress, [0.55, 1], [0, 1], clamp);
  const { w, h } = box;
  const bar = WINDOW_TITLE_BAR;
  const pencil = { stroke: PALETTE.graphite, strokeWidth: 3, roughness: 1.1, bowing: 0.8 };

  return (
    <div style={{ position: "absolute", left: box.x, top: box.y, width: w, height: h }}>
      <svg width={w + SHADOW + 8} height={h + SHADOW + 8} style={{ position: "absolute", left: 0, top: 0, overflow: "visible" }}>
        {/* Surface first, so the pencil lines sit on top of it. */}
        <rect x={2} y={2} width={w - 4} height={h - 4} fill={surface} opacity={fillP} />
        <rect x={2} y={2} width={w - 4} height={bar - 2} fill={PALETTE.cream} opacity={fillP} />
        {sidebarWidth > 0 && <rect x={2} y={bar} width={sidebarWidth - 2} height={h - bar - 2} fill={PALETTE.cream} opacity={fillP} />}
        <RoughDrawing
          seed={boil}
          progress={frameP}
          options={pencil}
          deps={[w, h, sidebarWidth]}
          build={(g, o) => [
            g.rectangle(0, 0, w, h, o),
            g.line(0, bar, w, bar, { ...o, strokeWidth: 2.2 }),
            ...DOTS.map((fill, i) =>
              g.circle(30 + i * 30, bar / 2, 17, { ...o, strokeWidth: 1.8, fill, fillStyle: "solid", roughness: 0.8 }),
            ),
            ...(sidebarWidth > 0 ? [g.line(sidebarWidth, bar, sidebarWidth, h, { ...o, strokeWidth: 2.2 })] : []),
            // A hatched pencil shadow along the right and bottom edges.
            g.polygon(
              [
                [w, SHADOW],
                [w + SHADOW, SHADOW],
                [w + SHADOW, h + SHADOW],
                [SHADOW, h + SHADOW],
                [SHADOW, h],
                [w, h],
              ],
              { ...o, stroke: "none", fill: PALETTE.pencil, fillStyle: "hachure", hachureGap: 7, hachureAngle: -52, fillWeight: 1.3 },
            ),
          ]}
        />
      </svg>
      <div
        style={{
          position: "absolute",
          left: 0,
          top: 0,
          width: w,
          height: bar,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontFamily: copy.font,
          fontSize: 24,
          fontWeight: 600,
          color: PALETTE.pencil,
          opacity: fillP,
        }}
      >
        {title}
      </div>
      {sidebarWidth > 0 && (
        <div
          style={{
            position: "absolute",
            left: 4,
            top: bar + 4,
            width: sidebarWidth - 8,
            height: h - bar - 8,
            overflow: "hidden",
            fontFamily: copy.font,
            fontSize: 24,
            color: PALETTE.pencil,
            opacity: fillP,
          }}
        >
          {sidebar}
        </div>
      )}
      <div
        style={{
          position: "absolute",
          left: sidebarWidth + 4,
          top: bar + 4,
          width: w - sidebarWidth - 8,
          height: h - bar - 8,
          overflow: "hidden",
          boxSizing: "border-box",
          padding: "20px 32px",
          fontFamily: copy.font,
          fontSize: 30,
          lineHeight: 1.4,
          color: PALETTE.ink,
          opacity: fillP,
          ...contentStyle,
        }}
      >
        {children}
      </div>
    </div>
  );
};
