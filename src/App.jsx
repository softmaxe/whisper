import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CopyRecoveryPanel } from "./components/dictation/CopyRecoveryPanel";
import { LiquidCancelButton } from "./components/dictation/LiquidCancelButton";
import { LiveTranscriptPanel } from "./components/dictation/LiveTranscriptPanel";
import { PillCommandMenu } from "./components/dictation/PillCommandMenu";
import { PillTooltip } from "./components/dictation/PillTooltip";
import { VoiceModePanelCore } from "./components/dictation/VoiceModePanelCore";
import { VoicePill } from "./components/dictation/VoicePill";
import { useToast } from "./components/ui/useToast";
import {
  isVoicePillActivationKey,
  LIVE_TRANSCRIPT_ENTRANCE_TIMING,
  resolveListeningEntrancePresentation,
  resolveLiveTranscriptEntrancePresentation,
  resolvePillVisualSuppression,
  resolveVoiceActivityPresentation,
  resolveVoiceHorizontalDirection,
  resolveVoicePanelCorePresentation,
  resolveVoicePillDock,
  resolveVoicePillInteraction,
  resolveVoicePillShape,
  shouldActivateVoicePill,
  shouldOfferLiveTranscriptReopen,
  VOICE_PILL_FOOTPRINT,
} from "./helpers/voicePillPresentation";
import { useAudioRecording } from "./hooks/useAudioRecording";
import { useHotkey } from "./hooks/useHotkey";
import { useListeningEntrancePhase } from "./hooks/useListeningEntrancePhase";
import { useLiveTranscriptPanel } from "./hooks/useLiveTranscriptPanel";
import { useMainProcessNotifications } from "./hooks/useMainProcessNotifications";
import { useMainWindowSizeOwner } from "./hooks/useMainWindowSizeOwner";
import { useWindowDrag } from "./hooks/useWindowDrag";
import { useWindowResizeCompensation } from "./hooks/useWindowResizeCompensation";
import "./index.css";
import { useSettingsStore } from "./stores/settingsStore";
import { formatHotkeyListLabel } from "./utils/hotkeys";
import { createMainWindowResizeCoordinator } from "./utils/mainWindowResizeCoordinator";

const formatPillHotkeyLabel = (value) =>
  formatHotkeyListLabel(value)
    .replace(/\s*\+\s*/g, " + ")
    .replace(/\s+/g, " ")
    .trim();

const UNMOUNTED_RESIZE = {
  success: false,
  superseded: true,
  message: "Resize coordinator not mounted",
};

