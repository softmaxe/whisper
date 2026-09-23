import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import AudioManager from "../helpers/audioManager";
import { RecordingStartupTrace } from "../helpers/recordingStartupTrace";
import { needsSttConfigBeforeStart } from "../helpers/sttConfigPolicy";
import { isManagedTranscriptionActive } from "../services/managedTranscription";
import { recordCleanupFailure } from "../stores/cleanupFailureStore";
import { isTranscriptionContextAllowed } from "../stores/policyRules";
import { usePolicyStore } from "../stores/policyStore";
import { getSettings } from "../stores/settingsStore";
import { playStartCue, playStopCue } from "../utils/dictationCues";
import { canStartDictation } from "../utils/dictationReadiness";
import logger from "../utils/logger";
import { isAccessibilitySkipped } from "../utils/permissions";
import { getRecordingErrorDescription, getRecordingErrorTitle } from "../utils/recordingErrors";
import { expandSnippets } from "../utils/snippets";
import {
  buildLiveTranscriptionPreview,
  shouldShowByokStreamingPreview,
} from "../utils/transcriptionPreview";

export const useAudioRecording = (toast, options = {}) => {
  const { t } = useTranslation();
  const [isRecording, setIsRecording] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [isStreaming, setIsStreaming] = useState(false);
  const [isPreparing, setIsPreparing] = useState(false);
  const [isStopping, setIsStopping] = useState(false);
  const [micCaptureStatus, setMicCaptureStatus] = useState("inactive");
  const [transcript, setTranscript] = useState("");
  const [partialTranscript, setPartialTranscript] = useState("");
  const audioManagerRef = useRef(null);
  const startupTraceRef = useRef(null);
  const feedbackTraceRef = useRef(null);
  const startLockRef = useRef(false);
  const pushForceStoppedRef = useRef(false);
  const stopLockRef = useRef(false);
  const preparationGenerationRef = useRef(0);
  const wasRecordingRef = useRef(false);
  const wasMicUnavailableRef = useRef(false);
  const reportedLifecycleRef = useRef(null);
  const { onToggle, dismissDictationError, onDictationError, onShowTranscript } = options;

  // Read through a ref so a re-render never tears down the AudioManager
  // (the mount effect below must not depend on this callback).
  const onShowTranscriptRef = useRef(onShowTranscript);
  useEffect(() => {
    onShowTranscriptRef.current = onShowTranscript;
  });

  // Reads only refs and the global electronAPI bridge, so a single stable
  // instance is safe to share between the mount effect (recording/processing
  // transitions) and performStartRecording (the toggle path's own "preparing"
  // report).
  const reportLifecycle = useCallback((state) => {
    if (reportedLifecycleRef.current === state) return;
    reportedLifecycleRef.current = state;
    window.electronAPI?.dictationLifecycleStateChanged?.(state);
  }, []);

  const invalidatePreparation = useCallback(() => {
    preparationGenerationRef.current += 1;
    startLockRef.current = false;
    setIsPreparing(false);
    setIsStopping(false);
  }, []);

  const getStartupTrace = useCallback((request) => {
    let trace = startupTraceRef.current;
    if (!trace || (request ? trace.requestId !== request.requestId : trace.outcome !== "pending")) {
      trace?.finish("incomplete", "superseded");
      trace = new RecordingStartupTrace(request);
      startupTraceRef.current = trace;
    }
    return trace;
  }, []);

  const performStartRecording = useCallback(
    async ({ startupRequest } = {}) => {
      if (startLockRef.current) return false;
      startLockRef.current = true;
      pushForceStoppedRef.current = false;
      let recordingStarted = false;
      const preparationGeneration = ++preparationGenerationRef.current;
      const manager = audioManagerRef.current;
      const isCurrent = () =>
        preparationGeneration === preparationGenerationRef.current &&
        manager === audioManagerRef.current;
      let startupTrace;
      try {
        if (!audioManagerRef.current) return false;
        const policyState = usePolicyStore.getState();
        if (
          !isManagedTranscriptionActive() &&
          !isTranscriptionContextAllowed(policyState, getSettings(), "dictation")
        ) {
          toast({ title: t("common.managedByOrg"), variant: "default" });
          return false;
        }

        if (!canStartDictation(audioManagerRef.current.getState())) return false;

        startupTrace = getStartupTrace(startupRequest);
        startupTrace.mark("preparationEntered");
        manager.selectMicrophoneForSession?.();

        setIsStopping(false);
        setIsPreparing(true);
        // Publish feedback while acquisition and target capture are pending.
        reportLifecycle("preparing");
        // Acquire alongside preparation feedback, even when the window cannot
        // draw. startRecording() joins this capture and retains its pre-roll.
        void audioManagerRef.current.prepareMicCapture?.(startupTrace);

        // The floating dictation panel is non-focusable, so the foreground app is
        // still the user's actual editing target here. Refresh it for recordings
        // started from the panel itself as well as from global hotkeys; otherwise
        // paste can reactivate a stale target from the preceding dictation.
        try {
          await window.electronAPI.captureDictationTarget?.();
        } catch (error) {
          logger.warn("Failed to refresh dictation target", { error: error?.message });
        }

        if (!isCurrent()) return false;

        // Retry STT config fetch if it wasn't loaded on mount (e.g. auth wasn't ready).
        // Await it only when it can change the start decision (signed-in
        // OpenWhispr-cloud streaming); for local STT or a signed-out session the
        // fetch stalls on auth resolution and would delay the mic open (#1673).
        if (needsSttConfigBeforeStart(getSettings()) && !audioManagerRef.current.sttConfig) {
          const configFetch = (async () => {
            const config = await window.electronAPI.getSttConfig?.();
            if (isCurrent() && config?.success) {
              audioManagerRef.current.setSttConfig(config);
            }
          })().catch((error) => {
            logger.warn("STT config fetch failed", { error: error?.message });
          });
          if (needsSttConfigBeforeStart(getSettings())) {
            await configFetch;
          }
        }

        if (!isCurrent()) return false;
        const didStart = audioManagerRef.current.shouldUseStreaming()
          ? await audioManagerRef.current.startStreamingRecording()
          : await audioManagerRef.current.startRecording(startupTrace);
        if (!isCurrent()) return false;
        recordingStarted = didStart;
        if (didStart) startupTrace.startSettled();
        else startupTrace.finish("failed", "start_failed");
        if (didStart) dismissDictationError?.();

        // A quick tap can end the recording inside the start call itself (deferred
        // streaming stop) — don't pause media for a recording that already ended. See #1060.
        if (didStart && audioManagerRef.current.getState().isRecording) {
          if (getSettings().pauseMediaOnDictation) {
            window.electronAPI?.pauseMediaPlayback?.();
          }
          window.electronAPI?.registerCancelHotkey?.("Escape");
          if (getSettings().audioCuesEnabled) startupTrace.mark("readyCueRequested");
          void playStartCue(() => startupTrace.mark("readyCueScheduled"));
        }

        return didStart;
      } catch (error) {
        startupTrace?.finish("failed", "start_exception");
        throw error;
      } finally {
        if (!recordingStarted) startupTrace?.finish("incomplete", "start_abandoned");
        if (isCurrent()) {
          startLockRef.current = false;
          if (!recordingStarted) {
            manager?.cancelPreparedMicCapture?.();
            setIsPreparing(false);
            // Covers every exit above that never started a recording — the
            // policy-block early return, the mic-open failure, a stale
            // preparation generation, etc. Without this, a failed start leaves
            // the main process stuck reporting "preparing" forever, since
            // startRecording's failure path only fires onError, never the
            // onStateChange that normally reports "idle". The dedup makes this
            // a no-op when onStateChange already reported it first.
            if (reportedLifecycleRef.current === "preparing") reportLifecycle("idle");
          }
        }
      }
    },
    [t, toast, dismissDictationError, reportLifecycle, getStartupTrace]
  );

  const performStopRecording = useCallback(async () => {
    startupTraceRef.current?.finish("incomplete", "stopped_before_observation");
    if (
      !audioManagerRef.current?.getState().isRecording &&
      (startLockRef.current || reportedLifecycleRef.current === "preparing")
    ) {
      invalidatePreparation();
      audioManagerRef.current?.cancelRecording();
      window.electronAPI?.unregisterCancelHotkey?.();
      reportLifecycle("idle");
      return true;
    }
    if (stopLockRef.current) return false;
    stopLockRef.current = true;
    try {
      if (!audioManagerRef.current) return false;

      const currentState = audioManagerRef.current.getState();
      if (!currentState.isRecording && !currentState.isStreamingStartInProgress) return false;

      window.electronAPI?.unregisterCancelHotkey?.();
      setIsPreparing(false);
      setIsStopping(true);

      if (currentState.isStreaming || currentState.isStreamingStartInProgress) {
        void playStopCue();
        return await audioManagerRef.current.stopStreamingRecording();
      }

      const didStop = audioManagerRef.current.stopRecording();

      if (didStop) {
        void playStopCue();
      }

      return didStop;
    } finally {
      stopLockRef.current = false;
      setIsStopping(false);
    }
  }, [invalidatePreparation, reportLifecycle]);

  useEffect(() => {
    audioManagerRef.current = new AudioManager();

    // Resolve and pin the input device now, not on the first hotkey press, which
    // would otherwise wait on the device lookup before the mic can open.
    void audioManagerRef.current.cacheMicrophoneDeviceId?.();

    // Reset stale main-process state after a renderer reload or crash recovery.
    reportLifecycle("idle");

    const getRecoverableTranscript = (fallback = "") =>
      buildLiveTranscriptionPreview(
        audioManagerRef.current?.streamingFinalText,
        audioManagerRef.current?.streamingPartialText
      ).trim() || fallback.trim();

    const showDictationError = ({ title, description, transcript = "", duration }) => {
      onDictationError?.();
      const recoverableTranscript = getRecoverableTranscript(transcript);
      const actions = [
        {
          label: t("common.retry"),
          icon: "retry",
          dismissOnClick: false,
          onClick: () => performStartRecording(),
        },
      ];

      if (recoverableTranscript) {
        actions.push({
          label: t("hooks.audioRecording.errorActions.viewTranscript"),
          icon: "transcript",
          onClick: () => {
            onShowTranscriptRef.current?.(recoverableTranscript);
          },
        });
      }

      toast({
        title,
        description,
        variant: "destructive",
        presentation: "dictation-error",
        duration,
        actions,
      });
    };

    audioManagerRef.current.setCallbacks({
      onStateChange: ({
        isRecording,
        isProcessing,
        isStreaming,
        micCaptureStatus,
        startupTrace,
      }) => {
        if (isRecording && startupTrace) {
          feedbackTraceRef.current = startupTrace;
          startupTrace.mark("recordingStarted");
        }
        reportLifecycle(isRecording ? "recording" : isProcessing ? "processing" : "idle");
        if (!isRecording) {
          window.electronAPI?.unregisterCancelHotkey?.();
          // Resume media the instant recording ends, not after transcription.
          if (wasRecordingRef.current && getSettings().pauseMediaOnDictation) {
            window.electronAPI?.resumeMediaPlayback?.();
          }
        }
        wasRecordingRef.current = isRecording;
        setIsRecording(isRecording);
        setIsProcessing(isProcessing);
        setIsStreaming(isStreaming ?? false);
        if (isRecording) setIsPreparing(false);
        if (!isRecording) setIsStopping(false);
        if (micCaptureStatus) {
          setMicCaptureStatus(micCaptureStatus);
          const unavailable = micCaptureStatus === "unavailable";
          if (unavailable && !wasMicUnavailableRef.current) {
            wasMicUnavailableRef.current = true;
            toast({
              title: t("hooks.audioRecording.micDisconnected.title"),
              description: t("hooks.audioRecording.micDisconnected.description"),
              variant: "default",
            });
          } else if (micCaptureStatus === "active" && wasMicUnavailableRef.current) {
            wasMicUnavailableRef.current = false;
            toast({
              title: t("hooks.audioRecording.micRestored.title"),
              description: t("hooks.audioRecording.micRestored.description"),
              variant: "default",
            });
          } else if (micCaptureStatus === "inactive") {
            wasMicUnavailableRef.current = false;
          }
        }
        if (!isStreaming) {
          setPartialTranscript("");
        }
      },
      onError: (error) => {
        if (error?.code === "MIC_CAPTURE_FAILED") {
          startupTraceRef.current?.finish("failed", "microphone_unavailable");
          invalidatePreparation();
          reportLifecycle("idle");
        }
        setIsPreparing(false);
        setIsStopping(false);
        if (error?.code === "TRANSCRIPTION_CANCELLED" || error?.code === "REASON_CANCELLED") return;
        if (error?.title !== "Paste Error") {
          window.electronAPI?.hideDictationPreview?.();
        }
        const title = getRecordingErrorTitle(error, t);
        const description = getRecordingErrorDescription(error, t);
        showDictationError({
          title,
          description,
          duration: error?.code === "AUTH_EXPIRED" ? 8000 : undefined,
        });
        if (getSettings().pauseMediaOnDictation) {
          window.electronAPI?.resumeMediaPlayback?.();
        }
      },
      onNoAudio: () => {
        setIsPreparing(false);
        setIsStopping(false);
        window.electronAPI?.hideDictationPreview?.();
        if (getSettings().pauseMediaOnDictation) {
          window.electronAPI?.resumeMediaPlayback?.();
        }
        showDictationError({
          title: t("hooks.audioRecording.noAudio.title"),
          description: t("hooks.audioRecording.noAudio.description"),
        });
      },
      onPartialTranscript: (text) => {
        setPartialTranscript(text);
        const settings = getSettings();
        if (
          audioManagerRef.current?.getStreamingProviderName?.() !== "tinfoil-realtime" &&
          shouldShowByokStreamingPreview(
            settings.showTranscriptionPreview,
            settings.cloudTranscriptionMode
          )
        ) {
          const previewText = buildLiveTranscriptionPreview(
            audioManagerRef.current?.streamingFinalText,
            text
          );
          window.electronAPI
            ?.updateDictationPreview?.(previewText)
            .catch((error) =>
              logger.warn("Failed to update transcription preview", { error: error?.message })
            );
        }
      },
      onTranscriptionComplete: async (result) => {
        if (result.success) {
          const completedRecordingGeneration = preparationGenerationRef.current;
          dismissDictationError?.();
          const transcribedText = result.text?.trim();

          if (!transcribedText) {
            window.electronAPI?.hideDictationPreview?.();
            showDictationError({
              title: t("hooks.audioRecording.noAudio.title"),
              description: t("hooks.audioRecording.noAudio.description"),
            });
            return;
          }

          result.text = expandSnippets(result.text, getSettings().snippets);

          setTranscript(result.text);
          window.electronAPI?.completeDictationPreview?.({ text: result.text });

          if (result.warning) {
            toast({
              title: t("hooks.audioRecording.partialTranscription.title"),
              description: t("hooks.audioRecording.partialTranscription.description"),
              variant: "default",
            });
          }

          const isStreaming = result.source?.includes("streaming");
          const { autoPasteEnabled, keepTranscriptionInClipboard } = getSettings();

          const persistencePromise = audioManagerRef.current
            .saveTranscription(result.text, result.rawText ?? result.text, {
              clientTranscriptionId: result.clientTranscriptionId,
              // Spread rather than set: a result with no analytics timestamp
              // must not gain the key as undefined, matching how audioManager
              // carries this field and keeping the options object exactly what
              // callers without Insights data expect.
              ...(result.analyticsOccurredAt
                ? { analyticsOccurredAt: result.analyticsOccurredAt }
                : {}),
            })
            .then(
              (persisted) => {
                if (!persisted) {
                  logger.error(
                    "Failed to persist transcription",
                    {
                      clientTranscriptionId: result.clientTranscriptionId,
                      source: result.source,
                    },
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
            try {
              const clipboardResult = await window.electronAPI.writeClipboard(result.text);
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

          if (pushForceStoppedRef.current && autoPasteEnabled) {
            // The push hit its safety ceiling while the trigger keys were still
            // down. Injecting the paste shortcut into those held modifiers is
            // what silently loses the transcript, so keep it instead.
            const keptInClipboard = await keepInClipboard("push-force-stopped");
            window.electronAPI?.hideDictationPreview?.();
            showDictationError({
              title: t("hooks.audioRecording.pushForceStopped.title"),
              // Never promise a clipboard that rejected the write; the transcript
              // action on this pill is the recovery path either way.
              description: t(
                keptInClipboard
                  ? "hooks.audioRecording.pushForceStopped.description"
                  : "hooks.audioRecording.pushForceStopped.descriptionClipboardFailed"
              ),
              transcript: result.rawText ?? result.text,
            });
          } else if (autoPasteEnabled) {
            const pasteStart = performance.now();
            let pasteSucceeded = true;
            try {
              pasteSucceeded = await audioManagerRef.current.safePaste(result.text, {
                ...(isStreaming ? { fromStreaming: true } : {}),
                restoreClipboard: !keepTranscriptionInClipboard,
                allowClipboardFallback: isAccessibilitySkipped(),
                suppressError: true,
              });
            } catch (error) {
              pasteSucceeded = false;
              logger.warn("Failed to paste transcription", { error: error?.message }, "clipboard");
            }
            if (
              !pasteSucceeded &&
              completedRecordingGeneration === preparationGenerationRef.current &&
              localStorage.getItem("onboardingCompleted") === "true"
            ) {
              const copied = await keepInClipboard("paste-fallback");
              if (completedRecordingGeneration === preparationGenerationRef.current) {
                onShowTranscriptRef.current?.(result.text, {
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
              "streaming"
            );
            // Successful delivery closes the preview; failed delivery keeps
            // the final text available in the manual-copy panel.
            if (pasteSucceeded) {
              window.electronAPI?.hideDictationPreview?.();
              if (result.cleanupFailure) recordCleanupFailure(result.cleanupFailure);
            }
          } else if (keepTranscriptionInClipboard) {
            await keepInClipboard("clipboard-only");
          }

          if (result.source === "openai" && getSettings().useLocalWhisper) {
            toast({
              title: t("hooks.audioRecording.fallback.title"),
              description: t("hooks.audioRecording.fallback.description"),
              variant: "default",
            });
          }

          // Cloud usage: limit reached after this transcription
          if (result.source === "openwhispr" && result.limitReached) {
            // Notify control panel to show UpgradePrompt dialog
            window.electronAPI?.notifyLimitReached?.({
              wordsUsed: result.wordsUsed,
              limit:
                result.wordsRemaining !== undefined
                  ? result.wordsUsed + result.wordsRemaining
                  : 2000,
            });
          }

          if (audioManagerRef.current.shouldUseStreaming()) {
            audioManagerRef.current.warmupStreamingConnection();
          }

          await persistencePromise;
        }
      },
    });
    // A policy refresh can flip the effective screen-context value mid-session;
    // re-sync overlay content protection when it does.
    const unsubscribePolicy = usePolicyStore.subscribe(() => {});

    const handleToggle = async ({ startupRequest } = {}) => {
      if (!audioManagerRef.current) return;
      const currentState = audioManagerRef.current.getState();

      // A start still awaiting the mic open leaves isRecording false, so without
      // the lock check this toggle-off would take the start branch and be lost.
      // A prepare event alone is only a hint: the main process sends it before
      // the first toggle, which must adopt that capture rather than stop it.
      if (startLockRef.current || currentState.isRecording) {
        await performStopRecording();
      } else if (canStartDictation(currentState)) {
        await performStartRecording({ startupRequest });
      }
    };

    const handleStart = async (options) => {
      await performStartRecording(options);
    };

    const handleStop = async () => {
      await performStopRecording();
    };

    const disposeToggle = window.electronAPI.onToggleDictation((options) => {
      handleToggle(options);
      onToggle?.();
    });

    const disposeStart = window.electronAPI.onStartDictation?.((options) => {
      handleStart(options);
      onToggle?.();
    });

    const disposePrepare = window.electronAPI.onPrepareDictation?.((options) => {
      if (!audioManagerRef.current || startLockRef.current) return;
      if (!canStartDictation(audioManagerRef.current.getState())) return;
      const startupTrace = getStartupTrace(options?.startupRequest);
      startupTrace.mark("preparationEntered");
      audioManagerRef.current.selectMicrophoneForSession?.();
      preparationGenerationRef.current += 1;
      setIsPreparing(true);
      reportLifecycle("preparing");
      void audioManagerRef.current.prepareMicCapture?.(startupTrace);
    });

    const disposeCancelPreparation = window.electronAPI.onCancelDictationPreparation?.(() => {
      startupTraceRef.current?.finish("cancelled", "preparation_cancelled");
      invalidatePreparation();
      audioManagerRef.current?.cancelRecording();
      if (reportedLifecycleRef.current === "preparing") reportLifecycle("idle");
    });

    const disposeStop = window.electronAPI.onStopDictation?.(() => {
      handleStop();
      onToggle?.();
    });

    const disposeForceStopped = window.electronAPI.onDictationForceStopped?.((payload) => {
      // Listed rather than negated: a future renderer-initiated stop would not
      // leave the keys down, and must not be swept in here.
      if (payload?.reason === "timeout" || payload?.reason === "reset") {
        pushForceStoppedRef.current = true;
      }
    });

    // Cleanup
    return () => {
      preparationGenerationRef.current += 1;
      startLockRef.current = false;
      startupTraceRef.current?.finish("incomplete", "renderer_teardown");
      reportLifecycle("idle");
      unsubscribePolicy();
      disposeToggle?.();
      disposeStart?.();
      disposePrepare?.();
      disposeCancelPreparation?.();
      disposeStop?.();
      disposeForceStopped?.();
      if (audioManagerRef.current) {
        audioManagerRef.current.cleanup();
      }
    };
  }, [
    toast,
    onToggle,
    performStartRecording,
    performStopRecording,
    dismissDictationError,
    onDictationError,
    reportLifecycle,
    invalidatePreparation,
    getStartupTrace,
    t,
  ]);

  const cancelRecording = useCallback(async () => {
    startupTraceRef.current?.finish("cancelled", "recording_cancelled");
    if (audioManagerRef.current) {
      invalidatePreparation();
      reportLifecycle("idle");
      audioManagerRef.current.cancelPreparedMicCapture?.();
      window.electronAPI?.unregisterCancelHotkey?.();
      const state = audioManagerRef.current.getState();
      if (getSettings().pauseMediaOnDictation) {
        window.electronAPI?.resumeMediaPlayback?.();
      }
      // A streaming start in its mic-open phase is not yet `isStreaming`;
      // only the streaming cancel knows how to abandon it.
      if (state.isStreaming || state.isStreamingStartInProgress) {
        return await audioManagerRef.current.cancelStreamingRecording();
      }
      return audioManagerRef.current.cancelRecording();
    }
    return false;
  }, [invalidatePreparation, reportLifecycle]);

  const cancelProcessing = useCallback(() => {
    if (audioManagerRef.current) {
      return audioManagerRef.current.cancelProcessing();
    }
    return false;
  }, []);

  const getAudioLevel = useCallback(
    () => audioManagerRef.current?.getRecordingAudioLevel() ?? null,
    []
  );

  useEffect(() => {
    if (isRecording) feedbackTraceRef.current?.mark("readyFeedback");
  }, [isRecording]);

  const toggleListening = async () => {
    if (startLockRef.current || isPreparing) {
      await performStopRecording();
    } else if (!isRecording && !isProcessing) {
      await performStartRecording();
    } else if (isRecording) {
      await performStopRecording();
    }
  };

  return {
    isRecording,
    isProcessing,
    isStreaming,
    isPreparing,
    isStopping,
    micCaptureStatus,
    transcript,
    partialTranscript,
    startRecording: performStartRecording,
    stopRecording: performStopRecording,
    cancelRecording,
    cancelProcessing,
    toggleListening,
    getAudioLevel,
  };
};
