// Transcription error codes → notes.upload.* i18n keys. Codes absent here fall
// back to the raw main-process message.
const TRANSCRIPTION_ERROR_KEYS: Record<string, string> = {
  CUSTOM_ENDPOINT_INVALID: "customEndpointInvalid",
};

// Call sites pass either a returned result or a caught error, so the key is
// resolved from whichever shape they hold.
export function transcriptionErrorKey(failure: unknown): string | undefined {
  const code = (failure as { code?: string } | null | undefined)?.code;
  return code ? TRANSCRIPTION_ERROR_KEYS[code] : undefined;
}
