import ReasoningService from "../services/ReasoningService";
import { getEffectiveCleanupModel, getSettings, useSettingsStore } from "../stores/settingsStore";
import {
  applyChineseScript,
  mergeWhisperPrompt,
  resolveChineseScriptTarget,
  resolveCleanupLanguage,
} from "../utils/chineseScript";
import { getBaseLanguageCode } from "../utils/languageSupport";
import logger from "../utils/logger";
import { observeFirstAudio, waitForFirstAudio } from "./firstAudio";
import { ActiveMicRecoveryController } from "./activeMicRecovery";
import { countSpokenWords, localDateKey, resolveAnalyticsMode } from "./analytics";
import { shouldSaveDiscardedRecording } from "./discardedRecording";
import {
  createLocalSpeechGateState,
  getLocalSpeechGateDecision,
  recordLocalSpeechWindow,
} from "./localSpeechGate";
import { waitForTrackReady } from "./micTrackHealth";
import {
  getMicrophoneSelectionMode,
  isCacheableMicrophoneResolution,
  resolvePreferredMicrophone,
} from "./microphoneSelection";
import {
  discardPreRoll,
  disposePreparedCapture,
  PRE_ROLL_MAX_AGE_MS,
  PreparedMicCapture,
} from "./preparedMicCapture";

import { getAgentName } from "../utils/agentName";
import {
  DICTIONARY_ECHO_CODE,
  dictionaryEchoError,
  matchesDictionaryPrompt,
} from "../utils/dictionaryEchoFilter.js";
import { dictionaryKeywords, usesTranscriptionKeywords } from "../utils/dictionaryKeywords.js";
import { dictionaryPromptLimit, trimDictionaryPrompt } from "../utils/dictionaryPromptCap.js";
import { getDictionaryHintWords } from "../utils/snippets";
import { isEmptyRecording } from "./recordingGuard";
import { evaluateFinishedRecording, withSalvageWarning } from "./recordingValidation";
import { resolveTranscriptionRoute } from "./transcriptionRoute.ts";

const REASONING_CACHE_TTL = 30000; // 30 seconds
const RECORDING_TIMESLICE_MS = 250; // flush chunks periodically so short recordings still carry audio frames. See #871.
const neverCancelled = () => false;
const SELF_HOSTED_SOURCE = "self-hosted";

const hasTextContent = (value) => typeof value === "string" && value.trim().length > 0;

const cleanupFailureFromError = (error) => ({
  message: error?.message || String(error),
  ...(error?.messageKey ? { messageKey: error.messageKey } : {}),
  ...(error?.messageParams ? { messageParams: error.messageParams } : {}),
  ...(error?.action ? { action: error.action } : {}),
  ...(error?.actionKey ? { actionKey: error.actionKey } : {}),
  ...(error?.copyCommand ? { copyCommand: error.copyCommand } : {}),
  ...(error?.technicalDetails ? { technicalDetails: error.technicalDetails } : {}),
});

const micDeviceKey = (settings) =>
  `${settings.microphoneSelectionMode}|${settings.selectedMicDeviceId}`;

function getEffectiveRetentionPreferences() {
  const { dataRetentionEnabled, audioRetentionDays } = getSettings();
  return { dataRetentionEnabled, audioRetentionDays };
}

// The cleanup request configuration, or null when no cleanup model is set.
// A truncated reply must fail rather than replace the dictation with its
// first part, so cleanup always requires complete output (#2091).
function resolveCleanupConfig(settings) {
  if (!settings.useCleanupModel || !getEffectiveCleanupModel()?.trim()) return null;
  return {
    inferenceScope: /** @type {const} */ ("dictationCleanup"),
    disableThinking: settings.cleanupDisableThinking,
    // Pin cleanup to 0; zero does not guarantee determinism.
    temperature: 0,
    requireCompleteOutput: true,
  };
}

function audioExtensionForMime(mimeType) {
  if (mimeType.includes("ogg")) return "ogg";
  if (mimeType.includes("mp4")) return "mp4";
  if (mimeType.includes("mpeg")) return "mp3";
  if (mimeType.includes("wav")) return "wav";
  return "webm";
}

const MICROPHONE_CAPTURE_ERROR = Object.freeze({
  code: "MIC_CAPTURE_FAILED",
  title: "Microphone unavailable",
  messageKey: "hooks.audioRecording.errorDescriptions.microphoneUnavailable",
});

class AudioManager {
  constructor() {
    this.mediaRecorder = null;
    this.audioChunks = [];
    this.isRecording = false;
    this.isProcessing = false;
    this.onStateChange = null;
    this.onError = null;
    this.onTranscriptionComplete = null;
    this.micCaptureStatus = "inactive";
    this._micWarmedAt = 0;
    this._startInProgress = false;
    this._micOpenReported = false;
    this.preparedMicCapture = new PreparedMicCapture({
      dispose: (prepared) => this._disposePrepared(prepared),
      onActiveChange: () => this._syncMicOpenGate(),
    });
    const micSettings = getSettings();
    this._micDeviceKey = micDeviceKey(micSettings);
    this._unsubscribeSettings = useSettingsStore.subscribe((state) => {
      const deviceKey = micDeviceKey(state);
      if (deviceKey !== this._micDeviceKey) {
        this._micDeviceKey = deviceKey;
        this.cachedMicDeviceId = null;
        this._micWarmedAt = 0;
      }
    });

    // Refresh next-session selection without changing the current capture.
    this._onDeviceChange = () => {
      this.cachedMicDeviceId = null;
      this._micWarmedAt = 0;
      // The main process keeps the OS default mic until told it changed, so
      // re-resolve now rather than on the next hotkey press.
      window.electronAPI?.getSystemDefaultMicrophone?.({ refresh: true })?.catch(() => {});
    };
    navigator.mediaDevices?.addEventListener?.("devicechange", this._onDeviceChange);
    this._unsubscribeLidState = window.electronAPI?.onLaptopLidStateChanged?.(() => {
      if (getMicrophoneSelectionMode(getSettings()) !== "auto") return;
      this.cachedMicDeviceId = null;
      this._micWarmedAt = 0;
    });
    this.recordingStartTime = null;
    this.reasoningAvailabilityCache = { value: false, expiresAt: 0 };
    this.cachedReasoningPreference = null;
    this.cachedMicDeviceId = null;
    this._activeTranscriptionAbortController = null;
    this.pendingCleanupFailure = null;
    this._processingCancellationGeneration = 0;
    this._activeProcessingPipeline = null;
    this.lastAudioBlob = null;
    this.lastAudioMetadata = null;
    this._localSpeechGateState = null;
    this._batchSegments = [];
    this._rotatingBatchRecorder = null;
    this._rotationResolve = null;
    this.micRecovery = new ActiveMicRecoveryController({
      mediaDevices: navigator.mediaDevices,
      onFailure: () => this.failMicrophoneCapture(),
      onStatusChange: (status) => this.setMicCaptureStatus(status),
    });
  }

  getCustomDictionaryPrompt() {
    const words = getDictionaryHintWords(getSettings());
    return words.length > 0 ? words.join(", ") : null;
  }

