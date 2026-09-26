import type { CSSProperties, ReactNode } from "react";
import { Img, staticFile } from "remotion";
import { useCopy } from "../lib/copy-context.tsx";
import { LIGHT, type TimeOfDay } from "../theme.ts";

export const SCREEN_W = 1600;
export const SCREEN_H = 1000;
const MENU_H = 30;

/** The listening ring with three bars, as in VoiceIdentityIcon. */
export function WhisperGlyph({
  size = 18,
  color = "currentColor",
}: {
  size?: number;
  color?: string;
}) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none">
      <circle cx="12" cy="12" r="9.5" stroke={color} strokeWidth="2" />
      {[
        [8.75, 10, 14],
        [12, 8, 16],
        [15.25, 10, 14],
      ].map(([x, y1, y2]) => (
        <path
          key={x}
          d={`M${x} ${y1}V${y2}`}
          stroke={color}
          strokeWidth="2"
          strokeLinecap="round"
        />
      ))}
    </svg>
  );
}

function Wallpaper({ time }: { time: TimeOfDay }) {
  const [a, b, c] = LIGHT[time].wallpaper;
  return (
    <svg width={SCREEN_W} height={SCREEN_H} style={{ position: "absolute", inset: 0 }}>
      <defs>
        <linearGradient id={`wp-${time}`} x1="0" y1="0" x2="0.3" y2="1">
          <stop offset="0" stopColor={a} />
          <stop offset="0.55" stopColor={b} />
          <stop offset="1" stopColor={c} />
        </linearGradient>
      </defs>
      <rect width={SCREEN_W} height={SCREEN_H} fill={`url(#wp-${time})`} />
      <path
        d="M0 720 C 300 620 520 700 800 640 S 1300 560 1600 660 V1000 H0Z"
        fill="#fff"
        opacity="0.12"
      />
      <path
        d="M0 820 C 360 740 640 830 960 770 S 1420 720 1600 780 V1000 H0Z"
        fill="#000"
        opacity="0.08"
      />
    </svg>
  );
}

function MenuBar({ app, clock, dark }: { app: string; clock: string; dark: boolean }) {
  const copy = useCopy();
  const color = dark ? "#fff" : "#1d1d1f";
  const items =
    copy.lang === "en"
      ? ["File", "Edit", "View", "Window", "Help"]
      : ["文件", "编辑", "显示", "窗口", "帮助"];
  return (
    <div
      style={{
        position: "absolute",
        inset: "0 0 auto 0",
        height: MENU_H,
        display: "flex",
        alignItems: "center",
        gap: 22,
        padding: "0 18px",
        fontSize: 15,
        color,
        background: dark ? "rgba(20,20,30,0.35)" : "rgba(255,255,255,0.45)",
        backdropFilter: "blur(20px)",
        fontFamily: copy.font,
      }}
    >
      <span style={{ width: 14, height: 14, borderRadius: 7, background: color, opacity: 0.85 }} />
      <b style={{ fontWeight: 700 }}>{app}</b>
      {items.map((item) => (
        <span key={item}>{item}</span>
      ))}
      <span style={{ flex: 1 }} />
      <WhisperGlyph size={17} color={color} />
      <svg width="18" height="14" viewBox="0 0 18 14">
        <path d="M9 12.5l2.4-2.9a3.6 3.6 0 0 0-4.8 0z" fill={color} />
        <path
          d="M3.4 6.8a8 8 0 0 1 11.2 0"
          stroke={color}
          strokeWidth="1.8"
          fill="none"
          strokeLinecap="round"
        />
        <path
          d="M1 3.9a11.6 11.6 0 0 1 16 0"
          stroke={color}
          strokeWidth="1.8"
          fill="none"
          strokeLinecap="round"
        />
      </svg>
      <span
        style={{
          width: 26,
          height: 12,
          border: `1.5px solid ${color}`,
          borderRadius: 3,
          padding: 1.5,
        }}
      >
        <span
          style={{
            display: "block",
            width: "70%",
            height: "100%",
            background: color,
            borderRadius: 1,
          }}
        />
      </span>
      <span style={{ fontVariantNumeric: "tabular-nums" }}>{clock}</span>
    </div>
  );
}

