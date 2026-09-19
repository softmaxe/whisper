const { execFileSync } = require("node:child_process");

const METRICS = Object.freeze({
  coldLaunch: "ms",
  windowAvailable: "ms",
  routineInteraction: "ms",
  shortcutToFirstAudio: "ms",
  shortcutToReadyFeedback: "ms",
  clientBeforeAcquisition: "ms",
  inputAcquisition: "ms",
  acquisitionToFirstAudio: "ms",
  firstAudioToFeedback: "ms",
  stopToRequestDispatch: "ms",
  processingCompleteToPasteDispatch: "ms",
  processingCompleteToPaste: "ms",
  serverRoundTrip: "ms",
  captureStartDuration: "ms",
  stopToCaptureRelease: "ms",
  recordingFinalization: "ms",
  asrPreparation: "ms",
  cleanupRoundTrip: "ms",
  cleanupProcessing: "ms",
  idleRss: "MiB",
  idleCpu: "percent-of-one-core",
  idleEnergy: "joules",
});
const OUTCOMES = ["success", "failed", "cancelled", "missing", "excluded"];
const validNumber = (value) => typeof value === "number" && Number.isFinite(value) && value >= 0;

function distribution(values) {
  const sorted = [...values].sort((a, b) => a - b);
  if (!sorted.length) return { count: 0, median: null, p90: null, min: null, max: null };
  const middle = Math.floor(sorted.length / 2);
  return {
    count: sorted.length,
    median: sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2,
    p90: sorted[Math.ceil(sorted.length * 0.9) - 1],
    min: sorted[0],
    max: sorted.at(-1),
  };
}

function summarizeObservations(observations) {
  if (!Array.isArray(observations)) throw new Error("Expected an observations array");
  const buckets = Object.fromEntries(Object.keys(METRICS).map((metric) => [metric, []]));
  for (const observation of observations) {
    if (
      !observation ||
      Object.keys(observation).some((key) => !["metric", "outcome", "value"].includes(key)) ||
      !Object.hasOwn(METRICS, observation.metric) ||
      !OUTCOMES.includes(observation.outcome) ||
      (observation.outcome === "success"
        ? !validNumber(observation.value)
        : observation.value !== undefined && observation.value !== null)
    ) {
      // Do not echo untrusted observations that might contain private text.
      throw new Error("Invalid observation; use a known metric, outcome, and numeric value only");
    }
    buckets[observation.metric].push(observation);
  }
  return Object.fromEntries(
    Object.entries(buckets).map(([metric, records]) => [
      metric,
      {
        unit: METRICS[metric],
        ...distribution(records.filter((r) => r.outcome === "success").map((r) => r.value)),
        outcomes: Object.fromEntries(
          OUTCOMES.map((outcome) => [outcome, records.filter((r) => r.outcome === outcome).length])
        ),
      },
    ])
  );
}

// File logging emits pretty-printed JSON. Parse only the fixed timing message.
function timingRecords(log, completion) {
  const records = [];
  const marker = completion
    ? /^\[[^\n]+\] \[INFO\]\[performance\](?:\[[^\]\n]+\])? Dictation completion (\{)/gm
    : /^\[[^\n]+\] \[INFO\]\[audio\](?:\[[^\]\n]+\])? Recording startup (\{)/gm;
  for (const match of log.matchAll(marker)) {
    const start = match.index + match[0].length - 1;
    let depth = 0;
    let quoted = false;
    let escaped = false;
    let complete = false;
    for (let cursor = start; cursor < log.length; cursor += 1) {
      const character = log[cursor];
      if (quoted) {
        if (escaped) escaped = false;
        else if (character === "\\") escaped = true;
        else if (character === '"') quoted = false;
      } else if (character === '"') quoted = true;
      else if (character === "{") depth += 1;
      else if (character === "}" && --depth === 0) {
        try {
          records.push(JSON.parse(log.slice(start, cursor + 1)));
          complete = true;
        } catch {
          throw new Error("Malformed timing JSON");
        }
        break;
      }
    }
    if (!complete) throw new Error("Truncated timing JSON");
  }
  return records;
}

