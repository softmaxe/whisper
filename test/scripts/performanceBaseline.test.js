const test = require("node:test");
const assert = require("node:assert/strict");
const {
  distribution,
  summarizeObservations,
  summarizeStartup,
  summarizeCompletion,
  summarizeNative,
  resourceObservation,
} = require("../../scripts/lib/performance-baseline");

const logRecord = (record) =>
  `[2026-09-20T00:00:00.000Z] [INFO][audio] Recording startup ${JSON.stringify(record, null, 2)}\n`;
const completed = (requestId, sequence = 10) => ({
  requestId,
  sequence,
  captureAttempt: 1,
  captureSource: "prepared",
  outcome: "completed",
  stages: {
    requestAccepted: 0,
    acquisitionRequested: 12,
    acquisitionCompleted: 40,
    firstAudio: 50,
    readyFeedback: 55,
  },
});

const nativeRecord = (patch = {}) => ({
  collectionId: "abcdef01-2345-6789-abcd-ef0123456789",
  requestId: "01234567-89ab-cdef-0123-456789abcdef",
  sequence: 3,
  origin: "hold",
  outcome: "completed",
  startup: "completed",
  delivery: "pasted",
  cleanup: "absent",
  stages: {
    requestAccepted: 0,
    acquisitionRequested: 5,
    captureConfigured: 10,
    captureStartRequested: 11,
    firstAudio: 25,
    readyFeedback: 150,
    captureStartReturned: 200,
    stopAccepted: 300,
    captureReleased: 310,
    recordingFinalized: 315,
    asrPreparationStarted: 320,
    asrRequestDispatched: 350,
    asrResponseReceived: 550,
    asrResponseCompleted: 555,
    processingComplete: 560,
    deliveryStarted: 561,
    pasteDispatched: 580,
    pasteSettled: 585,
    requestFinished: 586,
  },
  attempts: [
    {
      service: "asr",
      index: 0,
      dispatchedMs: 350,
      responseMs: 550,
      outcome: "completed",
      status: 200,
    },
  ],
  droppedRecords: 0,
  ...patch,
});
const nativeLog = (...records) =>
  [{ schema: "whisper-native-timing", version: 1 }, ...records]
    .map((row) => JSON.stringify(row))
    .join("\n") + "\n";

test("native summary preserves capture overlap and distinguishes multipart and transport work", () => {
  const result = summarizeNative(nativeLog(nativeRecord()));
  assert.equal(result.requestCount, 1);
  assert.equal(result.metrics.shortcutToFirstAudio.median, 25);
  assert.equal(result.metrics.captureStartDuration.median, 189);
  assert.equal(result.metrics.firstAudioToFeedback.median, 125);
  assert.equal(result.metrics.inputAcquisition.outcomes.excluded, 1);
  assert.equal(result.metrics.acquisitionToFirstAudio.median, null);
  assert.equal(result.metrics.stopToCaptureRelease.median, 10);
  assert.equal(result.metrics.recordingFinalization.median, 5);
  assert.equal(result.metrics.asrPreparation.median, 30);
  assert.equal(result.metrics.stopToRequestDispatch.median, 50);
  assert.equal(result.metrics.serverRoundTrip.median, 205);
  assert.equal(result.metrics.processingCompleteToPasteDispatch.median, 20);
  assert.equal(result.metrics.processingCompleteToPaste.median, 25);
  assert.equal(result.metrics.cleanupRoundTrip.outcomes.missing, 1);
  assert.equal(result.serviceAttempts.asr.completed, 1);
  assert.doesNotMatch(JSON.stringify(result), /01234567|requestId|stages|status/);
});

