const LLM_REQUEST_TIMEOUT_SECONDS = 30;

export const LLM_REQUEST_TIMEOUT_CODE = "LLM_REQUEST_TIMEOUT";

/** Client-side deadline for one LLM request. */
export function getLlmRequestTimeoutSeconds() {
  return LLM_REQUEST_TIMEOUT_SECONDS;
}

/** The coded error a request raises when its client-side deadline expires; createApiRetryStrategy never retries it. */
export function llmRequestTimeoutError(timeoutSeconds) {
  return Object.assign(new Error(`Request timed out after ${timeoutSeconds}s`), {
    code: LLM_REQUEST_TIMEOUT_CODE,
  });
}
