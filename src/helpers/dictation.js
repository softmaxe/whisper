// Mocked by import-path suffix in the renderer tests, so keep these
// specifiers ending in "/helpers/audioManager", "/stores/settingsStore" and
// "/utils/logger".
import AudioManager from "../helpers/audioManager";
import { RecordingStartupTrace } from "../helpers/recordingStartupTrace";
import { recordCleanupFailure } from "../stores/cleanupFailureStore";
import { getSettings } from "../stores/settingsStore";
import { playStartCue, playStopCue } from "../utils/dictationCues";
import { canStartDictation } from "../utils/dictationReadiness";
import logger from "../utils/logger";
import { isAccessibilitySkipped } from "../utils/permissions";
import { expandSnippets } from "../utils/snippets";

/**
 * The renderer's single owner of Dictation state (ADR-0003).
 *
 * It owns the idle → preparing → recording → processing → idle state machine,
 * the start and stop locks, the preparation generation that makes stale runs
 * inert, and publishing each transition to main. Main sees every transition,
 * including failed starts, exactly once per change.
 *
 * Presentation stays with the caller: the module describes what happened
 * through `onNotice` and the caller decides how to show it.
 *
 * @param {object} options
 * @param {(notice: object) => void} [options.onNotice] receives presentation
 *   notices: `dismissError`, `error`, `noAudio`, `pushForceStopped`,
 *   `micUnavailable`, `micRestored`, `partialTranscription`, `deliveryFallback`.
 * @param {() => void} [options.onCommand] runs after each toggle, start, or stop
 *   command from main.
 */