test("native startup outcomes stay independent of later failures and cancellation", () => {
  const outcomes = ["completed", "failed", "cancelled", "pending", "rejected", "incomplete"];
  const records = outcomes.map((outcome, index) =>
    nativeRecord({
      requestId: `00000000-0000-0000-0000-${String(index).padStart(12, "0")}`,
      outcome,
      startup: ["failed", "cancelled"].includes(outcome) ? "completed" : outcome,
    })
  );
  const result = summarizeNative(nativeLog(...records));
  assert.deepEqual(
    result.requestOutcomes,
    Object.fromEntries(outcomes.map((outcome) => [outcome, 1]))
  );
  assert.equal(result.metrics.shortcutToFirstAudio.outcomes.success, 3);
  assert.deepEqual(result.metrics.serverRoundTrip.outcomes, {
    success: 1,
    failed: 1,
    cancelled: 1,
    missing: 2,
    excluded: 1,
  });
  const missing = nativeRecord({ stages: { requestAccepted: 0 }, delivery: "copied" });
  const negativeInterval = nativeRecord({
    stages: { ...nativeRecord().stages, pasteSettled: 500 },
  });
  assert.equal(
    summarizeNative(nativeLog(missing)).metrics.shortcutToFirstAudio.outcomes.missing,
    1
  );
  assert.equal(
    summarizeNative(nativeLog(negativeInterval)).metrics.processingCompleteToPaste.outcomes.missing,
    1
  );
  assert.equal(
    summarizeNative(nativeLog(nativeRecord({ origin: "button" }))).metrics.shortcutToFirstAudio
      .outcomes.excluded,
    1
  );
  assert.equal(
    summarizeNative(nativeLog(nativeRecord({ origin: "pill" }))).metrics.shortcutToFirstAudio
      .outcomes.excluded,
    1
  );
});

test("native terminal snapshots cannot revive and aggregate drop counts remain visible", () => {
  const final = nativeRecord({ droppedRecords: 4 });
  const result = summarizeNative(
    nativeLog(
      nativeRecord({ sequence: 1, outcome: "pending", startup: "pending" }),
      final,
      nativeRecord({ sequence: 4, outcome: "pending", droppedRecords: 8 }),
      nativeRecord({ sequence: 2, outcome: "pending" })
    )
  );
  assert.equal(result.requestCount, 1);
  assert.equal(result.requestOutcomes.completed, 1);
  assert.equal(result.requestOutcomes.pending, 0);
  assert.equal(result.lateRecords, 1);
  assert.equal(result.droppedRecords, 8);
  assert.equal(result.metrics.serverRoundTrip.median, 205);
  const reopened = summarizeNative(
    nativeLog(
      final,
      nativeRecord({
        requestId: "11111111-1111-1111-1111-111111111111",
        collectionId: "22222222-2222-2222-2222-222222222222",
        droppedRecords: 3,
      })
    )
  );
  assert.equal(reopened.droppedRecords, 7);
});

test("native cleanup retry and soft fallback are not single successful round trips", () => {
  const attempts = [
    ...nativeRecord().attempts,
    {
      service: "cleanup",
      index: 1,
      dispatchedMs: 570,
      responseMs: 600,
      outcome: "failed",
      status: 400,
    },
    {
      service: "cleanup",
      index: 2,
      dispatchedMs: 610,
      responseMs: 650,
      outcome: "completed",
      status: 200,
    },
  ];
  const record = nativeRecord({
    cleanup: "completed",
    attempts,
    stages: { ...nativeRecord().stages, cleanupPreparationStarted: 560, cleanupCompleted: 655 },
  });
  const retried = summarizeNative(nativeLog(record));
  assert.equal(retried.metrics.cleanupRoundTrip.outcomes.excluded, 1);
  assert.equal(retried.metrics.cleanupProcessing.median, 95);
  assert.deepEqual(retried.serviceAttempts.cleanup, {
    pending: 0,
    completed: 1,
    failed: 1,
    cancelled: 0,
  });
  const fallback = summarizeNative(nativeLog({ ...record, cleanup: "failed" }));
  assert.equal(fallback.requestOutcomes.completed, 1);
  assert.equal(fallback.metrics.cleanupRoundTrip.outcomes.failed, 1);
  assert.equal(fallback.metrics.cleanupProcessing.outcomes.failed, 1);
  const single = { ...record, attempts: [attempts[0], { ...attempts[2], index: 1 }] };
  assert.equal(summarizeNative(nativeLog(single)).metrics.cleanupRoundTrip.median, 40);
});

