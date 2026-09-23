import type { ReasoningConfig } from "../BaseReasoningService";
import { getModelFamilyConstraints } from "./modelFamilyConstraints";
import { applyThinkingSuppression } from "./thinkingSuppression";

/**
 * Single place that turns (model, provider, endpoint, config) into the
 * parameter set of an OpenAI-compatible chat-completions body: token limit,
 * temperature, family reasoning effort, and thinking suppression. Self-hosted
 * servers (llama.cpp, Ollama, vLLM) speak the legacy shape: always
 * `max_tokens`, always `temperature`. Callers own `model`/`messages`/`stream`.
 */
export function applyChatCompletionsParams(
  requestBody: Record<string, unknown>,
  {
    model,
    provider,
    endpoint,
    config,
    maxTokens,
  }: {
    model: string;
    provider: string;
    endpoint?: string | null;
    config: ReasoningConfig;
    maxTokens: number;
  }
): void {
  // No systemPrompt override means the default cleanup path: a deterministic
  // transform, so zero temperature.
  const defaultTemperature = config.systemPrompt ? 0.3 : 0;
  requestBody.max_tokens = maxTokens;
  requestBody.temperature = config.temperature ?? defaultTemperature;

  // Deterministic transforms (cleanup) pin the family's preferred effort — see
  // modelFamilyConstraints for the gpt-oss rationale. applyThinkingSuppression
  // still wins when thinking is disabled by the user.
  const familyEffort = getModelFamilyConstraints(model)?.reasoningEffort;
  if (familyEffort?.cleanupValue && (!config.systemPrompt || config.requireCompleteOutput)) {
    requestBody.reasoning_effort = familyEffort.cleanupValue;
  }

  applyThinkingSuppression(requestBody, model, provider, config, endpoint ?? undefined);
}

/** Finish reasons that mean the output hit the token cap, across providers. */
export function isTruncatedFinishReason(reason: unknown): boolean {
  return reason === "length" || reason === "max_tokens";
}

/**
 * Dictation cleanup renders these errors as the toast title, so they carry the key
 * the renderer translates. Anthropic and enterprise attach the same key on the
 * main-process side; local llama reports a code that localInferenceError.ts maps.
 */
export const TRUNCATED_OUTPUT_MESSAGE_KEY =
  "hooks.audioRecording.errorDescriptions.cleanupTruncated";
const EMPTY_OUTPUT_MESSAGE_KEY = "hooks.audioRecording.errorDescriptions.cleanupEmptyReply";

/** Output cut off at the token cap; providers may pass their own wording for the logs. */
export function truncatedOutputError(
  message = "Model output was truncated"
): Error & { messageKey: string } {
  return Object.assign(new Error(message), { messageKey: TRUNCATED_OUTPUT_MESSAGE_KEY });
}

/** A reply with no text at all; providers pass their own wording for the logs. */
export function emptyOutputError(
  message = "Model returned an empty response"
): Error & { messageKey: string } {
  return Object.assign(new Error(message), { messageKey: EMPTY_OUTPUT_MESSAGE_KEY });
}

/**
 * Error for a completion that produced no text, or null when the caller may
 * echo its input instead. Only the default cleanup transform (no systemPrompt)
 * may echo: a prompted task would take its raw material as the finished result.
 */
export function emptyResponseError(
  providerName: string,
  config: ReasoningConfig,
  responseIncomplete: boolean
): Error | null {
  if (responseIncomplete) {
    return truncatedOutputError("Model ran out of output tokens before producing a response");
  }
  if (config.requireCompleteOutput) return emptyOutputError();
  if (config.systemPrompt) return new Error(`${providerName} returned empty response`);
  return null;
}

/**
 * Shaped params a backend may reject by name with a 400/422. Only params this
 * module's shaping layer added are strippable — never messages or model — so a
 * retry degrades the request (model reasons when asked not to, default
 * sampling) instead of failing it outright. The strip is logged loudly: a
 * fallback that fires on every request is a dialect bug to fix, not a rescue.
 */
const STRIPPABLE_SHAPED_PARAMS = [
  "reasoning_effort",
  "chat_template_kwargs",
  "thinking",
  "think",
  "temperature",
] as const;

/**
 * Fetch with a bounded param-stripping ladder for 400/422 rejections.
 * Rung 1 (blind): old Ollama/strict proxies reject the `reasoning` object
 * without naming it — drop it and retry once. Rung 2 (named): strip exactly
 * the shaped params the error body names and retry once. At most two retries;
 * the caller must build the body inside `doFetch` so retries re-serialize.
 */
export async function fetchWithParamFallback(
  doFetch: () => Promise<Response>,
  requestBody: Record<string, unknown>,
  logRejection: (details: { status: number; stripped: string[] }) => void
): Promise<Response> {
  let res = await doFetch();
  if (res.ok || (res.status !== 400 && res.status !== 422)) return res;

  if (requestBody.reasoning) {
    logRejection({ status: res.status, stripped: ["reasoning"] });
    delete requestBody.reasoning;
    void res.body?.cancel();
    res = await doFetch();
    if (res.ok || (res.status !== 400 && res.status !== 422)) return res;
  }

  const errorText = await res
    .clone()
    .text()
    .catch(() => "");
  const named = STRIPPABLE_SHAPED_PARAMS.filter((p) => p in requestBody && errorText.includes(p));
  if (named.length > 0) {
    logRejection({ status: res.status, stripped: named });
    for (const param of named) delete requestBody[param];
    void res.body?.cancel();
    res = await doFetch();
  }
  return res;
}
