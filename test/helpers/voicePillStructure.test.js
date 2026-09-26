const test = require("node:test");
const assert = require("node:assert/strict");

const { renderStatic } = require("./harness/reactSsr");

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

test("rendered pill dimensions match the native window footprint across states", async () => {
  const { VOICE_PILL_FOOTPRINT } = await import("../../src/helpers/voicePillPresentation.js");
  // Check shape selection through the rendered component so helper tests do not
  // repeat the same cases. Dimensions must agree with the native window sizing.
  for (const [state, expanded, overrides, shape] of [
    ["idle", false, {}, "sliver"],
    ["hover", false, {}, "peek"],
    ["processing", false, {}, "listening"],
    ["recording", false, {}, "listening"],
    ["recording", true, {}, "listening"],
    ["recording", false, { collapseToLogo: true }, "listening"],
    ["thinking", false, {}, "listening"],
    ["thinking", true, {}, "listening"],
    ["unavailable", true, {}, "listening"],
    ["recording", true, { variant: "panel", integratedWithPanel: true }, "panel"],
    ["idle", false, { variant: "panel" }, "panel"],
    ["idle", false, { variant: "panel", waveformOnlyWhileRecording: true }, "idle"],
    ["thinking", false, { variant: "panel" }, "idle"],
    ["recording", false, { variant: "panel", collapseToLogo: true }, "idle"],
  ]) {
    const markup = await renderPill(state, expanded, "right", overrides);
    const { width, height } = VOICE_PILL_FOOTPRINT[shape];
    assert.match(
      markup,
      new RegExp(`style="width:${width}px;height:${height}px`),
      JSON.stringify({ state, expanded, ...overrides })
    );
  }
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

test("the floating waveform goes live as soon as recording starts", async () => {
  const shown = await renderPill("recording", true);
  const entering = await renderPill("recording", false, "right", {
    collapseToLogo: true,
    waveformVisible: false,
  });
  assert.match(shown, /voice-flow-waveform[^>]*data-motion="live"[^>]*opacity:1/);
  // The entrance beats stage only the panel pill; the floating bar never makes
  // a capturing microphone look like it is still warming up.
  assert.match(entering, /voice-flow-waveform[^>]*data-motion="live"[^>]*opacity:1/);
});

test("the collapsed Live Transcript pill swaps its Flow bar for an expand chevron", async () => {
  const resting = await renderPill("recording", true);
  const hovered = await renderPill("recording", true, "right", {
    showExpandChevron: true,
  });

  assert.doesNotMatch(resting, /data-expand-chevron/);
  assert.match(resting, /voice-flow-waveform[^>]*opacity:1/);
  assert.match(hovered, /data-expand-chevron="true"/);
  assert.match(hovered, /voice-flow-waveform[^>]*opacity:0/);
});

test("an interactive voice pill is keyboard focusable", async () => {
  const interactive = await renderPill("recording", true, "right", {
    role: "button",
    tabIndex: 0,
  });

  assert.match(interactive, /role="button"/);
  assert.match(interactive, /tabindex="0"/);
});
