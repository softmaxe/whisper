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

// The pill's rendered footprints are a native-window contract (see
// VOICE_PILL_FOOTPRINT); every footprint assertion derives from the exported
// constants so the literals live in exactly one place.
const pillFootprints = async () => {
  const { VOICE_PILL_FOOTPRINT } = await import("../../src/helpers/voicePillPresentation.js");
  const asStyle = ({ width, height }) => new RegExp(`style="width:${width}px;height:${height}px`);
  return {
    idle: asStyle(VOICE_PILL_FOOTPRINT.idle),
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

  for (const markup of [thinking, recording]) {
    assert.match(markup, /^<span class="voice-pill-glow-anchor"/);
    assert.match(markup, /class="processing-signal-glow"/);
    assert.match(markup, /voice-pill-control/);
  }
  assert.match(thinking, /class="processing-signal-glow" data-active="true"/);
  assert.doesNotMatch(recording, /data-active/);
});

test("the pill renders exactly the footprints the native window ladder is sized around", async () => {
  const footprint = await pillFootprints();
  const idle = await renderPill("idle", false);
  const listening = await renderPill("recording", true);
  const panel = await renderPill("recording", true, "right", PANEL_RECORDING);

  assert.match(idle, footprint.idle);
  assert.doesNotMatch(idle, footprint.listening);
  assert.match(listening, footprint.listening);
  assert.doesNotMatch(listening, footprint.idle);
  assert.match(panel, footprint.panel);
});

test("the floating listening pill is an always-black Flow bar without the identity", async () => {
  const listening = await renderPill("recording", true);
  const idle = await renderPill("idle", false);

  assert.match(listening, /data-flow-bar="true"/);
  assert.doesNotMatch(idle, /data-flow-bar/);
  // The ink chrome comes from .voice-pill-control[data-flow-bar], not the
  // theme-driven STATE_APPEARANCE surface classes.
  assert.doesNotMatch(listening, /voice-pill-control[^"\n]*bg-surface-1/);
  // The identity slot collapses out of the flow instead of unmounting, so the
  // logo can morph back when recording stops.
  assert.match(listening, /voice-pill-identity-slot[^"\n]*" style="width:0;height:22px;opacity:0/);
  assert.doesNotMatch(listening, /voice-pill-control[^"\n]*pr-1/);
});

test("a Flow bar that loses its microphone pulses resting dots under a visible ring", async () => {
  const unavailable = await renderPill("unavailable", true);

  assert.match(unavailable, /data-flow-bar="true"/);
  assert.match(unavailable, /voice-flow-waveform[^"\n]*animate-pulse/);
  assert.match(unavailable, /rounded-full border-2 animate-pulse border-white\/30/);
  assert.doesNotMatch(unavailable, /border-foreground\/30/);
});

test("the Flow bar renders one symmetric bar set and hides it until the entrance reveals it", async () => {
  const shown = await renderPill("recording", true);
  const entering = await renderPill("recording", true, "right", { waveformVisible: false });
  const count = await flowBarCount();

  assert.equal((shown.match(/voice-flow-bar"/g) || []).length, count);
  assert.doesNotMatch(shown, /w-0\.5 rounded-full bg-current/);
  assert.match(shown, /voice-flow-waveform[^>]*opacity:1/);
  assert.match(entering, /voice-flow-waveform[^>]*opacity:0/);
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

test("the idle pill keeps the logo at normal foreground strength", async () => {
  const idle = await renderPill("idle", false);

  assert.match(idle, /border-border-hover[^"\n]*dark:border-border\/50/);
  assert.match(
    idle,
    /voice-identity-icon relative inline-block shrink-0 transition-\[width,height\] duration-200 text-foreground/
  );
});

test("the floating hover pill changes surface treatment without zooming", async () => {
  const footprint = await pillFootprints();
  const hovered = await renderPill("hover", false);

  assert.match(hovered, /border-border-hover bg-surface-3 text-foreground/);
  assert.match(hovered, /box-shadow:var\(--shadow-card-hover-subtle\)/);
  assert.doesNotMatch(hovered, /style="[^"]*transform:/);
  assert.match(hovered, footprint.idle);
  assert.match(hovered, /<svg width="22" height="22"/);
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

test("the panel waveform uses foreground contrast, rounded caps, and a pronounced height range", async () => {
  const recording = await renderPill("recording", true, "right", PANEL_RECORDING);
  const { WAVEFORM_BAR_MIN_PX, WAVEFORM_BAR_MAX_PX, resolveWaveformBarHeight } =
    await import("../../src/components/dictation/waveformMath.ts");

  assert.match(recording, /relative shrink-0 overflow-hidden text-foreground/);
  assert.equal(
    (recording.match(/w-0\.5 rounded-full bg-current/g) || []).length,
    await totalWaveBars()
  );
  assert.equal(WAVEFORM_BAR_MIN_PX, 4);
  assert.equal(WAVEFORM_BAR_MAX_PX, 22);
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
