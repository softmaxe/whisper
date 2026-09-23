const test = require("node:test");
const assert = require("node:assert/strict");

const load = () => import("../../src/helpers/voicePillPresentation.js");

test("Live Transcript stages footer growth, controls, then content", async () => {
  const { resolveLiveTranscriptEntrancePresentation } = await load();

  assert.deepEqual(resolveLiveTranscriptEntrancePresentation("encapsulate"), {
    coreStage: "encapsulated",
    controlsVisible: false,
    contentVisible: false,
  });
  assert.deepEqual(resolveLiveTranscriptEntrancePresentation("horizontal"), {
    coreStage: "footer",
    controlsVisible: false,
    contentVisible: false,
  });
  assert.deepEqual(resolveLiveTranscriptEntrancePresentation("controls"), {
    coreStage: "footer",
    controlsVisible: true,
    contentVisible: false,
  });
  assert.deepEqual(resolveLiveTranscriptEntrancePresentation("prepare"), {
    coreStage: "footer",
    controlsVisible: true,
    contentVisible: false,
  });
  assert.deepEqual(resolveLiveTranscriptEntrancePresentation("panel"), {
    coreStage: "content",
    controlsVisible: true,
    contentVisible: false,
  });
  assert.deepEqual(resolveLiveTranscriptEntrancePresentation("content"), {
    coreStage: "content",
    controlsVisible: true,
    contentVisible: true,
  });
});

test("Live Transcript entrance beats land strictly after one another", async () => {
  const { getLiveTranscriptEntranceTimeline } = await load();
  const timeline = getLiveTranscriptEntranceTimeline();

  // The invariant is the visual order, not the exact sums: the encapsulated
  // hold ends before the footer grows, controls appear before content is
  // prepared, and streaming starts only after the content settles.
  assert.ok(timeline.horizontalAtMs > 0);
  assert.ok(timeline.controlsAtMs > timeline.horizontalAtMs);
  assert.ok(timeline.prepareAtMs > timeline.controlsAtMs);
  assert.ok(timeline.panelAtMs > timeline.prepareAtMs);
  assert.ok(timeline.contentAtMs > timeline.panelAtMs);
  assert.ok(timeline.streamAtMs > timeline.contentAtMs);
});

test("voice mode direction mirrors the right baseline only for bottom-left", async () => {
  const { resolveVoiceHorizontalDirection } = await load();

  assert.equal(resolveVoiceHorizontalDirection("bottom-right"), "right");
  assert.equal(resolveVoiceHorizontalDirection("bottom-left"), "left");
  assert.equal(resolveVoiceHorizontalDirection("center"), "right");
});

test("the speaking pill resolves right-origin modes onto one interpolable dock system", async () => {
  const { resolveVoicePillDock } = await load();

  assert.equal(
    resolveVoicePillDock({
      liveTranscriptOpen: true,
      liveTranscriptEntrancePhase: "horizontal",
      panelStartPosition: "bottom-right",
    }),
    "live-transcript-bottom-left"
  );
  assert.equal(
    resolveVoicePillDock({
      liveTranscriptOpen: true,
      liveTranscriptEntrancePhase: "encapsulate",
      panelStartPosition: "bottom-right",
    }),
    "live-transcript-encapsulated-bottom-right"
  );
});

test("a left-origin session keeps the speaking pill left while surfaces grow right", async () => {
  const { resolveVoicePillDock } = await load();

  assert.equal(
    resolveVoicePillDock({
      liveTranscriptOpen: true,
      liveTranscriptEntrancePhase: "encapsulate",
      panelStartPosition: "bottom-left",
    }),
    "live-transcript-encapsulated-bottom-left"
  );
  for (const liveTranscriptEntrancePhase of ["horizontal", "controls", "content"]) {
    assert.equal(
      resolveVoicePillDock({
        liveTranscriptOpen: true,
        liveTranscriptEntrancePhase,
        panelStartPosition: "bottom-left",
      }),
      "live-transcript-bottom-left"
    );
  }
});

test("the idle pill keeps its configured resting dock", async () => {
  const { resolveVoicePillDock } = await load();

  assert.equal(
    resolveVoicePillDock({
      liveTranscriptOpen: false,
      liveTranscriptEntrancePhase: "idle",
      panelStartPosition: "center",
    }),
    "center"
  );
});

test("Live Transcript restores stop and cancel interactions", async () => {
  const { resolveVoicePillInteraction } = await load();

  assert.deepEqual(
    resolveVoicePillInteraction({
      liveTranscriptMounted: true,
      isRecording: true,
      isProcessing: false,
    }),
    { pillInteractive: true, cancelVisible: true }
  );
  assert.deepEqual(
    resolveVoicePillInteraction({
      liveTranscriptMounted: true,
      isRecording: false,
      isProcessing: true,
    }),
    { pillInteractive: false, cancelVisible: true }
  );
  // The bare pill (no Live Transcript) exposes discard on hover, as before the panel.
  assert.deepEqual(
    resolveVoicePillInteraction({
      liveTranscriptMounted: false,
      isRecording: false,
      isProcessing: true,
      isHovered: true,
    }),
    { pillInteractive: true, cancelVisible: true }
  );
  assert.deepEqual(
    resolveVoicePillInteraction({
      liveTranscriptMounted: false,
      isRecording: false,
      isProcessing: true,
      isHovered: false,
    }),
    { pillInteractive: true, cancelVisible: false }
  );
});

