// Read one frame from the recorder's own track. Unlike an analyser/worklet,
// this observes source delivery rather than a render quantum of zero-fill.
// Chromium's processor is a track sink; cancelling its reader detaches that
// sink without stopping the track or consuming MediaRecorder's input.
export function observeFirstAudio(track) {
  let reader;
  let cancelled = false;
  const cancel = () => {
    cancelled = true;
    void reader?.cancel().catch(() => {});
  };
  const firstAudio = (async () => {
    try {
      const processor = new MediaStreamTrackProcessor({ track, maxBufferSize: 1 });
      reader = processor.readable.getReader();
      while (!cancelled) {
        const { value, done } = await reader.read();
        if (done) return null;
        try {
          // Silence has frames too. Never copy, inspect, log or retain samples.
          if (!cancelled && value.numberOfFrames > 0) {
            return performance.timeOrigin + performance.now();
          }
        } finally {
          value.close();
        }
      }
    } catch {
      // Unsupported processors and ended inputs leave first audio unobserved.
    } finally {
      await reader?.cancel().catch(() => {});
      reader?.releaseLock();
    }
    return null;
  })();
  return { firstAudio, cancel };
}

// Buffering is already active while this waits. Source delivery, including
// silence, establishes readiness; track state alone never does.
export function waitForFirstAudio(observation, track, signal) {
  return new Promise((resolve, reject) => {
    const unavailable = () => {
      const error = new Error("The selected microphone did not deliver audio");
      error.name = "FirstAudioUnavailableError";
      finish(error);
    };
    const abort = () => finish(new DOMException("Capture request ended", "AbortError"));
    const timer = setTimeout(unavailable, 10000);
    const finish = (error) => {
      clearTimeout(timer);
      signal.removeEventListener("abort", abort);
      track?.removeEventListener?.("ended", unavailable);
      if (error) reject(error);
      else resolve();
    };
    signal.addEventListener("abort", abort, { once: true });
    track?.addEventListener?.("ended", unavailable, { once: true });
    if (signal.aborted) return abort();
    if (!observation || track?.readyState !== "live") return unavailable();
    void observation.firstAudio.then((timestamp) => {
      if (signal.aborted) abort();
      else if (timestamp === null || track.readyState !== "live" || track.muted) unavailable();
      else finish();
    });
  });
}