test("native reader rejects unknown or malformed content without echoing private input", () => {
  const secret = "private-input-fixture";
  for (const row of [
    nativeRecord({ transcript: secret }),
    nativeRecord({ origin: secret }),
    nativeRecord({ stages: { requestAccepted: 0, [secret]: 9 } }),
    nativeRecord({ stages: { requestAccepted: 0, firstAudio: -1 } }),
    nativeRecord({ stages: { requestAccepted: 0, firstAudio: null } }),
    nativeRecord({ attempts: [{ service: "asr", index: 0, outcome: "completed", responseMs: 2 }] }),
    nativeRecord({ attempts: [{ service: "asr", index: 0, outcome: "failed", status: 999 }] }),
    nativeRecord({ sequence: 0 }),
  ]) {
    assert.throws(
      () => summarizeNative(nativeLog(row)),
      (error) => error.message === "Invalid native timing data" && !error.message.includes(secret)
    );
  }
  for (const text of [
    secret,
    nativeLog(nativeRecord()).slice(0, -1),
    `{${secret}\n`,
    nativeLog({ schema: secret, version: 1 }),
  ]) {
    assert.throws(
      () => summarizeNative(text),
      (error) => !error.message.includes(secret)
    );
  }
});

test("reports nearest-rank p90 and leaves empty measurements absent", () => {
  assert.deepEqual(distribution([4, 1, 3, 2]), { count: 4, median: 2.5, p90: 4, min: 1, max: 4 });
  assert.deepEqual(distribution([]), { count: 0, median: null, p90: null, min: null, max: null });
  assert.equal(distribution(Array.from({ length: 20 }, (_, index) => index + 1)).p90, 18);
});

test("keeps failures, cancellation, missing and excluded samples out of successful latency", () => {
  const result = summarizeObservations([
    { metric: "coldLaunch", outcome: "success", value: 80 },
    { metric: "coldLaunch", outcome: "success", value: 100 },
    ...["failed", "cancelled", "missing", "excluded"].map((outcome) => ({
      metric: "coldLaunch",
      outcome,
    })),
  ]);
  assert.equal(result.coldLaunch.median, 90);
  assert.deepEqual(result.coldLaunch.outcomes, {
    success: 2,
    failed: 1,
    cancelled: 1,
    missing: 1,
    excluded: 1,
  });
  assert.equal(result.idleEnergy.median, null);
  for (const value of [null, undefined, NaN, Infinity, -1, "8"]) {
    assert.throws(() =>
      summarizeObservations([{ metric: "coldLaunch", outcome: "success", value }])
    );
  }
  assert.throws(() =>
    summarizeObservations([{ metric: "coldLaunch", outcome: "failed", value: 0 }])
  );
});

test("public summaries reject arbitrary fields without echoing their contents", () => {
  const secret = "private-fixture-label";
  assert.throws(
    () =>
      summarizeObservations([
        { metric: "idleRss", outcome: "success", value: 1, transcript: secret },
      ]),
    (error) => !error.message.includes(secret)
  );
  assert.throws(() =>
    summarizeObservations([{ metric: "constructor", outcome: "success", value: 1 }])
  );
});

test("startup summary groups asynchronous records, excludes late events, and exports no private fields", () => {
  const terminal = { ...completed("request-A"), ignoredPrivateMetadata: "private-fixture-label" };
  const log = [
    '[2026-09-20T00:00:00.000Z] [INFO][other] Private content {"text":"private-fixture-label"}\n',
    logRecord(terminal),
    logRecord({
      ...terminal,
      sequence: 11,
      lateStage: "readyCueScheduled",
      stages: { firstAudio: 900 },
    }),
    logRecord({ ...terminal, sequence: 2, outcome: "pending", stages: { requestAccepted: 0 } }),
    logRecord({
      requestId: "main-only",
      sequence: 0,
      outcome: "pending",
      stages: { requestAccepted: 0 },
    }),
  ].join("");
  const result = summarizeStartup(log);
  assert.equal(result.requestCount, 2);
  assert.equal(result.lateEvents, 1);
  assert.equal(result.metrics.shortcutToReadyFeedback.median, 55);
  assert.equal(result.metrics.inputAcquisition.median, 28);
  assert.equal(result.metrics.firstAudioToFeedback.median, 5);
  assert.equal(result.metrics.shortcutToFirstAudio.outcomes.missing, 1);
  assert.doesNotMatch(JSON.stringify(result), /request-A|private-fixture-label|main-only/);
});

