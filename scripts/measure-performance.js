#!/usr/bin/env node
const fs = require("node:fs");
const { setTimeout: delay } = require("node:timers/promises");
const {
  summarizeObservations,
  summarizeStartup,
  processSnapshot,
  resourceObservation,
} = require("./lib/performance-baseline");

async function main(args) {
  const [command, input, countArgument = "30", intervalArgument = "1000"] = args;
  if (command === "startup" && args.length === 2) {
    return summarizeStartup(fs.readFileSync(input, "utf8"));
  }
  if (command === "summarize" && args.length === 2) {
    return { metrics: summarizeObservations(JSON.parse(fs.readFileSync(input, "utf8"))) };
  }
  if (command === "sample-process" && args.length >= 2 && args.length <= 4) {
    if (process.platform !== "darwin") throw new Error("Process sampling requires macOS");
    const pid = Number(input);
    const count = Number(countArgument);
    const interval = Number(intervalArgument);
    if (
      !Number.isSafeInteger(pid) ||
      pid <= 0 ||
      !Number.isInteger(count) ||
      count < 1 ||
      count > 3600 ||
      !Number.isInteger(interval) ||
      interval < 100 ||
      interval > 60000
    ) {
      throw new Error("Use a positive PID, 1–3600 samples, and an interval of 100–60000 ms");
    }
    let previous = processSnapshot(pid);
    let timestamp = performance.now();
    const observations = [];
    const startedAt = new Date().toISOString();
    for (let index = 0; index < count; index += 1) {
      await delay(interval);
      const current = processSnapshot(pid);
      const now = performance.now();
      observations.push(...resourceObservation(previous, current, (now - timestamp) / 1000));
      timestamp = now;
      previous = current;
    }
    return {
      startedAt,
      endedAt: new Date().toISOString(),
      intervalMs: interval,
      observations,
      metrics: summarizeObservations(observations),
    };
  }
  throw new Error(
    "Usage: node scripts/measure-performance.js startup LOG | summarize JSON | sample-process PID [COUNT=30] [INTERVAL_MS=1000]"
  );
}

if (require.main === module) {
  main(process.argv.slice(2)).then(
    (result) => console.log(JSON.stringify(result, null, 2)),
    () => {
      // File paths, source JSON, and log contents may contain private information.
      console.error(
        "Measurement failed. Check command syntax, input format, and process availability. See docs/native-performance-baseline.md."
      );
      process.exitCode = 1;
    }
  );
}

module.exports = { main };
