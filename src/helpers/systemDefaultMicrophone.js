const { execFile } = require("child_process");
const { resolveBundledBinary } = require("./binaryResolver");
const debugLogger = require("./debugLogger");

const COMMAND_TIMEOUT_MS = 3000;
// A failed lookup is retried after this long, so a transient error (a helper
// still extracting) does not stick for the session.
const FAILURE_RETRY_MS = 30000;

function runFile(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    execFile(
      command,
      args,
      {
        encoding: "utf8",
        timeout: COMMAND_TIMEOUT_MS,
        maxBuffer: 256 * 1024,
        ...options,
      },
      (error, stdout, stderr) => {
        if (error) {
          error.stderr = stderr;
          reject(error);
          return;
        }
        resolve(stdout.trim());
      }
    );
  });
}

function parseJsonResult(output) {
  const line = String(output || "")
    .split(/\r?\n/)
    .map((candidate) => candidate.trim())
    .filter(Boolean)
    .at(-1);
  if (!line) return null;
  const result = JSON.parse(line);
  if (!result || typeof result.name !== "string" || !result.name.trim()) return null;
  return {
    name: result.name.trim(),
    nativeId: typeof result.id === "string" && result.id ? result.id : undefined,
  };
}

function createSystemDefaultMicrophoneResolver({
  run = runFile,
  resolveBinary = resolveBundledBinary,
  now = Date.now,
} = {}) {
  let cache = null;
  let cachedAt = 0;
  let inflight = null;

  const resolveDarwin = async () => {
    const helper = resolveBinary("macos-mic-listener", "audio");
    if (!helper) return null;
    return parseJsonResult(await run(helper, ["--print-default-input"]));
  };

  const lookup = async () => {
    try {
      const result = await resolveDarwin();
      cache = result ? { ...result, source: "system" } : { name: "", source: "unavailable" };
    } catch (error) {
      debugLogger.debug(
        "Failed to resolve the system default microphone",
        { error: error.message },
        "audio"
      );
      cache = { name: "", source: "unavailable" };
    }
    cachedAt = now();
    return cache;
  };

  // A successful lookup stays valid until a renderer sees a devicechange and
  // asks for a refresh, keeping helper spawns off the hotkey path. Every caller
  // joins an in-flight lookup so all windows share one process and see the same
  // answer.
  return async ({ refresh = false } = {}) => {
    if (inflight) return inflight;
    if (!refresh && cache && (cache.source === "system" || now() - cachedAt < FAILURE_RETRY_MS)) {
      return cache;
    }
    inflight = lookup().finally(() => {
      inflight = null;
    });
    return inflight;
  };
}

module.exports = {
  createSystemDefaultMicrophoneResolver,
  parseJsonResult,
  resolveSystemDefaultMicrophone: createSystemDefaultMicrophoneResolver(),
};