function summarizeTiming(log, { intervals, completion = false }) {
  const requests = new Map();
  let lateEvents = 0;
  for (const record of timingRecords(log, completion)) {
    if (
      typeof record.requestId !== "string" ||
      !Number.isInteger(record.sequence) ||
      record.sequence < 0 ||
      !["pending", "completed", "failed", "cancelled", "incomplete"].includes(record.outcome)
    ) {
      throw new Error("Invalid timing identity or outcome");
    }
    if (record.lateStage !== undefined) {
      lateEvents += 1;
      continue;
    }
    const previous = requests.get(record.requestId);
    if (!previous || record.sequence > previous.sequence) requests.set(record.requestId, record);
  }
  const observations = [];
  for (const record of requests.values()) {
    for (const [metric, [start, end]] of Object.entries(intervals)) {
      let outcome =
        { completed: "success", failed: "failed", cancelled: "cancelled" }[record.outcome] ||
        "missing";
      const stages = record.stages || {};
      const value = stages[end] - stages[start];
      if (outcome === "success") {
        if (!validNumber(stages[start]) || !validNumber(stages[end]) || !validNumber(value)) {
          outcome = "missing";
        } else if (
          !completion &&
          (record.captureSource === "held" || record.captureAttempt !== 1)
        ) {
          // A second attempt or held input is not a matched device-open trial.
          outcome = "excluded";
        }
      }
      observations.push({ metric, outcome, ...(outcome === "success" ? { value } : {}) });
    }
  }
  return { requestCount: requests.size, lateEvents, metrics: summarizeObservations(observations) };
}

function summarizeStartup(log) {
  return summarizeTiming(log, {
    intervals: {
      shortcutToFirstAudio: ["requestAccepted", "firstAudio"],
      shortcutToReadyFeedback: ["requestAccepted", "readyFeedback"],
      clientBeforeAcquisition: ["requestAccepted", "acquisitionRequested"],
      inputAcquisition: ["acquisitionRequested", "acquisitionCompleted"],
      acquisitionToFirstAudio: ["acquisitionCompleted", "firstAudio"],
      firstAudioToFeedback: ["firstAudio", "readyFeedback"],
    },
  });
}

function summarizeCompletion(log) {
  return summarizeTiming(log, {
    completion: true,
    intervals: {
      stopToRequestDispatch: ["stopAccepted", "asrRequestDispatched"],
      serverRoundTrip: ["asrRequestDispatched", "asrResponseCompleted"],
      processingCompleteToPasteDispatch: ["processingComplete", "pasteDispatched"],
      processingCompleteToPaste: ["processingComplete", "pasteSettled"],
    },
  });
}

const NATIVE_STAGES = new Set([
  "requestAccepted",
  "gestureResolved",
  "deviceResolved",
  "acquisitionRequested",
  "captureConfigured",
  "captureStartRequested",
  "captureStartReturned",
  "firstAudio",
  "readyFeedback",
  "stopAccepted",
  "captureReleased",
  "recordingFinalized",
  "asrPreparationStarted",
  "asrRequestDispatched",
  "asrResponseReceived",
  "asrResponseCompleted",
  "cleanupPreparationStarted",
  "cleanupRequestDispatched",
  "cleanupResponseReceived",
  "cleanupCompleted",
  "processingComplete",
  "deliveryStarted",
  "pasteDispatched",
  "pasteSettled",
  "requestFinished",
]);
const NATIVE_OUTCOMES = ["pending", "completed", "failed", "cancelled", "rejected", "incomplete"];
const nativeError = () => new Error("Invalid native timing data");
const nativeID = (value) =>
  typeof value === "string" &&
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
const hasOnly = (value, keys) =>
  value &&
  typeof value === "object" &&
  !Array.isArray(value) &&
  Object.keys(value).every((key) => keys.includes(key));

