const test = require("node:test");
const assert = require("node:assert/strict");

const { renderStatic } = require("./harness/reactSsr");

// The dictation-window feature styles are split out of index.css; selectors
// under test may live in either file.

// The compact panel pill renders the resting silhouette and the live waveform
// as two stacked bar sets, both sized from the shared bar count.
const totalWaveBars = async () => {
  const { WAVEFORM_BAR_COUNT } = await import("../../src/components/dictation/waveformMath.ts");
  return WAVEFORM_BAR_COUNT * 2;
};

const flowBarCount = async () => {
  const { FLOW_BAR_COUNT } = await import("../../src/components/dictation/waveformMath.ts");
  return FLOW_BAR_COUNT;
};

const PANEL_RECORDING = { variant: "panel", integratedWithPanel: true };
// The panel's own resting identity: the circle it collapses to between takes.
const PANEL_IDLE = {
  variant: "panel",
  integratedWithPanel: true,
  waveformOnlyWhileRecording: true,
};

// The pill's rendered footprints are a native-window contract (see
// VOICE_PILL_FOOTPRINT); every footprint assertion derives from the exported
// constants so the literals live in exactly one place.
const pillFootprints = async () => {
  const { VOICE_PILL_FOOTPRINT } = await import("../../src/helpers/voicePillPresentation.js");
  const asStyle = ({ width, height }) => new RegExp(`style="width:${width}px;height:${height}px`);
  return {
    idle: asStyle(VOICE_PILL_FOOTPRINT.idle),
    sliver: asStyle(VOICE_PILL_FOOTPRINT.sliver),
    peek: asStyle(VOICE_PILL_FOOTPRINT.peek),
    listening: asStyle(VOICE_PILL_FOOTPRINT.listening),
    panel: asStyle(VOICE_PILL_FOOTPRINT.panel),
  };
};

const renderPill = async (state, expanded, horizontalDirection = "right", overrides = {}) => {
  const { VoicePill } = await import("../../src/components/dictation/VoicePill.tsx");
  return renderStatic(VoicePill, {
    variant: "floating",
    state,
    expanded,
    horizontalDirection,
    getAudioLevel: () => 0,
    ...overrides,
  });
};

test("thinking and recording keep the same persistent glow and pill roots", async () => {
  const thinking = await renderPill("thinking", false);
  const recording = await renderPill("recording", true);
  const panelThinking = await renderPill("thinking", false, "right", PANEL_IDLE);

  for (const markup of [thinking, recording, panelThinking]) {
    assert.match(markup, /^<span class="voice-pill-glow-anchor"/);
    assert.match(markup, /class="processing-signal-glow"/);
    assert.match(markup, /voice-pill-control/);
  }
  // The floating Flow bar thinks with its own bars; only the panel's circular
  // identity still lights the Signal glow.
  assert.doesNotMatch(thinking, /data-active/);
  assert.doesNotMatch(recording, /data-active/);
  assert.match(panelThinking, /class="processing-signal-glow" data-active="true"/);
});

test("the pill renders exactly the footprints the native window ladder is sized around", async () => {
  const footprint = await pillFootprints();
  const sliver = await renderPill("idle", false);
  const peek = await renderPill("hover", false);
  const listening = await renderPill("recording", true);
  const panel = await renderPill("recording", true, "right", PANEL_RECORDING);
  const panelIdle = await renderPill("idle", false, "right", PANEL_IDLE);

  assert.match(sliver, footprint.sliver);
  assert.match(peek, footprint.peek);
  assert.match(listening, footprint.listening);
  assert.doesNotMatch(listening, footprint.idle);
  assert.match(panel, footprint.panel);
  assert.match(panelIdle, footprint.idle);
});

