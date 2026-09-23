import { ensureV1Suffix } from "../../config/constants";
import { isSecureHttpEndpoint } from "../../utils/urlUtils";
import i18n from "../../i18n";

// Never falls back: an invalid endpoint must not reroute the prompt and the
// server key to a host the user never chose.
export function resolveSelfHostedOpenAIBase(configuredBaseUrl: string): string {
  const normalized = ensureV1Suffix(configuredBaseUrl.trim());
  if (!normalized || !isSecureHttpEndpoint(normalized)) {
    throw new Error(i18n.t("reasoning.custom.httpsRequired"));
  }

  return normalized;
}
