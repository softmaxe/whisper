import logger from "../utils/logger";
import { observeFirstAudio } from "./firstAudio";

// performance.now() alone has a different origin in each Electron process.
const startupTimestamp = () => performance.timeOrigin + performance.now();
const CAPTURE_STAGES = [
  "deviceResolved",
  "acquisitionRequested",
  "acquisitionCompleted",
  "trackReady",
  "firstAudio",
  "firstAudioUnavailable",
];
const REQUIRED_STAGES = [
  "preparationEntered",
  "deviceResolved",
  "acquisitionRequested",
  "acquisitionCompleted",
  "trackReady",
  "firstAudio",
  "readyFeedback",
];
const OBSERVATION_TIMEOUT_MS = 30000;

export class RecordingStartupTrace {
  constructor(request) {
    this.requestId = request?.requestId ?? crypto.randomUUID();
    this.acceptedAt = request?.acceptedAt ?? startupTimestamp();
    this.stages = { requestAccepted: 0 };
    this.outcome = "pending";
    this.started = false;
    this.sequence = 0;
    this.captureAttempt = 0;
    this.captureSource = null;
    this.stopObservingCapture = null;
    this.timer = setTimeout(
      () => this.finish("incomplete", "observation_timeout"),
      OBSERVATION_TIMEOUT_MS
    );
    this.mark("rendererReceived");
  }

  mark(stage, timestamp = startupTimestamp()) {
    const elapsedMs = Math.round((timestamp - this.acceptedAt) * 100) / 100;
    if (this.outcome !== "pending") {
      this.emit({ lateStage: stage, elapsedMs });
      return;
    }
    if (this.stages[stage] !== undefined) return;
    this.stages[stage] = elapsedMs;
    this.emit();
    this.completeIfObserved();
  }

  beginCapture(source) {
    if (this.outcome !== "pending") {
      this.mark("acquisitionRequested");
      return this.captureAttempt;
    }
    this.stopObservingCapture?.();
    this.stopObservingCapture = null;
    this.captureAttempt += 1;
    this.captureSource = source;
    // An expired prepared capture can be replaced within one request. Its
    // first frame cannot establish readiness of the replacement input.
    for (const stage of CAPTURE_STAGES) delete this.stages[stage];
    this.mark("deviceResolved");
    this.mark("acquisitionRequested");
    return this.captureAttempt;
  }

  markCapture(attempt, stage, timestamp = startupTimestamp()) {
    if (attempt !== this.captureAttempt) {
      this.emit({
        lateStage: stage,
        lateCaptureAttempt: attempt,
        elapsedMs: Math.round((timestamp - this.acceptedAt) * 100) / 100,
      });
      return;
    }
    this.mark(stage, timestamp);
  }

  observeCapture(stream, attempt) {
    if (this.outcome !== "pending" || attempt !== this.captureAttempt) return;
    const track = stream.getAudioTracks()[0];
    if (!track) return;
    const checkTrack = () => {
      if (track.readyState === "live" && !track.muted) this.markCapture(attempt, "trackReady");
    };
    checkTrack();
    track.addEventListener?.("unmute", checkTrack);
    const observation = observeFirstAudio(track);
    this.stopObservingCapture = () => {
      track.removeEventListener?.("unmute", checkTrack);
      observation.cancel();
    };
    void observation.firstAudio.then((timestamp) => {
      if (timestamp !== null) this.markCapture(attempt, "firstAudio", timestamp);
      else if (this.outcome === "pending" && attempt === this.captureAttempt) {
        this.markCapture(attempt, "firstAudioUnavailable");
      }
    });
  }

  startSettled() {
    this.started = true;
    this.completeIfObserved();
  }

  completeIfObserved() {
    if (this.started && REQUIRED_STAGES.every((stage) => this.stages[stage] !== undefined)) {
      this.finish("completed", "startup_observed");
    }
  }

  finish(outcome, reason) {
    if (this.outcome !== "pending") return;
    this.outcome = outcome;
    clearTimeout(this.timer);
    this.emit({ reason });
    this.stopObservingCapture?.();
    this.stopObservingCapture = null;
  }

  emit(extra = {}) {
    // Only identities, numeric timings and fixed lifecycle vocabulary belong
    // here. Never pass settings, device objects or exception messages.
    void logger.info(
      "Recording startup",
      {
        requestId: this.requestId,
        sequence: ++this.sequence,
        acceptedAt: this.acceptedAt,
        captureAttempt: this.captureAttempt,
        captureSource: this.captureSource,
        stages: { ...this.stages },
        outcome: this.outcome,
        totalMs:
          this.outcome === "completed"
            ? Math.max(this.stages.firstAudio, this.stages.readyFeedback)
            : null,
        ...extra,
      },
      "audio"
    );
  }
}