  // Whisper only accepts language "zh"; script (简体/繁體) is applied here. See #975.
  // No transcript exists yet, so only an explicit zh-CN/zh-TW may bias the prompt.
  getWhisperPrompt(settings = getSettings(), dictionaryPrompt = this.getCustomDictionaryPrompt()) {
    return mergeWhisperPrompt(
      dictionaryPrompt,
      resolveChineseScriptTarget(
        this.getEffectiveSttLanguage(settings),
        settings.chineseScriptPreference
      )
    );
  }

  // Cleanup runs before the translate step, so it still works in the STT language.
  getCleanupLanguage(settings) {
    return resolveCleanupLanguage(this.getEffectiveSttLanguage(settings));
  }

  finalizeChineseScript(text, settings = getSettings()) {
    return applyChineseScript(
      text,
      resolveChineseScriptTarget(
        this.getEffectiveSttLanguage(settings),
        settings.chineseScriptPreference,
        text
      )
    );
  }

  // Check the dictionary on its own as well as the full prompt: the echo filter needs
  // 70% of the prompt's words to appear, and a Chinese script bias counts as one more
  // word, which alone pushes a one- or two-term dictionary under the threshold.
  isDictionaryEcho(text) {
    return (
      matchesDictionaryPrompt(text, this.getCustomDictionaryPrompt()) ||
      matchesDictionaryPrompt(text, this.getWhisperPrompt())
    );
  }

  setCallbacks({ onStateChange, onError, onNoAudio = undefined, onTranscriptionComplete }) {
    this.onStateChange = onStateChange;
    this.onError = onError;
    this.onNoAudio = onNoAudio;
    this.onTranscriptionComplete = onTranscriptionComplete;
  }

  setMicCaptureStatus(status) {
    if (this.micCaptureStatus === status) return;
    this.micCaptureStatus = status;
    if (!this.isRecording) return;
    this.onStateChange?.({
      isRecording: this.isRecording,
      isProcessing: this.isProcessing,
      micCaptureStatus: status,
    });
  }

  failMicrophoneCapture() {
    // Use the existing cancellation retention policy for interrupted recordings.
    this.cancelRecording();
    this.onError?.(MICROPHONE_CAPTURE_ERROR);
  }

  async beginMicRecovery(stream) {
    // A stop/cancel can land during the awaits between recorder start and this
    // call; never arm recovery for a recording that already ended.
    if (!this.isRecording) return;
    await this.micRecovery.start(stream, {
      followDefault: false,
    });
  }

  async replaceActiveMic(replacement, previous) {
    if (!this.isRecording) throw new Error("Recording is no longer active");
    await this.replaceBatchMic(replacement, previous);
  }

  async mergeRecordedSegments(segments) {
    // Header-only segments carry no audio frames and crash FFmpeg's concat (#871).
    const usable = segments.filter((segment) => segment && !isEmptyRecording(segment.size));
    if (usable.length === 0) return null;
    if (usable.length === 1) return usable[0];
    const payload = await Promise.all(
      usable.map(async (segment) => ({
        buffer: await segment.arrayBuffer(),
        mimeType: segment.type || "audio/webm",
      }))
    );
    const result = await window.electronAPI.mergeAudioSegments(payload);
    if (!result?.success) throw new Error(result?.error || "Failed to merge audio segments");
    return new Blob([result.buffer], { type: result.mimeType });
  }

  getLargestRecordedSegment(segments) {
    return segments
      .filter((segment) => segment && !isEmptyRecording(segment.size))
      .reduce(
        (largest, segment) => (segment.size > (largest?.size || 0) ? segment : largest),
        null
      );
  }

  // Called at every recording start so a result can never carry a cleanup
  // failure left over from an earlier, abandoned recording.
  resetRecordingRequest() {
    this.pendingCleanupFailure = null;
  }

  getEffectiveSttLanguage(settings) {
    return settings.preferredLanguage;
  }

  selectMicrophoneForSession() {
    if (this._captureSession?.preparationExpired && !this.isRecording && !this._startInProgress) {
      this._endCaptureSession();
    }
    void this.getAudioConstraints(this._getCaptureSession()).catch(() => {});
  }

  async getAudioConstraints(session = null) {
    if (session?.constraints) return session.constraints;
    const resolve = async () => {
      const resolution = await resolvePreferredMicrophone({
        settings: session?.settings ?? getSettings(),
        refreshSystemDefault: true,
      });
      const deviceId = resolution.device?.deviceId;
      // A label alone cannot establish that a changed ID is the same physical input.
      if (
        !deviceId ||
        ["default", "communications"].includes(deviceId) ||
        resolution.status === "remapped"
      ) {
        const error = new Error("Selected microphone is unavailable");
        error.name = "MicUnusableError";
        throw error;
      }
      return {
        audio: {
          deviceId: { exact: deviceId },
          echoCancellation: false,
          noiseSuppression: false,
          autoGainControl: false,
          channelCount: 2,
        },
      };
    };
    const pending = resolve();
    if (session) session.constraints = pending;
    return pending;
  }

  async cacheMicrophoneDeviceId() {
    if (this.cachedMicDeviceId) return; // Already cached

    try {
      const resolution = await resolvePreferredMicrophone({
        settings: { ...getSettings(), setSelectedMicDevice: undefined },
      });
      if (isCacheableMicrophoneResolution(resolution)) {
        this.cachedMicDeviceId = resolution.device.deviceId;
        logger.debug(
          "Microphone device ID pre-cached",
          { mode: resolution.mode, status: resolution.status },
          "audio"
        );
      }
    } catch (error) {
      logger.debug("Failed to pre-cache microphone device ID", { error: error.message }, "audio");
    }
  }

  // Open the mic the moment a dictation is likely (push-to-talk key-down,
  // toggle press) and hand the same stream — plus everything its pre-roll
  // recorder already captured — to the real recording. Replaces the one-shot
  // warmupMicDriver (#845): a raced warm-up never resolved before the
  // recording's own open, so it only ever added a concurrent double open.
  async prepareMicCapture(startupTrace = null) {
    // A start already awaiting the mic open leaves isRecording false for as long
    // as that open takes, so without this guard a second prepare would open the
    // device again and buffer a pre-roll no recording ever answers.
    if (
      this._startInProgress ||
      this.isRecording ||
      this.isProcessing ||
      this.mediaRecorder?.state === "recording"
    ) {
      return null;
    }
    const session = this._getCaptureSession();
    try {
      const prepared = await this.preparedMicCapture.prepare(async () => {
        const constraints = await this.getAudioConstraints(session);
        this._assertCaptureSession(session);
        const stream = await this._acquireCaptureStream(constraints, session, startupTrace);
        this._assertCaptureSession(session);
        void this.micRecovery.start(stream, { followDefault: false });
        const value = {
          stream,
          constraints,
          startupTrace,
          recorder: null,
          chunks: [],
          startedAt: Date.now(),
        };
        this._startPreRollRecorder(value);
        return value;
      });
      if (prepared) {
        this.observePreparedCapture(prepared, startupTrace);
        logger.debug("Microphone capture prepared", { preRoll: !!prepared.recorder }, "audio");
      }
      return prepared;
    } catch (e) {
      if (this._captureSession === session) this.failMicrophoneCapture();
      return null;
    }
  }

  cancelPreparedMicCapture() {
    this.preparedMicCapture.cancel();
  }