function summarizeNative(log) {
  if (typeof log !== "string" || !log.endsWith("\n")) throw nativeError();
  let rows;
  try {
    rows = log
      .trimEnd()
      .split("\n")
      .map((line) => JSON.parse(line));
  } catch {
    throw nativeError();
  }
  const header = rows.shift();
  if (
    !hasOnly(header, ["schema", "version"]) ||
    header.schema !== "whisper-native-timing" ||
    header.version !== 1
  )
    throw nativeError();
  const requests = new Map();
  let lateRecords = 0;
  const droppedByCollection = new Map();
  for (const record of rows) {
    if (
      !hasOnly(record, [
        "collectionId",
        "requestId",
        "sequence",
        "origin",
        "outcome",
        "startup",
        "delivery",
        "cleanup",
        "stages",
        "attempts",
        "droppedRecords",
      ]) ||
      !nativeID(record.collectionId) ||
      !nativeID(record.requestId) ||
      !Number.isSafeInteger(record.sequence) ||
      record.sequence < 1 ||
      !["button", "pill", "hold", "handsFree"].includes(record.origin) ||
      !NATIVE_OUTCOMES.includes(record.outcome) ||
      !NATIVE_OUTCOMES.includes(record.startup) ||
      !["none", "pasted", "copied", "recovery"].includes(record.delivery) ||
      !["absent", "pending", "completed", "failed"].includes(record.cleanup) ||
      !hasOnly(record.stages, [...NATIVE_STAGES]) ||
      record.stages.requestAccepted !== 0 ||
      !Object.values(record.stages).every(validNumber) ||
      !Array.isArray(record.attempts) ||
      record.attempts.length > 64 ||
      !Number.isSafeInteger(record.droppedRecords) ||
      record.droppedRecords < 0
    )
      throw nativeError();
    for (const [index, attempt] of record.attempts.entries()) {
      if (
        !hasOnly(attempt, [
          "service",
          "index",
          "dispatchedMs",
          "responseMs",
          "outcome",
          "status",
        ]) ||
        !["asr", "cleanup"].includes(attempt.service) ||
        attempt.index !== index ||
        !["pending", "completed", "failed", "cancelled"].includes(attempt.outcome) ||
        ["dispatchedMs", "responseMs"].some(
          (key) => key in attempt && !validNumber(attempt[key])
        ) ||
        ("status" in attempt &&
          (!Number.isInteger(attempt.status) || attempt.status < 100 || attempt.status > 599)) ||
        (validNumber(attempt.responseMs) &&
          (!validNumber(attempt.dispatchedMs) || attempt.responseMs < attempt.dispatchedMs))
      )
        throw nativeError();
    }
    droppedByCollection.set(
      record.collectionId,
      Math.max(droppedByCollection.get(record.collectionId) || 0, record.droppedRecords)
    );
    const previous = requests.get(record.requestId);
    if (previous && record.sequence > previous.sequence && previous.outcome !== "pending") {
      lateRecords += 1;
      continue;
    }
    if (!previous || record.sequence > previous.sequence) requests.set(record.requestId, record);
  }
  const intervals = {
    shortcutToFirstAudio: ["requestAccepted", "firstAudio"],
    shortcutToReadyFeedback: ["requestAccepted", "readyFeedback"],
    clientBeforeAcquisition: ["requestAccepted", "acquisitionRequested"],
    firstAudioToFeedback: ["firstAudio", "readyFeedback"],
    captureStartDuration: ["captureStartRequested", "captureStartReturned"],
    stopToCaptureRelease: ["stopAccepted", "captureReleased"],
    recordingFinalization: ["captureReleased", "recordingFinalized"],
    stopToRequestDispatch: ["stopAccepted", "asrRequestDispatched"],
    asrPreparation: ["asrPreparationStarted", "asrRequestDispatched"],
    serverRoundTrip: ["asrRequestDispatched", "asrResponseCompleted"],
    cleanupProcessing: ["cleanupPreparationStarted", "cleanupCompleted"],
    processingCompleteToPasteDispatch: ["processingComplete", "pasteDispatched"],
    processingCompleteToPaste: ["processingComplete", "pasteSettled"],
  };
  const observations = [];
  const requestOutcomes = Object.fromEntries(NATIVE_OUTCOMES.map((outcome) => [outcome, 0]));
  const startupOutcomes = Object.fromEntries(NATIVE_OUTCOMES.map((outcome) => [outcome, 0]));
  const serviceAttempts = Object.fromEntries(
    ["asr", "cleanup"].map((service) => [
      service,
      { pending: 0, completed: 0, failed: 0, cancelled: 0 },
    ])
  );
  for (const record of requests.values()) {
    requestOutcomes[record.outcome] += 1;
    startupOutcomes[record.startup] += 1;
    for (const attempt of record.attempts) serviceAttempts[attempt.service][attempt.outcome] += 1;
    const baseOutcome =
      { completed: "success", failed: "failed", cancelled: "cancelled", rejected: "excluded" }[
        record.outcome
      ] || "missing";
    for (const [metric, [start, end]] of Object.entries(intervals)) {
      const startupMetric = [
        "shortcutToFirstAudio",
        "shortcutToReadyFeedback",
        "clientBeforeAcquisition",
        "firstAudioToFeedback",
        "captureStartDuration",
      ].includes(metric);
      let outcome = startupMetric
        ? { completed: "success", failed: "failed", cancelled: "cancelled", rejected: "excluded" }[
            record.startup
          ] || "missing"
        : baseOutcome;
      const value = record.stages[end] - record.stages[start];
      if (outcome === "success") {
        if (["button", "pill"].includes(record.origin) && metric.startsWith("shortcutTo"))
          outcome = "excluded";
        else if (metric === "cleanupProcessing" && record.cleanup === "failed") outcome = "failed";
        else if (metric.startsWith("processingCompleteToPaste") && record.delivery !== "pasted")
          outcome = record.delivery === "recovery" ? "failed" : "missing";
        else if (
          !validNumber(record.stages[start]) ||
          !validNumber(record.stages[end]) ||
          !validNumber(value)
        )
          outcome = "missing";
      }
      observations.push({ metric, outcome, ...(outcome === "success" ? { value } : {}) });
    }
    const cleanup = record.attempts.filter((attempt) => attempt.service === "cleanup");
    let outcome = baseOutcome;
    if (outcome === "success") {
      if (record.cleanup === "failed") outcome = "failed";
      else if (cleanup.length > 1) outcome = "excluded";
      else if (
        cleanup.length !== 1 ||
        cleanup[0].outcome !== "completed" ||
        !validNumber(cleanup[0].responseMs) ||
        !validNumber(cleanup[0].dispatchedMs)
      )
        outcome = "missing";
    }
    observations.push({
      metric: "cleanupRoundTrip",
      outcome,
      ...(outcome === "success" ? { value: cleanup[0].responseMs - cleanup[0].dispatchedMs } : {}),
    });
    // AVCapture start may return after its first frame. These legacy linear intervals are not comparable.
    for (const metric of ["inputAcquisition", "acquisitionToFirstAudio"])
      observations.push({ metric, outcome: "excluded" });
  }
  return {
    requestCount: requests.size,
    requestOutcomes,
    startupOutcomes,
    serviceAttempts,
    lateRecords,
    droppedRecords: [...droppedByCollection.values()].reduce((sum, count) => sum + count, 0),
    metrics: summarizeObservations(observations),
  };
}

