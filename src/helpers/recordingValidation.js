// A recorded WebM/Opus blob with no audio frames — produced when the dictation
// hotkey toggles recording on and off within milliseconds (an accidental
// double-tap, or the KDE double-trigger fixed in main.js) — is essentially just
// the container header, well under 256 bytes. Real speech, even a single short
// word, produces far more. We gate on size, not wall-clock duration: a genuinely
// short utterance can last under any reasonable time threshold yet still carry
// real audio, so a duration gate would silently drop it. See issue #864.
const MIN_AUDIO_BYTES = 256;

export function isEmptyRecording(blobSize) {
  const size = typeof blobSize === "number" && Number.isFinite(blobSize) ? blobSize : 0;
  return size < MIN_AUDIO_BYTES;
}

// Decide whether a finished MediaRecorder session carries real audio before we
// hand it to the transcription backend. A fast tap can flush only the container
// header, or deliver no chunks at all, which crashes FFmpeg. See issue #871.
export function evaluateFinishedRecording({ blobSize, receivedAudioData } = {}) {
  if (!receivedAudioData) {
    return { usable: false, reason: "no-audio-data" };
  }
  if (isEmptyRecording(blobSize)) {
    return { usable: false, reason: "empty-container" };
  }
  return { usable: true, reason: null };
}

// A salvaged recording (segment merge failed, only the largest segment was
// kept) yields a transcript missing the dropped segments' audio. Mark the
// result so the renderer shows its partial-transcription warning; an existing
// warning (e.g. a truncated decode) already does.
export function withSalvageWarning(result, salvaged) {
  if (!salvaged || !result?.success || result.warning) return result;
  return { ...result, warning: "salvaged-recording" };
}
