import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { createDictation, IDLE_DICTATION_SNAPSHOT } from "../helpers/dictation";
import { getRecordingErrorDescription, getRecordingErrorTitle } from "../utils/recordingErrors";

/**
 * React binding over the renderer `dictation` module, which owns Dictation
 * state, locks, and lifecycle reports (ADR-0003). This hook mirrors the
 * module's snapshot into React state and turns its notices into toasts.
 */
export const useAudioRecording = (toast, options = {}) => {
  const { t } = useTranslation();
  const [snapshot, setSnapshot] = useState(IDLE_DICTATION_SNAPSHOT);
  const dictationRef = useRef(null);

  // The Dictation lives for the whole mount, so it reads the caller's latest
  // callbacks through a ref instead of being recreated when they change.
  const presentationRef = useRef({ toast, t, ...options });
  useEffect(() => {
    presentationRef.current = { toast, t, ...options };
  });

  const startRecording = useCallback(
    async (startOptions) =>
      dictationRef.current ? dictationRef.current.start(startOptions) : false,
    []
  );

  useEffect(() => {
    const showDictationError = ({ title, description, transcript = "" }) => {
      const { toast, t, onDictationError } = presentationRef.current;
      onDictationError?.();
      const recoverableTranscript = transcript.trim();
      const actions = [
        {
          label: t("common.retry"),
          icon: "retry",
          dismissOnClick: false,
          onClick: () => startRecording(),
        },
      ];

      if (recoverableTranscript) {
        actions.push({
          label: t("hooks.audioRecording.errorActions.viewTranscript"),
          icon: "transcript",
          onClick: () => {
            presentationRef.current.onShowTranscript?.(recoverableTranscript);
          },
        });
      }

      toast({
        title,
        description,
        variant: "destructive",
        presentation: "dictation-error",
        actions,
      });
    };

    const showNotice = (key) => {
      const { toast, t } = presentationRef.current;
      toast({
        title: t(`hooks.audioRecording.${key}.title`),
        description: t(`hooks.audioRecording.${key}.description`),
        variant: "default",
      });
    };

    const handleNotice = (notice) => {
      const { t } = presentationRef.current;
      switch (notice.type) {
        case "dismissError":
          presentationRef.current.dismissDictationError?.();
          break;
        case "error":
          showDictationError({
            title: getRecordingErrorTitle(notice.error, t),
            description: getRecordingErrorDescription(notice.error, t),
          });
          break;
        case "noAudio":
          showDictationError({
            title: t("hooks.audioRecording.noAudio.title"),
            description: t("hooks.audioRecording.noAudio.description"),
          });
          break;
        case "pushForceStopped":
          showDictationError({
            title: t("hooks.audioRecording.pushForceStopped.title"),
            // Never promise a clipboard that rejected the write; the transcript
            // action on this pill is the recovery path either way.
            description: t(
              notice.keptInClipboard
                ? "hooks.audioRecording.pushForceStopped.description"
                : "hooks.audioRecording.pushForceStopped.descriptionClipboardFailed"
            ),
            transcript: notice.transcript,
          });
          break;
        case "deliveryFallback":
          presentationRef.current.onShowTranscript?.(notice.text, {
            copyFallback: notice.copyFallback,
          });
          break;
        case "micUnavailable":
          showNotice("micDisconnected");
          break;
        case "micRestored":
          showNotice("micRestored");
          break;
        case "partialTranscription":
          showNotice("partialTranscription");
          break;
      }
    };

    const dictation = createDictation({
      onNotice: handleNotice,
      onCommand: () => presentationRef.current.onToggle?.(),
    });
    dictationRef.current = dictation;
    const unsubscribe = dictation.subscribe(setSnapshot);

    return () => {
      unsubscribe();
      dictation.dispose();
      if (dictationRef.current === dictation) dictationRef.current = null;
    };
  }, [startRecording]);

  const stopRecording = useCallback(
    async () => (dictationRef.current ? dictationRef.current.stop() : false),
    []
  );

  const cancelRecording = useCallback(
    async () => (dictationRef.current ? dictationRef.current.cancelRecording() : false),
    []
  );

  const cancelProcessing = useCallback(
    () => (dictationRef.current ? dictationRef.current.cancelProcessing() : false),
    []
  );

  const toggleListening = useCallback(async () => {
    await dictationRef.current?.toggleListening();
  }, []);

  const getAudioLevel = useCallback(() => dictationRef.current?.getAudioLevel() ?? null, []);

  useEffect(() => {
    if (snapshot.isRecording) dictationRef.current?.markReadyFeedback();
  }, [snapshot.isRecording]);

  return {
    ...snapshot,
    startRecording,
    stopRecording,
    cancelRecording,
    cancelProcessing,
    toggleListening,
    getAudioLevel,
  };
};
