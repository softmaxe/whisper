const test = require("node:test");
const assert = require("node:assert/strict");
const {
  distribution,
  summarizeObservations,
  summarizeStartup,
  summarizeCompletion,
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