  _getCaptureSession() {
    return (this._captureSession ??= {
      settings: { ...getSettings(), setSelectedMicDevice: undefined },
      controller: new AbortController(),
      streams: new Set(),
      observations: new Map(),
    });
  }

  _assertCaptureSession(session) {
    if (session.controller.signal.aborted || this._captureSession !== session) {
      throw new DOMException("Capture request ended", "AbortError");
    }
  }

  // End ownership before stopping tracks: queued device and recorder callbacks
  // must not be able to resume this request or change a subsequent recording.
  _endCaptureSession() {
    const session = this._captureSession;
    this._captureSession = null;
    session?.controller.abort();
    for (const observation of session?.observations.values() ?? []) observation.cancel();
    this._startInProgress = false;
    this.preparedMicCapture?.cancel();
    this.micRecovery.stop();
    this._rotatingBatchRecorder = null;
    this._rotationResolve?.();
    this._rotationResolve = null;
    for (const stream of session?.streams ?? []) {
      stream.getTracks().forEach((track) => track.stop());
    }
    this.teardownSpeechGate();
  }

  async _openCaptureStream(constraints, session, startupTrace = null, attempt = null) {
    this._assertCaptureSession(session);
    const stream = await navigator.mediaDevices.getUserMedia(constraints);
    startupTrace?.markCapture(attempt, "acquisitionCompleted");
    if (session.controller.signal.aborted || this._captureSession !== session) {
      stream.getTracks().forEach((track) => track.stop());
      this._assertCaptureSession(session);
    }
    session.streams.add(stream);
    return stream;
  }

  observePreparedCapture(prepared, startupTrace) {
    if (!startupTrace || prepared.startupTrace === startupTrace) return;
    // Another accepted request can reuse this capture. Observe it under the
    // new request without replacing its stream, recorder, or opening audio.
    prepared.startupTrace = startupTrace;
    const attempt = startupTrace.beginCapture("prepared");
    startupTrace.markCapture(attempt, "acquisitionCompleted");
    startupTrace.observeCapture(
      prepared.stream,
      attempt,
      this._getCaptureSession().observations.get(prepared.stream)
    );
  }

  // Tells the main process whether this renderer is preparing capture.
  _syncMicOpenGate() {
    const open = this.preparedMicCapture.active;
    if (open === this._micOpenReported) return;
    this._micOpenReported = open;
    window.electronAPI?.micWarmHoldChanged?.(open);
  }

  _disposePrepared(prepared) {
    if (!prepared) return;
    if (this._captureSession?.streams.has(prepared.stream)) {
      this._captureSession.preparationExpired = true;
    }
    if (this.micRecovery.stream === prepared.stream) this.micRecovery.stop();
    this._captureSession?.observations.get(prepared.stream)?.cancel();
    this._captureSession?.observations.delete(prepared.stream);
    disposePreparedCapture(prepared);
    this._markCaptureStreamReleased();
  }

  // Record from the instant the prepared stream delivers frames. If the hold
  // guard confirms a real dictation these chunks become the recording's opening;
  // a cancel discards them without the audio ever leaving the renderer.
  _startPreRollRecorder(prepared) {
    try {
      const recorder = new MediaRecorder(prepared.stream);
      const session = this._captureSession;
      recorder.onerror = () => {
        if (this._captureSession === session) this.failMicrophoneCapture();
      };
      recorder.ondataavailable = (event) => {
        if (event.data && event.data.size > 0) prepared.chunks.push(event.data);
      };
      recorder.start(RECORDING_TIMESLICE_MS);
      prepared.recorder = recorder;
    } catch (e) {
      logger.debug("Pre-roll recorder unavailable", { error: e.message }, "audio");
    }
  }

  _stampMicWarm() {
    this._micWarmedAt = Date.now();
  }

  // Remember a recent open for diagnostics without retaining capture.
  _markCaptureStreamReleased() {
    this._stampMicWarm();
  }

  async _acquireCaptureStream(
    constraints,
    session = this._getCaptureSession(),
    startupTrace = null
  ) {
    const attempt = startupTrace?.beginCapture("device");
    const rawStream = await this._openCaptureStream(constraints, session, startupTrace, attempt);
    const stream = await this.acquireHealthyMicStream(rawStream, constraints, session);
    this._assertCaptureSession(session);
    this._stampMicWarm();
    const observation = observeFirstAudio(stream.getAudioTracks()[0]);
    session.observations.set(stream, observation);
    startupTrace?.observeCapture(stream, attempt, observation);
    return stream;
  }

  async acquireHealthyMicStream(rawStream, _constraints, session = null) {
    const ready = await waitForTrackReady(
      rawStream.getAudioTracks()[0],
      600,
      session?.controller.signal
    );
    if (session) this._assertCaptureSession(session);
    if (!ready) {
      rawStream.getTracks().forEach((track) => track.stop());
      const error = new Error("Selected microphone is unavailable");
      error.name = "MicUnusableError";
      throw error;
    }
    return rawStream;
  }

