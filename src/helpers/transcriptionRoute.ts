// Single source of truth for batch speech-to-text routing across dictation,
// retry, and upload: every request goes to the user's self-hosted,
// OpenAI-compatible server.
//
// Loaded by the renderer and the main process alike (main uses dynamic import):
// erasable TypeScript syntax only, explicit import extensions, no store imports.
import { buildApiUrl, normalizeBaseUrl } from "../config/constants.ts";
import {
  isSecureHttpEndpoint,
  isAzureOpenAIEndpoint,
  buildAzureTranscriptionUrl,
} from "../utils/urlUtils.ts";

const CUSTOM_ENDPOINT_INVALID_CODE = "CUSTOM_ENDPOINT_INVALID";
const CUSTOM_ENDPOINT_INVALID_MESSAGE_KEY =
  "hooks.audioRecording.errorDescriptions.customEndpointInvalid";

export interface TranscriptionRouteSettings {
  remoteTranscriptionUrl?: string;
  remoteTranscriptionModel?: string;
  preferredLanguage?: string;
}

export interface TranscriptionRouteInput {
  settings: TranscriptionRouteSettings;
  request?: {
    /** Wins over preferredLanguage when the caller already resolved one. */
    effectiveLanguage?: string;
  };
}

export type TranscriptionRoute =
  | { transport: "error"; message: string; code: string; messageKey: string }
  | {
      transport: "http-batch";
      endpoint: string;
      model: string | null;
      language?: string;
    };

function error(message: string): TranscriptionRoute {
  return {
    transport: "error",
    message,
    code: CUSTOM_ENDPOINT_INVALID_CODE,
    messageKey: CUSTOM_ENDPOINT_INVALID_MESSAGE_KEY,
  };
}

// Azure routes by deployment in the path and needs an api-version query, so the
// plain {base}/audio/transcriptions shape returns DeploymentNotFound. Takes the
// raw URL because normalization strips the suffix that marks a pinned deployment.
function buildBatchEndpoint(rawUrl: string, base: string, model: string | null): string {
  const fallback = buildApiUrl(base, "/audio/transcriptions");
  if (!isAzureOpenAIEndpoint(base)) return fallback;
  return buildAzureTranscriptionUrl(rawUrl, model || "") || fallback;
}

export function resolveTranscriptionRoute({
  settings,
  request,
}: TranscriptionRouteInput): TranscriptionRoute {
  const s = settings || {};
  const language =
    request?.effectiveLanguage ??
    (!s.preferredLanguage || s.preferredLanguage === "auto"
      ? undefined
      : s.preferredLanguage.split("-")[0]);

  // Fail closed: without a usable server URL no audio leaves the device.
  const rawUrl = (s.remoteTranscriptionUrl || "").trim();
  if (!rawUrl) return error("Self-hosted transcription URL is not configured");
  const base = normalizeBaseUrl(rawUrl);
  if (!base || !isSecureHttpEndpoint(base)) {
    return error("Self-hosted transcription URL is invalid or unsupported");
  }

  const model = (s.remoteTranscriptionModel || "").trim() || null;
  return {
    transport: "http-batch",
    endpoint: buildBatchEndpoint(rawUrl, base, model),
    model,
    language,
  };
}