function parseCpuTime(value) {
  const match = /^(?:(\d+)-)?(?:(\d+):)?(\d+):(\d+(?:\.\d+)?)$/.exec(value);
  if (!match) throw new Error("Unsupported process CPU time");
  return (
    ((Number(match[1] || 0) * 24 + Number(match[2] || 0)) * 60 + Number(match[3])) * 60 +
    Number(match[4])
  );
}

function processSnapshot(rootPid) {
  const output = execFileSync("/bin/ps", ["-axo", "pid=,ppid=,rss=,time="], { encoding: "utf8" });
  const all = output
    .trim()
    .split("\n")
    .map((line) => {
      const [pid, parent, rss, time] = line.trim().split(/\s+/);
      return {
        pid: Number(pid),
        parent: Number(parent),
        rss: Number(rss),
        cpu: parseCpuTime(time),
      };
    });
  const selected = new Set([rootPid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const process of all) {
      if (selected.has(process.parent) && !selected.has(process.pid)) {
        selected.add(process.pid);
        changed = true;
      }
    }
  }
  if (!all.some((process) => process.pid === rootPid)) throw new Error("Root process exited");
  return all.filter((process) => selected.has(process.pid));
}

function resourceObservation(previous, current, elapsedSeconds) {
  const before = new Map(previous.map((process) => [process.pid, process.cpu]));
  const stable =
    previous.length === current.length && current.every((process) => before.has(process.pid));
  const cpuSeconds = current.reduce(
    (sum, process) => sum + process.cpu - (before.get(process.pid) ?? process.cpu),
    0
  );
  return [
    {
      metric: "idleRss",
      outcome: "success",
      value: current.reduce((sum, process) => sum + process.rss, 0) / 1024,
    },
    stable && cpuSeconds >= 0 && elapsedSeconds > 0
      ? { metric: "idleCpu", outcome: "success", value: (100 * cpuSeconds) / elapsedSeconds }
      : { metric: "idleCpu", outcome: "missing" },
  ];
}

module.exports = {
  distribution,
  summarizeObservations,
  summarizeStartup,
  summarizeCompletion,
  summarizeNative,
  processSnapshot,
  resourceObservation,
};