  async startRecording(startupTrace = null) {
    let prepared = null;
    let preparedAdopted = false;
    let session;
    try {
      if (
        this._startInProgress ||
        this.isRecording ||
        this.isProcessing ||
        this.mediaRecorder?.state === "recording"
      ) {
        return false;
      }

      session = this._getCaptureSession();
      this._startInProgress = true;

      prepared = await this.preparedMicCapture.take();
      this._assertCaptureSession(session);
      if (prepared) this.observePreparedCapture(prepared, startupTrace);
      const constraints =
        prepared?.constraints ?? (await this.getAudioConstraints(this._getCaptureSession()));
      this._assertCaptureSession(session);
      const micStream =
        prepared?.stream ?? (await this._acquireCaptureStream(constraints, session, startupTrace));
      this._assertCaptureSession(session);

      if (prepared) await this.acquireHealthyMicStream(micStream, constraints, session);
      this._assertCaptureSession(session);
      const audioTrack = micStream.getAudioTracks()[0];

      if (audioTrack) {
        const settings = audioTrack.getSettings();
        logger.info(
          "Recording started with microphone",
          {
            label: audioTrack.label,
            deviceId: settings.deviceId?.slice(0, 20) + "...",
            sampleRate: settings.sampleRate,
            channelCount: settings.channelCount,
            muted: audioTrack.muted,
            readyState: audioTrack.readyState,
          },
          "audio"
        );
      }

      try {
        this._silenceCtx = new AudioContext();
        if (this._silenceCtx.state === "suspended") {
          // Not awaited — resume() can hang when the output device is wedged.
          this._silenceCtx.resume().catch(() => {});
        }
        this._silenceAnalyser = this._silenceCtx.createAnalyser();
        this._silenceAnalyser.fftSize = 2048;
        this._silenceSource = this._silenceCtx.createMediaStreamSource(micStream);
        this._silenceSource.connect(this._silenceAnalyser);
        this._localSpeechGateState = createLocalSpeechGateState();
        const dataArray = new Uint8Array(this._silenceAnalyser.fftSize);
        this._silenceInterval = setInterval(() => {
          // A stalled context reads flat silence; recording no windows fails the gate open.
          if (this._silenceCtx?.state !== "running") return;
          this._silenceAnalyser.getByteTimeDomainData(dataArray);
          let sum = 0;
          let peak = 0;
          for (let i = 0; i < dataArray.length; i++) {
            const v = (dataArray[i] - 128) / 128;
            sum += v * v;
            const abs = Math.abs(v);
            if (abs > peak) peak = abs;
          }
          const rms = Math.sqrt(sum / dataArray.length);
          recordLocalSpeechWindow(this._localSpeechGateState, rms, peak);
        }, 100);
      } catch (e) {
        logger.warn("Audio level gate setup failed, skipping", { error: e.message }, "audio");
        this._localSpeechGateState = null;
      }

      this.audioChunks = [];
      this._batchSegments = [];
      this._receivedAudioData = false;
      const preRollUsable =
        prepared?.recorder?.state === "recording" &&
        Date.now() - prepared.startedAt <= PRE_ROLL_MAX_AGE_MS;
      if (!preRollUsable) discardPreRoll(prepared);
      const preRoll = preRollUsable
        ? { recorder: prepared.recorder, chunks: prepared.chunks }
        : null;
      // Pre-roll audio is part of the recording, so the reported duration
      // starts when the prepared stream started — but only when its recorder
      // was adopted; a prepared stream without pre-roll contributes no audio
      // before this point, and back-dating would inflate durationSeconds.
      this.recordingStartTime = preRoll ? prepared.startedAt : Date.now();
      this.createBatchRecorder(micStream, preRoll);
      preparedAdopted = true;
      await waitForFirstAudio(
        session.observations.get(micStream),
        micStream.getAudioTracks()[0],
        session.controller.signal
      );
      this._assertCaptureSession(session);
      this.isRecording = true;
      this.onStateChange?.({
        isRecording: true,
        isProcessing: false,
        micCaptureStatus: "active",
        startupTrace,
      });

      await this.beginMicRecovery(micStream);
      this._assertCaptureSession(session);

      return true;
    } catch (error) {
      // A prepared value the recording never adopted still owns a live stream
      // (and possibly a pre-roll recorder); release it before any retry.
      if (prepared && !preparedAdopted) this._disposePrepared(prepared);
      if (!session || this._captureSession !== session) return false;
      this._endCaptureSession();
      this.mediaRecorder?.stream?.getTracks().forEach((track) => track.stop());
      if (this.mediaRecorder) {
        this.mediaRecorder.onstop = null;
        this.mediaRecorder.ondataavailable = null;
        if (this.mediaRecorder.state !== "inactive") this.mediaRecorder.stop();
        this.mediaRecorder = null;
      }
      this.isRecording = false;
      if (error.name === "FirstAudioUnavailableError") {
        this.onNoAudio?.();
        return false;
      }
      this.onError?.(MICROPHONE_CAPTURE_ERROR);
      return false;
    } finally {
      if (this._captureSession === session) this._startInProgress = false;
    }
  }

  createBatchRecorder(micStream, adoption = null) {
    // Adopting the pre-roll recorder keeps every chunk captured since key-down:
    // rebinding the handlers below transfers ownership with no gap and no
    // re-encode, and the seeded chunks become the recording's opening.
    const recorder = adoption?.recorder ?? new MediaRecorder(micStream);
    const segmentChunks = adoption ? [...adoption.chunks] : [];
    if (segmentChunks.length > 0) {
      this._receivedAudioData = true;
      // The speech-gate analyser only attaches at recording start, so it never
      // measured these frames. Fail the gate open rather than let it discard a
      // short utterance spoken entirely in pre-roll (#845).
      this._localSpeechGateState = null;
    }
    this.mediaRecorder = recorder;
    this.audioChunks = segmentChunks;
    this.recordingMimeType = recorder.mimeType || "audio/webm";

    recorder.onerror = () => {
      if (this.mediaRecorder === recorder) this.failMicrophoneCapture();
    };
    recorder.ondataavailable = (event) => {
      if (event.data && event.data.size > 0) {
        if (this.mediaRecorder === recorder) this._receivedAudioData = true;
        segmentChunks.push(event.data);
      }
    };

    recorder.onstop = async () => {
      if (this.mediaRecorder !== recorder) return;
      // Stop/cancel must distinguish queued final data from an already delivered segment.
      recorder.onstop = null;
      const segment = new Blob(segmentChunks, { type: recorder.mimeType || "audio/webm" });
      segmentChunks.length = 0;
      const rotating = this._rotatingBatchRecorder === recorder;
      // The recorder also stops on its own when its mic track dies (the stream
      // goes inactive). Preserve delivered audio under the existing discard
      // policy and terminate capture instead of opening a replacement mic.
      if (rotating || this._captureSession) {
        if (segment.size > 0) this._batchSegments.push(segment);
        micStream.getTracks().forEach((track) => track.stop());
        this._markCaptureStreamReleased();
        if (rotating) {
          this._rotatingBatchRecorder = null;
          this._rotationResolve?.();
          this._rotationResolve = null;
        } else {
          this.failMicrophoneCapture();
        }
        return;
      }

      micStream.getTracks().forEach((track) => track.stop());
      this._markCaptureStreamReleased();
      await this.finalizeBatchRecording(segment);
    };

    if (!adoption) recorder.start(RECORDING_TIMESLICE_MS);
    return recorder;
  }

  async finalizeBatchRecording(finalSegment) {
    const processingPipeline = this._startProcessingPipeline();
    const wasCancelled = () => this._shouldAbandonProcessingPipeline(processingPipeline);
    this.micRecovery.stop();
    this.teardownSpeechGate();
    this.isRecording = false;
    this.isProcessing = true;
    this.onStateChange?.({
      isRecording: false,
      isProcessing: true,
      micCaptureStatus: "inactive",
    });

    const segments = finalSegment ? [...this._batchSegments, finalSegment] : this._batchSegments;
    this._batchSegments = [];
    const segmentsCount = segments.filter((segment) => segment?.size > 0).length;
    let audioBlob = null;
    let salvagedRecording = false;
    try {
      audioBlob = await this.mergeRecordedSegments(segments);
    } catch (error) {
      if (wasCancelled()) {
        this._settleProcessingPipeline(processingPipeline);
        return;
      }
      logger.error("Failed to assemble recovered recording", { error: error.message }, "audio");
      // Salvage the largest segment rather than dropping the whole recording.
      audioBlob = this.getLargestRecordedSegment(segments);
      salvagedRecording = !!audioBlob;
    }
    if (wasCancelled()) {
      this._settleProcessingPipeline(processingPipeline);
      return;
    }
    audioBlob = audioBlob || new Blob([], { type: this.recordingMimeType || "audio/webm" });
    this.lastAudioBlob = audioBlob;

    logger.info(
      "Recording stopped",
      {
        blobSize: audioBlob.size,
        blobType: audioBlob.type,
        segmentsCount,
      },
      "audio"
    );

    const durationSeconds = this.recordingStartTime
      ? (Date.now() - this.recordingStartTime) / 1000
      : null;
    const analyticsOccurredAt = new Date(this.recordingStartTime || Date.now()).toISOString();
    this.recordingStartTime = null;
    const recordingCheck = evaluateFinishedRecording({
      blobSize: audioBlob.size,
      receivedAudioData: this._receivedAudioData,
    });
    if (!recordingCheck.usable) {
      logger.info(
        "Dropping degenerate recording before transcription",
        {
          blobSize: audioBlob.size,
          reason: recordingCheck.reason,
          receivedAudioData: this._receivedAudioData,
        },
        "audio"
      );
      if (!this._settleProcessingPipeline(processingPipeline)) return;
      this._localSpeechGateState = null;
      this.onTranscriptionComplete?.({ success: true, text: "" });
      return;
    }
    await this.processAudio(
      audioBlob,
      {
        durationSeconds,
        analyticsOccurredAt,
        ...(salvagedRecording ? { salvagedRecording: true } : {}),
      },
      processingPipeline
    );
  }

