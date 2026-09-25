// Palettes for the cartoon world and the app's own dark interface. App colors
// follow src/index.css (dark theme) and src/styles/dictation-panel.css.

export const APP = {
  window: "#141518",
  sidebar: "#141518",
  panel: "#1b1c20",
  surface: "#222328",
  raised: "#2a2c31",
  border: "#2c2e33",
  borderHover: "#3b3d44",
  text: "#ececec",
  muted: "#9a9ba1",
  faint: "#6c6d73",
  primary: "#5b86f5",
  navActive: "#1e2233",
  success: "#34d399",
  flowInk: "#0c0c0e",
  flowRim: "rgba(255,255,255,0.16)",
};

export const INK = "#2b2733";

/** Skin, hair, and clothes for the one character who carries the day. */
export const PERSON = {
  skin: "#f1c3a0",
  skinShade: "#dfa585",
  hair: "#3a2a24",
  hairLight: "#4d392f",
  sweater: "#2f8f83",
  sweaterShade: "#26776d",
  collar: "#f4efe6",
};

export type TimeOfDay = "dawn" | "morning" | "noon" | "afternoon" | "evening" | "night";

export interface Light {
  skyTop: string;
  skyBottom: string;
  wall: string;
  wallShade: string;
  table: string;
  tableEdge: string;
  /** Multiplied over the whole world for time-of-day color. */
  tint: string;
  tintOpacity: number;
  wallpaper: [string, string, string];
}

export const LIGHT: Record<TimeOfDay, Light> = {
  dawn: {
    skyTop: "#ffb996",
    skyBottom: "#ffe3c4",
    wall: "#f7dcc4",
    wallShade: "#eec6a6",
    table: "#c98f62",
    tableEdge: "#a8714a",
    tint: "#ffb27a",
    tintOpacity: 0.1,
    wallpaper: ["#ffcfa8", "#f59b8b", "#8f7ad6"],
  },
  morning: {
    skyTop: "#8fcaf5",
    skyBottom: "#dff1ff",
    wall: "#e4ebf2",
    wallShade: "#cfd9e4",
    table: "#e9e2d6",
    tableEdge: "#cbbfae",
    tint: "#bfe0ff",
    tintOpacity: 0.06,
    wallpaper: ["#bfe3ff", "#7fb3f2", "#5d6fd8"],
  },
  noon: {
    skyTop: "#6ec1f7",
    skyBottom: "#c8ecff",
    wall: "#e9d3bd",
    wallShade: "#d8b99c",
    table: "#f3f0ea",
    tableEdge: "#d9d2c5",
    tint: "#fff4c7",
    tintOpacity: 0.05,
    wallpaper: ["#c7f0d8", "#7dd3c0", "#3fa0b8"],
  },
  afternoon: {
    skyTop: "#ffc978",
    skyBottom: "#ffecc2",
    wall: "#efe2cc",
    wallShade: "#e0cdae",
    table: "#e3d6c0",
    tableEdge: "#c8b89c",
    tint: "#ffcf7a",
    tintOpacity: 0.12,
    wallpaper: ["#ffe1a8", "#f7a86b", "#d96c7a"],
  },
  evening: {
    skyTop: "#6a57a8",
    skyBottom: "#ff9f7a",
    wall: "#c9a9b8",
    wallShade: "#b0909f",
    table: "#9b6b52",
    tableEdge: "#7d523d",
    tint: "#ff8a6a",
    tintOpacity: 0.14,
    wallpaper: ["#ffb088", "#c46aa0", "#4b3f8f"],
  },
  night: {
    skyTop: "#10143a",
    skyBottom: "#2b3170",
    wall: "#2d3158",
    wallShade: "#232649",
    table: "#5a4636",
    tableEdge: "#45352a",
    tint: "#1a1f5c",
    tintOpacity: 0.18,
    wallpaper: ["#2b2f6b", "#1b1d44", "#0e0f24"],
  },
};

export const MONO = '"JetBrains Mono", ui-monospace, monospace';