export default function App() {
  const [isHovered, setIsHovered] = useState(false);
  const [isCommandMenuOpen, setIsCommandMenuOpen] = useState(false);
  const buttonRef = useRef(null);
  const { toast, dismiss, toastCount, dictationErrorActionCount, dismissByPresentation } =
    useToast();
  const { t } = useTranslation();
  const { hotkey } = useHotkey();
  const { isDragging, handleMouseDown, handleMouseUp } = useWindowDrag();

  const [dragStartPos, setDragStartPos] = useState(null);
  const [hasDragged, setHasDragged] = useState(false);

  // Floating icon auto-hide setting (read from store, synced via IPC)
  const floatingIconAutoHide = useSettingsStore((s) => s.floatingIconAutoHide);
  const panelStartPosition = useSettingsStore((s) => s.panelStartPosition);
  const prevAutoHideRef = useRef(floatingIconAutoHide);
  const [voiceHorizontalDirection, setVoiceHorizontalDirection] = useState(() =>
    resolveVoiceHorizontalDirection(panelStartPosition)
  );
  const [mainWindowHorizontalDirection, setMainWindowHorizontalDirection] = useState(null);

  const setWindowInteractivity = React.useCallback((shouldCapture) => {
    window.electronAPI?.setMainWindowInteractivity?.(shouldCapture);
  }, []);
  const dismissDictationError = React.useCallback(
    () => dismissByPresentation("dictation-error"),
    [dismissByPresentation]
  );

  useEffect(() => {
    setWindowInteractivity(false);
    return () => setWindowInteractivity(false);
  }, [setWindowInteractivity]);

  useEffect(() => {
    let disposed = false;
    const applyDirection = (direction) => {
      if (!disposed && (direction === "left" || direction === "right")) {
        setMainWindowHorizontalDirection(direction);
      }
    };
    const unsubscribe =
      window.electronAPI?.onMainWindowHorizontalDirectionChanged?.(applyDirection);
    const initialDirection = window.electronAPI?.getMainWindowHorizontalDirection?.();
    initialDirection?.then(applyDirection).catch(() => {});
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, []);

  useWindowResizeCompensation();
  useMainProcessNotifications({ toast, dismiss, t });

  const mainWindowResizeCoordinatorRef = useRef(null);
  useEffect(() => {
    // Created in the effect, not lazily during render: React StrictMode's
    // dev-only setup→cleanup→setup cycle then disposes and recreates it
    // instead of disposing the only instance for the rest of the session.
    const coordinator = createMainWindowResizeCoordinator({
      resizeMainWindow: (sizeKey) => window.electronAPI?.resizeMainWindow?.(sizeKey),
      resizeAssistantWindowToContent: (height) =>
        window.electronAPI?.resizeAssistantWindowToContent?.(height),
    });
    mainWindowResizeCoordinatorRef.current = coordinator;
    return () => {
      coordinator.dispose();
      if (mainWindowResizeCoordinatorRef.current === coordinator) {
        mainWindowResizeCoordinatorRef.current = null;
      }
    };
  }, []);

  const requestMainWindowSize = React.useCallback(
    (sizeKey) =>
      mainWindowResizeCoordinatorRef.current?.resizeMainWindow(sizeKey) ??
      Promise.resolve(UNMOUNTED_RESIZE),
    []
  );
  const resizeLiveTranscriptToContent = React.useCallback(
    (height) =>
      mainWindowResizeCoordinatorRef.current?.resizeAssistantWindowToContent(height) ??
      Promise.resolve(UNMOUNTED_RESIZE),
    []
  );

  const onPanelOpened = React.useCallback(() => setIsHovered(false), []);

  // The live transcript needs recording state as effect deps, and recording
  // callbacks need the live transcript. This ref breaks the render-order
  // cycle; it is read only at event time, never during render.
  const liveTranscriptApiRef = useRef(null);

  const handleDictationError = React.useCallback(() => {
    liveTranscriptApiRef.current?.dismissForError();
  }, []);

  const handleDictationToggle = React.useCallback(() => {
    setIsCommandMenuOpen(false);
    if (!liveTranscriptApiRef.current?.openRef.current) {
      setWindowInteractivity(false);
    }
  }, [setWindowInteractivity]);

  const {
    isRecording,
    isProcessing,
    isPreparing,
    isStopping,
    micCaptureStatus,
    toggleListening,
    cancelRecording,
    cancelProcessing,
    getAudioLevel,
  } = useAudioRecording(toast, {
    onToggle: handleDictationToggle,
    dismissDictationError,
    onDictationError: handleDictationError,
    onShowTranscript: (text, options) => {
      liveTranscriptApiRef.current?.showFinalText(text, options);
    },
  });
  const isVisuallyProcessing = isProcessing || isPreparing || isStopping;

  const liveTranscript = useLiveTranscriptPanel({
    resizeToContent: resizeLiveTranscriptToContent,
    onWillOpen: onPanelOpened,
    isRecording,
    isProcessing,
  });

  useLayoutEffect(() => {
    liveTranscriptApiRef.current = liveTranscript;
  });

  // Must run before the size owner's ladder effect below: the error teardown
  // drops the live transcript's open ref, which the ladder reads this commit.
  useEffect(() => {
    if (dictationErrorActionCount > 0) handleDictationError();
  }, [dictationErrorActionCount, handleDictationError]);

  // Direction is part of the interaction's geometry, not a live decoration.
  // Hold the origin through processing and panel exit so every close animation
  // returns to the same side from which that voice session started.
  const voiceDirectionLocked = isRecording || isVisuallyProcessing || liveTranscript.mounted;
  useLayoutEffect(() => {
    if (voiceDirectionLocked) return;
    setVoiceHorizontalDirection(
      mainWindowHorizontalDirection ?? resolveVoiceHorizontalDirection(panelStartPosition)
    );
  }, [mainWindowHorizontalDirection, panelStartPosition, voiceDirectionLocked]);

  const voiceActivity = resolveVoiceActivityPresentation({
    isRecording,
    // Mic warm-up is an acknowledged press, not work on a transcript. Keeping
    // isPreparing out of the thinking state leaves the press on the pulsing
    // "processing" mic-state pill instead of lighting the glow at hotkey time.
    isProcessing: isProcessing || isStopping,
  });
  const listeningEntrancePhase = useListeningEntrancePhase(isRecording);
  const listeningEntrance = resolveListeningEntrancePresentation({
    isRecording,
    phase: listeningEntrancePhase,
  });
  const isCompactPill = isRecording ? listeningEntrance.compactPill : voiceActivity.compactPill;
  // BASE and RECORDING resolve to the same native box (windowConfig.js), so
  // recording edges never call setBounds — resizing the transparent window
  // always kicks a compositor frame. This flag still feeds the size ladder so
  // a menu opening over the compact pill resolves to EXPANDED geometry.
  const windowFitsCompactPill = isRecording || voiceActivity.compactPill;

  const { dictationErrorPillHandoffActive, panelReturnResizeActive } = useMainWindowSizeOwner({
    requestMainWindowSize,
    dictationErrorActionCount,
    toastCount,
    isCommandMenuOpen,
    isCompactPill: windowFitsCompactPill,
    isDictationActive: isRecording || isVisuallyProcessing,
    liveTranscriptOpen: liveTranscript.open,
    liveTranscriptMounted: liveTranscript.mounted,
    liveTranscriptOpenRef: liveTranscript.openRef,
    liveTranscriptCopyFallback: liveTranscript.copyFallback,
  });

  useEffect(() => {
    if (isCommandMenuOpen || toastCount > 0 || liveTranscript.mounted) {
      setWindowInteractivity(true);
    } else if (!isHovered) {
      setWindowInteractivity(false);
    }
  }, [isCommandMenuOpen, isHovered, toastCount, liveTranscript.mounted, setWindowInteractivity]);

  useEffect(() => {
    if (isRecording && dictationErrorActionCount > 0) {
      dismissByPresentation("dictation-error");
    }
  }, [isRecording, dictationErrorActionCount, dismissByPresentation]);

  // Sync auto-hide from main process — setState directly to avoid IPC echo
  useEffect(() => {
    const unsubscribe = window.electronAPI?.onFloatingIconAutoHideChanged?.((enabled) => {
      localStorage.setItem("floatingIconAutoHide", String(enabled));
      useSettingsStore.setState({ floatingIconAutoHide: enabled });
    });
    return () => unsubscribe?.();
  }, []);

  const isRecordingRef = useRef(isRecording);

  useLayoutEffect(() => {
    isRecordingRef.current = isRecording;
  }, [isRecording]);

  useEffect(() => {
    const unsubscribe = window.electronAPI?.onCancelHotkeyPressed?.(() => {
      if (isRecordingRef.current) cancelRecording();
    });
    return () => unsubscribe?.();
  }, [cancelRecording]);

  // Auto-hide the floating icon when idle (setting enabled or dictation cycle completed)
  useEffect(() => {
    let hideTimeout;

    if (
      floatingIconAutoHide &&
      !isRecording &&
      !isVisuallyProcessing &&
      toastCount === 0 &&
      !dictationErrorPillHandoffActive &&
      !liveTranscript.mounted &&
      !liveTranscript.copyFallback
    ) {
      // Delay briefly so processing can start after recording stops without a flash
      hideTimeout = setTimeout(() => {
        window.electronAPI?.hideWindow?.();
      }, 500);
    } else if (!floatingIconAutoHide && prevAutoHideRef.current) {
      window.electronAPI?.showDictationPanel?.();
    }

    prevAutoHideRef.current = floatingIconAutoHide;
    return () => clearTimeout(hideTimeout);
  }, [
    isRecording,
    isVisuallyProcessing,
    floatingIconAutoHide,
    toastCount,
    dictationErrorPillHandoffActive,
    liveTranscript.mounted,
    liveTranscript.copyFallback,
  ]);

  const handleClose = () => {
    window.electronAPI.hideWindow();
  };

  useEffect(() => {
    const handleKeyPress = (e) => {
      if (e.key === "Escape") {
        if (isCommandMenuOpen) {
          setIsCommandMenuOpen(false);
        } else if (isRecording) {
          cancelRecording();
        } else if (isPreparing) {
          cancelRecording();
        } else if (isProcessing) {
          cancelProcessing();
        } else if (liveTranscript.copyFallback) {
          liveTranscript.close({ suppress: true, clear: true });
        } else {
          handleClose();
        }
      }
    };

    document.addEventListener("keydown", handleKeyPress);
    return () => document.removeEventListener("keydown", handleKeyPress);
  }, [
    isCommandMenuOpen,
    isRecording,
    isPreparing,
    isProcessing,
    cancelRecording,
    cancelProcessing,
    liveTranscript.copyFallback,
    liveTranscript.close,
  ]);

  // Determine current mic state
  const getMicState = () => {
    if (isRecording && (micCaptureStatus === "reconnecting" || micCaptureStatus === "unavailable"))
      return "unavailable";
    if (isRecording) return "recording";
    if (isVisuallyProcessing) return "processing";
    if (isHovered && !isRecording && !isVisuallyProcessing) return "hover";
    return "idle";
  };

  const micState = getMicState();

  const getMicTooltip = () => {
    switch (micState) {
      case "recording":
        return t("app.mic.recording");
      case "unavailable":
        return t("app.mic.waitingForMicrophone");
      case "processing":
        return t("app.mic.processing");
      default:
        return formatPillHotkeyLabel(hotkey);
    }
  };

  const micTooltip = getMicTooltip();
  const panelOpen = liveTranscript.open;
  const panelMounted = liveTranscript.mounted;
  const canReopenLiveTranscript =
    shouldOfferLiveTranscriptReopen({
      manuallyCollapsed: liveTranscript.manuallyCollapsed,
      isRecording,
      isProcessing,
    }) && !panelMounted;
  const voicePillInteraction = resolveVoicePillInteraction({
    liveTranscriptMounted: liveTranscript.mounted,
    isRecording,
    isProcessing,
    isHovered,
  });
  const pillIsInteractive = voicePillInteraction.pillInteractive;
  // The cancel button pours out of the pill as a fused liquid skin — except
  // inside the Live Transcript panel, where the pill is already headless and
  // the classic bordered circle stays (with the same emergence motion).
  const [cancelSkinActive, setCancelSkinActive] = useState(false);
  const cancelFused = !liveTranscript.open;
  const activateVoicePill = () => {
    if (!pillIsInteractive) return;
    if (canReopenLiveTranscript) {
      liveTranscript.reopen();
      return;
    }
    if (
      shouldActivateVoicePill({
        hasDragged,
        liveTranscriptMounted: liveTranscript.mounted,
        isProcessing: micState === "processing",
      })
    ) {
      setIsCommandMenuOpen(false);
      toggleListening();
    }
  };
  // The core itself never unmounts; only its inner sections change ownership.
  const activeVoicePanel = resolveVoicePanelCorePresentation({
    liveTranscriptOpen: liveTranscript.open,
    liveTranscriptMounted: liveTranscript.mounted,
  });
  const activeVoicePanelMode = activeVoicePanel.mode;
  const liveTranscriptEntrance = resolveLiveTranscriptEntrancePresentation(
    liveTranscript.entrancePhase
  );
  const activeVoicePanelLabel =
    activeVoicePanelMode === "live-transcript" ? t("transcriptionPreview.label") : undefined;
  const commonPillState =
    micState === "unavailable"
      ? "unavailable"
      : listeningEntrance.activeState || voiceActivity.activeState || micState;
  // The pill shape tracks the pill's footprint through the entrance phases
  // (logo-collapsed thinking renders 40×40 even while recording). These are
  // targets, not rendered sizes: the pill transitions between footprints over
  // GROW_TRANSITION, and the skin tweens its geometry to match
  // (usePillFootprintTween in LiquidCancelButton).
  const pillShape = resolveVoicePillShape({
    variant: panelOpen ? "panel" : "floating",
    state: commonPillState,
    expanded: !panelOpen && isCompactPill,
    collapseToLogo: listeningEntrance.collapseToLogo,
    waveformOnlyWhileRecording: panelMounted,
  });
  const cancelPillFootprint = VOICE_PILL_FOOTPRINT[pillShape];
  const voicePillDock = resolveVoicePillDock({
    liveTranscriptOpen: liveTranscript.open,
    liveTranscriptEntrancePhase: liveTranscript.entrancePhase,
    panelStartPosition,
    horizontalDirection: voiceHorizontalDirection,
  });
  const voicePillTravelDuration =
    liveTranscript.open && liveTranscript.entrancePhase === "encapsulate"
      ? LIVE_TRANSCRIPT_ENTRANCE_TIMING.encapsulateMs
      : LIVE_TRANSCRIPT_ENTRANCE_TIMING.horizontalMs;
  const dictationErrorSuppressesPill =
    dictationErrorActionCount > 0 || dictationErrorPillHandoffActive;
  const pillVisuallySuppressed =
    Boolean(liveTranscript.copyFallback) ||
    resolvePillVisualSuppression({
      dictationErrorSuppressed: dictationErrorSuppressesPill,
      panelReturnResizeActive,
    });

  return (
    <div className="dictation-window">
      {/* The panel footer can hide this pill, but never unmounts it. */}
      <div
        className={`voice-pill-position voice-pill-position-${voicePillDock} fixed z-50 transition-opacity duration-150 ease-out ${
          pillVisuallySuppressed ? "pointer-events-none" : ""
        } ${pillVisuallySuppressed ? "opacity-0" : "opacity-100"}`}
        style={{
          "--voice-pill-travel-duration": `${voicePillTravelDuration}ms`,
        }}
        data-dictation-error-suppressed={dictationErrorSuppressesPill || undefined}
        aria-hidden={pillVisuallySuppressed || undefined}
      >
        <div
          className="relative flex items-center"
          onMouseEnter={() => {
            if (!pillIsInteractive) return;
            setIsHovered(true);
            setWindowInteractivity(true);
          }}
          onMouseLeave={() => {
            setIsHovered(false);
            if (!pillIsInteractive) return;
            if (!isCommandMenuOpen) {
              setWindowInteractivity(false);
            }
          }}
        >
          <PillTooltip
            content={canReopenLiveTranscript ? t("transcriptionPreview.label") : micTooltip}
            disabled={panelMounted}
            align={panelStartPosition === "center" ? "center" : voiceHorizontalDirection}
          >
            <VoicePill
              ref={buttonRef}
              variant={panelOpen ? "panel" : "floating"}
              state={commonPillState}
              expanded={!panelOpen && isCompactPill}
              collapseToLogo={listeningEntrance.collapseToLogo}
              waveformVisible={listeningEntrance.waveformVisible}
              waveformOnlyWhileRecording={panelMounted}
              integratedWithPanel={liveTranscript.open}
              liquidFused={cancelSkinActive}
              showExpandChevron={canReopenLiveTranscript && isHovered}
              getAudioLevel={getAudioLevel}
              isDragging={isDragging}
              horizontalDirection={voiceHorizontalDirection}
              role={pillIsInteractive ? "button" : "status"}
              tabIndex={pillIsInteractive ? 0 : undefined}
              aria-label={
                canReopenLiveTranscript || liveTranscript.mounted
                  ? t("transcriptionPreview.label")
                  : micTooltip
              }
              onMouseDown={(e) => {
                if (panelMounted) {
                  setHasDragged(false);
                  return;
                }
                setIsCommandMenuOpen(false);
                setDragStartPos({ x: e.clientX, y: e.clientY });
                setHasDragged(false);
                handleMouseDown(e);
              }}
              onMouseMove={(e) => {
                if (panelMounted) return;
                if (dragStartPos && !hasDragged) {
                  const distance = Math.sqrt(
                    Math.pow(e.clientX - dragStartPos.x, 2) +
                      Math.pow(e.clientY - dragStartPos.y, 2)
                  );
                  if (distance > 5) {
                    // 5px threshold for drag
                    setHasDragged(true);
                  }
                }
              }}
              onMouseUp={(e) => {
                if (panelMounted) return;
                handleMouseUp(e);
                setDragStartPos(null);
              }}
              onClick={(e) => {
                activateVoicePill();
                e.preventDefault();
              }}
              onKeyDown={(event) => {
                if (event.repeat || !isVoicePillActivationKey(event.key)) return;
                event.preventDefault();
                activateVoicePill();
              }}
              onContextMenu={(e) => {
                if (panelMounted) return;
                e.preventDefault();
                if (!hasDragged) {
                  setWindowInteractivity(true);
                  setIsCommandMenuOpen((prev) => !prev);
                }
              }}
            />
          </PillTooltip>
          <LiquidCancelButton
            visible={voicePillInteraction.cancelVisible}
            fused={cancelFused}
            pillWidth={cancelPillFootprint.width}
            pillHeight={cancelPillFootprint.height}
            pillState={commonPillState}
            flowBar={pillShape === "listening"}
            ariaLabel={
              isRecording ? t("app.buttons.cancelRecording") : t("app.buttons.cancelProcessing")
            }
            onCancel={() => {
              if (isRecording) cancelRecording();
              else cancelProcessing();
            }}
            onFusedSkinChange={setCancelSkinActive}
          />
          {!panelMounted && isCommandMenuOpen && (
            <PillCommandMenu
              buttonRef={buttonRef}
              isRecording={isRecording}
              isHovered={isHovered}
              setWindowInteractivity={setWindowInteractivity}
              onToggleListening={() => {
                toggleListening();
              }}
              onHide={() => {
                setIsCommandMenuOpen(false);
                setWindowInteractivity(false);
                handleClose();
              }}
              onClose={() => setIsCommandMenuOpen(false)}
            />
          )}
        </div>
      </div>

      <VoiceModePanelCore
        mode={liveTranscript.copyFallback ? null : activeVoicePanelMode}
        open={!liveTranscript.copyFallback && activeVoicePanel.open}
        stage={
          activeVoicePanelMode === "live-transcript" ? liveTranscriptEntrance.coreStage : "content"
        }
        horizontalDirection={voiceHorizontalDirection}
        label={activeVoicePanelLabel}
        measurementRevision={
          activeVoicePanelMode === "live-transcript" ? liveTranscript.measurementText : null
        }
        onPreferredHeightChange={liveTranscript.requestHeight}
      >
        {!liveTranscript.copyFallback && (
          <LiveTranscriptPanel
            text={liveTranscript.mounted ? liveTranscript.text : ""}
            measurementText={liveTranscript.mounted ? liveTranscript.measurementText : ""}
            phase={liveTranscript.phase}
            processing={liveTranscript.mounted && isProcessing}
            controlsVisible={liveTranscript.mounted && liveTranscriptEntrance.controlsVisible}
            contentVisible={liveTranscript.mounted && liveTranscriptEntrance.contentVisible}
            onCollapse={() => liveTranscript.close({ suppress: true })}
            onHoldChange={liveTranscript.holdFinal}
          />
        )}
      </VoiceModePanelCore>
      {liveTranscript.copyFallback && liveTranscript.mounted && (
        <CopyRecoveryPanel
          open={liveTranscript.open}
          text={liveTranscript.text}
          copyFallback={liveTranscript.copyFallback}
          onClose={() => liveTranscript.close({ suppress: true, clear: true })}
          onHoldChange={liveTranscript.holdFinal}
          onPreferredHeightChange={liveTranscript.requestHeight}
        />
      )}
    </div>
  );
}