  async replaceBatchMic(replacement) {
    const session = this._captureSession;
    const recorder = this.mediaRecorder;
    if (!recorder) throw new Error("Batch recorder is no longer active");
    // An inactive recorder can still have final data queued after its track ends.
    if (recorder.onstop) {
      await new Promise((resolve) => {
        this._rotatingBatchRecorder = recorder;
        this._rotationResolve = resolve;
        if (recorder.state !== "inactive") recorder.stop();
      });
    }
    this._assertCaptureSession(session);
    if (!this.isRecording) throw new Error("Recording stopped during microphone recovery");

    this._silenceSource?.disconnect();
    if (this._silenceCtx && this._silenceAnalyser) {
      this._silenceSource = this._silenceCtx.createMediaStreamSource(replacement);
      this._silenceSource.connect(this._silenceAnalyser);
    }
    this.createBatchRecorder(replacement);
  }

  stopRecording() {
    if (this._startInProgress && !this.isRecording) return this.cancelRecording();
    if (
      this.mediaRecorder &&
      (this.mediaRecorder.state === "recording" || (this.isRecording && this.mediaRecorder.onstop))
    ) {
      this._endCaptureSession();
      if (this.mediaRecorder.state === "recording") this.mediaRecorder.stop();
      this.mediaRecorder.stream?.getTracks().forEach((track) => track.stop());
      this.isRecording = false;
      this.isProcessing = true;
      this.onStateChange?.({
        isRecording: false,
        isProcessing: true,
        micCaptureStatus: "inactive",
      });
      return true;
    }
    if (this.isRecording) {
      // The mic died mid-recovery, so no live recorder exists; finalize what
      // was captured instead of leaving the recording unstoppable.
      this._endCaptureSession();
      void this.finalizeBatchRecording(null);
      return true;
    }
    return false;
  }

  teardownSpeechGate() {
    if (this._silenceInterval) {
      clearInterval(this._silenceInterval);
      this._silenceInterval = null;
    }
    this._silenceCtx?.close().catch(() => {});
    this._silenceCtx = null;
    this._silenceAnalyser = null;
    this._silenceSource = null;
    this._levelData = null;
  }

  // Live input level (RMS 0..~1) for the waveform, read from the speech-gate
  // analyser. Null (waveform rests) when no recording is live.
  getRecordingAudioLevel() {
    const ctx = this._silenceCtx;
    const analyser = this._silenceAnalyser;
    if (!ctx || !analyser) return null;
    if (ctx.state === "suspended") {
      // A suspended context reads flat silence — nudge it awake (not awaited;
      // resume() can hang when the output device is wedged).
      ctx.resume().catch(() => {});
      return null;
    }
    if (ctx.state !== "running") return null;
    if (!this._levelData || this._levelData.length !== analyser.fftSize) {
      this._levelData = new Uint8Array(analyser.fftSize);
    }
    analyser.getByteTimeDomainData(this._levelData);
    let sum = 0;
    for (let i = 0; i < this._levelData.length; i++) {
      const v = (this._levelData[i] - 128) / 128;
      sum += v * v;
    }
    return Math.sqrt(sum / this._levelData.length);
  }

  cancelRecording() {
    const hadCapture = !!this._captureSession || this.preparedMicCapture?.active;
    this._endCaptureSession();
    if (
      this.mediaRecorder &&
      (this.mediaRecorder.state === "recording" || (this.isRecording && this.mediaRecorder.onstop))
    ) {
      const recorder = this.mediaRecorder;
      const discarded = this.takeDiscardedBatchSnapshot();
      this.mediaRecorder.onstop = () => {
        recorder.stream?.getTracks().forEach((track) => track.stop());
        this.persistDiscardedBatchRecording(discarded);
      };

      // Detach from manager state before recorder.stop(): its final
      // dataavailable/onstop land async and must not block or observe the
      // next recording.
      this.resetDiscardedBatchRecordingState();

      if (recorder.state !== "inactive") recorder.stop();

      if (recorder.stream) {
        recorder.stream.getTracks().forEach((track) => track.stop());
        this._markCaptureStreamReleased();
      }

      return true;
    }
    if (this.isRecording) {
      // The mic died mid-recovery, so no live recorder exists; discard what was
      // captured instead of leaving the recording uncancelable.
      this.discardBatchRecording();
      return true;
    }
    return !!hadCapture;
  }

  discardBatchRecording() {
    const discarded = this.takeDiscardedBatchSnapshot();
    this.resetDiscardedBatchRecordingState();
    this.persistDiscardedBatchRecording(discarded);
  }

  takeDiscardedBatchSnapshot() {
    return {
      durationSeconds: this.recordingStartTime
        ? (Date.now() - this.recordingStartTime) / 1000
        : null,
      analyticsOccurredAt: new Date(this.recordingStartTime || Date.now()).toISOString(),
      chunks: this.audioChunks,
      segments: this._batchSegments,
      mimeType: this.recordingMimeType,
    };
  }

  resetDiscardedBatchRecordingState() {
    this.teardownSpeechGate();
    this._localSpeechGateState = null;

    this.isRecording = false;
    this.isProcessing = false;
    this.mediaRecorder = null;
    this.audioChunks = [];
    this._batchSegments = [];
    this.recordingStartTime = null;
    this.onStateChange?.({ isRecording: false, isProcessing: false });
  }

  persistDiscardedBatchRecording({
    durationSeconds,
    analyticsOccurredAt,
    chunks,
    segments,
    mimeType,
  }) {
    // This must run after MediaRecorder's final dataavailable event, so decide
    // whether to retain the discarded audio from the snapshot rather than live
    // manager state (which may already belong to a new recording).
    const shouldSave =
      shouldSaveDiscardedRecording(getSettings(), durationSeconds) &&
      (chunks.length > 0 || segments.length > 0);
    if (shouldSave) {
      // Assemble and save in the background — the merge crosses IPC into FFmpeg
      // and must not delay the recorder becoming available again.
      void (async () => {
        try {
          const current = new Blob(chunks, { type: mimeType });
          const blob = await this.mergeRecordedSegments([...segments, current]);
          if (blob)
            await this.saveDiscardedTranscription(blob, durationSeconds, analyticsOccurredAt);
        } catch (error) {
          const fallback = this.getLargestRecordedSegment([
            ...segments,
            new Blob(chunks, { type: mimeType }),
          ]);
          if (fallback) {
            try {
              await this.saveDiscardedTranscription(fallback, durationSeconds, analyticsOccurredAt);
            } catch (fallbackError) {
              logger.warn(
                "Failed to save discarded recording fallback",
                { error: fallbackError.message },
                "audio"
              );
            }
            return;
          }
          logger.warn("Failed to save discarded recording", { error: error.message }, "audio");
        }
      })();
    }
  }

