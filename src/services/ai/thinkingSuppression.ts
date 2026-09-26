import type { ReasoningConfig } from "../BaseReasoningService";
import { suppressThinking } from "./thinkingSuppressionDialects";

export function applyThinkingSuppression(
  requestBody: Record<string, unknown>,
  model: string,
  config: ReasoningConfig
): void {
  if (config.disableThinking !== true) return;
  suppressThinking(requestBody, model);
}
