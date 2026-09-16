const { ipcMain, app, shell, BrowserWindow, systemPreferences, net, session } = require("electron");
const path = require("path");
const fs = require("fs");
const os = require("os");
const { isRestorablePasteTarget } = require("./windowsPasteTarget");
const crypto = require("crypto");
const debugLogger = require("./debugLogger");
const { UPLOAD_AUDIO_EXTENSIONS } = require("../constants/uploadAudioFormats.json");
const { providerContentType, prepareProviderUpload } = require("./providerUploadAudio");
const { ANALYTICS_HISTORY_BACKFILL_VERSION } = require("./analytics");

const { isSherpaLocalProvider } = require("./parakeetModelInfo");
const { broadcastToWindows } = require("./windowBroadcast");
const { openExternalUrl } = require("./externalUrlOpener");
const { resolveFailedGpuBackends } = require("./whisper");
const { BYOK_API_KEYS } = require("../config/secretKeys");
const tokenStore = require("./tokenStore");
const accountScopeBinding = require("./accountScopeBinding");

const { withPolicyRequestHeaders } = require("./policyRequestHeaders");

const { createPolicyResponseError } = require("./policyResponseError");

const { resolveSystemDefaultMicrophone } = require("./systemDefaultMicrophone");
// The renderer's ModelRegistry is not main-loadable; the raw registry data is
// packaged, and the route resolver only needs {id, baseUrl} per provider.
const transcriptionProviderBaseUrls = () =>
  require("../models/modelRegistryData.json").transcriptionProviders;
// ipcMain.handle keeps only the message when a promise rejects, dropping custom
// props — proxy handlers return {error, code, messageKey} so the renderer can
// rebuild the error.

// Analytics uploads cross two asynchronous boundaries: renderer -> main and
// main -> cloud. Pin every local queue operation to the same authenticated
// account generation so a delayed pass cannot adopt a replacement session.

// Which diarization dialect a resolved endpoint speaks, for Custom endpoints
// that front a known provider. Null when the host offers no known dialect.

const { resolveLocalServerNeeds } = require("./localServerPolicy");
const autoStart = require("./autoStart");
const { getRelaunchOptions, getRelaunchWaiter } = require("./autoStartPolicy");

const { i18nMain, changeLanguage } = require("./i18nMain");

const { getCortiToken } = require("./cortiAuth");

const { focusWindowsHotkeyCaptureWindow } = require("./hotkeyCaptureFocus");

const { transcribeWithTinfoil } = require("./tinfoilTranscription");
const { transcribeWithGemini } = require("./geminiTranscription");
const AudioStorageManager = require("./audioStorage");
const LocalModelDownloadStatus = require("./localModelDownloadStatus");
const AgentStreamRequestRegistry = require("./agentStreamRequestRegistry");

const liveSpeakerIdentifier = require("./liveSpeakerIdentifier");

const { applySmartSpacing } = require("./smartSpacing");
const { applyAutoLearnSetting } = require("./autoLearnSetting");
const {
  DEFAULT_RETENTION_SETTINGS,
  createRetentionSettingsHandler,
} = require("./retentionSettings");

const {
  applyConfirmedSpeaker,
  applySuggestedSpeaker,
  canAutoRelabelSpeaker,
  isSpeakerLocked,
} = require("./speakerAssignmentPolicy");
const { normalizeStoredSpeakerCount } = require("./speakerCount");

const postMigrationDetector = require("./postMigrationDetector");

const {
  DEFAULT_EXPECTED_SPEAKER_COUNT,
  MAX_SPEAKER_COUNT,
} = require("../constants/speakerDetection.json");

const {
  DEFAULT_WHISPER_VAD_CONFIG,
  sanitizeWhisperVadConfig,
  resolveContextSileroEnabled,
} = require("./whisperVadConfig");

// Meeting capture runs at 24 kHz (see meetingRecordingStore AudioContext); cloud
// streaming providers must be told the true PCM rate or they misread the audio.

// The realtime clients default to a 0.6 server-VAD threshold, raised in #630 to
// keep mic ambient noise from opening turns. The system loopback channel's noise
// floor is digital silence, so it keeps the original, more sensitive threshold —
// at 0.6 quiet remote speech may never trip the VAD and the whole channel
// transcribes to nothing.

const MISTRAL_TRANSCRIPTION_URL = "https://api.mistral.ai/v1/audio/transcriptions";

const XAI_STT_URL = "https://api.x.ai/v1/stt";

// Debounce delay: wait for user to stop typing before processing corrections
const AUTO_LEARN_DEBOUNCE_MS = 1500;

// Route caps vary by provider (Gemini's inline-base64 limit is the lowest), so
// the message reports the cap that actually applied.
const byokSizeCapError = (sizeCapBytes) =>
  `File too large. Maximum size for bring-your-own-key is ${Math.floor(sizeCapBytes / (1024 * 1024))} MB.`;

const CLOUD_INLINE_LIMIT = 4 * 1024 * 1024;
// The enterprise "Test Connection" probe only needs one word back, but the
// Azure Responses API rejects max_output_tokens below 16.

const CLOUD_CHUNK_SEGMENT_SECONDS = 240;

const { createAbortError } = require("./abortError");
const { testProviderConnection } = require("./providerConnectionTest");
const { createUploadCancelRegistry } = require("./uploadCancelRegistry");
const { applyOpenWhisprOriginHeader } = require("./sessionHeaders");
const {
  CLOUD_UPLOAD_TIMEOUT_MS,
  CLOUD_CHUNK_MAX_ATTEMPTS,
  CLOUD_CHUNK_GLOBAL_CONCURRENCY,
  CLOUD_CHUNK_MAX_TEARDOWN_REFUNDS,
  CLOUD_CHUNK_MAX_LOSS_RATIO,
  SILENT_CHUNK,
  FATAL_CHUNK_CODES,
  isTransientChunkError,
  isNetworkLevelFailure,
  isConnectionPoisoningFailure,
  isTeardownCollateral,
  summarizeChunkResults,
  assembleChunkTranscript,
  chunkRetryDelayMs,
  abortableSleep,
  createTeardownGate,
  createUploadSlots,
  withoutChunkAnalytics,
} = require("./cloudChunkPolicy");

// Chunk retries need their own connection pool: recovering a wedged chunk pool
// must not abort an unrelated inline upload that has no collateral retry path.
const CLOUD_CHUNK_UPLOAD_SESSION_PARTITION = "ow-cloud-chunk-uploads";
const CLOUD_INLINE_UPLOAD_SESSION_PARTITION = "ow-cloud-uploads";
const cloudUploadSlots = createUploadSlots(CLOUD_CHUNK_GLOBAL_CONCURRENCY);
const shouldDropUploadPool = createTeardownGate();
const cloudUploadSessions = new Map();

function getCloudUploadSession(partition) {
  if (!cloudUploadSessions.has(partition)) {
    const uploadSession = session.fromPartition(partition);
    applyOpenWhisprOriginHeader(uploadSession);
    cloudUploadSessions.set(partition, uploadSession);
  }
  return cloudUploadSessions.get(partition);
}

function getChunkCloudUploadSession() {
  return getCloudUploadSession(CLOUD_CHUNK_UPLOAD_SESSION_PARTITION);
}

function getInlineCloudUploadSession() {
  return getCloudUploadSession(CLOUD_INLINE_UPLOAD_SESSION_PARTITION);
}

// Counts initiated pool drops so a chunk can tell whether its failure was
// collateral from a teardown that happened while its body was on the wire.
let uploadPoolTeardowns = 0;

async function dropUploadConnections(force = false) {
  if (!shouldDropUploadPool(force)) return;
  uploadPoolTeardowns++;
  try {
    await getChunkCloudUploadSession().closeAllConnections();
  } catch {
    // pool teardown is best-effort
  }
}

const { mergeSpeakersWithText } = require("./speakerMerge");

const { listLocalTranscriptionModels } = require("./localTranscriptionModels");

// Canonicalize allowed dirs so realpath'd inputs match on macOS (/var -> /private/var).
// Deliberately narrow: user-picked paths anywhere else are approved individually via
// approvedAudioPaths, so a compromised renderer can't read arbitrary files.
function getCanonicalAllowedAudioDirs() {
  const os = require("os");
  const { getSafeTempDir } = require("./safeTempDir");
  const dirs = [os.tmpdir(), getSafeTempDir(), app.getPath("userData")];
  return dirs.map((d) => {
    try {
      return fs.realpathSync(d);
    } catch {
      return d;
    }
  });
}

// User-picked paths (OS file dialog, real drag-dropped files) may live outside the
// static dirs (external volumes, /mnt, D:\) and are approved individually.
const approvedAudioPaths = new Set();

function approveAudioPath(filePath) {
  if (typeof filePath !== "string" || !filePath) return;
  try {
    approvedAudioPaths.add(fs.realpathSync(path.resolve(filePath)));
  } catch {
    // File vanished or unreadable; nothing to approve.
  }
}

// Returns the realpath'd file path if it lives under an allowed dir, else null.
function resolveAllowedAudioPath(filePath) {
  const real = fs.realpathSync(path.resolve(filePath));
  if (approvedAudioPaths.has(real)) {
    return real;
  }
  const allowed = getCanonicalAllowedAudioDirs();
  if (allowed.some((dir) => real === dir || real.startsWith(dir + path.sep))) {
    return real;
  }
  return null;
}

function buildMultipartBody(fileBuffer, fileName, contentType, fields = {}) {
  const boundary = `----OpenWhispr${Date.now()}`;
  const parts = [];

  parts.push(
    `--${boundary}\r\n` +
      `Content-Disposition: form-data; name="file"; filename="${fileName}"\r\n` +
      `Content-Type: ${contentType}\r\n\r\n`
  );
  parts.push(fileBuffer);
  parts.push("\r\n");

  for (const [name, value] of Object.entries(fields)) {
    if (value != null) {
      parts.push(
        `--${boundary}\r\n` +
          `Content-Disposition: form-data; name="${name}"\r\n\r\n` +
          `${value}\r\n`
      );
    }
  }

  parts.push(`--${boundary}--\r\n`);

  const bodyParts = parts.map((p) => (typeof p === "string" ? Buffer.from(p) : p));
  return { body: Buffer.concat(bodyParts), boundary };
}

async function postMultipart(
  url,
  body,
  boundary,
  headers = {},
  { signal, session: fetchSession } = {}
) {
  const response = await (fetchSession ?? net).fetch(url.toString(), {
    method: "POST",
    headers: {
      "Content-Type": `multipart/form-data; boundary=${boundary}`,
      ...headers,
    },
    body,
    useSessionCookies: false,
    signal,
  });
  const text = await response.text();
  try {
    return { statusCode: response.status, data: JSON.parse(text) };
  } catch {
    // Vercel platform errors (413 payload cap, 504 timeout) return non-JSON bodies.
    throw Object.assign(new Error(`Server error ${response.status}: ${text.slice(0, 120)}`), {
      code: "SERVER_ERROR",
      statusCode: response.status,
    });
  }
}

function interpretTranscribeResponse(data) {
  if (data.statusCode === 401) {
    throw Object.assign(new Error("Session expired"), { code: "AUTH_EXPIRED" });
  }
  if (data.statusCode === 503) {
    throw Object.assign(new Error("Request timed out"), { code: "SERVER_ERROR" });
  }
  if (data.statusCode === 429) {
    throw Object.assign(new Error("Daily word limit reached"), {
      code: "LIMIT_REACHED",
      ...data.data,
    });
  }
  if (data.statusCode === 422 && data.data?.code === "NO_SPEECH_DETECTED") {
    throw Object.assign(new Error(data.data.error || "No speech detected in audio"), {
      code: "NO_SPEECH_DETECTED",
    });
  }
  if (data.statusCode !== 200) {
    throw createPolicyResponseError(data.statusCode, data.data, `API error: ${data.statusCode}`);
  }
  return data.data;
}

async function chunkedCloudTranscribe({
  buffer = null,
  filePath = null,
  apiUrl,
  policyHeaders,
  multipartFields = {},
  onProgress,
  signal,
  segmentDuration = CLOUD_CHUNK_SEGMENT_SECONDS,
}) {
  const { splitAudioFile } = require("./ffmpegUtils");

  // Aborted by the caller cancelling or by the first fatal chunk error, so a
  // doomed job stops uploading its remaining chunks immediately.
  const jobController = new AbortController();
  const { signal: jobSignal } = jobController;
  const abortJob = () => jobController.abort();
  signal?.addEventListener("abort", abortJob, { once: true });
  if (signal?.aborted) abortJob();

  const jobId = `${Date.now()}-${crypto.randomBytes(4).toString("hex")}`;
  const chunkDir = path.join(os.tmpdir(), `ow-chunks-${jobId}`);
  let tmpInputPath = null;

  let inputPath = filePath;
  if (!inputPath && buffer) {
    tmpInputPath = path.join(os.tmpdir(), `ow-audio-${jobId}.webm`);
    fs.writeFileSync(tmpInputPath, buffer);
    inputPath = tmpInputPath;
  }

  fs.mkdirSync(chunkDir, { recursive: true });

  try {
    onProgress?.({ stage: "splitting", chunksTotal: 0, chunksCompleted: 0 });

    const { chunkPaths, durationSeconds } = await splitAudioFile(inputPath, chunkDir, {
      segmentDuration,
      signal: jobSignal,
    });
    const totalChunks = chunkPaths.length;

    onProgress?.({ stage: "transcribing", chunksTotal: totalChunks, chunksCompleted: 0 });

    const url = new URL(`${apiUrl}/api/transcribe`);
    const results = new Array(totalChunks).fill(null);
    let fatalError = null;
    let completedCount = 0;

    const transcribeChunk = async (index) => {
      let attempt = 1;
      let teardownRefunds = CLOUD_CHUNK_MAX_TEARDOWN_REFUNDS;
      while (true) {
        if (jobSignal.aborted) throw createAbortError();

        // Held only while a body is on the wire, so the backoff below never
        // occupies a slot and queue time never eats the upload timeout.
        const releaseSlot = await cloudUploadSlots.acquire(jobSignal);
        const timeoutSignal = AbortSignal.timeout(CLOUD_UPLOAD_TIMEOUT_MS);
        const teardownsAtStart = uploadPoolTeardowns;
        let failure = null;
        let timedOut = false;
        let collateral = false;
        try {
          try {
            const { body, boundary } = buildMultipartBody(
              fs.readFileSync(chunkPaths[index]),
              path.basename(chunkPaths[index]),
              "audio/mpeg",
              withoutChunkAnalytics(multipartFields)
            );
            const data = await postMultipart(url, body, boundary, policyHeaders, {
              signal: AbortSignal.any([jobSignal, timeoutSignal]),
              session: getChunkCloudUploadSession(),
            });
            results[index] = interpretTranscribeResponse(data);
          } catch (err) {
            failure = err;
            timedOut = timeoutSignal.aborted;
            collateral = isTeardownCollateral(err, {
              timedOut,
              teardownsDuringAttempt: uploadPoolTeardowns - teardownsAtStart,
            });
            // Drop the pool while still holding the slot — released first, a
            // queued sibling is admitted onto the pool microseconds before
            // closeAllConnections() kills it and burns an attempt it never
            // owned. A fatal TLS/protocol alert proves the pool is poisoned,
            // so that drop is forced through the cooldown gate.
            if (!jobSignal.aborted && !collateral) {
              const poisoned = isConnectionPoisoningFailure(err);
              if (poisoned || isNetworkLevelFailure(err, { timedOut })) {
                await dropUploadConnections(poisoned);
              }
            }
          }
        } finally {
          releaseSlot();
        }
        if (!failure) break;

        if (failure.code === "NO_SPEECH_DETECTED") {
          results[index] = SILENT_CHUNK;
          break;
        }

        if (jobSignal.aborted) throw createAbortError();
        if (collateral && teardownRefunds > 0) {
          teardownRefunds--;
          debugLogger.warn(`Chunk ${index} attempt ${attempt} killed by pool teardown, refunded`, {
            error: failure.message,
          });
          await abortableSleep(chunkRetryDelayMs(1), jobSignal);
          continue;
        }
        if (attempt >= CLOUD_CHUNK_MAX_ATTEMPTS || !(timedOut || isTransientChunkError(failure))) {
          throw failure;
        }
        debugLogger.warn(`Chunk ${index} attempt ${attempt} failed, retrying`, {
          error: failure.message,
          timedOut,
        });
        await abortableSleep(chunkRetryDelayMs(attempt), jobSignal);
        attempt++;
      }

      completedCount++;
      onProgress?.({
        stage: "transcribing",
        chunksTotal: totalChunks,
        chunksCompleted: completedCount,
      });
    };

    await Promise.all(
      chunkPaths.map((_, index) =>
        transcribeChunk(index).catch((err) => {
          // Only aborts the job itself caused, reported once below. A chunk's
          // own upload timeout also aborts, and that is a real failure.
          if (jobSignal.aborted && err.name === "AbortError") return;
          if (FATAL_CHUNK_CODES.has(err.code)) {
            fatalError ??= err;
            abortJob();
            return;
          }
          debugLogger.warn(`Chunk ${index} failed`, { error: err.message, code: err.code });
        })
      )
    );

    if (signal?.aborted) {
      throw Object.assign(createAbortError("Upload cancelled"), { code: "UPLOAD_CANCELLED" });
    }
    if (fatalError) throw fatalError;

    const { responses, failedChunks: failed, silentChunks } = summarizeChunkResults(results);
    if (responses.length === 0) {
      if (silentChunks === totalChunks) {
        throw Object.assign(new Error("No speech detected in audio"), {
          code: "NO_SPEECH_DETECTED",
        });
      }
      throw new Error("All chunks failed to transcribe");
    }

    if (failed / totalChunks > CLOUD_CHUNK_MAX_LOSS_RATIO) {
      throw Object.assign(new Error(`${failed} of ${totalChunks} audio segments were lost`), {
        code: "CHUNK_LOSS_EXCEEDED",
      });
    }

    const text = assembleChunkTranscript(results, segmentDuration, durationSeconds);
    return {
      text,
      responses,
      lastResponse: responses[responses.length - 1],
      ...(failed > 0
        ? {
            warning: `${failed} of ${totalChunks} chunks failed`,
            failedChunks: failed,
            totalChunks,
          }
        : {}),
    };
  } finally {
    signal?.removeEventListener("abort", abortJob);
    if (tmpInputPath) {
      try {
        fs.unlinkSync(tmpInputPath);
      } catch {
        // ignore
      }
    }
    try {
      fs.rmSync(chunkDir, { recursive: true, force: true });
    } catch (cleanupErr) {
      debugLogger.warn("Failed to cleanup chunk dir", { error: cleanupErr.message });
    }
  }
}