test("does not merge capture attempts or count failed starts as successful zero-time observations", () => {
  const log = [
    logRecord({ ...completed("retry"), captureAttempt: 2 }),
    logRecord({ ...completed("cancel"), outcome: "cancelled" }),
    logRecord({ ...completed("failure"), outcome: "failed" }),
    logRecord({ ...completed("no-frame"), stages: { requestAccepted: 0 } }),
    logRecord({ ...completed("negative"), stages: { requestAccepted: 0, firstAudio: -1 } }),
  ].join("");
  const result = summarizeStartup(log).metrics.shortcutToFirstAudio;
  assert.deepEqual(result.outcomes, {
    success: 0,
    failed: 1,
    cancelled: 1,
    missing: 2,
    excluded: 1,
  });
  assert.equal(result.median, null);
  assert.throws(
    () => summarizeStartup(logRecord(completed("truncated")).slice(0, -3)),
    /Truncated/
  );
});

test("process sampling sums RSS and interval CPU and flags process churn", () => {
  const previous = [
    { pid: 1, rss: 1024, cpu: 1 },
    { pid: 2, rss: 2048, cpu: 2 },
  ];
  const current = [
    { pid: 1, rss: 2048, cpu: 1.02 },
    { pid: 2, rss: 2048, cpu: 2.08 },
  ];
  const result = resourceObservation(previous, current, 2);
  assert.equal(result[0].value, 4);
  assert.ok(Math.abs(result[1].value - 5) < 0.00001);
  assert.equal(resourceObservation(previous, current.slice(0, 1), 2)[1].outcome, "missing");
  assert.equal(resourceObservation(previous, current, 0)[1].outcome, "missing");
});

test("completion reports separate client dispatch, server round trip and paste bridge timing", () => {
  const complete = {
    requestId: "synthetic-completion",
    sequence: 8,
    outcome: "completed",
    stages: {
      stopAccepted: 100,
      asrRequestDispatched: 125,
      asrResponseCompleted: 325,
      processingComplete: 400,
      pasteDispatched: 410,
      pasteSettled: 475,
    },
  };
  const write = (record) =>
    logRecord(record).replace(
      "[audio] Recording startup",
      "[performance][renderer] Dictation completion"
    );
  const result = summarizeCompletion(
    [
      write(complete),
      write({ ...complete, sequence: 9, lateStage: "pasteSettled", elapsedMs: 900 }),
      write({ ...complete, requestId: "cancelled", outcome: "cancelled" }),
      write({
        ...complete,
        requestId: "clipboard-only",
        stages: { stopAccepted: 100, asrRequestDispatched: 125 },
      }),
    ].join("")
  );
  assert.equal(result.requestCount, 3);
  assert.equal(result.lateEvents, 1);
  assert.equal(result.metrics.stopToRequestDispatch.median, 25);
  assert.equal(result.metrics.serverRoundTrip.median, 200);
  assert.equal(result.metrics.processingCompleteToPasteDispatch.median, 10);
  assert.equal(result.metrics.processingCompleteToPaste.median, 75);
  assert.equal(result.metrics.processingCompleteToPaste.outcomes.cancelled, 1);
  assert.equal(result.metrics.processingCompleteToPaste.outcomes.missing, 1);
  assert.doesNotMatch(JSON.stringify(result), /synthetic-completion|clipboard-only/);
});