  _startProcessingPipeline() {
    const pipeline = {
      cancellationGeneration: this._processingCancellationGeneration ?? 0,
    };
    this._activeProcessingPipeline = pipeline;
    return pipeline;
  }

  _shouldAbandonProcessingPipeline(pipeline) {
    return (
      pipeline.cancellationGeneration !== (this._processingCancellationGeneration ?? 0) ||
      this._activeProcessingPipeline !== pipeline
    );
  }

  _settleProcessingPipeline(pipeline) {
    if (this._activeProcessingPipeline !== pipeline) return false;
    this._activeProcessingPipeline = null;
    if (this.isProcessing) {
      this.isProcessing = false;
      this.onStateChange?.({ isRecording: false, isProcessing: false });
    }
    return true;
  }

  cancelProcessing() {
    if (this.isProcessing) {
      this._processingCancellationGeneration = (this._processingCancellationGeneration ?? 0) + 1;
      ReasoningService.cancelAllRequests();
      this._activeTranscriptionAbortController?.abort();
      this._activeTranscriptionAbortController = null;
      this.pendingCleanupFailure = null;
      this.isProcessing = false;
      this.onStateChange?.({ isRecording: false, isProcessing: false });
      return true;
    }
    return false;
  }

  async processAudio(audioBlob, metadata = {}, processingPipeline = null) {
    const pipeline = processingPipeline ?? this._startProcessingPipeline();
    if (processingPipeline && this._activeProcessingPipeline !== processingPipeline) return;
    const wasCancelled = () => this._shouldAbandonProcessingPipeline(pipeline);
    const pipelineStart = performance.now();
    let noAudioDetected = false;
    const speechGateDecision = getLocalSpeechGateDecision(this._localSpeechGateState);
    this._localSpeechGateState = null;

    if (speechGateDecision.skip && speechGateDecision.reason === "silence") {
      logger.info(
        "Speech gate skipped transcription",
        {
          reason: speechGateDecision.reason,
          peakRms: speechGateDecision.peakRms?.toFixed(4),
          peakAmplitude: speechGateDecision.peakAmplitude?.toFixed(4),
          speechWindowCount: speechGateDecision.speechWindowCount,
          maxConsecutiveSpeechWindows: speechGateDecision.maxConsecutiveSpeechWindows,
        },
        "audio"
      );
      if (!this._settleProcessingPipeline(pipeline)) return;
      this.onTranscriptionComplete?.({ success: true, text: "" });
      return;
    }

    try {
      const activeModel = this.getTranscriptionModel();
      let result = await this.processWithSelfHostedServer(audioBlob, metadata, wasCancelled);

      if (wasCancelled() || !this.isProcessing) {
        return;
      }

      this.lastAudioMetadata = {
        durationMs: metadata?.durationSeconds
          ? Math.round(metadata.durationSeconds * 1000)
          : Math.round(performance.now() - pipelineStart),
        provider: result?.source || SELF_HOSTED_SOURCE,
        model: activeModel || null,
      };

      result = withSalvageWarning(result, metadata.salvagedRecording);

      result = {
        ...result,
        ...(metadata.analyticsOccurredAt
          ? { analyticsOccurredAt: metadata.analyticsOccurredAt }
          : {}),
        ...this._takePendingResultExtras(),
      };
      this.onTranscriptionComplete?.(result);

      logger.info(
        "Pipeline timing",
        {
          mode: SELF_HOSTED_SOURCE,
          model: activeModel,
          audioDurationMs: metadata.durationSeconds
            ? Math.round(metadata.durationSeconds * 1000)
            : null,
          transcriptionProcessingDurationMs:
            result?.timings?.transcriptionProcessingDurationMs ?? null,
          reasoningProcessingDurationMs: result?.timings?.reasoningProcessingDurationMs ?? null,
          roundTripDurationMs: Math.round(performance.now() - pipelineStart),
          audioSizeBytes: audioBlob.size,
          audioFormat: audioBlob.type,
          outputTextLength: result?.text?.length,
        },
        "performance"
      );
    } catch (error) {
      const errorAtMs = Math.round(performance.now() - pipelineStart);

      if (wasCancelled()) {
        // The user cancelled mid-pipeline; the aborted request's rejection is
        // the expected outcome, not a failure to report or persist.
        logger.info("Transcription cancelled by user", { errorAtMs }, "performance");
        return;
      }

      logger.error(
        "Pipeline failed",
        {
          errorAtMs,
          error: error.message,
        },
        "performance"
      );

      if (error.code === DICTIONARY_ECHO_CODE) {
        // The transcript was discarded as an echo of the dictionary prompt.
        // Surface the shared soft no-audio outcome and keep the recording for
        // a manual retry — otherwise the whole utterance disappears with no
        // feedback (#1547).
        noAudioDetected = true;
        if (this.lastAudioBlob) {
          this.saveFailedTranscription(error.message, error.code, metadata);
        }
      } else if (error.message === "No audio detected") {
        noAudioDetected = true;
      } else {
        this.onError?.({
          title: "Transcription Error",
          description: `Transcription failed: ${error.message}`,
          code: error.code,
          messageKey: error.messageKey,
        });

        // Save failed transcription with audio so the user can retry later
        if (this.lastAudioBlob) {
          this.saveFailedTranscription(error.message, error.code || null, metadata);
        }
      }
    } finally {
      const shouldNotifyNoAudio =
        !wasCancelled() && noAudioDetected && this._activeProcessingPipeline === pipeline;
      this._settleProcessingPipeline(pipeline);
      // Genuine silence is reported through this one post-processing outcome.
      // The pill can now leave thinking before the error surface takes
      // ownership, instead of receiving an IPC event mid-pipeline.
      if (shouldNotifyNoAudio) this.onNoAudio?.();
    }
  }

  async processWithReasoningModel(text, model, agentName, config) {
    logger.logReasoning("CALLING_REASONING_SERVICE", {
      model,
      agentName,
      textLength: text.length,
      hasOverrides: !!config,
    });

    const startTime = Date.now();

    try {
      const result = await ReasoningService.processText(text, model, agentName, config);

      logger.logReasoning("REASONING_SERVICE_COMPLETE", {
        model,
        processingTimeMs: Date.now() - startTime,
        resultLength: result.length,
        success: true,
      });

      return result;
    } catch (error) {
      logger.logReasoning("REASONING_SERVICE_ERROR", {
        model,
        processingTimeMs: Date.now() - startTime,
        error: error.message,
        stack: error.stack,
      });
      throw error;
    }
  }

  // Consume the cleanup failure recorded during reasoning so that exactly one
  // transcription result carries it.
  _takePendingResultExtras() {
    const extras = this.pendingCleanupFailure ? { cleanupFailure: this.pendingCleanupFailure } : {};
    this.pendingCleanupFailure = null;
    return extras;
  }