// Cleanup toast wording for replies the main-process providers reject; mirror
// TRUNCATED_/EMPTY_OUTPUT_MESSAGE_KEY in services/ai/chatRequestBody.ts (#2091).
const CLEANUP_TRUNCATED_MESSAGE_KEY = "hooks.audioRecording.errorDescriptions.cleanupTruncated";

class IPCHandlers {
  constructor(managers) {
    this.environmentManager = managers.environmentManager;
    this.databaseManager = managers.databaseManager;
    this.clipboardManager = managers.clipboardManager;
    this.whisperManager = managers.whisperManager;
    this.parakeetManager = managers.parakeetManager;
    this.diarizationManager = managers.diarizationManager;
    this.windowManager = managers.windowManager;
    this.windowsKeyManager = managers.windowsKeyManager;
    this.linuxKeyManager = managers.linuxKeyManager;
    this.textEditMonitor = managers.textEditMonitor;
    this.selectionManager = managers.selectionManager;
    this.getTrayManager = managers.getTrayManager;
    this.whisperCudaManager = managers.whisperCudaManager;
    this.whisperVulkanManager = managers.whisperVulkanManager;
    this.googleCalendarManager = managers.googleCalendarManager;
    this.microsoftCalendarManager = managers.microsoftCalendarManager;
    this.appleCalendarManager = managers.appleCalendarManager;
    this.meetingDetectionEngine = managers.meetingDetectionEngine;
    this.audioTapManager = managers.audioTapManager;
    this.linuxPortalAudioManager = managers.linuxPortalAudioManager;
    this.windowsLoopbackAudioManager = managers.windowsLoopbackAudioManager;
    this.meetingAecManager = managers.meetingAecManager;
    this.getQdrantManager = managers.getQdrantManager;
    this.oauthProtocolRegistered = managers.oauthProtocolRegistered === true;
    this.oauthProtocol = managers.oauthProtocol || "openwhispr";
    this.sessionId = crypto.randomUUID();
    // requestId -> AbortControllers for in-flight audio-upload work (cloud
    // upload, or local transcription + diarization sharing one id), so a
    // cancel can abort the exact job.
    this._uploadCancelRegistry = createUploadCancelRegistry();
    this._agentStreamRequests = new AgentStreamRequestRegistry();
    this._cloudReasonRequests = new AgentStreamRequestRegistry();
    this._cloudTranscriptionRequests = new AgentStreamRequestRegistry();
    this._enterpriseReasoningRequests = new AgentStreamRequestRegistry();
    // webContents id -> its release listener, for renderers holding the mic open.
    this._micHoldSenders = new Map();
    this.assemblyAiStreaming = null;
    this.deepgramStreaming = null;
    this.geminiStreaming = null;
    this.cortiStreaming = null;
    this._dictationStreaming = null;
    this._dictationConnectPromise = null;
    this._dictationIdleTimer = null;
    this._dictationPreviewEnabled = false;
    this._meetingMicStreaming = null;
    this._meetingSystemStreaming = null;
    this._hotkeyCaptureMode = false;
    this._autoLearnEnabled = true; // Default on, synced from renderer
    this._autoLearnDebounceTimer = null;
    this._autoLearnLatestData = null;
    this._textEditHandler = null;
    this._activeRecordingPipeline = null;
    this._onboardingDemoSession = null;
    this.audioStorageManager = new AudioStorageManager();
    this.localModelDownloadStatus = new LocalModelDownloadStatus();
    this._retentionCleanupInterval = null;
    this._retentionSettings = { ...DEFAULT_RETENTION_SETTINGS }; // Synced from renderer
    this._retentionSettingsSynced = false;
    this._noteFilesEnabled = false;
    this._granolaImportPending = null;
    this._analyticsHistoryBackfillPromise = null;
    this.speakerDiarizationEnabled = true;
    this.activeMeetingSpeakerConfig = null;
    this.whisperVadSettings = {
      dictationSileroEnabled: false,
      noteRecordingSileroEnabled: true,
      meetingSileroEnabled: true,
      ...DEFAULT_WHISPER_VAD_CONFIG,
    };
    this._setupTextEditMonitor();
    this._setupRetentionCleanup();
    // Warm the OS default mic answer before the first hotkey press (~2s on Windows).
    resolveSystemDefaultMicrophone();
    this.setupHandlers();
    // Lives for the app's lifetime; IPCHandlers has no teardown path.
    tokenStore.subscribe(({ generation, token }) => {
      this.enterpriseIdentityManager?.clear();
      if (!token) {
        this.databaseManager.setActiveAccountId(null);
        accountScopeBinding.clear();
        broadcastToWindows("active-account-scope-changed", null);
      }
      broadcastToWindows("auth-token-state-changed", {
        generation,
        hasToken: Boolean(token),
      });
    });

    if (this.whisperManager?.serverManager) {
      // Remember the failed backend so it isn't re-attempted (and its model
      // reload re-paid) on every launch; cleared by retry, re-download, delete.
      this.whisperManager.serverManager.on("cuda-fallback", () => {
        this._recordWhisperGpuFailure("cuda");
        broadcastToWindows("cuda-fallback-notification", {});
      });
      this.whisperManager.serverManager.on("gpu-fallback", () => {
        this._recordWhisperGpuFailure("vulkan");
        broadcastToWindows("gpu-fallback-notification", {});
      });
      // Persist the discrete-GPU pin so later launches spawn pinned directly
      // instead of paying a second Vulkan cold start. See #1606.
      this.whisperManager.serverManager.on("vulkan-device-pinned", ({ index }) => {
        this._syncStartupEnv({ WHISPER_VULKAN_DEVICE: String(index) });
      });
      this.whisperManager.serverManager.on("vulkan-device-pin-cleared", () => {
        this._syncStartupEnv({}, ["WHISPER_VULKAN_DEVICE"]);
      });
    }
  }

  // Reconstructing counters from the transcripts already on disk records exactly
  // what "keep local history" turns off, so it answers to the same switch the
  // live path checks in audioManager.saveTranscription. The main process boots
  // with defaults rather than the user's choice, so an unsynced setting is not
  // consent either -- the renderer's first sync is what starts this (#1370).
  _canReconstructAnalyticsHistory() {
    return this._retentionSettingsSynced && this._retentionSettings.dataRetentionEnabled;
  }

  /** Whether a signed-in account is bound to this install. */
  _hasActiveAccountScope() {
    return Boolean(accountScopeBinding.read());
  }

  // The switch alone is not enough to start: a managed workspace can force local
  // history off, and that policy arrives over the network while this scan takes
  // milliseconds, so the renderer reports the permissive personal default until
  // it lands. Waiting for the real answer is only possible where there is one --
  // signed out the policy store stays idle forever and the user's own preference
  // is the only authority there is. Mid-scan arrival needs no separate check:
  // a policy that resolves "always_off" flips the switch, which the loop reads.
  _mayStartAnalyticsHistoryReconstruction() {
    if (!this._canReconstructAnalyticsHistory()) return false;
    if (this._retentionSettings.localHistoryPolicyResolved === true) return true;
    return !this._hasActiveAccountScope();
  }

  // Reconciliation is best-effort. Analytics reads await it so later-eligible
  // history shows up before the numbers are read, which means a failure here
  // must never fail the read itself: a broken scan would otherwise blank an
  // Insights summary that SQLite could have answered perfectly well.
  async _ensureAnalyticsHistoryBackfilled() {
    if (!this._mayStartAnalyticsHistoryReconstruction()) return { inserted: 0, scanned: 0 };
    if (this._analyticsHistoryBackfillPromise) return this._analyticsHistoryBackfillPromise;
    // The failure is absorbed inside this promise rather than around the
    // creator's await, because callers that join an in-flight pass are handed
    // this promise directly and would otherwise receive the raw rejection --
    // which is every analytics read that arrives while the startup pass is
    // still scanning.
    const backfillPromise = (async () => {
      let inserted = 0;
      let scanned = 0;
      let skipped = 0;
      let stoppedEarly = false;
      const state = this.databaseManager.getAnalyticsHistoryBackfillState(
        ANALYTICS_HISTORY_BACKFILL_VERSION
      );
      if (state.scannedThroughId >= state.targetId) return { inserted, scanned };
      while (true) {
        // The database reads its persisted cursor again for every batch. An
        // older row made eligible while this pass yields can move that cursor
        // backward without being overwritten by stale in-memory progress.
        const batch = this.databaseManager.backfillAnalyticsHistoryBatch({
          throughId: state.targetId,
          checkpointVersion: ANALYTICS_HISTORY_BACKFILL_VERSION,
        });
        inserted += batch.inserted;
        scanned += batch.scanned;
        skipped += batch.skipped;
        if (batch.complete) break;
        await new Promise((resolve) => setImmediate(resolve));
        // The switch can be turned off while this pass yields -- by the user, or
        // by a managed policy that resolved after the renderer's first sync sent
        // the personal default. Re-reading it here stops the scan at the next
        // batch boundary instead of mining the rest of a history the user has
        // just opted out of.
        if (!this._canReconstructAnalyticsHistory()) {
          stoppedEarly = true;
          break;
        }
      }
      if (inserted > 0) broadcastToWindows("analytics-changed");
      if (scanned > 0) {
        debugLogger.info(
          stoppedEarly
            ? "Analytics history backfill stopped: local history was turned off mid-scan"
            : "Analytics history backfill complete",
          { inserted, skipped, scanned },
          "analytics"
        );
      }
      return { inserted, scanned };
    })().catch((error) => {
      debugLogger.error("Analytics history backfill failed", { error: error.message }, "analytics");
      return { inserted: 0, scanned: 0 };
    });
    this._analyticsHistoryBackfillPromise = backfillPromise;
    // Cleared after the assignment above, never inside the pass: a scan that
    // finishes without ever awaiting would otherwise strand its own resolved
    // promise here and every later read would join a pass that already ended.
    void backfillPromise.then(() => {
      if (this._analyticsHistoryBackfillPromise === backfillPromise) {
        this._analyticsHistoryBackfillPromise = null;
      }
    });
    return backfillPromise;
  }

  // The dictation slot reports its own changes from the renderer. Slots
  // registered through IPC have to announce theirs here so macOS can re-derive
  // which keys the native Globe listener owns.
  _notifyHotkeyChanged(hotkey) {
    ipcMain.emit("hotkey-changed", null, hotkey);
  }

  _releaseMicHold(sender) {
    const release = this._micHoldSenders.get(sender.id);
    if (!release) return;
    this._micHoldSenders.delete(sender.id);
    sender.off("destroyed", release);
    sender.off("did-finish-load", release);
    this.meetingDetectionEngine?.setMicWarmHold(this._micHoldSenders.size > 0);
  }

  _getWhisperVadSettings() {
    const current = this.whisperVadSettings || {};
    return {
      dictationSileroEnabled: current.dictationSileroEnabled === true,
      noteRecordingSileroEnabled: current.noteRecordingSileroEnabled !== false,
      meetingSileroEnabled: current.meetingSileroEnabled !== false,
      ...sanitizeWhisperVadConfig(current),
    };
  }

  _updateNativeModelDownloadStatus(modelType, modelId, progressData) {
    if (progressData.type === "complete") {
      return this.localModelDownloadStatus.finish(modelType, modelId);
    }

    if (progressData.type === "installing") {
      return this.localModelDownloadStatus.update(modelType, modelId, {
        phase: "installing",
        progress: progressData.percentage || 100,
      });
    }

    return this.localModelDownloadStatus.update(modelType, modelId, {
      phase: "downloading",
      progress: progressData.percentage || 0,
      downloadedBytes: progressData.downloaded_bytes || 0,
      totalBytes: progressData.total_bytes || 0,
    });
  }

  _setWhisperVadSettings(update = {}) {
    const ALLOWED_KEYS = new Set([
      "dictationSileroEnabled",
      "noteRecordingSileroEnabled",
      "meetingSileroEnabled",
      ...Object.keys(require("../constants/whisperVad.json").DEFAULTS),
    ]);
    const filtered = {};
    for (const [k, v] of Object.entries(update)) {
      if (ALLOWED_KEYS.has(k)) filtered[k] = v;
    }
    this.whisperVadSettings = { ...this._getWhisperVadSettings(), ...filtered };
    return this._getWhisperVadSettings();
  }

  // Shared by the upload IPC handler and the CLI bridge. `filePath` must
  // already have passed resolveAllowedAudioPath (or approveAudioPath).
  async transcribeLocalFile(filePath, options = {}) {
    const audioBuffer = fs.readFileSync(filePath);
    if (isSherpaLocalProvider(options.provider)) {
      return this.parakeetManager.transcribeLocalParakeet(audioBuffer, options);
    }
    return this.whisperManager.transcribeLocalWhisper(audioBuffer, {
      ...options,
      ...this._resolveWhisperVadOptions("noteRecording"),
    });
  }

  approveAudioPath(filePath) {
    approveAudioPath(filePath);
  }

  listLocalTranscriptionModels() {
    return listLocalTranscriptionModels({
      whisperManager: this.whisperManager,
      parakeetManager: this.parakeetManager,
    });
  }

  _resolveWhisperVadOptions(context) {
    const settings = this._getWhisperVadSettings();
    const {
      dictationSileroEnabled,
      noteRecordingSileroEnabled,
      meetingSileroEnabled,
      ...vadConfig
    } = settings;
    return {
      vadEnabled: resolveContextSileroEnabled(settings, context),
      vadConfig,
    };
  }

  _asyncVectorUpsert(note) {
    setImmediate(() => {
      const vectorIndex = require("./vectorIndex");
      if (!vectorIndex.isReady()) return;
      const { LocalEmbeddings } = require("./localEmbeddings");
      const text = LocalEmbeddings.noteEmbedText(note.title, note.content, note.enhanced_content);
      vectorIndex
        .upsertNote(note.id, text, { space_id: note.space_id, folder_id: note.folder_id ?? null })
        .catch(() => {});
    });
  }

