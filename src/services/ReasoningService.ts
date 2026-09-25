import { BaseReasoningService, ReasoningConfig } from "./BaseReasoningService";
import { withRetry, createApiRetryStrategy, httpError } from "../utils/retry";
import { TOKEN_LIMITS, buildApiUrl } from "../config/constants";
import logger from "../utils/logger";
import { getSettings } from "../stores/settingsStore";
import { wrapCleanupTranscript } from "../config/prompts";
import { stripThinkingTags } from "../helpers/stripThinking.js";
import {
  getLlmRequestTimeoutSeconds,
  llmRequestTimeoutError,
} from "../helpers/llmRequestTimeout.js";
import { resolveSelfHostedOpenAIBase } from "./ai/openaiBase";
import {
  applyChatCompletionsParams,
  emptyResponseError,
  fetchWithParamFallback,
  isTruncatedFinishReason,
  truncatedOutputError,
} from "./ai/chatRequestBody";
import { openCodeSessionHeaders } from "./ai/openCodeSession";
import { extractApiErrorMessage } from "./ai/apiErrorMessage";

const PROVIDER_NAME = "LAN";

function logParamFallback(logEvent: string) {
  return (details: { status: number; stripped: string[] }) =>
    logger.logReasoning(logEvent, details);
}

// Text cleanup through the user's self-hosted OpenAI-compatible server.
class ReasoningService extends BaseReasoningService {
  private activeRequestControllers = new Set<AbortController>();
  private requestCancellationGeneration = 0;

  constructor() {
    super();
    if (typeof window !== "undefined") {
      window.addEventListener("beforeunload", () => this.destroy());
    }
  }

  private async callChatCompletionsApi(
    endpoint: string,
    apiKey: string,
    model: string,
    text: string,
    agentName: string | null,
    config: ReasoningConfig
  ): Promise<string> {
    // No systemPrompt override means the default cleanup path: a deterministic
    // transform, so zero temperature and a delimited transcript.
    const isCleanup = !config.systemPrompt;
    const systemPrompt = config.systemPrompt || this.getSystemPrompt(agentName);
    const userPrompt = isCleanup ? wrapCleanupTranscript(text) : text;

    const messages = [
      { role: "system", content: systemPrompt },
      { role: "user", content: userPrompt },
    ];

    const requestBody: Record<string, unknown> = { model, messages };
    applyChatCompletionsParams(requestBody, {
      model,
      provider: "lan",
      endpoint,
      config,
      maxTokens:
        config.maxTokens ||
        Math.max(
          4096,
          this.calculateMaxTokens(
            text.length,
            TOKEN_LIMITS.MIN_TOKENS,
            TOKEN_LIMITS.MAX_TOKENS,
            TOKEN_LIMITS.TOKEN_MULTIPLIER
          )
        ),
    });

    logger.logReasoning("LAN_REQUEST", {
      endpoint,
      model,
      hasApiKey: !!apiKey,
      requestBody: JSON.stringify(requestBody).substring(0, 200),
    });

    // Minted before the retry loop so every attempt of this call is one conversation.
    const openCodeHeaders = openCodeSessionHeaders(endpoint);

    const requestGeneration = this.requestCancellationGeneration;
    const response = await withRetry(async () => {
      if (requestGeneration !== this.requestCancellationGeneration) {
        throw httpError("Request cancelled", 499);
      }
      const controller = new AbortController();
      this.activeRequestControllers.add(controller);
      const timeoutSeconds = getLlmRequestTimeoutSeconds();
      const timeoutId = setTimeout(() => controller.abort(), timeoutSeconds * 1000);
      try {
        const headers: Record<string, string> = {
          "Content-Type": "application/json",
          ...openCodeHeaders,
        };
        if (apiKey) {
          headers["Authorization"] = `Bearer ${apiKey}`;
        }

        const res = await fetchWithParamFallback(
          () =>
            fetch(endpoint, {
              method: "POST",
              headers,
              body: JSON.stringify(requestBody),
              signal: controller.signal,
            }),
          requestBody,
          logParamFallback("LAN_PARAM_FALLBACK")
        );

        if (!res.ok) {
          const errorText = await res.text();
          let errorData: unknown;
          try {
            errorData = JSON.parse(errorText);
          } catch {
            errorData = { error: errorText || res.statusText };
          }

          const errorMessage = extractApiErrorMessage(
            errorData,
            `${PROVIDER_NAME} API error: ${res.status}`
          );

          logger.logReasoning("LAN_API_ERROR_DETAIL", {
            status: res.status,
            statusText: res.statusText,
            error: errorData,
            errorMessage,
            fullResponse: errorText.substring(0, 500),
          });
          throw httpError(errorMessage, res.status);
        }

        return await res.json();
      } catch (error) {
        if ((error as Error).name === "AbortError") {
          if (requestGeneration !== this.requestCancellationGeneration) {
            throw httpError("Request cancelled", 499);
          }
          throw llmRequestTimeoutError(timeoutSeconds);
        }
        throw error;
      } finally {
        clearTimeout(timeoutId);
        this.activeRequestControllers.delete(controller);
      }
    }, createApiRetryStrategy());

    if (!response.choices || !response.choices[0]) {
      logger.logReasoning("LAN_RESPONSE_ERROR", {
        model,
        response: JSON.stringify(response).substring(0, 500),
      });
      throw new Error(`Invalid response structure from ${PROVIDER_NAME} API`);
    }

    const choice = response.choices[0];
    if (config.requireCompleteOutput && isTruncatedFinishReason(choice?.finish_reason)) {
      throw truncatedOutputError();
    }
    // Reasoning models leak <think> blocks into the output; strip them unless
    // the user explicitly enabled thinking.
    const rawContent = choice.message?.content?.trim() || "";
    const responseText =
      config.disableThinking !== false ? stripThinkingTags(rawContent) : rawContent;

    if (!responseText) {
      logger.logReasoning("LAN_EMPTY_RESPONSE", {
        model,
        finishReason: choice.finish_reason,
        response: JSON.stringify(choice).substring(0, 500),
      });
      throw (
        emptyResponseError(PROVIDER_NAME, config, isTruncatedFinishReason(choice.finish_reason)) ??
        new Error(`${PROVIDER_NAME} returned empty response`)
      );
    }

    logger.logReasoning("LAN_RESPONSE", {
      model,
      responseLength: responseText.length,
      tokensUsed: response.usage?.total_tokens || 0,
    });

    return responseText;
  }

