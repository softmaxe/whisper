export const WAVEFORM_BAR_COUNT = 11;
export const WAVEFORM_BAR_MIN_PX = 4;
export const WAVEFORM_BAR_MAX_PX = 22;

// A pronounced eleven-bar rhythm keeps rounded short bars readable while tall
// peaks use nearly the full lane. Only the resting wave draws this shape —
// the panel's live wave renders the measured signal, never a decorative profile.
export const RESTING_WAVE_SILHOUETTE = [6, 12, 5, 9, 7, 22, 18, 5, 20, 12, 17];

// Conversational speech RMS sits around 0.02–0.15. The gain puts loud voicing
// at the top of the lane; the 0.75 exponent keeps quiet speech visible while
// preserving contrast between neighboring samples — a square-root curve
// compressed them into a fluid ridge with little bar-to-bar variance.
const LEVEL_GAIN = 8;
const LEVEL_EXPONENT = 0.75;
const toBarLevel = (rms: number) =>
  Math.min(1, Math.pow(Math.max(0, rms) * LEVEL_GAIN, LEVEL_EXPONENT));

export const resolveWaveformBarHeight = (rms: number) =>
  WAVEFORM_BAR_MIN_PX + toBarLevel(rms) * (WAVEFORM_BAR_MAX_PX - WAVEFORM_BAR_MIN_PX);

// The floating Flow bar: a symmetric set of bars that all follow the current
// level at once. Silence rests every bar at a dot; speech lifts them under a
// center-weighted envelope, so loudness reads as the whole shape swelling.
export const FLOW_BAR_COUNT = 10;
export const FLOW_BAR_MIN_PX = 3;
export const FLOW_BAR_MAX_PX = 18;
export const FLOW_BAR_ENVELOPE = Object.freeze([
  0.35, 0.55, 0.75, 0.92, 1, 1, 0.92, 0.75, 0.55, 0.35,
]);

// Each bar drifts on its own speed and phase so neighbors never move in
// lockstep; the drift only scales a bar between 55% and 100% of its envelope.
const FLOW_DRIFT_RATE_PER_MS = 0.012;
const FLOW_DRIFT_SPEED_STEP = 0.37;
const FLOW_DRIFT_PHASE_STEP = 1.7;
const FLOW_DRIFT_FLOOR = 0.55;

/**
 * Target height (0..1 of the Flow bar lane) for one bar. The level gates
 * everything, so silence stays flat.
 */
export const resolveFlowBarTarget = (rms: number, index: number, nowMs: number) => {
  const phase =
    nowMs * FLOW_DRIFT_RATE_PER_MS * (1 + index * FLOW_DRIFT_SPEED_STEP) +
    index * FLOW_DRIFT_PHASE_STEP;
  const drift = 0.5 + 0.5 * Math.sin(phase);
  return (
    toBarLevel(rms) * FLOW_BAR_ENVELOPE[index] * (FLOW_DRIFT_FLOOR + (1 - FLOW_DRIFT_FLOOR) * drift)
  );
};

export const resolveFlowBarHeight = (lane: number) =>
  FLOW_BAR_MIN_PX + lane * (FLOW_BAR_MAX_PX - FLOW_BAR_MIN_PX);