  _asyncVectorDelete(noteId) {
    setImmediate(() => {
      const vectorIndex = require("./vectorIndex");
      if (!vectorIndex.isReady()) return;
      vectorIndex.deleteNote(noteId).catch(() => {});
    });
  }

  // Space vector purges are persisted (pending_vector_purges) so a purge that
  // lands while Qdrant is booting or down is retried once the index is ready.
  drainPendingVectorPurges() {
    setImmediate(() => {
      void (async () => {
        const vectorIndex = require("./vectorIndex");
        if (!vectorIndex.isReady()) return;
        for (const { space_id } of this.databaseManager.getPendingVectorPurges()) {
          if (await vectorIndex.deleteBySpace(space_id)) {
            this.databaseManager.clearPendingVectorPurge(space_id);
          }
        }
      })().catch((error) => {
        debugLogger.error(
          "Pending vector purge drain failed",
          { error: error?.message || String(error) },
          "semantic-search"
        );
      });
    });
  }

  _mirrorDeleteFolderIfUnshared(folderName) {
    if (!this._noteFilesEnabled) return;
    // Folder names are only unique per space — a live same-named folder in
    // another space shares the mirror directory, so leave it on disk.
    const stillLive = this.databaseManager.db
      .prepare("SELECT 1 FROM folders WHERE name = ? AND deleted_at IS NULL")
      .get(folderName);
    if (stillLive) return;
    const markdownMirror = require("./markdownMirror");
    markdownMirror.deleteFolder(folderName);
  }

  _asyncMirrorWrite(note) {
    if (!this._noteFilesEnabled) {
      debugLogger.debug(
        "Mirror write skipped: note files disabled",
        { noteId: note.id },
        "note-files"
      );
      return;
    }
    setImmediate(() => {
      const markdownMirror = require("./markdownMirror");
      const folderName = this._getFolderName(note.folder_id);
      markdownMirror.writeNote(note, folderName);
      if (note.transcript) {
        markdownMirror.writeTranscript(note, folderName, this._buildSpeakerMappings(note.id));
      }
    });
  }

  _asyncMirrorDelete(noteId) {
    if (!this._noteFilesEnabled) {
      debugLogger.debug("Mirror delete skipped: note files disabled", { noteId }, "note-files");
      return;
    }
    setImmediate(() => {
      const markdownMirror = require("./markdownMirror");
      markdownMirror.deleteNote(noteId);
    });
  }

  _buildFolderMap() {
    const folders = this.databaseManager.getFolders();
    const map = {};
    for (const f of folders) {
      map[f.id] = f.name;
    }
    return map;
  }

  _buildSpeakerMappings(noteId) {
    const arr = this.databaseManager.getSpeakerMappings(noteId);
    const map = {};
    for (const m of arr) {
      map[m.speaker_id] = m.display_name;
    }
    return map;
  }

  _parseNonSelfParticipants(participantsJson) {
    if (!participantsJson) return [];
    let participants;
    try {
      participants = JSON.parse(participantsJson);
    } catch (_) {
      return [];
    }
    if (!Array.isArray(participants) || participants.length === 0) return [];
    const googleEmails = new Set(
      this.databaseManager.getGoogleAccounts().map((a) => a.email.toLowerCase())
    );
    return participants.filter(
      (p) => p && p.self !== true && !googleEmails.has((p.email || "").toLowerCase())
    );
  }

  _getNoteNonSelfParticipants(noteId) {
    if (!noteId) return [];
    try {
      const note = this.databaseManager.getNote(noteId);
      return this._parseNonSelfParticipants(note?.participants);
    } catch (_) {
      return [];
    }
  }

  _resolveOneOnOneOtherParticipant(participantsJson) {
    const others = this._parseNonSelfParticipants(participantsJson);
    if (others.length !== 1) return null;
    const displayName = others[0].displayName || others[0].email;
    if (!displayName) return null;
    const email = (others[0].email || "").toLowerCase().trim() || null;
    return { displayName, email };
  }

  _noteExpectedSpeakerCountOrNull(note) {
    const stored = normalizeStoredSpeakerCount(note?.expected_speaker_count);
    if (stored != null) {
      return stored;
    }
    const others = this._parseNonSelfParticipants(note?.participants).length;
    if (others > 0) {
      return Math.min(others + 1, MAX_SPEAKER_COUNT);
    }
    return null;
  }

  _resolveNoteExpectedSpeakerCount(note) {
    return this._noteExpectedSpeakerCountOrNull(note) ?? DEFAULT_EXPECTED_SPEAKER_COUNT;
  }

  _resolveInitialMeetingSpeakerConfig(noteId) {
    let note = null;
    if (noteId != null) {
      try {
        note = this.databaseManager.getNote(noteId);
      } catch (_) {
        note = null;
      }
    }
    const enabled =
      (note?.diarization_enabled == null
        ? this.speakerDiarizationEnabled
        : note.diarization_enabled !== 0) !== false;
    return { enabled, expectedCount: this._resolveNoteExpectedSpeakerCount(note) };
  }

  // Participants added mid-meeting must raise the speaker cap that was derived
  // from the note at recording start. A count the user set via the stepper
  // (explicit) is never overridden.
  //
  // Raise-only: lowering the cap below the clusters already discovered would make
  // _assignOrForceCluster fold every later voice onto an existing speaker — the
  // exact identity collapse this refresh exists to prevent. A roster that shrinks
  // (or empties) mid-meeting therefore leaves the cap where it is.
  _refreshMeetingSpeakerConfigFromNote(noteId, note) {
    const config = this.activeMeetingSpeakerConfig;
    if (!config || config.explicit) return;
    if (noteId == null || this._activeMeetingNoteId !== noteId) return;

    const expectedCount = this._noteExpectedSpeakerCountOrNull(note);
    if (expectedCount == null || expectedCount <= config.expectedCount) return;

    this.activeMeetingSpeakerConfig = { ...config, expectedCount };
    liveSpeakerIdentifier.setMaxSpeakers(Math.max(1, expectedCount - 1));
    broadcastToWindows("meeting-session-speaker-config-updated", {
      enabled: config.enabled,
      expectedCount,
    });
    debugLogger.info(
      "Meeting speaker config refreshed from participants",
      { noteId, expectedCount },
      "speaker"
    );
  }

  _rebuildMirror(basePath) {
    const markdownMirror = require("./markdownMirror");
    if (basePath) markdownMirror.init(basePath);
    const notes = this.databaseManager.getNotes(null, 99999);
    const speakerMappingsMap = {};
    for (const note of notes) {
      if (note.transcript) {
        speakerMappingsMap[note.id] = this._buildSpeakerMappings(note.id);
      }
    }
    markdownMirror.rebuildAll(notes, this._buildFolderMap(), speakerMappingsMap);
  }

  _getFolderName(folderId) {
    if (!folderId) return "Personal";
    const folder = this.databaseManager.db
      .prepare("SELECT name FROM folders WHERE id = ?")
      .get(folderId);
    return folder?.name || "Personal";
  }

  _getDictionarySafe() {
    try {
      return this.databaseManager.getDictionary();
    } catch {
      return [];
    }
  }

  _cleanupTextEditMonitor() {
    if (this._autoLearnDebounceTimer) {
      clearTimeout(this._autoLearnDebounceTimer);
      this._autoLearnDebounceTimer = null;
    }
    this._autoLearnLatestData = null;
    if (this.textEditMonitor && this._textEditHandler) {
      this.textEditMonitor.removeListener("text-edited", this._textEditHandler);
      this._textEditHandler = null;
    }
  }

  async _logDetectedGpus() {
    const { listNvidiaGpus } = require("../utils/gpuDetection");
    const gpus = await listNvidiaGpus();
    if (gpus.length > 0) {
      debugLogger.info(
        "NVIDIA GPUs detected",
        {
          count: gpus.length,
          devices: gpus.map((g) => `[${g.index}] ${g.name} (${g.vramMb}MB) ${g.uuid}`),
        },
        "gpu"
      );
    } else {
      debugLogger.debug("No NVIDIA GPUs detected", {}, "gpu");
    }
  }

  _setupRetentionCleanup() {
    const SIX_HOURS_MS = 6 * 60 * 60 * 1000;
    // No sweep at startup: _retentionSettings still holds the 30-day default
    // until the renderer syncs, so sweeping here deletes audio a user set to
    // keep for 60/90 days or forever (#1370). The first sync runs the first
    // sweep instead.
    this._retentionCleanupInterval = setInterval(() => {
      if (this._retentionSettingsSynced) this._runRetentionCleanup();
    }, SIX_HOURS_MS);
  }

  _runRetentionCleanup() {
    const { audioRetentionDays, transcriptRetentionDays } = this._retentionSettings;
    try {
      if (transcriptRetentionDays > 0) {
        const { ids, analyticsPurged } =
          this.databaseManager.deleteTranscriptionsExpiredBefore(transcriptRetentionDays);
        for (const id of ids) {
          this.audioStorageManager.deleteAudio(id);
          broadcastToWindows("transcription-deleted", { id });
        }
        if (analyticsPurged > 0) broadcastToWindows("analytics-changed");
      }
      if (audioRetentionDays > 0) {
        this.audioStorageManager.cleanupExpiredAudio(audioRetentionDays, this.databaseManager);
      }
    } catch (error) {
      debugLogger.error("Retention cleanup failed", { error: error.message }, "audio-storage");
    }
  }

  _setupTextEditMonitor() {
    if (!this.textEditMonitor) return;

    this._textEditHandler = (data) => {
      if (
        !data ||
        typeof data.originalText !== "string" ||
        typeof data.newFieldValue !== "string"
      ) {
        debugLogger.debug("[AutoLearn] Invalid event payload, skipping");
        return;
      }

      const { originalText, newFieldValue } = data;

      debugLogger.debug("[AutoLearn] text-edited event", {
        originalPreview: originalText.substring(0, 80),
        newValuePreview: newFieldValue.substring(0, 80),
      });

      this._autoLearnLatestData = { originalText, newFieldValue };

      if (this._autoLearnDebounceTimer) {
        clearTimeout(this._autoLearnDebounceTimer);
      }

      this._autoLearnDebounceTimer = setTimeout(() => {
        this._processCorrections();
      }, AUTO_LEARN_DEBOUNCE_MS);
    };

    this.textEditMonitor.on("text-edited", this._textEditHandler);
  }

  _processCorrections() {
    this._autoLearnDebounceTimer = null;
    if (!this._autoLearnLatestData) return;
    if (!this._autoLearnEnabled) {
      debugLogger.debug("[AutoLearn] Disabled, skipping correction processing");
      this._autoLearnLatestData = null;
      return;
    }

    const { originalText, newFieldValue } = this._autoLearnLatestData;
    this._autoLearnLatestData = null;

    try {
      const { extractCorrections } = require("../utils/correctionLearner");
      const currentDict = this._getDictionarySafe();
      const corrections = extractCorrections(originalText, newFieldValue, currentDict);
      debugLogger.debug("[AutoLearn] Corrections result", {
        corrections,
        dictSize: currentDict.length,
      });

      if (corrections.length > 0) {
        const saveResult = this.databaseManager.applyDictionaryChanges(
          { add: corrections },
          "learned"
        );

        if (saveResult?.success === false) {
          debugLogger.debug("[AutoLearn] Failed to save dictionary", { error: saveResult.error });
          return;
        }

        // Broadcast the post-save normalized list, not the raw input (which
        // still has case-variant dupes), so renderers don't flash ghost rows.
        broadcastToWindows("dictionary-updated", this.databaseManager.getDictionary());

        // Show the overlay so the toast is visible (it may have been hidden after dictation)
        this.windowManager.showDictationPanel();
        broadcastToWindows("corrections-learned", corrections);
        debugLogger.debug("[AutoLearn] Saved corrections", { corrections });
      }
    } catch (error) {
      debugLogger.debug("[AutoLearn] Error processing corrections", { error: error.message });
    }
  }

  _whisperGpuFailedBackends() {
    return resolveFailedGpuBackends(process.env.WHISPER_GPU_FAILED);
  }

  _recordWhisperGpuFailure(backend) {
    const failed = this._whisperGpuFailedBackends();
    if (!failed.includes(backend)) failed.push(backend);
    this._syncStartupEnv({ WHISPER_GPU_FAILED: failed.join(",") });
  }

  _clearWhisperGpuFailure(backend) {
    const failed = this._whisperGpuFailedBackends().filter((b) => b !== backend);
    if (failed.length > 0) {
      this._syncStartupEnv({ WHISPER_GPU_FAILED: failed.join(",") });
    } else {
      this._syncStartupEnv({}, ["WHISPER_GPU_FAILED"]);
    }
  }

  // Captured before a handler stops the server to touch pack files (stopServer
  // clears currentServerModel); tells _applyWhisperGpuPreference what to reload.
  _whisperReloadModel() {
    return this.whisperManager.serverManager.isRemote
      ? null
      : this.whisperManager.currentServerModel;
  }

  // Apply a GPU pack change to the loaded server without blocking the caller's
  // IPC reply (a Vulkan cold start can take minutes); the renderer follows
  // progress by polling whisper-server-status. Returns whether a reload was
  // kicked off so the UI shows "activating" only when one is coming.
  _applyWhisperGpuPreference(modelName) {
    this.whisperManager.restartServerWithGpuPreference(modelName).catch((err) => {
      debugLogger.error("whisper-server GPU preference restart failed", { error: err.message });
    });
    return !!modelName;
  }

  _syncStartupEnv(setVars, clearVars = []) {
    let changed = false;
    for (const [key, value] of Object.entries(setVars)) {
      if (process.env[key] !== value) {
        process.env[key] = value;
        changed = true;
      }
    }
    for (const key of clearVars) {
      if (process.env[key]) {
        delete process.env[key];
        changed = true;
      }
    }
    if (changed) {
      debugLogger.debug("Synced startup env vars", {
        set: Object.keys(setVars),
        cleared: clearVars.filter((k) => !process.env[k]),
      });
      // A swallowed .env write failure here left GPU enablement flags silently
      // out of sync with the packs on disk (#1340) — log which keys were lost.
      this.environmentManager.saveAllKeysToEnvFile().catch((err) => {
        debugLogger.error("Failed to persist startup env vars to .env", {
          set: Object.keys(setVars),
          clearRequested: clearVars,
          error: err.message,
        });
      });
    }
  }

  // Mints a Corti access token from stored BYOK credentials. Shared by the
  // dictation streaming handlers and the meeting realtime-token resolver.
  async _mintStoredCortiToken(options = {}) {
    const clientId = this.environmentManager.getCortiClientId();
    const clientSecret = this.environmentManager.getCortiClientSecret();
    if (!clientId || !clientSecret) {
      const err = new Error("No Corti credentials configured. Add them in Settings.");
      err.code = "NO_API";
      throw err;
    }
    const environment = options.environment || "us";
    const tenant = (options.tenant || "").trim() || "base";
    const token = await getCortiToken({ environment, tenant, clientId, clientSecret });
    return { token, environment, tenant };
  }