test("a mounted Live Transcript can stop after the floating pill was previously dragged", async () => {
  const { shouldActivateVoicePill } = await load();

  assert.equal(
    shouldActivateVoicePill({
      hasDragged: true,
      liveTranscriptMounted: true,
      isProcessing: false,
    }),
    true
  );
  assert.equal(
    shouldActivateVoicePill({
      hasDragged: true,
      liveTranscriptMounted: false,
      isProcessing: false,
    }),
    false
  );
});

test("the interactive pill recognizes standard keyboard activation keys", async () => {
  const { isVoicePillActivationKey } = await load();

  assert.equal(isVoicePillActivationKey("Enter"), true);
  assert.equal(isVoicePillActivationKey(" "), true);
  assert.equal(isVoicePillActivationKey("Escape"), false);
});

test("a collapsed completed transcript leaves the normal pill interaction available", async () => {
  const { resolveVoicePillInteraction } = await load();

  assert.deepEqual(
    resolveVoicePillInteraction({
      liveTranscriptMounted: false,
      isRecording: false,
      isProcessing: false,
    }),
    { pillInteractive: true, cancelVisible: false }
  );
});

test("the actual window side overrides a stale edge preference", async () => {
  const { resolveVoicePillDock } = await load();

  assert.equal(
    resolveVoicePillDock({
      liveTranscriptOpen: false,
      liveTranscriptEntrancePhase: "idle",
      panelStartPosition: "bottom-right",
      horizontalDirection: "left",
    }),
    "bottom-left"
  );
  assert.equal(
    resolveVoicePillDock({
      liveTranscriptOpen: false,
      liveTranscriptEntrancePhase: "idle",
      panelStartPosition: "bottom-left",
      horizontalDirection: "right",
    }),
    "bottom-right"
  );
});

test("listening entrance starts in the thinking circle before expanding", async () => {
  const { resolveListeningEntrancePresentation } = await load();
  assert.deepEqual(resolveListeningEntrancePresentation({ isRecording: true, phase: "idle" }), {
    activeState: "recording",
    collapseToLogo: true,
    compactPill: false,
    waveformVisible: false,
  });
});

test("listening entrance expands before revealing the waveform", async () => {
  const { resolveListeningEntrancePresentation } = await load();
  assert.deepEqual(
    resolveListeningEntrancePresentation({ isRecording: true, phase: "expanding" }),
    {
      activeState: "recording",
      collapseToLogo: false,
      compactPill: true,
      waveformVisible: false,
    }
  );
});

test("listening entrance settles at full width before revealing the waveform", async () => {
  const { resolveListeningEntrancePresentation } = await load();
  const settled = resolveListeningEntrancePresentation({
    isRecording: true,
    phase: "settled",
  });
  const waveform = resolveListeningEntrancePresentation({
    isRecording: true,
    phase: "waveform",
  });

  assert.deepEqual(settled, {
    activeState: "recording",
    collapseToLogo: false,
    compactPill: true,
    waveformVisible: false,
  });
  assert.deepEqual(waveform, { ...settled, waveformVisible: true });
});

test("listening entrance timers preserve the visual order", async () => {
  const { getListeningEntranceTimeline } = await load();

  const timeline = getListeningEntranceTimeline();
  assert.ok(timeline.expandAtMs > 0);
  assert.ok(timeline.settleAtMs > timeline.expandAtMs);
  assert.ok(timeline.waveformAtMs > timeline.settleAtMs);
});

test("stopping during the entrance cancels the staged recording presentation", async () => {
  const { resolveListeningEntrancePresentation } = await load();
  assert.deepEqual(
    resolveListeningEntrancePresentation({ isRecording: false, phase: "expanding" }),
    {
      activeState: null,
      collapseToLogo: false,
      compactPill: false,
      waveformVisible: true,
    }
  );
});

test("regular dictation transcription contracts to the rotating thinking circle", async () => {
  const { resolveVoiceActivityPresentation } = await load();
  assert.deepEqual(
    resolveVoiceActivityPresentation({
      isRecording: false,
      isProcessing: true,
    }),
    { activeState: "thinking", compactPill: false }
  );
});

test("the voice panel core stays mounted but contentless while idle", async () => {
  const { resolveVoicePanelCorePresentation } = await load();

  assert.deepEqual(
    resolveVoicePanelCorePresentation({
      liveTranscriptOpen: false,
      liveTranscriptMounted: false,
    }),
    { mode: null, open: false }
  );
});

test("Live Transcript reopen belongs only to an active normal dictation", async () => {
  const { shouldOfferLiveTranscriptReopen } = await load();

  assert.equal(
    shouldOfferLiveTranscriptReopen({
      manuallyCollapsed: true,
      isRecording: true,
      isProcessing: false,
    }),
    true
  );
  assert.equal(
    shouldOfferLiveTranscriptReopen({
      manuallyCollapsed: true,
      isRecording: false,
      isProcessing: true,
    }),
    true
  );
  assert.equal(
    shouldOfferLiveTranscriptReopen({
      manuallyCollapsed: true,
      isRecording: false,
      isProcessing: false,
    }),
    false
  );
});
