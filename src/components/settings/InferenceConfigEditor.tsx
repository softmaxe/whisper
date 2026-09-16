import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useShallow } from "zustand/react/shallow";
import {
  INFERENCE_SCOPES,
  type InferenceScope,
  type InferenceScopeDefinition,
} from "../../config/inferenceScopes";
import { usePolicySnapshot } from "../../hooks/usePolicy";
import {
  selectPolicyEffectiveSettings,
  selectResolvedLLMConfig,
  setResolvedLLMConfig,
  useSettingsStore,
  type ResolvedLLMConfig,
} from "../../stores/settingsStore";
import OpenAICompatiblePanel from "../OpenAICompatiblePanel";
import { Toggle } from "../ui/toggle";

interface InferenceConfigEditorProps {
  scope: InferenceScope;
}

export default function InferenceConfigEditor({ scope }: InferenceConfigEditorProps) {
  const { t } = useTranslation();
  const policyState = usePolicySnapshot();
  const config = useSettingsStore(
    useShallow((settings): ResolvedLLMConfig => {
      const effective = selectPolicyEffectiveSettings(settings, policyState);
      const resolved = selectResolvedLLMConfig(effective, scope);
      const definition: InferenceScopeDefinition = INFERENCE_SCOPES[scope];
      // Inherited runtime defaults are not an explicit selection in an optional picker.
      return definition.optional
        ? { ...resolved, model: effective[definition.storeKeys.model] as string }
        : resolved;
    })
  );
  const setField = useCallback(
    <K extends keyof Omit<typeof config, "scope">>(field: K) =>
      (value: NonNullable<(typeof config)[K]>) => {
        setResolvedLLMConfig(scope, { [field]: value });
      },
    [scope]
  );

  return (
    <div className="space-y-3">
      {
        <OpenAICompatiblePanel
          baseUrl={config.remoteUrl ?? ""}
          setBaseUrl={setField("remoteUrl")}
          apiKey={config.customApiKey ?? ""}
          setApiKey={setField("customApiKey")}
          model={config.model}
          setModel={setField("model")}
          baseUrlPlaceholder="http://192.168.1.126:11434/v1"
          helpExamples={
            <p className="text-xs text-muted-foreground">
              {t("reasoning.selfHosted.endpointHelp")}
            </p>
          }
        />
      }

      {
        <div className="flex items-start justify-between gap-3 pt-1">
          <div className="flex-1 min-w-0">
            <h4 className="text-sm font-medium text-foreground">
              {t("reasoning.disableThinking.label")}
            </h4>
            <p className="text-xs text-muted-foreground">{t("reasoning.disableThinking.help")}</p>
          </div>
          <Toggle checked={config.disableThinking} onChange={setField("disableThinking")} />
        </div>
      }
    </div>
  );
}