  async isReasoningAvailable() {
    if (typeof window === "undefined") {
      return false;
    }

    const useReasoning = !!getSettings().useCleanupModel;
    const now = Date.now();
    const cacheValid =
      this.reasoningAvailabilityCache &&
      now < this.reasoningAvailabilityCache.expiresAt &&
      this.cachedReasoningPreference === useReasoning;

    if (cacheValid) {
      return this.reasoningAvailabilityCache.value;
    }

    const cache = (value) => {
      this.reasoningAvailabilityCache = { value, expiresAt: now + REASONING_CACHE_TTL };
      this.cachedReasoningPreference = useReasoning;
      return value;
    };

    if (!useReasoning) return cache(false);

    try {
      const isAvailable = await ReasoningService.isAvailable();
      logger.logReasoning("REASONING_AVAILABILITY", { isAvailable });
      return cache(isAvailable);
    } catch (error) {
      logger.logReasoning("REASONING_AVAILABILITY_ERROR", {
        error: error.message,
        stack: error.stack,
      });
      return cache(false);
    }
  }

  async processTranscription(text, source, wasCancelled = neverCancelled) {
    const result = await this.processTranscriptionCore(text, source, wasCancelled);
    if (wasCancelled()) return result;
    return this.finalizeChineseScript(result);
  }

  async processTranscriptionCore(text, source, wasCancelled = neverCancelled) {
    const normalizedText = typeof text === "string" ? text.trim() : "";

    if (!normalizedText) {
      logger.logReasoning("TRANSCRIPTION_EMPTY_SKIPPING_REASONING", {
        source,
        reason: "Empty text after normalization",
      });
      return normalizedText;
    }
    if (wasCancelled()) return normalizedText;

    logger.logReasoning("TRANSCRIPTION_RECEIVED", {
      source,
      textLength: normalizedText.length,
      textPreview: normalizedText.substring(0, 100) + (normalizedText.length > 100 ? "..." : ""),
      timestamp: new Date().toISOString(),
    });

    const settings = getSettings();
    const cleanupConfig = resolveCleanupConfig(settings);
    if (!cleanupConfig) {
      logger.logReasoning("REASONING_SKIPPED", { reason: "No cleanup model available" });
      return normalizedText;
    }

    const useReasoning = await this.isReasoningAvailable();
    if (wasCancelled() || !useReasoning) return normalizedText;

    const cleanupModel = getEffectiveCleanupModel();
    const agentName =
      typeof window !== "undefined" && window.localStorage ? getAgentName() : "Whisper";

    try {
      logger.logReasoning("SENDING_TO_REASONING", {
        preparedTextLength: normalizedText.length,
        model: cleanupModel,
        disableThinking: cleanupConfig.disableThinking,
      });

      const result = await this.processWithReasoningModel(
        normalizedText,
        cleanupModel,
        agentName,
        cleanupConfig
      );

      logger.logReasoning("REASONING_SUCCESS", {
        resultLength: result.length,
        resultPreview: result.substring(0, 100) + (result.length > 100 ? "..." : ""),
        processingTime: new Date().toISOString(),
      });

      // A blank reply must not wipe the dictation — keep the transcript (#1616).
      return hasTextContent(result) ? result : normalizedText;
    } catch (error) {
      if (wasCancelled()) return normalizedText;
      logger.logReasoning("REASONING_FAILED", {
        error: error.message,
        stack: error.stack,
      });
      logger.warn("Cleanup failed, using raw transcript", { source, error: error.message });
      this.pendingCleanupFailure = cleanupFailureFromError(error);
      return normalizedText;
    }
  }

  async processWithSelfHostedServer(audioBlob, metadata = {}, wasCancelled = neverCancelled) {
    const timings = {};
    let requestController = null;
    const settings = getSettings();
    const language = getBaseLanguageCode(this.getEffectiveSttLanguage(settings));
    const { endpoint, model } = this.resolveTranscriptionTarget(settings);

    try {
      logger.debug(
        "Transcription request starting",
        {
          model,
          blobSize: audioBlob.size,
          blobType: audioBlob.type,
          durationSeconds: metadata.durationSeconds ?? null,
          language,
        },
        "transcription"
      );

      const mimeType = audioBlob.type || "audio/webm";
      const formData = new FormData();
      formData.append("file", audioBlob, `audio.${audioExtensionForMime(mimeType)}`);
      if (model) formData.append("model", model);
      if (language) {
        formData.append("language", language);
      }

      // gpt-transcribe takes the dictionary on its own keywords[] channel (see
      // dictionaryKeywords), so its prompt carries only the Chinese script bias.
      const usesKeywords = usesTranscriptionKeywords(model || "");
      const dictionary = this.getCustomDictionaryPrompt();

      // The cut is a request bound, not a priority rule: Whisper decoders read
      // the tail of whatever they are given (see dictionaryPromptCap).
      const maxPromptChars = dictionaryPromptLimit({ endpoint, model: model || "" });
      const trimmedPrompt = trimDictionaryPrompt(
        this.getWhisperPrompt(settings, usesKeywords ? null : dictionary),
        maxPromptChars
      );
      const dictionaryPrompt = trimmedPrompt.prompt;
      if (dictionaryPrompt) {
        if (trimmedPrompt.truncated) {
          logger.debug(
            "Custom dictionary prompt truncated",
            {
              originalLength: trimmedPrompt.originalLength,
              truncatedLength: dictionaryPrompt.length,
              maxChars: maxPromptChars,
            },
            "transcription"
          );
        }
        formData.append("prompt", dictionaryPrompt);
      }
      if (usesKeywords) {
        for (const keyword of dictionaryKeywords(dictionary)) {
          formData.append("keywords[]", keyword);
        }
      }

      const apiCallStart = performance.now();
      requestController = new AbortController();
      this._activeTranscriptionAbortController = requestController;
      const response = await fetch(endpoint, {
        method: "POST",
        body: formData,
        signal: requestController.signal,
      });

      if (!response.ok) {
        const errorText = await response.text();
        logger.error(
          "Transcription API error response",
          { status: response.status, errorText },
          "transcription"
        );
        const err = new Error(`API Error: ${response.status} ${errorText}`);
        if (response.status === 401) err.code = "INVALID_KEY";
        else if (response.status === 429) {
          err.code = "PROVIDER_RATE_LIMITED";
          err.messageKey = "hooks.audioRecording.errorDescriptions.providerRateLimited";
        } else if (response.status >= 500) err.code = "SERVER_ERROR";
        throw err;
      }

      const rawBody = await response.text();
      let result;
      try {
        result = JSON.parse(rawBody);
      } catch (parseError) {
        logger.error(
          "Failed to parse JSON response",
          { parseError: parseError.message, rawText: rawBody.substring(0, 500) },
          "transcription"
        );
        throw new Error(`Failed to parse API response: ${parseError.message}`);
      }

      if (!hasTextContent(result.text)) {
        logger.info(
          "Transcription returned empty - check audio input",
          { model, endpoint, blobSize: audioBlob.size, blobType: audioBlob.type },
          "transcription"
        );
        throw new Error(
          "No text transcribed - audio may be too short, silent, or in an unsupported format"
        );
      }
      if (this.isDictionaryEcho(result.text)) {
        throw dictionaryEchoError();
      }
      timings.transcriptionProcessingDurationMs = Math.round(performance.now() - apiCallStart);

      const reasoningStart = performance.now();
      const text = await this.processTranscription(result.text, SELF_HOSTED_SOURCE, wasCancelled);
      timings.reasoningProcessingDurationMs = Math.round(performance.now() - reasoningStart);

      const source = (await this.isReasoningAvailable())
        ? `${SELF_HOSTED_SOURCE}-reasoned`
        : SELF_HOSTED_SOURCE;
      return { success: true, text, rawText: result.text, source, timings };
    } finally {
      if (this._activeTranscriptionAbortController === requestController) {
        this._activeTranscriptionAbortController = null;
      }
    }
  }