const DOCK_COLORS = ["#4b9bff", "#34c759", "#ff9f0a", "#af52de", "#ff375f", "#5ac8fa", "#8e8e93"];

function Dock() {
  return (
    <div
      style={{
        position: "absolute",
        left: "50%",
        bottom: 10,
        transform: "translateX(-50%)",
        display: "flex",
        gap: 10,
        padding: 9,
        borderRadius: 22,
        background: "rgba(255,255,255,0.28)",
        border: "1px solid rgba(255,255,255,0.35)",
        backdropFilter: "blur(24px)",
      }}
    >
      {DOCK_COLORS.map((color) => (
        <span
          key={color}
          style={{
            width: 52,
            height: 52,
            borderRadius: 13,
            background: `linear-gradient(160deg, ${color}, ${color}cc)`,
            boxShadow: "inset 0 1px 0 rgba(255,255,255,0.35)",
          }}
        />
      ))}
      <Img src={staticFile("icon.png")} style={{ width: 52, height: 52, borderRadius: 13 }} />
    </div>
  );
}

interface DesktopProps {
  time: TimeOfDay;
  app: string;
  clock: string;
  children?: ReactNode;
  /** Floating layer above windows, e.g. the Recording pill. */
  overlay?: ReactNode;
}

/** A Mac desktop at screen resolution (1600×1000 points). */
export function Desktop({ time, app, clock, children, overlay }: DesktopProps) {
  const dark = time === "night" || time === "evening";
  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        overflow: "hidden",
        width: SCREEN_W,
        height: SCREEN_H,
      }}
    >
      <Wallpaper time={time} />
      <div style={{ position: "absolute", inset: 0 }}>{children}</div>
      <Dock />
      <MenuBar app={app} clock={clock} dark={dark} />
      {overlay}
    </div>
  );
}

interface WindowProps {
  x: number;
  y: number;
  width: number;
  height: number;
  title?: string;
  dark?: boolean;
  children?: ReactNode;
  style?: CSSProperties;
  bodyStyle?: CSSProperties;
  /** Hide the standard title bar when the app draws its own. */
  bare?: boolean;
}

/** A standard macOS window with traffic lights. */
export function Window({
  x,
  y,
  width,
  height,
  title,
  dark,
  children,
  style,
  bodyStyle,
  bare,
}: WindowProps) {
  const copy = useCopy();
  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        width,
        height,
        borderRadius: 14,
        overflow: "hidden",
        background: dark ? "#1b1c20" : "#fbfbfd",
        color: dark ? "#ececec" : "#1d1d1f",
        boxShadow: "0 30px 70px rgba(0,0,0,0.28), 0 0 0 1px rgba(0,0,0,0.12)",
        fontFamily: copy.font,
        ...style,
      }}
    >
      {!bare && (
        <div
          style={{
            height: 44,
            display: "flex",
            alignItems: "center",
            padding: "0 16px",
            gap: 8,
            borderBottom: `1px solid ${dark ? "#2c2e33" : "#e6e6ea"}`,
            background: dark ? "#18191c" : "#f3f3f6",
            position: "relative",
          }}
        >
          <TrafficLights />
          <span
            style={{
              position: "absolute",
              left: 0,
              right: 0,
              textAlign: "center",
              fontSize: 15,
              fontWeight: 600,
              opacity: 0.8,
              pointerEvents: "none",
            }}
          >
            {title}
          </span>
        </div>
      )}
      <div style={{ position: "absolute", inset: bare ? 0 : "44px 0 0 0", ...bodyStyle }}>
        {children}
      </div>
    </div>
  );
}

export function TrafficLights() {
  return (
    <span style={{ display: "flex", gap: 8 }}>
      {["#ff5f57", "#febc2e", "#28c840"].map((color) => (
        <span key={color} style={{ width: 13, height: 13, borderRadius: 7, background: color }} />
      ))}
    </span>
  );
}