export function createDictation({ onNotice, onCommand } = {}) {
  const bridge = () => window.electronAPI;
  const notify = (notice) => onNotice?.(notice);

  const manager = new AudioManager();
  let disposed = false;

  // State machine and the transition publisher. `state` and `reportedId` are
  // what main has been told; repeated transitions are not re-sent. Every
  // report names its Dictation, so main can discard stale ones. Main assigns
  // the id when it starts a Dictation; a renderer-started one (pill, Retry)
  // uses its startup trace's id. The mount report alone carries no id: it
  // belongs to no Dictation and resets main's record.
  let state = null;
  let dictationId = null;
  let reportedId = null;
  const transition = (next) => {
    if (next !== "idle") dictationId ??= crypto.randomUUID();
    if (state === next && (next === "idle" || reportedId === dictationId)) return;
    const previous = state;
    state = next;
    reportedId = dictationId;
    bridge()?.dictationLifecycleStateChanged?.(next, dictationId);
    if (next === "idle") dictationId = null;
    // An id-only re-report (main's id adopted) leaves the state, and so
    // the held recording effects, unchanged.
    if (previous === "recording" && next !== "recording") releaseRecordingEffects();
  };

  // Main's id wins: a command naming another id while a Dictation is in
  // progress (for example a hotkey press racing a pill click) renames it, so
  // the reports that follow match main's record.
  const adoptDictationId = (startupRequest) => {
    const id = startupRequest?.requestId;
    if (!id || state === null || state === "idle" || id === dictationId) return;
    dictationId = id;
    transition(state);
  };

  // Locks, generation, and per-Dictation latches.
  let startLock = false;
  let stopLock = false;
  let generation = 0;
  let pushForceStopped = false;
  let wasMicUnavailable = false;
  let startupTrace = null;
  let feedbackTrace = null;

  // A Dictation's side effects belong to this module, never to the UI call
  // sites that cause them. Media and the cancel hotkey are held from a
  // completed start until recording ends, so every exit (stop, cancel,
  // failure, teardown) releases them.
  let recordingEffectsHeld = false;
  const holdRecordingEffects = (trace) => {
    // A quick tap can end the recording inside the start call itself; a
    // recording that already ended must not pause media or cue. See #1060.
    if (state !== "recording" || !manager.getState().isRecording) return;
    recordingEffectsHeld = true;
    if (getSettings().pauseMediaOnDictation) bridge()?.pauseMediaPlayback?.();
    bridge()?.registerCancelHotkey?.("Escape");
    if (getSettings().audioCuesEnabled) trace.mark("readyCueRequested");
    void playStartCue(() => trace.mark("readyCueScheduled"));
  };

  // Runs as the Dictation leaves recording.
  const releaseRecordingEffects = () => {
    if (!recordingEffectsHeld) return;
    recordingEffectsHeld = false;
    bridge()?.unregisterCancelHotkey?.();
    // Resume media the instant recording ends, not after transcription.
    if (getSettings().pauseMediaOnDictation) bridge()?.resumeMediaPlayback?.();
  };

  // The dictation preview closes as soon as an outcome settles it, without
  // waiting for History. Outcomes that leave the text for manual recovery
  // never ask for this.
  const hidePreview = () => bridge()?.hideDictationPreview?.();

  // Snapshot for UI bindings.
  let snapshot = {
    isRecording: false,
    isProcessing: false,
    isPreparing: false,
    isStopping: false,
    micCaptureStatus: "inactive",
    transcript: "",
  };
  const listeners = new Set();
  const update = (patch) => {
    let changed = false;
    for (const key of Object.keys(patch)) {
      if (snapshot[key] !== patch[key]) changed = true;
    }
    if (!changed) return;
    snapshot = { ...snapshot, ...patch };
    for (const listener of listeners) listener(snapshot);
  };

  const runIsCurrent = () => {
    const run = generation;
    return () => !disposed && run === generation;
  };

  const invalidatePreparation = () => {
    generation += 1;
    startLock = false;
    update({ isPreparing: false, isStopping: false });
  };

  const getStartupTrace = (request) => {
    let trace = startupTrace;
    if (!trace || (request ? trace.requestId !== request.requestId : trace.outcome !== "pending")) {
      trace?.finish("incomplete", "superseded");
      trace = new RecordingStartupTrace(request);
      startupTrace = trace;
    }
    return trace;
  };

  const start = async ({ startupRequest } = {}) => {
    if (startLock) return false;
    if (disposed || !canStartDictation(manager.getState())) return false;
    startLock = true;
    pushForceStopped = false;
    let recordingStarted = false;
    generation += 1;
    const isCurrent = runIsCurrent();
    let trace;
    try {
      trace = getStartupTrace(startupRequest);
      dictationId = trace.requestId;
      trace.mark("preparationEntered");
      manager.selectMicrophoneForSession?.();

      update({ isStopping: false, isPreparing: true });
      // Publish feedback while acquisition and target capture are pending.
      transition("preparing");
      // A retry owns the presentation as soon as it starts connecting. Keep
      // the old error from covering a slow microphone's preparation feedback.
      notify({ type: "dismissError" });
      // Acquire alongside preparation feedback, even when the window cannot
      // draw. startRecording() joins this capture and retains its pre-roll.
      void manager.prepareMicCapture?.(trace);

      // The floating dictation panel is non-focusable, so the foreground app is
      // still the user's actual editing target here. Refresh it for recordings
      // started from the panel itself as well as from global hotkeys; otherwise
      // paste can reactivate a stale target from the preceding dictation.
      // Main binds the target to this Dictation; paste requests name it.
      try {
        await bridge().captureDictationTarget?.(dictationId);
      } catch (error) {
        logger.warn("Failed to refresh dictation target", { error: error?.message });
      }

      if (!isCurrent()) return false;
      manager.resetRecordingRequest();
      const didStart = await manager.startRecording(trace);
      if (!isCurrent()) return false;
      recordingStarted = didStart;
      if (didStart) {
        trace.startSettled();
        holdRecordingEffects(trace);
      } else {
        trace.finish("failed", "start_failed");
      }
      return didStart;
    } catch (error) {
      trace?.finish("failed", "start_exception");
      throw error;
    } finally {
      if (!recordingStarted) trace?.finish("incomplete", "start_abandoned");
      if (isCurrent()) {
        startLock = false;
        if (!recordingStarted) {
          manager.cancelPreparedMicCapture?.();
          update({ isPreparing: false });
          // Covers every exit above that never started a recording — the
          // policy-block early return, the mic-open failure, a stale
          // preparation generation, etc. startRecording's failure path only
          // fires onError, never the onStateChange that normally ends the
          // Dictation, so without this main would stay in "preparing".
          if (state === "preparing") transition("idle");
        }
      }
    }
  };

  const stop = async () => {
    startupTrace?.finish("incomplete", "stopped_before_observation");
    if (!disposed && !manager.getState().isRecording && (startLock || state === "preparing")) {
      invalidatePreparation();
      manager.cancelRecording();
      transition("idle");
      return true;
    }
    if (stopLock) return false;
    stopLock = true;
    try {
      if (disposed) return false;
      if (!manager.getState().isRecording) return false;
      update({ isPreparing: false, isStopping: true });
      const didStop = manager.stopRecording();
      // The stop cue answers the user's stop; cancel, failure, and stops the
      // recorder makes on its own end the recording silently.
      if (didStop) void playStopCue();
      return didStop;
    } finally {
      stopLock = false;
      update({ isStopping: false });
    }
  };

  // Hotkey toggle from main. A start still awaiting the mic open leaves
  // isRecording false, so without the lock check this toggle-off would take
  // the start branch and be lost. A prepare alone is only a hint: main sends
  // it before the first toggle, which must adopt that capture, not stop it.
  const toggle = async ({ startupRequest } = {}) => {
    if (disposed) return;
    const managerState = manager.getState();
    if (startLock || managerState.isRecording) {
      await stop();
    } else if (canStartDictation(managerState)) {
      await start({ startupRequest });
    }
  };

  // Toggle from the Recording pill, which treats any visible preparation as
  // something to stop.
  const toggleListening = async () => {
    if (startLock || snapshot.isPreparing) {
      await stop();
    } else if (!snapshot.isRecording && !snapshot.isProcessing) {
      await start();
    } else if (snapshot.isRecording) {
      await stop();
    }
  };

  const prepare = ({ startupRequest } = {}) => {
    if (disposed || startLock) return;
    if (!canStartDictation(manager.getState())) return;
    const trace = getStartupTrace(startupRequest);
    dictationId = trace.requestId;
    trace.mark("preparationEntered");
    manager.selectMicrophoneForSession?.();
    generation += 1;
    update({ isPreparing: true });
    transition("preparing");
    notify({ type: "dismissError" });
    void manager.prepareMicCapture?.(trace);
  };

  const cancelPreparation = () => {
    startupTrace?.finish("cancelled", "preparation_cancelled");
    invalidatePreparation();
    if (!disposed) manager.cancelRecording();
    if (state === "preparing") transition("idle");
  };

  const cancelRecording = async () => {
    startupTrace?.finish("cancelled", "recording_cancelled");
    if (disposed) return false;
    invalidatePreparation();
    manager.cancelPreparedMicCapture?.();
    // Cancel capture before reporting idle: ending the capture session can
    // still publish a recording status, which must not re-enter recording.
    const cancelled = manager.cancelRecording();
    transition("idle");
    return cancelled;
  };

  const cancelProcessing = () => {
    if (disposed) return false;
    invalidatePreparation();
    hidePreview();
    return manager.cancelProcessing();
  };

  const noteForceStopped = (payload) => {
    // Listed rather than negated: a future renderer-initiated stop would not
    // leave the keys down, and must not be swept in here.
    if (payload?.reason === "timeout" || payload?.reason === "reset") {
      pushForceStopped = true;
    }
  };

  const deliver = async (result) => {
    const isCurrent = runIsCurrent();
    notify({ type: "dismissError" });
    const transcribedText = result.text?.trim();

    if (!transcribedText) {
      hidePreview();
      notify({ type: "noAudio" });
      return;
    }

    result.text = expandSnippets(result.text, getSettings().snippets);

    update({ transcript: result.text });
    bridge()?.completeDictationPreview?.({ text: result.text });

    if (result.warning) {
      notify({ type: "partialTranscription" });
    }

    const { autoPasteEnabled, keepTranscriptionInClipboard } = getSettings();

    const persistencePromise = manager
      .saveTranscription(result.text, result.rawText ?? result.text, {
        clientTranscriptionId: result.clientTranscriptionId,
        // Spread rather than set: a result with no analytics timestamp must
        // not gain the key as undefined, matching how audioManager carries
        // this field and keeping the options object exactly what callers
        // without Insights data expect.
        ...(result.analyticsOccurredAt ? { analyticsOccurredAt: result.analyticsOccurredAt } : {}),
      })
      .then(
        (persisted) => {
          if (!persisted) {
            logger.error(
              "Failed to persist transcription",
              { clientTranscriptionId: result.clientTranscriptionId, source: result.source },
              "audio"
            );
          }
          return persisted;
        },
        (error) => {
          logger.error(
            "Failed to persist transcription",
            {
              clientTranscriptionId: result.clientTranscriptionId,
              error: error?.message,
              source: result.source,
            },
            "audio"
          );
          return false;
        }
      );

    const keepInClipboard = async (delivery) => {
      if (!isCurrent()) return false;
      try {
        const clipboardResult = await bridge().writeClipboard(result.text);
        if (clipboardResult?.success === false) {
          throw new Error("clipboard-write-failed");
        }
        return true;
      } catch (error) {
        logger.warn(
          "Failed to keep transcription in clipboard",
          { delivery, error: error?.message },
          "clipboard"
        );
        return false;
      }
    };

    if (pushForceStopped && autoPasteEnabled) {
      // The push hit its safety ceiling while the trigger keys were still
      // down. Injecting the paste shortcut into those held modifiers is what
      // silently loses the transcript, so keep it instead.
      const keptInClipboard = await keepInClipboard("push-force-stopped");
      if (!isCurrent()) return;
      hidePreview();
      notify({
        type: "pushForceStopped",
        keptInClipboard,
        transcript: result.rawText ?? result.text,
      });
    } else if (autoPasteEnabled) {
      const pasteStart = performance.now();
      let pasteSucceeded = true;
      try {
        pasteSucceeded = await manager.safePaste(result.text, {
          dictationId,
          restoreClipboard: !keepTranscriptionInClipboard,
          allowClipboardFallback: isAccessibilitySkipped(),
          suppressError: true,
        });
      } catch (error) {
        pasteSucceeded = false;
        logger.warn("Failed to paste transcription", { error: error?.message }, "clipboard");
      }
      if (!isCurrent()) return;
      if (!pasteSucceeded && localStorage.getItem("onboardingCompleted") === "true") {
        const copied = await keepInClipboard("paste-fallback");
        if (isCurrent()) {
          notify({
            type: "deliveryFallback",
            text: result.text,
            copyFallback: copied ? "copied" : "copy",
          });
        }
      }
      logger.info(
        "Paste timing",
        {
          pasteMs: Math.round(performance.now() - pasteStart),
          source: result.source,
          textLength: result.text.length,
          success: pasteSucceeded,
        },
        "clipboard"
      );
      // Successful delivery closes the preview; failed delivery keeps the
      // final text available in the manual-copy panel.
      if (pasteSucceeded && isCurrent()) {
        hidePreview();
        if (result.cleanupFailure) recordCleanupFailure(result.cleanupFailure);
      }
    } else if (keepTranscriptionInClipboard) {
      await keepInClipboard("clipboard-only");
    }

    await persistencePromise;
  };

  // Reset stale main-process state after a renderer reload or crash recovery.
  transition("idle");

  manager.setCallbacks({
    onStateChange: ({ isRecording, isProcessing, micCaptureStatus, startupTrace: trace }) => {
      if (isRecording && trace) {
        feedbackTrace = trace;
        trace.mark("recordingStarted");
      }
      transition(isRecording ? "recording" : isProcessing ? "processing" : "idle");
      const patch = { isRecording, isProcessing };
      if (isRecording) patch.isPreparing = false;
      if (!isRecording) patch.isStopping = false;
      if (micCaptureStatus) patch.micCaptureStatus = micCaptureStatus;
      update(patch);
      if (micCaptureStatus) {
        const unavailable = micCaptureStatus === "unavailable";
        if (unavailable && !wasMicUnavailable) {
          wasMicUnavailable = true;
          notify({ type: "micUnavailable" });
        } else if (micCaptureStatus === "active" && wasMicUnavailable) {
          wasMicUnavailable = false;
          notify({ type: "micRestored" });
        } else if (micCaptureStatus === "inactive") {
          wasMicUnavailable = false;
        }
      }
    },
    onError: (error) => {
      if (error?.code === "MIC_CAPTURE_FAILED") {
        startupTrace?.finish("failed", "microphone_unavailable");
        invalidatePreparation();
        transition("idle");
      }
      update({ isPreparing: false, isStopping: false });
      if (error?.code === "TRANSCRIPTION_CANCELLED" || error?.code === "REASON_CANCELLED") return;
      if (error?.title !== "Paste Error") hidePreview();
      notify({ type: "error", error });
    },
    onNoAudio: () => {
      update({ isPreparing: false, isStopping: false });
      hidePreview();
      notify({ type: "noAudio" });
    },
    onTranscriptionComplete: async (result) => {
      if (result.success) await deliver(result);
    },
  });

  const api = bridge();
  const disposers = [
    api.onToggleDictation((options) => {
      adoptDictationId(options?.startupRequest);
      toggle(options);
      onCommand?.();
    }),
    api.onStartDictation?.((options) => {
      adoptDictationId(options?.startupRequest);
      start(options);
      onCommand?.();
    }),
    api.onPrepareDictation?.((options) => {
      adoptDictationId(options?.startupRequest);
      prepare(options);
    }),
    api.onCancelDictationPreparation?.(() => cancelPreparation()),
    api.onStopDictation?.(() => {
      stop();
      onCommand?.();
    }),
    api.onDictationForceStopped?.((payload) => noteForceStopped(payload)),
  ];

  const dispose = () => {
    generation += 1;
    startLock = false;
    startupTrace?.finish("incomplete", "renderer_teardown");
    transition("idle");
    disposed = true;
    for (const disposeListener of disposers) disposeListener?.();
    listeners.clear();
    manager.cleanup();
  };

  return {
    getSnapshot: () => snapshot,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    start,
    stop,
    toggleListening,
    cancelRecording,
    cancelProcessing,
    getAudioLevel: () => manager.getRecordingAudioLevel() ?? null,
    markReadyFeedback: () => feedbackTrace?.mark("readyFeedback"),
    dispose,
  };
}
