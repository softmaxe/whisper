import type { ReasoningConfig } from "../BaseReasoningService";
import { detectEndpointDialect, suppressThinking } from "./thinkingSuppressionDialects";

export function applyThinkingSuppression(
  requestBody: Record<string, unknown>,
  model: string,
  provider: string,
  config: ReasoningConfig,
  baseUrl?: string
): void {
  if (config.disableThinking !== true) return;
  // A known endpoint host wins over the generic provider dialect.
  const providerKey = detectEndpointDialect(baseUrl)?.key ?? provider.toLowerCase();
  suppressThinking(requestBody, providerKey, model);
}
