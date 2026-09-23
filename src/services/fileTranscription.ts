import { resolveTranscriptionRoute } from "../helpers/transcriptionRoute";

export interface FileTranscriptionResult {
  success: boolean;
  text?: string;
  error?: string;
  code?: string;
  messageKey?: string;
  warning?: string;
  failedChunks?: number;
  totalChunks?: number;
}

export interface FileTranscriptionConfig {
  remoteTranscriptionUrl: string;
  remoteTranscriptionModel: string;
  language: string;
}

// Single dispatch shared by the single-file flow and the batch queue. The
// renderer pre-flight surfaces a misconfigured server without an IPC round
// trip; the main-process handler re-resolves the same route as defense in depth.
export async function transcribeFile(
  filePath: string,
  cfg: FileTranscriptionConfig
): Promise<FileTranscriptionResult> {
  const route = resolveTranscriptionRoute({
    settings: {
      remoteTranscriptionUrl: cfg.remoteTranscriptionUrl,
      remoteTranscriptionModel: cfg.remoteTranscriptionModel,
    },
    request: { effectiveLanguage: cfg.language || undefined },
  });
  if (route.transport === "error") {
    return {
      success: false,
      error: route.message,
      code: route.code,
      messageKey: route.messageKey,
    };
  }

  return window.electronAPI.transcribeAudioFile!({
    filePath,
    language: cfg.language,
    remoteTranscriptionUrl: cfg.remoteTranscriptionUrl,
    remoteTranscriptionModel: cfg.remoteTranscriptionModel,
  });
}
