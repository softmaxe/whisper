import { getModelFamilyConstraints } from "./modelFamilyConstraints";

/** Disable thinking through the self-hosted OpenAI-compatible request shape. */
export function suppressThinking(requestBody: Record<string, unknown>, model: string): void {
  const family = getModelFamilyConstraints(model);

  // The nested reasoning object disables Ollama thinking without the flat
  // reasoning_effort value that vLLM can reject. gpt-oss has no off switch,
  // so keep its minimum effort consistent with the cleanup request.
  requestBody.reasoning = { effort: family?.reasoningEffort?.suppressValue ?? "none" };
  requestBody.chat_template_kwargs = { enable_thinking: false };
}
