/**
 * Shared Film palette: warm paper, graphite pencil, ink, the red pen and the
 * Whis mascot's black and white. Beats keep one-off colours as local constants.
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
  /** Whis's body and the white waveform W from the app icon. */
  whisBlack: "#1d1b1c",
  whisWhite: "#f7f3ea",
  /** Watercolour wash keyframes over the working day. */
  washDawn: "#f3c89a",
  washDay: "#f6e3b0",
  washDusk: "#e3a07f",
  washNight: "#3d4a78",
} as const;

/** The red-pen stroke style shared by every annotation. */
export const RED_PEN = { stroke: PALETTE.redPen, strokeWidth: 5, roughness: 1.1, bowing: 0.9 } as const;
