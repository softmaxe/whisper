/**
 * Shared Film palette: warm paper, graphite pencil, ink, the red pen, the app
 * icon's black and white, and Clawd's terracotta. Beats keep one-off colours
 * as local constants.
 */
export const PALETTE = {
  paper: "#f4ead5",
  paperShade: "#e6d6b4",
  /** Cream sheets, cards and app surfaces laid on the paper. */
  cream: "#fbf6ea",
  /** Near-white paper for windows, panels and notes that must stand out from the cream. */
  whitePaper: "#fffdf6",
  graphite: "#3b3230",
  pencil: "#6b5d55",
  ink: "#3a2e2a",
  /** The red pen: circles, strike-throughs, ticks, arrows and corrections. */
  redPen: "#c2562b",
  /** The app icon's black and white: the Recording pill and the logo. */
  iconBlack: "#1d1b1c",
  iconWhite: "#f7f3ea",
  /** Clawd's terracotta body and its dark eyes. */
  clawdOrange: "#d97757",
  clawdEye: "#221a18",
  /** Watercolour wash keyframes over the working day. */
  washDawn: "#f3c89a",
  washDay: "#f6e3b0",
  washDusk: "#e3a07f",
  washNight: "#2a4bb0",
} as const;

/** The red-pen stroke style shared by every annotation. */
export const RED_PEN = { stroke: PALETTE.redPen, strokeWidth: 5, roughness: 1.1, bowing: 0.9 } as const;