  setupHandlers() {
    ipcMain.handle("set-assistant-panel-open", (event, open) => {
      const dictationWindow = this.windowManager?.mainWindow;
      if (
        !dictationWindow ||
        dictationWindow.isDestroyed() ||
        event.sender !== dictationWindow.webContents
      ) {
        return { success: false, error: "Not the dictation window" };
      }
      this.windowManager.setAssistantPanelOpen(open);
      return { success: true };
    });
    ipcMain.handle("set-assistant-panel-busy", (event, busy) => {
      const dictationWindow = this.windowManager?.mainWindow;
      if (
        !dictationWindow ||
        dictationWindow.isDestroyed() ||
        event.sender !== dictationWindow.webContents
      ) {
        return { success: false, error: "Not the dictation window" };
      }
      this.windowManager.setAssistantPanelBusy(busy);
      return { success: true };
    });
    ipcMain.handle("onboarding-set-window-mode", (_event, mode) =>
      this.windowManager.setOnboardingWindowMode(mode)
    );

    ipcMain.handle("onboarding-set-active", (_event, active) => {
      if (typeof active !== "boolean") return false;
      return this.windowManager.setOnboardingActive(active);
    });

    ipcMain.handle("test-provider-connection", async (_event, config) => {
      if (config?.provider === "corti" && config?.scope === "transcription") {
        try {
          const clientId = String(config.clientId || "").trim();
          const clientSecret = String(config.clientSecret || "").trim();
          if (clientId && clientSecret) {
            await getCortiToken({
              environment: config.environment || "us",
              tenant: String(config.tenant || "").trim() || "base",
              clientId,
              clientSecret,
            });
          } else {
            await this._mintStoredCortiToken({
              environment: config.environment,
              tenant: config.tenant,
            });
          }
          return { success: true };
        } catch (error) {
          // errorCode is the machine-readable field the renderer maps to i18n;
          // the English string stays for logs/back-compat. Only a response
          // Corti actually sent counts as a rejection (getCortiToken prefixes
          // those); a fetch that never reached it is a network problem, and
          // reporting it as "credentials rejected" sends the user to re-type a
          // key that was never the issue.
          if (error?.name === "AbortError") {
            return {
              success: false,
              errorCode: "timeout",
              error: "The connection test timed out.",
            };
          }
          if (!/^(Corti authentication failed|Invalid Corti)/.test(error?.message || "")) {
            return {
              success: false,
              errorCode: "network",
              error: "The provider could not be reached.",
            };
          }
          return {
            success: false,
            errorCode: "credentialsRejected",
            error: "Corti rejected these credentials.",
          };
        }
      }
      return testProviderConnection(config);
    });

    ipcMain.handle("window-minimize", () => {
      if (this.windowManager.controlPanelWindow) {
        this.windowManager.controlPanelWindow.minimize();
      }
    });

    ipcMain.handle("window-maximize", () => {
      if (this.windowManager.controlPanelWindow) {
        if (this.windowManager.controlPanelWindow.isMaximized()) {
          this.windowManager.controlPanelWindow.unmaximize();
        } else {
          this.windowManager.controlPanelWindow.maximize();
        }
      }
    });

    ipcMain.handle("window-close", () => {
      if (this.windowManager.controlPanelWindow) {
        this.windowManager.controlPanelWindow.close();
      }
    });

    ipcMain.handle("window-is-maximized", () => {
      if (this.windowManager.controlPanelWindow) {
        return this.windowManager.controlPanelWindow.isMaximized();
      }
      return false;
    });

    ipcMain.handle("hide-window", () => {
      this.windowManager.hideDictationPanel();
    });

    ipcMain.handle("show-dictation-panel", () => {
      this.windowManager.showDictationPanel({ reposition: true });
    });

    ipcMain.handle("capture-dictation-target", async () => {
      const pid = (await this.textEditMonitor?.captureTargetPid?.()) ?? null;
      await this.selectionManager?.captureTarget?.();
      return { success: true, pid };
    });

    ipcMain.handle("force-stop-dictation", () => {
      if (this.windowManager?.forceStopMacCompoundPush) {
        this.windowManager.forceStopMacCompoundPush("manual");
      }
      return { success: true };
    });

    ipcMain.handle("set-main-window-interactivity", (event, shouldCapture) => {
      this.windowManager.setMainWindowInteractivity(Boolean(shouldCapture));
      return { success: true };
    });

    ipcMain.handle("set-main-window-input-region", (event, region) => {
      if (event.sender !== this.windowManager.mainWindow?.webContents) return null;
      return this.windowManager.setMainWindowInputRegion(region);
    });

    ipcMain.handle("get-main-window-horizontal-direction", () => {
      return this.windowManager.getMainWindowHorizontalDirection();
    });

    ipcMain.handle("resize-main-window", (event, sizeKey) => {
      return this.windowManager.resizeMainWindow(sizeKey);
    });

    ipcMain.handle("resize-dictation-error-window-to-content", (event, surfaceHeight) => {
      return this.windowManager.resizeDictationErrorWindowToContent(surfaceHeight);
    });

    for (const k of BYOK_API_KEYS) {
      ipcMain.handle(`get-${k.base}-key`, () => this.environmentManager[k.get]());
      ipcMain.handle(`save-${k.base}-key`, (event, key) => this.environmentManager[k.save](key));
    }

    ipcMain.handle("db-save-transcription", async (event, text, rawText, options) => {
      const result = this.databaseManager.saveTranscription(text, rawText, options);
      if (result?.success && result?.transcription) {
        setImmediate(() => {
          broadcastToWindows("transcription-added", result.transcription);
        });
      }
      return result;
    });

    ipcMain.handle("db-get-transcriptions", async (event, limit = 50, options = {}) => {
      return this.databaseManager.getTranscriptions(limit, options);
    });

    ipcMain.handle("analytics-record-event", async (_event, input) => {
      // The renderer only warns when this write fails, then saves the
      // transcription as completed anyway -- leaving a row the backfill is
      // the only thing that will ever reconcile.
      const result = this.databaseManager.recordAnalyticsEvent(input);
      // Dictation and the control panel are separate renderers, so the
      // Insights view can only learn about a new event through the main process.
      if (result?.success && !result.ignored) {
        setImmediate(() => {
          broadcastToWindows("analytics-changed");
        });
      }
      return result;
    });

    ipcMain.handle("analytics-get-summary", async () => {
      await this._ensureAnalyticsHistoryBackfilled();
      return this.databaseManager.getAnalyticsSummary();
    });

    ipcMain.handle("db-clear-transcriptions", async (event) => {
      this.audioStorageManager.deleteAllAudio();
      const result = this.databaseManager.clearTranscriptions();
      if (result?.success) {
        setImmediate(() => {
          broadcastToWindows("transcriptions-cleared", {
            cleared: result.cleared,
          });
          broadcastToWindows("analytics-changed");
        });
      }
      return result;
    });

    ipcMain.handle("db-delete-transcription", async (event, id) => {
      return this.deleteTranscriptionInternal(id);
    });

    // Audio storage handlers
    ipcMain.handle("save-transcription-audio", async (event, id, audioBuffer, metadata) => {
      const transcription = this.databaseManager.getTranscriptionById(id);
      const timestamp = transcription?.timestamp || null;
      const result = this.audioStorageManager.saveAudio(id, Buffer.from(audioBuffer), timestamp);
      if (result.success) {
        this.databaseManager.updateTranscriptionAudio(id, {
          hasAudio: 1,
          audioDurationMs: metadata?.durationMs || null,
          provider: metadata?.provider || null,
          model: metadata?.model || null,
        });
        const updated = this.databaseManager.getTranscriptionById(id);
        if (updated) broadcastToWindows("transcription-updated", updated);
      }
      return result;
    });

    ipcMain.handle("get-audio-path", async (event, id) => {
      return this.audioStorageManager.getAudioPath(id);
    });

    ipcMain.handle("show-audio-in-folder", async (event, id) => {
      const filePath = this.audioStorageManager.getAudioPath(id);
      if (!filePath) return { success: false };
      shell.showItemInFolder(filePath);
      return { success: true };
    });

    ipcMain.handle("get-audio-buffer", async (event, id) => {
      const buffer = this.audioStorageManager.getAudioBuffer(id);
      return buffer ? buffer.buffer : null;
    });

    ipcMain.handle("delete-transcription-audio", async (event, id) => {
      const result = this.audioStorageManager.deleteAudio(id);
      if (result.success) {
        this.databaseManager.updateTranscriptionAudio(id, {
          hasAudio: 0,
          audioDurationMs: null,
          provider: null,
          model: null,
        });
      }
      return result;
    });

    ipcMain.handle("get-audio-storage-usage", async () => {
      return this.audioStorageManager.getStorageUsage();
    });

    ipcMain.on(
      "retention-settings-changed",
      createRetentionSettingsHandler({
        getCurrentSettings: () => this._retentionSettings,
        getOwner: () => this.windowManager.mainWindow?.webContents,
        hasSynced: () => this._retentionSettingsSynced,
        onSettingsChanged: (settings) => {
          this._retentionSettings = settings;
          this._retentionSettingsSynced = true;
          this._runRetentionCleanup();
        },
      })
    );

    ipcMain.handle("delete-all-audio", async () => {
      const result = this.audioStorageManager.deleteAllAudio();
      try {
        const rows = this.databaseManager.db
          .prepare("SELECT id FROM transcriptions WHERE has_audio = 1")
          .all();
        if (rows.length > 0) {
          this.databaseManager.clearAudioFlags(rows.map((r) => r.id));
        }
      } catch (error) {
        debugLogger.error(
          "Failed to clear audio flags after delete-all",
          { error: error.message },
          "audio-storage"
        );
      }
      return result;
    });

    ipcMain.handle("get-transcription-by-id", async (event, id) => {
      return this.databaseManager.getTranscriptionById(id);
    });

    // Every window's AudioManager can hold the mic open outside a recording, so
    // gate the audio-evidence meeting detector until they all release. A
    // renderer that reloads or goes away releases implicitly — otherwise a
    // crash mid-hold would gate detection for the rest of the session.
    ipcMain.on("mic-warm-hold-changed", (event, active) => {
      if (!active) {
        this._releaseMicHold(event.sender);
        return;
      }
      if (this._micHoldSenders.has(event.sender.id)) return;
      const release = () => this._releaseMicHold(event.sender);
      this._micHoldSenders.set(event.sender.id, release);
      event.sender.on("destroyed", release);
      event.sender.on("did-finish-load", release);
      this.meetingDetectionEngine?.setMicWarmHold(true);
    });

    // Hotkey handlers run in main, while AudioManager owns the real lifecycle
    // in the dictation renderer. Only confirmed renderer state may change the
    // main-process recording gate; raw key presses are merely requests and can
    // be declined while a transcript is still being finalized.
    ipcMain.on("dictation-lifecycle-state-changed", (event, state, inputKind) => {
      const dictationWindow = this.windowManager.mainWindow;
      if (
        !dictationWindow ||
        dictationWindow.isDestroyed() ||
        event.sender !== dictationWindow.webContents
      ) {
        return;
      }
      this.windowManager.setDictationLifecycleState(state, inputKind);
    });

    ipcMain.on("dictation-audio-level-changed", (event, level) => {
      const dictationWindow = this.windowManager.mainWindow;
      if (
        !dictationWindow ||
        dictationWindow.isDestroyed() ||
        event.sender !== dictationWindow.webContents
      ) {
        return;
      }
      this.windowManager.setDictationAudioLevel(level);
    });

    // Dictionary handlers
    ipcMain.on("auto-learn-changed", (_event, enabled) => {
      // Both renderer windows re-sync this on mount — ignore same-value updates (#1080).
      const { changed, enabled: next } = applyAutoLearnSetting(this._autoLearnEnabled, enabled);
      if (!changed) return;
      this._autoLearnEnabled = next;
      if (!this._autoLearnEnabled) {
        if (this._autoLearnDebounceTimer) {
          clearTimeout(this._autoLearnDebounceTimer);
          this._autoLearnDebounceTimer = null;
        }
        this._autoLearnLatestData = null;
      }
      debugLogger.debug("[AutoLearn] Setting changed", { enabled: this._autoLearnEnabled });
    });

    ipcMain.handle("db-get-dictionary", async () => {
      return this.databaseManager.getDictionary();
    });

    ipcMain.handle("db-set-dictionary", async (event, words) => {
      if (!Array.isArray(words)) {
        throw new Error("words must be an array");
      }
      return this.databaseManager.setDictionary(words);
    });

    ipcMain.handle("db-apply-dictionary-changes", async (_event, changes) => {
      const { add, remove } = changes ?? {};
      if (add !== undefined && !Array.isArray(add)) {
        throw new Error("add must be an array");
      }
      if (remove !== undefined && !Array.isArray(remove)) {
        throw new Error("remove must be an array");
      }
      return this.databaseManager.applyDictionaryChanges({ add, remove });
    });

    ipcMain.handle("db-get-snippets", async () => {
      return this.databaseManager.getSnippets();
    });

    ipcMain.handle("db-set-snippets", async (_event, snippets) => {
      if (!Array.isArray(snippets)) {
        throw new Error("snippets must be an array");
      }
      return this.databaseManager.setSnippets(snippets);
    });

    ipcMain.handle("undo-learned-corrections", async (_event, words) => {
      try {
        if (!Array.isArray(words) || words.length === 0) {
          return { success: false };
        }
        const validWords = words.filter((w) => typeof w === "string" && w.trim().length > 0);
        if (validWords.length === 0) {
          return { success: false };
        }
        const saveResult = this.databaseManager.applyDictionaryChanges({ remove: validWords });
        if (saveResult?.success === false) {
          debugLogger.debug("[AutoLearn] Undo failed to save dictionary", {
            error: saveResult.error,
          });
          return { success: false };
        }
        broadcastToWindows("dictionary-updated", this.databaseManager.getDictionary());
        debugLogger.debug("[AutoLearn] Undo: removed words", { words: validWords });
        return { success: true };
      } catch (err) {
        debugLogger.debug("[AutoLearn] Undo failed", { error: err.message });
        return { success: false };
      }
    });

    ipcMain.handle("export-transcript", async (event, noteId, format) => {
      try {
        const note = this.databaseManager.getNote(noteId);
        if (!note) return { success: false, error: "Note not found" };

        const segments = JSON.parse(note.transcript || "[]");
        if (!segments.length) return { success: false, error: "No transcript available" };

        const speakerMappings = this._buildSpeakerMappings(noteId);

        const { dialog } = require("electron");
        const fs = require("fs");
        const extMap = { srt: "srt", json: "json", md: "md" };
        const ext = extMap[format] || "txt";
        const safeName = (note.title || "Untitled").replace(/[/\\?%*:|"<>]/g, "-");

        const result = await dialog.showSaveDialog({
          defaultPath: `${safeName}.${ext}`,
          filters: [
            { name: "Text", extensions: ["txt"] },
            { name: "SubRip Subtitles", extensions: ["srt"] },
            { name: "JSON", extensions: ["json"] },
            { name: "Markdown", extensions: ["md"] },
          ],
        });

        if (result.canceled || !result.filePath) return { success: false };

        const transcriptFormatter = require("./transcriptFormatter");
        let exportContent;
        if (format === "txt") {
          exportContent = transcriptFormatter.formatTxt(note, segments, speakerMappings);
        } else if (format === "srt") {
          exportContent = transcriptFormatter.formatSrt(segments, speakerMappings, note);
        } else if (format === "md") {
          exportContent = transcriptFormatter.formatMd(note, segments, speakerMappings);
        } else {
          exportContent = transcriptFormatter.formatJson(note, segments, speakerMappings);
        }

        fs.writeFileSync(result.filePath, exportContent, "utf-8");
        return { success: true };
      } catch (error) {
        debugLogger.error("Error exporting transcript", { error: error.message }, "notes");
        return { success: false, error: error.message };
      }
    });

    ipcMain.handle("export-dictionary", async (event, words) => {
      try {
        const { dialog } = require("electron");
        const fs = require("fs");

        const result = await dialog.showSaveDialog({
          defaultPath: "dictionary.txt",
          filters: [{ name: "Text", extensions: ["txt"] }],
        });

        if (result.canceled || !result.filePath) return { success: false };

        fs.writeFileSync(result.filePath, words.join("\n"), "utf-8");
        return { success: true };
      } catch (error) {
        debugLogger.error("Error exporting dictionary", { error: error.message }, "dictionary");
        return { success: false, error: error.message };
      }
    });

    ipcMain.handle("select-audio-file", async (event, options = {}) => {
      const { dialog } = require("electron");
      const properties = ["openFile"];
      if (options.multiple === true) properties.push("multiSelections");
      const result = await dialog.showOpenDialog({
        properties,
        filters: [{ name: "Audio and Video Files", extensions: UPLOAD_AUDIO_EXTENSIONS }],
      });
      if (result.canceled || !result.filePaths.length) {
        return { canceled: true };
      }
      result.filePaths.forEach(approveAudioPath);
      if (options.multiple === true) {
        return { canceled: false, filePaths: result.filePaths };
      }
      return { canceled: false, filePath: result.filePaths[0] };
    });

    ipcMain.handle("cancel-upload-transcription", async (_event, requestId) => {
      return { success: this._uploadCancelRegistry.cancel(requestId) > 0 };
    });

    ipcMain.handle(
      "transcribe-audio-file-byok",
      async (event, { filePath, language, remoteTranscriptionUrl, remoteTranscriptionModel }) => {
        const fs = require("fs");
        let cleanupUpload = null;
        try {
          if (typeof filePath !== "string") {
            return { success: false, error: "Invalid file path" };
          }
          const sourcePath = resolveAllowedAudioPath(filePath);
          if (!sourcePath) return { success: false, error: "File path not allowed" };

          const { resolveTranscriptionRoute } = await import("./transcriptionRoute.ts");
          const route = resolveTranscriptionRoute({
            settings: {
              transcriptionMode: "self-hosted",
              remoteTranscriptionUrl,
              remoteTranscriptionModel,
            },
            providers: transcriptionProviderBaseUrls(),
            request: { effectiveLanguage: language || undefined },
          });

          // Fail closed: a misconfigured route must never fall through to a default.
          if (route.transport === "error") {
            return {
              success: false,
              error: route.message,
              code: route.code,
              messageKey: route.messageKey,
            };
          }

          const upload = await prepareProviderUpload(sourcePath);
          cleanupUpload = upload.cleanup;
          const realByok = upload.path;

          // User's own server, so the 25 MB third-party cap does not apply.
          const { body, boundary } = buildMultipartBody(
            fs.readFileSync(realByok),
            path.basename(realByok),
            providerContentType(realByok),
            { model: route.model, language: route.language }
          );
          const data = await postMultipart(new URL(route.endpoint), body, boundary);
          if (data.statusCode !== 200) {
            throw new Error(
              data.data?.error?.message ||
                data.data?.error ||
                `Self-hosted API Error: ${data.statusCode}`
            );
          }
          return { success: true, text: data.data.text };
        } catch (error) {
          debugLogger.error("BYOK audio file transcription error", { error: error.message });
          return {
            success: false,
            error: error.message,
            code: error.code,
            messageKey: error.messageKey,
          };
        } finally {
          cleanupUpload?.();
        }
      }
    );

    // Fired by the preload's getPathForFile for real drag-dropped files; a
    // renderer-constructed File yields "" there, so this can't be forged.
    ipcMain.on("approve-audio-path", (_event, filePath) => {
      approveAudioPath(filePath);
    });

    ipcMain.handle("get-file-size", async (_event, filePath) => {
      const fs = require("fs");
      try {
        if (typeof filePath !== "string") return 0;
        const real = resolveAllowedAudioPath(filePath);
        if (!real) return 0;
        const stats = fs.statSync(real);
        return stats.size;
      } catch {
        return 0;
      }
    });

    ipcMain.handle("delete-temp-file", async (event, filePath) => {
      try {
        if (typeof filePath !== "string") {
          return { success: false, error: "Invalid file path" };
        }
        const { getSafeTempDir } = require("./safeTempDir");
        const resolved = path.resolve(filePath);
        const basename = path.basename(resolved);
        if (!basename.startsWith("ow-url-") && !basename.startsWith("ow-diarize-")) {
          return { success: false, error: "Not an OpenWhispr temp file" };
        }
        const real = fs.realpathSync(resolved);
        let tempDir = getSafeTempDir();
        try {
          tempDir = fs.realpathSync(tempDir);
        } catch {}
        const rel = path.relative(tempDir, real);
        if (rel.startsWith("..") || path.isAbsolute(rel)) {
          return { success: false, error: "Not an OpenWhispr temp file" };
        }
        fs.unlinkSync(real);
        return { success: true };
      } catch (error) {
        debugLogger.warn("Failed to delete temp file", { error: error.message });
        return { success: false, error: error.message };
      }
    });

    ipcMain.handle("capture-selected-text", async (event, options = {}) => {
      if (!this.selectionManager) {
        return { status: "unavailable", code: "selection_manager_unavailable" };
      }
      return this.selectionManager.captureSelectedText({
        probeEditable: options.probeEditable === true,
      });
    });

    ipcMain.handle("replace-selected-text", async (event, sessionId, text, options = {}) => {
      if (!this.selectionManager) {
        return { success: false, code: "selection_manager_unavailable" };
      }
      return this.selectionManager.replaceSelectedText(sessionId, text, {
        restoreClipboard: options.restoreClipboard !== false,
        allowClipboardFallback: options.allowClipboardFallback === true,
        webContents: event.sender,
      });
    });

    ipcMain.handle("paste-at-captured-target", async (event, sessionId, text, options = {}) => {
      if (!this.selectionManager) {
        return { success: false, code: "selection_manager_unavailable" };
      }
      return this.selectionManager.pasteAtCapturedTarget(sessionId, text, {
        restoreClipboard: options.restoreClipboard !== false,
        allowClipboardFallback: options.allowClipboardFallback === true,
        webContents: event.sender,
      });
    });

    ipcMain.handle("paste-text", async (event, text, options) => {
      // An onboarding demo already puts the transcript in its own textarea from
      // the demo event, and that textarea is what has focus — pasting on top of
      // it appends the same sentence a second time. This is a successful no-op,
      // not a completed paste, so callers can avoid reporting paste-dependent
      // fallbacks as if text reached another application.
      if (this.windowManager?.isOnboardingDemoActive()) {
        return { success: true, pasted: false };
      }

      const mainWindow = this.windowManager?.mainWindow;
      const targetPid = this.textEditMonitor?.lastTargetPid || null;

      // Activating the target by PID is more reliable than hide()'s implicit
      // focus hand-off for Chromium apps like Claude desktop and Brave (#668).
      let activated = false;
      if (process.platform === "darwin" && this.textEditMonitor) {
        activated = await this.textEditMonitor.activateTargetPid();
      }

      if (!activated && mainWindow && !mainWindow.isDestroyed() && mainWindow.isFocused()) {
        if (process.platform === "darwin") {
          mainWindow.hide();
          await new Promise((resolve) => setTimeout(resolve, 120));
          mainWindow.showInactive();
        } else {
          mainWindow.blur();
          await new Promise((resolve) => setTimeout(resolve, 80));
        }
      }

      // Smart spacing (#856): append a trailing space so the next paste's leading
      // space self-corrects the gap. Reading the char before the cursor to space
      // the front instead would take a macOS Accessibility read costing hundreds
      // of ms — too slow for the paste hot path.
      const textToPaste = applySmartSpacing(text);

      // Windows: restore the foreground window captured at record start so the
      // paste lands in the field the user was dictating into, not wherever focus
      // drifted during transcription (#859). macOS handles this via
      // activateTargetPid above; Linux re-detects the target inside pasteLinux.
      const winTarget =
        process.platform === "win32"
          ? ((await this.selectionManager?.getWinTarget?.()) ?? null)
          : null;
      const targetWindow =
        winTarget &&
        isRestorablePasteTarget({
          target: winTarget,
          ownExeName: path.basename(process.execPath),
          ownWindowHandles: BrowserWindow.getAllWindows()
            .filter((win) => !win.isDestroyed())
            .map((win) => win.getNativeWindowHandle()),
        })
          ? winTarget.id
          : null;

      const pasteResult = await this.clipboardManager.pasteText(textToPaste, {
        ...options,
        webContents: event.sender,
        targetWindow,
      });
      const pasted = pasteResult?.pasted !== false;
      debugLogger.debug("[AutoLearn] Paste completed", {
        autoLearnEnabled: this._autoLearnEnabled,
        hasMonitor: !!this.textEditMonitor,
        targetPid,
        pasted,
      });
      if (pasted && this.textEditMonitor && this._autoLearnEnabled) {
        setTimeout(() => {
          try {
            debugLogger.debug("[AutoLearn] Starting monitoring", {
              textPreview: text.substring(0, 80),
            });
            this.textEditMonitor.startMonitoring(text, 30000, { targetPid });
          } catch (err) {
            debugLogger.debug("[AutoLearn] Failed to start monitoring", { error: err.message });
          }
        }, 500);
      }
      // ClipboardManager returns `restoreComplete` so main-process callers can
      // serialize subsequent clipboard work behind its delayed restore. A
      // Promise cannot cross Electron's IPC boundary, though, and renderer
      // callers need to know whether text was pasted, but not the delayed
      // clipboard restoration promise. Successful platform paths predate the
      // explicit `pasted` outcome; only the clipboard-only fallback sets false.
      return { success: true, pasted };
    });

    ipcMain.handle("check-accessibility-permission", async (_event, silent = false) => {
      return this.clipboardManager.checkAccessibilityPermissions(silent);
    });

    // Passes `true` to isTrustedAccessibilityClient to trigger the macOS system prompt
    ipcMain.handle("prompt-accessibility-permission", async () => {
      if (process.platform !== "darwin") return true;
      return systemPreferences.isTrustedAccessibilityClient(true);
    });

    ipcMain.handle("read-clipboard", async (event) => {
      return this.clipboardManager.readClipboard();
    });

    ipcMain.handle("write-clipboard", async (event, text) => {
      return this.clipboardManager.writeClipboard(text, event.sender);
    });

    ipcMain.handle("check-paste-tools", async () => {
      return this.clipboardManager.checkPasteTools();
    });

    ipcMain.handle("check-ffmpeg-availability", async (event) => {
      return this.whisperManager.checkFFmpegAvailability();
    });

    // Under `npm run dev` the Vite server dies with Electron, so a relaunched dev
    // instance would have no renderer: just quit there.
    ipcMain.handle("relaunch-app", async () => {
      if (process.env.NODE_ENV === "development") return app.quit();
      const { launcherPath, args } = getRelaunchOptions({
        argv: process.argv,
        protocol: this.oauthProtocol,
        appImagePath: process.env.APPIMAGE,
        portableExecutablePath: process.env.PORTABLE_EXECUTABLE_FILE,
      });
      if (launcherPath) {
        const waiter = getRelaunchWaiter({
          platform: process.platform,
          launcherPath,
          args,
          pid: process.pid,
          ppid: process.ppid,
          systemRoot: process.env.SystemRoot,
        });
        require("child_process")
          .spawn(waiter.file, waiter.args, {
            detached: true,
            stdio: "ignore",
            cwd: path.dirname(launcherPath), // never inside the directory being removed
          })
          .unref();
      } else {
        app.relaunch({ args });
      }
      app.quit();
    });

    ipcMain.handle("update-hotkey", async (event, hotkey) => {
      return await this.windowManager.updateHotkey(hotkey);
    });

    ipcMain.handle("set-hotkey-listening-mode", async (event, enabled) => {
      if (enabled) {
        const captureWindow = BrowserWindow.fromWebContents(event.sender);
        // Only the control panel owns editable hotkey fields. Refocusing the
        // sender before the idempotence check also repairs a stale capture-mode
        // flag after Windows has moved foreground focus to another window.
        if (captureWindow === this.windowManager.controlPanelWindow) {
          focusWindowsHotkeyCaptureWindow(captureWindow);
        }
      }
      if (this._hotkeyCaptureMode === enabled) return { success: true, skipped: true };
      this._hotkeyCaptureMode = enabled;
      this.windowManager.setHotkeyListeningMode(enabled);
      ipcMain.emit("hotkey-listening-mode-changed", null, enabled);
      const hotkeyManager = this.windowManager.hotkeyManager;

      // Restore from slot state only. A freshly captured hotkey is registered by
      // its own update IPC (invoked before this one); re-binding it here would
      // overwrite the primary on DE backends or leak untracked registrations.
      const effectiveHotkey = hotkeyManager.getCurrentHotkey();

      const {
        isGlobeLikeHotkey,
        isModifierOnlyHotkey,
        isRightSideModifier,
        isMouseButtonHotkey,
      } = require("./hotkeyManager");
      const usesNativeListener = (hotkey) =>
        !hotkey ||
        isGlobeLikeHotkey(hotkey) ||
        isMouseButtonHotkey(hotkey) ||
        isModifierOnlyHotkey(hotkey) ||
        isRightSideModifier(hotkey);

      if (enabled) {
        // Entering capture mode — unregister ALL slots so none intercept keypresses.
        // Dictation is always active; meeting and agent may or may not be set.
        const allSlots = hotkeyManager.slots;
        for (const [slot, info] of allSlots) {
          // Native-listener entries (null accelerator) are handled by stopping
          // the key listeners below.
          for (const accel of info?.accelerators || []) {
            if (!accel) continue;
            debugLogger.log(
              `[IPC] Unregistering globalShortcut "${accel}" (slot "${slot}") for capture mode`
            );
            const { globalShortcut } = require("electron");
            try {
              globalShortcut.unregister(accel);
            } catch {}
          }
        }

        // On Windows, stop the Windows key listener
        if (process.platform === "win32" && this.windowsKeyManager) {
          debugLogger.log("[IPC] Stopping Windows key listener for hotkey capture mode");
          this.windowsKeyManager.stop();
        }

        // On Linux, stop the Linux key listener
        if (process.platform === "linux" && this.linuxKeyManager) {
          debugLogger.log("[IPC] Stopping Linux key listener for hotkey capture mode");
          this.linuxKeyManager.stop();
        }

        // On GNOME, unregister all native keybindings during capture
        if (hotkeyManager.isUsingGnome() && hotkeyManager.gnomeManager) {
          await hotkeyManager.gnomeManager.unregisterPushToTalk();
          for (const slot of [...hotkeyManager.gnomeManager.registeredSlots]) {
            debugLogger.log(
              `[IPC] Unregistering GNOME keybinding (slot "${slot}") for capture mode`
            );
            await hotkeyManager.gnomeManager.unregisterKeybinding(slot).catch((err) => {
              debugLogger.warn(`[IPC] Failed to unregister GNOME slot "${slot}":`, err.message);
            });
          }
        }

        // On Hyprland Wayland, unregister the keybinding during capture
        if (hotkeyManager.isUsingHyprland() && hotkeyManager.hyprlandManager) {
          debugLogger.log("[IPC] Unregistering Hyprland keybinding for hotkey capture mode");
          const unregistered = await hotkeyManager.hyprlandManager
            .unregisterKeybinding()
            .catch((err) => {
              debugLogger.warn("[IPC] Failed to unregister Hyprland keybinding:", err.message);
              return false;
            });
          if (!unregistered) {
            debugLogger.warn("[IPC] Hyprland keybinding remained active during capture");
          }
        }
      } else {
        // Exiting capture mode - re-register globalShortcut if not already registered
        // Skip for KDE/GNOME/Hyprland — updateHotkey handles re-registration via native path
        const usesNativePath =
          hotkeyManager.isUsingKDE() ||
          hotkeyManager.isUsingGnome() ||
          hotkeyManager.isUsingHyprland();
        if (!usesNativePath) {
          const { globalShortcut } = require("electron");
          // Re-register every globalShortcut-backed dictation hotkey (the slot
          // may hold several).
          for (const hk of hotkeyManager.getSlotHotkeys("dictation")) {
            if (!hk || usesNativeListener(hk)) continue;
            const accelerator = hk;
            if (!globalShortcut.isRegistered(accelerator)) {
              debugLogger.log(
                `[IPC] Re-registering globalShortcut "${accelerator}" after capture mode`
              );
              const callback = this.windowManager.createHotkeyCallback();
              const registered = globalShortcut.register(accelerator, () => callback(hk));
              if (!registered) {
                debugLogger.warn(
                  `[IPC] Failed to re-register globalShortcut "${accelerator}" after capture mode`
                );
              }
            }
          }
        }

        // Re-sync native key listeners (Windows/Linux) across all hotkey slots now
        // that capture is done. Idempotent — reads the current slot hotkeys.
        this.windowManager.reconcileNativeKeyListeners();

        // On GNOME, re-register the keybinding with the effective hotkey
        if (hotkeyManager.isUsingGnome() && hotkeyManager.gnomeManager && effectiveHotkey) {
          debugLogger.log(
            `[IPC] Re-registering GNOME keybinding "${effectiveHotkey}" after capture mode`
          );
          await hotkeyManager.registerGnomeDictationHotkey(
            effectiveHotkey,
            this.windowManager.createHotkeyCallback()
          );
        }

        // On Hyprland Wayland, re-register the keybinding with the effective hotkey
        if (hotkeyManager.isUsingHyprland() && hotkeyManager.hyprlandManager && effectiveHotkey) {
          debugLogger.log(
            `[IPC] Re-registering Hyprland keybinding "${effectiveHotkey}" after capture mode`
          );
          await hotkeyManager.hyprlandManager.registerKeybinding(
            effectiveHotkey,
            this.windowManager.getActivationMode() === "push"
          );
        }

        // On KDE (X11 or Wayland), re-register the keybinding with the effective hotkey
        if (hotkeyManager.isUsingKDE() && hotkeyManager.kdeManager && effectiveHotkey) {
          debugLogger.log(
            `[IPC] Re-registering KDE keybinding "${effectiveHotkey}" after capture mode`
          );
          const callback = this.windowManager.createHotkeyCallback();
          const result = await hotkeyManager.kdeManager.registerKeybinding(
            effectiveHotkey,
            "dictation",
            callback,
            this.windowManager.getActivationMode() === "push"
          );
          if (result !== true) {
            debugLogger.warn(
              `[IPC] Failed to re-register KDE keybinding "${effectiveHotkey}" after capture mode`,
              { result }
            );
          }
        }

        // Re-register non-dictation slots (meeting, agent) that were unregistered on capture enter
        for (const [slot, info] of hotkeyManager.slots) {
          const hotkeys = info?.hotkeys || [];
          if (slot === "dictation" || slot === "cancel" || hotkeys.length === 0 || !info?.callback)
            continue;
          debugLogger.log(
            `[IPC] Re-registering slot "${slot}" ("${hotkeys.join(", ")}") after capture mode`
          );
          const result = await hotkeyManager
            .registerSlot(slot, hotkeys, info.callback)
            .catch((err) => {
              debugLogger.warn(`[IPC] Failed to re-register slot "${slot}":`, err.message);
              return { success: false };
            });
          if (!result.success) {
            debugLogger.warn(`[IPC] Slot "${slot}" was not restored after capture`);
          }
        }
      }

      return { success: true };
    });

    ipcMain.handle("get-hotkey-mode-info", async (_event, requestedHotkey) => {
      const hotkeyManager = this.windowManager.hotkeyManager;
      const hotkey =
        typeof requestedHotkey === "string" && requestedHotkey.trim()
          ? requestedHotkey.split(",")[0].trim()
          : hotkeyManager.getCurrentHotkey();
      const isUsingNativeShortcut = this.windowManager.isUsingNativeShortcutHotkeys();
      const supportsPushToTalk =
        process.platform === "linux"
          ? isUsingNativeShortcut
            ? hotkeyManager.supportsPushToTalk(hotkey)
            : this.linuxKeyManager?.isAvailable?.() === true
          : process.platform === "darwin"
            ? hotkeyManager.supportsPushToTalk(hotkey)
            : !isUsingNativeShortcut;

      return {
        isUsingGnome: this.windowManager.isUsingGnomeHotkeys(),
        isUsingHyprland: this.windowManager.isUsingHyprlandHotkeys(),
        isUsingKDE: this.windowManager.isUsingKDEHotkeys(),
        isUsingNativeShortcut,
        supportsPushToTalk,
        pushToTalkUnavailableReason: supportsPushToTalk
          ? null
          : hotkeyManager.getPushToTalkUnavailableReason(hotkey),
      };
    });

    ipcMain.handle("get-hyprland-config-status", async () => {
      if (!this.windowManager.isUsingHyprlandHotkeys()) return null;
      return this.windowManager.getHyprlandConfigStatus();
    });

    ipcMain.handle("register-cancel-hotkey", async (event, key) => {
      const hotkeyManager = this.windowManager.hotkeyManager;
      const mainWindow = this.windowManager.mainWindow;
      return hotkeyManager.registerSlot("cancel", key, () => {
        mainWindow?.webContents?.send("cancel-hotkey-pressed");
      });
    });

    ipcMain.handle("unregister-cancel-hotkey", async () => {
      this.windowManager.hotkeyManager.unregisterSlot("cancel");
      return { success: true };
    });

    ipcMain.handle("start-window-drag", async (event) => {
      return await this.windowManager.startWindowDrag();
    });

    ipcMain.handle("stop-window-drag", async (event) => {
      return await this.windowManager.stopWindowDrag();
    });

    ipcMain.handle("start-control-panel-drag", async () => {
      return await this.windowManager.startControlPanelDrag();
    });

    ipcMain.handle("stop-control-panel-drag", async () => {
      return await this.windowManager.stopControlPanelDrag();
    });

    ipcMain.handle("open-external", async (event, url) => {
      try {
        const { protocol } = new URL(url);
        if (!["http:", "https:", "mailto:"].includes(protocol)) {
          return { success: false, error: `Blocked URL scheme: ${protocol}` };
        }
        await openExternalUrl(url);
        return { success: true };
      } catch (error) {
        return { success: false, error: error.message };
      }
    });

    ipcMain.handle("get-auto-start-enabled", async () => {
      try {
        return autoStart.getAutoStartState();
      } catch (error) {
        debugLogger.error("Error getting auto-start status:", error);
        return { enabled: false, requiresApproval: false };
      }
    });

    ipcMain.handle("set-auto-start-enabled", async (event, enabled) => {
      try {
        autoStart.setAutoStartEnabled(enabled);
        debugLogger.debug("Auto-start setting updated", { enabled });
        return { success: true };
      } catch (error) {
        debugLogger.error("Error setting auto-start:", error);
        return { success: false, error: error.message };
      }
    });

    ipcMain.handle("get-custom-transcription-key", async () => {
      return this.environmentManager.getCustomTranscriptionKey();
    });

    ipcMain.handle("save-custom-transcription-key", async (event, key) => {
      return this.environmentManager.saveCustomTranscriptionKey(key);
    });

    ipcMain.handle("get-cleanup-custom-key", async () => {
      return this.environmentManager.getCleanupCustomKey();
    });

    ipcMain.handle("save-cleanup-custom-key", async (event, key) => {
      return this.environmentManager.saveCleanupCustomKey(key);
    });

    ipcMain.handle("get-dictation-key", async () => {
      return this.environmentManager.getDictationKey();
    });

    ipcMain.handle("save-dictation-key", async (event, key) => {
      return this.environmentManager.saveDictationKey(key);
    });

    ipcMain.handle("get-active-dictation-key", async () => {
      const hotkeys = this.windowManager?.hotkeyManager?.getSlotHotkeys?.("dictation") ?? [];
      return hotkeys.length > 0 ? hotkeys.join(",") : null;
    });

    ipcMain.handle("get-effective-default-hotkey", async () => {
      return this.windowManager?.hotkeyManager?.getEffectiveDefaultHotkey() ?? null;
    });

    ipcMain.handle("get-activation-mode", async () => {
      return this.environmentManager.getActivationMode();
    });

    ipcMain.handle("save-activation-mode", async (event, mode) => {
      return this.environmentManager.saveActivationMode(mode);
    });

    ipcMain.handle("get-ui-language", async () => {
      return this.environmentManager.getUiLanguage();
    });

    ipcMain.handle("get-menu-bar-icon-visible", () => {
      return this.environmentManager.getMenuBarIconVisible();
    });

    ipcMain.handle("set-menu-bar-icon-visible", async (_event, visible) => {
      if (typeof visible !== "boolean") throw new TypeError("Expected a boolean visibility");
      await Promise.all([
        this.environmentManager.saveMenuBarIconVisible(visible),
        process.platform === "darwin" ? this.getTrayManager?.()?.setVisible(visible) : undefined,
      ]);
      const currentVisible = this.environmentManager.getMenuBarIconVisible();
      broadcastToWindows("setting-updated", { key: "showMenuBarIcon", value: currentVisible });
      return currentVisible;
    });

    ipcMain.handle("save-ui-language", async (event, language) => {
      return this.environmentManager.saveUiLanguage(language);
    });

    ipcMain.handle("set-ui-language", async (event, language) => {
      const result = this.environmentManager.saveUiLanguage(language);
      process.env.UI_LANGUAGE = result.language;
      changeLanguage(result.language);
      this.windowManager?.refreshLocalizedUi?.();
      this.getTrayManager?.()?.updateTrayMenu?.();
      return { success: true, language: result.language };
    });

    ipcMain.handle("sync-startup-preferences", async (event, prefs) => {
      const setVars = {};
      const clearVars = [];

      if (prefs.useLocalWhisper && prefs.model) {
        // Local mode with model selected - set provider and model for pre-warming
        setVars.LOCAL_TRANSCRIPTION_PROVIDER = prefs.localTranscriptionProvider;
        if (prefs.language) setVars.DICTATION_LANGUAGE = prefs.language;
        if (isSherpaLocalProvider(prefs.localTranscriptionProvider)) {
          setVars.PARAKEET_MODEL = prefs.model;
          clearVars.push("LOCAL_WHISPER_MODEL");
          this.whisperManager.stopServer().catch((err) => {
            debugLogger.error("Failed to stop whisper-server on provider switch", {
              error: err.message,
            });
          });
        } else {
          setVars.LOCAL_WHISPER_MODEL = prefs.model;
          clearVars.push("PARAKEET_MODEL");
          this.parakeetManager.stopServer().catch((err) => {
            debugLogger.error("Failed to stop parakeet-server on provider switch", {
              error: err.message,
            });
          });
        }
      } else if (prefs.useLocalWhisper) {
        // Local mode enabled but no model selected - clear pre-warming vars
        clearVars.push("LOCAL_TRANSCRIPTION_PROVIDER", "PARAKEET_MODEL", "LOCAL_WHISPER_MODEL");
      } else {
        // Cloud mode - stop local servers to free RAM
        clearVars.push("LOCAL_TRANSCRIPTION_PROVIDER", "PARAKEET_MODEL", "LOCAL_WHISPER_MODEL");
        this.whisperManager.stopServer().catch((err) => {
          debugLogger.error("Failed to stop whisper-server on cloud switch", {
            error: err.message,
          });
        });
        this.parakeetManager.stopServer().catch((err) => {
          debugLogger.error("Failed to stop parakeet-server on cloud switch", {
            error: err.message,
          });
        });
      }

      const localServer = resolveLocalServerNeeds(prefs);

      if (localServer.cleanup) {
        setVars.CLEANUP_PROVIDER = "local";
        setVars.LOCAL_CLEANUP_MODEL = localServer.cleanup;
      } else {
        clearVars.push("CLEANUP_PROVIDER", "LOCAL_CLEANUP_MODEL");
      }
      // TODO: drop legacy REASONING_PROVIDER / LOCAL_REASONING_MODEL clears once
      // the read fallback is removed (~2 releases after this lands).
      clearVars.push("REASONING_PROVIDER", "LOCAL_REASONING_MODEL");

      if (localServer.dictationAgent) {
        setVars.DICTATION_AGENT_PROVIDER = "local";
        setVars.LOCAL_DICTATION_AGENT_MODEL = localServer.dictationAgent;
      } else {
        clearVars.push("DICTATION_AGENT_PROVIDER", "LOCAL_DICTATION_AGENT_MODEL");
      }

      // Stop the shared llama-server only when neither scope still needs it, so
      // the active scope keeps its server when the other one switches away.
      if (localServer.stopServer) {
        const modelManager = require("./modelManagerBridge").default;
        modelManager.stopServer().catch((err) => {
          debugLogger.error("Failed to stop llama-server on provider switch", {
            error: err.message,
          });
        });
      }

      this._syncStartupEnv(setVars, clearVars);
    });

    ipcMain.handle("get-log-level", async () => {
      return debugLogger.getLevel();
    });

    ipcMain.handle("app-log", async (event, entry) => {
      debugLogger.logEntry(entry);
      return { success: true };
    });

    const SYSTEM_SETTINGS_URLS = {
      darwin: {
        microphone: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
        sound: "x-apple.systempreferences:com.apple.preference.sound?input",
        accessibility:
          "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        systemAudio:
          "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
        screenRecording:
          "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
        calendars: "x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars",
        loginItems: "x-apple.systempreferences:com.apple.LoginItems-Settings.extension",
      },
      win32: {
        microphone: "ms-settings:privacy-microphone",
        sound: "ms-settings:sound",
        loginItems: "ms-settings:startupapps",
      },
    };

    const openSystemSettings = async (settingType) => {
      const platform = process.platform;
      const urls = SYSTEM_SETTINGS_URLS[platform];
      const url = urls?.[settingType];

      if (!url) {
        // Platform doesn't support this settings URL
        const messages = {
          microphone: i18nMain.t("systemSettings.microphone"),
          sound: i18nMain.t("systemSettings.sound"),
          accessibility: i18nMain.t("systemSettings.accessibility"),
          systemAudio: i18nMain.t("systemSettings.systemAudio"),
          screenRecording: i18nMain.t("systemSettings.screenRecording"),
          loginItems: i18nMain.t("systemSettings.loginItems"),
        };
        return {
          success: false,
          error:
            messages[settingType] || `${settingType} settings are not available on this platform.`,
        };
      }

      try {
        await shell.openExternal(url);
        return { success: true };
      } catch (error) {
        debugLogger.error(`Failed to open ${settingType} settings:`, error);
        return { success: false, error: error.message };
      }
    };

    ipcMain.handle("open-microphone-settings", () => openSystemSettings("microphone"));
    ipcMain.handle("open-sound-input-settings", () => openSystemSettings("sound"));
    ipcMain.handle("get-system-default-microphone", (_event, options = {}) =>
      resolveSystemDefaultMicrophone({ refresh: options?.refresh === true })
    );
    ipcMain.handle("open-accessibility-settings", () => openSystemSettings("accessibility"));
    ipcMain.handle("open-login-items-settings", () => openSystemSettings("loginItems"));

    ipcMain.handle("toggle-media-playback", () => {
      const mediaPlayer = require("./mediaPlayer");
      return mediaPlayer.toggleMedia();
    });

    ipcMain.handle("pause-media-playback", () => {
      const mediaPlayer = require("./mediaPlayer");
      return mediaPlayer.pauseMedia();
    });

    ipcMain.handle("resume-media-playback", () => {
      const mediaPlayer = require("./mediaPlayer");
      return mediaPlayer.resumeMedia();
    });

    ipcMain.handle("request-microphone-access", async () => {
      if (process.platform !== "darwin") {
        return { granted: true, status: "granted" };
      }
      const granted = await systemPreferences.askForMediaAccess("microphone");
      return { granted };
    });

    ipcMain.handle("check-microphone-access", () => {
      if (process.platform !== "darwin") {
        return { granted: true, status: "granted" };
      }
      const status = systemPreferences.getMediaAccessStatus("microphone");
      return { granted: status === "granted", status };
    });

    // In production, VITE_* env vars aren't available in the main process because
    // Vite only inlines them into the renderer bundle at build time. Load the
    // runtime-env.json that the Vite build writes to src/dist/ as a fallback.
    const runtimeEnv = (() => {
      const fs = require("fs");
      const envPath = path.join(__dirname, "..", "dist", "runtime-env.json");
      try {
        if (fs.existsSync(envPath)) return JSON.parse(fs.readFileSync(envPath, "utf8"));
      } catch {}
      return {};
    })();

    const getApiUrl = () =>
      process.env.OPENWHISPR_API_URL ||
      process.env.VITE_OPENWHISPR_API_URL ||
      runtimeEnv.VITE_OPENWHISPR_API_URL ||
      "";

    const getAuthUrl = () =>
      process.env.AUTH_URL ||
      process.env.VITE_AUTH_URL ||
      runtimeEnv.VITE_AUTH_URL ||
      "https://auth.openwhispr.com";

    const getSessionCookiesFromWindow = async (win) => {
      const scopedUrls = [getAuthUrl(), getApiUrl()].filter(Boolean);
      const cookiesByName = new Map();

      for (const url of scopedUrls) {
        try {
          const scopedCookies = await win.webContents.session.cookies.get({ url });
          for (const cookie of scopedCookies) {
            if (!cookiesByName.has(cookie.name)) {
              cookiesByName.set(cookie.name, cookie.value);
            }
          }
        } catch (error) {
          debugLogger.warn("Failed to read scoped auth cookies", {
            url,
            error: error.message,
          });
        }
      }

      // Fallback for older sessions where cookies are not URL-scoped as expected.
      if (cookiesByName.size === 0) {
        const allCookies = await win.webContents.session.cookies.get({});
        for (const cookie of allCookies) {
          if (!cookiesByName.has(cookie.name)) {
            cookiesByName.set(cookie.name, cookie.value);
          }
        }
      }

      const cookieHeader = [...cookiesByName.entries()]
        .map(([name, value]) => `${name}=${value}`)
        .join("; ");

      debugLogger.debug(
        "Resolved auth cookies for cloud request",
        {
          cookieCount: cookiesByName.size,
          scopedUrls,
        },
        "auth"
      );

      return cookieHeader;
    };

    // Bearer auth is preferred. Cookie fallback covers the brief window before
    // main.js's startup migration bridge runs (or if it failed for this user).
    const getAuthHeaderFromWindow = async (win) => {
      const token = tokenStore.get();
      if (token) return { Authorization: `Bearer ${token}` };
      const cookieHeader = win ? await getSessionCookiesFromWindow(win) : "";
      return cookieHeader ? { Cookie: cookieHeader } : {};
    };

    // Honors system proxy via Electron's net stack. useSessionCookies:false so
    // Electron doesn't auto-attach jar cookies on top of our explicit headers.
    const proxyFetch = (url, init = {}) => net.fetch(url, { ...init, useSessionCookies: false });
    const withPolicyHeaders = (headers) => withPolicyRequestHeaders(headers, app.getVersion());

    ipcMain.handle("retry-transcription", async (event, id, settings) => {
      const buffer = this.audioStorageManager.getAudioBuffer(id);
      if (!buffer) return { success: false, error: "Audio file not found" };
      try {
        let result;
        const preferredLanguage = settings?.preferredLanguage;
        const language =
          preferredLanguage && preferredLanguage !== "auto"
            ? preferredLanguage.split("-")[0]
            : undefined;
        const { resolveTranscriptionRoute } = await import("./transcriptionRoute.ts");
        // Renderer pre-flight owns policy; retry re-routes stored audio through
        // whatever is selected NOW.
        const route = resolveTranscriptionRoute({
          settings: settings || {},
          providers: transcriptionProviderBaseUrls(),
          managed: settings?.managed,
          request: { effectiveLanguage: language },
        });

        // An error route is fatal unless OpenWhispr cloud is selected — a
        // leftover BYOK misconfiguration must not block the cloud pipeline.
        if (
          route.transport === "error" &&
          (settings?.transcriptionMode === "self-hosted" ||
            settings?.cloudTranscriptionMode !== "openwhispr")
        ) {
          const err = new Error(route.message);
          if (route.code) err.code = route.code;
          if (route.messageKey) err.messageKey = route.messageKey;
          throw err;
        }

        if (route.transport === "managed") {
          const text = await this.executeManagedTranscription(event, route, {
            audioBuffer: buffer,
            fileName: "audio.webm",
            contentType: "audio/webm",
          });
          result = { text, source: "azure-managed", model: route.deployment };
        } else if (route.transport === "http-batch" && route.provider === "self-hosted") {
          const formData = new FormData();
          formData.append("file", new Blob([buffer], { type: "audio/webm" }), "audio.webm");
          if (route.model) {
            formData.append("model", route.model);
          }
          if (route.language) {
            formData.append("language", route.language);
          }

          const response = await proxyFetch(route.endpoint, {
            method: "POST",
            body: formData,
          });
          if (!response.ok) {
            const errorText = await response.text();
            throw new Error(`Self-hosted API Error: ${response.status} ${errorText}`);
          }
          const data = await response.json();
          if (data?.text) {
            result = {
              text: data.text,
              source: "self-hosted",
              model: route.model,
            };
          }
        } else if (route.transport === "local") {
          if (isSherpaLocalProvider(settings.localTranscriptionProvider)) {
            const model =
              (settings.localTranscriptionProvider === "cohere"
                ? settings.cohereModel
                : settings.parakeetModel) ||
              process.env.PARAKEET_MODEL ||
              "parakeet-tdt-0.6b-v3";
            result = await this.parakeetManager.transcribeLocalParakeet(buffer, {
              model,
              language,
            });
          } else if (this.whisperManager?.serverManager?.isAvailable?.()) {
            const vadOptions = this._resolveWhisperVadOptions("noteRecording");
            result = await this.whisperManager.transcribeLocalWhisper(buffer, {
              model: settings.whisperModel,
              language,
              ...vadOptions,
            });
          }
        } else if (settings?.cloudTranscriptionMode === "openwhispr") {
          const win = BrowserWindow.fromWebContents(event.sender);
          if (win) {
            const authHeader = await getAuthHeaderFromWindow(win);
            if (Object.keys(authHeader).length) {
              const apiUrl = getApiUrl();
              if (apiUrl) {
                const multipartFields = {
                  language,
                  clientType: "desktop",
                  appVersion: app.getVersion(),
                  sessionId: this.sessionId,
                };
                if (buffer.length > CLOUD_INLINE_LIMIT) {
                  const { text } = await chunkedCloudTranscribe({
                    buffer,
                    apiUrl,
                    policyHeaders: withPolicyHeaders(authHeader),
                    multipartFields,
                  });
                  result = { text, source: "openwhispr", model: "cloud" };
                } else {
                  const { body, boundary } = buildMultipartBody(
                    buffer,
                    "audio.webm",
                    "audio/webm",
                    multipartFields
                  );
                  const url = new URL(`${apiUrl}/api/transcribe`);
                  const data = await postMultipart(
                    url,
                    body,
                    boundary,
                    withPolicyHeaders(authHeader),
                    {
                      signal: AbortSignal.timeout(CLOUD_UPLOAD_TIMEOUT_MS),
                      session: getInlineCloudUploadSession(),
                    }
                  );
                  const responseData = interpretTranscribeResponse(data);
                  result = {
                    text: responseData.text,
                    source: "openwhispr",
                    model: "cloud",
                  };
                }
              }
            }
          }
        } else if (route.transport === "proxied" && route.provider === "tinfoil") {
          // Attested transport, so this can't reuse the generic fetch below.
          const { text, model } = await transcribeWithTinfoil({
            audioBuffer: buffer,
            fileName: "audio.webm",
            contentType: "audio/webm",
            language: route.language,
            apiKey: this.environmentManager.getTinfoilKey(),
          });
          if (text) result = { text, source: "tinfoil", model };
        } else if (route.transport === "proxied" && route.provider === "corti") {
          // Corti uses OAuth + an interaction-based REST flow, so it can't use
          // the generic fetch below.
          const clientId = this.environmentManager.getCortiClientId();
          const clientSecret = this.environmentManager.getCortiClientSecret();
          if (!clientId || !clientSecret) {
            throw new Error("Corti credentials not configured. Add them in Settings.");
          }
          const { transcribeAudio } = require("./cortiTranscription");
          const { text } = await transcribeAudio({
            environment: route.cortiEnvironment,
            tenant: route.cortiTenant,
            clientId,
            clientSecret,
            audioBuffer: buffer,
            language: route.language,
          });
          if (text) result = { text, source: "corti", model: route.model };
        } else if (route.transport === "proxied" && route.provider === "gemini") {
          if (route.sizeCapBytes && buffer.byteLength > route.sizeCapBytes) {
            throw new Error(byokSizeCapError(route.sizeCapBytes));
          }
          const { text } = await transcribeWithGemini({
            audioBuffer: buffer,
            model: route.model,
            contentType: "audio/webm",
            language: route.language,
            apiKey: this.environmentManager.getGeminiKey(),
          });
          if (text) result = { text, source: "gemini", model: route.model };
        } else {
          // mistral/xai have no OpenAI-compatible endpoint — main talks to them
          // directly; everything else consumes the route endpoint as-is.
          const provider = route.provider;
          const endpoint =
            provider === "mistral"
              ? MISTRAL_TRANSCRIPTION_URL
              : provider === "xai"
                ? XAI_STT_URL
                : route.endpoint;
          const apiKey =
            provider === "mistral"
              ? this.environmentManager.getMistralKey()
              : provider === "xai"
                ? this.environmentManager.getXaiKey()
                : route.auth.keyRef === "custom"
                  ? this.environmentManager.getCustomTranscriptionKey()
                  : route.auth.keyRef === "groq"
                    ? this.environmentManager.getGroqKey()
                    : this.environmentManager.getOpenAIKey();
          if (!apiKey && provider !== "custom") {
            throw new Error(`${provider} API key not configured`);
          }

          const formData = new FormData();
          formData.append("file", new Blob([buffer], { type: "audio/webm" }), "audio.webm");
          if (provider === "xai") {
            // xAI STT does not accept a model field; the route pre-filters language
            if (route.language) {
              formData.append("language", route.language);
              formData.append("format", "true");
            }
          } else {
            formData.append("model", route.model);
            if (route.language) formData.append("language", route.language);
          }
          const headers = {};
          if (provider === "mistral") {
            headers["x-api-key"] = apiKey;
          } else if (apiKey) {
            if (route.transport === "http-batch" && route.auth.scheme === "azure-api-key") {
              headers["api-key"] = apiKey;
            } else {
              headers.Authorization = `Bearer ${apiKey}`;
            }
          }

          const response = await proxyFetch(endpoint, { method: "POST", headers, body: formData });
          if (!response.ok) {
            const errorText = await response.text();
            throw new Error(`${provider} API Error: ${response.status} ${errorText}`);
          }
          const data = await response.json();
          if (data?.text) {
            result = { text: data.text, source: provider, model: route.model };
          }
        }

        if (!result?.text) {
          return { success: false, error: "No transcription engine available" };
        }

        this.databaseManager.updateTranscriptionText(id, result.text, result.text);
        this.databaseManager.updateTranscriptionStatus(id, "completed");
        const providerName = result.source || "local";
        const modelName = result.model || null;
        const existingRow = this.databaseManager.getTranscriptionById(id);
        this.databaseManager.updateTranscriptionAudio(id, {
          hasAudio: 1,
          audioDurationMs: existingRow?.audio_duration_ms ?? null,
          provider: providerName,
          model: modelName,
        });
        const updated = this.databaseManager.getTranscriptionById(id);
        if (updated) {
          setImmediate(() => {
            broadcastToWindows("transcription-updated", updated);
          });
        }
        return { success: true, transcription: updated };
      } catch (error) {
        debugLogger.error(
          "Retry transcription failed",
          { id, error: error.message, code: error.code },
          "audio-storage"
        );
        if (error.code) {
          return { success: false, error: error.message, code: error.code, ...error };
        }
        return { success: false, error: error.message };
      }
    });

    const fs = require("fs");

    ipcMain.handle("update-transcription-text", async (_event, id, text, rawText) => {
      try {
        this.databaseManager.updateTranscriptionText(id, text, rawText);
        const updated = this.databaseManager.getTranscriptionById(id);
        return { success: true, transcription: updated };
      } catch (error) {
        debugLogger.error(
          "Failed to update transcription text",
          { id, error: error.message },
          "audio-storage"
        );
        return { success: false, error: error.message };
      }
    });

    ipcMain.handle("get-ydotool-status", () => {
      const { getYdotoolStatus } = require("./ensureYdotool");
      const { getLinuxSessionInfo } = require("./linuxSession");
      const { execFileSync } = require("child_process");
      const status = getYdotoolStatus();
      const { isKde } = getLinuxSessionInfo();
      let hasXclip = false;
      let hasXsel = false;
      if (isKde) {
        try {
          execFileSync("which", ["xclip"], { timeout: 1000 });
          hasXclip = true;
        } catch {}
        try {
          execFileSync("which", ["xsel"], { timeout: 1000 });
          hasXsel = true;
        } catch {}
      }
      return { ...status, hasXclip, hasXsel };
    });

    ipcMain.handle("get-debug-state", async () => {
      try {
        return {
          enabled: debugLogger.isEnabled(),
          logPath: debugLogger.getLogPath(),
          logLevel: debugLogger.getLevel(),
        };
      } catch (error) {
        debugLogger.error("Failed to get debug state:", error);
        return { enabled: false, logPath: null, logLevel: "info" };
      }
    });

    ipcMain.handle("set-debug-logging", async (event, enabled) => {
      try {
        const path = require("path");
        const fs = require("fs");
        const envPath = path.join(app.getPath("userData"), ".env");

        // Read current .env content
        let envContent = "";
        if (fs.existsSync(envPath)) {
          envContent = fs.readFileSync(envPath, "utf8");
        }

        // Parse lines
        const lines = envContent.split("\n");
        const logLevelIndex = lines.findIndex((line) =>
          line.trim().startsWith("OPENWHISPR_LOG_LEVEL=")
        );

        if (enabled) {
          // Set to debug
          if (logLevelIndex !== -1) {
            lines[logLevelIndex] = "OPENWHISPR_LOG_LEVEL=debug";
          } else {
            // Add new line
            if (lines.length > 0 && lines[lines.length - 1] !== "") {
              lines.push("");
            }
            lines.push("# Debug logging setting");
            lines.push("OPENWHISPR_LOG_LEVEL=debug");
          }
        } else {
          // Remove or set to info
          if (logLevelIndex !== -1) {
            lines[logLevelIndex] = "OPENWHISPR_LOG_LEVEL=info";
          }
        }

        // Write back
        fs.writeFileSync(envPath, lines.join("\n"), "utf8");

        // Update environment variable
        process.env.OPENWHISPR_LOG_LEVEL = enabled ? "debug" : "info";

        // Refresh logger state
        debugLogger.refreshLogLevel();

        return {
          success: true,
          enabled: debugLogger.isEnabled(),
          logPath: debugLogger.getLogPath(),
        };
      } catch (error) {
        debugLogger.error("Failed to set debug logging:", error);
        return { success: false, error: error.message };
      }
    });

    ipcMain.handle("open-logs-folder", async () => {
      try {
        const logsDir = path.join(app.getPath("userData"), "logs");
        await shell.openPath(logsDir);
        return { success: true };
      } catch (error) {
        debugLogger.error("Failed to open logs folder:", error);
        return { success: false, error: error.message };
      }
    });

    ipcMain.handle("get-app-version", async () => {
      return { version: app.getVersion() };
    });

    ipcMain.handle("get-post-migration-state", () => ({
      justMigrated: postMigrationDetector.isReturningFromOldBundle(),
    }));

    ipcMain.handle("mark-bundle-migrated", () => {
      postMigrationDetector.markBundleMigrated();
    });

    ipcMain.handle("mark-bundle-migration-dismissed", () => {
      postMigrationDetector.markBundleMigrationDismissed();
    });

    ipcMain.handle("acquire-recording-lock", async (_event, pipeline) => {
      if (this._activeRecordingPipeline && this._activeRecordingPipeline !== pipeline) {
        return { success: false, holder: this._activeRecordingPipeline };
      }
      this._activeRecordingPipeline = pipeline;
      return { success: true };
    });

    ipcMain.handle("release-recording-lock", async (_event, pipeline) => {
      if (this._activeRecordingPipeline === pipeline) {
        this._activeRecordingPipeline = null;
      }
      return { success: true };
    });

    ipcMain.handle("whisper-vad-get-config", async () => {
      try {
        return { success: true, config: this._getWhisperVadSettings() };
      } catch (error) {
        return { success: false, error: error.message };
      }
    });

    ipcMain.handle("whisper-vad-set-config", async (_event, payload) => {
      try {
        const config = this._setWhisperVadSettings(payload || {});
        return { success: true, config };
      } catch (error) {
        return { success: false, error: error.message };
      }
    });
  }

  _retroactiveMapping(profile) {
    setImmediate(async () => {
      try {
        const speakerEmbeddings = require("./speakerEmbeddings");
        const noteIds = this.databaseManager.getNotesWithUnmappedSpeakers();

        const profileEmb = new Float32Array(
          profile.embedding.buffer,
          profile.embedding.byteOffset,
          profile.embedding.byteLength / 4
        );

        for (const noteId of noteIds) {
          const embeddings = this.databaseManager.getNoteSpeakerEmbeddings(noteId);
          const existing = this.databaseManager.getSpeakerMappings(noteId);
          const mappedSpeakers = new Set(existing.map((m) => m.speaker_id));
          for (const emb of embeddings) {
            if (mappedSpeakers.has(emb.speaker_id)) continue;

            const speakerEmb = new Float32Array(
              emb.embedding.buffer,
              emb.embedding.byteOffset,
              emb.embedding.byteLength / 4
            );
            const similarity = speakerEmbeddings.cosineSimilarity(profileEmb, speakerEmb);

            if (similarity > 0.6) {
              this.databaseManager.setSpeakerMapping(
                noteId,
                emb.speaker_id,
                profile.id,
                profile.display_name
              );

              const note = this.databaseManager.getNote(noteId);
              if (note?.transcript) {
                try {
                  const segments = JSON.parse(note.transcript);
                  let changed = false;
                  for (const seg of segments) {
                    if (seg.speaker === emb.speaker_id && !seg.speakerName) {
                      if (canAutoRelabelSpeaker(seg)) {
                        applyConfirmedSpeaker(seg, {
                          speakerName: profile.display_name,
                          speakerIsPlaceholder: false,
                        });
                      } else {
                        seg.speakerName = profile.display_name;
                        seg.speakerIsPlaceholder = false;
                      }
                      changed = true;
                    }
                  }
                  if (changed) {
                    this.databaseManager.updateNote(noteId, {
                      transcript: JSON.stringify(segments),
                    });
                  }
                } catch (_) {}
              }
            }
          }
        }
      } catch (err) {
        debugLogger.warn("Retroactive speaker mapping failed", { error: err.message });
      }
    });
  }

  _tryAutoLabelOneOnOne(noteId) {
    setImmediate(async () => {
      try {
        const note = this.databaseManager.getNote(noteId);
        const other = this._resolveOneOnOneOtherParticipant(note?.participants);
        if (!other) return;
        const { displayName, email } = other;

        const embeddings = this.databaseManager.getNoteSpeakerEmbeddings(noteId);
        if (!embeddings.length) return;

        const existingMappings = this.databaseManager.getSpeakerMappings(noteId);
        const mappedSpeakers = new Set(existingMappings.map((m) => m.speaker_id));

        const transcript = note.transcript ? JSON.parse(note.transcript) : [];
        const systemSpeakers = new Set(
          transcript.filter((s) => s.source !== "mic" && s.speaker).map((s) => s.speaker)
        );

        const unmapped = embeddings.filter(
          (e) => !mappedSpeakers.has(e.speaker_id) && systemSpeakers.has(e.speaker_id)
        );
        if (!unmapped.length) return;

        let profile = null;
        for (const emb of unmapped) {
          profile = this.databaseManager.upsertSpeakerProfile(
            displayName,
            email,
            emb.embedding,
            profile?.id ?? null
          );
          this.databaseManager.setSpeakerMapping(noteId, emb.speaker_id, profile.id, displayName);
          liveSpeakerIdentifier.mapSpeaker(emb.speaker_id, profile.id, displayName, noteId);
        }

        const unmappedSystemSpeakers = new Set(unmapped.map((e) => e.speaker_id));
        let changed = false;
        for (const seg of transcript) {
          if (!unmappedSystemSpeakers.has(seg.speaker)) continue;
          if (seg.speakerName && !seg.speakerIsPlaceholder) continue;
          if (canAutoRelabelSpeaker(seg)) {
            applyConfirmedSpeaker(seg, { speakerName: displayName, speakerIsPlaceholder: false });
          } else {
            seg.speakerName = displayName;
            seg.speakerIsPlaceholder = false;
          }
          changed = true;
        }

        if (changed) {
          this.databaseManager.updateNote(noteId, { transcript: JSON.stringify(transcript) });
          const updated = this.databaseManager.getNote(noteId);
          if (updated) broadcastToWindows("note-updated", updated);
        }

        if (profile) this._retroactiveMapping(profile);

        debugLogger.info(
          "Auto-labeled 1-on-1 meeting speakers",
          { noteId, displayName, speakerCount: unmapped.length },
          "speaker"
        );
      } catch (err) {
        debugLogger.warn("Auto-label 1-on-1 failed", { noteId, error: err.message }, "speaker");
      }
    });
  }

  _applySpeakerName(segments, speakerId, displayName) {
    if (!displayName) {
      return;
    }

    for (const segment of segments) {
      if (segment.speaker !== speakerId) {
        continue;
      }

      applyConfirmedSpeaker(segment, {
        speakerName: displayName,
        speakerIsPlaceholder: false,
        suggestedName: undefined,
        suggestedProfileId: undefined,
      });
    }
  }

  _reconcileLiveSpeakerState(liveSpeakerState, speakerEmbeddingsMap, enrichedSegments) {
    if (!liveSpeakerState || !speakerEmbeddingsMap) {
      return new Set();
    }

    const speakerEmbeddings = require("./speakerEmbeddings");
    const reconciledSpeakers = new Set();
    const usedLiveSpeakers = new Set();
    const noteMappings = new Map();

    const liveEntries = Object.entries(liveSpeakerState)
      .map(([speakerId, data]) => ({
        speakerId,
        displayName: data?.displayName || null,
        profileId: data?.profileId ?? null,
        noteId: data?.noteId ?? null,
        embedding: Array.isArray(data?.embedding) ? new Float32Array(data.embedding) : null,
      }))
      .filter((entry) => entry.embedding);

    const getMappingsForNote = (noteId) => {
      if (!noteMappings.has(noteId)) {
        noteMappings.set(noteId, this.databaseManager.getSpeakerMappings(noteId));
      }
      return noteMappings.get(noteId);
    };

    for (const [mappedId, embeddingArray] of Object.entries(speakerEmbeddingsMap)) {
      let bestEntry = null;
      let bestSimilarity = 0;

      for (const entry of liveEntries) {
        if (usedLiveSpeakers.has(entry.speakerId)) {
          continue;
        }

        const similarity = speakerEmbeddings.cosineSimilarity(
          new Float32Array(embeddingArray),
          entry.embedding
        );
        if (similarity > bestSimilarity) {
          bestSimilarity = similarity;
          bestEntry = entry;
        }
      }

      if (!bestEntry || bestSimilarity <= 0.6) {
        continue;
      }

      usedLiveSpeakers.add(bestEntry.speakerId);
      reconciledSpeakers.add(mappedId);

      let displayName = bestEntry.displayName;
      let profileId = bestEntry.profileId;

      if (bestEntry.noteId) {
        const liveMapping = getMappingsForNote(bestEntry.noteId).find(
          (mapping) => mapping.speaker_id === bestEntry.speakerId
        );
        if (liveMapping) {
          displayName = liveMapping.display_name || displayName;
          profileId = liveMapping.profile_id ?? profileId;
          this.databaseManager.setSpeakerMapping(
            bestEntry.noteId,
            mappedId,
            profileId,
            displayName
          );
          // Live and offline ids share one namespace now, so they often match —
          // removing the "old" row would delete the mapping just written.
          if (bestEntry.speakerId !== mappedId) {
            this.databaseManager.removeSpeakerMapping(bestEntry.noteId, bestEntry.speakerId);
          }
        } else if (displayName) {
          this.databaseManager.setSpeakerMapping(
            bestEntry.noteId,
            mappedId,
            profileId,
            displayName
          );
        }
      }

      this._applySpeakerName(enrichedSegments, mappedId, displayName);
    }

    return reconciledSpeakers;
  }

  _resolveSpeakerExpectation({ sessionConfig, noteId, observedSpeakerIds, diarizedSource }) {
    // Only a count the user set explicitly outranks the note: participants added
    // mid-meeting postdate the config snapshot taken at recording start.
    let expectedTotal = sessionConfig?.explicit ? sessionConfig.expectedCount : null;

    if (!expectedTotal && noteId != null) {
      try {
        expectedTotal = this._noteExpectedSpeakerCountOrNull(this.databaseManager.getNote(noteId));
      } catch (_) {
        expectedTotal = null;
      }
    }

    // Diarizing the mic track (in-person session) means the user is one of the
    // diarized voices, so the expected total applies without the -1 the
    // system-audio branches use.
    const micMode = diarizedSource === "mic";

    if (expectedTotal) {
      const total = Math.min(expectedTotal, MAX_SPEAKER_COUNT);
      const numSpeakers = micMode ? total : Math.max(1, total - 1);
      return { numSpeakers, cap: numSpeakers };
    }

    if (observedSpeakerIds.size >= 2) {
      const numSpeakers = Math.min(observedSpeakerIds.size, MAX_SPEAKER_COUNT);
      return { numSpeakers, cap: numSpeakers };
    }

    if (micMode) {
      return { numSpeakers: -1, cap: DEFAULT_EXPECTED_SPEAKER_COUNT };
    }

    // Only system audio reaches the diarizer (the mic track is "you"), so the cap
    // counts other speakers — same total - 1 basis as the branches above.
    return { numSpeakers: -1, cap: Math.max(1, DEFAULT_EXPECTED_SPEAKER_COUNT - 1) };
  }

  _startOrSkipDiarization(
    sessionId,
    rawPcmPath,
    audioStartedAt,
    transcriptSegments,
    win,
    liveSpeakerState = null,
    sessionConfig = null,
    noteId = null,
    diarizedSource = "system"
  ) {
    const send = (payload) => {
      if (win && !win.isDestroyed()) {
        win.webContents.send("meeting-diarization-complete", { sessionId, noteId, ...payload });
      }
    };

    const diarizationEnabled = (sessionConfig?.enabled ?? this.speakerDiarizationEnabled) !== false;

    if (!diarizationEnabled || !this.diarizationManager?.isAvailable() || !rawPcmPath) {
      send({
        segments: transcriptSegments.map((segment, index) => ({
          ...segment,
          id: segment.id || `segment-${index}`,
        })),
      });
      return;
    }

    const fs = require("fs");

    (async () => {
      let tmpWav = null;
      try {
        tmpWav = await this.diarizationManager.convertRawPcmToWav(rawPcmPath, 24000);
        const observedSpeakerIds = new Set(
          transcriptSegments
            .filter((segment) => segment.source === "system" && segment.speaker)
            .map((segment) => segment.speaker)
        );
        for (const speakerId of Object.keys(liveSpeakerState || {})) {
          observedSpeakerIds.add(speakerId);
        }

        if (observedSpeakerIds.size > 10) {
          debugLogger.warn("Excessive speaker count from live identification", {
            observedSpeakers: observedSpeakerIds.size,
          });
        }

        const { numSpeakers, cap } = this._resolveSpeakerExpectation({
          sessionConfig,
          noteId,
          observedSpeakerIds,
          diarizedSource,
        });
        let diarizationSegments = await this.diarizationManager.diarize(
          tmpWav,
          numSpeakers > 0 ? { numSpeakers } : {}
        );
        if (cap != null) {
          diarizationSegments = this.diarizationManager.capSpeakerClusters(
            diarizationSegments,
            cap
          );
        }

        const startMs =
          (Number.isFinite(audioStartedAt) && audioStartedAt) ||
          transcriptSegments.find((segment) => segment.source === diarizedSource)?.timestamp ||
          transcriptSegments[0]?.timestamp ||
          0;
        const isEpochMs = startMs > 1e9;
        const normalized = transcriptSegments.map((seg) => ({
          ...seg,
          timestamp:
            seg.timestamp != null
              ? isEpochMs
                ? (seg.timestamp - startMs) / 1000
                : seg.timestamp
              : undefined,
        }));

        const enrichedSegments = this.diarizationManager.mergeWithTranscript(
          normalized,
          diarizationSegments,
          { diarizedSource }
        );

        const speakerSet = new Set(diarizationSegments.map((d) => d.speaker));
        const speakerRenumber = new Map();
        let sIdx = 0;
        for (const sp of speakerSet) {
          speakerRenumber.set(sp, `speaker_${sIdx}`);
          sIdx++;
        }

        // Mirrors the mic-mode single-cluster softening in mergeWithTranscript:
        // every segment stays "you", so persisting an embedding keyed to a
        // cluster id that owns no segments would leave the note inconsistent.
        const micSingleClusterSoftened = diarizedSource === "mic" && speakerSet.size === 1;

        let speakerEmbeddingsMap = null;
        const speakerEmb = require("./speakerEmbeddings");
        try {
          if (!micSingleClusterSoftened && speakerEmb.isAvailable() && tmpWav) {
            const speakerIds = [...new Set(diarizationSegments.map((s) => s.speaker))];
            speakerEmbeddingsMap = {};

            for (const spk of speakerIds) {
              const segs = diarizationSegments.filter((s) => s.speaker === spk);
              const sorted = segs.sort((a, b) => b.end - b.start - (a.end - a.start)).slice(0, 3);
              const embeddings = [];
              for (const seg of sorted) {
                if (seg.end - seg.start < 1.5) continue;
                const emb = await speakerEmb.extractEmbedding(tmpWav, seg.start, seg.end);
                if (emb) embeddings.push(emb);
              }
              if (embeddings.length > 0) {
                const centroid = speakerEmb.computeCentroid(embeddings);
                const mappedId = speakerRenumber.get(spk) || spk;
                speakerEmbeddingsMap[mappedId] = Array.from(centroid);
              }
            }
          }
        } catch (err) {
          debugLogger.debug("Speaker embedding extraction skipped", { error: err.message });
        }

        const reconciledSpeakers = this._reconcileLiveSpeakerState(
          liveSpeakerState,
          speakerEmbeddingsMap,
          enrichedSegments
        );

        if (speakerEmbeddingsMap) {
          try {
            const profiles = this.databaseManager.getSpeakerProfiles(true);

            if (profiles.length > 0) {
              for (const [mappedId, embArr] of Object.entries(speakerEmbeddingsMap)) {
                const alreadyMapped = enrichedSegments.some(
                  (segment) => segment.speaker === mappedId && segment.speakerName
                );
                if (reconciledSpeakers.has(mappedId) || alreadyMapped) {
                  continue;
                }

                const emb = new Float32Array(embArr);
                let bestProfile = null;
                let bestSim = 0;

                for (const profile of profiles) {
                  const profileEmb = new Float32Array(
                    profile.embedding.buffer,
                    profile.embedding.byteOffset,
                    profile.embedding.byteLength / 4
                  );
                  const sim = speakerEmb.cosineSimilarity(emb, profileEmb);
                  if (sim > bestSim) {
                    bestSim = sim;
                    bestProfile = profile;
                  }
                }

                if (bestProfile && bestSim > 0.6) {
                  for (const seg of enrichedSegments) {
                    if (seg.speaker === mappedId) {
                      applyConfirmedSpeaker(seg, {
                        speakerName: bestProfile.display_name,
                        speakerIsPlaceholder: false,
                        suggestedName: undefined,
                        suggestedProfileId: undefined,
                      });
                    }
                  }
                } else if (bestProfile && bestSim > 0.5) {
                  for (const seg of enrichedSegments) {
                    if (seg.speaker === mappedId) {
                      if (isSpeakerLocked(seg)) {
                        continue;
                      }
                      applySuggestedSpeaker(seg, {
                        suggestedName: bestProfile.display_name,
                        suggestedProfileId: bestProfile.id,
                      });
                    }
                  }
                }
              }
            }
          } catch (err) {
            debugLogger.debug("Auto speaker recognition skipped", { error: err.message });
          }
        }

        send({ segments: enrichedSegments, speakerEmbeddings: speakerEmbeddingsMap });
      } catch (err) {
        debugLogger.warn("Background diarization failed", { error: err.message });
        send({ segments: [] });
      } finally {
        try {
          fs.unlinkSync(rawPcmPath);
        } catch (_) {}
        if (tmpWav) {
          try {
            fs.unlinkSync(tmpWav);
          } catch (_) {}
        }
      }
    })();
  }

  deleteTranscriptionInternal(id) {
    this.audioStorageManager.deleteAudio(id);
    const result = this.databaseManager.deleteTranscription(id);
    if (result?.success) {
      setImmediate(() => {
        broadcastToWindows("transcription-deleted", { id });
      });
    }
    return result;
  }

  deleteNoteInternal(id) {
    const result = this.databaseManager.deleteNote(id);
    if (result?.success) {
      setImmediate(() => broadcastToWindows("note-deleted", { id }));
      this._asyncVectorDelete(id);
      this._asyncMirrorDelete(id);
    }
    return result;
  }
}

module.exports = IPCHandlers;