test("the floating pill is always-black Flow chrome without the identity in every state", async () => {
  const panelIdle = await renderPill("idle", false, "right", PANEL_IDLE);

  for (const [state, expanded] of [
    ["idle", false],
    ["hover", false],
    ["processing", false],
    ["recording", true],
    ["thinking", false],
  ]) {
    const markup = await renderPill(state, expanded);
    assert.match(markup, /data-flow-bar="true"/, state);
    // The ink chrome comes from .voice-pill-control[data-flow-bar], not the
    // theme-driven STATE_APPEARANCE surface classes.
    assert.doesNotMatch(markup, /voice-pill-control[^"\n]*bg-surface-/, state);
    // The identity slot stays mounted but collapsed, so the panel layout can
    // grow it back when Live Transcript takes the pill.
    assert.match(markup, /voice-pill-identity-slot[^"\n]*" style="width:0;height:22px;opacity:0/);
    assert.doesNotMatch(markup, /voice-pill-control[^"\n]*pr-1/);
  }
  assert.doesNotMatch(panelIdle, /data-flow-bar/);
});

test("the floating pill's bars say what it is doing", async () => {
  const { FLOW_PEEK_OPACITY } = await import("../../src/components/dictation/waveformMath.ts");
  const sliver = await renderPill("idle", false);
  const peek = await renderPill("hover", false);
  const warmUp = await renderPill("processing", false);
  const thinking = await renderPill("thinking", false);

  // The resting sliver is empty; hovering it previews dim dots.
  assert.match(sliver, /voice-flow-waveform[^>]*opacity:0[;"]/);
  assert.match(peek, new RegExp(`voice-flow-waveform[^>]*opacity:${FLOW_PEEK_OPACITY}[;"]`));
  assert.doesNotMatch(peek, /data-motion/);
  // Warm-up sweeps a light across the dots; thinking ripples a travelling wave.
  assert.match(warmUp, /voice-flow-waveform[^>]*data-motion="sweep"[^>]*opacity:1/);
  assert.match(thinking, /voice-flow-waveform[^>]*data-motion="wave"[^>]*opacity:1/);
});

test("a Flow bar that loses its microphone pulses resting dots under a visible ring", async () => {
  const unavailable = await renderPill("unavailable", true);

  assert.match(unavailable, /data-flow-bar="true"/);
  assert.match(unavailable, /voice-flow-waveform[^"\n]*animate-pulse/);
  assert.match(unavailable, /rounded-full border-2 animate-pulse/);
});

test("the Flow bar renders one symmetric bar set that goes live as soon as recording starts", async () => {
  const shown = await renderPill("recording", true);
  const entering = await renderPill("recording", false, "right", {
    collapseToLogo: true,
    waveformVisible: false,
  });
  const count = await flowBarCount();

  assert.equal((shown.match(/voice-flow-bar"/g) || []).length, count);
  assert.doesNotMatch(shown, /w-0\.5 rounded-full bg-current/);
  assert.match(shown, /voice-flow-waveform[^>]*data-motion="live"[^>]*opacity:1/);
  // The entrance beats stage only the panel pill; the floating bar never makes
  // a capturing microphone look like it is still warming up.
  assert.match(entering, /voice-flow-waveform[^>]*data-motion="live"[^>]*opacity:1/);
});

test("the waveform stays to the right of the identity across docks and voice modes", async () => {
  const right = await renderPill("recording", true, "right", PANEL_RECORDING);
  const left = await renderPill("recording", true, "left", PANEL_RECORDING);
  const leftLiveTranscript = await renderPill("recording", true, "left", {
    variant: "panel",
    integratedWithPanel: true,
  });

  assert.match(right, /data-horizontal-direction="right"/);
  assert.match(right, /voice-pill-control[^"\n]*pr-1/);
  assert.match(left, /data-horizontal-direction="left"/);
  assert.match(left, /voice-pill-control[^"\n]*pr-1/);

  const floatingLeft = await renderPill("recording", true, "left");
  assert.match(floatingLeft, /data-horizontal-direction="left"/);
  assert.doesNotMatch(floatingLeft, /flex-row-reverse/);

  for (const markup of [right, left, leftLiveTranscript]) {
    assert.doesNotMatch(markup, /flex-row-reverse/);
    assert.ok(markup.indexOf("voice-pill-identity-slot") >= 0);
    assert.ok(markup.indexOf("voice-pill-waveform") > markup.indexOf("voice-pill-identity-slot"));
  }
});

test("the collapsed Live Transcript pill swaps its Flow bar for an expand chevron", async () => {
  const resting = await renderPill("recording", true);
  const hovered = await renderPill("recording", true, "right", {
    showExpandChevron: true,
  });

  assert.doesNotMatch(resting, /data-expand-chevron/);
  assert.match(resting, /voice-flow-chevron[^"\n]*opacity-0/);
  assert.match(resting, /voice-flow-waveform[^>]*opacity:1/);
  assert.match(hovered, /data-expand-chevron="true"/);
  assert.match(hovered, /voice-flow-chevron[^"\n]*scale-100 opacity-100/);
  assert.match(hovered, /voice-flow-waveform[^>]*opacity:0/);
});

test("the compact panel pill transitions its logo into an expand chevron", async () => {
  const resting = await renderPill("recording", true, "right", PANEL_RECORDING);
  const hovered = await renderPill("recording", true, "right", {
    ...PANEL_RECORDING,
    showExpandChevron: true,
  });

  assert.match(resting, /voice-pill-identity-logo[^"\n]*scale-100 opacity-100/);
  assert.match(resting, /voice-pill-expand-chevron[^"\n]*opacity-0/);
  assert.match(hovered, /voice-pill-identity-logo[^"\n]*opacity-0/);
  assert.match(hovered, /voice-pill-expand-chevron[^"\n]*scale-100 opacity-100/);
  assert.ok(hovered.indexOf("voice-pill-expand-chevron") < hovered.indexOf("voice-pill-waveform"));

  const leftHovered = await renderPill("recording", true, "left", {
    ...PANEL_RECORDING,
    showExpandChevron: true,
  });
  assert.doesNotMatch(leftHovered, /flex-row-reverse/);
  assert.ok(
    leftHovered.indexOf("voice-pill-expand-chevron") < leftHovered.indexOf("voice-pill-waveform")
  );
});

test("hovering the sliver grows it to the peek capsule without zooming", async () => {
  const footprint = await pillFootprints();
  const hovered = await renderPill("hover", false);

  assert.match(hovered, footprint.peek);
  assert.doesNotMatch(hovered, /style="[^"]*transform:/);
  assert.doesNotMatch(hovered, /--shadow-card-hover-subtle/);
});

test("the waveform pill keeps the normal compact logo footprint", async () => {
  const idle = await renderPill("idle", false);
  const recording = await renderPill("recording", true);
  const liveTranscript = await renderPill("recording", true, "right", {
    variant: "panel",
    integratedWithPanel: true,
  });

  for (const markup of [idle, recording, liveTranscript]) {
    assert.match(markup, /<svg width="22" height="22"/);
  }
});

test("an interactive voice pill is keyboard focusable", async () => {
  const interactive = await renderPill("recording", true, "right", {
    role: "button",
    tabIndex: 0,
  });

  assert.match(interactive, /role="button"/);
  assert.match(interactive, /tabindex="0"/);
});

test("the panel waveform uses rounded caps and a pronounced height range", async () => {
  const recording = await renderPill("recording", true, "right", PANEL_RECORDING);
  const { WAVEFORM_BAR_MIN_PX, WAVEFORM_BAR_MAX_PX, resolveWaveformBarHeight } =
    await import("../../src/components/dictation/waveformMath.ts");

  assert.equal(
    (recording.match(/w-0\.5 rounded-full bg-current/g) || []).length,
    await totalWaveBars()
  );
  assert.equal(resolveWaveformBarHeight(0), WAVEFORM_BAR_MIN_PX);
  assert.equal(resolveWaveformBarHeight(1), WAVEFORM_BAR_MAX_PX);
  assert.ok(resolveWaveformBarHeight(0.15) > 20);
});

test("Live Transcript hands visual border ownership to the shared panel", async () => {
  const integrated = await renderPill("recording", true, "right", {
    variant: "panel",
    integratedWithPanel: true,
  });
  const standalone = await renderPill("recording", true);

  assert.match(integrated, /voice-pill-control/);
  assert.match(integrated, /data-integrated-with-panel="true"/);
  assert.doesNotMatch(standalone, /data-integrated-with-panel/);
});

test("Flow bar heights rest as dots in silence and peak at the center while speaking", async () => {
  const { FLOW_BAR_COUNT, FLOW_BAR_ENVELOPE, resolveFlowBarTarget } =
    await import("../../src/components/dictation/waveformMath.ts");

  assert.equal(FLOW_BAR_ENVELOPE.length, FLOW_BAR_COUNT);
  assert.deepEqual([...FLOW_BAR_ENVELOPE].reverse(), [...FLOW_BAR_ENVELOPE]);

  for (let index = 0; index < FLOW_BAR_COUNT; index += 1) {
    assert.equal(resolveFlowBarTarget(0, index, 1234), 0);
  }
  // Across a stretch of loud speech the center bars reach higher than the edges,
  // and no bar ever leaves the 0..1 lane.
  const peak = (index) => {
    let max = 0;
    for (let now = 0; now < 2000; now += 16) {
      const target = resolveFlowBarTarget(0.2, index, now);
      assert.ok(target >= 0 && target <= 1);
      max = Math.max(max, target);
    }
    return max;
  };
  const center = FLOW_BAR_COUNT / 2;
  assert.ok(peak(center) > peak(0) * 2);
  assert.ok(peak(center - 1) > peak(FLOW_BAR_COUNT - 1) * 2);
});

test("the warm-up sweep carries one bright spot across the resting dots", async () => {
  const { FLOW_BAR_COUNT, resolveFlowSweepOpacity } =
    await import("../../src/components/dictation/waveformMath.ts");

  const brightest = (now) => {
    let best = 0;
    for (let index = 1; index < FLOW_BAR_COUNT; index += 1) {
      if (resolveFlowSweepOpacity(index, now) > resolveFlowSweepOpacity(best, now)) best = index;
    }
    return best;
  };
  for (let now = 0; now < 3000; now += 16) {
    for (let index = 0; index < FLOW_BAR_COUNT; index += 1) {
      const opacity = resolveFlowSweepOpacity(index, now);
      assert.ok(opacity > 0.2 && opacity <= 1);
    }
  }
  // Mid-pass the spot moves left to right.
  const start = 300;
  assert.ok(brightest(start + 140) > brightest(start));
});

test("the thinking wave travels left to right and stays well below speech height", async () => {
  const { FLOW_BAR_COUNT, FLOW_WAVE_STEP_MS, resolveFlowWaveTarget } =
    await import("../../src/components/dictation/waveformMath.ts");

  let max = 0;
  let min = 1;
  for (let now = 0; now < 3000; now += 16) {
    for (let index = 0; index < FLOW_BAR_COUNT; index += 1) {
      const lane = resolveFlowWaveTarget(index, now);
      max = Math.max(max, lane);
      min = Math.min(min, lane);
    }
  }
  assert.ok(min >= 0 && max <= 0.5);
  assert.ok(max - min > 0.3);
  // A bar repeats its left neighbour's height a moment later.
  for (let index = 1; index < FLOW_BAR_COUNT; index += 1) {
    const here = resolveFlowWaveTarget(index, 1000 + FLOW_WAVE_STEP_MS);
    const neighbour = resolveFlowWaveTarget(index - 1, 1000);
    assert.ok(Math.abs(here - neighbour) < 1e-9);
  }
});