  async processText(
    text: string,
    model: string = "",
    agentName: string | null = null,
    config: ReasoningConfig = {}
  ): Promise<string> {
    const settings = getSettings();
    // An explicit URL (Prompt Studio's draft endpoint) brings its own key; the
    // saved cleanup key only belongs to the saved cleanup endpoint.
    const lanUrl = (config.lanUrl || settings.cleanupRemoteUrl || "").trim();
    const apiKey =
      config.customApiKey?.trim() ||
      (config.lanUrl ? "" : settings.cleanupCustomApiKey?.trim()) ||
      "";
    const resolvedModel = model?.trim() || "default";

    logger.logReasoning("LAN_START", { url: lanUrl, agentName, model: resolvedModel });
    const startTime = Date.now();
    try {
      const endpoint = buildApiUrl(resolveSelfHostedOpenAIBase(lanUrl), "/chat/completions");
      const result = await this.callChatCompletionsApi(
        endpoint,
        apiKey,
        resolvedModel,
        text,
        agentName,
        config
      );
      logger.logReasoning("PROVIDER_SUCCESS", {
        model: resolvedModel,
        processingTimeMs: Date.now() - startTime,
        resultLength: result.length,
      });
      return result;
    } catch (error) {
      logger.logReasoning("LAN_ERROR", {
        url: lanUrl,
        error: (error as Error).message,
        errorType: (error as Error).name,
      });
      throw error;
    }
  }

  async isAvailable(): Promise<boolean> {
    return !!getSettings().cleanupRemoteUrl?.trim();
  }

  cancelAllRequests(): void {
    this.requestCancellationGeneration += 1;
    for (const controller of this.activeRequestControllers) controller.abort();
    this.activeRequestControllers.clear();
  }

  destroy(): void {
    this.cancelAllRequests();
  }
}

export default new ReasoningService();
