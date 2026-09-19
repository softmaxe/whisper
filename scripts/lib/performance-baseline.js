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
  processSnapshot,
  resourceObservation,
};
