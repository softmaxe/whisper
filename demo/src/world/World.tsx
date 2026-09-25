import type { ReactNode } from "react";
import { AbsoluteFill } from "remotion";
import { lerp } from "../lib/anim.ts";
import { SCREEN_H, SCREEN_W } from "../screen/Desktop.tsx";
import { LIGHT, type TimeOfDay } from "../theme.ts";
import { Backdrop, DeskProps, type Place } from "./Backdrop.tsx";
import { Arms, Person } from "./Person.tsx";

// The laptop screen in the wide shot, in world pixels.
const SCREEN_SCALE = 0.36;
const SCREEN_X = 812;
const SCREEN_Y = 286;
const SCREEN_CENTER = {
  x: SCREEN_X + (SCREEN_W * SCREEN_SCALE) / 2,
  y: SCREEN_Y + (SCREEN_H * SCREEN_SCALE) / 2,
};
/** World zoom at which the screen fills the frame height. */
const CLOSE_ZOOM = 1080 / (SCREEN_H * SCREEN_SCALE);

interface WorldProps {
  place: Place;
  time: TimeOfDay;
  /** 0 = wide shot of the room, 1 = the screen fills the frame. */
  push: number;
  screen: ReactNode;
  speaking?: number;
  press?: number;
  stretch?: number;
}

function Table({ time }: { time: TimeOfDay }) {
  const light = LIGHT[time];
  return (
    <svg
      width="1920"
      height="1080"
      viewBox="0 0 1920 1080"
      style={{ position: "absolute", inset: 0 }}
    >
      <rect x="0" y="660" width="1920" height="420" fill={light.table} />
      <rect x="0" y="660" width="1920" height="10" fill={light.tableEdge} />
      <rect x="0" y="670" width="1920" height="60" fill="#000" opacity="0.05" />
    </svg>
  );
}

function LaptopBody() {
  const right = SCREEN_X + SCREEN_W * SCREEN_SCALE;
  const bottom = SCREEN_Y + SCREEN_H * SCREEN_SCALE;
  return (
    <svg
      width="1920"
      height="1080"
      viewBox="0 0 1920 1080"
      style={{ position: "absolute", inset: 0 }}
    >
      <rect
        x={SCREEN_X - 14}
        y={SCREEN_Y - 14}
        width={right - SCREEN_X + 28}
        height={bottom - SCREEN_Y + 26}
        rx="18"
        fill="#17181c"
      />
      <path
        d={`M${SCREEN_X - 40} ${bottom + 12} H${right + 40} L${right + 90} ${bottom + 58} H${SCREEN_X - 90} Z`}
        fill="#c9ced6"
      />
      <path
        d={`M${SCREEN_X + 10} ${bottom + 20} H${right - 10} L${right + 20} ${bottom + 44} H${SCREEN_X - 20} Z`}
        fill="#3a3d45"
      />
      <rect
        x={SCREEN_X - 90}
        y={bottom + 56}
        width={right - SCREEN_X + 180}
        height="10"
        rx="5"
        fill="#a9afb9"
      />
    </svg>
  );
}

/**
 * The room, the desk, and the person, with a camera that pushes into the
 * laptop screen. The screen renders at full resolution at every zoom.
 */
export function World({
  place,
  time,
  push,
  screen,
  speaking = 0,
  press = 0,
  stretch = 0,
}: WorldProps) {
  const zoom = lerp(1, CLOSE_ZOOM, push);
  const cx = lerp(960, SCREEN_CENTER.x, push);
  const cy = lerp(540, SCREEN_CENTER.y, push);
  const light = LIGHT[time];
  // The person drifts out of the way faster than the room zooms.
  const personShift = push * 260;

  return (
    <AbsoluteFill style={{ overflow: "hidden", background: light.wall }}>
      <AbsoluteFill
        style={{
          transformOrigin: "0 0",
          transform: `translate(960px, 540px) scale(${zoom}) translate(${-cx}px, ${-cy}px)`,
        }}
      >
        <AbsoluteFill style={{ filter: push > 0.02 ? `blur(${push * 6}px)` : undefined }}>
          <Backdrop place={place} time={time} />
          <Table time={time} />
          <DeskProps place={place} time={time} />
        </AbsoluteFill>
        <LaptopBody />
        <div
          style={{
            position: "absolute",
            left: SCREEN_X,
            top: SCREEN_Y,
            width: SCREEN_W,
            height: SCREEN_H,
            transform: `scale(${SCREEN_SCALE})`,
            transformOrigin: "0 0",
            borderRadius: 18,
            overflow: "hidden",
          }}
        >
          {screen}
        </div>
        <AbsoluteFill
          style={{
            transform: `translate(${-personShift}px, ${personShift * 0.5}px)`,
            opacity: 1 - Math.max(0, push - 0.6) / 0.4,
          }}
        >
          <Arms press={press} stretch={stretch} />
          <Person speaking={speaking} press={press} stretch={stretch} night={time === "night"} />
        </AbsoluteFill>
      </AbsoluteFill>
      <AbsoluteFill
        style={{
          background: light.tint,
          opacity: light.tintOpacity * (1 - push),
          mixBlendMode: "multiply",
        }}
      />
    </AbsoluteFill>
  );
}