  getTranscriptionModel() {
    return (getSettings().remoteTranscriptionModel || "").trim() || null;
  }

  resolveTranscriptionTarget(settings) {
    const route = resolveTranscriptionRoute({ settings });
    if (route.transport === "error") {
      throw Object.assign(new Error(route.message), {
        code: route.code,
        messageKey: route.messageKey,
      });
    }
    logger.debug("STT endpoint resolved", { endpoint: route.endpoint }, "transcription");
    return { endpoint: route.endpoint, model: route.model };
  }

  async safePaste(text, options = {}) {
    const { suppressError = false, ...pasteOptions } = options;
    try {
      const result = await window.electronAPI.pasteText(text, pasteOptions);
      return result?.pasted === true;
    } catch (error) {
      const message =
        error?.message ??
        (typeof error?.toString === "function" ? error.toString() : String(error));
      if (!suppressError) {
        this.onError?.({
          title: "Paste Error",
          description: `Failed to paste text. Please check accessibility permissions. ${message}`,
        });
      }
      return false;
    }
  }

  async saveTranscription(
    text,
    rawText = null,
    { clientTranscriptionId, analyticsOccurredAt } = {}
  ) {
    const { dataRetentionEnabled, audioRetentionDays } = getEffectiveRetentionPreferences();
    if (!dataRetentionEnabled) {
      logger.debug("Skipping transcription save — data retention disabled", {}, "audio");
      this.lastAudioBlob = null;
      this.lastAudioMetadata = null;
      return true;
    }

    const eventId = clientTranscriptionId || crypto.randomUUID();
    const occurredAt = analyticsOccurredAt ? new Date(analyticsOccurredAt) : new Date();
    const metadata = this.lastAudioMetadata || {};
    try {
      await window.electronAPI.recordAnalyticsEvent({
        eventId,
        wordCount: countSpokenWords(rawText || text),
        occurredAt: occurredAt.toISOString(),
        localDate: localDateKey(occurredAt),
        spokenDurationMs: metadata.durationMs || null,
        mode: resolveAnalyticsMode(getSettings(), metadata.provider),
        provider: metadata.provider || null,
        model: metadata.model || null,
      });
    } catch (analyticsError) {
      logger.warn(
        "Failed to record local analytics event",
        { error: analyticsError.message },
        "analytics"
      );
    }

    try {
      const result = await window.electronAPI.saveTranscription(text, rawText, {
        clientTranscriptionId: eventId,
        analyticsOccurredAt: occurredAt.toISOString(),
      });

      // Save audio if we have a captured blob and the transcription was saved successfully
      if (result?.id && this.lastAudioBlob) {
        if (audioRetentionDays > 0) {
          try {
            const arrayBuffer = await this.lastAudioBlob.arrayBuffer();
            await window.electronAPI.saveTranscriptionAudio(
              result.id,
              arrayBuffer,
              this.lastAudioMetadata
            );
          } catch (audioErr) {
            // Non-blocking: transcription is saved even if audio save fails
            logger.warn("Failed to save transcription audio", { error: audioErr.message }, "audio");
          }
        }
        this.lastAudioBlob = null;
        this.lastAudioMetadata = null;
      }

      return true;
    } catch (error) {
      return false;
    }
  }

  async saveFailedTranscription(errorMessage, errorCode = null, metadata = {}) {
    const { dataRetentionEnabled, audioRetentionDays } = getEffectiveRetentionPreferences();
    if (!dataRetentionEnabled) {
      logger.debug("Skipping failed transcription save — data retention disabled", {}, "audio");
      this.lastAudioBlob = null;
      this.lastAudioMetadata = null;
      return;
    }

    try {
      const result = await window.electronAPI.saveTranscription("", null, {
        status: "failed",
        errorMessage,
        errorCode,
        ...(metadata?.analyticsOccurredAt
          ? { analyticsOccurredAt: metadata.analyticsOccurredAt }
          : {}),
      });

      if (result?.id && this.lastAudioBlob) {
        if (audioRetentionDays > 0) {
          try {
            const durationMs = metadata?.durationSeconds
              ? Math.round(metadata.durationSeconds * 1000)
              : null;
            const arrayBuffer = await this.lastAudioBlob.arrayBuffer();
            await window.electronAPI.saveTranscriptionAudio(result.id, arrayBuffer, {
              durationMs,
              provider: null,
              model: null,
            });
          } catch (audioErr) {
            logger.warn(
              "Failed to save audio for failed transcription",
              {
                error: audioErr.message,
              },
              "audio"
            );
          }
        }
        this.lastAudioBlob = null;
        this.lastAudioMetadata = null;
      }
    } catch (error) {
      logger.error(
        "Failed to save failed transcription record",
        {
          error: error.message,
        },
        "audio"
      );
    }
  }

  async saveDiscardedTranscription(blob, durationSeconds, analyticsOccurredAt = null) {
    let savedId = null;
    try {
      const result = await window.electronAPI.saveTranscription("", null, {
        status: "discarded",
        ...(analyticsOccurredAt ? { analyticsOccurredAt } : {}),
      });
      if (!result?.id) return;
      savedId = result.id;

      if (blob) {
        const durationMs = durationSeconds ? Math.round(durationSeconds * 1000) : null;
        const arrayBuffer = await blob.arrayBuffer();
        await window.electronAPI.saveTranscriptionAudio(savedId, arrayBuffer, {
          durationMs,
          provider: null,
          model: null,
        });
      }
    } catch (error) {
      logger.error(
        "Failed to save discarded transcription record",
        { error: error.message },
        "audio"
      );
      // A discarded row is only recoverable through its audio; if the audio save
      // failed, drop the dead row instead of leaving an empty unrecoverable entry. See #907.
      if (savedId != null) {
        try {
          await window.electronAPI.deleteTranscription(savedId);
        } catch (cleanupError) {
          logger.warn(
            "Failed to clean up discarded row after audio save failure",
            { error: cleanupError.message },
            "audio"
          );
        }
      }
    }
  }

  getState() {
    return {
      isRecording: this.isRecording,
      isProcessing: this.isProcessing,
      micCaptureStatus: this.micCaptureStatus,
    };
  }

  cleanup() {
    this.cancelRecording();
    this.cancelProcessing();
    if (this.mediaRecorder) {
      this.mediaRecorder.onstop = null;
      this.mediaRecorder.ondataavailable = null;
      this.mediaRecorder = null;
    }
    this._unsubscribeSettings?.();
    this.preparedMicCapture.cancel();
    this.lastAudioBlob = null;
    this.lastAudioMetadata = null;
    this.onStateChange = null;
    this.onError = null;
    this.onTranscriptionComplete = null;
    if (this._onDeviceChange) {
      navigator.mediaDevices?.removeEventListener?.("devicechange", this._onDeviceChange);
    }
    this._unsubscribeLidState?.();
    this._unsubscribeLidState = null;
  }
}

export { resolveCleanupConfig };
export default AudioManager;
