// The dictionary request budget and the decoder window used by the UI warning.

// Whisper-family decoders (whisper-large-v3, whisper.cpp) read
// at most 223 prompt tokens and keep the TAIL of anything longer, silently. 900
// chars is the historical client-side cut for them: it is well above what the
// decoder reads, so it only bounds the request, never which words survive.
export const WHISPER_PROMPT_CHARS = 900;

// What the decoder's 223-token window is actually worth in characters. A
// comma-separated list of names and technical terms tokenizes at ~2.3-2.9
// chars/token (measured against Whisper's own BPE) — far denser than the ~4
// chars/token rule of thumb for prose, because every ", " costs a token and
// rare proper nouns fragment. Used to warn in the UI, not to trim: the true
// budget is tokens, and no character count is right for every language.
export const WHISPER_DECODER_PROMPT_CHARS = 550;

// Cuts at the last comma inside the budget so no entry is sent half-spelled.
// Returns the (possibly shorter) prompt plus what changed, for logging.
// Callers must classify dictionary echoes against the returned prompt, not the
// original: the echo filter needs 70% of the prompt's words back, which a full
// echo of a trimmed prompt only clears when measured against the same string.
export function trimDictionaryPrompt(prompt, maxChars) {
  if (!prompt || prompt.length <= maxChars) {
    return { prompt, originalLength: prompt ? prompt.length : 0, truncated: false };
  }
  const head = prompt.slice(0, maxChars);
  const lastComma = head.lastIndexOf(",");
  return {
    prompt: lastComma > 0 ? head.slice(0, lastComma) : head,
    originalLength: prompt.length,
    truncated: true,
  };
}
